use super::*;
use crate::endpoint::{EndpointArea, Protocol};

fn endpoint(addr: &str) -> Endpoint {
    let mut endpoint = Endpoint::from((Protocol::Quic, addr.parse().unwrap()));
    endpoint.set_area(EndpointArea::ServerReflexive);
    endpoint
}

#[test]
fn observations_use_complete_endpoints_and_require_two_samples() {
    let now = 1_000_000;
    assert_eq!(
        NatProfile::from_observations(&[], now, Duration::from_secs(1)).observation,
        NatMappingObservation::Unknown
    );
    assert_eq!(
        NatProfile::from_observations(
            &[endpoint("198.51.100.1:4000")],
            now,
            Duration::from_secs(1),
        )
        .observation,
        NatMappingObservation::Unknown
    );

    let stable = NatProfile::from_observations(
        &[endpoint("198.51.100.1:4000"), endpoint("198.51.100.1:4000")],
        now,
        Duration::from_secs(1),
    );
    assert_eq!(stable.observation, NatMappingObservation::NonSymmetricLike);
    assert!(stable.prediction_hint.is_none());

    let changed_ip = NatProfile::from_observations(
        &[endpoint("198.51.100.1:4000"), endpoint("198.51.100.2:4002")],
        now,
        Duration::from_secs(1),
    );
    assert_eq!(changed_ip.observation, NatMappingObservation::SymmetricLike);
    assert!(changed_ip.prediction_hint.is_none());
}

#[test]
fn prediction_hint_requires_consistent_delta_and_obeys_bounds() {
    let now = 2_000_000;
    let profile = NatProfile::from_observations(
        &[
            endpoint("198.51.100.1:4000"),
            endpoint("198.51.100.1:4002"),
            endpoint("198.51.100.1:4004"),
        ],
        now,
        Duration::from_secs(2),
    );
    let base = endpoint("198.51.100.1:5000");
    let hint = profile.usable_prediction_hint(now).unwrap();
    assert_eq!(hint.port_delta, 2);
    assert_eq!(hint.parity, NatPortParityRelation::Same);
    assert_eq!(
        hint.predicted_ports(&base, MAX_NAT_PREDICTION_PORTS + 10),
        vec![5002, 5004, 5006, 5008, 5010, 5012, 5014, 5016]
    );
    assert!(
        hint.predicted_ports(&endpoint("203.0.113.1:5000"), 2)
            .is_empty()
    );
    assert!(
        hint.predicted_ports(&endpoint("198.51.100.1:65535"), 2)
            .is_empty()
    );

    let irregular = NatProfile::from_observations(
        &[
            endpoint("198.51.100.1:4000"),
            endpoint("198.51.100.1:4002"),
            endpoint("198.51.100.1:4005"),
        ],
        now,
        Duration::from_secs(2),
    );
    assert!(irregular.prediction_hint.is_none());
}

#[test]
fn freshness_and_context_roles_fail_closed_at_boundaries() {
    let now = 3_000_000;
    let profile = NatProfile::from_observations(
        &[endpoint("198.51.100.1:4000"), endpoint("198.51.100.1:4000")],
        now,
        Duration::from_micros(10),
    );
    assert!(!profile.is_fresh(now - 1));
    assert!(profile.is_fresh(now));
    assert!(profile.is_fresh(now + 10));
    assert!(!profile.is_fresh(now + 11));

    let caller = P2pId::from(vec![1; 32]);
    let callee = P2pId::from(vec![2; 32]);
    let context =
        NatTraversalContext::new(caller.clone(), callee.clone(), profile.clone(), profile);
    assert!(context.is_valid_for(&caller, &callee, now));
    assert!(!context.is_valid_for(&callee, &caller, now));
    assert!(!context.is_valid_for(&caller, &caller, now));
    assert!(!context.is_valid_for(&caller, &callee, now + 11));
}
