use std::sync::Arc;
use std::time::Duration;
use bucky_crypto::PrivateKey;
use bucky_objects::{Device, DeviceId, Endpoint};
use bucky_rustls::{ServerCertResolver, ServerCertResolverRef};
use crate::error::BdtResult;
use crate::finder::DeviceCache;
use crate::history::keystore::Keystore;
use crate::LocalDeviceRef;
use crate::sockets::{NetListener, NetListenerRef, QuicListenerEventListener};
use crate::sockets::quic::QuicListenerRef;
use crate::sockets::tcp::TcpListenerEventListener;

pub struct NetManager {
    net_listener: NetListenerRef,
    cert_resolver: ServerCertResolverRef,
    device_cache: Arc<DeviceCache>,
}
pub type NetManagerRef = Arc<NetManager>;

impl NetManager {
    pub async fn open(
        device_cache: Arc<DeviceCache>,
        endpoints: &[Endpoint],
        port_mapping: Option<Vec<(Endpoint, u16)>>,
        tcp_accept_timout: Duration,
        udp_recv_buffer: usize,) -> BdtResult<Self> {
        let cert_resolver = ServerCertResolver::new();
        Ok(Self {
            net_listener: NetListener::open(device_cache.clone(), cert_resolver.clone(), endpoints, port_mapping, tcp_accept_timout).await?,
            cert_resolver,
            device_cache,
        })
    }

    pub fn listen(&self) {
        self.net_listener.start();
    }
    pub fn set_quic_listener_event_listener(&self, listener: Arc<dyn QuicListenerEventListener>) {
        self.net_listener.set_udp_listener_event_listener(listener);
    }

    pub fn set_tcp_listener_event_listener(&self, listener: Arc<dyn TcpListenerEventListener>) {
        self.net_listener.set_tcp_listener_event_listener(listener);
    }

    // pub fn get_udp_socket(&self, ep: &Endpoint) -> Option<Arc<UDPSocket>> {
    //     self.net_listener.udp_of(ep).map(|udp| udp.socket().as_ref().unwrap().clone())
    // }

    pub fn udp_listeners(&self) -> &Vec<QuicListenerRef> {
        self.net_listener.udp()
    }

    pub fn add_listen_device(&self, device: Device, key: PrivateKey) {
        self.cert_resolver.add_device(device, key);
    }

    pub fn remove_listen_device(&self, device_id: &DeviceId) {
        self.cert_resolver.remove_device(device_id);
    }

    pub fn get_listen_device(&self, device_id: &DeviceId) -> Option<Device> {
        self.cert_resolver.get_device(device_id)
    }

    pub fn get_device_cache(&self) -> &Arc<DeviceCache> {
        &self.device_cache
    }
}
