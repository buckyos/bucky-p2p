use std::str::FromStr;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use quinn::Incoming;
use quinn::crypto::rustls::{HandshakeData, QuicServerConfig};
use rustls::pki_types::{CertificateDer};
use rustls::version::TLS13;
use crate::error::{p2p_err, P2pErrorCode, P2pResult, into_p2p_err};
use crate::endpoint::{Endpoint, Protocol};
use crate::executor::Executor;
use crate::finder::DeviceCache;
use crate::p2p_connection::{P2pConnectionEventListener, P2pListener};
use crate::p2p_identity::{P2pId, P2pIdentityCertFactoryRef};
use crate::sockets::{QuicConnection, UpdateOuterResult};
use crate::tls::ServerCertResolverRef;

struct QuicListenerState {
    local: Option<Endpoint>,
    outer: Option<Endpoint>,
    socket: Option<quinn::Endpoint>,
    mapping_port: Option<u16>,
}

pub struct QuicListener {
    device_cache: Arc<DeviceCache>,
    cert_resolver: ServerCertResolverRef,
    state: RwLock<QuicListenerState>,
    quic_listener: RwLock<Option<Arc<dyn P2pConnectionEventListener>>>,
    cert_factory: P2pIdentityCertFactoryRef,
}
pub type QuicListenerRef = Arc<QuicListener>;

impl QuicListener {
    pub fn new(
        device_cache: Arc<DeviceCache>,
        cert_resolver: ServerCertResolverRef,
        cert_factory: P2pIdentityCertFactoryRef,
    ) -> Arc<Self> {
        Arc::new(Self {
            device_cache,
            cert_resolver,
            state: RwLock::new(QuicListenerState {
                local: None,
                outer: None,
                socket: None,
                mapping_port: None,
            }),
            quic_listener: RwLock::new(None),
            cert_factory,
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
        let mut server_config =
            rustls::ServerConfig::builder_with_provider(crate::tls::provider().into())
                .with_protocol_versions(&[&TLS13])
                .map_err(into_p2p_err!(P2pErrorCode::TlsError, "Create server config error"))?
                .with_client_cert_verifier(Arc::new(crate::tls::TlsClientCertVerifier::new(self.cert_factory.clone())))
                .with_cert_resolver(self.cert_resolver.clone());
        server_config.key_log = Arc::new(rustls::KeyLogFile::new());

        let mut server_config =
            quinn::ServerConfig::with_crypto(Arc::new(QuicServerConfig::try_from(server_config).map_err(into_p2p_err!(P2pErrorCode::TlsError, "create quic server config failed"))?));
        let transport_config = Arc::get_mut(&mut server_config.transport).unwrap();
        transport_config.max_idle_timeout(Some(std::time::Duration::from_secs(600).try_into().unwrap()));

        let endpoint = quinn::Endpoint::server(server_config, local.addr().clone()).map_err(into_p2p_err!(P2pErrorCode::QuicError, "Create quic server error"))?;

        let mut state = self.state.write().unwrap();
        state.local = Some(local.clone());
        state.outer = out;
        state.socket = Some(endpoint);
        state.mapping_port = mapping_port;

        Ok(())
    }

    async fn accept(&self, conn: Incoming) -> P2pResult<QuicConnection> {
        let connection = conn.await.map_err(into_p2p_err!(P2pErrorCode::QuicError, "QuicListener accept error"))?;
        let (local_id, remote_cert) = {
            let handshake_data = connection.handshake_data();
            if handshake_data.is_none() {
                return Err(p2p_err!(P2pErrorCode::TlsError, "no handshake data"));
            }
            let handshake_data = handshake_data.as_ref().unwrap().as_ref().downcast_ref::<HandshakeData>();
            if handshake_data.is_none() {
                return Err(p2p_err!(P2pErrorCode::TlsError, "no handshake data"));
            }

            let serve_name = handshake_data.unwrap().server_name.as_ref();
            if serve_name.is_none() {
                return Err(p2p_err!(P2pErrorCode::TlsError, "no server name"));
            }
            let peer_identity = connection.peer_identity();
            let remote_cert = peer_identity.as_ref().unwrap().as_ref().downcast_ref::<Vec<CertificateDer>>();
            if remote_cert.is_none() || remote_cert.as_ref().unwrap().len() == 0 {
                return Err(p2p_err!(P2pErrorCode::CertError, "no cert"));
            }

            (P2pId::from_str(serve_name.unwrap())?, remote_cert.unwrap()[0].as_ref().to_vec())
        };

        let remote_device = self.cert_factory.create(&remote_cert)?;
        self.device_cache.add(&remote_device.get_id(), &remote_device);
        let remote_addr = connection.remote_address();
        let mut socket = QuicConnection::new(connection,
                                         local_id,
                                         remote_device.get_id(),
                                         self.local(),
                                         Endpoint::from((Protocol::Quic, remote_addr)));
        socket.accept().await?;
        Ok(socket)
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
                                    if let Err(e) = quic_listener.on_new_connection(Arc::new(socket)).await {
                                        error!("QuicListener on_new_connection error: {}", e);
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
}

impl P2pListener for QuicListener {
    fn set_connection_event_listener(&self, event: impl P2pConnectionEventListener) {
        *self.quic_listener.write().unwrap() = Some(Arc::new(event));
    }
}
