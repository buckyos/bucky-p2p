use super::*;
use crate::endpoint::Protocol;
use crate::nat_type::{NatProfile, NatTraversalContext};
use bucky_raw_codec::{RawConvertTo, RawFrom};
use std::time::Duration;

fn context(now: Timestamp) -> NatTraversalContext {
    let endpoint = Endpoint::from((Protocol::Quic, "198.51.100.1:4000".parse().unwrap()));
    let profile =
        NatProfile::from_observations(&[endpoint, endpoint], now, Duration::from_secs(10));
    NatTraversalContext::new(
        P2pId::from(vec![1; 32]),
        P2pId::from(vec![2; 32]),
        profile.clone(),
        profile,
    )
}

fn legacy_called() -> LegacySnCalled {
    LegacySnCalled {
        seq: 1u32.into(),
        sn_peer_id: P2pId::from(vec![3; 32]),
        to_peer_id: P2pId::from(vec![2; 32]),
        reverse_endpoint_array: vec![],
        active_pn_list: vec![],
        peer_info: vec![],
        tunnel_id: 2u32.into(),
        call_send_time: 1_000_000,
        call_type: TunnelType::Stream,
        payload: vec![],
    }
}

#[test]
fn called_context_is_additive_and_legacy_bases_remain_bidirectionally_compatible() {
    let legacy = legacy_called();
    let decoded = SnCalled::clone_from_slice(&legacy.to_vec().unwrap()).unwrap();
    assert!(decoded.nat_context.is_none());

    let called = SnCalled {
        seq: legacy.seq,
        sn_peer_id: legacy.sn_peer_id.clone(),
        to_peer_id: legacy.to_peer_id.clone(),
        reverse_endpoint_array: vec![],
        active_pn_list: vec![],
        peer_info: vec![],
        tunnel_id: legacy.tunnel_id,
        call_send_time: legacy.call_send_time,
        call_type: legacy.call_type,
        payload: vec![],
        nat_context: Some(context(legacy.call_send_time)),
    };
    let bytes = called.to_vec().unwrap();
    assert_eq!(
        SnCalled::clone_from_slice(&bytes).unwrap().nat_context,
        called.nat_context
    );
    assert_eq!(
        LegacySnCalled::clone_from_slice(&bytes).unwrap().tunnel_id,
        called.tunnel_id
    );

    let mut truncated = legacy.to_vec().unwrap();
    truncated.extend_from_slice(&SN_CALLED_EXTENSION_MAGIC.to_be_bytes());
    assert!(
        SnCalled::clone_from_slice(&truncated)
            .unwrap()
            .nat_context
            .is_none()
    );

    let mut unknown_version = legacy.to_vec().unwrap();
    unknown_version.extend_from_slice(&SN_CALLED_EXTENSION_MAGIC.to_be_bytes());
    unknown_version.push(0xff);
    unknown_version.extend_from_slice(&0u32.to_be_bytes());
    assert!(
        SnCalled::clone_from_slice(&unknown_version)
            .unwrap()
            .nat_context
            .is_none()
    );
}

#[test]
fn sn_call_response_layout_has_no_nat_extension_or_profile_field() {
    let response = SnCallResp {
        seq: 5u32.into(),
        sn_peer_id: P2pId::from(vec![6; 32]),
        result: 0,
        to_peer_info: None,
    };
    let bytes = response.to_vec().unwrap();
    let decoded = SnCallResp::clone_from_slice(&bytes).unwrap();
    assert_eq!(decoded.seq, response.seq);
    assert_eq!(decoded.sn_peer_id, response.sn_peer_id);
    assert_eq!(decoded.result, response.result);
    assert!(decoded.to_peer_info.is_none());
}
