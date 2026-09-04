use std::collections::HashMap as ProtocolHashMap;
use crate::error::P2pError;

#[derive(Clone)]
enum ProtocolDetailReply {
    Detail(ServingPeerDetail),
    Missing,
    Error(P2pErrorCode),
}

struct ProtocolVersionOwnerClient {
    leases: Vec<ServingLease>,
}

#[async_trait::async_trait]
impl OwnerDirectoryClient for ProtocolVersionOwnerClient {
    async fn publish_serving_lease(
        &self,
        _local_sn_id: P2pId,
        _peer_id: P2pId,
        _sequence: u64,
    ) -> P2pResult<()> {
        Ok(())
    }

    async fn query_serving_leases(
        &self,
        _local_sn_id: &P2pId,
        _peer_id: &P2pId,
    ) -> P2pResult<Vec<ServingLease>> {
        Ok(self.leases.clone())
    }
}

struct ProtocolVersionInterClient {
    replies: ProtocolHashMap<P2pId, ProtocolDetailReply>,
}

#[async_trait::async_trait]
impl SnInterClient for ProtocolVersionInterClient {
    async fn query_detail_from_sn(
        &self,
        remote_sn_id: &P2pId,
        _peer_id: P2pId,
    ) -> P2pResult<Option<ServingPeerDetail>> {
        match self.replies.get(remote_sn_id) {
            Some(ProtocolDetailReply::Detail(detail)) => Ok(Some(detail.clone())),
            Some(ProtocolDetailReply::Missing) | None => Ok(None),
            Some(ProtocolDetailReply::Error(code)) => {
                Err(P2pError::new(*code, "protocol detail test failure".to_owned()))
            }
        }
    }

    async fn relay_call_to_sn(
        &self,
        _remote_sn_id: &P2pId,
        _call_req: SnCall,
    ) -> P2pResult<RelayCallOutcome> {
        Err(p2p_err!(P2pErrorCode::NotSupport, "unused in protocol version test"))
    }
}

fn protocol_lease(peer_id: &P2pId, serving_sn_id: P2pId, sequence: u64) -> ServingLease {
    ServingLease {
        peer_id: peer_id.clone(),
        serving_sn_id,
        sequence,
        expires_at: bucky_time_now() + 60_000_000,
    }
}

fn protocol_detail(seed: u8, version: Option<u8>) -> ServingPeerDetail {
    let endpoint = Endpoint::from((
        Protocol::Quic,
        format!("198.51.100.{}:{}", seed, 41000 + u16::from(seed))
            .parse()
            .unwrap(),
    ));
    ServingPeerDetail {
        peer_info: vec![seed; 32],
        endpoints: vec![endpoint],
        net_profile: None,
        target_protocol_version: version,
    }
}

fn protocol_query_service(
    local_sn_id: P2pId,
    peer_id: &P2pId,
    participants: Vec<P2pId>,
    replies: ProtocolHashMap<P2pId, ProtocolDetailReply>,
) -> SnServiceRef {
    let leases = participants
        .into_iter()
        .enumerate()
        .map(|(index, serving_sn_id)| {
            protocol_lease(peer_id, serving_sn_id, index as u64 + 1)
        })
        .collect();
    SnService::new_with_test_inter_sn_client(
        Arc::new(TestIdentityCertFactory),
        allow_all_sn_connection_validator(),
        allow_all_sn_inter_service_validator(),
        Arc::new(ProtocolVersionOwnerClient { leases }),
        Arc::new(ProtocolVersionInterClient { replies }),
        local_sn_id,
    )
}

#[tokio::test]
async fn remote_version_consensus_requires_every_serving_lease() {
    let peer = test_id(101);
    let local_sn = test_id(102);
    let sn_a = test_id(103);
    let sn_b = test_id(104);

    let cases = vec![
        (
            "all-known-zero",
            vec![sn_a.clone(), sn_b.clone()],
            ProtocolHashMap::from([
                (sn_a.clone(), ProtocolDetailReply::Detail(protocol_detail(103, Some(0)))),
                (sn_b.clone(), ProtocolDetailReply::Detail(protocol_detail(104, Some(0)))),
            ]),
            Some(0),
            2,
        ),
        (
            "conflict",
            vec![sn_a.clone(), sn_b.clone()],
            ProtocolHashMap::from([
                (sn_a.clone(), ProtocolDetailReply::Detail(protocol_detail(103, Some(0)))),
                (sn_b.clone(), ProtocolDetailReply::Detail(protocol_detail(104, Some(1)))),
            ]),
            None,
            2,
        ),
        (
            "old-none",
            vec![sn_a.clone(), sn_b.clone()],
            ProtocolHashMap::from([
                (sn_a.clone(), ProtocolDetailReply::Detail(protocol_detail(103, Some(1)))),
                (sn_b.clone(), ProtocolDetailReply::Detail(protocol_detail(104, None))),
            ]),
            None,
            2,
        ),
        (
            "missing",
            vec![sn_a.clone(), sn_b.clone()],
            ProtocolHashMap::from([
                (sn_a.clone(), ProtocolDetailReply::Detail(protocol_detail(103, Some(1)))),
                (sn_b.clone(), ProtocolDetailReply::Missing),
            ]),
            None,
            1,
        ),
        (
            "error",
            vec![sn_a.clone(), sn_b.clone()],
            ProtocolHashMap::from([
                (sn_a.clone(), ProtocolDetailReply::Detail(protocol_detail(103, Some(1)))),
                (sn_b.clone(), ProtocolDetailReply::Error(P2pErrorCode::Timeout)),
            ]),
            None,
            1,
        ),
        (
            "local-miss-local-lease",
            vec![local_sn.clone(), sn_a.clone()],
            ProtocolHashMap::from([(
                sn_a.clone(),
                ProtocolDetailReply::Detail(protocol_detail(103, Some(1))),
            )]),
            None,
            1,
        ),
    ];

    for (name, participants, replies, expected, expected_details) in cases {
        let service = protocol_query_service(local_sn.clone(), &peer, participants, replies);
        let remote = service.query_remote_details(&local_sn, &peer).await;
        assert_eq!(remote.target_protocol_version, expected, "case={name}");
        assert_eq!(remote.details.len(), expected_details, "case={name}");
    }
}

#[tokio::test]
async fn unknown_remote_version_preserves_successful_detail_data() {
    let peer = test_id(105);
    let requester = test_id(106);
    let local_sn = test_id(107);
    let sn_a = test_id(108);
    let sn_b = test_id(109);
    let detail = protocol_detail(108, Some(1));
    let expected_endpoint = detail.endpoints[0];
    let service = protocol_query_service(
        local_sn.clone(),
        &peer,
        vec![sn_a.clone(), sn_b.clone()],
        ProtocolHashMap::from([
            (sn_a, ProtocolDetailReply::Detail(detail)),
            (sn_b, ProtocolDetailReply::Missing),
        ]),
    );

    let response = service
        .handle_query_sn(
            &local_sn,
            &PeerId::from(requester.as_slice()),
            1u32.into(),
            SnQuery {
                protocol_version: SN_PROTOCOL_VERSION,
                stack_version: 0,
                seq: 2u32.into(),
                query_id: peer,
            },
        )
        .await
        .unwrap();

    assert_eq!(response.target_protocol_version, None);
    assert_eq!(response.peer_info, Some(vec![108; 32]));
    assert_eq!(response.end_point_array, vec![expected_endpoint]);
}

#[tokio::test]
async fn local_cache_option_is_authoritative_over_remote_version() {
    let peer = test_id(110);
    let requester = test_id(111);
    let local_sn = test_id(112);
    let remote_sn = test_id(113);
    let service = protocol_query_service(
        local_sn.clone(),
        &peer,
        vec![remote_sn.clone()],
        ProtocolHashMap::from([(
            remote_sn,
            ProtocolDetailReply::Detail(protocol_detail(113, Some(1))),
        )]),
    );
    let local_cert: P2pIdentityCertRef = Arc::new(TestIdentityCert {
        id: peer.clone(),
        encoded: vec![110; 32],
    });
    service.peer_mgr.add_peer_info(peer.clone(), local_cert.clone());

    let query = || SnQuery {
        protocol_version: SN_PROTOCOL_VERSION,
        stack_version: 0,
        seq: 3u32.into(),
        query_id: peer.clone(),
    };
    let response = service
        .handle_query_sn(
            &local_sn,
            &PeerId::from(requester.as_slice()),
            1u32.into(),
            query(),
        )
        .await
        .unwrap();
    assert_eq!(response.peer_info, Some(vec![110; 32]));
    assert_eq!(response.target_protocol_version, None);

    service
        .peer_mgr
        .add_or_update_peer(&peer, &Some(local_cert), 0, Vec::new(), &Vec::new());
    let response = service
        .handle_query_sn(
            &local_sn,
            &PeerId::from(requester.as_slice()),
            1u32.into(),
            query(),
        )
        .await
        .unwrap();
    assert_eq!(response.peer_info, Some(vec![110; 32]));
    assert_eq!(response.target_protocol_version, Some(0));
}

#[tokio::test]
async fn report_claimed_identity_mismatch_rejects_before_peer_state_mutation() {
    let service = test_sn_service(allow_all_sn_connection_validator());
    let authenticated = test_id(114);
    let claimed = test_id(115);
    let local_sn = test_id(116);
    let authenticated_cert: P2pIdentityCertRef = Arc::new(TestIdentityCert {
        id: authenticated.clone(),
        encoded: authenticated.as_slice().to_vec(),
    });
    service.peer_mgr.add_or_update_peer(
        &authenticated,
        &Some(authenticated_cert),
        0,
        Vec::new(),
        &Vec::new(),
    );
    let mut report = test_report(claimed.clone(), authenticated.as_slice().to_vec());
    report.protocol_version = 1;

    let error = service
        .handle_report_sn(
            &local_sn,
            &PeerId::from(authenticated.as_slice()),
            1u32.into(),
            report,
        )
        .await
        .unwrap_err();

    assert_eq!(error.code(), P2pErrorCode::PermissionDenied);
    assert_eq!(
        service
            .peer_mgr
            .find_peer(&authenticated)
            .unwrap()
            .protocol_version,
        Some(0)
    );
    assert!(service.peer_mgr.find_peer(&claimed).is_none());
}

#[tokio::test]
async fn authenticated_report_updates_local_query_version() {
    let service = test_sn_service(allow_all_sn_connection_validator());
    let peer = test_id(117);
    let local_sn = test_id(118);
    let mut report = test_report(peer.clone(), peer.as_slice().to_vec());
    report.protocol_version = 1;

    service
        .handle_report_sn(
            &local_sn,
            &PeerId::from(peer.as_slice()),
            1u32.into(),
            report,
        )
        .await
        .unwrap();

    let response = service
        .handle_query_sn(
            &local_sn,
            &PeerId::from(test_id(119).as_slice()),
            2u32.into(),
            SnQuery {
                protocol_version: SN_PROTOCOL_VERSION,
                stack_version: 0,
                seq: 4u32.into(),
                query_id: peer,
            },
        )
        .await
        .unwrap();
    assert_eq!(response.target_protocol_version, Some(1));
}
