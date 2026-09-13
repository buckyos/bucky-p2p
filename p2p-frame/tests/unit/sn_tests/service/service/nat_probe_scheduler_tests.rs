use crate::sn::service::nat_probe_scheduler::{
    MAX_CONCURRENT_NAT_PROBES, NAT_PROBE_PERIOD, NatProbeAuthorityRemovalReason, NatProbeScheduler,
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

fn scheduler_probe_ports(first_port: u16) -> Vec<u16> {
    vec![first_port, first_port + 1]
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
fn nat_probe_scheduler_issues_once_and_does_not_reschedule_periodically() {
    let sn = scheduler_peer(1);
    let peer = scheduler_peer(2);
    let tunnel = CmdTunnelId::from(11);
    let remote = scheduler_endpoint(Protocol::Quic, 50000);
    let mut scheduler = NatProbeScheduler::new(sn);
    scheduler.set_ports(vec![32001, 32002]);

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
        .is_none(),
        "server must not issue periodic directives after a completed probe"
    );
}

#[test]
fn nat_probe_scheduler_rejects_tcp_and_does_not_let_tcp_override_quic_authority() {
    let sn = scheduler_peer(3);
    let peer = scheduler_peer(4);
    let quic_tunnel = CmdTunnelId::from(21);
    let tcp_tunnel = CmdTunnelId::from(22);
    let mut scheduler = NatProbeScheduler::new(sn);
    scheduler.set_ports(scheduler_probe_ports(32101));

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
    scheduler.set_ports(vec![32601, 32602]);
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
fn nat_probe_scheduler_address_change_defers_profile_clearing_and_advances_generation() {
    let sn = scheduler_peer(5);
    let peer = scheduler_peer(6);
    let tunnel = CmdTunnelId::from(31);
    let mut scheduler = NatProbeScheduler::new(sn);
    scheduler.set_ports(scheduler_probe_ports(32201));
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
    assert!(changed.profile_update.is_none());
    assert!(changed_directive.registration_generation > first.registration_generation);

    let affected = scheduler.set_ports(scheduler_probe_ports(32203));
    assert_eq!(affected, vec![peer.clone()]);
    let config_report = scheduler
        .observe_report(
            &peer,
            tunnel,
            scheduler_endpoint(Protocol::Quic, 52001),
            None,
            30,
    );
    assert!(config_report.directive.is_none());
    let config_changed = scheduler
        .observe_report(
            &peer,
            tunnel,
            scheduler_endpoint(Protocol::Quic, 52002),
            None,
            40,
        )
        .directive
        .unwrap();
    assert!(config_changed.probe_config_generation > changed_directive.probe_config_generation);
    assert_eq!(config_changed.ports, scheduler_probe_ports(32203));
}

#[test]
fn nat_probe_scheduler_failed_result_does_not_reissue_without_address_change() {
    let sn = scheduler_peer(7);
    let peer = scheduler_peer(8);
    let tunnel = CmdTunnelId::from(41);
    let remote = scheduler_endpoint(Protocol::Quic, 53000);
    let mut scheduler = NatProbeScheduler::new(sn);
    scheduler.set_ports(scheduler_probe_ports(32301));
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

    assert!(scheduler
        .observe_report(
            &peer,
            tunnel,
            remote,
            None,
            failed_at + scheduler_duration(NAT_PROBE_PERIOD),
        )
        .directive
        .is_none(),
        "no demand/periodic re-probe after a failed result"
    );
    assert!(scheduler
        .observe_report(
            &peer,
            tunnel,
            scheduler_endpoint(Protocol::Quic, 53001),
            None,
            failed_at + scheduler_duration(NAT_PROBE_PERIOD) + 1,
        )
        .directive
        .is_some(),
        "only an observed address change can issue a fresh directive"
    );
}

#[test]
fn nat_probe_scheduler_rejects_late_result_after_observation_generation_changes() {
    let sn = scheduler_peer(9);
    let peer = scheduler_peer(10);
    let tunnel = CmdTunnelId::from(51);
    let mut scheduler = NatProbeScheduler::new(sn);
    scheduler.set_ports(scheduler_probe_ports(32401));
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
    scheduler.set_ports(scheduler_probe_ports(32501));
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
        .is_none(),
        "a timeout must not reschedule a periodic server directive"
    );
}

#[test]
fn nat_probe_scheduler_never_directs_a_client_without_control_capability() {
    let sn = scheduler_peer(15);
    let peer = scheduler_peer(16);
    let tunnel = CmdTunnelId::from(81);
    let remote = scheduler_endpoint(Protocol::Quic, 57000);
    let mut scheduler = NatProbeScheduler::new(sn);
    scheduler.set_ports(scheduler_probe_ports(32701));

    let legacy = scheduler.observe_capable_report(&peer, tunnel, remote, None, None, 1);
    assert!(legacy.directive.is_none());
    assert!(scheduler
        .observe_capable_report(
            &peer,
            tunnel,
            remote,
            None,
            None,
            2,
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
            3,
        )
        .directive
        .is_some());
}

#[test]
fn nat_probe_scheduler_control_address_change_preserves_profile_until_report() {
    let sn = scheduler_peer(17);
    let peer = scheduler_peer(18);
    let tunnel = CmdTunnelId::from(91);
    let remote = scheduler_endpoint(Protocol::Quic, 58000);
    let mut scheduler = NatProbeScheduler::new(sn);
    scheduler.set_ports(scheduler_probe_ports(32801));
    let directive = scheduler
        .observe_report(&peer, tunnel, remote, None, 10)
        .directive
        .unwrap();
    let result = NatProbeResult::from_directive(&directive, scheduler_profile(20));
    scheduler.observe_report(&peer, tunnel, remote, Some(result), 20);
    assert!(scheduler.current_profile(&peer, 21).is_some());

    let changed_remote = scheduler_endpoint(Protocol::Quic, 58001);
    let changed = scheduler.observe_control(&peer, tunnel, changed_remote, 30);
    assert!(changed.profile_update.is_none());
    assert!(changed.directive.is_none());
    assert!(scheduler.current_profile(&peer, 30).is_some());
    assert!(scheduler
        .observe_report(&peer, tunnel, changed_remote, None, 31)
        .directive
        .is_some());
    assert!(scheduler.current_profile(&peer, 31).is_some());
}

#[test]
fn nat_probe_scheduler_external_address_report_keeps_old_profile_until_new_result() {
    let sn = scheduler_peer(199);
    let peer = scheduler_peer(200);
    let tunnel = CmdTunnelId::from(1901);
    let remote = scheduler_endpoint(Protocol::Quic, 59100);
    let mut scheduler = NatProbeScheduler::new(sn);
    scheduler.set_ports(scheduler_probe_ports(33301));

    let first = scheduler
        .observe_report(&peer, tunnel, remote, None, 1_000_000)
        .directive
        .unwrap();
    let old_profile = scheduler_profile(2_000_000);
    let accepted = NatProbeResult::from_directive(&first, old_profile.clone());
    scheduler.observe_report(&peer, tunnel, remote, Some(accepted), 2_000_000);
    assert!(scheduler.current_profile(&peer, 2_000_001).is_some());

    let changed_remote = scheduler_endpoint(Protocol::Quic, 59101);
    let changed = scheduler.observe_report(&peer, tunnel, changed_remote, None, 3_000_000);
    assert!(changed.directive.is_some());
    assert!(changed.profile_update.is_none(), "address change must not clear the published profile");
    assert!(scheduler.current_profile(&peer, 3_000_000).is_some());

    // Same request's net_profile is the old profile, so it only confirms the
    // profile already retained by the scheduler; no clearing is overwritten.
    let reported = scheduler.observe_reported_profile(
        &peer,
        tunnel,
        changed_remote,
        old_profile.clone(),
        3_000_000,
    );
    assert!(reported
        .profile_update
        .as_ref()
        .and_then(|profile| profile.as_ref())
        .is_some());
    assert_eq!(
        scheduler.current_profile(&peer, 3_000_000),
        Some(old_profile.clone())
    );

    let new_profile = scheduler_profile(4_000_000);
    let completed = NatProbeResult::from_directive(&changed.directive.unwrap(), new_profile.clone());
    scheduler.observe_report(
        &peer,
        tunnel,
        changed_remote,
        Some(completed),
        4_000_000,
    );
    assert_eq!(scheduler.current_profile(&peer, 4_000_000), Some(new_profile));
}

#[test]
fn nat_probe_scheduler_bounds_global_inflight_and_releases_capacity_on_timeout() {
    let sn = scheduler_peer(19);
    let mut scheduler = NatProbeScheduler::new(sn);
    scheduler.set_ports(scheduler_probe_ports(32901));
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
fn nat_probe_scheduler_rejects_invalid_server_port_sets() {
    let mut scheduler = NatProbeScheduler::new(scheduler_peer(20));
    scheduler.set_ports(vec![33001]);
    assert!(scheduler.ports().is_empty());
    scheduler.set_ports(vec![33001, 33001]);
    assert!(scheduler.ports().is_empty());
    scheduler.set_ports(vec![0, 33002]);
    assert!(scheduler.ports().is_empty());
    scheduler.set_ports(scheduler_probe_ports(33001));
    assert_eq!(scheduler.ports(), scheduler_probe_ports(33001));
}

#[tokio::test]
async fn nat_probe_scheduler_maintenance_removes_a_vanished_quic_authority_without_report() {
    let service = test_sn_service(allow_all_sn_connection_validator());
    let peer = scheduler_peer(21);
    let tunnel = CmdTunnelId::from(501);
    {
        let mut scheduler = service.nat_probe_scheduler.lock().unwrap();
        scheduler.set_ports(scheduler_probe_ports(33101));
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn nat_probe_authority_liveness_keeps_registration_while_a_same_path_stream_is_alive() {
    let (sn_service, _caller, caller_id, _observer, _observer_id, _sn_id, _cert_factory) =
        crate::sn::tests::setup_sn_and_two_clients().await;

    // Real authenticated command stream of that peer. Its server-observed
    // endpoint is the registered observed path the authority check must use.
    let cmd_peer_id = sfo_cmd_server::PeerId::from(caller_id.as_slice());
    let tunnels = sn_service
        .get_cmd_server()
        .get_peer_tunnels(&cmd_peer_id)
        .await;
    assert_eq!(
        tunnels.len(),
        1,
        "the setup must leave exactly one accepted command stream for the client"
    );
    let live_observed = tunnels[0].send.get().await.remote();
    assert!(live_observed.is_udp());

    // The authority registration points at a command stream that is no longer
    // accepted by the SN (for example one closed by a single command QA
    // timeout) while a multiplexed stream on the same observed path is still
    // alive. The missing authority identity is injected because the command
    // client never signals a per-stream close, so the reported state cannot be
    // produced end to end through the public client API.
    let missing_authority = CmdTunnelId::from(0x7511);
    let now = bucky_time::bucky_time_now();
    let profile = scheduler_profile(now);
    {
        let mut scheduler = sn_service.service().nat_probe_scheduler.lock().unwrap();
        assert!(scheduler.remove_peer(&caller_id, NatProbeAuthorityRemovalReason::PeerDisconnected));
        assert!(scheduler
            .observe_capable_report(
                &caller_id,
                missing_authority,
                live_observed,
                Some(crate::sn::protocol::NAT_PROBE_CONTROL_VERSION),
                None,
                now,
            )
            .directive
            .is_none());
        assert_eq!(
            scheduler.authority_tunnel(&caller_id),
            Some(missing_authority)
        );
        assert!(scheduler
            .observe_reported_profile(
                &caller_id,
                missing_authority,
                live_observed,
                profile.clone(),
                now,
            )
            .profile_update
            .is_some());
    }

    sn_service.service().maintain_nat_probe_state().await;

    {
        let scheduler = sn_service.service().nat_probe_scheduler.lock().unwrap();
        assert_eq!(
            scheduler.authority_tunnel(&caller_id),
            Some(missing_authority),
            "the registration must survive while a command stream on the same observed path is alive"
        );
        assert_eq!(
            scheduler.current_profile(&caller_id, now),
            Some(profile.clone()),
            "the published profile must survive the same reconcile"
        );
    }

    // Control: a registration whose observed path has no accepted command
    // stream is still recycled instead of lingering forever.
    let absent_path = scheduler_endpoint(Protocol::Quic, 61001);
    {
        let mut scheduler = sn_service.service().nat_probe_scheduler.lock().unwrap();
        assert_eq!(
            scheduler.authority_tunnel(&caller_id),
            Some(missing_authority)
        );
        scheduler.observe_capable_report(
            &caller_id,
            missing_authority,
            absent_path,
            Some(crate::sn::protocol::NAT_PROBE_CONTROL_VERSION),
            None,
            now + 1,
        );
    }
    sn_service.service().maintain_nat_probe_state().await;
    assert!(sn_service
        .service()
        .nat_probe_scheduler
        .lock()
        .unwrap()
        .authority_tunnel(&caller_id)
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
    scheduler.set_ports(scheduler_probe_ports(33201));

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

    let changed_remote = scheduler_endpoint(Protocol::Quic, 60101);
    let changed = scheduler.observe_report(&peer, tunnel, changed_remote, None, 1_500);
    let changed_directive = changed
        .directive
        .expect("an observed address change must issue a fresh directive");
    assert!(changed.profile_update.is_none());

    let after_timeout = changed_directive.expires_at + 1;
    let timed_out = scheduler.observe_report(&peer, tunnel, changed_remote, None, after_timeout);
    assert!(timed_out.profile_update == Some(None));
    assert!(timed_out.directive.is_none());

    let reboot_remote = scheduler_endpoint(Protocol::Quic, 60102);
    let fresh = scheduler.observe_report(&peer, tunnel, reboot_remote, None, after_timeout + 1);
    assert!(fresh.directive.is_some());
    let accepted = NatProbeResult::from_directive(&fresh.directive.unwrap(), scheduler_profile(after_timeout + 2));
    scheduler.observe_report(
        &peer,
        tunnel,
        reboot_remote,
        Some(accepted),
        after_timeout + 2,
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
    assert!(has("event=nat_probe_directive_timeout"));
    assert!(has("event=nat_probe_directive_issued") && has("trigger=external_address"));
    assert!(has("event=nat_probe_authority_removed") && has("reason=peer_disconnected"));
    assert!(logs.iter().all(|message| message.contains("sn_id=")));
    assert!(logs.iter().all(|message| message.contains("peer_id=")));
}

#[test]
fn remove_peer_if_authority_only_removes_a_matching_registration() {
    let sn = scheduler_peer(30);
    let peer = scheduler_peer(31);
    let tunnel = CmdTunnelId::from(301);
    let mut scheduler = NatProbeScheduler::new(sn);
    scheduler.set_ports(scheduler_probe_ports(36001));
    let first = scheduler
        .observe_report(
            &peer,
            tunnel,
            scheduler_endpoint(Protocol::Quic, 63000),
            None,
            1,
        )
        .directive
        .unwrap();
    let first_generation = first.registration_generation;

    // A stale generation on an otherwise-matching tunnel must not delete the
    // current registration (a concurrent re-registration bumped the generation).
    assert!(!scheduler.remove_peer_if_authority(
        &peer,
        tunnel,
        first_generation + 1,
        NatProbeAuthorityRemovalReason::TunnelMissing
    ));
    assert_eq!(scheduler.authority_registration(&peer), Some((tunnel, first_generation)));

    // A stale tunnel with the matching generation must also be refused.
    assert!(!scheduler.remove_peer_if_authority(
        &peer,
        CmdTunnelId::from(399),
        first_generation,
        NatProbeAuthorityRemovalReason::TunnelMissing
    ));
    assert_eq!(scheduler.authority_registration(&peer), Some((tunnel, first_generation)));

    // A snapshot that still matches removes the registration.
    assert!(scheduler.remove_peer_if_authority(
        &peer,
        tunnel,
        first_generation,
        NatProbeAuthorityRemovalReason::TunnelMissing
    ));
    assert!(scheduler.authority_registration(&peer).is_none());

    // Removing an already-absent registration is a no-op.
    assert!(!scheduler.remove_peer_if_authority(
        &peer,
        tunnel,
        first_generation,
        NatProbeAuthorityRemovalReason::TunnelMissing
    ));
}

#[tokio::test]
async fn stale_reconcile_does_not_delete_a_registration_rebuilt_during_tunnel_scan() {
    let service = test_sn_service(allow_all_sn_connection_validator());
    let peer = test_id(70);
    let identity = test_identity_for_id(peer.clone(), Vec::new());
    let cert = identity.get_identity_cert().unwrap();
    service.peer_mgr.add_or_update_peer(&peer, &Some(cert), 0, Vec::new(), &Vec::new());

    let old_tunnel = CmdTunnelId::from(601);
    let new_tunnel = CmdTunnelId::from(602);
    let old_remote = scheduler_endpoint(Protocol::Quic, 59001);
    let new_remote = scheduler_endpoint(Protocol::Quic, 59002);

    // 1) First registration; capture the reconcile snapshot that the service
    //    reads before it awaits the connection list.
    {
        let mut scheduler = service.nat_probe_scheduler.lock().unwrap();
        scheduler.set_ports(scheduler_probe_ports(33111));
        assert!(scheduler
            .observe_capable_report(
                &peer,
                old_tunnel,
                old_remote,
                Some(crate::sn::protocol::NAT_PROBE_CONTROL_VERSION),
                None,
                1,
            )
            .directive
            .is_some());
    }
    let (old_tunnel_snapshot, old_generation) = service
        .nat_probe_scheduler
        .lock()
        .unwrap()
        .authority_registration(&peer)
        .unwrap();

    // 2) Concurrent disconnect + reconnect: drop the registration, re-register
    //    on the new tunnel, and publish a fresh profile through a completed
    //    probe while the stale scan is still in flight.
    let completed = {
        let mut scheduler = service.nat_probe_scheduler.lock().unwrap();
        scheduler.remove_peer(&peer, NatProbeAuthorityRemovalReason::PeerDisconnected);
        let directive = scheduler
            .observe_capable_report(
                &peer,
                new_tunnel,
                new_remote,
                Some(crate::sn::protocol::NAT_PROBE_CONTROL_VERSION),
                None,
                2_000_000,
            )
            .directive
            .unwrap();
        let result = NatProbeResult::from_directive(&directive, scheduler_profile(3_000_000));
        scheduler.observe_capable_report(
            &peer,
            new_tunnel,
            new_remote,
            Some(crate::sn::protocol::NAT_PROBE_CONTROL_VERSION),
            Some(result),
            3_000_000,
        )
    };
    service.apply_nat_probe_transition(&peer, completed);

    // 3) The stale reconciliation resumes after the reconnect finished: the
    //    old snapshot no longer matches, so it must give up entirely.
    service.finish_nat_probe_authority_reconcile(
        &peer,
        old_tunnel_snapshot,
        old_generation,
        false,
    );

    // The rebuilt registration and its published profile must both survive.
    let (current_tunnel, current_generation) = service
        .nat_probe_scheduler
        .lock()
        .unwrap()
        .authority_registration(&peer)
        .unwrap();
    assert_eq!(current_tunnel, new_tunnel);
    assert!(current_generation > old_generation);
    assert!(service
        .peer_mgr
        .find_peer(&peer)
        .and_then(|cached| cached.fresh_net_profile(3_000_000))
        .is_some());
}

#[tokio::test]
async fn stale_reconcile_still_removes_a_genuinely_missing_authority() {
    let service = test_sn_service(allow_all_sn_connection_validator());
    let peer = test_id(71);
    let identity = test_identity_for_id(peer.clone(), Vec::new());
    let cert = identity.get_identity_cert().unwrap();
    service.peer_mgr.add_or_update_peer(&peer, &Some(cert), 0, Vec::new(), &Vec::new());

    let tunnel = CmdTunnelId::from(603);
    {
        let mut scheduler = service.nat_probe_scheduler.lock().unwrap();
        scheduler.set_ports(scheduler_probe_ports(33112));
        assert!(scheduler
            .observe_capable_report(
                &peer,
                tunnel,
                scheduler_endpoint(Protocol::Quic, 59003),
                Some(crate::sn::protocol::NAT_PROBE_CONTROL_VERSION),
                None,
                1,
            )
            .directive
            .is_some());
    }
    let (snapshot_tunnel, snapshot_generation) = service
        .nat_probe_scheduler
        .lock()
        .unwrap()
        .authority_registration(&peer)
        .unwrap();

    // The authority truly vanished and nothing replaced it, so reconciliation
    // still removes the stale registration.
    service.finish_nat_probe_authority_reconcile(
        &peer,
        snapshot_tunnel,
        snapshot_generation,
        false,
    );

    assert!(service
        .nat_probe_scheduler
        .lock()
        .unwrap()
        .authority_registration(&peer)
        .is_none());
}

#[test]
fn scheduler_publishes_fresh_client_profile_and_ignores_stale_or_unknown() {
    let sn = scheduler_peer(88);
    let peer = scheduler_peer(89);
    let tunnel = CmdTunnelId::from(899);
    let remote = scheduler_endpoint(Protocol::Quic, 59900);
    let mut scheduler = NatProbeScheduler::new(sn);
    scheduler.set_ports(vec![34001, 34002]);

    let registered = scheduler.observe_report(&peer, tunnel, remote, None, 12_000_000);
    assert!(registered.directive.is_some());

    let observed_at = 13_000_000;
    let profile = scheduler_profile(observed_at);
    let accepted = scheduler.observe_reported_profile(
        &peer,
        tunnel,
        remote,
        profile.clone(),
        observed_at,
    );
    assert!(accepted
        .profile_update
        .as_ref()
        .and_then(|profile| profile.as_ref())
        .is_some());
    assert_eq!(
        scheduler.current_profile(&peer, observed_at),
        Some(profile.clone())
    );

    let older = scheduler_profile(observed_at - 1);
    let ignored = scheduler.observe_reported_profile(&peer, tunnel, remote, older, observed_at);
    assert!(ignored.profile_update.is_none());
    assert_eq!(
        scheduler.current_profile(&peer, observed_at),
        Some(profile.clone())
    );

    let unknown = NatProfile::unknown();
    let ignored_unknown =
        scheduler.observe_reported_profile(&peer, tunnel, remote, unknown, observed_at);
    assert!(ignored_unknown.profile_update.is_none());
    assert_eq!(
        scheduler.current_profile(&peer, observed_at),
        Some(profile.clone())
    );
}

#[test]
fn nat_probe_scheduler_client_profile_requires_udp_authority_tunnel() {
    let sn = scheduler_peer(90);
    let peer = scheduler_peer(91);
    let authority_tunnel = CmdTunnelId::from(901);
    let concurrent_tunnel = CmdTunnelId::from(902);
    let authority_remote = scheduler_endpoint(Protocol::Quic, 60001);
    let mut scheduler = NatProbeScheduler::new(sn);
    scheduler.set_ports(vec![34101, 34102]);

    scheduler
        .observe_report(&peer, authority_tunnel, authority_remote, None, 20_000_000)
        .directive
        .unwrap();
    let observed_at = 21_000_000;
    let authority_profile = scheduler_profile(observed_at);
    let accepted = scheduler.observe_reported_profile(
        &peer,
        authority_tunnel,
        authority_remote,
        authority_profile.clone(),
        observed_at,
    );
    assert!(accepted
        .profile_update
        .as_ref()
        .and_then(|profile| profile.as_ref())
        .is_some());

    let concurrent_remote = scheduler_endpoint(Protocol::Quic, 60002);
    let concurrent_profile = scheduler_profile(observed_at + 1);
    let ignored_quic = scheduler.observe_reported_profile(
        &peer,
        concurrent_tunnel,
        concurrent_remote,
        concurrent_profile.clone(),
        observed_at + 1,
    );
    assert!(ignored_quic.profile_update.is_none());
    assert_eq!(
        scheduler.current_profile(&peer, observed_at + 1),
        Some(authority_profile.clone())
    );

    let tcp_remote = scheduler_endpoint(Protocol::Tcp, 60003);
    let ignored_tcp = scheduler.observe_reported_profile(
        &peer,
        concurrent_tunnel,
        tcp_remote,
        concurrent_profile.clone(),
        observed_at + 2,
    );
    assert!(ignored_tcp.profile_update.is_none());
    assert_eq!(
        scheduler.current_profile(&peer, observed_at + 2),
        Some(authority_profile)
    );
}

#[test]
fn nat_probe_scheduler_accepts_any_udp_protocol_as_authority_client_profile() {
    let sn = scheduler_peer(92);
    let peer = scheduler_peer(93);
    let tunnel = CmdTunnelId::from(903);
    let remote = scheduler_endpoint(Protocol::Ext(1), 60011);
    let mut scheduler = NatProbeScheduler::new(sn);
    scheduler.set_ports(vec![34111, 34112]);

    let registered = scheduler.observe_report(&peer, tunnel, remote, None, 30_000_000);
    assert!(registered.directive.is_some());
    assert_eq!(scheduler.authority_tunnel(&peer), Some(tunnel));

    let observed_at = 31_000_000;
    let profile = scheduler_profile(observed_at);
    let accepted = scheduler.observe_reported_profile(
        &peer,
        tunnel,
        remote,
        profile.clone(),
        observed_at,
    );
    assert!(accepted
        .profile_update
        .as_ref()
        .and_then(|profile| profile.as_ref())
        .is_some());
    assert_eq!(scheduler.current_profile(&peer, observed_at), Some(profile));
}

#[test]
fn nat_probe_scheduler_accepts_multiplexed_stream_on_registered_observed_path() {
    let sn = scheduler_peer(94);
    let peer = scheduler_peer(95);
    let authority_tunnel = CmdTunnelId::from(904);
    let multiplexed_tunnel = CmdTunnelId::from(905);
    let remote = scheduler_endpoint(Protocol::Quic, 60021);
    let mut scheduler = NatProbeScheduler::new(sn);
    scheduler.set_ports(vec![34121, 34122]);

    let directive = scheduler
        .observe_report(&peer, authority_tunnel, remote, None, 40_000_000)
        .directive
        .unwrap();
    assert_eq!(scheduler.authority_tunnel(&peer), Some(authority_tunnel));

    // The client multiplexes probe results and periodic profiles over any
    // command stream opened on the same bearer, so a different command stream
    // id on the registered observed path must stay authoritative.
    let completed_at = 41_000_000;
    let result = NatProbeResult::from_directive(&directive, scheduler_profile(completed_at));
    let completed = scheduler.observe_report(
        &peer,
        multiplexed_tunnel,
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
    assert_eq!(scheduler.authority_tunnel(&peer), Some(authority_tunnel));

    let reported_at = completed_at + 1_000_000;
    let profile = scheduler_profile(reported_at);
    let reported = scheduler.observe_reported_profile(
        &peer,
        multiplexed_tunnel,
        remote,
        profile.clone(),
        reported_at,
    );
    assert!(reported
        .profile_update
        .as_ref()
        .and_then(|profile| profile.as_ref())
        .is_some());
    assert_eq!(scheduler.current_profile(&peer, reported_at), Some(profile));

    // A command stream on a different observed path still must not take over
    // the registration.
    let other_path = scheduler.observe_report(
        &peer,
        CmdTunnelId::from(906),
        scheduler_endpoint(Protocol::Quic, 60022),
        None,
        reported_at + 1_000_000,
    );
    assert!(other_path.directive.is_none());
    assert!(other_path.profile_update.is_none());
    assert_eq!(scheduler.authority_tunnel(&peer), Some(authority_tunnel));

    // The same address on a different transport protocol is a different path.
    let other_protocol = scheduler.observe_report(
        &peer,
        CmdTunnelId::from(907),
        scheduler_endpoint(Protocol::Ext(1), 60021),
        None,
        reported_at + 2_000_000,
    );
    assert!(other_protocol.directive.is_none());
    assert!(other_protocol.profile_update.is_none());
    assert_eq!(scheduler.authority_tunnel(&peer), Some(authority_tunnel));
}

#[test]
fn nat_probe_scheduler_observed_path_identity_requires_protocol_and_address() {
    let quic = scheduler_endpoint(Protocol::Quic, 60031);
    let same_address_ext = scheduler_endpoint(Protocol::Ext(1), 60031);
    let other_address = scheduler_endpoint(Protocol::Quic, 60032);

    assert!(NatProbeScheduler::same_observed_path(&quic, &quic));
    assert!(!NatProbeScheduler::same_observed_path(
        &quic,
        &same_address_ext
    ));
    assert!(!NatProbeScheduler::same_observed_path(
        &quic,
        &other_address
    ));
}
