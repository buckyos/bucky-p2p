use std::sync::Arc;
use cyfs_base::{BuckyError, DeviceDesc, DeviceId, Endpoint, RawEncodeWithContext};
use crate::error::{bdt_err, BdtErrorCode, BdtResult, into_bdt_err};
use crate::MixAesKey;
use crate::protocol::{MTU_LARGE, PackageBox, PackageBoxEncodeContext};
use crate::sockets::{DataSender, DataSenderFactory, ExtraParams, NetManager, SocketType};
use crate::sockets::udp::UDPSocket;

#[derive(Clone)]
pub struct UdpDataSender {
    socket: Arc<UDPSocket>,
    remote: Endpoint,
    remote_device_id: DeviceId,
    local_device_id: DeviceId,
    key: MixAesKey,
}

impl UdpDataSender {
    pub fn new(socket: Arc<UDPSocket>,
               remote: Endpoint,
               remote_device_id: DeviceId,
               local_device_id: DeviceId,
               key: MixAesKey,) -> Self {
        Self {
            socket,
            remote,
            remote_device_id,
            local_device_id,
            key,
        }
    }
}

#[async_trait::async_trait]
impl DataSender for UdpDataSender {
    async fn send_resp(&self, data: &[u8]) -> BdtResult<()> {
        self.socket.send_to(data, &self.remote).await.map_err(into_bdt_err!(BdtErrorCode::IoError))?;
        Ok(())
    }

    async fn send_pkg_box(&self, pkg: &PackageBox) -> BdtResult<()> {
        let mut buf = [0u8; MTU_LARGE];
        let data = {
            let mut context = PackageBoxEncodeContext::default();
            pkg.raw_tail_encode_with_context(buf.as_mut(), &mut context, &None).map_err(into_bdt_err!(BdtErrorCode::RawCodecError))?
        };
        self.send_resp(data).await
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

    fn local_device_id(&self) -> &DeviceId {
        &self.local_device_id
    }

    fn key(&self) -> &MixAesKey {
        &self.key
    }

    fn socket_type(&self) -> SocketType {
        SocketType::UDP
    }
}

pub struct UdpExtraParams {
    pub local_ep: Endpoint
}

impl ExtraParams for UdpExtraParams {
}

#[async_trait::async_trait]
impl DataSenderFactory<UdpExtraParams, UdpDataSender> for NetManager {
    async fn create_sender(&self, local_device_id: DeviceId, remote_device: DeviceDesc, remote_ep: Endpoint, p: UdpExtraParams) -> BdtResult<UdpDataSender> {
        let key = self.key_store.create_key(&local_device_id, &remote_device);
        let socket = self.get_udp_socket(&p.local_ep).ok_or_else(|| {
            bdt_err!(BdtErrorCode::Failed, "udp socket not found")
        })?;
        Ok(UdpDataSender::new(socket, remote_ep, remote_device.device_id().clone(), local_device_id, key.key))
    }
}
