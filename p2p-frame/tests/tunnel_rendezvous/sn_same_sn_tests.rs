use super::*;
use crate::endpoint::EndpointArea;
use crate::sn::client::SnTunnelRendezvousActionAck;
use crate::sn::protocol::{
    NAT_PROBE_CONTROL_VERSION, ReportSnResp, SN_PROTOCOL_VERSION, SnTunnelRendezvousNotify,
    SnTunnelRendezvousOperation,
};
use tokio::sync::Semaphore;

async fn send_report_with_local_endpoints(
    stack: &P2pStackRef,
    sn_id: &P2pId,
    local_eps: Vec<Endpoint>,
    seq: u32,
) -> P2pResult<ReportSnResp> {
    let active = stack
        .sn_client()
        .get_active_sn_list()
        .into_iter()
        .find(|active| &active.sn_peer_id == sn_id)
        .expect("test client has an authenticated active SN tunnel");
    let report = ReportSn {
        protocol_version: SN_PROTOCOL_VERSION,
        stack_version: 0,
        seq: Sequence::from(seq),
        sn_peer_id: sn_id.clone(),
        from_peer_id: Some(stack.local_identity().get_id()),
        peer_info: Some(
            stack
                .local_identity()
                .get_identity_cert()?
                .get_encoded_cert()?,
        ),
        send_time: bucky_time::bucky_time_now(),
        contract_id: None,
        receipt: None,
        map_ports: Vec::new(),
        local_eps,
        net_profile: None,
        nat_probe_control_version: Some(NAT_PROBE_CONTROL_VERSION),
        nat_probe_result: None,
    };
    let report_body = report
        .to_vec()
        .map_err(|error| crate::error::P2pError::new(P2pErrorCode::RawCodecError, error.to_string()))?;
    let mut response_body = stack
        .sn_client()
        .get_cmd_client()
        .send_by_specify_tunnel_with_resp(
            active.conn_id,
            PackageCmdCode::ReportSn as u8,
            0,
            report_body.as_slice(),
            CALL_TIMEOUT,
        )
        .await
        .map_err(|error| crate::error::P2pError::new(P2pErrorCode::Failed, error.to_string()))?;
    let response = ReportSnResp::clone_from_slice(
        response_body
            .read_all()
            .await
            .map_err(|error| crate::error::P2pError::new(P2pErrorCode::IoError, error.to_string()))?
            .as_slice(),
    )
    .map_err(|error| crate::error::P2pError::new(P2pErrorCode::RawCodecError, error.to_string()))?;
    assert_eq!(response.seq, Sequence::from(seq));
    assert_eq!(response.sn_peer_id, *sn_id);
    Ok(response)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn same_sn_rendezvous_arms_target_before_success_and_deduplicates() {
    let (_sn_service, caller, caller_id, target, target_id, sn_id, cert_factory) =
        setup_sn_and_two_clients().await;
    let callback_count = Arc::new(AtomicUsize::new(0));
    let observed_count = callback_count.clone();
    let expected_caller_id = caller_id.clone();
    let expected_sn_id = sn_id.clone();
    let cert_factory_for_listener = cert_factory.clone();
    let action_release = Arc::new(Semaphore::new(0));
    let listener_release = action_release.clone();
    let (action_tx, mut action_rx) = mpsc::channel::<SnTunnelRendezvousNotify>(1);

    target.sn_client().set_rendezvous_listener(
        move |notify: SnTunnelRendezvousNotify, serving_sn_id: P2pId| {
            let observed_count = observed_count.clone();
            let expected_caller_id = expected_caller_id.clone();
            let expected_sn_id = expected_sn_id.clone();
            let cert_factory = cert_factory_for_listener.clone();
            let listener_release = listener_release.clone();
            let action_tx = action_tx.clone();
            async move {
                assert_eq!(serving_sn_id, expected_sn_id);
                assert_eq!(notify.operation, SnTunnelRendezvousOperation::WaitIncoming);
                assert!(notify.end_point_array.is_empty());
                assert!(!notify.need_predict_endpoint);
                assert_eq!(
                    cert_factory.create(&notify.peer_info)?.get_id(),
                    expected_caller_id
                );
                observed_count.fetch_add(1, Ordering::SeqCst);
                action_tx.send(notify).await.unwrap();
                let _permit = listener_release.acquire().await.unwrap();
                Ok(SnTunnelRendezvousActionAck::without_prediction())
            }
        },
    );

    let request = caller
        .sn_client()
        .new_rendezvous_request(
            TunnelId::from(0x2402),
            &target_id,
            SnTunnelRendezvousOperation::WaitIncoming,
            Vec::new(),
            false,
        )
        .unwrap();
    let caller_for_request = caller.clone();
    let sn_for_request = sn_id.clone();
    let request_for_task = request.clone();
    let response_task = tokio::spawn(async move {
        caller_for_request
            .sn_client()
            .rendezvous_via_sn(&sn_for_request, &request_for_task)
            .await
    });

    let notify = tokio::time::timeout(CALL_TIMEOUT, action_rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(notify.seq, request.seq);
    assert_eq!(notify.tunnel_id, request.tunnel_id);
    assert!(!response_task.is_finished());
    action_release.add_permits(1);

    let response = response_task.await.unwrap().unwrap();
    assert!(response.is_success());
    assert_eq!(response.seq, request.seq);
    assert!(response.predicted_endpoint_array.is_empty());
    assert_eq!(callback_count.load(Ordering::SeqCst), 1);

    let replay = caller
        .sn_client()
        .rendezvous_via_sn(&sn_id, &request)
        .await
        .unwrap();
    assert_eq!(replay, response);
    assert_eq!(callback_count.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn same_sn_rendezvous_rejects_third_party_endpoints_and_returns_generic_failure() {
    let (_sn_service, caller, caller_id, target, target_id, sn_id, _cert_factory) =
        setup_sn_and_two_clients().await;
    let callback_count = Arc::new(AtomicUsize::new(0));
    let observed_count = callback_count.clone();
    target.sn_client().set_rendezvous_listener(
        move |_notify: SnTunnelRendezvousNotify, _serving_sn_id: P2pId| {
            let observed_count = observed_count.clone();
            async move {
                observed_count.fetch_add(1, Ordering::SeqCst);
                Ok(SnTunnelRendezvousActionAck::without_prediction())
            }
        },
    );

    let mut third_party_endpoint = Endpoint::from((
        Protocol::Quic,
        "192.0.2.44:42444".parse::<SocketAddr>().unwrap(),
    ));
    third_party_endpoint.set_area(EndpointArea::ServerReflexive);
    let report_response = send_report_with_local_endpoints(
        &caller,
        &sn_id,
        vec![third_party_endpoint],
        0x5301,
    )
    .await
    .unwrap();
    assert_eq!(report_response.result, P2pErrorCode::Ok.as_u8());
    let cached = target.sn_client().query(&caller_id).await.unwrap();
    assert!(cached.peer_info.is_some());
    assert!(
        cached
            .end_point_array
            .iter()
            .all(|endpoint| endpoint.addr().ip() != third_party_endpoint.addr().ip()),
        "an authenticated peer must not persist its self-reported third-party public IP"
    );

    let third_party = caller
        .sn_client()
        .new_rendezvous_request(
            TunnelId::from(0x2403),
            &target_id,
            SnTunnelRendezvousOperation::PunchOnly,
            vec![third_party_endpoint],
            false,
        )
        .unwrap();
    let ownership_error = caller
        .sn_client()
        .rendezvous_via_sn(&sn_id, &third_party)
        .await
        .unwrap_err();
    assert_eq!(ownership_error.code(), P2pErrorCode::Failed);
    assert_eq!(callback_count.load(Ordering::SeqCst), 0);

    target.sn_client().set_rendezvous_listener(
        |_notify: SnTunnelRendezvousNotify, _serving_sn_id: P2pId| async move {
            Err(crate::error::P2pError::new(
                P2pErrorCode::InvalidData,
                "target action rejected".to_owned(),
            ))
        },
    );
    let rejected = caller
        .sn_client()
        .new_rendezvous_request(
            TunnelId::from(0x2404),
            &target_id,
            SnTunnelRendezvousOperation::WaitIncoming,
            Vec::new(),
            false,
        )
        .unwrap();
    let rejection = caller
        .sn_client()
        .rendezvous_via_sn(&sn_id, &rejected)
        .await
        .unwrap_err();
    assert_eq!(rejection.code(), P2pErrorCode::Failed);
}

#[cfg(feature = "test-real-socket-matrix")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn same_sn_rendezvous_accepts_only_current_control_tunnel_ip_with_any_port() {
    let (_sn_server, caller, _caller_id, target, target_id, sn_id, _cert_factory) =
        setup_sn_and_two_clients().await;
    let active = caller
        .sn_client()
        .get_active_sn_list()
        .into_iter()
        .find(|active| active.sn_peer_id == sn_id)
        .expect("caller has a current authenticated SN control tunnel");
    let observed = active
        .wan_ep_list
        .first()
        .copied()
        .expect("active SN report returned the observation for this control tunnel");
    let alternate_port = if observed.addr().port() == u16::MAX {
        observed.addr().port() - 1
    } else {
        observed.addr().port() + 1
    };
    let mut owned = Endpoint::from((Protocol::Quic, observed.addr().ip(), alternate_port));
    owned.set_area(EndpointArea::ServerReflexive);
    let callback_count = Arc::new(AtomicUsize::new(0));
    let observed_count = callback_count.clone();
    target.sn_client().set_rendezvous_listener(
        move |notify: SnTunnelRendezvousNotify, _serving_sn_id: P2pId| {
            let observed_count = observed_count.clone();
            async move {
                assert_eq!(notify.end_point_array, vec![owned]);
                observed_count.fetch_add(1, Ordering::SeqCst);
                Ok(SnTunnelRendezvousActionAck::without_prediction())
            }
        },
    );

    let accepted = caller
        .sn_client()
        .new_rendezvous_request(
            TunnelId::from(0x5302),
            &target_id,
            SnTunnelRendezvousOperation::PunchOnly,
            vec![owned],
            false,
        )
        .unwrap();
    assert!(caller
        .sn_client()
        .rendezvous_via_sn(&sn_id, &accepted)
        .await
        .unwrap()
        .is_success());
    assert_eq!(callback_count.load(Ordering::SeqCst), 1);

    let mut third_party = Endpoint::from((
        Protocol::Quic,
        "192.0.2.53:45303".parse::<SocketAddr>().unwrap(),
    ));
    third_party.set_area(EndpointArea::ServerReflexive);
    let mixed = caller
        .sn_client()
        .new_rendezvous_request(
            TunnelId::from(0x5303),
            &target_id,
            SnTunnelRendezvousOperation::PunchOnly,
            vec![owned, third_party],
            false,
        )
        .unwrap();
    let error = caller
        .sn_client()
        .rendezvous_via_sn(&sn_id, &mixed)
        .await
        .unwrap_err();
    assert_eq!(error.code(), P2pErrorCode::Failed);
    assert_eq!(callback_count.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn over_budget_report_does_not_replace_previously_cached_endpoints() {
    let (_sn_service, caller, caller_id, target, _target_id, sn_id, _cert_factory) =
        setup_sn_and_two_clients().await;
    let retained = Endpoint::from((
        Protocol::Tcp,
        "10.53.0.1:35301".parse::<SocketAddr>().unwrap(),
    ));
    send_report_with_local_endpoints(&caller, &sn_id, vec![retained], 0x5304)
        .await
        .unwrap();
    let before = target.sn_client().query(&caller_id).await.unwrap();
    assert!(before.end_point_array.contains(&retained));

    let oversized = (1..=33)
        .map(|last_octet| {
            Endpoint::from((
                Protocol::Quic,
                format!("10.53.1.{last_octet}:{}", 35_400 + last_octet)
                    .parse::<SocketAddr>()
                    .unwrap(),
            ))
        })
        .collect::<Vec<_>>();
    let error = send_report_with_local_endpoints(&caller, &sn_id, oversized.clone(), 0x5305)
        .await
        .unwrap_err();
    assert_eq!(error.code(), P2pErrorCode::Failed);

    let after = target.sn_client().query(&caller_id).await.unwrap();
    assert!(after.peer_info.is_some());
    assert!(after.end_point_array.contains(&retained));
    assert!(
        oversized
            .iter()
            .all(|endpoint| !after.end_point_array.contains(endpoint))
    );
}

#[cfg(feature = "test-real-socket-matrix")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn same_sn_rendezvous_response_owner_accepts_observed_ip_with_predicted_port() {
    let (sn_server, caller, _caller_id, target, target_id, sn_id, _cert_factory) =
        setup_sn_and_two_clients().await;
    let observed = sn_server
        .service()
        .get_peer_observed_ep(&sfo_cmd_server::PeerId::from(target_id.as_slice()))
        .await;
    let observed = observed
        .first()
        .copied()
        .expect("target has a current serving-SN observation");
    let predicted_port = if observed.addr().port() == u16::MAX {
        observed.addr().port() - 1
    } else {
        observed.addr().port() + 1
    };
    let mut predicted = Endpoint::from((Protocol::Quic, observed.addr().ip(), predicted_port));
    predicted.set_area(EndpointArea::ServerReflexive);
    target.sn_client().set_rendezvous_listener(
        move |_notify: SnTunnelRendezvousNotify, _serving_sn_id: P2pId| async move {
            Ok(SnTunnelRendezvousActionAck {
                predicted_endpoints: vec![predicted],
                socket_binding_generation: 1,
                valid_until: bucky_time::bucky_time_now() + 1_000_000,
            })
        },
    );

    let request = caller
        .sn_client()
        .new_rendezvous_request(
            TunnelId::from(0x2452),
            &target_id,
            SnTunnelRendezvousOperation::WaitIncoming,
            Vec::new(),
            true,
        )
        .unwrap();
    let response = caller
        .sn_client()
        .rendezvous_via_sn(&sn_id, &request)
        .await
        .unwrap();

    assert_eq!(response.predicted_endpoint_array, vec![predicted]);
    assert_ne!(predicted.addr().port(), observed.addr().port());
}

#[cfg(feature = "test-real-socket-matrix")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn same_sn_rendezvous_response_owner_rejects_mixed_unowned_prediction() {
    let (sn_server, caller, _caller_id, target, target_id, sn_id, _cert_factory) =
        setup_sn_and_two_clients().await;
    let observed = sn_server
        .service()
        .get_peer_observed_ep(&sfo_cmd_server::PeerId::from(target_id.as_slice()))
        .await;
    let observed = observed
        .first()
        .copied()
        .expect("target has a current serving-SN observation");
    let mut owned = Endpoint::from((Protocol::Quic, observed.addr().ip(), 42_551));
    owned.set_area(EndpointArea::ServerReflexive);
    let mut third_party =
        Endpoint::from((Protocol::Quic, "192.0.2.99:42552".parse::<SocketAddr>().unwrap()));
    third_party.set_area(EndpointArea::ServerReflexive);
    target.sn_client().set_rendezvous_listener(
        move |_notify: SnTunnelRendezvousNotify, _serving_sn_id: P2pId| async move {
            Ok(SnTunnelRendezvousActionAck {
                predicted_endpoints: vec![owned, third_party],
                socket_binding_generation: 1,
                valid_until: bucky_time::bucky_time_now() + 1_000_000,
            })
        },
    );

    let request = caller
        .sn_client()
        .new_rendezvous_request(
            TunnelId::from(0x2453),
            &target_id,
            SnTunnelRendezvousOperation::WaitIncoming,
            Vec::new(),
            true,
        )
        .unwrap();
    let error = caller
        .sn_client()
        .rendezvous_via_sn(&sn_id, &request)
        .await
        .unwrap_err();

    assert_eq!(error.code(), P2pErrorCode::Failed);
}
