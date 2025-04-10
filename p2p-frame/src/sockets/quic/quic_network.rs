use std::sync::{Arc, Mutex};
use std::time::Duration;
use crate::endpoint::{Endpoint, Protocol};
use crate::error::{p2p_err, P2pErrorCode, P2pResult};
use crate::finder::DeviceCache;
use rustls::pki_types::{CertificateDer};
use crate::p2p_connection::{P2pConnectionEventListener, P2pConnection, P2pListenerRef};
use crate::p2p_identity::{P2pId, P2pIdentityCertCacheRef, P2pIdentityCertFactoryRef, P2pIdentityRef};
use crate::p2p_network::P2pNetwork;
use crate::sockets::{QuicCongestionAlgorithm, QuicConnection, QuicListener, QuicListenerRef};
use crate::tls::ServerCertResolverRef;

pub struct QuicNetwork {
    quic_listener: Mutex<Vec<QuicListenerRef>>,
    cert_resolver: ServerCertResolverRef,
    cert_factory: P2pIdentityCertFactoryRef,
    cert_cache: P2pIdentityCertCacheRef,
    timeout: Duration,
    idle_timeout: Duration,
    congestion_algorithm: QuicCongestionAlgorithm,
}

impl QuicNetwork {
    pub fn new(
        cert_cache: P2pIdentityCertCacheRef,
        cert_resolver: ServerCertResolverRef,
        cert_factory: P2pIdentityCertFactoryRef,
        congestion_algorithm: QuicCongestionAlgorithm,
        timeout: Duration,
        idle_timeout: Duration) -> Self {
        Self {
            quic_listener: Mutex::new(Vec::new()),
            cert_resolver,
            cert_factory,
            cert_cache,
            timeout,
            idle_timeout,
            congestion_algorithm,
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
            self.cert_factory.clone(),
            self.congestion_algorithm, );
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

    async fn create_stream_connect(&self, local_identity: &P2pIdentityRef, remote: &Endpoint, remote_id: &P2pId, remote_name: Option<String>) -> P2pResult<Vec<P2pConnection>> {
        let mut conn_list: Vec<P2pConnection> = Vec::new();
        let quic_listener = {
            self.quic_listener.lock().unwrap().clone()
        };
        for listener in quic_listener.iter() {
            if !listener.local().is_same_ip_version(remote) {
                continue;
            }
            let mut conn = QuicConnection::connect_with_ep(listener.quic_ep(),
                                                           local_identity.clone(),
                                                           self.cert_factory.clone(),
                                                           remote_id.clone(),
                                                           remote_name.clone(),
                                                           remote.clone(),
                                                           self.congestion_algorithm,
                                                           self.timeout,
                                                           self.idle_timeout).await?;
            let (read, send) = conn.open_bi_stream().await?;
            let (remote_id, remote_name) = if remote_id.is_default() {
                let peer_identity = conn.socket().peer_identity();
                let remote_cert = peer_identity.as_ref().unwrap().as_ref().downcast_ref::<Vec<CertificateDer>>();
                if remote_cert.is_none() || remote_cert.as_ref().unwrap().len() == 0 {
                    continue;
                }
                let cert = remote_cert.unwrap()[0].as_ref();
                let cert = self.cert_factory.create(&cert.to_vec())?;
                (cert.get_id(), cert.get_name())
            } else {
                (remote_id.clone(), remote_name.clone().unwrap_or(remote_id.to_string()))
            };
            let read = Box::new(super::QuicRead::new(conn.socket().clone(),
                                                     read,
                                                     remote_id.clone(),
                                                     local_identity.get_id(),
                                                     conn.local().clone(),
                                                     conn.remote().clone(),
                                                     remote_name.clone()));
            let write = Box::new(super::QuicWrite::new(conn.socket().clone(),
                                                       send,
                                                       remote_id.clone(),
                                                       local_identity.get_id(),
                                                       conn.local().clone(),
                                                       conn.remote().clone(),
                                                       remote_name.clone()));
            conn_list.push(P2pConnection::new(read, write));
        }
        Ok(conn_list)
    }

    async fn create_stream_connect_with_local_ep(&self, local_identity: &P2pIdentityRef, local_ep: &Endpoint, remote: &Endpoint, remote_id: &P2pId, remote_name: Option<String>) -> P2pResult<P2pConnection> {
        let quic_listener = {
            self.quic_listener.lock().unwrap().clone()
        };
        for listener in quic_listener.iter() {
            let listen_ep = listener.local();
            if &listen_ep != local_ep || !listener.local().is_same_ip_version(remote) {
                continue;
            }

            let mut conn = QuicConnection::connect_with_ep(listener.quic_ep(),
                                                           local_identity.clone(),
                                                           self.cert_factory.clone(),
                                                           remote_id.clone(),
                                                           remote_name.clone(),
                                                           remote.clone(),
                                                           self.congestion_algorithm,
                                                           self.timeout,
                                                           self.idle_timeout).await?;
            let (read, send) = conn.open_bi_stream().await?;
            let (remote_id, remote_name) = if remote_id.is_default() {
                let peer_identity = conn.socket().peer_identity();
                let remote_cert = peer_identity.as_ref().unwrap().as_ref().downcast_ref::<Vec<CertificateDer>>();
                if remote_cert.is_none() || remote_cert.as_ref().unwrap().len() == 0 {
                    continue;
                }
                let cert = remote_cert.unwrap()[0].as_ref();
                let cert = self.cert_factory.create(&cert.to_vec())?;
                (cert.get_id(), cert.get_name())
            } else {
                (remote_id.clone(), remote_name.clone().unwrap_or(remote_id.to_string()))
            };
            let read = Box::new(super::QuicRead::new(conn.socket().clone(),
                                                     read,
                                                     remote_id.clone(),
                                                     local_identity.get_id(),
                                                     conn.local().clone(),
                                                     conn.remote().clone(),
                                                     remote_name.clone()));
            let write = Box::new(super::QuicWrite::new(conn.socket().clone(),
                                                       send,
                                                       remote_id.clone(),
                                                       local_identity.get_id(),
                                                       conn.local().clone(),
                                                       conn.remote().clone(),
                                                       remote_name.clone()));
            return Ok(P2pConnection::new(read, write));
        }
        Err(p2p_err!(P2pErrorCode::NotFound, "no listener found for local ep: {}", local_ep))
    }

    async fn create_datagram_connect(&self, local_identity: &P2pIdentityRef, remote: &Endpoint, remote_id: &P2pId, remote_name: Option<String>) -> P2pResult<Vec<P2pConnection>> {
        self.create_stream_connect(local_identity, remote, remote_id, remote_name).await
    }

    async fn create_datagram_connect_with_local_ep(&self, local_identity: &P2pIdentityRef, local_ep: &Endpoint, remote: &Endpoint, remote_id: &P2pId, remote_name: Option<String>) -> P2pResult<P2pConnection> {
        self.create_stream_connect_with_local_ep(local_identity, local_ep, remote, remote_id,remote_name).await
    }
}
