struct ProfileDetailInterSnPeer {
    local_sn_id: P2pId,
    detail: Mutex<Option<ServingPeerDetail>>,
}

impl ProfileDetailInterSnPeer {
    fn new(local_sn_id: P2pId) -> Arc<Self> {
        Arc::new(Self {
            local_sn_id,
            detail: Mutex::new(None),
        })
    }
}

#[async_trait]
impl InterSnPeer for ProfileDetailInterSnPeer {
    fn sn_id(&self) -> Option<P2pId> {
        Some(self.local_sn_id.clone())
    }

    async fn query_detail_from_sn(
        &self,
        _remote_sn_id: P2pId,
        _peer_id: P2pId,
    ) -> P2pResult<Option<ServingPeerDetail>> {
        Ok(self.detail.lock().unwrap().clone())
    }
}

#[tokio::test]
async fn sn_distributed_detail_response_forwards_profile_without_replication() {
    use crate::endpoint::{EndpointArea, Protocol};

    let local_peer = ProfileDetailInterSnPeer::new(test_id(30));
    let remote_sn_id = test_id(31);
    let peer_id = test_id(32);
    let mut observed = Endpoint::from((Protocol::Quic, "198.51.100.30:43000".parse().unwrap()));
    observed.set_area(EndpointArea::ServerReflexive);
    let profile =
        NatProfile::from_observations(&[observed, observed], 1_000_000, Duration::from_secs(10));
    *local_peer.detail.lock().unwrap() = Some(ServingPeerDetail {
        peer_info: vec![1, 2, 3],
        endpoints: vec![observed],
        net_profile: Some(profile.clone()),
        target_protocol_version: Some(0),
    });

    let response = dispatch_owner_cmd(
        local_peer,
        remote_sn_id,
        InterSnCommandCode::QueryDetail,
        InterSnRequest::QueryDetail(SnDetailQuery { peer_id }),
    )
    .await;

    match response {
        InterSnResponse::Detail(detail) => {
            assert_eq!(detail.net_profile, Some(profile));
            assert_eq!(detail.end_point_array, vec![observed]);
            assert_eq!(detail.target_protocol_version, Some(0));
        }
        other => panic!("unexpected response: {:?}", other),
    }
}

include!("inter_sn_rendezvous_tests.rs");
