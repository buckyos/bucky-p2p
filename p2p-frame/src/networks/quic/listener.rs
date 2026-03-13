use super::tunnel::QuicTunnel;
use crate::endpoint::{Endpoint, Protocol};
use crate::error::{P2pErrorCode, P2pResult, into_p2p_err, p2p_err};
use crate::executor::{Executor, SpawnHandle};
use crate::finder::DeviceCache;
use crate::networks::{QuicCongestionAlgorithm, TunnelForm, TunnelListener, TunnelRef};
use crate::p2p_identity::{
    P2pId, P2pIdentityCertCacheRef, P2pIdentityCertFactoryRef, P2pIdentityRef,
};
use crate::runtime;
use crate::tls::{ServerCertResolverRef, TlsServerCertResolver};
use quinn::Incoming;
use quinn::crypto::rustls::{HandshakeData, QuicServerConfig};
use rustls::pki_types::CertificateDer;
use rustls::version::TLS13;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use tokio::sync::Mutex as AsyncMutex;
use tokio::sync::{Notify, mpsc};

struct QuicTunnelListenerState {
    local: Option<Endpoint>,
    outer: Option<Endpoint>,
    socket: Option<quinn::Endpoint>,
    mapping_port: Option<u16>,
}

pub(crate) struct QuicTunnelListener {
    cert_cache: P2pIdentityCertCacheRef,
    cert_resolver: ServerCertResolverRef,
    cert_factory: P2pIdentityCertFactoryRef,
    congestion_algorithm: QuicCongestionAlgorithm,
    state: RwLock<QuicTunnelListenerState>,
    accepted: AsyncMutex<mpsc::UnboundedReceiver<P2pResult<TunnelRef>>>,
    accepted_tx: Mutex<Option<mpsc::UnboundedSender<P2pResult<TunnelRef>>>>,
    accept_task: Mutex<Option<SpawnHandle<()>>>,
    close_notify: Notify,
    closed: AtomicBool,
}

impl QuicTunnelListener {
    pub(crate) fn new(
        cert_cache: P2pIdentityCertCacheRef,
        cert_resolver: ServerCertResolverRef,
        cert_factory: P2pIdentityCertFactoryRef,
        congestion_algorithm: QuicCongestionAlgorithm,
    ) -> Arc<Self> {
        let (accepted_tx, accepted_rx) = mpsc::unbounded_channel();
        Arc::new(Self {
            cert_cache,
            cert_resolver,
            cert_factory,
            congestion_algorithm,
            state: RwLock::new(QuicTunnelListenerState {
                local: None,
                outer: None,
                socket: None,
                mapping_port: None,
            }),
            accepted: AsyncMutex::new(accepted_rx),
            accepted_tx: Mutex::new(Some(accepted_tx)),
            accept_task: Mutex::new(None),
            close_notify: Notify::new(),
            closed: AtomicBool::new(false),
        })
    }

    pub(crate) fn local(&self) -> Endpoint {
        self.state.read().unwrap().local.unwrap()
    }

    pub(crate) fn bound_local(&self) -> Endpoint {
        let endpoint = self.state.read().unwrap().socket.clone().unwrap();
        Endpoint::from((
            crate::endpoint::Protocol::Quic,
            endpoint.local_addr().unwrap(),
        ))
    }

    pub(crate) fn mapping_port(&self) -> Option<u16> {
        self.state.read().unwrap().mapping_port
    }

    pub(crate) fn quic_ep(&self) -> quinn::Endpoint {
        self.state.read().unwrap().socket.clone().unwrap()
    }

    pub(crate) fn close(&self) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        self.close_notify.notify_waiters();
        self.accepted_tx.lock().unwrap().take();
        if let Some(task) = self.accept_task.lock().unwrap().take() {
            task.abort();
        }
        let ep = {
            let mut state = self.state.write().unwrap();
            state.socket.take()
        };
        if let Some(ep) = ep {
            ep.close(0_u32.into(), b"close all listeners");
        }
    }

    pub(crate) async fn bind(
        &self,
        local: Endpoint,
        out: Option<Endpoint>,
        mapping_port: Option<u16>,
    ) -> P2pResult<()> {
        let mut server_config =
            rustls::ServerConfig::builder_with_provider(crate::tls::provider().into())
                .with_protocol_versions(&[&TLS13])
                .map_err(into_p2p_err!(
                    P2pErrorCode::TlsError,
                    "Create server config error"
                ))?
                .with_client_cert_verifier(Arc::new(crate::tls::TlsClientCertVerifier::new(
                    self.cert_factory.clone(),
                )))
                .with_cert_resolver(self.cert_resolver.clone().get_resolves_server_cert());
        server_config.key_log = Arc::new(rustls::KeyLogFile::new());

        let mut server_config = quinn::ServerConfig::with_crypto(Arc::new(
            QuicServerConfig::try_from(server_config).map_err(into_p2p_err!(
                P2pErrorCode::TlsError,
                "create quic server config failed"
            ))?,
        ));
        let transport_config = Arc::get_mut(&mut server_config.transport).unwrap();
        transport_config.max_idle_timeout(Some(
            std::time::Duration::from_secs(600).try_into().unwrap(),
        ));
        match self.congestion_algorithm {
            QuicCongestionAlgorithm::Bbr => {
                transport_config.congestion_controller_factory(Arc::new(
                    quinn::congestion::BbrConfig::default(),
                ));
            }
            QuicCongestionAlgorithm::Cubic => {
                transport_config.congestion_controller_factory(Arc::new(
                    quinn::congestion::CubicConfig::default(),
                ));
            }
            QuicCongestionAlgorithm::NewReno => {
                transport_config.congestion_controller_factory(Arc::new(
                    quinn::congestion::NewRenoConfig::default(),
                ));
            }
        }

        let endpoint = quinn::Endpoint::server(server_config, local.addr().to_owned()).map_err(
            into_p2p_err!(P2pErrorCode::QuicError, "Create quic server error"),
        )?;

        let mut state = self.state.write().unwrap();
        state.local = Some(local);
        state.outer = out;
        state.socket = Some(endpoint);
        state.mapping_port = mapping_port;

        Ok(())
    }

    async fn accept_connection(&self, conn: Incoming) -> P2pResult<TunnelRef> {
        let connection = conn.await.map_err(into_p2p_err!(
            P2pErrorCode::QuicError,
            "QuicTunnelListener accept error"
        ))?;
        let server_name = {
            let handshake_data = connection
                .handshake_data()
                .ok_or_else(|| p2p_err!(P2pErrorCode::TlsError, "no handshake data"))?;
            let handshake_data = handshake_data
                .as_ref()
                .downcast_ref::<HandshakeData>()
                .ok_or_else(|| p2p_err!(P2pErrorCode::TlsError, "no handshake data"))?;
            let server_name = handshake_data
                .server_name
                .as_ref()
                .ok_or_else(|| p2p_err!(P2pErrorCode::TlsError, "no server name"))?;
            parse_server_name(server_name).to_owned()
        };

        let remote_cert = {
            let peer_identity = connection
                .peer_identity()
                .ok_or_else(|| p2p_err!(P2pErrorCode::CertError, "no peer identity"))?;
            let remote_cert = peer_identity
                .as_ref()
                .downcast_ref::<Vec<CertificateDer>>()
                .ok_or_else(|| p2p_err!(P2pErrorCode::CertError, "peer cert type invalid"))?;
            if remote_cert.is_empty() {
                return Err(p2p_err!(P2pErrorCode::CertError, "no cert"));
            }
            remote_cert[0].as_ref().to_vec()
        };

        let local_identity = self
            .cert_resolver
            .get_server_identity(server_name.as_str())
            .await
            .ok_or_else(|| p2p_err!(P2pErrorCode::CertError, "no local cert"))?;
        let remote_identity = self.cert_factory.create(&remote_cert)?;
        self.cert_cache
            .add(&remote_identity.get_id(), &remote_identity)
            .await?;

        let remote_addr = connection.remote_address();
        Ok(QuicTunnel::accept(
            connection,
            local_identity.get_id(),
            remote_identity.get_id(),
            self.local(),
            Endpoint::from((Protocol::Quic, remote_addr)),
        )
        .await?)
    }

    pub(crate) fn start(self: &Arc<Self>) {
        if self.closed.load(Ordering::SeqCst) {
            return;
        }
        let this = self.clone();
        let socket = self.quic_ep();
        let handle = Executor::spawn_with_handle(async move {
            loop {
                match socket.accept().await {
                    Some(conn) => {
                        let result = this.accept_connection(conn).await;
                        let sender = this.accepted_tx.lock().unwrap().clone();
                        let Some(tx) = sender else {
                            break;
                        };
                        if tx.send(result).is_err() {
                            break;
                        }
                    }
                    None => break,
                }
            }
        })
        .unwrap();
        *self.accept_task.lock().unwrap() = Some(handle);
    }
}

#[async_trait::async_trait]
impl TunnelListener for QuicTunnelListener {
    async fn accept_tunnel(&self) -> P2pResult<TunnelRef> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(p2p_err!(P2pErrorCode::ErrorState, "tunnel listener closed"));
        }
        let mut accepted = self.accepted.lock().await;
        let closed = self.close_notify.notified();
        tokio::pin!(closed);
        tokio::select! {
            result = accepted.recv() => {
                match result {
                    Some(result) => result,
                    None => Err(p2p_err!(P2pErrorCode::ErrorState, "tunnel listener closed")),
                }
            }
            _ = &mut closed => {
                Err(p2p_err!(P2pErrorCode::ErrorState, "tunnel listener closed"))
            }
        }
    }
}

fn validate_server_name(server_name: String) -> String {
    match rustls::pki_types::ServerName::try_from(server_name.as_str()) {
        Ok(_) => server_name,
        Err(_) => format!("p2p.{}.com", server_name),
    }
}

fn parse_server_name(server_name: &str) -> &str {
    if server_name.starts_with("p2p.") && server_name.ends_with(".com") {
        server_name
            .trim_start_matches("p2p.")
            .trim_end_matches(".com")
    } else {
        server_name
    }
}

pub(crate) async fn connect_with_ep(
    ep: quinn::Endpoint,
    local_identity_ref: P2pIdentityRef,
    cert_factory: P2pIdentityCertFactoryRef,
    remote_identity_id: P2pId,
    remote_name: Option<String>,
    remote: Endpoint,
    congestion_algorithm: QuicCongestionAlgorithm,
    timeout: std::time::Duration,
    idle_timeout: std::time::Duration,
) -> P2pResult<quinn::Connection> {
    let client_key = local_identity_ref.get_encoded_identity()?;
    let client_cert = local_identity_ref.get_identity_cert()?.get_encoded_cert()?;
    let mut config = rustls::ClientConfig::builder_with_provider(crate::tls::provider().into())
        .with_protocol_versions(&[&TLS13])
        .unwrap()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(crate::tls::TlsServerCertVerifier::new(
            cert_factory,
            remote_identity_id.clone(),
        )))
        .with_client_auth_cert(
            vec![CertificateDer::from(client_cert)],
            rustls::pki_types::PrivatePkcs8KeyDer::from(client_key).into(),
        )
        .map_err(into_p2p_err!(P2pErrorCode::TlsError))?;
    config.enable_early_data = true;

    let mut client_config = quinn::ClientConfig::new(Arc::new(
        quinn::crypto::rustls::QuicClientConfig::try_from(config).unwrap(),
    ));
    let mut transport_config = quinn::TransportConfig::default();
    transport_config.max_idle_timeout(Some(idle_timeout.try_into().unwrap()));
    if idle_timeout > std::time::Duration::from_secs(15) {
        transport_config.keep_alive_interval(Some(std::time::Duration::from_secs(15)));
    }
    match congestion_algorithm {
        QuicCongestionAlgorithm::Bbr => {
            transport_config
                .congestion_controller_factory(Arc::new(quinn::congestion::BbrConfig::default()));
        }
        QuicCongestionAlgorithm::Cubic => {
            transport_config
                .congestion_controller_factory(Arc::new(quinn::congestion::CubicConfig::default()));
        }
        QuicCongestionAlgorithm::NewReno => {
            transport_config.congestion_controller_factory(Arc::new(
                quinn::congestion::NewRenoConfig::default(),
            ));
        }
    }
    client_config.transport_config(Arc::new(transport_config));

    let remote_name = remote_name.unwrap_or(remote_identity_id.to_string());
    let remote_name = validate_server_name(remote_name);
    runtime::timeout(
        timeout,
        ep.connect_with(
            client_config,
            remote.addr().to_owned(),
            remote_name.as_str(),
        )
        .unwrap(),
    )
    .await
    .map_err(into_p2p_err!(
        P2pErrorCode::ConnectFailed,
        "quic to {} connect failed",
        remote
    ))?
    .map_err(into_p2p_err!(
        P2pErrorCode::ConnectFailed,
        "quic to {} connect failed",
        remote
    ))
}
