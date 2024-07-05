use std::str::FromStr;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use as_any::{AsAny, Downcast};
use bucky_objects::{Device, DeviceId, Endpoint, NamedObject, Protocol};
use bucky_raw_codec::{RawConvertTo, RawFrom};
use quinn::Incoming;
use quinn::crypto::rustls::{HandshakeData, QuicServerConfig};
use quinn::ServerConfig;
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
use rustls::version::TLS13;
use tokio::net::TcpListener;
use bucky_rustls::ServerCertResolverRef;
use crate::error::{bdt_err, BdtErrorCode, BdtResult, into_bdt_err};
use crate::executor::Executor;
use crate::finder::DeviceCache;
use crate::LocalDeviceRef;
use crate::sockets::quic::quic_socket::QuicSocket;
use crate::sockets::tcp::{TcpListenerEventListener, TCPSocket};
use crate::sockets::UpdateOuterResult;

#[callback_trait::callback_trait]
pub trait QuicListenerEventListener: Send + Sync + 'static {
    async fn on_new_connection(
        &self,
        socket: QuicSocket,
    ) -> BdtResult<()>;
}

struct QuicListenerState {
    local: Option<Endpoint>,
    outer: Option<Endpoint>,
    socket: Option<quinn::Endpoint>,
    mapping_port: Option<u16>,
}

pub struct QuicListener {
    device_cache: Arc<DeviceCache>,
    cert_resolver: ServerCertResolverRef,
    accept_timout: Duration,
    state: RwLock<QuicListenerState>,
    quic_listener: RwLock<Option<Arc<dyn QuicListenerEventListener>>>,
}
pub type QuicListenerRef = Arc<QuicListener>;

impl QuicListener {
    pub fn new(
        device_cache: Arc<DeviceCache>,
        cert_resolver: ServerCertResolverRef,
        accept_timout: Duration
    ) -> Arc<Self> {
        Arc::new(Self {
            device_cache,
            cert_resolver,
            accept_timout,
            state: RwLock::new(QuicListenerState {
                local: None,
                outer: None,
                socket: None,
                mapping_port: None,
            }),
            quic_listener: RwLock::new(None),
        })
    }

    pub fn local(&self) -> Endpoint {
        self.state.read().unwrap().local.clone().unwrap()
    }

    pub fn outer(&self) -> Option<Endpoint> {
        self.state.read().unwrap().outer.clone()
    }

    pub fn set_listener(&self, listener: Arc<dyn QuicListenerEventListener>) {
        *self.quic_listener.write().unwrap() = Some(listener);
    }

    pub fn set_local(&self, local: Endpoint) {
        self.state.write().unwrap().local = Some(local);
    }

    pub fn quic_ep(&self) -> quinn::Endpoint {
        self.state.read().unwrap().socket.clone().unwrap()
    }

    pub async fn reset(&self, new_ep: &Endpoint) -> Arc<Self> {
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


    pub async fn bind(&self, local: Endpoint, out: Option<Endpoint>, mapping_port: Option<u16>) -> BdtResult<()> {
        let mut server_config =
            rustls::ServerConfig::builder_with_provider(bucky_rustls::provider().into())
                .with_protocol_versions(&[&TLS13])
                .map_err(into_bdt_err!(BdtErrorCode::TlsError, "Create server config error"))?
                .with_client_cert_verifier(Arc::new(bucky_rustls::BuckyClientCertVerifier::new()))
                .with_cert_resolver(self.cert_resolver.clone());
        server_config.key_log = Arc::new(rustls::KeyLogFile::new());

        let mut server_config =
            quinn::ServerConfig::with_crypto(Arc::new(QuicServerConfig::try_from(server_config).map_err(into_bdt_err!(BdtErrorCode::TlsError, "create quic server config failed"))?));
        let transport_config = Arc::get_mut(&mut server_config.transport).unwrap();
        transport_config.max_idle_timeout(Some(std::time::Duration::from_secs(600).try_into().unwrap()));

        let endpoint = quinn::Endpoint::server(server_config, local.addr().clone()).map_err(into_bdt_err!(BdtErrorCode::QuicError, "Create quic server error"))?;

        let mut state = self.state.write().unwrap();
        state.local = Some(local.clone());
        state.outer = out;
        state.socket = Some(endpoint);
        state.mapping_port = mapping_port;

        Ok(())
    }

    async fn accept(&self, conn: Incoming) -> BdtResult<QuicSocket> {
        let connection = conn.await.map_err(into_bdt_err!(BdtErrorCode::QuicError, "QuicListener accept error"))?;
        let handshake_data = connection.handshake_data();
        if handshake_data.is_none() {
            return Err(bdt_err!(BdtErrorCode::TlsError, "no handshake data"));
        }
        let handshake_data = handshake_data.as_ref().unwrap().as_ref().downcast_ref::<HandshakeData>();
        if handshake_data.is_none() {
            return Err(bdt_err!(BdtErrorCode::TlsError, "no handshake data"));
        }

        let serve_name = handshake_data.unwrap().server_name.as_ref();
        if serve_name.is_none() {
            return Err(bdt_err!(BdtErrorCode::TlsError, "no server name"));
        }

        let local_id = DeviceId::from_str(serve_name.unwrap()).map_err(into_bdt_err!(BdtErrorCode::TlsError, "parse device id error"))?;
        let peer_identity = connection.peer_identity();
        let remote_cert = peer_identity.as_ref().unwrap().as_ref().downcast_ref::<Vec<CertificateDer>>();
        if remote_cert.is_none() || remote_cert.as_ref().unwrap().len() == 0 {
            return Err(bdt_err!(BdtErrorCode::CertError, "no cert"));
        }
        let remote_device = Device::clone_from_slice(remote_cert.unwrap()[0].as_ref())
            .map_err(into_bdt_err!(BdtErrorCode::CertError, "parse cert error"))?;
        self.device_cache.add(&remote_device.desc().device_id(), &remote_device);
        let remote_addr = connection.remote_address();
        let socket = QuicSocket::new(connection,
                                     local_id,
                                     remote_device.desc().device_id(),
                                     self.local(),
                                     Endpoint::from((Protocol::Udp, remote_addr)));
        Ok(socket)
    }

    pub fn start(self: &Arc<Self>) {
        let this = self.clone();
        let socket = self.state.read().unwrap().socket.clone().unwrap();
        let quic_listener = self.quic_listener.read().unwrap().as_ref().unwrap().clone();
        Executor::spawn(async move {
            loop {
                match socket.accept().await {
                    None => {
                        error!("QuicListener accept error");
                        break;
                    }
                    Some(conn) => {
                        let this = this.clone();
                        let quic_listener = quic_listener.clone();
                        Executor::spawn(async move {
                            match this.accept(conn).await {
                                Ok(socket) => {
                                    if let Err(e) = quic_listener.on_new_connection(socket).await {
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
