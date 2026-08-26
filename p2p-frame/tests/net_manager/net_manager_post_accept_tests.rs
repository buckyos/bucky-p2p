use super::*;

fn register_acceptance_subscriber(
    manager: &NetManagerRef,
    local_id: P2pId,
    acceptance: IncomingTunnelAcceptance,
) {
    manager
        .register_incoming_tunnel_acceptance_subscriber(
            local_id,
            Arc::new(move |_| Box::pin(async move { acceptance })),
        )
        .unwrap();
}

#[tokio::test]
async fn acceptance_callback_commits_only_explicit_acceptance() {
    let accepted_manager = new_test_manager(TestValidator::new(HashMap::new()));
    let accepted_local = test_id(31);
    register_acceptance_subscriber(
        &accepted_manager,
        accepted_local.clone(),
        IncomingTunnelAcceptance::Accepted,
    );
    let accepted_tunnel = TestTunnel::new(accepted_local, test_id(32), 31, 311);

    let accepted =
        accepted_manager.incoming_tunnel_acceptance_callback()(Ok(accepted_tunnel.clone())).await;

    assert_eq!(accepted, IncomingTunnelAcceptance::Accepted);
    assert_eq!(accepted_tunnel.close_count(), 0);

    let rejected_manager = new_test_manager(TestValidator::new(HashMap::new()));
    let rejected_local = test_id(33);
    register_acceptance_subscriber(
        &rejected_manager,
        rejected_local.clone(),
        IncomingTunnelAcceptance::Rejected,
    );
    let rejected_tunnel = TestTunnel::new(rejected_local, test_id(34), 32, 321);

    let rejected =
        rejected_manager.incoming_tunnel_acceptance_callback()(Ok(rejected_tunnel.clone())).await;

    assert_eq!(rejected, IncomingTunnelAcceptance::Rejected);
    assert_eq!(rejected_tunnel.close_count(), 1);
}

#[tokio::test]
async fn validator_and_missing_subscriber_rejections_are_explicit() {
    for (seed, decision) in [(41, TestDecision::Reject), (43, TestDecision::Error)] {
        let local_id = test_id(seed);
        let remote_id = test_id(seed + 1);
        let manager = new_test_manager(TestValidator::new(HashMap::from([(
            remote_id.clone(),
            decision,
        )])));
        register_acceptance_subscriber(
            &manager,
            local_id.clone(),
            IncomingTunnelAcceptance::Accepted,
        );
        let tunnel = TestTunnel::new(local_id, remote_id, seed as u32, seed as u32 + 1000);

        let acceptance = manager.incoming_tunnel_acceptance_callback()(Ok(tunnel.clone())).await;

        assert_eq!(acceptance, IncomingTunnelAcceptance::Rejected);
        assert_eq!(tunnel.close_count(), 1);
    }

    let manager = new_test_manager(TestValidator::new(HashMap::new()));
    let tunnel = TestTunnel::new(test_id(45), test_id(46), 45, 1045);
    let acceptance = manager.incoming_tunnel_acceptance_callback()(Ok(tunnel.clone())).await;

    assert_eq!(acceptance, IncomingTunnelAcceptance::Rejected);
    assert_eq!(tunnel.close_count(), 1);
}

#[tokio::test]
async fn legacy_subscriber_liveness_remains_separate_from_acceptance_path() {
    let manager = new_test_manager(TestValidator::new(HashMap::new()));
    let local_id = test_id(51);
    manager
        .register_incoming_tunnel_subscriber(
            local_id.clone(),
            Arc::new(|_| Box::pin(async { false })),
        )
        .unwrap();
    let duplicate = manager.register_incoming_tunnel_acceptance_subscriber(
        local_id.clone(),
        Arc::new(|_| Box::pin(async { IncomingTunnelAcceptance::Accepted })),
    );
    assert_eq!(duplicate.unwrap_err().code(), P2pErrorCode::AlreadyExists);

    let first = TestTunnel::new(local_id.clone(), test_id(52), 51, 1051);
    let first_result = manager.incoming_tunnel_acceptance_callback()(Ok(first.clone())).await;
    assert_eq!(first_result, IncomingTunnelAcceptance::Rejected);
    assert_eq!(first.close_count(), 1);

    let second = TestTunnel::new(local_id, test_id(53), 52, 1052);
    let second_result = manager.incoming_tunnel_acceptance_callback()(Ok(second.clone())).await;
    assert_eq!(second_result, IncomingTunnelAcceptance::Rejected);
    assert_eq!(second.close_count(), 1);
}
