use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, Mutex, RwLock};
use std::sync::atomic::Ordering;
use cyfs_base::{BuckyError, BuckyErrorCode, BuckyResult, DeviceId, Endpoint, RawDecode, RawDecodeWithContext, RawFixedBytes};
use futures::future::AbortHandle;
use crate::executor::Executor;
use crate::history::keystore::Keystore;
use crate::protocol::{DynamicPackage, Exchange, merge_context, MTU_LARGE, OtherBoxTcpDecodeContext, PackageBox, PackageCmdCode};
use crate::receive_processor::RespSender;
use crate::sockets::{DataSender, SocketType, UdpDataSender};
use crate::sockets::tcp::{TcpListenerEventListener, TCPSocket};
use crate::sockets::udp::{UDPListenerEventListener, UdpPackageBox, UDPSocket};
use crate::types::MixAesKey;

#[callback_trait::unsafe_callback_trait]
pub trait PackageBoxProcessor: 'static + Send + Sync {
    async fn on_package(&self,
                        resp_sender: &mut RespSender,
                        pkg: DynamicPackage, ) -> BuckyResult<()>;
}

pub struct ReceiveProcessor {
    package_box_processors: HashMap<u16, Arc<Pin<Box<dyn PackageBoxProcessor>>>>,
}
pub type ReceiveProcessorRef = Arc<ReceiveProcessor>;

impl ReceiveProcessor {
    pub fn new() -> Self {
        Self {
            package_box_processors: HashMap::new(),
        }
    }

    pub fn add_package_box_processor(&mut self, cmd_code: PackageCmdCode, handle: impl PackageBoxProcessor) {
        self.package_box_processors.insert(cmd_code as u16, Arc::new(Box::pin(handle)));
    }

    pub fn get_package_box_processor(&self, cmd_code: PackageCmdCode) -> Option<Arc<Pin<Box<dyn PackageBoxProcessor>>>> {
        self.package_box_processors.get(&(cmd_code as u16)).map(|h| h.clone())
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
                               first_box: PackageBox,) -> BuckyResult<()> {
        let processor = self.get_processor(first_box.local());
        if processor.is_none() {
            return Ok(());
        }
        let processor = processor.unwrap();
        let pkg: PackageBox = first_box.into();
        if pkg.has_exchange() {
            let key_store = self.key_store.clone();
            let key = pkg.key().clone();
            let local = pkg.local().clone();
            let remote = pkg.remote().clone();
            key_store.add_key(
                &key,
                &local,
                &remote,
            );
        }
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
        Ok(())
    }
}

#[derive(Eq, PartialEq)]
enum BoxType {
    Package,
    RawData,
}

pub enum RecvBox<'a> {
    Package(PackageBox),
    RawData(&'a [u8]),
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
            match self.receive_package(&mut recv_buf).await {
                Ok(recv_box) => {
                    match recv_box {
                        RecvBox::Package(package_box) => {
                            if package_box.has_exchange() {
                                let key_store = self.key_store.clone();
                                let key = package_box.key().clone();
                                let local = package_box.local().clone();
                                let remote = package_box.remote().clone();
                                key_store.add_key(
                                    &key,
                                    &local,
                                    &remote
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

    async fn receive_box<'a>(&self, recv_buf: &'a mut [u8]) -> BuckyResult<(BoxType, &'a mut [u8])> {
        let header_len = u16::raw_bytes().unwrap();
        let box_header = &mut recv_buf[..header_len];
        self.socket.recv_exact(box_header).await?;
        let mut box_len = u16::raw_decode(box_header).map(|(v, _)| v as usize)?;
        let box_type = if box_len > 32768 {
            box_len -= 32768;
            BoxType::RawData
        } else {
            BoxType::Package
        };
        if box_len + header_len > recv_buf.len() {
            return Err(BuckyError::new(
                BuckyErrorCode::OutOfLimit,
                "buffer not enough",
            ));
        }
        let box_buf = &mut recv_buf[header_len..(header_len + box_len)];
        self.socket.recv_exact(box_buf).await?;
        Ok((box_type, box_buf))
    }

    async fn receive_package<'a>(&self, recv_buf: &'a mut [u8]) -> BuckyResult<RecvBox<'a>> {
        let (box_type, box_buf) = self.receive_box(recv_buf).await?;
        match box_type {
            BoxType::Package => {
                let context = OtherBoxTcpDecodeContext::new_inplace(
                    box_buf.as_mut_ptr(),
                    box_buf.len(),
                    self.socket.local_device_id(),
                    self.socket.remote_device_id(),
                    self.socket.key(),
                );
                let package = PackageBox::raw_decode_with_context(box_buf, context)
                    .map(|(package_box, _)| package_box)?;
                Ok(RecvBox::Package(package))
            }
            BoxType::RawData => Ok(RecvBox::RawData(box_buf)),
        }
    }
}

#[async_trait::async_trait]
impl DataSender for TCPReceiver {
    async fn send_resp(&self, data: &[u8]) -> BuckyResult<()> {
        self.socket.send(data).await?;
        Ok(())
    }

    async fn send_pkg_box(&self, pkg: &PackageBox) -> BuckyResult<()> {
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
