//! Test-only local-resource support for p2p-frame tests.
//!
//! The guard in this module keeps a TCP port occupied from the moment it is
//! selected until the production network path binds the same port for its own
//! socket. This closes the "reserve then drop" window that let concurrent
//! tests steal the same ephemeral port and fail with `AddrInUse`.

use crate::endpoint::{Endpoint, Protocol};
use socket2::{Domain, Protocol as SocketProtocol, Socket, Type};
use std::io;
use std::net::SocketAddr;

/// Non-listening, `SO_REUSEADDR`-bound TCP reservation for `127.0.0.1`.
///
/// While the guard is alive the OS keeps the port busy, but a second socket
/// with `SO_REUSEADDR` (as used by [`crate::networks::tcp::connection`] when
/// the network's reuse flag is on) may bind the same local endpoint for an
/// outgoing connection. Drop the guard only after that connection socket has
/// bound the port.
pub(crate) struct TestTcpPortGuard {
    socket: Socket,
    addr: SocketAddr,
}

impl TestTcpPortGuard {
    /// Reserve a fresh loopback TCP port and hold it.
    pub(crate) fn bind_ipv4_loopback() -> io::Result<Self> {
        let socket = Socket::new(Domain::IPV4, Type::STREAM, Some(SocketProtocol::TCP))?;
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        #[cfg(target_os = "linux")]
        socket.set_reuse_address(true)?;
        let sock_addr: socket2::SockAddr = addr.into();
        socket.bind(&sock_addr)?;
        let local_addr = socket
            .local_addr()?
            .as_socket()
            .expect("ipv4 tcp socket local address");
        Ok(Self {
            socket,
            addr: local_addr,
        })
    }

    /// The endpoint this guard currently owns.
    pub(crate) fn endpoint(&self) -> Endpoint {
        Endpoint::from((Protocol::Tcp, self.addr))
    }
}
