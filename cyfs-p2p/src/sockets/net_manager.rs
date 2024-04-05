use std::sync::Arc;
use std::time::Duration;
use cyfs_base::{BuckyResult, DeviceId, Endpoint};
use crate::history::keystore::Keystore;
use crate::LocalDeviceRef;
use crate::sockets::{NetListener, NetListenerRef};
use crate::sockets::udp::UDPSocket;

pub struct NetManager {
    pub net_listener: NetListenerRef,
}
pub type NetManagerRef = Arc<NetManager>;

impl NetManager {
    pub async fn open(
        key_store: Arc<Keystore>,
        local_device_id: LocalDeviceRef,
        endpoints: &[Endpoint],
        port_mapping: Option<Vec<(Endpoint, u16)>>,
        tcp_accept_timout: Duration,
        udp_recv_buffer: usize,) -> BuckyResult<Self> {
        Ok(Self {
            net_listener: NetListener::open(key_store, local_device_id, endpoints, port_mapping, tcp_accept_timout, udp_recv_buffer, false).await?
        })
    }

    pub fn listen(&self) {
        self.net_listener.start();
    }

    pub fn get_udp_socket(&self, ep: &Endpoint) -> Option<Arc<UDPSocket>> {
        self.net_listener.udp_of(ep).map(|udp| udp.socket().as_ref().unwrap().clone())
    }
}
