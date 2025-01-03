use std::sync::{Arc};
use std::time::Duration;
use crate::endpoint::Endpoint;
use crate::error::P2pResult;
use crate::finder::DeviceCache;
use crate::p2p_connection::{P2pConnectionEventListener, P2pConnectionRef};
use crate::p2p_identity::{P2pId, P2pIdentityRef, P2pIdentityCertFactoryRef};
use crate::sockets::{NetListener, NetListenerRef, QuicListenerRef};
use crate::sockets::tcp::TCPListenerRef;
use crate::tls::{ServerCertResolverRef};

pub struct NetManager {
    net_listeners: NetListenerRef,
    cert_resolver: ServerCertResolverRef,
    device_cache: Arc<DeviceCache>,
}
pub type NetManagerRef = Arc<NetManager>;

impl NetManager {
    pub async fn open(device_cache: Arc<DeviceCache>,
                      cert_resolver: ServerCertResolverRef,
                      cert_factory: P2pIdentityCertFactoryRef,
                      endpoints: &[Endpoint],
                      port_mapping: Option<Vec<(Endpoint, u16)>>,
                      tcp_accept_timout: Duration,) -> P2pResult<Self> {
        Ok(Self {
            net_listeners: NetListener::open(device_cache.clone(), cert_resolver.clone(), cert_factory, endpoints, port_mapping, tcp_accept_timout).await?,
            cert_resolver,
            device_cache,
        })
    }

    pub fn set_conn_event_listener(&self, listener: impl P2pConnectionEventListener) {
        let listener = Arc::new(listener);
        self.net_listeners.set_connect_event_listener(move |conn: P2pConnectionRef| {
            let listener = listener.clone();
            async move {
                listener.on_new_connection(conn).await
            }
        });
    }

    // pub fn get_udp_socket(&self, ep: &Endpoint) -> Option<Arc<UDPSocket>> {
    //     self.net_listener.udp_of(ep).map(|udp| udp.socket().as_ref().unwrap().clone())
    // }

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

    pub fn port_mapping(&self) -> &Vec<(Endpoint, u16)> {
        self.net_listeners.port_mapping()
    }

    pub fn quic_listeners(&self) -> &Vec<QuicListenerRef> {
        self.net_listeners.quic()
    }

    pub fn tcp_listeners(&self) -> &Vec<TCPListenerRef> {
        self.net_listeners.tcp()
    }
}
