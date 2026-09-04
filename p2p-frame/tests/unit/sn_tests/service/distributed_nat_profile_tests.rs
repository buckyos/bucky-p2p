use crate::nat_type::NatProfile;
use crate::sn::client::SnTunnelRendezvousActionAck;
use std::collections::HashMap;

fn reported_endpoint(protocol: Protocol, addr: &str, area: EndpointArea) -> Endpoint {
    let mut endpoint = Endpoint::from((protocol, addr.parse::<SocketAddr>().unwrap()));
    endpoint.set_area(area);
    endpoint
}

#[test]
fn reported_endpoint_sanitizer_retains_only_lan_and_observed_public_addresses() {
    let observed = reported_endpoint(
        Protocol::Quic,
        "198.51.100.7:41000",
        EndpointArea::ServerReflexive,
    );
    let inputs = vec![
        reported_endpoint(Protocol::Tcp, "10.1.2.3:1001", EndpointArea::Wan),
        reported_endpoint(
            Protocol::Tcp,
            "10.1.2.3:1001",
            EndpointArea::ServerReflexive,
        ),
        reported_endpoint(
            Protocol::Quic,
            "169.254.10.20:1002",
            EndpointArea::Mapped,
        ),
        reported_endpoint(Protocol::Tcp, "[fd00::1]:1003", EndpointArea::Wan),
        reported_endpoint(
            Protocol::Quic,
            "[fe80::1234]:1004",
            EndpointArea::ServerReflexive,
        ),
        reported_endpoint(
            Protocol::Quic,
            "198.51.100.7:1005",
            EndpointArea::Lan,
        ),
        reported_endpoint(Protocol::Quic, "10.9.8.7:0", EndpointArea::Lan),
        reported_endpoint(Protocol::Ext(8), "10.9.8.7:1006", EndpointArea::Lan),
        reported_endpoint(Protocol::Quic, "127.0.0.1:1007", EndpointArea::Wan),
        reported_endpoint(Protocol::Quic, "[::1]:1008", EndpointArea::Wan),
        reported_endpoint(Protocol::Quic, "0.0.0.0:1009", EndpointArea::Wan),
        reported_endpoint(Protocol::Quic, "[::]:1010", EndpointArea::Wan),
        reported_endpoint(Protocol::Quic, "224.0.0.1:1011", EndpointArea::Wan),
        reported_endpoint(Protocol::Quic, "[ff02::1]:1012", EndpointArea::Wan),
        reported_endpoint(
            Protocol::Quic,
            "255.255.255.255:1013",
            EndpointArea::Wan,
        ),
        reported_endpoint(
            Protocol::Quic,
            "203.0.113.9:1014",
            EndpointArea::ServerReflexive,
        ),
        reported_endpoint(
            Protocol::Quic,
            "[2001:4860:4860::8888]:1015",
            EndpointArea::Wan,
        ),
    ];

    let sanitized = SnService::sanitize_reported_endpoints(&inputs, Some(&observed)).unwrap();

    assert_eq!(sanitized.len(), 5);
    assert_eq!(sanitized[0], reported_endpoint(Protocol::Tcp, "10.1.2.3:1001", EndpointArea::Lan));
    assert_eq!(sanitized[1], reported_endpoint(Protocol::Quic, "169.254.10.20:1002", EndpointArea::Lan));
    assert_eq!(sanitized[2], reported_endpoint(Protocol::Tcp, "[fd00::1]:1003", EndpointArea::Lan));
    assert_eq!(sanitized[3], reported_endpoint(Protocol::Quic, "[fe80::1234]:1004", EndpointArea::Lan));
    assert_eq!(sanitized[4], reported_endpoint(Protocol::Quic, "198.51.100.7:1005", EndpointArea::Wan));
    assert!(
        sanitized[..4]
            .iter()
            .all(|endpoint| endpoint.get_area() == EndpointArea::Lan)
    );
    assert_eq!(sanitized[4].get_area(), EndpointArea::Wan);
}

#[test]
fn reported_endpoint_sanitizer_rejects_over_budget_report_atomically() {
    let reported = (0..=MAX_REPORTED_LOCAL_ENDPOINTS)
        .map(|index| {
            reported_endpoint(
                Protocol::Quic,
                &format!("10.10.0.{}:{}", index + 1, 20_000 + index),
                EndpointArea::Lan,
            )
        })
        .collect::<Vec<_>>();

    let error = SnService::sanitize_reported_endpoints(&reported, None).unwrap_err();

    assert_eq!(error.code(), P2pErrorCode::OutOfLimit);
}

#[tokio::test]
async fn rendezvous_initiator_ownership_requires_the_exact_current_command_tunnel() {
    let service = test_sn_service(allow_all_sn_connection_validator());
    let cached_endpoint = reported_endpoint(
        Protocol::Quic,
        "203.0.113.53:45300",
        EndpointArea::ServerReflexive,
    );
    let identity: P2pIdentityRef = Arc::new(
        crate::x509::generate_rsa_x509_identity(Some(
            "rendezvous-initiator-exact-tunnel".to_owned(),
        ))
        .unwrap(),
    );
    let identity = identity.update_endpoints(vec![cached_endpoint]);
    let cert = identity.get_identity_cert().unwrap();
    let peer_id = cert.get_id();
    let cmd_peer_id = PeerId::from(peer_id.as_slice());
    assert!(cert.endpoints().contains(&cached_endpoint));
    service.peer_mgr.add_or_update_peer(
        &peer_id,
        &Some(cert),
        SN_PROTOCOL_VERSION,
        Vec::new(),
        &vec![cached_endpoint],
    );
    let missing_tunnel_id = CmdTunnelId::from(0x5300_ffff);

    assert!(
        service
            .rendezvous_endpoints_owned_by(&cmd_peer_id, missing_tunnel_id, &[])
            .await
    );
    assert!(
        !service
            .rendezvous_endpoints_owned_by(
                &cmd_peer_id,
                missing_tunnel_id,
                &[cached_endpoint],
            )
            .await,
        "cached local endpoints and certificate endpoints cannot replace a current tunnel observation"
    );

    let local_endpoint = reported_endpoint(
        Protocol::Quic,
        "198.51.100.1:45301",
        EndpointArea::Wan,
    );
    let observed_endpoint = reported_endpoint(
        Protocol::Quic,
        "198.51.100.53:45302",
        EndpointArea::ServerReflexive,
    );
    let requested_on_observed_ip = reported_endpoint(
        Protocol::Quic,
        "198.51.100.53:45303",
        EndpointArea::ServerReflexive,
    );
    let ((read, write), remote_write) = make_stream_pair();
    let tunnel = CmdTunnel::new(
        SnTunnelRead::new(
            read,
            local_endpoint,
            observed_endpoint,
            test_id(9),
            peer_id.clone(),
        ),
        SnTunnelWrite::new(
            write,
            local_endpoint,
            observed_endpoint,
            test_id(9),
            peer_id,
        ),
    );
    service.handle_tunnel(tunnel).await.unwrap();
    let tunnels = service.cmd_server.get_peer_tunnels(&cmd_peer_id).await;
    assert_eq!(tunnels.len(), 1);
    let actual_tunnel_id = tunnels[0].conn_id;

    assert!(
        service
            .rendezvous_endpoints_owned_by(
                &cmd_peer_id,
                actual_tunnel_id,
                &[requested_on_observed_ip],
            )
            .await,
        "the exact live command tunnel authorizes its observed IP regardless of requested port"
    );
    assert!(
        !service
            .rendezvous_endpoints_owned_by(
                &cmd_peer_id,
                missing_tunnel_id,
                &[requested_on_observed_ip],
            )
            .await,
        "another live tunnel for the same peer cannot authorize a missing tunnel id"
    );
    assert!(
        !service
            .rendezvous_endpoints_owned_by(
                &cmd_peer_id,
                actual_tunnel_id,
                &[cached_endpoint],
            )
            .await,
        "cached and certificate endpoint IPs remain unauthorized on a live tunnel with another observed IP"
    );

    drop(remote_write);
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if service
                .cmd_server
                .get_peer_tunnels(&cmd_peer_id)
                .await
                .is_empty()
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

struct DirectInterSnClient {
    local_sn_id: P2pId,
    peers: HashMap<P2pId, Arc<dyn InterSnPeer>>,
}

#[async_trait::async_trait]
impl SnInterClient for DirectInterSnClient {
    async fn query_detail_from_sn(
        &self,
        remote_sn_id: &P2pId,
        peer_id: P2pId,
    ) -> P2pResult<Option<ServingPeerDetail>> {
        self.peers
            .get(remote_sn_id)
            .ok_or_else(|| {
                p2p_err!(
                    P2pErrorCode::NotFound,
                    "explicit test inter-sn peer missing: {}",
                    remote_sn_id
                )
            })?
            .query_detail_from_sn(self.local_sn_id.clone(), peer_id)
            .await
    }

    async fn relay_call_to_sn(
        &self,
        remote_sn_id: &P2pId,
        call_req: SnCall,
    ) -> P2pResult<RelayCallOutcome> {
        self.peers
            .get(remote_sn_id)
            .ok_or_else(|| {
                p2p_err!(
                    P2pErrorCode::NotFound,
                    "explicit test inter-sn peer missing: {}",
                    remote_sn_id
                )
            })?
            .relay_call_from_sn(self.local_sn_id.clone(), call_req)
            .await
    }

    async fn relay_rendezvous_to_sn(
        &self,
        remote_sn_id: &P2pId,
        target_peer_id: P2pId,
        notify: SnTunnelRendezvousNotify,
    ) -> P2pResult<SnTunnelRendezvousResp> {
        self.peers
            .get(remote_sn_id)
            .ok_or_else(|| {
                p2p_err!(
                    P2pErrorCode::NotFound,
                    "explicit test inter-sn peer missing: {}",
                    remote_sn_id
                )
            })?
            .relay_rendezvous_from_sn(self.local_sn_id.clone(), target_peer_id, notify)
            .await
    }
}

struct StaticDetailInterSnClient {
    serving_sn_id: P2pId,
    peer_id: P2pId,
    detail: ServingPeerDetail,
}

#[async_trait::async_trait]
impl SnInterClient for StaticDetailInterSnClient {
    async fn query_detail_from_sn(
        &self,
        remote_sn_id: &P2pId,
        peer_id: P2pId,
    ) -> P2pResult<Option<ServingPeerDetail>> {
        if remote_sn_id != &self.serving_sn_id || peer_id != self.peer_id {
            return Err(p2p_err!(
                P2pErrorCode::NotFound,
                "unexpected distributed detail query: serving_sn={}, peer={}",
                remote_sn_id,
                peer_id
            ));
        }

        Ok(Some(self.detail.clone()))
    }

    async fn relay_call_to_sn(
        &self,
        _remote_sn_id: &P2pId,
        _call_req: SnCall,
    ) -> P2pResult<RelayCallOutcome> {
        Err(p2p_err!(
            P2pErrorCode::NotSupport,
            "relay call is unused in distributed detail query test"
        ))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cross_sn_rendezvous_reaches_second_service_and_real_target_transport() {
    let (serving_server, target, serving_sn, target_id, cert_factory) =
        crate::sn::tests::setup_sn_and_one_client("cross-sn-rendezvous-target").await;
    let querying_sn = test_id(180);
    let owner_sn = test_id(181);
    let membership =
        OwnerMembership::with_options(vec![owner_sn.clone()], 1, Duration::from_secs(60)).unwrap();
    let owner = test_owner_service(owner_sn, membership, allow_all_sn_inter_service_validator());
    owner
        .service()
        .election_node()
        .renew_serving_session(
            serving_sn.clone(),
            0,
            Duration::from_secs(60),
            bucky_time_now(),
        )
        .await
        .unwrap();
    owner
        .publish_lease_from_sn(
            serving_sn.clone(),
            ServingLease {
                peer_id: target_id.clone(),
                serving_sn_id: serving_sn.clone(),
                sequence: 1,
                expires_at: bucky_time_now() + 60_000_000,
            },
        )
        .await
        .unwrap();

    let direct_inter_sn: SnInterClientRef = Arc::new(DirectInterSnClient {
        local_sn_id: querying_sn.clone(),
        peers: HashMap::from([(
            serving_sn.clone(),
            serving_server.service().clone() as Arc<dyn InterSnPeer>,
        )]),
    });
    let querying = SnService::new_with_test_inter_sn_client(
        cert_factory.clone(),
        allow_all_sn_connection_validator(),
        allow_all_sn_inter_service_validator(),
        direct_owner_client(owner),
        direct_inter_sn,
        querying_sn.clone(),
    );

    let initiator =
        crate::x509::generate_rsa_x509_identity(Some("cross-sn-rendezvous-initiator".to_owned()))
            .unwrap();
    let initiator_cert = initiator.get_identity_cert().unwrap();
    let initiator_id = initiator_cert.get_id();
    let observed = Arc::new(Mutex::new(None));
    let observed_for_listener = observed.clone();
    target.sn_client().set_rendezvous_listener(
        move |notify: SnTunnelRendezvousNotify, received_serving_sn: P2pId| {
            let observed = observed_for_listener.clone();
            let initiator_id = initiator_id.clone();
            let cert_factory = cert_factory.clone();
            let serving_sn = serving_sn.clone();
            async move {
                assert_eq!(received_serving_sn, serving_sn);
                assert_eq!(
                    cert_factory.create(&notify.peer_info)?.get_id(),
                    initiator_id
                );
                *observed.lock().unwrap() = Some((notify.seq, notify.tunnel_id));
                Ok(SnTunnelRendezvousActionAck::without_prediction())
            }
        },
    );

    let request = SnTunnelRendezvous {
        seq: Sequence::from(182),
        tunnel_id: TunnelId::from(183),
        to_peer_id: target_id,
        operation: SnTunnelRendezvousOperation::WaitIncoming,
        end_point_array: Vec::new(),
        need_predict_endpoint: false,
    };
    let notify = SnTunnelRendezvousNotify {
        seq: request.seq,
        tunnel_id: request.tunnel_id,
        peer_info: initiator_cert.get_encoded_cert().unwrap(),
        operation: request.operation,
        end_point_array: Vec::new(),
        need_predict_endpoint: false,
    };

    let response = querying
        .process_rendezvous_request(&initiator_cert.get_id(), &request, &notify, &querying_sn)
        .await;
    assert!(response.is_success());
    assert_eq!(
        observed.lock().unwrap().as_ref(),
        Some(&(request.seq, request.tunnel_id))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cross_sn_rendezvous_rejects_unowned_target_prediction_at_serving_sn() {
    let (serving_server, target, serving_sn, target_id, _cert_factory) =
        crate::sn::tests::setup_sn_and_one_client("cross-sn-unowned-prediction").await;
    let mut third_party =
        Endpoint::from((Protocol::Quic, "192.0.2.199:42601".parse().unwrap()));
    third_party.set_area(EndpointArea::ServerReflexive);
    target.sn_client().set_rendezvous_listener(
        move |_notify: SnTunnelRendezvousNotify, received_serving_sn: P2pId| {
            let serving_sn = serving_sn.clone();
            async move {
                assert_eq!(received_serving_sn, serving_sn);
                Ok(SnTunnelRendezvousActionAck {
                    predicted_endpoints: vec![third_party],
                    socket_binding_generation: 1,
                    valid_until: bucky_time_now() + 1_000_000,
                })
            }
        },
    );
    let initiator =
        crate::x509::generate_rsa_x509_identity(Some("cross-sn-owner-check-initiator".to_owned()))
            .unwrap();
    let initiator_cert = initiator.get_identity_cert().unwrap();
    let notify = SnTunnelRendezvousNotify {
        seq: Sequence::from(184),
        tunnel_id: TunnelId::from(185),
        peer_info: initiator_cert.get_encoded_cert().unwrap(),
        operation: SnTunnelRendezvousOperation::WaitIncoming,
        end_point_array: Vec::new(),
        need_predict_endpoint: true,
    };

    let error = serving_server
        .service()
        .relay_rendezvous_from_sn(test_id(186), target_id, notify)
        .await
        .unwrap_err();

    assert_eq!(error.code(), P2pErrorCode::PermissionDenied);
}

#[tokio::test]
async fn rendezvous_response_owner_rejects_self_reported_ip_without_observation() {
    let service = test_sn_service(allow_all_sn_connection_validator());
    let target = test_id(187);
    service
        .validate_rendezvous_response_owner(
            &target,
            &SnTunnelRendezvousResp::success(Sequence::from(187), Vec::new()),
        )
        .await
        .unwrap();
    let mut self_reported =
        Endpoint::from((Protocol::Quic, "198.51.100.187:42602".parse().unwrap()));
    self_reported.set_area(EndpointArea::ServerReflexive);
    service.peer_mgr.add_or_update_peer(
        &target,
        &Some(Arc::new(TestIdentityCert {
            id: target.clone(),
            encoded: target.as_slice().to_vec(),
        })),
        SN_PROTOCOL_VERSION,
        Vec::new(),
        &vec![self_reported],
    );
    let response = SnTunnelRendezvousResp::success(Sequence::from(188), vec![self_reported]);

    let error = service
        .validate_rendezvous_response_owner(&target, &response)
        .await
        .unwrap_err();

    assert_eq!(error.code(), P2pErrorCode::PermissionDenied);
}

#[tokio::test]
async fn cold_distributed_query_returns_remote_profile_in_final_sn_query_response() {
    let owner_sn = test_id(90);
    let querying_sn = test_id(91);
    let serving_sn = test_id(92);
    let peer = test_id(93);
    let requester = test_id(94);
    let membership =
        OwnerMembership::with_options(vec![owner_sn.clone()], 1, Duration::from_secs(60)).unwrap();
    let owner = test_owner_service(
        owner_sn,
        membership,
        allow_all_sn_inter_service_validator(),
    );
    let mut observed = Endpoint::from((Protocol::Quic, "198.51.100.92:49200".parse().unwrap()));
    observed.set_area(EndpointArea::ServerReflexive);
    let profile = NatProfile::from_observations(
        &[observed.clone(), observed.clone()],
        bucky_time_now(),
        Duration::from_secs(30),
    );
    owner
        .service()
        .election_node()
        .renew_serving_session(
            serving_sn.clone(),
            0,
            Duration::from_secs(60),
            bucky_time_now(),
        )
        .await
        .unwrap();
    owner
        .publish_lease_from_sn(
            serving_sn.clone(),
            ServingLease {
                peer_id: peer.clone(),
                serving_sn_id: serving_sn.clone(),
                sequence: 1,
                expires_at: bucky_time_now() + 60_000_000,
            },
        )
        .await
        .unwrap();
    let direct_inter_sn: SnInterClientRef = Arc::new(StaticDetailInterSnClient {
        serving_sn_id: serving_sn,
        peer_id: peer.clone(),
        detail: ServingPeerDetail {
            peer_info: peer.as_slice().to_vec(),
            endpoints: vec![observed.clone()],
            net_profile: Some(profile.clone()),
            target_protocol_version: None,
        },
    });
    let querying = SnService::new_with_test_inter_sn_client(
        Arc::new(TestIdentityCertFactory),
        allow_all_sn_connection_validator(),
        allow_all_sn_inter_service_validator(),
        direct_owner_client(owner),
        direct_inter_sn,
        querying_sn.clone(),
    );

    assert!(querying.peer_mgr.find_peer(&peer).is_none());
    let response = querying
        .handle_query_sn(
            &querying_sn,
            &PeerId::from(requester.as_slice()),
            90u32.into(),
            SnQuery {
                protocol_version: 0,
                stack_version: 0,
                seq: 91u32.into(),
                query_id: peer.clone(),
            },
        )
        .await
        .unwrap();

    assert_eq!(response.peer_info, Some(peer.as_slice().to_vec()));
    assert_eq!(response.net_profile, Some(profile));
    assert_eq!(response.end_point_array, vec![observed]);
    assert_eq!(response.target_protocol_version, None);
    assert!(querying.peer_mgr.find_peer(&peer).is_none());
}
