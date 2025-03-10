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
use crate::sockets::{QuicConnection, UpdateOuterResult};
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
}
pub type QuicListenerRef = Arc<QuicListener>;

impl QuicListener {
    pub fn new(
        cert_cache: P2pIdentityCertCacheRef,
        cert_resolver: ServerCertResolverRef,
        cert_factory: P2pIdentityCertFactoryRef,
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

            let serve_name = handshake_data.unwrap().server_name.as_ref();
            if serve_name.is_none() {
                return Err(p2p_err!(P2pErrorCode::TlsError, "no server name"));
            }
            let peer_identity = connection.peer_identity();
            let remote_cert = peer_identity.as_ref().unwrap().as_ref().downcast_ref::<Vec<CertificateDer>>();
            if remote_cert.is_none() || remote_cert.as_ref().unwrap().len() == 0 {
                return Err(p2p_err!(P2pErrorCode::CertError, "no cert"));
            }

            (serve_name.unwrap().to_string(), remote_cert.unwrap()[0].as_ref().to_vec())
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
