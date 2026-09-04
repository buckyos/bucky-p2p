#[test]
fn rendezvous_mapping_covers_all_four_target_operations() {
    let cases = [
        (
            ConnectPlan {
                strategy: ConnectStrategy::Matrix,
                connector: Some(PlanParty::Caller),
                connector_candidates: CandidateMode::Base,
                peer_candidates: Some(CandidateMode::Base),
                reverse: false,
                fallback: PlanFallback::Proxy,
            },
            SnTunnelRendezvousOperation::PunchOnly,
            Some(CandidateMode::Base),
            false,
            RendezvousCallerAction::Connect {
                use_predicted_response: false,
            },
        ),
        (
            ConnectPlan {
                strategy: ConnectStrategy::Matrix,
                connector: Some(PlanParty::Caller),
                connector_candidates: CandidateMode::Predicted,
                peer_candidates: None,
                reverse: false,
                fallback: PlanFallback::Proxy,
            },
            SnTunnelRendezvousOperation::WaitIncoming,
            None,
            true,
            RendezvousCallerAction::Connect {
                use_predicted_response: true,
            },
        ),
        (
            ConnectPlan {
                strategy: ConnectStrategy::Matrix,
                connector: Some(PlanParty::Callee),
                connector_candidates: CandidateMode::Predicted,
                peer_candidates: Some(CandidateMode::Predicted),
                reverse: true,
                fallback: PlanFallback::Proxy,
            },
            SnTunnelRendezvousOperation::PunchAndReverseConnect,
            Some(CandidateMode::Predicted),
            true,
            RendezvousCallerAction::PunchThenWait {
                use_predicted_response: true,
            },
        ),
        (
            ConnectPlan {
                strategy: ConnectStrategy::Public,
                connector: Some(PlanParty::Callee),
                connector_candidates: CandidateMode::Base,
                peer_candidates: None,
                reverse: true,
                fallback: PlanFallback::Proxy,
            },
            SnTunnelRendezvousOperation::ReverseConnectOnly,
            Some(CandidateMode::Base),
            false,
            RendezvousCallerAction::WaitIncoming,
        ),
    ];

    for (plan, operation, request_candidates, need_prediction, caller_action) in cases {
        let rendezvous = plan.rendezvous_plan().unwrap();
        assert_eq!(rendezvous.operation, operation);
        assert_eq!(rendezvous.request_candidates, request_candidates);
        assert_eq!(rendezvous.need_predict_endpoint, need_prediction);
        assert_eq!(rendezvous.caller_action, caller_action);
    }
}

#[test]
fn legacy_plan_does_not_start_the_new_protocol() {
    assert_eq!(ConnectPlan::legacy().rendezvous_plan(), None);
}

#[test]
fn symmetric_symmetric_plan_requires_owner_side_prediction_on_both_ends() {
    let now = 9_000_000;
    let plan = select_connect_plan(
        &context(symmetric(now), symmetric(now)),
        now,
        false,
        false,
    );
    let rendezvous = plan.rendezvous_plan().unwrap();

    assert_eq!(rendezvous.operation, SnTunnelRendezvousOperation::PunchOnly);
    assert_eq!(rendezvous.request_candidates, Some(CandidateMode::Predicted));
    assert!(rendezvous.need_predict_endpoint);
    assert_eq!(
        rendezvous.caller_action,
        RendezvousCallerAction::Connect {
            use_predicted_response: true,
        }
    );
}
