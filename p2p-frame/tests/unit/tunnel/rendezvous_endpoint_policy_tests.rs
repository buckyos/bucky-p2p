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
    assert_eq!(
        reverse,
        vec![
            server_reflexive,
            quic_wan,
            quic_mapped,
            tcp_wan,
            tcp_mapped
        ]
    );

    let punch = TunnelManager::rendezvous_base_endpoints(
        &[server_reflexive, quic_wan, quic_mapped, tcp_wan],
        SnTunnelRendezvousOperation::PunchOnly,
    );
    #[cfg(not(feature = "test-real-socket-matrix"))]
    assert_eq!(punch, vec![server_reflexive]);
    #[cfg(feature = "test-real-socket-matrix")]
    assert_eq!(punch, vec![server_reflexive, quic_wan, quic_mapped]);
}
