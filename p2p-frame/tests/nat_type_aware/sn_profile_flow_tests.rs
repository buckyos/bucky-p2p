use super::*;
use crate::endpoint::EndpointArea;
use crate::nat_type::{NatMappingObservation, NatProfile, NatTraversalContext};
use bucky_time::bucky_time_now;

#[test]
fn active_sn_profiles_are_kept_per_sn_id() {
    use crate::sn::client::{ActiveSN, SNServiceState};

    let now = bucky_time_now();
    let mut first_observed = localhost_quic_endpoint(45001);
    first_observed.set_area(EndpointArea::ServerReflexive);
    let mut second_observed = localhost_quic_endpoint(45002);
    second_observed.set_area(EndpointArea::ServerReflexive);
    let first_profile = NatProfile::from_observations(
        &[first_observed, first_observed],
        now,
        Duration::from_secs(10),
    );
    let second_profile = NatProfile::from_observations(
        &[second_observed, second_observed],
        now,
        Duration::from_secs(10),
    );
    let first_sn = P2pId::from(vec![41; 32]);
    let second_sn = P2pId::from(vec![42; 32]);
    let state = SNServiceState {
        pinging_handle: None,
        active_sn_list: vec![
            ActiveSN {
                sn_peer_id: first_sn.clone(),
                latest_time: now,
                conn_id: 1u32.into(),
                protocol: Protocol::Quic,
                sn_endpoint: localhost_quic_endpoint(46001),
                wan_ep_list: vec![],
                nat_probe_endpoints: vec![],
                nat_probe_signer: None,
                net_profile: first_profile.clone(),
                nat_probe_registration_generation: 1,
                last_nat_probe_request_id: 1,
            },
            ActiveSN {
                sn_peer_id: second_sn.clone(),
                latest_time: now,
                conn_id: 2u32.into(),
                protocol: Protocol::Quic,
                sn_endpoint: localhost_quic_endpoint(46002),
                wan_ep_list: vec![],
                nat_probe_endpoints: vec![],
                nat_probe_signer: None,
                net_profile: second_profile.clone(),
                nat_probe_registration_generation: 1,
                last_nat_probe_request_id: 1,
            },
        ],
        latest_sn_interval: 0,
    };

    assert_eq!(
        state
            .active_sn_list
            .iter()
            .find(|active| active.sn_peer_id == first_sn)
            .unwrap()
            .net_profile,
        first_profile
    );
    assert_eq!(
        state
            .active_sn_list
            .iter()
            .find(|active| active.sn_peer_id == second_sn)
            .unwrap()
            .net_profile,
        second_profile
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_reflectors_bind_all_wildcard_ports_before_spawning_any_task() {
    let identity_factory = Arc::new(X509IdentityFactory);
    let cert_factory = Arc::new(X509IdentityCertFactory);
    let sn_identity = build_identity(
        "nat-probe-atomic-bind-sn",
        localhost_quic_endpoint(next_port()),
    );
    let first_reservation = std::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).unwrap();
    let first_port = first_reservation.local_addr().unwrap().port();
    let second_reservation = std::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).unwrap();
    let second_port = second_reservation.local_addr().unwrap().port();
    drop(first_reservation);

    let server = create_sn_service(
        SnServiceConfig::new(
            sn_identity,
            identity_factory,
            cert_factory,
            test_server_runtime(),
        )
        .set_nat_probe_ports(vec![first_port, second_port]),
    )
    .await
    .unwrap();
    let error = server.start().await.unwrap_err();
    assert_eq!(error.code(), P2pErrorCode::IoError);

    let first_rebind = std::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, first_port))
        .expect("a failed later bind must release the earlier wildcard reflector socket");
    assert_eq!(first_rebind.local_addr().unwrap().port(), first_port);
    drop(first_rebind);
    drop(second_reservation);
    server.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn first_query_returns_remote_profile_and_call_forwards_exact_snapshot() {
    let identity_factory = Arc::new(X509IdentityFactory);
    let cert_factory = Arc::new(X509IdentityCertFactory);
    let mut sn_endpoint = localhost_quic_endpoint(next_port());
    sn_endpoint.set_area(EndpointArea::Wan);
    let sn_identity = build_identity("nat-profile-sn", sn_endpoint);
    let sn_id = sn_identity.get_id();
    let probe_ports = vec![next_port(), next_port()];
    let sn_service = create_sn_service(
        SnServiceConfig::new(
            sn_identity.clone(),
            identity_factory.clone(),
            cert_factory.clone(),
            test_server_runtime(),
        )
        .set_nat_probe_ports(probe_ports.clone()),
    )
    .await
    .unwrap();
    sn_service.start().await.unwrap();
    let sn_list = vec![build_sn_entry(&sn_identity)];

    let caller_identity = build_identity("nat-caller", localhost_quic_endpoint(next_port()));
    let caller_id = caller_identity.get_id();
    let caller = start_client_stack(
        caller_identity,
        sn_list.clone(),
        identity_factory.clone(),
        cert_factory.clone(),
    )
    .await
    .unwrap();
    let callee_identity = build_identity("nat-callee", localhost_quic_endpoint(next_port()));
    let callee_id = callee_identity.get_id();
    let callee = start_client_stack(callee_identity, sn_list, identity_factory, cert_factory)
        .await
        .unwrap();
    caller.wait_online(Some(ONLINE_TIMEOUT)).await.unwrap();
    callee.wait_online(Some(ONLINE_TIMEOUT)).await.unwrap();

    let active = caller
        .sn_client()
        .get_active_sn_list()
        .into_iter()
        .find(|active| active.sn_peer_id == sn_id)
        .expect("authenticated SN report must publish an ActiveSN");
    let active_signer = active
        .nat_probe_signer
        .expect("authenticated SN report must publish a trusted PNAT signer");
    assert_eq!(active_signer.get_id(), sn_id);
    assert_eq!(active.sn_endpoint, sn_endpoint);
    assert_eq!(active.nat_probe_endpoints.len(), probe_ports.len());
    for endpoint in &active.nat_probe_endpoints {
        assert_eq!(endpoint.protocol(), Protocol::Quic);
        assert_eq!(endpoint.addr().ip(), sn_endpoint.addr().ip());
        assert_eq!(endpoint.get_area(), EndpointArea::Wan);
    }
    let mut actual_ports: Vec<u16> = active
        .nat_probe_endpoints
        .iter()
        .map(|endpoint| endpoint.addr().port())
        .collect();
    actual_ports.sort_unstable();
    let mut expected_ports = probe_ports;
    expected_ports.sort_unstable();
    assert_eq!(actual_ports, expected_ports);

    let query = caller
        .sn_client()
        .query_with_context(&callee_id)
        .await
        .unwrap();
    assert_eq!(query.sn_peer_id, sn_id);
    assert_eq!(
        query.local_net_profile.observation,
        NatMappingObservation::NonSymmetricLike
    );
    let remote_profile = query.response.net_profile.clone().unwrap();
    assert_eq!(
        remote_profile.observation,
        NatMappingObservation::NonSymmetricLike
    );
    let context = NatTraversalContext::new(
        caller_id.clone(),
        callee_id.clone(),
        query.local_net_profile,
        remote_profile,
    );

    let (tx, mut rx) = mpsc::channel::<SnCalled>(1);
    callee.sn_client().set_listener(move |called: SnCalled| {
        let tx = tx.clone();
        async move {
            tx.send(called).await.unwrap();
            Ok(())
        }
    });
    let response = caller
        .sn_client()
        .call_via_sn(
            &sn_id,
            0x7701u32.into(),
            None,
            &callee_id,
            TunnelType::Stream,
            vec![],
            Some(&context),
        )
        .await
        .unwrap();
    assert_eq!(response.result, P2pErrorCode::Ok.as_u8());
    let called = tokio::time::timeout(CALL_TIMEOUT, rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(called.sn_peer_id, sn_id);
    assert_eq!(called.nat_context, Some(context));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn periodic_deadline_issues_exactly_one_real_probe_directive() {
    super::enable_nat_probe_test_logging();
    let log_start = super::nat_probe_test_logs().len();
    let identity_factory = Arc::new(X509IdentityFactory);
    let cert_factory = Arc::new(X509IdentityCertFactory);
    let mut sn_endpoint = localhost_quic_endpoint(next_port());
    sn_endpoint.set_area(EndpointArea::Wan);
    let sn_identity = build_identity("nat-periodic-sn", sn_endpoint);
    let sn_id = sn_identity.get_id();
    let sn_service = create_sn_service(
        SnServiceConfig::new(
            sn_identity.clone(),
            identity_factory.clone(),
            cert_factory.clone(),
            test_server_runtime(),
        )
        .set_nat_probe_ports(vec![next_port(), next_port()]),
    )
    .await
    .unwrap();
    sn_service.start().await.unwrap();
    let client_identity =
        build_identity("nat-periodic-client", localhost_quic_endpoint(next_port()));
    let client_id = client_identity.get_id();
    let client = start_client_stack(
        client_identity,
        vec![build_sn_entry(&sn_identity)],
        identity_factory,
        cert_factory,
    )
    .await
    .unwrap();
    client.wait_online(Some(ONLINE_TIMEOUT)).await.unwrap();

    let active = tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            if let Some(active) =
                client
                    .sn_client()
                    .get_active_sn_list()
                    .into_iter()
                    .find(|active| {
                        active.sn_peer_id == sn_id
                            && active.net_profile.observation != NatMappingObservation::Unknown
                    })
            {
                break active;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap();
    let peer_text = client_id.to_string();
    let initial_logs: Vec<String> = super::nat_probe_test_logs()[log_start..]
        .iter()
        .map(|(_, message)| message)
        .filter(|message| message.contains(&peer_text))
        .cloned()
        .collect();
    let initial_has = |needle: &str| initial_logs.iter().any(|message| message.contains(needle));
    assert!(initial_has("event=nat_probe_authority_established"));
    assert!(initial_has("event=nat_probe_directive_issued"));
    assert!(initial_has("trigger=online"));
    assert!(initial_has("event=nat_probe_client_started"));
    assert!(initial_has("event=nat_probe_client_completed"));
    assert!(initial_has("event=nat_probe_result_reported"));
    assert!(initial_has("event=nat_probe_result_accepted"));
    assert!(initial_logs.iter().any(|message| {
        message.contains(&format!(
            "registration_generation={}",
            active.nat_probe_registration_generation
        )) && message.contains(&format!("request_id={}", active.last_nat_probe_request_id))
    }));
    let forced_at = bucky_time_now();
    assert!(
        sn_service
            .service()
            .force_nat_probe_period_due_for_test(&client_id, forced_at)
    );

    let mut due = client
        .sn_client()
        .report_for_test(active.conn_id, sn_id.clone(), None)
        .await
        .unwrap();
    let directive = due
        .nat_probe_directive
        .take()
        .expect("forced periodic deadline must issue one directive");
    let result = client
        .sn_client()
        .execute_probe_directive_for_test(
            sn_id.clone(),
            active.protocol,
            active.nat_probe_registration_generation,
            active.last_nat_probe_request_id,
            Some(directive),
        )
        .await
        .expect("periodic directive must pass the client gate");
    assert_ne!(result.profile.observation, NatMappingObservation::Unknown);
    let completed = client
        .sn_client()
        .report_for_test(active.conn_id, sn_id.clone(), Some(&result))
        .await
        .unwrap();
    assert!(completed.nat_probe_directive.is_none());
    let lifecycle_logs: Vec<(log::Level, String)> = super::nat_probe_test_logs()[log_start..]
        .iter()
        .filter(|(_, message)| message.contains(&peer_text))
        .cloned()
        .collect();
    assert!(lifecycle_logs.iter().any(|(_, message)| {
        message.contains("event=nat_probe_directive_issued")
            && message.contains("trigger=periodic")
            && message.contains(&format!("request_id={}", result.request_id))
    }));
    for (level, message) in &lifecycle_logs {
        for forbidden in [
            "certificate=",
            "client_cert=",
            "private_key=",
            "secret=",
            "token=",
            "payload=",
            "packet_body=",
            "raw_bytes=",
        ] {
            assert!(!message.contains(forbidden), "forbidden field in {message}");
        }
        if *level <= log::Level::Info {
            assert!(
                !message.contains("endpoints="),
                "endpoint list in {message}"
            );
            assert!(
                !message.contains("remote_endpoint="),
                "endpoint value in {message}"
            );
        }
    }
    let high_level_before_stable = lifecycle_logs
        .iter()
        .filter(|(level, _)| *level <= log::Level::Info)
        .count();
    let stable = client
        .sn_client()
        .report_for_test(active.conn_id, sn_id, None)
        .await
        .unwrap();
    assert!(stable.nat_probe_directive.is_none());
    tokio::time::sleep(Duration::from_millis(350)).await;
    let high_level_after_stable = super::nat_probe_test_logs()[log_start..]
        .iter()
        .filter(|(level, message)| *level <= log::Level::Info && message.contains(&peer_text))
        .count();
    assert_eq!(
        high_level_after_stable, high_level_before_stable,
        "stable reports and maintenance ticks must not add info/warn NAT probe logs"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn initial_probe_and_result_report_failure_do_not_gate_online() {
    super::enable_nat_probe_test_logging();
    let log_start = super::nat_probe_test_logs().len();
    let identity_factory = Arc::new(X509IdentityFactory);
    let cert_factory = Arc::new(X509IdentityCertFactory);
    let sn_identity = build_identity(
        "nat-online-before-probe",
        localhost_quic_endpoint(next_port()),
    );
    let sn_id = sn_identity.get_id();
    let sn_service = create_sn_service(SnServiceConfig::new(
        sn_identity.clone(),
        identity_factory.clone(),
        cert_factory.clone(),
        test_server_runtime(),
    ))
    .await
    .unwrap();
    let result_report_seen = Arc::new(AtomicUsize::new(0));
    let result_report_started = Arc::new(tokio::sync::Notify::new());
    let result_report_release = Arc::new(tokio::sync::Notify::new());
    let seen = result_report_seen.clone();
    let started = result_report_started.clone();
    let release = result_report_release.clone();
    let handler_sn_id = sn_id.clone();
    sn_service.get_cmd_server().register_cmd_handler(
        PackageCmdCode::ReportSn as u8,
        move |_local_id,
              peer_id: sfo_cmd_server::PeerId,
              _tunnel_id,
              _header,
              mut body: CmdBody| {
            let seen = seen.clone();
            let started = started.clone();
            let release = release.clone();
            let sn_id = handler_sn_id.clone();
            async move {
                let report = ReportSn::clone_from_slice(body.read_all().await?.as_slice()).unwrap();
                if report.nat_probe_result.is_some() {
                    seen.fetch_add(1, Ordering::SeqCst);
                    started.notify_one();
                    release.notified().await;
                    return Ok(Some(CmdBody::from(vec![0xff])));
                }
                let directive = crate::sn::protocol::NatProbeDirective {
                    version: crate::sn::protocol::NAT_PROBE_CONTROL_VERSION,
                    sn_peer_id: sn_id.clone(),
                    peer_id: P2pId::from(peer_id.as_slice()),
                    registration_generation: 1,
                    request_id: 1,
                    probe_config_generation: 1,
                    expires_at: bucky_time_now() + Duration::from_secs(30).as_micros() as u64,
                    ports: vec![39001, 39002],
                };
                let response = ReportSnResp {
                    seq: report.seq,
                    sn_peer_id: sn_id,
                    result: P2pErrorCode::Ok.as_u8(),
                    peer_info: None,
                    end_point_array: vec![],
                    receipt: None,
                    nat_probe_ports: vec![],
                    nat_probe_directive: Some(directive),
                };
                Ok(Some(CmdBody::from(response.to_vec().unwrap())))
            }
        },
    );
    sn_service.start().await.unwrap();

    let client_identity = build_identity("nat-online-client", localhost_quic_endpoint(next_port()));
    let client_id = client_identity.get_id();
    let client = start_client_stack(
        client_identity,
        vec![build_sn_entry(&sn_identity)],
        identity_factory,
        cert_factory,
    )
    .await
    .unwrap();

    client.wait_online(Some(ONLINE_TIMEOUT)).await.unwrap();
    tokio::time::timeout(Duration::from_secs(8), result_report_started.notified())
        .await
        .expect("NAT probe result report did not start");
    assert_eq!(result_report_seen.load(Ordering::SeqCst), 1);
    assert_eq!(client.sn_client().get_active_sn_list().len(), 1);
    result_report_release.notify_one();
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if super::nat_probe_test_logs()[log_start..]
                .iter()
                .any(|(level, message)| {
                    *level == log::Level::Warn
                        && message.contains("event=nat_probe_result_report_failed")
                        && message.contains(&client_id.to_string())
                        && message.contains("registration_generation=1")
                        && message.contains("request_id=1")
                })
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("failed result report must emit a correlated warn event");
    assert_eq!(client.sn_client().get_active_sn_list().len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tcp_only_registration_never_receives_or_executes_probe() {
    super::enable_nat_probe_test_logging();
    let log_start = super::nat_probe_test_logs().len();
    let identity_factory = Arc::new(X509IdentityFactory);
    let cert_factory = Arc::new(X509IdentityCertFactory);
    let mut advertised_quic = localhost_quic_endpoint(next_port());
    advertised_quic.set_area(EndpointArea::Wan);
    let sn_identity = build_identity("nat-tcp-only-sn", advertised_quic)
        .update_endpoints(vec![advertised_quic, localhost_tcp_endpoint(next_port())]);
    let sn_service = create_sn_service(
        SnServiceConfig::new(
            sn_identity.clone(),
            identity_factory.clone(),
            cert_factory.clone(),
            test_server_runtime(),
        )
        .set_nat_probe_ports(vec![next_port(), next_port()]),
    )
    .await
    .unwrap();
    sn_service.start().await.unwrap();
    let client_identity =
        build_identity("nat-tcp-only-client", localhost_tcp_endpoint(next_port()));
    let client_id = client_identity.get_id();
    let sn_cert = sn_identity.get_identity_cert().unwrap();
    let tcp_only_sn = P2pSn::new(
        sn_cert.get_id(),
        sn_cert.get_name(),
        vec![
            *sn_identity
                .endpoints()
                .iter()
                .find(|endpoint| endpoint.protocol() == Protocol::Tcp)
                .unwrap(),
        ],
    );
    let client = start_client_stack(
        client_identity,
        vec![tcp_only_sn],
        identity_factory,
        cert_factory,
    )
    .await
    .unwrap();

    let tcp_sn_endpoint = *sn_identity
        .endpoints()
        .iter()
        .find(|endpoint| endpoint.protocol() == Protocol::Tcp)
        .unwrap();
    let tunnel_id = client
        .sn_client()
        .get_cmd_client()
        .find_tunnel_id_by_classified(SnTunnelClassification::new(None, tcp_sn_endpoint))
        .await
        .unwrap();
    let client_identity = client.local_identity();
    let report = ReportSn {
        protocol_version: 0,
        stack_version: 0,
        seq: 0x9901u32.into(),
        sn_peer_id: sn_identity.get_id(),
        from_peer_id: Some(client_identity.get_id()),
        peer_info: Some(
            client_identity
                .get_identity_cert()
                .unwrap()
                .get_encoded_cert()
                .unwrap(),
        ),
        send_time: bucky_time_now(),
        contract_id: None,
        receipt: None,
        map_ports: vec![],
        local_eps: client_identity.endpoints(),
        net_profile: None,
        nat_probe_control_version: Some(crate::sn::protocol::NAT_PROBE_CONTROL_VERSION),
        nat_probe_result: None,
    };
    let mut response = client
        .sn_client()
        .get_cmd_client()
        .send_by_specify_tunnel_with_resp(
            tunnel_id,
            PackageCmdCode::ReportSn as u8,
            0,
            report.to_vec().unwrap().as_slice(),
            Duration::from_secs(3),
        )
        .await
        .unwrap();
    let response =
        ReportSnResp::clone_from_slice(response.read_all().await.unwrap().as_slice()).unwrap();
    assert_eq!(response.result, P2pErrorCode::Ok.as_u8());
    assert!(response.nat_probe_directive.is_none());

    let query = SnQuery {
        protocol_version: 0,
        stack_version: 0,
        seq: 0x9902u32.into(),
        query_id: client_identity.get_id(),
    };
    let mut query_response = client
        .sn_client()
        .get_cmd_client()
        .send_by_specify_tunnel_with_resp(
            tunnel_id,
            PackageCmdCode::SnQuery as u8,
            0,
            query.to_vec().unwrap().as_slice(),
            Duration::from_secs(3),
        )
        .await
        .unwrap();
    let query_response =
        SnQueryResp::clone_from_slice(query_response.read_all().await.unwrap().as_slice()).unwrap();
    assert!(query_response.peer_info.is_some());
    assert!(query_response.net_profile.is_none());
    assert!(
        !super::nat_probe_test_logs()[log_start..]
            .iter()
            .any(|(_, message)| {
                message.contains("event=nat_probe_client_started")
                    && message.contains(&client_id.to_string())
            })
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn legacy_quic_client_without_capability_never_receives_demand_directive() {
    let identity_factory = Arc::new(X509IdentityFactory);
    let cert_factory = Arc::new(X509IdentityCertFactory);
    let mut sn_endpoint = localhost_quic_endpoint(next_port());
    sn_endpoint.set_area(EndpointArea::Wan);
    let sn_identity = build_identity("nat-legacy-sn", sn_endpoint);
    let sn_id = sn_identity.get_id();
    let sn_service = create_sn_service(
        SnServiceConfig::new(
            sn_identity.clone(),
            identity_factory.clone(),
            cert_factory.clone(),
            test_server_runtime(),
        )
        .set_nat_probe_ports(vec![next_port(), next_port()]),
    )
    .await
    .unwrap();
    sn_service.start().await.unwrap();
    let client_identity = build_identity("nat-legacy-client", localhost_quic_endpoint(next_port()));
    let client = start_client_stack(
        client_identity,
        vec![build_sn_entry(&sn_identity)],
        identity_factory,
        cert_factory,
    )
    .await
    .unwrap();
    client.sn_client().stop();
    let tunnel_id = client
        .sn_client()
        .get_cmd_client()
        .find_tunnel_id_by_classified(SnTunnelClassification::new(None, sn_endpoint))
        .await
        .unwrap();
    let local_identity = client.local_identity();
    let make_report = |seq: u32| ReportSn {
        protocol_version: 0,
        stack_version: 0,
        seq: seq.into(),
        sn_peer_id: sn_id.clone(),
        from_peer_id: Some(local_identity.get_id()),
        peer_info: Some(
            local_identity
                .get_identity_cert()
                .unwrap()
                .get_encoded_cert()
                .unwrap(),
        ),
        send_time: bucky_time_now(),
        contract_id: None,
        receipt: None,
        map_ports: vec![],
        local_eps: local_identity.endpoints(),
        net_profile: None,
        nat_probe_control_version: None,
        nat_probe_result: None,
    };
    let send_report = |report: ReportSn| {
        let cmd_client = client.sn_client().get_cmd_client().clone();
        async move {
            let mut response = cmd_client
                .send_by_specify_tunnel_with_resp(
                    tunnel_id,
                    PackageCmdCode::ReportSn as u8,
                    0,
                    report.to_vec().unwrap().as_slice(),
                    Duration::from_secs(3),
                )
                .await
                .unwrap();
            ReportSnResp::clone_from_slice(response.read_all().await.unwrap().as_slice()).unwrap()
        }
    };
    assert!(
        send_report(make_report(0x9911))
            .await
            .nat_probe_directive
            .is_none()
    );

    let query = SnQuery {
        protocol_version: 0,
        stack_version: 0,
        seq: 0x9912u32.into(),
        query_id: local_identity.get_id(),
    };
    let mut query_response = client
        .sn_client()
        .get_cmd_client()
        .send_by_specify_tunnel_with_resp(
            tunnel_id,
            PackageCmdCode::SnQuery as u8,
            0,
            query.to_vec().unwrap().as_slice(),
            Duration::from_secs(3),
        )
        .await
        .unwrap();
    let query_response =
        SnQueryResp::clone_from_slice(query_response.read_all().await.unwrap().as_slice()).unwrap();
    assert!(query_response.net_profile.is_none());
    assert!(
        send_report(make_report(0x9913))
            .await
            .nat_probe_directive
            .is_none()
    );
}
