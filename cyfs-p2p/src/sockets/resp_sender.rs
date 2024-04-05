use std::sync::Arc;
use cyfs_base::{BuckyResult, DeviceId, Endpoint, RawEncodeWithContext};
use crate::MixAesKey;
use crate::protocol::{DynamicPackage, MTU_LARGE, PackageBox, PackageBoxEncodeContext};
use crate::sockets::tcp::TCPSocket;
use crate::sockets::udp::UDPSocket;

#[derive(Copy, Clone, Eq, PartialEq)]
pub enum SocketType {
    TCP,
    UDP
}

#[async_trait::async_trait]
pub trait DataSender: Send + Sync + 'static {
    async fn send_resp(&self, data: &[u8]) -> BuckyResult<()>;
    async fn send_dynamic_pkg(&self, dynamic_package: DynamicPackage) -> BuckyResult<()> {
        let pkg = PackageBox::from_package(self.remote_device_id().clone(), self.key().clone(), dynamic_package);
        self.send_pkg_box(&pkg).await
    }

    async fn send_dynamic_pkgs(&self, dynamic_packages: Vec<DynamicPackage>) -> BuckyResult<()> {
        let pkg = PackageBox::from_packages(self.remote_device_id().clone(), self.key().clone(), dynamic_packages);
        self.send_pkg_box(&pkg).await
    }

    async fn send_pkg_box(&self, pkg: &PackageBox) -> BuckyResult<()> {
        let mut buf = Box::new([0u8; MTU_LARGE]);
        let data = {
            let mut context = PackageBoxEncodeContext::default();
            pkg.raw_tail_encode_with_context(buf.as_mut(), &mut context, &None)?
        };
        self.send_resp(data).await
    }

    fn remote(&self) -> &Endpoint;
    fn local(&self) -> &Endpoint;
    fn remote_device_id(&self) -> &DeviceId;
    fn key(&self) -> &MixAesKey;
    fn socket_type(&self) -> SocketType;
}

#[async_trait::async_trait]
impl DataSender for TCPSocket {
    async fn send_resp(&self, data: &[u8]) -> BuckyResult<()> {
        self.send(data).await?;
        Ok(())
    }

    fn remote(&self) -> &Endpoint {
        TCPSocket::remote(self)
    }

    fn local(&self) -> &Endpoint {
        self.local()
    }

    fn remote_device_id(&self) -> &DeviceId {
        TCPSocket::remote_device_id(self)
    }

    fn key(&self) -> &MixAesKey {
        TCPSocket::key(self)
    }

    fn socket_type(&self) -> SocketType {
        SocketType::TCP
    }
}

#[derive(Clone)]
pub struct UdpDataSender {
    socket: Arc<UDPSocket>,
    remote: Endpoint,
    remote_device_id: DeviceId,
    key: MixAesKey,
}

impl UdpDataSender {
    pub fn new(socket: Arc<UDPSocket>,
               remote: Endpoint,
               remote_device_id: DeviceId,
               key: MixAesKey,) -> Self {
        Self {
            socket,
            remote,
            remote_device_id,
            key,
        }
    }
}

#[async_trait::async_trait]
impl DataSender for UdpDataSender {
    async fn send_resp(&self, data: &[u8]) -> BuckyResult<()> {
        self.socket.send_to(data, &self.remote).await?;
        Ok(())
    }

    fn remote(&self) -> &Endpoint {
        &self.remote
    }

    fn local(&self) -> &Endpoint {
        self.socket.local()
    }

    fn remote_device_id(&self) -> &DeviceId {
        &self.remote_device_id
    }

    fn key(&self) -> &MixAesKey {
        &self.key
    }

    fn socket_type(&self) -> SocketType {
        SocketType::UDP
    }
}

#[derive(Clone)]
pub enum GeneralDataSender {
    TCP(TCPSocket),
    UDP(UdpDataSender),
}

#[async_trait::async_trait]
impl DataSender for GeneralDataSender {
    async fn send_resp(&self, data: &[u8]) -> BuckyResult<()> {
        match self {
            GeneralDataSender::TCP(s) => s.send_resp(data).await,
            GeneralDataSender::UDP(s) => s.send_resp(data).await,
        }
    }

    async fn send_dynamic_pkg(&self, dynamic_package: DynamicPackage) -> BuckyResult<()> {
        match self {
            GeneralDataSender::TCP(s) => s.send_dynamic_pkg(dynamic_package).await,
            GeneralDataSender::UDP(s) => s.send_dynamic_pkg(dynamic_package).await,
        }
    }

    async fn send_dynamic_pkgs(&self, dynamic_packages: Vec<DynamicPackage>) -> BuckyResult<()> {
        match self {
            GeneralDataSender::TCP(s) => s.send_dynamic_pkgs(dynamic_packages).await,
            GeneralDataSender::UDP(s) => s.send_dynamic_pkgs(dynamic_packages).await,
        }
    }

    async fn send_pkg_box(&self, pkg: &PackageBox) -> BuckyResult<()> {
        match self {
            GeneralDataSender::TCP(s) => s.send_pkg_box(pkg).await,
            GeneralDataSender::UDP(s) => s.send_pkg_box(pkg).await,
        }
    }

    fn remote(&self) -> &Endpoint {
        match self {
            GeneralDataSender::TCP(s) => s.remote(),
            GeneralDataSender::UDP(s) => s.remote(),
        }
    }

    fn local(&self) -> &Endpoint {
        match self {
            GeneralDataSender::TCP(s) => s.local(),
            GeneralDataSender::UDP(s) => s.local(),
        }
    }
    fn remote_device_id(&self) -> &DeviceId {
        match self {
            GeneralDataSender::TCP(s) => s.remote_device_id(),
            GeneralDataSender::UDP(s) => s.remote_device_id(),
        }
    }

    fn key(&self) -> &MixAesKey {
        match self {
            GeneralDataSender::TCP(s) => s.key(),
            GeneralDataSender::UDP(s) => s.key(),
        }
    }

    fn socket_type(&self) -> SocketType {
        match self {
            GeneralDataSender::TCP(s) => s.socket_type(),
            GeneralDataSender::UDP(s) => s.socket_type(),
        }
    }
}

impl From<TCPSocket> for GeneralDataSender {
    fn from(s: TCPSocket) -> Self {
        GeneralDataSender::TCP(s)
    }
}

impl From<UdpDataSender> for GeneralDataSender {
    fn from(s: UdpDataSender) -> Self {
        GeneralDataSender::UDP(s)
    }
}
