use super::*;

fn endpoint(protocol: Protocol, port: u16, area: EndpointArea) -> Endpoint {
    let mut endpoint = Endpoint::from((protocol, ([8, 8, 8, 8], port).into()));
    endpoint.set_area(area);
    endpoint
}

#[test]
fn reverse_connect_request_candidates_accept_public_wan_and_mapped() {
    let server_reflexive = endpoint(Protocol::Quic, 22011, EndpointArea::ServerReflexive);
    let quic_wan = endpoint(Protocol::Quic, 22012, EndpointArea::Wan);
    let quic_mapped = endpoint(Protocol::Quic, 22013, EndpointArea::Mapped);
    let lan = endpoint(Protocol::Quic, 22014, EndpointArea::Lan);
    let tcp_wan = endpoint(Protocol::Tcp, 22015, EndpointArea::Wan);
    let tcp_mapped = endpoint(Protocol::Tcp, 22016, EndpointArea::Mapped);

    let reverse = TunnelManager::rendezvous_base_endpoints(
        &[
            server_reflexive,
            quic_wan,
            quic_mapped,
            lan,
            tcp_wan,
            tcp_mapped,
        ],
        SnTunnelRendezvousOperation::ReverseConnectOnly,
    );
    assert_eq!(reverse, vec![server_reflexive, quic_wan, quic_mapped]);

    let punch = TunnelManager::rendezvous_base_endpoints(
        &[server_reflexive, quic_wan, quic_mapped, tcp_wan],
        SnTunnelRendezvousOperation::PunchOnly,
    );
    #[cfg(not(feature = "test-real-socket-matrix"))]
    assert_eq!(punch, vec![server_reflexive]);
    #[cfg(feature = "test-real-socket-matrix")]
    assert_eq!(punch, vec![server_reflexive, quic_wan, quic_mapped]);
}

#[test]
fn reverse_connect_falls_back_to_single_tcp_transport_when_no_quic_candidate() {
    let tcp_wan = endpoint(Protocol::Tcp, 22020, EndpointArea::Wan);
    let tcp_mapped = endpoint(Protocol::Tcp, 22021, EndpointArea::Mapped);
    let tcp_lan = endpoint(Protocol::Tcp, 22022, EndpointArea::Lan);

    let reverse = TunnelManager::rendezvous_base_endpoints(
        &[tcp_wan, tcp_mapped, tcp_lan],
        SnTunnelRendezvousOperation::ReverseConnectOnly,
    );
    // LAN is not reverse-connect eligible; the homogeneous array is TCP-only.
    assert_eq!(reverse, vec![tcp_wan, tcp_mapped]);
}

#[test]
fn reverse_connect_mixed_transport_with_no_quic_eligible_anchors_to_tcp() {
    // A TCP endpoint appears first, but no QUIC endpoint is area-eligible, so the
    // candidate set must fall back to a single TCP transport instead of mixing.
    let tcp_wan = endpoint(Protocol::Tcp, 22025, EndpointArea::Wan);
    let tcp_mapped = endpoint(Protocol::Tcp, 22026, EndpointArea::Mapped);
    let quic_lan = endpoint(Protocol::Quic, 22027, EndpointArea::Lan);

    let reverse = TunnelManager::rendezvous_base_endpoints(
        &[tcp_wan, quic_lan, tcp_mapped],
        SnTunnelRendezvousOperation::ReverseConnectOnly,
    );
    assert_eq!(reverse, vec![tcp_wan, tcp_mapped]);
    for endpoint in &reverse {
        assert_eq!(endpoint.protocol(), Protocol::Tcp);
    }
}
