use super::*;

#[test]
fn reverse_connect_accepts_public_wan_and_mapped_without_broadening_punch_policy() {
    for area in [
        EndpointArea::ServerReflexive,
        EndpointArea::Wan,
        EndpointArea::Mapped,
    ] {
        let mut endpoint = Endpoint::from((
            Protocol::Quic,
            "203.0.113.9:3458".parse::<SocketAddr>().unwrap(),
        ));
        endpoint.set_area(area);
        assert!(rendezvous_reverse_connect_eligible_area(&endpoint));

        if area != EndpointArea::ServerReflexive {
            #[cfg(not(feature = "test-real-socket-matrix"))]
            assert!(!rendezvous_eligible_area(&endpoint));
        }
    }

    let mut lan = Endpoint::from((
        Protocol::Quic,
        "203.0.113.10:3459".parse::<SocketAddr>().unwrap(),
    ));
    lan.set_area(EndpointArea::Lan);
    assert!(!rendezvous_reverse_connect_eligible_area(&lan));

    let mut private = Endpoint::from((
        Protocol::Quic,
        "192.168.1.20:3460".parse::<SocketAddr>().unwrap(),
    ));
    private.set_area(EndpointArea::Wan);
    #[cfg(not(feature = "test-real-socket-matrix"))]
    assert!(!rendezvous_reverse_connect_eligible_area(&private));
    #[cfg(feature = "test-real-socket-matrix")]
    assert!(rendezvous_reverse_connect_eligible_area(&private));
}
