use std::sync::Arc;
use std::time::Duration;
use crate::endpoint::Endpoint;
use crate::error::P2pResult;
use crate::finder::DeviceCache;
use crate::p2p_identity::{P2pId, P2pIdentityRef, P2pIdentityCertFactoryRef};
use crate::sockets::{NetListener, NetListenerRef, QuicListenerEventListener, QuicListenerRef};
use crate::sockets::tcp::{TCPListenerRef, TcpListenerEventListener};
use crate::tls::{ServerCertResolverRef, TlsServerCertResolver};

pub struct NetManager {
    net_listener: NetListenerRef,
    cert_resolver: ServerCertResolverRef,
    device_cache: Arc<DeviceCache>,
}
pub type NetManagerRef = Arc<NetManager>;

impl NetManager {
    pub async fn open(
        device_cache: Arc<DeviceCache>,
        cert_factory: P2pIdentityCertFactoryRef,
        endpoints: &[Endpoint],
        port_mapping: Option<Vec<(Endpoint, u16)>>,
        tcp_accept_timout: Duration,) -> P2pResult<Self> {
        let cert_resolver = TlsServerCertResolver::new();
        Ok(Self {
            net_listener: NetListener::open(device_cache.clone(), cert_resolver.clone(), cert_factory, endpoints, port_mapping, tcp_accept_timout).await?,
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

    pub fn quic_of(&self, ep: &Endpoint) -> Option<&QuicListenerRef> {
        self.net_listener.udp_of(ep)
    }

    pub fn quic_listeners(&self) -> &Vec<QuicListenerRef> {
        self.net_listener.udp()
    }

    pub fn tcp_listeners(&self) -> &Vec<TCPListenerRef> {
        self.net_listener.tcp()
    }

    pub fn add_listen_device(&self, device: P2pIdentityRef) {
        log::info!("add_listen_device {:?}", device.get_id());
        self.cert_resolver.add_device(device);
    }

    pub fn remove_listen_device(&self, device_id: &P2pId) {
        log::info!("remove_listen_device {:?}", device_id);
        self.cert_resolver.remove_device(device_id);
    }

    pub fn get_listen_device(&self, device_id: &P2pId) -> Option<P2pIdentityRef> {
        self.cert_resolver.get_device(device_id)
    }

    pub fn get_device_cache(&self) -> &Arc<DeviceCache> {
        &self.device_cache
    }
}
