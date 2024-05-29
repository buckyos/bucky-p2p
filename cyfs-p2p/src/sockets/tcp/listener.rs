use std::io::ErrorKind;
use std::net::Shutdown;
use log::*;
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::Duration;
use async_std::net::{TcpListener, TcpStream};
use async_std::{future, task};
use async_std::io::ReadExt;
use cyfs_base::{BuckyError, BuckyErrorCode, DeviceId, Endpoint, endpoint, RawDecode, RawDecodeWithContext, RawFixedBytes};
use crate::error::BdtResult;
use crate::executor::Executor;
use crate::history::keystore::Keystore;
use crate::protocol::{Exchange, FirstBoxTcpDecodeContext, MTU_LARGE, PackageBox, PackageBoxDecodeContext, PackageCmdCode};
use crate::types::LocalDeviceRef;
use super::super::UpdateOuterResult;
use super::TCPSocket;

#[callback_trait::callback_trait]
pub trait TcpListenerEventListener: Send + Sync + 'static {
    async fn on_new_connection(
        &self,
        socket: Arc<TCPSocket>,
        first_box: PackageBox,
    ) -> BdtResult<()>;
}

struct TCPListenerState {
    local: Option<Endpoint>,
    outer: Option<Endpoint>,
    socket: Option<Arc<TcpListener>>,
    mapping_port: Option<u16>,
}

pub(crate) struct TCPListener {
    key_store: Arc<Keystore>,
    accept_timout: Duration,
    state: RwLock<TCPListenerState>,
    tcp_listener: RwLock<Option<Arc<dyn TcpListenerEventListener>>>,
}
pub(crate) type TCPListenerRef = Arc<TCPListener>;

#[derive(Eq, PartialEq)]
enum BoxType {
    Package,
    RawData,
}

impl TCPListener {
    pub fn new(
        key_store: Arc<Keystore>,
        accept_timout: Duration,
    ) -> Arc<Self> {
        Arc::new(Self {
            key_store,
            accept_timout,
            state: RwLock::new(TCPListenerState {
                local: None,
                outer: None,
                socket: None,
                mapping_port: None,
            }),
            tcp_listener: RwLock::new(None),
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

    pub async fn bind(&self, local: Endpoint, out: Option<Endpoint>, mapping_port: Option<u16>) -> Result<(), BuckyError> {
        let socket = {
            if local.addr().is_ipv6() {
                #[cfg(windows)]
                {
                    let mut default_local = Endpoint::default_tcp(&local);
                    default_local.mut_addr().set_port(local.addr().port());
                    TcpListener::bind(default_local.addr()).await.map_err(|err| BuckyError::from(err))
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
                            Err(BuckyError::from((
                                BuckyErrorCode::AlreadyExists,
                                "bind port failed",
                            )))
                        } else {
                            Ok(TcpListener::from_raw_fd(raw_sock))
                        }
                    }
                }
            } else if local.is_sys_default() {
                let mut default_local = Endpoint::default_tcp(&local);
                default_local.mut_addr().set_port(local.addr().port());
                TcpListener::bind(default_local.addr()).await.map_err(|err| BuckyError::from(err))
            } else {
                TcpListener::bind(local.addr()).await.map_err(|err| BuckyError::from(err))
            }
        }?;

        let mut state = self.state.write().unwrap();
        state.local = Some(local.clone());
        state.outer = out;
        state.socket = Some(Arc::new(socket));
        state.mapping_port = mapping_port;

        Ok(())
    }

    async fn receive_box<'a>(
        socket: &TcpStream,
        recv_buf: &'a mut [u8],
    ) -> Result<(BoxType, &'a mut [u8]), BuckyError> {
        let mut socket = socket.clone();
        let header_len = u16::raw_bytes().unwrap();
        let box_header = &mut recv_buf[..header_len];
        socket.read_exact(box_header).await?;
        let mut box_len = u16::raw_decode(box_header).map(|(v, _)| v as usize)?;
        let box_type = if box_len > 32768 {
            box_len -= 32768;
            BoxType::RawData
        } else {
            BoxType::Package
        };
        if box_len + header_len > recv_buf.len() {
            return Err(BuckyError::new(
                BuckyErrorCode::OutOfLimit,
                "buffer not enough",
            ));
        }
        let box_buf = &mut recv_buf[header_len..(header_len + box_len)];
        socket.read_exact(box_buf).await?;
        Ok((box_type, box_buf))
    }

    async fn accept(&self, socket: TcpStream) -> Result<(Arc<TCPSocket>, PackageBox), BuckyError> {
        let remote = socket.peer_addr().map_err(|e| BuckyError::from(e))?;
        let local = socket.local_addr().map_err(|e| BuckyError::from(e))?;
        let remote = Endpoint::from((endpoint::Protocol::Tcp, remote));
        let local = Endpoint::from((endpoint::Protocol::Tcp, local));

        let mut recv_buf = [0u8; MTU_LARGE];
        let (box_type, box_buf) =
            future::timeout(self.accept_timout, Self::receive_box(&socket, &mut recv_buf)).await??;
        if box_type != BoxType::Package {
            let msg = format!("recv first box raw data from {}", remote);
            error!("{}", msg);
            return Err(BuckyError::new(BuckyErrorCode::InvalidData, msg));
        }
        let first_box = {
            let context =
                FirstBoxTcpDecodeContext::new_inplace(box_buf.as_mut_ptr(), box_buf.len(), self.key_store.as_ref());
            PackageBox::raw_decode_with_context(box_buf, context)
                .map(|(package_box, _)| package_box)?
        };

        let exchg = match first_box.packages().get(0) {
            Some(first_pkg) => match first_pkg.cmd_code() {
                PackageCmdCode::Exchange => first_pkg.as_any().downcast_ref::<Exchange>(),
                _ => None,
            },
            None => return Err(BuckyError::new(BuckyErrorCode::InvalidData, "no package")),
        };
        if let Some(exchg) = exchg {
            if !exchg.verify(first_box.local()).await {
                warn!("tcp exchg verify failed.");
                return Err(BuckyError::new(
                    BuckyErrorCode::InvalidData,
                    "sign-verify failed",
                ));
            }
        }
        info!("recv key {}", first_box.key());
        Ok((Arc::new(TCPSocket::new(socket, first_box.local().clone(), first_box.remote().clone(), local, remote, first_box.key().clone())), first_box))
    }

    pub fn start(self: &Arc<Self>) {
        let this = self.clone();
        let socket = self.state.read().unwrap().socket.clone().unwrap();
        let tcp_listener = self.tcp_listener.read().unwrap().as_ref().unwrap().clone();
        thread::spawn(move || {
            Executor::block_on(async move {
                loop {
                    match socket.accept().await {
                        Ok((socket, _from_addr)) => {
                            let tcp_listener = tcp_listener.clone();
                            let this = this.clone();
                            Executor::spawn(async move {
                                match this.accept(socket.clone()).await {
                                    Ok((socket, first_box)) => {
                                        if let Err(e) = tcp_listener.on_new_connection(socket, first_box).await {
                                            error!("tcp-listener accept error({}).", e);
                                        }
                                    }
                                    Err(e) => {
                                        warn!("tcp-listener accept a stream, but the first package read failed. err: {}", e);
                                        let _ = socket.shutdown(Shutdown::Both);
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
            use std::os::unix::io::AsRawFd;
            unsafe {
                let raw = self.state.read().unwrap().socket.as_ref().unwrap().as_raw_socket();
                libc::close(raw);
            }
        }
    }
}
