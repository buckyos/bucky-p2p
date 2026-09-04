use crate::sn::service::nat_probe_scheduler::{
    MAX_CONCURRENT_NAT_PROBES, NAT_PROBE_FAILURE_BACKOFF, NAT_PROBE_PERIOD,
    NatProbeAuthorityRemovalReason,
};

fn scheduler_peer(byte: u8) -> P2pId {
    P2pId::from(vec![byte; 32])
}

include!("rendezvous_state_tests.rs");

fn scheduler_endpoint(protocol: Protocol, port: u16) -> Endpoint {
    Endpoint::from((
        protocol,
        format!("198.51.100.10:{port}").parse().unwrap(),
    ))
}

fn scheduler_probe_endpoints(first_port: u16) -> Vec<Endpoint> {
    vec![
        scheduler_endpoint(Protocol::Quic, first_port),
        scheduler_endpoint(Protocol::Quic, first_port + 1),
    ]
}

fn scheduler_profile(now: Timestamp) -> NatProfile {
    NatProfile::from_observations(
        &[
            scheduler_endpoint(Protocol::Quic, 41000),
            scheduler_endpoint(Protocol::Quic, 41000),
        ],
        now,
        NAT_PROBE_PERIOD,
    )
}

fn scheduler_duration(duration: Duration) -> Timestamp {
    duration.as_micros() as Timestamp
}

#[test]
fn nat_probe_scheduler_issues_once_and_reschedules_two_hours_from_completion() {
    let sn = scheduler_peer(1);
    let peer = scheduler_peer(2);
    let tunnel = CmdTunnelId::from(11);
    let remote = scheduler_endpoint(Protocol::Quic, 50000);
    let mut scheduler = NatProbeScheduler::new(sn);
    scheduler.set_endpoints(vec![
        scheduler_endpoint(Protocol::Quic, 32001),
        scheduler_endpoint(Protocol::Quic, 32002),
    ]);

    let initial = scheduler.observe_report(&peer, tunnel, remote, None, 1_000_000);
    let directive = initial.directive.expect("first QUIC report must probe");
    assert!(initial.profile_update == Some(None));
    assert!(scheduler
        .observe_report(&peer, tunnel, remote, None, 2_000_000)
        .directive
        .is_none());

    let completed_at = 3_000_000;
    let result = NatProbeResult::from_directive(&directive, scheduler_profile(completed_at));
    let completed = scheduler.observe_report(
        &peer,
        tunnel,
        remote,
        Some(result),
        completed_at,
    );
    assert!(completed.directive.is_none());
    assert!(completed
        .profile_update
        .as_ref()
        .and_then(|profile| profile.as_ref())
        .is_some());

    let deadline = completed_at + scheduler_duration(NAT_PROBE_PERIOD);
    assert!(scheduler
        .observe_report(&peer, tunnel, remote, None, deadline - 1)
        .directive
        .is_none());
    assert!(scheduler
        .observe_report(&peer, tunnel, remote, None, deadline)
        .directive
        .is_some());
}

#[test]
fn nat_probe_scheduler_rejects_tcp_and_does_not_let_tcp_override_quic_authority() {
    let sn = scheduler_peer(3);
    let peer = scheduler_peer(4);
    let quic_tunnel = CmdTunnelId::from(21);
    let tcp_tunnel = CmdTunnelId::from(22);
    let mut scheduler = NatProbeScheduler::new(sn);
    scheduler.set_endpoints(scheduler_probe_endpoints(32101));

    assert!(scheduler
        .observe_report(
            &peer,
            tcp_tunnel,
            scheduler_endpoint(Protocol::Tcp, 51000),
            None,
            1,
        )
        .directive
        .is_none());
    let quic = scheduler.observe_report(
        &peer,
        quic_tunnel,
        scheduler_endpoint(Protocol::Quic, 51001),
        None,
        2,
    );
    assert!(quic.directive.is_some());
    assert_eq!(scheduler.authority_tunnel(&peer), Some(quic_tunnel));

    let tcp = scheduler.observe_report(
        &peer,
        tcp_tunnel,
        scheduler_endpoint(Protocol::Tcp, 51002),
        None,
        3,
    );
    assert!(tcp.directive.is_none());
    assert!(tcp.profile_update.is_none());
    assert_eq!(scheduler.authority_tunnel(&peer), Some(quic_tunnel));
}

#[test]
fn nat_probe_scheduler_does_not_flap_between_concurrent_quic_tunnels() {
    let sn = scheduler_peer(13);
    let peer = scheduler_peer(14);
    let authority = CmdTunnelId::from(71);
    let concurrent = CmdTunnelId::from(72);
    let mut scheduler = NatProbeScheduler::new(sn);
    scheduler.set_endpoints(vec![
        scheduler_endpoint(Protocol::Quic, 32601),
        scheduler_endpoint(Protocol::Quic, 32602),
    ]);
    let first = scheduler
        .observe_report(
            &peer,
            authority,
            scheduler_endpoint(Protocol::Quic, 56000),
            None,
            1,
        )
        .directive
        .unwrap();

    let ignored = scheduler.observe_report(
        &peer,
        concurrent,
        scheduler_endpoint(Protocol::Quic, 56001),
        None,
        2,
    );
    assert!(ignored.directive.is_none());
    assert!(ignored.profile_update.is_none());
    assert_eq!(scheduler.authority_tunnel(&peer), Some(authority));
    assert_eq!(
        scheduler
            .observe_report(
                &peer,
                authority,
                scheduler_endpoint(Protocol::Quic, 56000),
                None,
                3,
            )
            .directive,
        None
    );
    assert_eq!(first.registration_generation, 1);
}

#[test]
fn nat_probe_scheduler_address_and_config_events_invalidate_and_advance_generation() {
    let sn = scheduler_peer(5);
    let peer = scheduler_peer(6);
    let tunnel = CmdTunnelId::from(31);
    let mut scheduler = NatProbeScheduler::new(sn);
    scheduler.set_endpoints(scheduler_probe_endpoints(32201));
    let first = scheduler
        .observe_report(
            &peer,
            tunnel,
            scheduler_endpoint(Protocol::Quic, 52000),
            None,
            10,
        )
        .directive
        .unwrap();

    let changed = scheduler.observe_report(
        &peer,
        tunnel,
        scheduler_endpoint(Protocol::Quic, 52001),
        None,
        20,
    );
    let changed_directive = changed.directive.unwrap();
    assert!(changed.profile_update == Some(None));
    assert!(changed_directive.registration_generation > first.registration_generation);

    let affected = scheduler.set_endpoints(scheduler_probe_endpoints(32203));
    assert_eq!(affected, vec![peer.clone()]);
    let config_changed = scheduler
        .observe_report(
            &peer,
            tunnel,
            scheduler_endpoint(Protocol::Quic, 52001),
            None,
            30,
        )
        .directive
        .unwrap();
    assert!(config_changed.probe_config_generation > changed_directive.probe_config_generation);
    assert_eq!(config_changed.endpoints, scheduler_probe_endpoints(32203));
}

#[test]
fn nat_probe_scheduler_demand_obeys_failure_backoff_and_current_request_does_not_wait() {
    let sn = scheduler_peer(7);
    let peer = scheduler_peer(8);
    let tunnel = CmdTunnelId::from(41);
    let remote = scheduler_endpoint(Protocol::Quic, 53000);
    let mut scheduler = NatProbeScheduler::new(sn);
    scheduler.set_endpoints(scheduler_probe_endpoints(32301));
    let directive = scheduler
        .observe_report(&peer, tunnel, remote, None, 100)
        .directive
        .unwrap();
    let failed_at = 200;
    let failed = NatProbeResult::from_directive(&directive, NatProfile::unknown());
    let transition =
        scheduler.observe_report(&peer, tunnel, remote, Some(failed), failed_at);
    assert!(transition.profile_update == Some(None));
    assert!(transition.directive.is_none());

    scheduler.mark_demand(&peer, failed_at);
    assert!(scheduler
        .observe_report(
            &peer,
            tunnel,
            remote,
            None,
            failed_at + scheduler_duration(NAT_PROBE_FAILURE_BACKOFF) - 1,
        )
        .directive
        .is_none());
    assert!(scheduler
        .observe_report(
            &peer,
            tunnel,
            remote,
            None,
            failed_at + scheduler_duration(NAT_PROBE_FAILURE_BACKOFF),
        )
        .directive
        .is_some());
}

#[test]
fn nat_probe_scheduler_rejects_late_result_after_observation_generation_changes() {
    let sn = scheduler_peer(9);
    let peer = scheduler_peer(10);
    let tunnel = CmdTunnelId::from(51);
    let mut scheduler = NatProbeScheduler::new(sn);
    scheduler.set_endpoints(scheduler_probe_endpoints(32401));
    let old = scheduler
        .observe_report(
            &peer,
            tunnel,
            scheduler_endpoint(Protocol::Quic, 54000),
            None,
            100,
        )
        .directive
        .unwrap();
    let new_remote = scheduler_endpoint(Protocol::Quic, 54001);
    let new = scheduler
        .observe_report(&peer, tunnel, new_remote, None, 200)
        .directive
        .unwrap();
    assert!(new.registration_generation > old.registration_generation);

    let late = NatProbeResult::from_directive(&old, scheduler_profile(300));
    let transition = scheduler.observe_report(&peer, tunnel, new_remote, Some(late), 300);
    assert!(transition.profile_update.is_none());
    assert!(transition.directive.is_none());
    assert!(scheduler.current_profile(&peer, 300).is_none());
}

#[test]
fn nat_probe_scheduler_timeout_ends_inflight_without_immediate_retry() {
    let sn = scheduler_peer(11);
    let peer = scheduler_peer(12);
    let tunnel = CmdTunnelId::from(61);
    let remote = scheduler_endpoint(Protocol::Quic, 55000);
    let now = 1_000;
    let mut scheduler = NatProbeScheduler::new(sn);
    scheduler.set_endpoints(scheduler_probe_endpoints(32501));
    let directive = scheduler
        .observe_report(&peer, tunnel, remote, None, now)
        .directive
        .unwrap();
    let after_timeout = directive.expires_at + 1;
    let transition = scheduler.observe_report(&peer, tunnel, remote, None, after_timeout);
    assert!(transition.profile_update == Some(None));
    assert!(transition.directive.is_none());
    assert!(scheduler
        .observe_report(
            &peer,
            tunnel,
            remote,
            None,
            after_timeout + scheduler_duration(NAT_PROBE_PERIOD),
        )
        .directive
        .is_some());
}

#[test]
fn nat_probe_scheduler_never_directs_a_client_without_control_capability() {
    let sn = scheduler_peer(15);
    let peer = scheduler_peer(16);
    let tunnel = CmdTunnelId::from(81);
    let remote = scheduler_endpoint(Protocol::Quic, 57000);
    let mut scheduler = NatProbeScheduler::new(sn);
    scheduler.set_endpoints(scheduler_probe_endpoints(32701));

    let legacy = scheduler.observe_capable_report(&peer, tunnel, remote, None, None, 1);
    assert!(legacy.directive.is_none());
    scheduler.mark_demand(&peer, 2);
    assert!(scheduler
        .observe_capable_report(
            &peer,
            tunnel,
            remote,
            None,
            None,
            scheduler_duration(NAT_PROBE_PERIOD) + 2,
        )
        .directive
        .is_none());

    assert!(scheduler
        .observe_capable_report(
            &peer,
            tunnel,
            remote,
            Some(crate::sn::protocol::NAT_PROBE_CONTROL_VERSION),
            None,
            scheduler_duration(NAT_PROBE_PERIOD) + 3,
        )
        .directive
        .is_some());
}

#[test]
fn nat_probe_scheduler_control_address_change_invalidates_before_next_report() {
    let sn = scheduler_peer(17);
    let peer = scheduler_peer(18);
    let tunnel = CmdTunnelId::from(91);
    let remote = scheduler_endpoint(Protocol::Quic, 58000);
    let mut scheduler = NatProbeScheduler::new(sn);
    scheduler.set_endpoints(scheduler_probe_endpoints(32801));
    let directive = scheduler
        .observe_report(&peer, tunnel, remote, None, 10)
        .directive
        .unwrap();
    let result = NatProbeResult::from_directive(&directive, scheduler_profile(20));
    scheduler.observe_report(&peer, tunnel, remote, Some(result), 20);
    assert!(scheduler.current_profile(&peer, 21).is_some());

    let changed_remote = scheduler_endpoint(Protocol::Quic, 58001);
    let changed = scheduler.observe_control(&peer, tunnel, changed_remote, 30);
    assert_eq!(changed.profile_update, Some(None));
    assert!(changed.directive.is_none());
    assert!(scheduler.current_profile(&peer, 30).is_none());
    assert!(scheduler
        .observe_report(&peer, tunnel, changed_remote, None, 31)
        .directive
        .is_some());
}

#[test]
fn nat_probe_scheduler_bounds_global_inflight_and_releases_capacity_on_timeout() {
    let sn = scheduler_peer(19);
    let mut scheduler = NatProbeScheduler::new(sn);
    scheduler.set_endpoints(scheduler_probe_endpoints(32901));
    let mut directives = Vec::new();
    for index in 0..=MAX_CONCURRENT_NAT_PROBES {
        let mut peer_bytes = vec![0u8; 32];
        peer_bytes[..8].copy_from_slice(&(index as u64).to_be_bytes());
        let peer = P2pId::from(peer_bytes);
        let tunnel = CmdTunnelId::from(1000 + index as u32);
        let remote = scheduler_endpoint(Protocol::Quic, 10000 + index as u16);
        directives.push(
            scheduler
                .observe_report(&peer, tunnel, remote, None, 100)
                .directive,
        );
    }
    assert_eq!(
        directives.iter().filter(|directive| directive.is_some()).count(),
        MAX_CONCURRENT_NAT_PROBES
    );
    assert!(directives.last().unwrap().is_none());

    let expires_at = directives[0].as_ref().unwrap().expires_at;
    assert_eq!(
        scheduler.expire_due(expires_at + 1).len(),
        MAX_CONCURRENT_NAT_PROBES
    );
    let blocked_index = MAX_CONCURRENT_NAT_PROBES;
    let mut blocked_peer_bytes = vec![0u8; 32];
    blocked_peer_bytes[..8].copy_from_slice(&(blocked_index as u64).to_be_bytes());
    let blocked_peer = P2pId::from(blocked_peer_bytes);
    assert!(scheduler
        .observe_report(
            &blocked_peer,
            CmdTunnelId::from(1000 + blocked_index as u32),
            scheduler_endpoint(Protocol::Quic, 10000 + blocked_index as u16),
            None,
            expires_at + 2,
        )
        .directive
        .is_some());
}

#[test]
fn nat_probe_scheduler_rejects_invalid_server_endpoint_sets() {
    let mut scheduler = NatProbeScheduler::new(scheduler_peer(20));
    scheduler.set_endpoints(vec![scheduler_endpoint(Protocol::Quic, 33001)]);
    assert!(scheduler.endpoints().is_empty());
    scheduler.set_endpoints(vec![
        scheduler_endpoint(Protocol::Quic, 33001),
        Endpoint::from((
            Protocol::Quic,
            "203.0.113.12:33002".parse().unwrap(),
        )),
    ]);
    assert!(scheduler.endpoints().is_empty());
    scheduler.set_endpoints(scheduler_probe_endpoints(33001));
    assert_eq!(scheduler.endpoints().len(), 2);
}

#[tokio::test]
async fn nat_probe_scheduler_maintenance_removes_a_vanished_quic_authority_without_report() {
    let service = test_sn_service(allow_all_sn_connection_validator());
    let peer = scheduler_peer(21);
    let tunnel = CmdTunnelId::from(501);
    {
        let mut scheduler = service.nat_probe_scheduler.lock().unwrap();
        scheduler.set_endpoints(scheduler_probe_endpoints(33101));
        assert!(scheduler
            .observe_capable_report(
                &peer,
                tunnel,
                scheduler_endpoint(Protocol::Quic, 59000),
                Some(crate::sn::protocol::NAT_PROBE_CONTROL_VERSION),
                None,
                1,
            )
            .directive
            .is_some());
        assert_eq!(scheduler.authority_tunnel(&peer), Some(tunnel));
    }

    service.maintain_nat_probe_state().await;
    assert!(service
        .nat_probe_scheduler
        .lock()
        .unwrap()
        .authority_tunnel(&peer)
        .is_none());
}

#[test]
fn nat_probe_scheduler_logs_correlated_lifecycle_reasons_without_stable_report_noise() {
    crate::sn::tests::enable_nat_probe_test_logging();
    let log_start = crate::sn::tests::nat_probe_test_logs().len();
    let sn = scheduler_peer(231);
    let peer = scheduler_peer(232);
    let tunnel = CmdTunnelId::from(2301);
    let remote = scheduler_endpoint(Protocol::Quic, 60100);
    let mut scheduler = NatProbeScheduler::new(sn.clone());
    scheduler.set_endpoints(scheduler_probe_endpoints(33201));

    let online = scheduler
        .observe_report(&peer, tunnel, remote, None, 1_000)
        .directive
        .expect("online report must issue the initial directive");
    let quiet_start = crate::sn::tests::nat_probe_test_logs()
        .iter()
        .filter(|(level, message)| {
            *level <= log::Level::Info && message.contains(&peer.to_string())
        })
        .count();
    assert!(scheduler
        .observe_report(&peer, tunnel, remote, None, 1_001)
        .directive
        .is_none());
    assert_eq!(
        crate::sn::tests::nat_probe_test_logs()
            .iter()
            .filter(|(level, message)| {
                *level <= log::Level::Info && message.contains(&peer.to_string())
            })
            .count(),
        quiet_start,
        "a stable report with no due work must not add info/warn logs"
    );

    scheduler.mark_demand(&peer, 1_002);
    assert!(scheduler
        .observe_report(&peer, tunnel, remote, None, 1_003)
        .directive
        .is_none());
    assert!(scheduler
        .observe_report(&peer, tunnel, remote, None, online.expires_at + 1)
        .directive
        .is_none());

    let retry_at = online.expires_at
        + 1
        + scheduler_duration(NAT_PROBE_FAILURE_BACKOFF);
    let demand = scheduler
        .observe_report(&peer, tunnel, remote, None, retry_at)
        .directive
        .expect("queued demand must run after failure backoff");
    let accepted_at = retry_at + 1;
    let accepted = NatProbeResult::from_directive(&demand, scheduler_profile(accepted_at));
    scheduler.observe_report(&peer, tunnel, remote, Some(accepted), accepted_at);

    assert!(scheduler.force_periodic_due(&peer, accepted_at + 1));
    let periodic = scheduler
        .observe_report(&peer, tunnel, remote, None, accepted_at + 1)
        .directive
        .expect("forced deadline must issue a periodic directive");
    let mut rejected = NatProbeResult::from_directive(
        &periodic,
        scheduler_profile(accepted_at + 2),
    );
    rejected.request_id = rejected.request_id.wrapping_add(1);
    scheduler.observe_report(
        &peer,
        tunnel,
        remote,
        Some(rejected),
        accepted_at + 2,
    );
    assert!(scheduler.remove_peer(
        &peer,
        NatProbeAuthorityRemovalReason::PeerDisconnected,
    ));

    let peer_text = peer.to_string();
    let logs: Vec<String> = crate::sn::tests::nat_probe_test_logs()[log_start..]
        .iter()
        .map(|(_, message)| message)
        .filter(|message| message.contains(&peer_text))
        .cloned()
        .collect();
    let has = |needle: &str| logs.iter().any(|message| message.contains(needle));
    assert!(has("event=nat_probe_authority_established"));
    assert!(has("event=nat_probe_directive_issued") && has("trigger=online"));
    assert!(has("event=nat_probe_directive_suppressed") && has("reason=in_flight"));
    assert!(has("event=nat_probe_directive_timeout"));
    assert!(has("reason=failure_backoff"));
    assert!(has("trigger=demand"));
    assert!(has("event=nat_probe_result_accepted"));
    assert!(has("trigger=periodic"));
    assert!(has("event=nat_probe_result_rejected") && has("reason=request_mismatch"));
    assert!(has("event=nat_probe_authority_removed") && has("reason=peer_disconnected"));
    assert!(logs.iter().all(|message| message.contains("sn_id=")));
    assert!(logs.iter().all(|message| message.contains("peer_id=")));
}
