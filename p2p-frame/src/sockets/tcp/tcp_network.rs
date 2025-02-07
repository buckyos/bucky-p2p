use std::sync::{Arc, Mutex};
use std::time::Duration;
use crate::endpoint::{Endpoint, Protocol};
use crate::error::P2pResult;
use crate::finder::DeviceCache;
use crate::p2p_connection::{P2pConnectionEventListener, P2pConnectionRef, P2pListener, P2pListenerRef};
use crate::p2p_identity::{P2pId, P2pIdentityCertCacheRef, P2pIdentityCertFactoryRef, P2pIdentityRef};
use crate::p2p_network::P2pNetwork;
use crate::sockets::tcp::{TCPConnection, TCPListener, TCPListenerRef};
use crate::tls::ServerCertResolverRef;

pub struct TcpNetwork {
    tcp_listeners: Mutex<Vec<TCPListenerRef>>,
    cert_resolver: ServerCertResolverRef,
    cert_factory: P2pIdentityCertFactoryRef,
    device_cache: P2pIdentityCertCacheRef,
    timeout: Duration,
}

impl TcpNetwork {
    pub fn new(
        device_cache: P2pIdentityCertCacheRef,
        cert_resolver: ServerCertResolverRef,
        cert_factory: P2pIdentityCertFactoryRef,
        timeout: Duration) -> Self {
        Self {
            tcp_listeners: Mutex::new(Vec::new()),
            cert_resolver,
            cert_factory,
            device_cache,
            timeout,
        }
    }
}

#[async_trait::async_trait]
impl P2pNetwork for TcpNetwork {
    fn protocol(&self) -> Protocol {
        Protocol::Tcp
    }

    fn is_udp(&self) -> bool {
        false
    }

    async fn listen(&self, local: &Endpoint, out: Option<Endpoint>, mapping_port: Option<u16>, event: Arc<dyn P2pConnectionEventListener>) -> P2pResult<P2pListenerRef> {
        let tcp_listener = TCPListener::new(self.device_cache.clone(), self.cert_resolver.clone(), self.cert_factory.clone());
        tcp_listener.bind(local.clone(), out, mapping_port).await?;
        tcp_listener.set_connection_event_listener(event);
        tcp_listener.start();
        self.tcp_listeners.lock().unwrap().push(tcp_listener.clone());
        Ok(tcp_listener)
    }

    async fn close_all_listener(&self) -> P2pResult<()> {
        self.tcp_listeners.lock().unwrap().clear();
        Ok(())
    }

    fn listeners(&self) -> Vec<P2pListenerRef> {
        self.tcp_listeners.lock().unwrap().iter().map(|v| v.clone() as P2pListenerRef).collect::<Vec<_>>()
    }

    async fn create_stream_connect(&self, local_identity: &P2pIdentityRef, remote: &Endpoint, remote_id: &P2pId) -> P2pResult<Vec<P2pConnectionRef>> {
        let conn = TCPConnection::connect(self.cert_factory.clone(), local_identity.clone(), remote.clone(), remote_id.clone(), self.timeout).await?;
        Ok(vec![Arc::new(conn)])
    }

    async fn create_stream_connect_with_local_ep(&self, local_identity: &P2pIdentityRef, local_ep: &Endpoint, remote: &Endpoint, remote_id: &P2pId) -> P2pResult<P2pConnectionRef> {
        let conn = TCPConnection::connect_with_ep(self.cert_factory.clone(), local_identity.clone(), local_ep.clone(), remote.clone(), remote_id.clone(), self.timeout).await?;
        Ok(Arc::new(conn))
    }

    async fn create_datagram_connect(&self, local_identity: &P2pIdentityRef, remote: &Endpoint, remote_id: &P2pId) -> P2pResult<Vec<P2pConnectionRef>> {
        self.create_stream_connect(local_identity, remote, remote_id).await
    }

    async fn create_datagram_connect_with_local_ep(&self, local_identity: &P2pIdentityRef, local_ep: &Endpoint, remote: &Endpoint, remote_id: &P2pId) -> P2pResult<P2pConnectionRef> {
        self.create_stream_connect_with_local_ep(local_identity, local_ep, remote, remote_id).await
    }
}
