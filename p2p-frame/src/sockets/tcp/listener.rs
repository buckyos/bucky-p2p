use std::io::ErrorKind;
use std::str::FromStr;
use log::*;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use rustls::ServerConfig;
use rustls::version::TLS13;
use crate::runtime::{TcpListener, TcpStream, TlsAcceptor};
use crate::endpoint::{Endpoint, Protocol};
use crate::error::{bdt_err, BdtError, BdtErrorCode, BdtResult, into_bdt_err};
use crate::executor::Executor;
use crate::finder::DeviceCache;
use crate::p2p_identity::{P2pId, P2pIdentityCertFactoryRef};
use crate::runtime;
use crate::tls::ServerCertResolverRef;
use super::super::UpdateOuterResult;
use super::TCPSocket;

#[callback_trait::callback_trait]
pub trait TcpListenerEventListener: Send + Sync + 'static {
    async fn on_new_connection(
        &self,
        socket: TCPSocket,
    ) -> BdtResult<()>;
}

struct TCPListenerState {
    local: Option<Endpoint>,
    outer: Option<Endpoint>,
    socket: Option<Arc<TcpListener>>,
    mapping_port: Option<u16>,
}

pub struct TCPListener {
    device_cache: Arc<DeviceCache>,
    cert_resolver: ServerCertResolverRef,
    accept_timout: Duration,
    state: RwLock<TCPListenerState>,
    tcp_listener: RwLock<Option<Arc<dyn TcpListenerEventListener>>>,
    tls_acceptor: TlsAcceptor,
    cert_factory: P2pIdentityCertFactoryRef,
}
pub type TCPListenerRef = Arc<TCPListener>;

#[derive(Eq, PartialEq)]
enum BoxType {
    Package,
    RawData,
}

impl TCPListener {
    pub fn new(
        device_cache: Arc<DeviceCache>,
        cert_resolver: ServerCertResolverRef,
        cert_factory: P2pIdentityCertFactoryRef,
        accept_timout: Duration,
    ) -> Arc<Self> {
        let mut server_config =
            ServerConfig::builder_with_provider(crate::tls::provider().into())
                .with_protocol_versions(&[&TLS13])
                .unwrap()
                .with_client_cert_verifier(Arc::new(crate::tls::TlsClientCertVerifier::new(cert_factory.clone())))
                .with_cert_resolver(cert_resolver.clone());

        server_config.key_log = Arc::new(rustls::KeyLogFile::new());

        Arc::new(Self {
            device_cache,
            cert_resolver,
            accept_timout,
            state: RwLock::new(TCPListenerState {
                local: None,
                outer: None,
                socket: None,
                mapping_port: None,
            }),
            tcp_listener: RwLock::new(None),
            tls_acceptor: TlsAcceptor::from(Arc::new(server_config)),
            cert_factory,
        })
    }

    pub fn local(&self) -> Endpoint {
        self.state.read().unwrap().local.clone().unwrap()
    }

    pub fn outer(&self) -> Option<Endpoint> {
        self.state.read().unwrap().outer.clone()
    }

    pub fn set_listener(&self, listener: Arc<dyn TcpListenerEventListener>) {
        *self.tcp_listener.write().unwrap() = Some(listener);
    }

    pub fn set_local(&self, local: Endpoint) {
        self.state.write().unwrap().local = Some(local);
    }

    pub fn update_outer(&self, outer: &Endpoint) -> UpdateOuterResult {
        let mut state = self.state.write().unwrap();
        if let Some(outer_ep) = state.outer.as_ref() {
            if *outer_ep != *outer {
                info!("reset outer to {}", outer);
                state.outer = Some(*outer);
                UpdateOuterResult::Update
            } else {
                trace!("ignore update outer to {}", outer);
                UpdateOuterResult::None
            }
        } else {
            info!("update outer to {}", outer);
            state.outer = Some(*outer);
            UpdateOuterResult::Update
        }
    }

    pub fn mapping_port(&self) -> Option<u16> {
        self.state.read().unwrap().mapping_port
    }

    pub async fn bind(&self, local: Endpoint, out: Option<Endpoint>, mapping_port: Option<u16>) -> Result<(), BdtError> {
        let socket = {
            if local.addr().is_ipv6() {
                #[cfg(windows)]
                {
                    let mut default_local = Endpoint::default_tcp(&local);
                    default_local.mut_addr().set_port(local.addr().port());
                    TcpListener::bind(default_local.addr()).await
                        .map_err(into_bdt_err!(BdtErrorCode::AlreadyExists, "bind port failed"))
                }
                #[cfg(not(windows))]
                {
                    use std::os::unix::io::FromRawFd;
                    unsafe {
                        let raw_sock = libc::socket(libc::AF_INET6, libc::SOCK_STREAM, 0);
                        let yes: libc::c_int = 1;
                        libc::setsockopt(
                            raw_sock,
                            libc::IPPROTO_IPV6,
                            libc::IPV6_V6ONLY,
                            &yes as *const libc::c_int as *const libc::c_void,
                            std::mem::size_of::<libc::c_int>().try_into().unwrap(),
                        );
                        let addr = libc::sockaddr_in6 {
                            #[cfg(any(target_os = "macos", target_os = "ios"))]
                            sin6_len: 24,
                            sin6_family: libc::AF_INET6 as libc::sa_family_t,
                            sin6_port: local.addr().port().to_be(),
                            sin6_flowinfo: 0,
                            sin6_addr: libc::in6_addr { s6_addr: [0u8; 16] },
                            sin6_scope_id: 0,
                        };
                        if libc::bind(
                            raw_sock,
                            &addr as *const libc::sockaddr_in6 as *const libc::sockaddr,
                            std::mem::size_of::<libc::sockaddr_in6>()
                                .try_into()
                                .unwrap(),
                        ) < 0
                        {
                            Err(BdtError::new(
                                BdtErrorCode::AlreadyExists,
                                "bind port failed".to_string(),
                            ))
                        } else {
                            #[cfg(feature = "runtime-tokio")]
                            {
                                Ok(TcpListener::from_std(std::net::TcpListener::from_raw_fd(raw_sock)).unwrap())
                            }

                            #[cfg(feature = "runtime-async-std")]
                            {
                                Ok(TcpListener::from_raw_fd(raw_sock))
                            }
                        }
                    }
                }
            } else if local.is_sys_default() {
                let mut default_local = Endpoint::default_tcp(&local);
                default_local.mut_addr().set_port(local.addr().port());
                TcpListener::bind(default_local.addr()).await.map_err(into_bdt_err!(BdtErrorCode::AlreadyExists, "bind port failed"))
            } else {
                TcpListener::bind(local.addr()).await.map_err(into_bdt_err!(BdtErrorCode::AlreadyExists, "bind port failed"))
            }
        }?;

        let mut state = self.state.write().unwrap();
        state.local = Some(local.clone());
        state.outer = out;
        state.socket = Some(Arc::new(socket));
        state.mapping_port = mapping_port;

        Ok(())
    }

    async fn accept(&self, socket: TcpStream) -> Result<TCPSocket, BdtError> {
        let remote = socket.peer_addr().map_err(into_bdt_err!(BdtErrorCode::Failed))?;
        let local = socket.local_addr().map_err(into_bdt_err!(BdtErrorCode::Failed))?;
        let remote = Endpoint::from((Protocol::Tcp, remote));
        let local = Endpoint::from((Protocol::Tcp, local));

        let tls_stream = self.tls_acceptor.accept(socket).await.map_err(into_bdt_err!(BdtErrorCode::ConnectFailed))?;
        let (_, tls_conn) = tls_stream.get_ref();
        let cert = tls_conn.peer_certificates();
        if cert.is_none() {
            return Err(bdt_err!(BdtErrorCode::CertError, "no cert"));
        }
        let cert = cert.unwrap();
        if cert.len() == 0 {
            return Err(bdt_err!(BdtErrorCode::CertError, "no cert"));
        }

        let local_identity_id = P2pId::from_str(tls_conn.server_name().unwrap()).map_err(into_bdt_err!(BdtErrorCode::TlsError, "decode cert failed."))?;
        let remote_device = self.cert_factory.create(&cert[0].as_ref().to_vec())?;
        let remote_id = remote_device.get_id();
        self.device_cache.add(&remote_id, &remote_device);
        Ok(TCPSocket::new(runtime::TlsStream::from(tls_stream), local_identity_id, remote_id, local, remote))
    }

    pub fn start(self: &Arc<Self>) {
        let this = self.clone();
        let socket: Arc<TcpListener> = self.state.read().unwrap().socket.clone().unwrap();
        let tcp_listener = self.tcp_listener.read().unwrap().as_ref().unwrap().clone();
        Executor::spawn(async move {
            loop {
                match socket.accept().await {
                    Ok((socket, _from_addr)) => {
                        let tcp_listener = tcp_listener.clone();
                        let this = this.clone();
                        Executor::spawn(async move {
                            match this.accept(socket).await {
                                Ok(socket) => {
                                    if let Err(e) = tcp_listener.on_new_connection(socket).await {
                                        error!("tcp-listener accept error({}).", e);
                                    }
                                }
                                Err(e) => {
                                    warn!("tcp-listener accept a stream, but the first package read failed. err: {}", e);
                                }
                            }
                        });
                    }
                    Err(e) => match e.kind() {
                        ErrorKind::Interrupted
                        | ErrorKind::WouldBlock
                        | ErrorKind::AlreadyExists
                        | ErrorKind::TimedOut => continue,
                        _ => {
                            warn!("tcp-listener accept fatal error({}). will stop.", e);
                            break;
                        }
                    },
                }
            }
        });
    }

    pub async fn reset(self: &Arc<Self>, local: &Endpoint) -> Arc<Self> {
        let mut new = self.state.write().unwrap();
        new.local = Some(local.clone());
        new.outer = None;
        self.clone()
    }

    pub fn close(&self) {
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawSocket;
            use winapi::um::winsock2::closesocket;
            unsafe {
                let raw = self.state.read().unwrap().socket.as_ref().unwrap().as_raw_socket();
                closesocket(raw.try_into().unwrap());
            }
        }
        #[cfg(not(windows))]
        {
            #[cfg(feature = "runtime-async-std")]
            {
                use std::os::unix::io::AsRawFd;
                unsafe {
                    let raw = self.state.read().unwrap().socket.as_ref().unwrap().as_raw_socket();
                    libc::close(raw);
                }
            }
            #[cfg(feature = "runtime-tokio")]
            {
                use std::os::fd::AsRawFd;
                unsafe {
                    let raw = self.state.read().unwrap().socket.as_ref().unwrap().as_raw_fd();
                    libc::close(raw.try_into().unwrap());
                }
            }
        }
    }
}
