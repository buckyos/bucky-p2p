mod command;
pub(crate) mod control_stream;
mod net_manager;
mod network;
mod quic;
mod tcp;
mod tunnel;
mod udp_network;
mod validator;

use crate::error::{P2pErrorCode, P2pResult, p2p_err};
pub use crate::tunnel::{
    DefaultDeviceFinder, DeviceFinder, DeviceFinderRef, TunnelManager, TunnelManagerRef,
};
pub use command::*;
use futures::FutureExt;
pub use net_manager::*;
pub use network::*;
pub use quic::*;
use rustls::pki_types::ServerName;
use std::fmt::Debug;
use std::future::Future;
pub use tcp::*;
pub use tunnel::*;
pub use udp_network::*;
pub use validator::*;

pub fn validate_server_name(server_name: String) -> String {
    match ServerName::try_from(server_name.as_str()) {
        Ok(_) => server_name,
        Err(_) => format!("p2p.{}.com", server_name),
    }
}

pub fn parse_server_name(server_name: &str) -> &str {
    if server_name.starts_with("p2p.") && server_name.ends_with(".com") {
        server_name
            .trim_start_matches("p2p.")
            .trim_end_matches(".com")
    } else {
        server_name
    }
}

/// Whether an inbound listener socket must be IPv6-only for this bind address.
///
/// An explicit IPv6 listener must never claim the IPv4 port through a
/// dual-stack wildcard: without `IPV6_V6ONLY` the platform default makes
/// `0.0.0.0:<port>` and `[::]:<port>` mutually exclusive on Linux and turns
/// IPv4 peers into v4-mapped observations on platforms that allow both. IPv4
/// traffic is only served by an explicitly configured IPv4 listener.
pub(crate) fn listener_bind_requires_only_v6(
    bind_addr: std::net::SocketAddr,
) -> bool {
    bind_addr.is_ipv6()
}

/// Apply the listener socket options required by the bind address.
///
/// The socket type is supplied by the caller so the policy stays testable
/// without depending on the socket library version used by the listener layer.
pub(crate) fn apply_listener_socket_options<S, F>(
    bind_addr: std::net::SocketAddr,
    socket: &S,
    set_only_v6: F,
) -> Result<(), sfo_reuseport::Error>
where
    F: FnOnce(&S, bool) -> std::io::Result<()>,
{
    if listener_bind_requires_only_v6(bind_addr) {
        set_only_v6(socket, true)?;
    }
    Ok(())
}

pub async fn select_successful<T, E: Debug, F>(futures: Vec<F>) -> P2pResult<T>
where
    F: Future<Output = Result<T, E>> + Unpin,
{
    let mut futures = futures.into_iter().map(FutureExt::fuse).collect::<Vec<_>>();

    while futures.len() > 0 {
        let select_all = futures::future::select_all(futures);
        match select_all.await {
            (Ok(result), _index, _remaining) => {
                return Ok(result);
            }
            (Err(e), _index, remaining) => {
                log::trace!("select failed {:?}", e);
                futures = remaining;
            }
        }
    }
    Err(p2p_err!(P2pErrorCode::ConnectFailed, "connect failed"))
}

#[cfg(test)]
mod listener_socket_option_tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

    #[test]
    fn only_v6_is_required_for_ipv6_bind_addresses_only() {
        assert!(listener_bind_requires_only_v6(SocketAddr::new(
            IpAddr::V6(Ipv6Addr::UNSPECIFIED),
            0
        )));
        assert!(listener_bind_requires_only_v6(SocketAddr::new(
            IpAddr::V6(Ipv6Addr::LOCALHOST),
            0
        )));
        assert!(!listener_bind_requires_only_v6(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            0
        )));
        assert!(!listener_bind_requires_only_v6(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            0
        )));
    }

    #[test]
    fn listener_socket_options_request_only_v6_for_ipv6_bind_addresses() {
        let mut ipv6_applied: Vec<bool> = Vec::new();
        apply_listener_socket_options(
            SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 0),
            &(),
            |_, only_v6| {
                ipv6_applied.push(only_v6);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(ipv6_applied, vec![true]);

        let mut ipv4_applied: Vec<bool> = Vec::new();
        apply_listener_socket_options(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
            &(),
            |_, only_v6| {
                ipv4_applied.push(only_v6);
                Ok(())
            },
        )
        .unwrap();
        assert!(
            ipv4_applied.is_empty(),
            "IPv4 listeners must keep the platform default"
        );
    }
}
