use crate::endpoint::{Endpoint, Protocol};
use crate::error::{P2pError, P2pErrorCode, P2pResult, p2p_err};
use crate::finder::DeviceCache;
use crate::p2p_connection::{P2pConnection, P2pConnectionEventListener, P2pListenerRef};
use crate::p2p_identity::{
    P2pId, P2pIdentityCertCacheRef, P2pIdentityCertFactoryRef, P2pIdentityRef,
};
use crate::p2p_network::P2pNetwork;
use crate::sockets::{QuicCongestionAlgorithm, QuicConnection, QuicListener, QuicListenerRef};
use crate::tls::ServerCertResolverRef;
use rustls::pki_types::CertificateDer;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
struct QuicConnKey {
    listener_local_ep: Endpoint,
    local_identity_id: P2pId,
    remote_ep: Endpoint,
    remote_id_hint: Option<P2pId>,
}

#[derive(Clone)]
struct QuicConnEntry {
    socket: quinn::Connection,
    local_ep: Endpoint,
    remote_ep: Endpoint,
    remote_id: P2pId,
    remote_name: String,
}

pub struct QuicNetwork {
    quic_listener: Mutex<Vec<QuicListenerRef>>,
    connection_pool: Mutex<HashMap<QuicConnKey, QuicConnEntry>>,
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
        idle_timeout: Duration,
    ) -> Self {
        Self {
            quic_listener: Mutex::new(Vec::new()),
            connection_pool: Mutex::new(HashMap::new()),
            cert_resolver,
            cert_factory,
            cert_cache,
            timeout,
            idle_timeout,
            congestion_algorithm,
        }
    }

    fn make_conn_key(
        &self,
        listener_local_ep: Endpoint,
        local_identity: &P2pIdentityRef,
        remote: &Endpoint,
        remote_id: &P2pId,
    ) -> QuicConnKey {
        QuicConnKey {
            listener_local_ep,
            local_identity_id: local_identity.get_id(),
            remote_ep: *remote,
            remote_id_hint: if remote_id.is_default() {
                None
            } else {
                Some(remote_id.clone())
            },
        }
    }

    fn resolve_remote_identity(
        &self,
        socket: &quinn::Connection,
        remote_id: &P2pId,
        remote_name: Option<String>,
    ) -> P2pResult<(P2pId, String)> {
        if remote_id.is_default() {
            let peer_identity = socket.peer_identity();
            let peer_identity = peer_identity
                .as_ref()
                .ok_or_else(|| p2p_err!(P2pErrorCode::CertError, "no peer identity"))?;
            let remote_cert = peer_identity
                .as_ref()
                .downcast_ref::<Vec<CertificateDer>>()
                .ok_or_else(|| p2p_err!(P2pErrorCode::CertError, "peer cert type invalid"))?;
            if remote_cert.is_empty() {
                return Err(p2p_err!(P2pErrorCode::CertError, "no cert"));
            }
            let cert = self
                .cert_factory
                .create(&remote_cert[0].as_ref().to_vec())?;
            Ok((cert.get_id(), cert.get_name()))
        } else {
            Ok((
                remote_id.clone(),
                remote_name.unwrap_or_else(|| remote_id.to_string()),
            ))
        }
    }

    fn build_p2p_connection(
        &self,
        entry: &QuicConnEntry,
        read: quinn::RecvStream,
        send: quinn::SendStream,
        local_identity: &P2pIdentityRef,
    ) -> P2pConnection {
        let read = Box::new(super::QuicRead::new(
            entry.socket.clone(),
            read,
            entry.remote_id.clone(),
            local_identity.get_id(),
            entry.local_ep,
            entry.remote_ep,
            entry.remote_name.clone(),
        ));
        let write = Box::new(super::QuicWrite::new(
            entry.socket.clone(),
            send,
            entry.remote_id.clone(),
            local_identity.get_id(),
            entry.local_ep,
            entry.remote_ep,
            entry.remote_name.clone(),
        ));
        P2pConnection::new(read, write)
    }

    async fn open_or_connect(
        &self,
        listener: &QuicListenerRef,
        local_identity: &P2pIdentityRef,
        remote: &Endpoint,
        remote_id: &P2pId,
        remote_name: Option<String>,
    ) -> P2pResult<P2pConnection> {
        let key = self.make_conn_key(listener.local(), local_identity, remote, remote_id);

        let cached = { self.connection_pool.lock().unwrap().get(&key).cloned() };
        if let Some(entry) = cached {
            if entry.socket.close_reason().is_none() {
                match entry.socket.open_bi().await {
                    Ok((send, recv)) => {
                        self.connection_pool
                            .lock()
                            .unwrap()
                            .insert(key, entry.clone());
                        return Ok(self.build_p2p_connection(&entry, recv, send, local_identity));
                    }
                    Err(e) => {
                        warn!(
                            "quic pooled open_bi failed, reconnect. remote={} err={}",
                            remote, e
                        );
                        self.connection_pool.lock().unwrap().remove(&key);
                    }
                }
            } else {
                self.connection_pool.lock().unwrap().remove(&key);
            }
        }

        let mut conn = QuicConnection::connect_with_ep(
            listener.quic_ep(),
            local_identity.clone(),
            self.cert_factory.clone(),
            remote_id.clone(),
            remote_name.clone(),
            *remote,
            self.congestion_algorithm,
            self.timeout,
            self.idle_timeout,
        )
        .await?;
        let socket = conn.socket().clone();
        let (read, send) = conn.open_bi_stream().await?;
        let (resolved_remote_id, resolved_remote_name) =
            self.resolve_remote_identity(&socket, remote_id, remote_name)?;

        let entry = QuicConnEntry {
            socket,
            local_ep: conn.local(),
            remote_ep: conn.remote(),
            remote_id: resolved_remote_id,
            remote_name: resolved_remote_name,
        };
        self.connection_pool
            .lock()
            .unwrap()
            .insert(key, entry.clone());

        Ok(self.build_p2p_connection(&entry, read, send, local_identity))
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

    async fn listen(
        &self,
        local: &Endpoint,
        out: Option<Endpoint>,
        mapping_port: Option<u16>,
        event: Arc<dyn P2pConnectionEventListener>,
    ) -> P2pResult<P2pListenerRef> {
        let udp_listener = QuicListener::new(
            self.cert_cache.clone(),
            self.cert_resolver.clone(),
            self.cert_factory.clone(),
            self.congestion_algorithm,
        );
        udp_listener.bind(local.clone(), out, mapping_port).await?;
        udp_listener.set_connection_event_listener(event);
        udp_listener.start();
        self.quic_listener
            .lock()
            .unwrap()
            .push(udp_listener.clone());
        Ok(udp_listener)
    }

    async fn close_all_listener(&self) -> P2pResult<()> {
        let conn_list = {
            self.connection_pool
                .lock()
                .unwrap()
                .drain()
                .map(|(_, v)| v)
                .collect::<Vec<_>>()
        };
        for conn in conn_list {
            conn.socket.close(0_u32.into(), b"close all listeners");
        }
        self.quic_listener.lock().unwrap().clear();
        Ok(())
    }

    fn listeners(&self) -> Vec<P2pListenerRef> {
        self.quic_listener
            .lock()
            .unwrap()
            .iter()
            .map(|v| v.clone() as P2pListenerRef)
            .collect()
    }

    async fn create_stream_connect(
        &self,
        local_identity: &P2pIdentityRef,
        remote: &Endpoint,
        remote_id: &P2pId,
        remote_name: Option<String>,
    ) -> P2pResult<Vec<P2pConnection>> {
        let mut conn_list: Vec<P2pConnection> = Vec::new();
        let mut last_err: Option<P2pError> = None;
        let quic_listener = { self.quic_listener.lock().unwrap().clone() };
        for listener in quic_listener.iter() {
            if !listener.local().is_same_ip_version(remote) {
                continue;
            }
            match self
                .open_or_connect(
                    listener,
                    local_identity,
                    remote,
                    remote_id,
                    remote_name.clone(),
                )
                .await
            {
                Ok(conn) => conn_list.push(conn),
                Err(e) => {
                    last_err = Some(e);
                }
            }
        }
        if conn_list.is_empty() {
            if let Some(err) = last_err {
                Err(err)
            } else {
                Err(p2p_err!(
                    P2pErrorCode::NotFound,
                    "no listener found for remote: {}",
                    remote
                ))
            }
        } else {
            Ok(conn_list)
        }
    }

    async fn create_stream_connect_with_local_ep(
        &self,
        local_identity: &P2pIdentityRef,
        local_ep: &Endpoint,
        remote: &Endpoint,
        remote_id: &P2pId,
        remote_name: Option<String>,
    ) -> P2pResult<P2pConnection> {
        let quic_listener = { self.quic_listener.lock().unwrap().clone() };
        for listener in quic_listener.iter() {
            let listen_ep = listener.local();
            if &listen_ep != local_ep || !listener.local().is_same_ip_version(remote) {
                continue;
            }
            return self
                .open_or_connect(listener, local_identity, remote, remote_id, remote_name)
                .await;
        }
        Err(p2p_err!(
            P2pErrorCode::NotFound,
            "no listener found for local ep: {}",
            local_ep
        ))
    }

    async fn create_datagram_connect(
        &self,
        local_identity: &P2pIdentityRef,
        remote: &Endpoint,
        remote_id: &P2pId,
        remote_name: Option<String>,
    ) -> P2pResult<Vec<P2pConnection>> {
        self.create_stream_connect(local_identity, remote, remote_id, remote_name)
            .await
    }

    async fn create_datagram_connect_with_local_ep(
        &self,
        local_identity: &P2pIdentityRef,
        local_ep: &Endpoint,
        remote: &Endpoint,
        remote_id: &P2pId,
        remote_name: Option<String>,
    ) -> P2pResult<P2pConnection> {
        self.create_stream_connect_with_local_ep(
            local_identity,
            local_ep,
            remote,
            remote_id,
            remote_name,
        )
        .await
    }
}

#[cfg(all(test, feature = "x509"))]
mod tests {
    use std::sync::{Arc, Once};
    use std::time::Duration;

    use crate::endpoint::{Endpoint, Protocol};
    use crate::error::P2pErrorCode;
    use crate::executor::Executor;
    use crate::finder::{DeviceCache, DeviceCacheConfig};
    use crate::p2p_connection::{P2pConnection, P2pConnectionEventListener};
    use crate::p2p_identity::{
        P2pId, P2pIdentityCertCacheRef, P2pIdentityCertFactoryRef, P2pIdentityRef,
    };
    use crate::p2p_network::P2pNetwork;
    use crate::sockets::QuicCongestionAlgorithm;
    use crate::tls::{DefaultTlsServerCertResolver, TlsServerCertResolver};
    use crate::x509::{X509IdentityCertFactory, X509IdentityFactory, generate_x509_identity};

    use super::QuicNetwork;

    static TLS_INIT: Once = Once::new();

    struct NoopConnListener;

    #[async_trait::async_trait]
    impl P2pConnectionEventListener for NoopConnListener {
        async fn on_new_connection(&self, _conn: P2pConnection) -> crate::error::P2pResult<()> {
            Ok(())
        }
    }

    fn init_tls_once() {
        TLS_INIT.call_once(|| {
            Executor::init();
            crate::tls::init_tls(Arc::new(X509IdentityFactory));
        });
    }

    fn new_cert_cache() -> P2pIdentityCertCacheRef {
        Arc::new(DeviceCache::new(
            &DeviceCacheConfig {
                expire: Duration::from_secs(60),
                capacity: 64,
            },
            None,
        ))
    }

    fn new_identity() -> P2pIdentityRef {
        Arc::new(generate_x509_identity(None).unwrap())
    }

    fn new_cert_factory() -> P2pIdentityCertFactoryRef {
        Arc::new(X509IdentityCertFactory)
    }

    fn quic_network(
        cert_cache: P2pIdentityCertCacheRef,
        resolver: Arc<DefaultTlsServerCertResolver>,
        cert_factory: P2pIdentityCertFactoryRef,
    ) -> QuicNetwork {
        QuicNetwork::new(
            cert_cache,
            resolver,
            cert_factory,
            QuicCongestionAlgorithm::Bbr,
            Duration::from_secs(3),
            Duration::from_secs(10),
        )
    }

    async fn setup_network_pair() -> (
        QuicNetwork,
        P2pIdentityRef,
        Endpoint,
        QuicNetwork,
        P2pIdentityRef,
        Endpoint,
    ) {
        init_tls_once();

        let cert_factory = new_cert_factory();

        let server_resolver = DefaultTlsServerCertResolver::new();
        let server_identity = new_identity();
        server_resolver
            .add_server_identity(server_identity.clone())
            .await
            .unwrap();
        server_resolver
            .set_default_server_identity(&server_identity.get_id())
            .unwrap();
        let server_network = quic_network(new_cert_cache(), server_resolver, cert_factory.clone());
        let _ = server_network
            .listen(
                &Endpoint::from((Protocol::Quic, "127.0.0.1:0".parse().unwrap())),
                None,
                None,
                Arc::new(NoopConnListener),
            )
            .await
            .unwrap();
        let server_remote_ep = Endpoint::from((
            Protocol::Quic,
            server_network
                .quic_listener
                .lock()
                .unwrap()
                .first()
                .unwrap()
                .quic_ep()
                .local_addr()
                .unwrap(),
        ));

        let client_resolver = DefaultTlsServerCertResolver::new();
        let client_identity = new_identity();
        client_resolver
            .add_server_identity(client_identity.clone())
            .await
            .unwrap();
        let client_network = quic_network(new_cert_cache(), client_resolver, cert_factory);
        let _ = client_network
            .listen(
                &Endpoint::from((Protocol::Quic, "127.0.0.1:0".parse().unwrap())),
                None,
                None,
                Arc::new(NoopConnListener),
            )
            .await
            .unwrap();
        let client_local_ep = client_network
            .quic_listener
            .lock()
            .unwrap()
            .first()
            .unwrap()
            .local();

        (
            client_network,
            client_identity,
            client_local_ep,
            server_network,
            server_identity,
            server_remote_ep,
        )
    }

    #[tokio::test]
    async fn quic_network_reuse_connection_pool_same_key() {
        let (
            client_network,
            client_identity,
            client_local_ep,
            _server_network,
            server_identity,
            server_remote_ep,
        ) = setup_network_pair().await;

        let _ = client_network
            .create_stream_connect_with_local_ep(
                &client_identity,
                &client_local_ep,
                &server_remote_ep,
                &server_identity.get_id(),
                Some(server_identity.get_name()),
            )
            .await
            .unwrap();

        let first_stable_id = {
            let pool = client_network.connection_pool.lock().unwrap();
            assert_eq!(pool.len(), 1);
            pool.values().next().unwrap().socket.stable_id()
        };

        let _ = client_network
            .create_stream_connect_with_local_ep(
                &client_identity,
                &client_local_ep,
                &server_remote_ep,
                &server_identity.get_id(),
                Some(server_identity.get_name()),
            )
            .await
            .unwrap();

        let second_stable_id = {
            let pool = client_network.connection_pool.lock().unwrap();
            assert_eq!(pool.len(), 1);
            pool.values().next().unwrap().socket.stable_id()
        };

        assert_eq!(first_stable_id, second_stable_id);
    }

    #[tokio::test]
    async fn quic_network_pool_key_separates_by_local_identity() {
        let (
            client_network,
            client_identity,
            client_local_ep,
            _server_network,
            server_identity,
            server_remote_ep,
        ) = setup_network_pair().await;

        let other_identity = new_identity();

        let _ = client_network
            .create_stream_connect_with_local_ep(
                &client_identity,
                &client_local_ep,
                &server_remote_ep,
                &server_identity.get_id(),
                Some(server_identity.get_name()),
            )
            .await
            .unwrap();

        let _ = client_network
            .create_stream_connect_with_local_ep(
                &other_identity,
                &client_local_ep,
                &server_remote_ep,
                &server_identity.get_id(),
                Some(server_identity.get_name()),
            )
            .await
            .unwrap();

        let pool = client_network.connection_pool.lock().unwrap();
        assert_eq!(pool.len(), 2);
    }

    #[tokio::test]
    async fn quic_network_close_all_listener_clears_pool() {
        let (
            client_network,
            client_identity,
            client_local_ep,
            _server_network,
            server_identity,
            server_remote_ep,
        ) = setup_network_pair().await;

        let _ = client_network
            .create_stream_connect_with_local_ep(
                &client_identity,
                &client_local_ep,
                &server_remote_ep,
                &server_identity.get_id(),
                Some(server_identity.get_name()),
            )
            .await
            .unwrap();

        assert_eq!(client_network.connection_pool.lock().unwrap().len(), 1);
        client_network.close_all_listener().await.unwrap();
        assert_eq!(client_network.connection_pool.lock().unwrap().len(), 0);
        assert!(client_network.quic_listener.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn quic_network_create_stream_connect_no_listener_returns_not_found() {
        init_tls_once();

        let cert_factory = new_cert_factory();
        let resolver = DefaultTlsServerCertResolver::new();
        let network = quic_network(new_cert_cache(), resolver, cert_factory);
        let local_identity = new_identity();

        let ret = network
            .create_stream_connect(
                &local_identity,
                &Endpoint::from((Protocol::Quic, "127.0.0.1:44500".parse().unwrap())),
                &P2pId::default(),
                None,
            )
            .await;

        assert!(ret.is_err());
        let err = ret.err().unwrap();
        assert_eq!(err.code(), P2pErrorCode::NotFound);
    }

    #[tokio::test]
    async fn quic_network_create_stream_connect_with_local_ep_not_found() {
        let (
            client_network,
            client_identity,
            _client_local_ep,
            _server_network,
            server_identity,
            server_remote_ep,
        ) = setup_network_pair().await;

        let wrong_local_ep = Endpoint::from((Protocol::Quic, "127.0.0.1:45432".parse().unwrap()));
        let ret = client_network
            .create_stream_connect_with_local_ep(
                &client_identity,
                &wrong_local_ep,
                &server_remote_ep,
                &server_identity.get_id(),
                Some(server_identity.get_name()),
            )
            .await;

        assert!(ret.is_err());
        let err = ret.err().unwrap();
        assert_eq!(err.code(), P2pErrorCode::NotFound);
    }

    #[tokio::test]
    async fn quic_network_create_stream_connect_wrong_remote_id_should_fail_and_not_cache() {
        let (
            client_network,
            client_identity,
            client_local_ep,
            _server_network,
            server_identity,
            server_remote_ep,
        ) = setup_network_pair().await;

        let wrong_remote_id = {
            let id = new_identity();
            assert_ne!(id.get_id(), server_identity.get_id());
            id.get_id()
        };

        let ret = client_network
            .create_stream_connect_with_local_ep(
                &client_identity,
                &client_local_ep,
                &server_remote_ep,
                &wrong_remote_id,
                Some(server_identity.get_name()),
            )
            .await;

        assert!(ret.is_err());
        assert_eq!(client_network.connection_pool.lock().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn quic_network_protocol_udp_and_listeners_ok() {
        let (
            client_network,
            _client_identity,
            _client_local_ep,
            _server_network,
            _server_identity,
            _server_remote_ep,
        ) = setup_network_pair().await;

        assert_eq!(client_network.protocol(), Protocol::Quic);
        assert!(client_network.is_udp());
        assert!(!client_network.listeners().is_empty());
    }

    #[tokio::test]
    async fn quic_network_create_datagram_connect_wrappers_ok() {
        let (
            client_network,
            client_identity,
            client_local_ep,
            _server_network,
            server_identity,
            server_remote_ep,
        ) = setup_network_pair().await;

        let conns = client_network
            .create_datagram_connect(
                &client_identity,
                &server_remote_ep,
                &server_identity.get_id(),
                Some(server_identity.get_name()),
            )
            .await
            .unwrap();
        assert!(!conns.is_empty());

        let conn = client_network
            .create_datagram_connect_with_local_ep(
                &client_identity,
                &client_local_ep,
                &server_remote_ep,
                &server_identity.get_id(),
                Some(server_identity.get_name()),
            )
            .await
            .unwrap();
        drop(conn);
    }

    #[tokio::test]
    async fn quic_network_create_stream_connect_default_remote_id_ok() {
        let (
            client_network,
            client_identity,
            client_local_ep,
            _server_network,
            server_identity,
            server_remote_ep,
        ) = setup_network_pair().await;

        let conn = client_network
            .create_stream_connect_with_local_ep(
                &client_identity,
                &client_local_ep,
                &server_remote_ep,
                &P2pId::default(),
                None,
            )
            .await
            .unwrap();
        drop(conn);

        let pool = client_network.connection_pool.lock().unwrap();
        assert_eq!(pool.len(), 1);
        let entry = pool.values().next().unwrap();
        assert_eq!(entry.remote_id, server_identity.get_id());
    }

    #[tokio::test]
    async fn quic_network_reconnect_when_cached_connection_closed() {
        let (
            client_network,
            client_identity,
            client_local_ep,
            _server_network,
            server_identity,
            server_remote_ep,
        ) = setup_network_pair().await;

        let _ = client_network
            .create_stream_connect_with_local_ep(
                &client_identity,
                &client_local_ep,
                &server_remote_ep,
                &server_identity.get_id(),
                Some(server_identity.get_name()),
            )
            .await
            .unwrap();

        let first_stable_id = {
            let pool = client_network.connection_pool.lock().unwrap();
            pool.values().next().unwrap().socket.stable_id()
        };

        {
            let pool = client_network.connection_pool.lock().unwrap();
            let entry = pool.values().next().unwrap();
            entry.socket.close(0_u32.into(), b"test close");
        }

        tokio::time::sleep(Duration::from_millis(50)).await;

        let _ = client_network
            .create_stream_connect_with_local_ep(
                &client_identity,
                &client_local_ep,
                &server_remote_ep,
                &server_identity.get_id(),
                Some(server_identity.get_name()),
            )
            .await
            .unwrap();

        let second_stable_id = {
            let pool = client_network.connection_pool.lock().unwrap();
            pool.values().next().unwrap().socket.stable_id()
        };

        assert_ne!(first_stable_id, second_stable_id);
    }
}
