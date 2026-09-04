use super::*;
use crate::endpoint::{Endpoint, EndpointArea, Protocol};
use crate::nat_type::NatProfile;
use std::time::Duration;

fn endpoint(port: u16) -> Endpoint {
    let mut endpoint = Endpoint::from((
        Protocol::Quic,
        "198.51.100.1".parse::<std::net::IpAddr>().unwrap(),
        port,
    ));
    endpoint.set_area(EndpointArea::ServerReflexive);
    endpoint
}

fn non_symmetric(now: u64) -> NatProfile {
    NatProfile::from_observations(
        &[endpoint(4000), endpoint(4000)],
        now,
        Duration::from_secs(10),
    )
}

fn symmetric(now: u64) -> NatProfile {
    NatProfile::from_observations(
        &[endpoint(4000), endpoint(4002), endpoint(4004)],
        now,
        Duration::from_secs(10),
    )
}

fn context(caller: NatProfile, callee: NatProfile) -> NatTraversalContext {
    NatTraversalContext::new(
        crate::p2p_identity::P2pId::from(vec![1; 32]),
        crate::p2p_identity::P2pId::from(vec![2; 32]),
        caller,
        callee,
    )
}

#[test]
fn ordered_matrix_selects_one_connector_and_matching_peer_action() {
    let now = 1_000_000;
    let cases = [
        (
            context(non_symmetric(now), non_symmetric(now)),
            PlanAction::Connect {
                candidates: CandidateMode::Base,
                reverse: false,
            },
            PlanAction::PunchThenWait {
                candidates: CandidateMode::Base,
            },
        ),
        (
            context(non_symmetric(now), symmetric(now)),
            PlanAction::PunchThenWait {
                candidates: CandidateMode::Predicted,
            },
            PlanAction::Connect {
                candidates: CandidateMode::Base,
                reverse: true,
            },
        ),
        (
            context(symmetric(now), non_symmetric(now)),
            PlanAction::Connect {
                candidates: CandidateMode::Base,
                reverse: false,
            },
            PlanAction::PunchThenWait {
                candidates: CandidateMode::Predicted,
            },
        ),
        (
            context(symmetric(now), symmetric(now)),
            PlanAction::Connect {
                candidates: CandidateMode::Predicted,
                reverse: false,
            },
            PlanAction::PunchThenWait {
                candidates: CandidateMode::Predicted,
            },
        ),
    ];
    for (context, caller_action, callee_action) in cases {
        let plan = select_connect_plan(&context, now, false, false);
        assert_eq!(plan.strategy, ConnectStrategy::Matrix);
        assert_eq!(plan.action_for(PlanParty::Caller), caller_action);
        assert_eq!(plan.action_for(PlanParty::Callee), callee_action);
    }
}

#[test]
fn public_unknown_and_unpredictable_profiles_choose_explicit_fallbacks() {
    let now = 2_000_000;
    let known = context(non_symmetric(now), non_symmetric(now));
    let callee_public = select_connect_plan(&known, now, false, true);
    assert_eq!(callee_public.strategy, ConnectStrategy::Public);
    assert!(matches!(
        callee_public.action_for(PlanParty::Caller),
        PlanAction::Connect { reverse: false, .. }
    ));
    let caller_public = select_connect_plan(&known, now, true, false);
    assert!(matches!(
        caller_public.action_for(PlanParty::Callee),
        PlanAction::Connect { reverse: true, .. }
    ));

    let unknown = context(NatProfile::unknown(), non_symmetric(now));
    assert_eq!(
        select_connect_plan(&unknown, now, false, false),
        ConnectPlan::legacy()
    );

    let unpredictable = NatProfile::from_observations(
        &[endpoint(4000), {
            let mut other = endpoint(4002);
            *other.mut_addr() = "198.51.100.2:4002".parse().unwrap();
            other
        }],
        now,
        Duration::from_secs(10),
    );
    let best_effort = select_connect_plan(
        &context(non_symmetric(now), unpredictable),
        now,
        false,
        false,
    );
    assert_eq!(best_effort.strategy, ConnectStrategy::BoundedBestEffort);
    assert_eq!(best_effort.connector_candidates, CandidateMode::Base);
    assert_eq!(best_effort.peer_candidates, Some(CandidateMode::Base));
}

include!("rendezvous_tests.rs");
