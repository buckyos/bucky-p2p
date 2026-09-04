use p2p_frame::endpoint::{Endpoint, EndpointArea, Protocol};
use p2p_frame::nat_type::{NatMappingObservation, NatProfile, NatTraversalContext};
use p2p_frame::p2p_identity::P2pId;
use std::time::Duration;

#[test]
fn external_consumer_can_build_exported_profile_and_context() {
    let mut endpoint = Endpoint::from((Protocol::Quic, "198.51.100.1:4000".parse().unwrap()));
    endpoint.set_area(EndpointArea::ServerReflexive);
    let profile =
        NatProfile::from_observations(&[endpoint, endpoint], 1_000_000, Duration::from_secs(10));
    assert_eq!(profile.observation, NatMappingObservation::NonSymmetricLike);
    let context = NatTraversalContext::new(
        P2pId::from(vec![1; 32]),
        P2pId::from(vec![2; 32]),
        profile.clone(),
        profile,
    );
    assert!(context.is_supported());
}
