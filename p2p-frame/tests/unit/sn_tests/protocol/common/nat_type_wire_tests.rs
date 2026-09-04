use super::*;
use crate::endpoint::{EndpointArea, Protocol};
use crate::nat_type::NatProfile;
use bucky_raw_codec::{RawConvertTo, RawFrom};

fn endpoint(port: u16) -> Endpoint {
    let mut endpoint = Endpoint::from((
        Protocol::Quic,
        "198.51.100.1".parse::<std::net::IpAddr>().unwrap(),
        port,
    ));
    endpoint.set_area(EndpointArea::ServerReflexive);
    endpoint
}

fn profile(now: Timestamp) -> NatProfile {
    NatProfile::from_observations(
        &[endpoint(4000), endpoint(4000)],
        now,
        Duration::from_secs(10),
    )
}

fn context(now: Timestamp) -> NatTraversalContext {
    NatTraversalContext::new(
        P2pId::from(vec![1; 32]),
        P2pId::from(vec![2; 32]),
        profile(now),
        profile(now),
    )
}

#[test]
fn legacy_query_detail_and_report_bases_decode_without_extensions() {
    let query = LegacySnQueryResp {
        seq: 7u32.into(),
        peer_info: None,
        end_point_array: vec![endpoint(4100)],
    };
    let decoded = SnQueryResp::clone_from_slice(&query.to_vec().unwrap()).unwrap();
    assert_eq!(decoded.seq, query.seq);
    assert_eq!(decoded.end_point_array, query.end_point_array);
    assert!(decoded.net_profile.is_none());
    assert!(decoded.target_protocol_version.is_none());

    let detail = LegacySnDetailResp {
        peer_info: None,
        end_point_array: vec![endpoint(4200)],
    };
    let decoded = SnDetailResp::clone_from_slice(&detail.to_vec().unwrap()).unwrap();
    assert_eq!(decoded.end_point_array, detail.end_point_array);
    assert!(decoded.net_profile.is_none());
    assert!(decoded.target_protocol_version.is_none());

    let report = LegacyReportSnResp {
        seq: 8u32.into(),
        sn_peer_id: P2pId::from(vec![3; 32]),
        result: 0,
        peer_info: None,
        end_point_array: vec![],
        receipt: None,
    };
    let decoded = ReportSnResp::clone_from_slice(&report.to_vec().unwrap()).unwrap();
    assert!(decoded.nat_probe_endpoints.is_empty());
    assert!(decoded.nat_probe_directive.is_none());
}

#[test]
fn nat_probe_directive_and_result_roundtrip_after_existing_optional_tails() {
    let sn_peer_id = P2pId::from(vec![21; 32]);
    let peer_id = P2pId::from(vec![22; 32]);
    let directive = NatProbeDirective {
        version: NAT_PROBE_CONTROL_VERSION,
        sn_peer_id: sn_peer_id.clone(),
        peer_id: peer_id.clone(),
        registration_generation: 7,
        request_id: 8,
        probe_config_generation: 9,
        expires_at: 10_000_000,
        endpoints: vec![endpoint(4500), endpoint(4501)],
    };
    let response = ReportSnResp {
        seq: 20u32.into(),
        sn_peer_id: sn_peer_id.clone(),
        result: 0,
        peer_info: None,
        end_point_array: vec![],
        receipt: None,
        nat_probe_endpoints: vec![endpoint(4400)],
        nat_probe_directive: Some(directive.clone()),
    };
    let response_bytes = response.to_vec().unwrap();
    let decoded_response = ReportSnResp::clone_from_slice(&response_bytes).unwrap();
    assert_eq!(decoded_response.nat_probe_directive, Some(directive.clone()));
    assert_eq!(decoded_response.nat_probe_endpoints, response.nat_probe_endpoints);
    assert_eq!(
        LegacyReportSnResp::clone_from_slice(&response_bytes)
            .unwrap()
            .seq,
        response.seq
    );

    let result = NatProbeResult::from_directive(&directive, profile(5_000_000));
    let request = ReportSn {
        protocol_version: 0,
        stack_version: 0,
        seq: 21u32.into(),
        sn_peer_id,
        from_peer_id: Some(peer_id),
        peer_info: None,
        send_time: 5_000_000,
        contract_id: None,
        receipt: None,
        map_ports: vec![],
        local_eps: vec![],
        net_profile: Some(profile(5_000_000)),
        nat_probe_control_version: Some(NAT_PROBE_CONTROL_VERSION),
        nat_probe_result: Some(result.clone()),
    };
    let request_bytes = request.to_vec().unwrap();
    let decoded_request = ReportSn::clone_from_slice(&request_bytes).unwrap();
    assert_eq!(
        decoded_request.nat_probe_control_version,
        Some(NAT_PROBE_CONTROL_VERSION)
    );
    assert_eq!(decoded_request.nat_probe_result, Some(result));
    assert!(decoded_request.net_profile.is_some());
    assert_eq!(
        LegacyReportSn::clone_from_slice(&request_bytes)
            .unwrap()
            .seq,
        request.seq
    );
}

#[test]
fn unsupported_nat_probe_control_payloads_fail_closed() {
    let mut directive = NatProbeDirective {
        version: NAT_PROBE_CONTROL_VERSION + 1,
        sn_peer_id: P2pId::from(vec![23; 32]),
        peer_id: P2pId::from(vec![24; 32]),
        registration_generation: 1,
        request_id: 2,
        probe_config_generation: 3,
        expires_at: 9_000_000,
        endpoints: vec![endpoint(4600)],
    };
    let response = ReportSnResp {
        seq: 22u32.into(),
        sn_peer_id: directive.sn_peer_id.clone(),
        result: 0,
        peer_info: None,
        end_point_array: vec![],
        receipt: None,
        nat_probe_endpoints: vec![],
        nat_probe_directive: Some(directive.clone()),
    };
    assert!(ReportSnResp::clone_from_slice(&response.to_vec().unwrap())
        .unwrap()
        .nat_probe_directive
        .is_none());

    directive.version = NAT_PROBE_CONTROL_VERSION;
    directive.endpoints.clear();
    let response = ReportSnResp {
        nat_probe_directive: Some(directive),
        ..response
    };
    assert!(ReportSnResp::clone_from_slice(&response.to_vec().unwrap())
        .unwrap()
        .nat_probe_directive
        .is_none());

    let unsupported_client = ReportSn {
        protocol_version: 0,
        stack_version: 0,
        seq: 23u32.into(),
        sn_peer_id: P2pId::from(vec![23; 32]),
        from_peer_id: Some(P2pId::from(vec![24; 32])),
        peer_info: None,
        send_time: 9_000_000,
        contract_id: None,
        receipt: None,
        map_ports: vec![],
        local_eps: vec![],
        net_profile: None,
        nat_probe_control_version: Some(NAT_PROBE_CONTROL_VERSION + 1),
        nat_probe_result: None,
    };
    let decoded = ReportSn::clone_from_slice(&unsupported_client.to_vec().unwrap()).unwrap();
    assert!(decoded.nat_probe_control_version.is_none());
    assert!(decoded.nat_probe_result.is_none());
}

#[test]
fn new_query_report_detail_and_call_extensions_roundtrip_and_old_decoders_ignore_them() {
    let now = 2_000_000;
    let query = SnQueryResp {
        seq: 9u32.into(),
        peer_info: None,
        end_point_array: vec![endpoint(4300)],
        net_profile: Some(profile(now)),
        target_protocol_version: None,
    };
    let bytes = query.to_vec().unwrap();
    assert_eq!(
        SnQueryResp::clone_from_slice(&bytes).unwrap().net_profile,
        query.net_profile
    );
    assert_eq!(
        LegacySnQueryResp::clone_from_slice(&bytes).unwrap().seq,
        query.seq
    );

    let report = ReportSnResp {
        seq: 10u32.into(),
        sn_peer_id: P2pId::from(vec![4; 32]),
        result: 0,
        peer_info: None,
        end_point_array: vec![],
        receipt: None,
        nat_probe_endpoints: vec![endpoint(4400), endpoint(4401)],
        nat_probe_directive: None,
    };
    let bytes = report.to_vec().unwrap();
    assert_eq!(
        ReportSnResp::clone_from_slice(&bytes)
            .unwrap()
            .nat_probe_endpoints,
        report.nat_probe_endpoints
    );
    assert_eq!(
        LegacyReportSnResp::clone_from_slice(&bytes)
            .unwrap()
            .sn_peer_id,
        report.sn_peer_id
    );

    let detail = SnDetailResp {
        peer_info: None,
        end_point_array: vec![endpoint(4500)],
        net_profile: Some(profile(now)),
        target_protocol_version: None,
    };
    let bytes = detail.to_vec().unwrap();
    assert_eq!(
        SnDetailResp::clone_from_slice(&bytes).unwrap().net_profile,
        detail.net_profile
    );
    assert_eq!(
        LegacySnDetailResp::clone_from_slice(&bytes)
            .unwrap()
            .end_point_array,
        detail.end_point_array
    );

    let call = SnCall {
        protocol_version: 0,
        stack_version: 0,
        seq: 11u32.into(),
        tunnel_id: 12u32.into(),
        sn_peer_id: P2pId::from(vec![4; 32]),
        to_peer_id: P2pId::from(vec![2; 32]),
        from_peer_id: P2pId::from(vec![1; 32]),
        reverse_endpoint_array: Some(vec![endpoint(4600)]),
        active_pn_list: None,
        peer_info: None,
        send_time: now,
        call_type: TunnelType::Stream,
        payload: vec![],
        is_always_call: false,
        nat_context: Some(context(now)),
    };
    let bytes = call.to_vec().unwrap();
    assert_eq!(
        SnCall::clone_from_slice(&bytes).unwrap().nat_context,
        call.nat_context
    );
    assert_eq!(
        LegacySnCall::clone_from_slice(&bytes).unwrap().tunnel_id,
        call.tunnel_id
    );

    let report_request = ReportSn {
        protocol_version: 0,
        stack_version: 0,
        seq: 12u32.into(),
        sn_peer_id: P2pId::from(vec![5; 32]),
        from_peer_id: Some(P2pId::from(vec![6; 32])),
        peer_info: None,
        send_time: now,
        contract_id: None,
        receipt: None,
        map_ports: vec![],
        local_eps: vec![endpoint(4700)],
        net_profile: Some(profile(now)),
        nat_probe_control_version: Some(NAT_PROBE_CONTROL_VERSION),
        nat_probe_result: None,
    };
    let bytes = report_request.to_vec().unwrap();
    assert_eq!(
        ReportSn::clone_from_slice(&bytes).unwrap().net_profile,
        report_request.net_profile
    );
    assert_eq!(
        LegacyReportSn::clone_from_slice(&bytes).unwrap().seq,
        report_request.seq
    );

    let legacy_report_request = LegacyReportSn::from(&ReportSn {
        net_profile: None,
        ..report_request
    });
    assert!(
        ReportSn::clone_from_slice(&legacy_report_request.to_vec().unwrap())
            .unwrap()
            .net_profile
            .is_none()
    );
}

#[test]
fn malformed_or_unknown_query_extension_fails_closed_without_rejecting_legacy_base() {
    let legacy = LegacySnQueryResp {
        seq: 13u32.into(),
        peer_info: None,
        end_point_array: vec![],
    };
    let mut truncated = legacy.to_vec().unwrap();
    truncated.extend_from_slice(&SN_QUERY_RESP_EXTENSION_MAGIC.to_be_bytes());
    assert!(
        SnQueryResp::clone_from_slice(&truncated)
            .unwrap()
            .net_profile
            .is_none()
    );

    let mut unknown_version = legacy.to_vec().unwrap();
    unknown_version.extend_from_slice(&SN_QUERY_RESP_EXTENSION_MAGIC.to_be_bytes());
    unknown_version.push(SN_EXTENSION_VERSION + 1);
    unknown_version.extend_from_slice(&0u32.to_be_bytes());
    assert!(
        SnQueryResp::clone_from_slice(&unknown_version)
            .unwrap()
            .net_profile
            .is_none()
    );

    let legacy_report = LegacyReportSn {
        protocol_version: 0,
        stack_version: 0,
        seq: 14u32.into(),
        sn_peer_id: P2pId::from(vec![7; 32]),
        from_peer_id: None,
        peer_info: None,
        send_time: 3_000_000,
        contract_id: None,
        receipt: None,
        map_ports: vec![],
        local_eps: vec![],
    };
    let mut malformed_report = legacy_report.to_vec().unwrap();
    malformed_report.extend_from_slice(&REPORT_SN_EXTENSION_MAGIC.to_be_bytes());
    assert!(
        ReportSn::clone_from_slice(&malformed_report)
            .unwrap()
            .net_profile
            .is_none()
    );

    let mut unknown_report = legacy_report.to_vec().unwrap();
    unknown_report.extend_from_slice(&REPORT_SN_EXTENSION_MAGIC.to_be_bytes());
    unknown_report.push(SN_EXTENSION_VERSION + 1);
    unknown_report.extend_from_slice(&0u32.to_be_bytes());
    assert!(
        ReportSn::clone_from_slice(&unknown_report)
            .unwrap()
            .net_profile
            .is_none()
    );
}

#[test]
fn target_protocol_version_query_and_detail_wire_distinguish_unknown_zero_and_one() {
    for expected in [None, Some(0), Some(1)] {
        let query = SnQueryResp {
            seq: 30u32.into(),
            peer_info: None,
            end_point_array: vec![endpoint(4800)],
            net_profile: Some(profile(4_000_000)),
            target_protocol_version: expected,
        };
        let mut query_bytes = query.to_vec().unwrap();
        let decoded_query = SnQueryResp::clone_from_slice(&query_bytes).unwrap();
        assert_eq!(decoded_query.target_protocol_version, expected);
        assert_eq!(decoded_query.net_profile, query.net_profile);
        let legacy_query = LegacySnQueryResp::clone_from_slice(&query_bytes).unwrap();
        assert_eq!(legacy_query.seq, query.seq);
        assert_eq!(legacy_query.end_point_array, query.end_point_array);
        query_bytes.extend_from_slice(b"QTAIL");
        let (decoded_query, query_remainder) = SnQueryResp::raw_decode(&query_bytes).unwrap();
        assert_eq!(decoded_query.target_protocol_version, expected);
        assert_eq!(query_remainder, b"QTAIL");

        let detail = SnDetailResp {
            peer_info: None,
            end_point_array: vec![endpoint(4900)],
            net_profile: Some(profile(4_000_000)),
            target_protocol_version: expected,
        };
        let mut detail_bytes = detail.to_vec().unwrap();
        let decoded_detail = SnDetailResp::clone_from_slice(&detail_bytes).unwrap();
        assert_eq!(decoded_detail.target_protocol_version, expected);
        assert_eq!(decoded_detail.net_profile, detail.net_profile);
        let legacy_detail = LegacySnDetailResp::clone_from_slice(&detail_bytes).unwrap();
        assert_eq!(legacy_detail.end_point_array, detail.end_point_array);
        detail_bytes.extend_from_slice(b"DTAIL");
        let (decoded_detail, detail_remainder) = SnDetailResp::raw_decode(&detail_bytes).unwrap();
        assert_eq!(decoded_detail.target_protocol_version, expected);
        assert_eq!(detail_remainder, b"DTAIL");
    }
}

#[test]
fn malformed_protocol_version_extensions_fail_closed_after_existing_profile_extension() {
    let query = SnQueryResp {
        seq: 31u32.into(),
        peer_info: None,
        end_point_array: vec![],
        net_profile: Some(profile(5_000_000)),
        target_protocol_version: None,
    };
    let mut malformed_query = query.to_vec().unwrap();
    malformed_query.extend_from_slice(&SN_QUERY_RESP_PROTOCOL_VERSION_EXTENSION_MAGIC.to_be_bytes());
    malformed_query.push(SN_EXTENSION_VERSION);
    malformed_query.extend_from_slice(&0u32.to_be_bytes());
    let (decoded_query, query_remainder) = SnQueryResp::raw_decode(&malformed_query).unwrap();
    assert_eq!(decoded_query.net_profile, query.net_profile);
    assert_eq!(decoded_query.target_protocol_version, None);
    assert!(query_remainder.is_empty());

    let detail = SnDetailResp {
        peer_info: None,
        end_point_array: vec![],
        net_profile: Some(profile(5_000_000)),
        target_protocol_version: None,
    };
    let mut truncated_detail = detail.to_vec().unwrap();
    truncated_detail.extend_from_slice(&SN_DETAIL_RESP_PROTOCOL_VERSION_EXTENSION_MAGIC.to_be_bytes());
    let (decoded_detail, detail_remainder) = SnDetailResp::raw_decode(&truncated_detail).unwrap();
    assert_eq!(decoded_detail.net_profile, detail.net_profile);
    assert_eq!(decoded_detail.target_protocol_version, None);
    assert!(detail_remainder.is_empty());
}

#[test]
fn sn_protocol_version_constant_is_used_by_all_three_client_producers() {
    assert_eq!(SN_PROTOCOL_VERSION, 1);
    let client_source = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/sn/client/sn_service.rs"
    ));
    assert_eq!(
        client_source
            .matches("protocol_version: SN_PROTOCOL_VERSION")
            .count(),
        3
    );
}
