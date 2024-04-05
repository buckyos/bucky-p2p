use std::sync::RwLock;
use async_std::net::UdpSocket;
use cyfs_base::{BuckyError, BuckyErrorCode, Endpoint, Protocol, RawEncodeWithContext};
use socket2::{Socket, Domain, Type};
use crate::protocol::{PackageBox, PackageBoxEncodeContext};
use crate::types::MixAesKey;

pub struct UDPSocket {
    local: Endpoint,
    socket: UdpSocket,
}

impl std::fmt::Display for UDPSocket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "UDPSocket {{local:{}}}", self.local())
    }
}

impl std::fmt::Debug for UDPSocket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "UDPSocket {{local:{}}}", self.local())
    }
}

impl UDPSocket {
    pub fn local(&self) -> &Endpoint {
        &self.local
    }

    pub fn is_same(&self, other: &Self) -> bool {
        self as *const Self == other as *const Self
    }

    pub fn bind(local: &Endpoint, recv_buffer: usize) -> Result<Self, BuckyError> {
        fn bind_socket(bind_addr: &Endpoint, recv_buffer: usize) -> Result<UdpSocket, BuckyError> {
            let domain = if bind_addr.addr().is_ipv6() {
                Domain::IPV6
            } else {
                Domain::IPV4
            };

            let socket = Socket::new(domain, Type::DGRAM, None).unwrap();

            let _ = socket.set_recv_buffer_size(recv_buffer);
            match socket.bind(&bind_addr.addr().clone().into()) {
                Ok(_) => Ok(UdpSocket::from(std::net::UdpSocket::from(socket))),
                Err(err) => Err(BuckyError::from(err))
            }
        }

        let socket = {
            if local.addr().is_ipv6() {
                #[cfg(windows)]
                {
                    let mut default_local = Endpoint::default_udp(&local);
                    default_local.mut_addr().set_port(local.addr().port());
                    match bind_socket(&default_local, recv_buffer) {
                        Ok(socket) => {
                            // 避免udp被对端reset
                            cyfs_util::init_udp_socket(&socket).map(|_| socket)
                        }
                        Err(err) => Err(BuckyError::from(err)),
                    }
                }
                #[cfg(not(windows))]
                {
                    use std::os::unix::io::FromRawFd;
                    unsafe {
                        let raw_sock = libc::socket(libc::AF_INET6, libc::SOCK_DGRAM, 0);
                        let yes: libc::c_int = 1;
                        libc::setsockopt(
                            raw_sock,
                            libc::IPPROTO_IPV6,
                            libc::IPV6_V6ONLY,
                            &yes as *const libc::c_int as *const libc::c_void,
                            std::mem::size_of::<libc::c_int>().try_into().unwrap(),
                        );
                        libc::setsockopt(
                            raw_sock,
                            libc::SOL_SOCKET,
                            libc::SO_REUSEADDR | libc::SO_BROADCAST,
                            &yes as *const libc::c_int as *const libc::c_void,
                            std::mem::size_of::<libc::c_int>().try_into().unwrap(),
                        );

                        let recv_buf: libc::c_int = config.recv_buffer as libc::c_int;
                        libc::setsockopt(raw_sock,
                                         libc::SOL_SOCKET,
                                         libc::SO_RCVBUF,
                                         &recv_buf as *const libc::c_int as *const libc::c_void,
                                         std::mem::size_of::<libc::c_int>().try_into().unwrap(),);

                        let addr = libc::sockaddr_in6 {
                            #[cfg(any(target_os = "macos",target_os = "ios"))]
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
                            Ok(UdpSocket::from_raw_fd(raw_sock))
                        }
                    }
                }
            } else {
                let bind_addr = {
                    if local.is_sys_default() {
                        let mut default_local = Endpoint::default_udp(&local);
                        default_local.mut_addr().set_port(local.addr().port());

                        default_local
                    } else {
                        local.clone()
                    }
                };

                bind_socket(&bind_addr, recv_buffer)
            }
        }?;

        Ok(Self {
            local: local.clone(),
            socket,
        })
    }

    pub async fn recv_from(&self, buf: &mut [u8]) -> std::io::Result<(usize, Endpoint)> {
        let (size, addr) = self.socket.recv_from(buf).await?;
        Ok((size, Endpoint::from((Protocol::Udp, addr))))
    }

    pub async fn send_to(&self, buf: &[u8], addr: &Endpoint) -> std::io::Result<usize> {
        let size = self.socket.send_to(buf, addr.addr()).await?;
        Ok(size)
    }

    pub fn close(&self) {
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawSocket;
            use winapi::um::winsock2::closesocket;
            unsafe {
                let raw = self.socket.as_raw_socket();
                closesocket(raw.try_into().unwrap());
            }
        }
        #[cfg(not(windows))]
        {
            use std::os::unix::io::AsRawFd;
            unsafe {
                let raw = self.socket.as_raw_fd();
                libc::close(raw);
            }
        }
    }
}
