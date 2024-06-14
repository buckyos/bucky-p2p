use std::time::Duration;
use bucky_objects::{DeviceDesc, DeviceId, Endpoint};
use bucky_raw_codec::RawEncodeWithContext;
use crate::error::{BdtErrorCode, BdtResult, into_bdt_err};
use crate::MixAesKey;
use crate::protocol::{FirstBoxTcpEncodeContext, MTU_LARGE, OtherBoxTcpEncodeContext, PackageBox, PackageBoxEncodeContext, PackageBoxTcpEncodeContext};
use crate::sockets::{DataSender, DataSenderFactory, ExtraParams, NetManager, SocketType};
use crate::sockets::tcp::TCPSocket;

#[async_trait::async_trait]
impl DataSender for TCPSocket {
    async fn send_resp(&self, data: &[u8]) -> BdtResult<()> {
        self.send(data).await?;
        Ok(())
    }

    async fn send_pkg_box(&self, pkg: &PackageBox) -> BdtResult<()> {
        let mut buf = [0u8; MTU_LARGE];
        let data_buf = if pkg.has_exchange() {
            let mut context = PackageBoxTcpEncodeContext(FirstBoxTcpEncodeContext::default());
            pkg.raw_tail_encode_with_context(buf.as_mut(), &mut context, &None).map_err(into_bdt_err!(BdtErrorCode::RawCodecError))?
        } else {
            let mut context = PackageBoxTcpEncodeContext(OtherBoxTcpEncodeContext::default());
            pkg.raw_tail_encode_with_context(buf.as_mut(), &mut context, &None).map_err(into_bdt_err!(BdtErrorCode::RawCodecError))?
        };
        self.send_resp(data_buf).await
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

    fn local_device_id(&self) -> &DeviceId {
        TCPSocket::local_device_id(self)
    }

    fn key(&self) -> &MixAesKey {
        TCPSocket::key(self)
    }

    fn socket_type(&self) -> SocketType {
        SocketType::TCP
    }
}

pub struct TcpExtraParams {
    pub timeout: Duration,
}

impl ExtraParams for TcpExtraParams {}

#[async_trait::async_trait]
impl DataSenderFactory<TcpExtraParams, TCPSocket> for NetManager {
    async fn create_sender(&self, local_device_id: DeviceId, remote_device: DeviceDesc, remote_ep: Endpoint, p: TcpExtraParams) -> BdtResult<TCPSocket> {
        let key = self.key_store.create_key(&local_device_id, &remote_device);
        TCPSocket::connect(local_device_id, remote_ep, remote_device.device_id(), remote_device, key.key, p.timeout).await
    }
}
