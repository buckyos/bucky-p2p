use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering as AtomicOrdering};

struct DropFlag(Arc<AtomicBool>);

impl Drop for DropFlag {
    fn drop(&mut self) {
        self.0.store(true, AtomicOrdering::SeqCst);
    }
}

fn remote() -> Endpoint {
    Endpoint::from((Protocol::Quic, "192.0.2.1:49152".parse().unwrap()))
}

#[tokio::test]
async fn udp_punch_quic_nat_connect_success_and_final_error_drop_owned_punch() {
    let success_punch_dropped = Arc::new(AtomicBool::new(false));
    let success_drop = DropFlag(success_punch_dropped.clone());
    let success = connect_with_owned_udp_punch(
        remote(),
        Instant::now(),
        Duration::from_secs(10),
        async { Ok::<_, P2pError>(7usize) },
        async move {
            let _drop_flag = success_drop;
            std::future::pending::<()>().await;
        },
    )
    .await
    .unwrap();
    assert_eq!(success, 7);
    assert!(success_punch_dropped.load(AtomicOrdering::SeqCst));

    let error_punch_dropped = Arc::new(AtomicBool::new(false));
    let error_drop = DropFlag(error_punch_dropped.clone());
    let error = connect_with_owned_udp_punch(
        remote(),
        Instant::now(),
        Duration::from_secs(10),
        async {
            Err::<usize, _>(p2p_err!(
                P2pErrorCode::ConnectFailed,
                "synthetic final connect error"
            ))
        },
        async move {
            let _drop_flag = error_drop;
            std::future::pending::<()>().await;
        },
    )
    .await
    .unwrap_err();
    assert_eq!(error.code(), P2pErrorCode::ConnectFailed);
    assert!(error_punch_dropped.load(AtomicOrdering::SeqCst));
}

#[tokio::test]
async fn udp_punch_quic_nat_owner_cancellation_drops_connect_and_punch_futures() {
    let connect_dropped = Arc::new(AtomicBool::new(false));
    let punch_dropped = Arc::new(AtomicBool::new(false));
    let connect_drop = connect_dropped.clone();
    let punch_drop = punch_dropped.clone();
    let owner = tokio::spawn(connect_with_owned_udp_punch(
        remote(),
        Instant::now(),
        Duration::from_secs(10),
        async move {
            let _drop_flag = DropFlag(connect_drop);
            std::future::pending::<P2pResult<()>>().await
        },
        async move {
            let _drop_flag = DropFlag(punch_drop);
            std::future::pending::<()>().await;
        },
    ));

    tokio::task::yield_now().await;
    owner.abort();
    assert!(owner.await.unwrap_err().is_cancelled());
    assert!(connect_dropped.load(AtomicOrdering::SeqCst));
    assert!(punch_dropped.load(AtomicOrdering::SeqCst));
}

#[tokio::test]
async fn udp_punch_quic_nat_connect_success_after_one_second_polls_once_and_drops_owned_punch() {
    let connect_polls = Arc::new(AtomicUsize::new(0));
    let punch_dropped = Arc::new(AtomicBool::new(false));
    let connect_poll = connect_polls.clone();
    let punch_drop = punch_dropped.clone();
    let started_at = Instant::now();

    let result = connect_with_owned_udp_punch(
        remote(),
        started_at,
        Duration::from_secs(2),
        async move {
            connect_poll.fetch_add(1, AtomicOrdering::SeqCst);
            tokio::time::sleep(Duration::from_millis(1100)).await;
            Ok::<_, P2pError>(23usize)
        },
        async move {
            let _drop_flag = DropFlag(punch_drop);
            std::future::pending::<()>().await;
        },
    )
    .await
    .unwrap();

    assert_eq!(result, 23);
    assert!(started_at.elapsed() >= Duration::from_secs(1));
    assert_eq!(connect_polls.load(AtomicOrdering::SeqCst), 1);
    assert!(punch_dropped.load(AtomicOrdering::SeqCst));
}

#[tokio::test]
async fn udp_punch_quic_nat_deadline_completion_keeps_awaiting_same_connect_future() {
    let connect_polls = Arc::new(AtomicUsize::new(0));
    let connect_poll = connect_polls.clone();
    let result = connect_with_owned_udp_punch(
        remote(),
        Instant::now(),
        Duration::from_secs(2),
        async move {
            connect_poll.fetch_add(1, AtomicOrdering::SeqCst);
            tokio::time::sleep(Duration::from_millis(25)).await;
            Ok::<_, P2pError>(29usize)
        },
        async {},
    )
    .await
    .unwrap();

    assert_eq!(result, 29);
    assert_eq!(connect_polls.load(AtomicOrdering::SeqCst), 1);
}
