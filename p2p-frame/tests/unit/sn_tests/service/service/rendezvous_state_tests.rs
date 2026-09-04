use super::*;
use crate::endpoint::{Endpoint, EndpointArea, Protocol};
use crate::sn::protocol::{
    SN_TUNNEL_RENDEZVOUS_RESULT_FAILED, SnTunnelRendezvous, SnTunnelRendezvousOperation,
    SnTunnelRendezvousResp,
};
use crate::sn::service::rendezvous_state::MAX_INFLIGHT_WAITERS_PER_ATTEMPT;
use crate::types::Sequence;

const REQUEST_LIFETIME: Timestamp = 30 * 1_000_000;

fn peer(seed: u64) -> P2pId {
    let mut bytes = vec![0u8; 32];
    bytes[..8].copy_from_slice(&seed.to_be_bytes());
    P2pId::from(bytes)
}

fn endpoint(port: u16) -> Endpoint {
    let mut endpoint = Endpoint::from((
        Protocol::Quic,
        "192.0.2.10".parse::<std::net::IpAddr>().unwrap(),
        port,
    ));
    endpoint.set_area(EndpointArea::ServerReflexive);
    endpoint
}

fn request(target: u64, seq: u32, tunnel_id: u32, port: u16) -> SnTunnelRendezvous {
    SnTunnelRendezvous {
        seq: Sequence::from(seq),
        tunnel_id: TunnelId::from(tunnel_id),
        to_peer_id: peer(target),
        operation: SnTunnelRendezvousOperation::PunchOnly,
        end_point_array: vec![endpoint(port)],
        need_predict_endpoint: false,
    }
}

fn response(request: &SnTunnelRendezvous) -> SnTunnelRendezvousResp {
    SnTunnelRendezvousResp::success(request.seq, Vec::new())
}

fn error_code<T>(result: P2pResult<T>) -> P2pErrorCode {
    result.err().expect("expected error").code()
}

#[tokio::test]
async fn rendezvous_state_authenticated_initiator_seq_and_tunnel_key_exact_duplicates_share_waiter_and_cache()
 {
    let now = 1_000_000;
    let initiator = peer(1);
    let other_initiator = peer(2);
    let request = request(10, 7, 9, 40_001);
    let mut state = RendezvousState::new();

    assert!(matches!(
        state.begin(&initiator, &request, now),
        Ok(RendezvousBegin::New)
    ));
    let waiter = match state.begin(&initiator, &request, now).unwrap() {
        RendezvousBegin::InFlight(waiter) => waiter,
        _ => panic!("exact duplicate must share the in-flight response"),
    };
    assert!(matches!(
        state.begin(&other_initiator, &request, now),
        Ok(RendezvousBegin::New)
    ));

    let cached = response(&request);
    state
        .cache_response(&initiator, &request, cached.clone(), now)
        .unwrap();
    assert_eq!(waiter.await.unwrap().unwrap(), cached);
    assert!(matches!(
        state.begin(&initiator, &request, now),
        Ok(RendezvousBegin::Cached(value)) if value == cached
    ));
}

#[tokio::test]
async fn rendezvous_state_prediction_response_is_live_only_and_replay_is_generic() {
    let now = 1_500_000;
    let initiator = peer(21);
    let mut request = request(22, 70, 90, 40_010);
    request.need_predict_endpoint = true;
    let mut state = RendezvousState::new();

    assert!(matches!(
        state.begin(&initiator, &request, now),
        Ok(RendezvousBegin::New)
    ));
    let waiter = match state.begin(&initiator, &request, now).unwrap() {
        RendezvousBegin::InFlight(waiter) => waiter,
        _ => panic!("exact duplicate must share the live response"),
    };
    let live = SnTunnelRendezvousResp::success(request.seq, vec![endpoint(40_011)]);
    state
        .cache_response(&initiator, &request, live.clone(), now)
        .unwrap();

    assert_eq!(waiter.await.unwrap().unwrap(), live);
    match state.begin(&initiator, &request, now).unwrap() {
        RendezvousBegin::Cached(replay) => {
            assert_eq!(replay.result, SN_TUNNEL_RENDEZVOUS_RESULT_FAILED);
            assert!(replay.predicted_endpoint_array.is_empty());
        }
        _ => panic!("completed prediction request must replay only a generic failure"),
    }
}

#[test]
fn rendezvous_state_same_authenticated_key_with_any_request_change_is_conflict() {
    let now = 2_000_000;
    let initiator = peer(3);
    let request = request(11, 8, 10, 40_002);
    let mut state = RendezvousState::new();
    state.begin(&initiator, &request, now).unwrap();

    let mut changed_target = request.clone();
    changed_target.to_peer_id = peer(12);
    assert_eq!(
        error_code(state.begin(&initiator, &changed_target, now)),
        P2pErrorCode::Conflict
    );

    let mut changed_body = request.clone();
    changed_body.end_point_array[0] = endpoint(40_003);
    assert_eq!(
        error_code(state.begin(&initiator, &changed_body, now)),
        P2pErrorCode::Conflict
    );

    let mut changed_operation = request.clone();
    changed_operation.operation = SnTunnelRendezvousOperation::ReverseConnectOnly;
    assert_eq!(
        error_code(state.begin(&initiator, &changed_operation, now)),
        P2pErrorCode::Conflict
    );
}

#[test]
fn rendezvous_state_cache_response_reports_not_found_and_request_conflict() {
    let now = 2_500_000;
    let initiator = peer(31);
    let request = request(32, 80, 100, 40_020);
    let mut state = RendezvousState::new();

    assert_eq!(
        error_code(state.cache_response(&initiator, &request, response(&request), now)),
        P2pErrorCode::NotFound
    );

    state.begin(&initiator, &request, now).unwrap();
    let mut changed_request = request.clone();
    changed_request.end_point_array[0] = endpoint(40_021);
    assert_eq!(
        error_code(state.cache_response(
            &initiator,
            &changed_request,
            response(&changed_request),
            now,
        )),
        P2pErrorCode::Conflict
    );
    state
        .cache_response(&initiator, &request, response(&request), now)
        .unwrap();
}

#[tokio::test]
async fn rendezvous_state_strict_thirty_second_expiry_rejects_late_cache_and_wakes_waiter() {
    let now = 3_000_000;
    let expires_at = now + REQUEST_LIFETIME;
    let initiator = peer(4);
    let request = request(13, 9, 11, 40_004);
    let mut state = RendezvousState::new();
    state.begin(&initiator, &request, now).unwrap();
    let waiter = match state.begin(&initiator, &request, now).unwrap() {
        RendezvousBegin::InFlight(waiter) => waiter,
        _ => panic!("exact duplicate must share the in-flight response"),
    };

    assert_eq!(
        error_code(state.cache_response(&initiator, &request, response(&request), expires_at,)),
        P2pErrorCode::Expired
    );
    assert_eq!(
        waiter.await.unwrap().unwrap_err().code(),
        P2pErrorCode::Expired
    );

    assert!(matches!(
        state.begin(&initiator, &request, expires_at),
        Ok(RendezvousBegin::New)
    ));
    assert_eq!(
        error_code(state.cache_response(
            &initiator,
            &request,
            response(&request),
            expires_at + REQUEST_LIFETIME,
        )),
        P2pErrorCode::Expired
    );
}

#[test]
fn rendezvous_state_pair_total_rate_and_duplicate_waiter_capacity_are_bounded() {
    let now = 4_000_000;
    let initiator = peer(5);

    let mut pair_state = RendezvousState::new();
    for index in 0..8 {
        assert!(matches!(
            pair_state.begin(
                &initiator,
                &request(20, 100 + index, 200 + index, 41_000 + index as u16),
                now,
            ),
            Ok(RendezvousBegin::New)
        ));
    }
    assert_eq!(
        error_code(pair_state.begin(&initiator, &request(20, 999, 999, 41_999), now)),
        P2pErrorCode::OutOfLimit
    );

    let mut rate_state = RendezvousState::new();
    for index in 0..32 {
        assert!(matches!(
            rate_state.begin(
                &initiator,
                &request(
                    100 + u64::from(index),
                    1_000 + index,
                    2_000 + index,
                    42_000 + index as u16,
                ),
                now,
            ),
            Ok(RendezvousBegin::New)
        ));
    }
    assert_eq!(
        error_code(rate_state.begin(&initiator, &request(999, 9_999, 9_999, 42_999), now)),
        P2pErrorCode::OutOfLimit
    );

    let mut total_state = RendezvousState::new();
    for index in 0..256u32 {
        assert!(matches!(
            total_state.begin(
                &peer(10_000 + u64::from(index)),
                &request(
                    20_000,
                    20_000 + index,
                    30_000 + index,
                    43_000 + index as u16,
                ),
                now,
            ),
            Ok(RendezvousBegin::New)
        ));
    }
    assert_eq!(
        error_code(total_state.begin(&peer(99_000), &request(20_000, 60_000, 60_000, 45_000), now)),
        P2pErrorCode::OutOfLimit
    );

    let duplicate = request(88_001, 8_800, 8_800, 46_000);
    let duplicate_initiator = peer(88_000);
    let mut duplicate_state = RendezvousState::new();
    duplicate_state
        .begin(&duplicate_initiator, &duplicate, now)
        .unwrap();
    let mut waiters = Vec::new();
    for _ in 0..MAX_INFLIGHT_WAITERS_PER_ATTEMPT {
        match duplicate_state
            .begin(&duplicate_initiator, &duplicate, now)
            .unwrap()
        {
            RendezvousBegin::InFlight(waiter) => waiters.push(waiter),
            _ => panic!("duplicate must join the bounded waiter set"),
        }
    }
    assert_eq!(
        error_code(duplicate_state.begin(&duplicate_initiator, &duplicate, now)),
        P2pErrorCode::OutOfLimit
    );
}

#[test]
fn rendezvous_state_rate_window_releases_only_after_sixty_seconds() {
    let now = 4_500_000;
    let initiator = peer(5_500);
    let mut state = RendezvousState::new();

    for index in 0..32u32 {
        assert!(matches!(
            state.begin(
                &initiator,
                &request(
                    6_000 + u64::from(index),
                    7_000 + index,
                    8_000 + index,
                    46_100 + index as u16,
                ),
                now,
            ),
            Ok(RendezvousBegin::New)
        ));
    }

    let next = request(6_100, 7_100, 8_100, 46_200);
    assert_eq!(
        error_code(state.begin(&initiator, &next, now + 60 * 1_000_000)),
        P2pErrorCode::OutOfLimit
    );
    assert!(matches!(
        state.begin(&initiator, &next, now + 60 * 1_000_000 + 1),
        Ok(RendezvousBegin::New)
    ));
}

#[tokio::test]
async fn rendezvous_state_fail_unanswered_and_remove_peer_release_entries_waiters_and_rate_budget()
{
    let now = 5_000_000;
    let initiator = peer(6);
    let target = peer(300);
    let first = request(300, 21, 31, 47_000);
    let mut state = RendezvousState::new();

    state.begin(&initiator, &first, now).unwrap();
    let failed_waiter = match state.begin(&initiator, &first, now).unwrap() {
        RendezvousBegin::InFlight(waiter) => waiter,
        _ => panic!("exact duplicate must share the in-flight response"),
    };
    state.fail_unanswered(&initiator, &first);
    assert_eq!(
        failed_waiter.await.unwrap().unwrap_err().code(),
        P2pErrorCode::NetworkError
    );
    assert!(matches!(
        state.begin(&initiator, &first, now),
        Ok(RendezvousBegin::New)
    ));

    let removed_waiter = match state.begin(&initiator, &first, now).unwrap() {
        RendezvousBegin::InFlight(waiter) => waiter,
        _ => panic!("exact duplicate must share the in-flight response"),
    };
    state.remove_peer(&target);
    assert_eq!(
        removed_waiter.await.unwrap().unwrap_err().code(),
        P2pErrorCode::UserCanceled
    );
    assert!(matches!(
        state.begin(&initiator, &first, now),
        Ok(RendezvousBegin::New)
    ));

    let initiator_removed_waiter = match state.begin(&initiator, &first, now).unwrap() {
        RendezvousBegin::InFlight(waiter) => waiter,
        _ => panic!("exact duplicate must share the in-flight response"),
    };
    state.remove_peer(&initiator);
    assert_eq!(
        initiator_removed_waiter.await.unwrap().unwrap_err().code(),
        P2pErrorCode::UserCanceled
    );
    for index in 0..32 {
        state
            .begin(
                &initiator,
                &request(
                    400 + u64::from(index),
                    3_000 + index,
                    4_000 + index,
                    48_000 + index as u16,
                ),
                now,
            )
            .unwrap();
    }
}
