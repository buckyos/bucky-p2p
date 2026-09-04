use bucky_raw_codec::RawConvertTo;
use p2p_frame::endpoint::{Endpoint, EndpointArea, Protocol};
use p2p_frame::error::P2pErrorCode;
use p2p_frame::p2p_identity::P2pId;
use p2p_frame::sn::protocol::{
    MAX_SN_TUNNEL_RENDEZVOUS_ENDPOINTS, PackageCmdCode, SN_TUNNEL_RENDEZVOUS_CMD_VERSION,
    SN_TUNNEL_RENDEZVOUS_RESULT_FAILED, SN_TUNNEL_RENDEZVOUS_RESULT_OK, SnTunnelRendezvous,
    SnTunnelRendezvousNotify, SnTunnelRendezvousOperation, SnTunnelRendezvousResp,
};
use p2p_frame::types::{Sequence, TunnelId};

fn peer(seed: u8) -> P2pId {
    P2pId::from(vec![seed; 32])
}

fn endpoint(protocol: Protocol, address: &str, port: u16, area: EndpointArea) -> Endpoint {
    let mut endpoint =
        Endpoint::from((protocol, address.parse::<std::net::IpAddr>().unwrap(), port));
    endpoint.set_area(area);
    endpoint
}

fn public_quic_endpoint(port: u16) -> Endpoint {
    endpoint(
        Protocol::Quic,
        "192.0.2.20",
        port,
        EndpointArea::ServerReflexive,
    )
}

fn request(
    operation: SnTunnelRendezvousOperation,
    end_point_array: Vec<Endpoint>,
    need_predict_endpoint: bool,
) -> SnTunnelRendezvous {
    SnTunnelRendezvous {
        seq: Sequence::from(17),
        tunnel_id: TunnelId::from(23),
        to_peer_id: peer(3),
        operation,
        end_point_array,
        need_predict_endpoint,
    }
}

fn notify(request: &SnTunnelRendezvous) -> SnTunnelRendezvousNotify {
    SnTunnelRendezvousNotify {
        seq: request.seq,
        tunnel_id: request.tunnel_id,
        peer_info: vec![9, 8, 7, 6],
        operation: request.operation,
        end_point_array: request.end_point_array.clone(),
        need_predict_endpoint: request.need_predict_endpoint,
    }
}

#[test]
fn flat_request_notify_and_response_exact_fields_round_trip() {
    let request = request(
        SnTunnelRendezvousOperation::PunchOnly,
        vec![public_quic_endpoint(40_000)],
        true,
    );
    request.validate().unwrap();
    let decoded = SnTunnelRendezvous::clone_from_slice(&request.to_vec().unwrap()).unwrap();
    assert_eq!(decoded, request);
    assert_eq!(decoded.seq, Sequence::from(17));
    assert_eq!(decoded.tunnel_id, TunnelId::from(23));
    assert_eq!(decoded.to_peer_id, peer(3));
    assert_eq!(decoded.operation, SnTunnelRendezvousOperation::PunchOnly);
    assert_eq!(decoded.end_point_array, vec![public_quic_endpoint(40_000)]);
    assert!(decoded.need_predict_endpoint);

    let notify = notify(&request);
    notify.validate().unwrap();
    let decoded = SnTunnelRendezvousNotify::clone_from_slice(&notify.to_vec().unwrap()).unwrap();
    assert_eq!(decoded, notify);
    assert_eq!(decoded.seq, request.seq);
    assert_eq!(decoded.tunnel_id, request.tunnel_id);
    assert_eq!(decoded.peer_info, vec![9, 8, 7, 6]);
    assert_eq!(decoded.operation, request.operation);
    assert_eq!(decoded.end_point_array, request.end_point_array);
    assert!(decoded.need_predict_endpoint);

    let response = SnTunnelRendezvousResp::success(request.seq, vec![public_quic_endpoint(40_001)]);
    response.validate(request.seq, true).unwrap();
    let decoded = SnTunnelRendezvousResp::clone_from_slice(&response.to_vec().unwrap()).unwrap();
    assert_eq!(decoded, response);
    assert_eq!(decoded.seq, request.seq);
    assert_eq!(decoded.result, 0);
    assert_eq!(
        decoded.predicted_endpoint_array,
        vec![public_quic_endpoint(40_001)]
    );
}

#[test]
fn rendezvous_command_version_ids_and_result_values_are_isolated() {
    assert_eq!(SN_TUNNEL_RENDEZVOUS_CMD_VERSION, 1);
    assert_eq!(PackageCmdCode::SnTunnelRendezvous as u8, 0x2c);
    assert_eq!(PackageCmdCode::SnTunnelRendezvousNotify as u8, 0x2d);
    assert_eq!(
        PackageCmdCode::try_from(0x2c).unwrap(),
        PackageCmdCode::SnTunnelRendezvous
    );
    assert_eq!(
        PackageCmdCode::try_from(0x2d).unwrap(),
        PackageCmdCode::SnTunnelRendezvousNotify
    );
    for obsolete_layout_command in 0x28..=0x2b {
        assert!(PackageCmdCode::try_from(obsolete_layout_command).is_err());
    }

    assert_eq!(SN_TUNNEL_RENDEZVOUS_RESULT_OK, 0);
    assert_eq!(SN_TUNNEL_RENDEZVOUS_RESULT_FAILED, 1);
    assert_eq!(SN_TUNNEL_RENDEZVOUS_RESULT_OK, P2pErrorCode::Ok.into_u8());
    assert_eq!(
        SN_TUNNEL_RENDEZVOUS_RESULT_FAILED,
        P2pErrorCode::Failed.into_u8()
    );

    let success = SnTunnelRendezvousResp::success(Sequence::from(31), Vec::new());
    assert!(success.is_success());
    assert_eq!(success.result, 0);
    let failure = SnTunnelRendezvousResp::failure(Sequence::from(31));
    assert!(!failure.is_success());
    assert_eq!(failure.result, 1);

    let mut unknown = failure;
    unknown.result = 2;
    assert!(unknown.validate(Sequence::from(31), false).is_err());
}

#[test]
fn flat_decoders_reject_trailing_bytes() {
    let request = request(
        SnTunnelRendezvousOperation::PunchOnly,
        vec![public_quic_endpoint(41_000)],
        true,
    );
    let notify = notify(&request);
    let response = SnTunnelRendezvousResp::success(request.seq, vec![public_quic_endpoint(41_001)]);

    let mut request_bytes = request.to_vec().unwrap();
    request_bytes.push(0xff);
    assert!(SnTunnelRendezvous::clone_from_slice(&request_bytes).is_err());

    let mut notify_bytes = notify.to_vec().unwrap();
    notify_bytes.push(0xff);
    assert!(SnTunnelRendezvousNotify::clone_from_slice(&notify_bytes).is_err());

    let mut response_bytes = response.to_vec().unwrap();
    response_bytes.push(0xff);
    assert!(SnTunnelRendezvousResp::clone_from_slice(&response_bytes).is_err());

    let mut request_bytes = request.to_vec().unwrap();
    request_bytes.pop();
    assert!(SnTunnelRendezvous::clone_from_slice(&request_bytes).is_err());

    let mut notify_bytes = notify.to_vec().unwrap();
    notify_bytes.pop();
    assert!(SnTunnelRendezvousNotify::clone_from_slice(&notify_bytes).is_err());

    let mut response_bytes = response.to_vec().unwrap();
    response_bytes.pop();
    assert!(SnTunnelRendezvousResp::clone_from_slice(&response_bytes).is_err());
}

#[test]
fn zero_sequence_and_invalid_operation_discriminant_are_rejected() {
    let mut zero_seq = request(
        SnTunnelRendezvousOperation::PunchOnly,
        vec![public_quic_endpoint(41_100)],
        false,
    );
    zero_seq.seq = Sequence::from(0);
    assert!(zero_seq.validate().is_err());

    let valid = request(
        SnTunnelRendezvousOperation::PunchOnly,
        vec![public_quic_endpoint(41_101)],
        false,
    );
    let mut bytes = valid.to_vec().unwrap();
    let operation_offset = valid.seq.to_vec().unwrap().len()
        + valid.tunnel_id.to_vec().unwrap().len()
        + valid.to_peer_id.to_vec().unwrap().len();
    let operation_len = valid.operation.to_vec().unwrap().len();
    assert!(operation_len > 0);
    bytes[operation_offset..operation_offset + operation_len].fill(0xff);
    assert!(SnTunnelRendezvous::clone_from_slice(&bytes).is_err());
}

#[test]
fn operations_and_endpoint_domains_fail_closed() {
    for operation in [
        SnTunnelRendezvousOperation::PunchOnly,
        SnTunnelRendezvousOperation::PunchAndReverseConnect,
        SnTunnelRendezvousOperation::ReverseConnectOnly,
    ] {
        assert!(
            request(operation, vec![public_quic_endpoint(42_000)], false)
                .validate()
                .is_ok()
        );
        assert!(request(operation, Vec::new(), false).validate().is_err());
    }
    assert!(
        request(SnTunnelRendezvousOperation::WaitIncoming, Vec::new(), false)
            .validate()
            .is_ok()
    );
    assert!(
        request(
            SnTunnelRendezvousOperation::WaitIncoming,
            vec![public_quic_endpoint(42_001)],
            false,
        )
        .validate()
        .is_err()
    );

    let max = (0..MAX_SN_TUNNEL_RENDEZVOUS_ENDPOINTS)
        .map(|index| public_quic_endpoint(43_000 + index as u16))
        .collect::<Vec<_>>();
    assert!(
        request(SnTunnelRendezvousOperation::PunchOnly, max.clone(), false)
            .validate()
            .is_ok()
    );
    let mut over = max;
    over.push(public_quic_endpoint(44_000));
    assert!(
        request(SnTunnelRendezvousOperation::PunchOnly, over, false)
            .validate()
            .is_err()
    );

    let duplicate = public_quic_endpoint(44_001);
    assert!(
        request(
            SnTunnelRendezvousOperation::PunchOnly,
            vec![duplicate, duplicate],
            false,
        )
        .validate()
        .is_err()
    );
    assert!(
        request(
            SnTunnelRendezvousOperation::PunchOnly,
            vec![endpoint(
                Protocol::Ext(7),
                "192.0.2.20",
                44_002,
                EndpointArea::ServerReflexive,
            )],
            false,
        )
        .validate()
        .is_err()
    );
    assert!(
        request(
            SnTunnelRendezvousOperation::PunchOnly,
            vec![endpoint(
                Protocol::Quic,
                "192.168.1.20",
                44_003,
                EndpointArea::ServerReflexive,
            )],
            false,
        )
        .validate()
        .is_err()
    );
    assert!(
        request(
            SnTunnelRendezvousOperation::PunchOnly,
            vec![endpoint(
                Protocol::Quic,
                "192.0.2.20",
                44_004,
                EndpointArea::Lan,
            )],
            false,
        )
        .validate()
        .is_err()
    );
    assert!(
        request(
            SnTunnelRendezvousOperation::ReverseConnectOnly,
            vec![endpoint(
                Protocol::Quic,
                "2001:db8::1",
                44_005,
                EndpointArea::ServerReflexive,
            )],
            false,
        )
        .validate()
        .is_err()
    );
    assert!(
        request(
            SnTunnelRendezvousOperation::ReverseConnectOnly,
            vec![endpoint(
                Protocol::Quic,
                "192.0.2.20",
                0,
                EndpointArea::ServerReflexive,
            )],
            false,
        )
        .validate()
        .is_err()
    );
    assert!(
        request(
            SnTunnelRendezvousOperation::ReverseConnectOnly,
            vec![
                public_quic_endpoint(44_006),
                endpoint(
                    Protocol::Tcp,
                    "192.0.2.20",
                    44_007,
                    EndpointArea::ServerReflexive,
                ),
            ],
            false,
        )
        .validate()
        .is_err()
    );

    for area in [EndpointArea::Wan, EndpointArea::Mapped] {
        let public_reverse = endpoint(Protocol::Quic, "192.0.2.21", 44_008, area);
        let reverse_request = request(
            SnTunnelRendezvousOperation::ReverseConnectOnly,
            vec![public_reverse],
            false,
        );
        assert!(reverse_request.validate().is_ok());
        assert!(notify(&reverse_request).validate().is_ok());

        assert!(
            request(
                SnTunnelRendezvousOperation::PunchAndReverseConnect,
                vec![public_reverse],
                false,
            )
            .validate()
            .is_ok()
        );
        #[cfg(not(feature = "test-real-socket-matrix"))]
        assert!(
            request(
                SnTunnelRendezvousOperation::PunchOnly,
                vec![public_reverse],
                false,
            )
            .validate()
            .is_err()
        );

        let public_tcp_reverse = endpoint(Protocol::Tcp, "192.0.2.22", 44_009, area);
        let tcp_reverse_request = request(
            SnTunnelRendezvousOperation::ReverseConnectOnly,
            vec![public_tcp_reverse],
            false,
        );
        assert!(tcp_reverse_request.validate().is_ok());
        assert!(notify(&tcp_reverse_request).validate().is_ok());
        assert!(
            request(
                SnTunnelRendezvousOperation::PunchAndReverseConnect,
                vec![public_tcp_reverse],
                false,
            )
            .validate()
            .is_err()
        );
    }
}

#[test]
fn response_prediction_true_false_and_failure_shapes_are_unambiguous() {
    let seq = Sequence::from(51);
    assert!(
        SnTunnelRendezvousResp::success(seq, Vec::new())
            .validate(seq, false)
            .is_ok()
    );
    assert!(
        SnTunnelRendezvousResp::success(seq, vec![public_quic_endpoint(45_000)])
            .validate(seq, false)
            .is_err()
    );
    assert!(
        SnTunnelRendezvousResp::success(seq, vec![public_quic_endpoint(45_001)])
            .validate(seq, true)
            .is_ok()
    );
    assert!(
        SnTunnelRendezvousResp::success(seq, Vec::new())
            .validate(seq, true)
            .is_err()
    );

    let failure = SnTunnelRendezvousResp::failure(seq);
    assert!(failure.validate(seq, true).is_ok());
    let mut malformed_failure = failure;
    malformed_failure.predicted_endpoint_array = vec![public_quic_endpoint(45_002)];
    assert!(malformed_failure.validate(seq, true).is_err());

    assert!(
        SnTunnelRendezvousResp::success(seq, Vec::new())
            .validate(Sequence::from(52), false)
            .is_err()
    );
}

#[test]
fn obsolete_envelope_body_digest_result_and_terminal_symbols_are_absent() {
    let protocol_source = include_str!("../src/sn/protocol/sn.rs");
    let command_source = include_str!("../src/sn/protocol/common.rs");
    for obsolete_symbol in [
        "SnTunnelRendezvousEnvelope",
        concat!("SnTunnel", "RendezvousRequest", "Body"),
        "SnTunnelRendezvousResponseBody",
        "SnTunnelRendezvousDigestInput",
        "SnTunnelRendezvousResult",
        "SnTunnelRendezvousTerminal",
        "SnTunnelRendezvousComplete",
        "SnTunnelRendezvousCancel",
    ] {
        assert!(!protocol_source.contains(obsolete_symbol));
        assert!(!command_source.contains(obsolete_symbol));
    }
    for obsolete_field in [
        "attempt_id",
        "request_digest",
        "socket_binding_generation",
        "valid_until",
        "deadline",
    ] {
        assert!(
            !protocol_source[..protocol_source
                .find("pub(super) fn extension_measure")
                .unwrap()]
                .contains(obsolete_field)
        );
    }
}
