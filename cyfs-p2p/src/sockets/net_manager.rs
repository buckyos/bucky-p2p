use std::sync::Arc;
use std::time::Duration;
use cyfs_base::{BuckyResult, DeviceId, Endpoint};
use crate::history::keystore::Keystore;
use crate::LocalDeviceRef;
use crate::sockets::{NetListener, NetListenerRef};
use crate::sockets::tcp::TcpListenerEventListener;
use crate::sockets::udp::{UDPListenerEventListener, UDPListenerRef, UDPSocket};

pub struct NetManager {
    net_listener: NetListenerRef,
    pub(super) key_store: Arc<Keystore>,
}
pub type NetManagerRef = Arc<NetManager>;

impl NetManager {
    pub async fn open(
        key_store: Arc<Keystore>,
        endpoints: &[Endpoint],
        port_mapping: Option<Vec<(Endpoint, u16)>>,
        tcp_accept_timout: Duration,
        udp_recv_buffer: usize,) -> BuckyResult<Self> {
        Ok(Self {
            net_listener: NetListener::open(key_store.clone(), endpoints, port_mapping, tcp_accept_timout, udp_recv_buffer, false).await?,
            key_store,
        })
    }

    pub fn listen(&self) {
        self.net_listener.start();
    }
    pub fn set_udp_listener_event_listener(&self, listener: Arc<dyn UDPListenerEventListener>) {
        self.net_listener.set_udp_listener_event_listener(listener);
    }

    pub fn set_tcp_listener_event_listener(&self, listener: Arc<dyn TcpListenerEventListener>) {
        self.net_listener.set_tcp_listener_event_listener(listener);
    }

    pub fn get_udp_socket(&self, ep: &Endpoint) -> Option<Arc<UDPSocket>> {
        self.net_listener.udp_of(ep).map(|udp| udp.socket().as_ref().unwrap().clone())
    }

    pub fn udp_listeners(&self) -> &Vec<UDPListenerRef> {
        self.net_listener.udp()
    }

    pub fn key_store(&self) -> &Arc<Keystore> {
        &self.key_store
    }
}
