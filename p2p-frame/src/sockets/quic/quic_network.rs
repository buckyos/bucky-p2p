use std::sync::{Arc, Mutex};
use std::time::Duration;
use crate::endpoint::{Endpoint, Protocol};
use crate::error::P2pResult;
use crate::finder::DeviceCache;
use crate::p2p_connection::{P2pConnectionEventListener, P2pConnectionRef, P2pListenerRef};
use crate::p2p_identity::{P2pId, P2pIdentityCertCacheRef, P2pIdentityCertFactoryRef, P2pIdentityRef};
use crate::p2p_network::P2pNetwork;
use crate::sockets::{QuicConnection, QuicListener, QuicListenerRef};
use crate::tls::ServerCertResolverRef;

pub struct QuicNetwork {
    quic_listener: Mutex<Vec<QuicListenerRef>>,
    cert_resolver: ServerCertResolverRef,
    cert_factory: P2pIdentityCertFactoryRef,
    cert_cache: P2pIdentityCertCacheRef,
    timeout: Duration,
    idle_timeout: Duration
}

impl QuicNetwork {
    pub fn new(
        cert_cache: P2pIdentityCertCacheRef,
        cert_resolver: ServerCertResolverRef,
        cert_factory: P2pIdentityCertFactoryRef,
        timeout: Duration,
        idle_timeout: Duration) -> Self {
        Self {
            quic_listener: Mutex::new(Vec::new()),
            cert_resolver,
            cert_factory,
            cert_cache,
            timeout,
            idle_timeout,
        }
    }

}

#[async_trait::async_trait]
impl P2pNetwork for QuicNetwork {
    fn protocol(&self) -> Protocol {
        Protocol::Quic
    }

    fn is_udp(&self) -> bool {
        true
    }

    async fn listen(&self, local: &Endpoint, out: Option<Endpoint>, mapping_port: Option<u16>, event: Arc<dyn P2pConnectionEventListener>) -> P2pResult<P2pListenerRef> {
        let udp_listener = QuicListener::new(
            self.cert_cache.clone(),
            self.cert_resolver.clone(),
            self.cert_factory.clone(),);
        udp_listener.bind(local.clone(), out, mapping_port).await?;
        udp_listener.set_connection_event_listener(event);
        udp_listener.start();
        self.quic_listener.lock().unwrap().push(udp_listener.clone());
        Ok(udp_listener)
    }

    async fn close_all_listener(&self) -> P2pResult<()> {
        self.quic_listener.lock().unwrap().clear();
        Ok(())
    }

    fn listeners(&self) -> Vec<P2pListenerRef> {
        self.quic_listener.lock().unwrap().iter().map(|v| v.clone() as P2pListenerRef).collect()
    }

    async fn create_stream_connect(&self, local_identity: &P2pIdentityRef, remote: &Endpoint, remote_id: &P2pId) -> P2pResult<Vec<P2pConnectionRef>> {
        let mut conn_list: Vec<P2pConnectionRef> = Vec::new();
        let quic_listener = {
            self.quic_listener.lock().unwrap().clone()
        };
        for listener in quic_listener.iter() {
            let mut conn = QuicConnection::connect_with_ep(listener.quic_ep(), local_identity.clone(), self.cert_factory.clone(), remote_id.clone(), remote.clone(), self.timeout, self.idle_timeout).await?;
            conn.open_bi_stream().await?;
            conn_list.push(Arc::new(conn));
        }
        Ok(conn_list)
    }

    async fn create_datagram_connect(&self, local_identity: &P2pIdentityRef, remote: &Endpoint, remote_id: &P2pId) -> P2pResult<Vec<P2pConnectionRef>> {
        self.create_stream_connect(local_identity, remote, remote_id).await
    }
}
