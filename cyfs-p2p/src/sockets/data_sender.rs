use std::sync::Arc;
use std::time::Duration;
use bucky_objects::{DeviceDesc, DeviceId, Endpoint};
use crate::error::BdtResult;
use crate::MixAesKey;
use crate::protocol::{DynamicPackage, MTU_LARGE, PackageBox, PackageBoxEncodeContext};
use crate::sockets::NetManager;
use crate::sockets::tcp::TCPSocket;
use crate::sockets::udp::UDPSocket;

#[derive(Copy, Clone, Eq, PartialEq)]
pub enum SocketType {
    TCP,
    UDP
}

#[async_trait::async_trait]
pub trait DataSender: Send + Sync + 'static {
    async fn send_resp(&self, data: &[u8]) -> BdtResult<()>;
    async fn send_dynamic_pkg(&self, dynamic_package: DynamicPackage) -> BdtResult<()> {
        let pkg = PackageBox::from_package(self.local_device_id().clone(), self.remote_device_id().clone(), self.key().clone(), dynamic_package);
        self.send_pkg_box(&pkg).await
    }

    async fn send_dynamic_pkgs(&self, dynamic_packages: Vec<DynamicPackage>) -> BdtResult<()> {
        let pkg = PackageBox::from_packages(self.local_device_id().clone(), self.remote_device_id().clone(), self.key().clone(), dynamic_packages);
        self.send_pkg_box(&pkg).await
    }

    async fn send_pkg_box(&self, pkg: &PackageBox) -> BdtResult<()>;
    fn remote(&self) -> &Endpoint;
    fn local(&self) -> &Endpoint;
    fn remote_device_id(&self) -> &DeviceId;
    fn local_device_id(&self) -> &DeviceId;
    fn key(&self) -> &MixAesKey;
    fn socket_type(&self) -> SocketType;
}

pub trait ExtraParams: 'static + Send + Sync {}

#[async_trait::async_trait]
pub trait DataSenderFactory<P: ExtraParams, T: DataSender> {
    async fn create_sender(&self, local_device_id: DeviceId, remote_device: DeviceDesc, remote_ep: Endpoint, p: P) -> BdtResult<T>;
}

