use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, RwLock};
use cyfs_base::{BuckyError, BuckyResult, DeviceId, Endpoint};
use crate::protocol::{DynamicPackage, PackageBox, PackageCmdCode};
use crate::sockets::{DataSender, UdpDataSender};
use crate::sockets::tcp::{TcpListenerEventListener, TCPSocket};
use crate::sockets::udp::{UDPListenerEventListener, UdpPackageBox, UDPSocket};
use crate::types::MixAesKey;

#[callback_trait::unsafe_callback_trait]
pub trait PackageBoxProcessor: 'static + Send + Sync {
    async fn on_package(&self,
                        resp_sender: &mut dyn DataSender,
                        remote_device_id: &DeviceId,
                        key: &MixAesKey,
                        pkg: &DynamicPackage) -> BuckyResult<()>;
}

pub struct RecieveProcessor {
    package_box_processors: HashMap<u16, Arc<Pin<Box<dyn PackageBoxProcessor>>>>,
}
pub type RecieveProcessorRef = Arc<RecieveProcessor>;

impl RecieveProcessor {
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

#[async_trait::async_trait]
impl UDPListenerEventListener for RecieveProcessor {
    async fn on_udp_package_box(&self, socket: Arc<UDPSocket>, package_box: UdpPackageBox) {
        let mut resp_sender = Box::new(UdpDataSender::new(socket,
                                                          package_box.remote().clone(),
                                                          package_box.as_ref().remote().clone(),
                                                          package_box.as_ref().key().clone()));
        for pkg in package_box.as_ref().packages() {
            let cmd_code = pkg.cmd_code();
            if let Some(handle) = self.get_package_box_processor(cmd_code) {
                let remote_device_id = package_box.as_ref().remote().clone();
                let key = package_box.as_ref().key().clone();
                let _ = handle.on_package(resp_sender.as_mut(), &remote_device_id, &key, pkg).await;
            }
        }
    }

    async fn on_udp_raw_data(&self, data: &[u8], context: (Arc<UDPSocket>, DeviceId, MixAesKey, Endpoint)) -> Result<(), BuckyError> {
        todo!()
    }
}

#[async_trait::async_trait]
impl TcpListenerEventListener for RecieveProcessor {
    async fn on_new_connection(&self,
                               socket: TCPSocket,
                               first_box: PackageBox,) -> BuckyResult<()> {
        todo!()
    }
}
