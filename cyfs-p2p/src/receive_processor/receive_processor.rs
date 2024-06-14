use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, Mutex, RwLock};
use std::sync::atomic::Ordering;
use as_any::Downcast;
use bucky_error::BuckyError;
use bucky_objects::{DeviceId, Endpoint};
use futures::future::AbortHandle;
use crate::error::BdtResult;
use crate::executor::Executor;
use crate::history::keystore::{EncryptedKey, Keystore};
use crate::protocol::{DynamicPackage, Exchange, merge_context, MTU_LARGE, OtherBoxTcpDecodeContext, PackageBox, PackageCmdCode};
use crate::receive_processor::RespSender;
use crate::sockets::{DataSender, SocketType, UdpDataSender};
use crate::sockets::tcp::{RecvBox, TcpListenerEventListener, TCPSocket};
use crate::sockets::udp::{UDPListenerEventListener, UdpPackageBox, UDPSocket};
use crate::types::MixAesKey;

#[callback_trait::unsafe_callback_trait]
pub trait PackageBoxProcessor: 'static + Send + Sync {
    async fn on_package(&self,
                        resp_sender: &mut RespSender,
                        pkg: DynamicPackage, ) -> BdtResult<()>;
}

pub struct ReceiveProcessor {
    package_box_processors: HashMap<u16, Arc<dyn PackageBoxProcessor>>,
    tcp_processor: Option<Arc<dyn TcpListenerEventListener>>,
}
pub type ReceiveProcessorRef = Arc<ReceiveProcessor>;

impl ReceiveProcessor {
    pub fn new() -> Self {
        Self {
            package_box_processors: HashMap::new(),
            tcp_processor: None,
        }
    }

    pub fn add_package_box_processor(&mut self, cmd_code: PackageCmdCode, handle: impl PackageBoxProcessor) {
        self.package_box_processors.insert(cmd_code as u16, Arc::new(handle));
    }

    pub fn get_package_box_processor(&self, cmd_code: PackageCmdCode) -> Option<Arc<dyn PackageBoxProcessor>> {
        self.package_box_processors.get(&(cmd_code as u16)).map(|h| h.clone())
    }

    pub fn add_tcp_processor(&mut self, processor: impl TcpListenerEventListener) {
        self.tcp_processor = Some(Arc::new(processor));
    }

    pub fn get_tcp_processor(&self) -> &Option<Arc<dyn TcpListenerEventListener>> {
        &self.tcp_processor
    }
}

pub struct ReceiveDispatcher {
    processors: RwLock<HashMap<DeviceId, ReceiveProcessorRef>>,
    key_store: Arc<Keystore>,
}
pub type ReceiveDispatcherRef = Arc<ReceiveDispatcher>;

impl ReceiveDispatcher {
    pub fn new(key_store: Arc<Keystore>) -> Arc<Self> {
        Arc::new(Self {
            processors: RwLock::new(HashMap::new()),
            key_store,
        })
    }

    pub fn add_processor(&self, device_id: DeviceId, processor: ReceiveProcessorRef) {
        self.processors.write().unwrap().insert(device_id, processor);
    }

    pub fn get_processor(&self, device_id: &DeviceId) -> Option<ReceiveProcessorRef> {
        self.processors.read().unwrap().get(device_id).map(|p| p.clone())
    }

    pub fn remove_processor(&self, device_id: &DeviceId) {
        self.processors.write().unwrap().remove(device_id);
    }
}


#[async_trait::async_trait]
impl UDPListenerEventListener for ReceiveDispatcher {
    async fn on_udp_package_box(&self, socket: Arc<UDPSocket>, package_box: UdpPackageBox) {
        let resp_sender = Arc::new(UdpDataSender::new(socket,
                                                          package_box.remote().clone(),
                                                          package_box.as_ref().remote().clone(),
                                                          package_box.as_ref().local().clone(),
                                                          package_box.as_ref().key().clone()));
        let mut resp_sender = RespSender::new(resp_sender);
        let processor = self.get_processor(package_box.as_ref().local());
        if processor.is_none() {
            return;
        }
        let processor = processor.unwrap();
        let pkg: PackageBox = package_box.into();
        let pkg_list: Vec<DynamicPackage> = pkg.into();
        for pkg in pkg_list {
            let cmd_code = pkg.cmd_code();
            log::info!("recv package cmd_code: {:?} local {:?} remote {:?}", cmd_code, resp_sender.local_device_id(), resp_sender.remote_device_id());
            if let Some(handle) = processor.get_package_box_processor(cmd_code) {
                if let Err(err) = handle.on_package(&mut resp_sender, pkg).await {
                    log::error!("on_package error: {:?}", err);
                    return;
                }
            }
        }

        if let Err(e) = resp_sender.send_cache().await {
            log::error!("send_cache error: {:?}", e);
        }
    }

    async fn on_udp_raw_data(&self, data: &[u8], context: (Arc<UDPSocket>, DeviceId, MixAesKey, Endpoint)) -> Result<(), BuckyError> {
        todo!()
    }
}

#[async_trait::async_trait]
impl TcpListenerEventListener for ReceiveDispatcher {
    async fn on_new_connection(&self,
                               socket: Arc<TCPSocket>,
                               first_box: PackageBox,) -> BdtResult<()> {
        let processor = self.get_processor(first_box.local());
        if processor.is_none() {
            return Ok(());
        }
        let processor = processor.unwrap();
        let pkg: PackageBox = first_box.into();

        if let Some(tcp_processor) = processor.get_tcp_processor() {
            tcp_processor.on_new_connection(socket, pkg).await?;
        } else {
            let resp_sender = TCPReceiver::new(socket, processor.clone(), self.key_store.clone());
            let mut resp_sender = RespSender::new(resp_sender);
            let pkg_list: Vec<DynamicPackage> = pkg.into();
            for pkg in pkg_list {
                let cmd_code = pkg.cmd_code();
                if let Some(handle) = processor.get_package_box_processor(cmd_code) {
                    if let Err(err) = handle.on_package(&mut resp_sender, pkg).await {
                        log::error!("on_package error: {:?}", err);
                        return Ok(());
                    }
                }
            }

            if let Err(e) = resp_sender.send_cache().await {
                log::error!("send_cache error: {:?}", e);
            }
        }
        Ok(())
    }
}

pub struct TCPReceiver {
    socket: Arc<TCPSocket>,
    processor: ReceiveProcessorRef,
    key_store: Arc<Keystore>,
    recv_handle: Mutex<Option<AbortHandle>>,
}

impl TCPReceiver {
    pub fn new(socket: Arc<TCPSocket>,
               processor: ReceiveProcessorRef,
               key_store: Arc<Keystore>) -> Arc<Self> {
        let this = Arc::new(Self {
            socket,
            processor,
            key_store,
            recv_handle: Mutex::new(None),
        });
        let receiver = this.clone();
        let (abort_future, handle) = futures::future::abortable(async move {
            receiver.recv_proc().await
        });
        *this.recv_handle.lock().unwrap() = Some(handle);
        Executor::spawn(async move {
            abort_future.await;
        });
        this
    }

    pub async fn recv_proc(self: &Arc<Self>) {
        let mut recv_buf = [0u8; MTU_LARGE];
        loop {
            match self.socket.receive_package(&mut recv_buf).await {
                Ok(recv_box) => {
                    match recv_box {
                        RecvBox::Package(package_box) => {
                            if package_box.has_exchange() {
                                let exchg = package_box.packages()[0].downcast_ref::<Exchange>().unwrap();
                                let key_store = self.key_store.clone();
                                let key = package_box.key().clone();
                                let local = package_box.local().clone();
                                let remote = package_box.remote().clone();
                                key_store.add_key(
                                    &key,
                                    &local,
                                    &remote,
                                    EncryptedKey::Unconfirmed(exchg.key_encrypted.clone()),
                                );
                            }
                            let mut resp_sender = RespSender::new(self.clone());
                            let pkg_list: Vec<DynamicPackage> = package_box.into();
                            for pkg in pkg_list {
                                let cmd_code = pkg.cmd_code();
                                if let Some(handle) = self.processor.get_package_box_processor(cmd_code) {
                                    if let Err(err) = handle.on_package(&mut resp_sender, pkg).await {
                                            log::error!("on_package error: {:?}", err);
                                            break;
                                    }
                                }
                            }
                            if let Err(e) = resp_sender.send_cache().await {
                                log::error!("send_cache error: {:?}", e);
                                break;
                            }
                        },
                        RecvBox::RawData(raw_data) => {
                        }
                    }
                },
                Err(err) => {
                    break;
                }
            }
        }
    }

}

#[async_trait::async_trait]
impl DataSender for TCPReceiver {
    async fn send_resp(&self, data: &[u8]) -> BdtResult<()> {
        self.socket.send(data).await?;
        Ok(())
    }

    async fn send_pkg_box(&self, pkg: &PackageBox) -> BdtResult<()> {
        self.socket.send_pkg_box(pkg).await
    }

    fn remote(&self) -> &Endpoint {
        self.socket.remote()
    }

    fn local(&self) -> &Endpoint {
        self.socket.local()
    }

    fn remote_device_id(&self) -> &DeviceId {
        self.socket.remote_device_id()
    }

    fn local_device_id(&self) -> &DeviceId {
        self.socket.local_device_id()
    }

    fn key(&self) -> &MixAesKey {
        self.socket.key()
    }

    fn socket_type(&self) -> SocketType {
        self.socket.socket_type()
    }
}

impl Drop for TCPReceiver {
    fn drop(&mut self) {
        if let Some(handle) = self.recv_handle.lock().unwrap().take() {
            handle.abort();
        }
    }
}
