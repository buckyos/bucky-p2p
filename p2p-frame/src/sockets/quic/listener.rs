use std::str::FromStr;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use quinn::Incoming;
use quinn::crypto::rustls::{HandshakeData, QuicServerConfig};
use rustls::pki_types::{CertificateDer};
use rustls::server::ResolvesServerCert;
use rustls::version::TLS13;
use crate::error::{p2p_err, P2pErrorCode, P2pResult, into_p2p_err};
use crate::endpoint::{Endpoint, Protocol};
use crate::executor::Executor;
use crate::finder::DeviceCache;
use crate::p2p_connection::{P2pConnection, P2pConnectionEventListener, P2pListener};
use crate::p2p_identity::{P2pId, P2pIdentityCertCache, P2pIdentityCertCacheRef, P2pIdentityCertFactoryRef};
use crate::sockets::{parse_server_name, QuicCongestionAlgorithm, QuicConnection, UpdateOuterResult};
use crate::tls::ServerCertResolverRef;

struct QuicListenerState {
    local: Option<Endpoint>,
    outer: Option<Endpoint>,
    socket: Option<quinn::Endpoint>,
    mapping_port: Option<u16>,
}

pub struct QuicListener {
    cert_cache: P2pIdentityCertCacheRef,
    cert_resolver: ServerCertResolverRef,
    state: RwLock<QuicListenerState>,
    quic_listener: RwLock<Option<Arc<dyn P2pConnectionEventListener>>>,
    cert_factory: P2pIdentityCertFactoryRef,
    congestion_algorithm: QuicCongestionAlgorithm,
}
pub type QuicListenerRef = Arc<QuicListener>;

impl QuicListener {
    pub fn new(
        cert_cache: P2pIdentityCertCacheRef,
        cert_resolver: ServerCertResolverRef,
        cert_factory: P2pIdentityCertFactoryRef,
        congestion_algorithm: QuicCongestionAlgorithm,
    ) -> Arc<Self> {
        Arc::new(Self {
            cert_cache,
            cert_resolver,
            state: RwLock::new(QuicListenerState {
                local: None,
                outer: None,
                socket: None,
                mapping_port: None,
            }),
            quic_listener: RwLock::new(None),
            cert_factory,
            congestion_algorithm,
        })
    }

    pub fn local(&self) -> Endpoint {
        self.state.read().unwrap().local.clone().unwrap()
    }

    pub fn outer(&self) -> Option<Endpoint> {
        self.state.read().unwrap().outer.clone()
    }

    pub fn set_local(&self, local: Endpoint) {
        self.state.write().unwrap().local = Some(local);
    }

    pub fn quic_ep(&self) -> quinn::Endpoint {
        self.state.read().unwrap().socket.clone().unwrap()
    }

    pub fn mapping_port(&self) -> Option<u16> {
        self.state.read().unwrap().mapping_port
    }

    pub async fn reset(&self, _new_ep: &Endpoint) -> Arc<Self> {
        todo!()
    }

    pub fn update_outer(&self, outer: &Endpoint) -> UpdateOuterResult {
        let self_outer = &mut *self.state.write().unwrap();
        if let Some(outer_ep) = self_outer.outer.as_ref() {
            if *outer_ep != *outer {
                info!("{:?} reset outer to {}", self_outer.socket.as_ref().unwrap(), outer);
                self_outer.outer = Some(*outer);
                UpdateOuterResult::Reset
            } else {
                trace!("{:?} ignore update outer to {}", self_outer.socket.as_ref().unwrap(), outer);
                UpdateOuterResult::None
            }
        } else {
            info!("{:?} update outer to {}", self_outer.socket.as_ref().unwrap(), outer);
            self_outer.outer = Some(*outer);
            UpdateOuterResult::Update
        }
    }


    pub async fn bind(&self, local: Endpoint, out: Option<Endpoint>, mapping_port: Option<u16>) -> P2pResult<()> {
        let cert_resolver = self.cert_resolver.clone();
        let mut server_config =
            rustls::ServerConfig::builder_with_provider(crate::tls::provider().into())
                .with_protocol_versions(&[&TLS13])
                .map_err(into_p2p_err!(P2pErrorCode::TlsError, "Create server config error"))?
                .with_client_cert_verifier(Arc::new(crate::tls::TlsClientCertVerifier::new(self.cert_factory.clone())))
                .with_cert_resolver(cert_resolver.get_resolves_server_cert());
        server_config.key_log = Arc::new(rustls::KeyLogFile::new());

        let mut server_config =
            quinn::ServerConfig::with_crypto(Arc::new(QuicServerConfig::try_from(server_config).map_err(into_p2p_err!(P2pErrorCode::TlsError, "create quic server config failed"))?));
        let transport_config = Arc::get_mut(&mut server_config.transport).unwrap();
        transport_config.max_idle_timeout(Some(std::time::Duration::from_secs(600).try_into().unwrap()));
        match self.congestion_algorithm {
            QuicCongestionAlgorithm::Bbr => {
                transport_config.congestion_controller_factory(Arc::new(quinn::congestion::BbrConfig::default()));
            }
            QuicCongestionAlgorithm::Cubic => {
                transport_config.congestion_controller_factory(Arc::new(quinn::congestion::CubicConfig::default()));
            }
            QuicCongestionAlgorithm::NewReno => {
                transport_config.congestion_controller_factory(Arc::new(quinn::congestion::NewRenoConfig::default()));
            }
        }

        let endpoint = quinn::Endpoint::server(server_config, local.addr().clone()).map_err(into_p2p_err!(P2pErrorCode::QuicError, "Create quic server error"))?;

        let mut state = self.state.write().unwrap();
        state.local = Some(local.clone());
        state.outer = out;
        state.socket = Some(endpoint);
        state.mapping_port = mapping_port;

        Ok(())
    }

    async fn accept(&self, conn: Incoming) -> P2pResult<Option<P2pConnection>> {
        let connection = conn.await.map_err(into_p2p_err!(P2pErrorCode::QuicError, "QuicListener accept error"))?;
        let (server_name, remote_cert) = {
            let handshake_data = connection.handshake_data();
            if handshake_data.is_none() {
                return Err(p2p_err!(P2pErrorCode::TlsError, "no handshake data"));
            }
            let handshake_data = handshake_data.as_ref().unwrap().as_ref().downcast_ref::<HandshakeData>();
            if handshake_data.is_none() {
                return Err(p2p_err!(P2pErrorCode::TlsError, "no handshake data"));
            }

            let server_name = handshake_data.unwrap().server_name.as_ref();
            if server_name.is_none() {
                return Err(p2p_err!(P2pErrorCode::TlsError, "no server name"));
            }
            let peer_identity = connection.peer_identity();
            let remote_cert = peer_identity.as_ref().unwrap().as_ref().downcast_ref::<Vec<CertificateDer>>();
            if remote_cert.is_none() || remote_cert.as_ref().unwrap().len() == 0 {
                return Err(p2p_err!(P2pErrorCode::CertError, "no cert"));
            }

            let server_name = server_name.unwrap();
            let server_name = parse_server_name(server_name);

            (server_name.to_string(), remote_cert.unwrap()[0].as_ref().to_vec())
        };

        let local_cert = self.cert_resolver.get_server_identity(server_name.as_str()).await;
        if local_cert.is_none() {
            return Err(p2p_err!(P2pErrorCode::CertError, "no local cert"));
        }
        let local_id = local_cert.unwrap().get_id();

        let remote_device = self.cert_factory.create(&remote_cert)?;
        self.cert_cache.add(&remote_device.get_id(), &remote_device).await?;
        let remote_addr = connection.remote_address();
        let mut conn = QuicConnection::new(connection,
                                         self.local(),
                                         Endpoint::from((Protocol::Quic, remote_addr)));
        let (read, send) = conn.accept().await?;
        if send.is_some() {
            let read = Box::new(super::QuicRead::new(conn.socket().clone(),
                                                     read,
                                                     remote_device.get_id(),
                                                     local_id.clone(),
                                                     conn.local().clone(),
                                                     conn.remote().clone(),
                                                     remote_device.get_name()));
            let write = Box::new(super::QuicWrite::new(conn.socket().clone(),
                                                       send.unwrap(),
                                                       remote_device.get_id(),
                                                       local_id,
                                                       conn.local().clone(),
                                                       conn.remote().clone(),
                                                       remote_device.get_name()));
            Ok(Some(P2pConnection::new(read, write)))
        } else {
            Ok(None)
        }
    }

    pub fn start(self: &Arc<Self>) {
        let this = self.clone();
        let socket = self.state.read().unwrap().socket.clone().unwrap();
        let quic_listener = self.quic_listener.read().unwrap().as_ref().unwrap().clone();
        let _ = Executor::spawn(async move {
            loop {
                match socket.accept().await {
                    None => {
                        error!("QuicListener accept error");
                        break;
                    }
                    Some(conn) => {
                        let this = this.clone();
                        let quic_listener = quic_listener.clone();
                        let _ = Executor::spawn(async move {
                            match this.accept(conn).await {
                                Ok(socket) => {
                                    if let Some(socket) = socket {
                                        if let Err(e) = quic_listener.on_new_connection(socket).await {
                                            error!("QuicListener on_new_connection error: {}", e);
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("QuicListener accept error: {}", e);
                                }
                            }
                        });
                    }
                }
            }
        });
    }

    pub fn set_connection_event_listener(&self, event: Arc<dyn P2pConnectionEventListener>) {
        *self.quic_listener.write().unwrap() = Some(event);
    }

}

impl P2pListener for QuicListener {
    fn mapping_port(&self) -> Option<u16> {
        self.state.read().unwrap().mapping_port
    }

    fn local(&self) -> Endpoint {
        self.state.read().unwrap().local.clone().unwrap()
    }

}

#[cfg(all(test, feature = "x509"))]
mod tests {
    use std::sync::{Arc, Once};
    use std::time::Duration;

    use tokio::sync::oneshot;

    use crate::endpoint::{Endpoint, Protocol};
    use crate::executor::Executor;
    use crate::finder::{DeviceCache, DeviceCacheConfig};
    use crate::p2p_connection::{P2pConnection, P2pConnectionEventListener, P2pListener};
    use crate::p2p_identity::{P2pIdentityCertCacheRef, P2pIdentityCertFactoryRef, P2pIdentityRef};
    use crate::sockets::{QuicCongestionAlgorithm, QuicConnection, UpdateOuterResult};
    use crate::tls::{DefaultTlsServerCertResolver, TlsServerCertResolver};
    use crate::x509::{generate_x509_identity, X509IdentityCertFactory, X509IdentityFactory};

    use super::QuicListener;

    static TLS_INIT: Once = Once::new();

    struct NoopConnListener;

    #[async_trait::async_trait]
    impl P2pConnectionEventListener for NoopConnListener {
        async fn on_new_connection(&self, _conn: P2pConnection) -> crate::error::P2pResult<()> {
            Ok(())
        }
    }

    struct SignalConnListener {
        tx: std::sync::Mutex<Option<oneshot::Sender<()>>>,
    }

    #[async_trait::async_trait]
    impl P2pConnectionEventListener for SignalConnListener {
        async fn on_new_connection(&self, _conn: P2pConnection) -> crate::error::P2pResult<()> {
            if let Some(tx) = self.tx.lock().unwrap().take() {
                let _ = tx.send(());
            }
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

    async fn setup_listener(
        algo: QuicCongestionAlgorithm,
        mapping_port: Option<u16>,
        out: Option<Endpoint>,
    ) -> (Arc<QuicListener>, Arc<DefaultTlsServerCertResolver>, P2pIdentityRef, Endpoint) {
        init_tls_once();
        let cert_factory = new_cert_factory();
        let cert_cache = new_cert_cache();
        let server_identity = new_identity();
        let resolver = DefaultTlsServerCertResolver::new();
        resolver
            .add_server_identity(server_identity.clone())
            .await
            .unwrap();

        let listener = QuicListener::new(cert_cache, resolver.clone(), cert_factory, algo);
        listener
            .bind(
                Endpoint::from((Protocol::Quic, "127.0.0.1:0".parse().unwrap())),
                out,
                mapping_port,
            )
            .await
            .unwrap();

        let ep = Endpoint::from((Protocol::Quic, listener.quic_ep().local_addr().unwrap()));
        (listener, resolver, server_identity, ep)
    }

    #[tokio::test]
    async fn quic_listener_state_getters_and_mapping_port_ok() {
        let out = Endpoint::from((Protocol::Quic, "127.0.0.1:45678".parse().unwrap()));
        let (listener, _resolver, _identity, _ep) =
            setup_listener(QuicCongestionAlgorithm::Bbr, Some(10001), Some(out)).await;

        assert_eq!(listener.outer(), Some(out));
        assert_eq!(listener.mapping_port(), Some(10001));

        let changed_local = Endpoint::from((Protocol::Quic, "127.0.0.1:12345".parse().unwrap()));
        listener.set_local(changed_local);
        assert_eq!(listener.local(), changed_local);

        let as_trait: &dyn P2pListener = listener.as_ref();
        assert_eq!(as_trait.local(), changed_local);
        assert_eq!(as_trait.mapping_port(), Some(10001));
    }

    #[tokio::test]
    async fn quic_listener_update_outer_states_ok() {
        let out = Endpoint::from((Protocol::Quic, "127.0.0.1:43001".parse().unwrap()));
        let (listener, _resolver, _identity, _ep) =
            setup_listener(QuicCongestionAlgorithm::Bbr, None, Some(out)).await;

        assert!(matches!(listener.update_outer(&out), UpdateOuterResult::None));

        let out2 = Endpoint::from((Protocol::Quic, "127.0.0.1:43002".parse().unwrap()));
        assert!(matches!(listener.update_outer(&out2), UpdateOuterResult::Reset));

        listener.state.write().unwrap().outer = None;
        assert!(matches!(listener.update_outer(&out), UpdateOuterResult::Update));
    }

    #[tokio::test]
    async fn quic_listener_bind_with_all_congestion_algorithms_ok() {
        let _ = setup_listener(QuicCongestionAlgorithm::Cubic, None, None).await;
        let _ = setup_listener(QuicCongestionAlgorithm::NewReno, None, None).await;
    }

    #[tokio::test]
    async fn quic_listener_start_accepts_bi_and_dispatches_event() {
        let (listener, _resolver, server_identity, server_ep) =
            setup_listener(QuicCongestionAlgorithm::Bbr, None, None).await;
        let (tx, rx) = oneshot::channel();
        listener.set_connection_event_listener(Arc::new(SignalConnListener {
            tx: std::sync::Mutex::new(Some(tx)),
        }));
        listener.start();

        let client_identity = new_identity();
        let client_ep = quinn::Endpoint::client("0.0.0.0:0".parse().unwrap()).unwrap();
        let mut client_conn = QuicConnection::connect_with_ep(
            client_ep,
            client_identity,
            new_cert_factory(),
            server_identity.get_id(),
            Some(server_identity.get_name()),
            server_ep,
            QuicCongestionAlgorithm::Bbr,
            Duration::from_secs(3),
            Duration::from_secs(10),
        )
        .await
        .unwrap();

        let _ = client_conn.open_bi_stream().await.unwrap();

        let ret = tokio::time::timeout(Duration::from_secs(2), rx).await;
        assert!(ret.is_ok());
    }

    #[tokio::test]
    async fn quic_listener_start_accepts_uni_without_dispatch() {
        let (listener, _resolver, server_identity, server_ep) =
            setup_listener(QuicCongestionAlgorithm::Bbr, None, None).await;
        listener.set_connection_event_listener(Arc::new(NoopConnListener));
        listener.start();

        let client_identity = new_identity();
        let client_ep = quinn::Endpoint::client("0.0.0.0:0".parse().unwrap()).unwrap();
        let mut client_conn = QuicConnection::connect_with_ep(
            client_ep,
            client_identity,
            new_cert_factory(),
            server_identity.get_id(),
            Some(server_identity.get_name()),
            server_ep,
            QuicCongestionAlgorithm::Bbr,
            Duration::from_secs(3),
            Duration::from_secs(10),
        )
        .await
        .unwrap();

        let _ = client_conn.open_ui().await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
