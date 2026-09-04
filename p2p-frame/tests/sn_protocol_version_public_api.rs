use p2p_frame::p2p_identity::P2pId;
use p2p_frame::sn::inter_sn::ServingPeerDetail;
use p2p_frame::sn::protocol::{SN_PROTOCOL_VERSION, SnDetailResp, SnQuery, SnQueryResp};

#[test]
fn external_consumer_can_construct_and_read_protocol_version_api() {
    assert_eq!(SN_PROTOCOL_VERSION, 1);
    let query = SnQuery {
        protocol_version: SN_PROTOCOL_VERSION,
        stack_version: 0,
        seq: 1u32.into(),
        query_id: P2pId::from(vec![1; 32]),
    };
    assert_eq!(query.protocol_version, SN_PROTOCOL_VERSION);

    let response = SnQueryResp {
        seq: query.seq,
        peer_info: None,
        end_point_array: Vec::new(),
        net_profile: None,
        target_protocol_version: Some(0),
    };
    assert_eq!(response.target_protocol_version, Some(0));

    let detail = SnDetailResp {
        peer_info: None,
        end_point_array: Vec::new(),
        net_profile: None,
        target_protocol_version: None,
    };
    assert_eq!(detail.target_protocol_version, None);

    let serving = ServingPeerDetail {
        peer_info: vec![2; 32],
        endpoints: Vec::new(),
        net_profile: None,
        target_protocol_version: Some(SN_PROTOCOL_VERSION),
    };
    assert_eq!(serving.target_protocol_version, Some(1));
}
