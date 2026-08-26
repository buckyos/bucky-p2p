use super::*;
use crate::networks::allow_all_listen_vports;
use std::future;
use tokio::io::{AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::Notify;

struct NoopSender;

impl ControlDataSender for NoopSender {
    fn send(&self, _payload: Vec<u8>) -> ControlDataSenderFuture {
        Box::pin(async { Ok(()) })
    }
}

struct CaptureSender {
    tx: mpsc::Sender<Vec<u8>>,
}

impl ControlDataSender for CaptureSender {
    fn send(&self, payload: Vec<u8>) -> ControlDataSenderFuture {
        let tx = self.tx.clone();
        Box::pin(async move {
            assert!(payload.len() <= MAX_CONTROL_DATA_FRAME_SIZE);
            tx.send(payload)
                .await
                .map_err(|_| p2p_err!(P2pErrorCode::Interrupted, "capture sender closed"))
        })
    }
}

struct NeverSender;

impl ControlDataSender for NeverSender {
    fn send(&self, _payload: Vec<u8>) -> ControlDataSenderFuture {
        Box::pin(future::pending())
    }
}

struct OrderedSender {
    tx: mpsc::Sender<ControlStreamFrame>,
    data_gate: Arc<Notify>,
}

impl ControlDataSender for OrderedSender {
    fn send(&self, payload: Vec<u8>) -> ControlDataSenderFuture {
        let tx = self.tx.clone();
        let data_gate = self.data_gate.clone();
        Box::pin(async move {
            let frame = ControlStreamFrame::clone_from_slice(payload.as_slice())
                .map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
            if matches!(frame, ControlStreamFrame::Data { .. }) {
                data_gate.notified().await;
            }
            tx.send(frame)
                .await
                .map_err(|_| p2p_err!(P2pErrorCode::Interrupted, "ordered sender closed"))
        })
    }
}

fn linked_runtimes() -> (ControlStreamRuntime, ControlStreamRuntime) {
    let (a_tx, mut a_rx) = mpsc::channel::<Vec<u8>>(CONTROL_STREAM_QUEUE_CAPACITY);
    let (b_tx, mut b_rx) = mpsc::channel::<Vec<u8>>(CONTROL_STREAM_QUEUE_CAPACITY);
    let a = ControlStreamRuntime::new(true, Arc::new(a_tx));
    let b = ControlStreamRuntime::new(false, Arc::new(b_tx));
    let a_in = a.clone();
    runtime::task::spawn(async move {
        while let Some(payload) = b_rx.recv().await {
            let _ = a_in.on_data(payload).await;
        }
    });
    let b_in = b.clone();
    runtime::task::spawn(async move {
        while let Some(payload) = a_rx.recv().await {
            let _ = b_in.on_data(payload).await;
        }
    });
    (a, b)
}

fn poll_write_once(write: &mut ControlStreamWrite, bytes: &[u8]) -> Poll<io::Result<usize>> {
    let waker = std::task::Waker::noop();
    let mut cx = Context::from_waker(waker);
    Pin::new(write).poll_write(&mut cx, bytes)
}

#[tokio::test]
async fn control_stream_open_listen_and_transfer() {
    let (a, b) = linked_runtimes();
    let (accepted_tx, mut accepted_rx) = mpsc::channel(1);
    b.listen(
        allow_all_listen_vports(),
        Arc::new(move |result| {
            let accepted_tx = accepted_tx.clone();
            Box::pin(async move {
                accepted_tx.send(result).await.unwrap();
            })
        }),
    )
    .await
    .unwrap();

    let purpose = TunnelPurpose::from_value(&7u16).unwrap();
    let (mut a_read, mut a_write) = a.open(purpose.clone()).await.unwrap();
    let (_accepted_purpose, mut b_read, mut b_write) =
        accepted_rx.recv().await.unwrap().unwrap();

    a_write.write_all(b"ping").await.unwrap();
    let mut buf = [0u8; 4];
    b_read.read_exact(&mut buf).await.unwrap();
    assert_eq!(&buf, b"ping");

    b_write.write_all(b"pong").await.unwrap();
    a_read.read_exact(&mut buf).await.unwrap();
    assert_eq!(&buf, b"pong");
}

#[tokio::test]
async fn callback_can_wait_for_stream_data_without_blocking_control_receive() {
    let (a, b) = linked_runtimes();
    let (result_tx, result_rx) = oneshot::channel();
    let result_tx = Arc::new(Mutex::new(Some(result_tx)));
    b.listen(
        allow_all_listen_vports(),
        Arc::new(move |result| {
            let result_tx = result_tx.clone();
            Box::pin(async move {
                let (_, mut read, _) = result.unwrap();
                let mut bytes = [0u8; 4];
                read.read_exact(&mut bytes).await.unwrap();
                if let Some(tx) = result_tx.lock().unwrap().take() {
                    let _ = tx.send(bytes);
                }
            })
        }),
    )
    .await
    .unwrap();

    let (_, mut write) = a
        .open(TunnelPurpose::from_value(&11u16).unwrap())
        .await
        .unwrap();
    write.write_all(b"data").await.unwrap();
    let bytes = runtime::timeout(Duration::from_secs(1), result_rx)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&bytes, b"data");
}

#[tokio::test]
async fn control_stream_rejects_invalid_and_duplicate_peer_ids() {
    let (capture_tx, mut capture_rx) = mpsc::channel(4);
    let runtime = ControlStreamRuntime::new(true, Arc::new(CaptureSender { tx: capture_tx }));
    let (accepted_tx, mut accepted_rx) = mpsc::channel(1);
    runtime
        .listen(
            allow_all_listen_vports(),
            Arc::new(move |result| {
                let accepted_tx = accepted_tx.clone();
                Box::pin(async move {
                    accepted_tx.send(result).await.unwrap();
                })
            }),
        )
        .await
        .unwrap();
    let purpose = TunnelPurpose::from_value(&13u16).unwrap();

    runtime
        .handle_open(1, purpose.clone())
        .await
        .unwrap();
    assert!(matches!(
        ControlStreamFrame::clone_from_slice(&capture_rx.recv().await.unwrap()).unwrap(),
        ControlStreamFrame::OpenResp {
            stream_id: 1,
            result: TunnelCommandResult::InvalidParam
        }
    ));
    assert!(runtime.inner.streams.lock().unwrap().is_empty());

    runtime.handle_open(2, purpose.clone()).await.unwrap();
    assert!(matches!(
        ControlStreamFrame::clone_from_slice(&capture_rx.recv().await.unwrap()).unwrap(),
        ControlStreamFrame::OpenResp {
            stream_id: 2,
            result: TunnelCommandResult::Success
        }
    ));
    let _accepted = accepted_rx.recv().await.unwrap().unwrap();

    runtime.handle_open(2, purpose).await.unwrap();
    assert!(matches!(
        ControlStreamFrame::clone_from_slice(&capture_rx.recv().await.unwrap()).unwrap(),
        ControlStreamFrame::OpenResp {
            stream_id: 2,
            result: TunnelCommandResult::ConflictLost
        }
    ));
    assert_eq!(runtime.inner.streams.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn stream_id_allocation_skips_zero_and_live_ids() {
    let runtime = ControlStreamRuntime::new(true, Arc::new(NoopSender));
    runtime.inner.next_stream_id.store(0, Ordering::SeqCst);
    assert_eq!(runtime.alloc_stream_id(), 2);

    let (tx, _rx) = ControlStreamRuntime::inbound_channel();
    runtime.inner.streams.lock().unwrap().insert(5, tx);
    runtime.inner.next_stream_id.store(5, Ordering::SeqCst);
    assert_eq!(runtime.alloc_stream_id(), 7);
}

#[tokio::test]
async fn inbound_overflow_resets_only_the_full_stream_and_preserves_terminal_order() {
    let (capture_tx, mut capture_rx) = mpsc::channel(1);
    let runtime = ControlStreamRuntime::new(true, Arc::new(CaptureSender { tx: capture_tx }));
    let (full_tx, mut full_rx) = ControlStreamRuntime::inbound_channel();
    let (healthy_tx, mut healthy_rx) = ControlStreamRuntime::inbound_channel();
    runtime.inner.streams.lock().unwrap().insert(2, full_tx);
    runtime.inner.streams.lock().unwrap().insert(4, healthy_tx);

    for _ in 0..CONTROL_STREAM_QUEUE_CAPACITY {
        runtime.deliver(2, vec![1]).await.unwrap();
    }
    runtime.deliver(2, vec![2]).await.unwrap();
    assert!(!runtime.inner.closed.load(Ordering::SeqCst));
    assert!(!runtime.inner.streams.lock().unwrap().contains_key(&2));
    assert!(runtime.inner.streams.lock().unwrap().contains_key(&4));

    runtime.deliver(4, vec![9]).await.unwrap();
    assert!(matches!(healthy_rx.recv().await, Some(InboundItem::Data(bytes)) if bytes == vec![9]));
    assert!(matches!(
        ControlStreamFrame::clone_from_slice(&capture_rx.recv().await.unwrap()).unwrap(),
        ControlStreamFrame::Reset {
            stream_id: 2,
            reason: TunnelCommandResult::AcceptQueueFull
        }
    ));

    for _ in 0..CONTROL_STREAM_QUEUE_CAPACITY {
        assert!(matches!(full_rx.recv().await, Some(InboundItem::Data(_))));
    }
    assert!(matches!(
        full_rx.recv().await,
        Some(InboundItem::Reset(P2pErrorCode::OutOfLimit, _))
    ));
}

#[tokio::test]
async fn reader_closed_resets_only_its_stream() {
    let (capture_tx, mut capture_rx) = mpsc::channel(1);
    let runtime = ControlStreamRuntime::new(true, Arc::new(CaptureSender { tx: capture_tx }));
    let (tx, rx) = ControlStreamRuntime::inbound_channel();
    runtime.inner.streams.lock().unwrap().insert(2, tx);
    drop(rx);

    runtime.deliver(2, vec![1]).await.unwrap();
    assert!(!runtime.inner.closed.load(Ordering::SeqCst));
    assert!(matches!(
        ControlStreamFrame::clone_from_slice(&capture_rx.recv().await.unwrap()).unwrap(),
        ControlStreamFrame::Reset {
            stream_id: 2,
            reason: TunnelCommandResult::Interrupted
        }
    ));
}

#[tokio::test]
async fn shutdown_waits_for_pending_data_before_sending_fin() {
    let (frame_tx, mut frame_rx) = mpsc::channel(2);
    let data_gate = Arc::new(Notify::new());
    let runtime = ControlStreamRuntime::new(
        true,
        Arc::new(OrderedSender {
            tx: frame_tx,
            data_gate: data_gate.clone(),
        }),
    );
    let mut write = ControlStreamWrite::new(1, runtime.inner.clone());

    assert!(matches!(poll_write_once(&mut write, b"data"), Poll::Pending));
    data_gate.notify_one();
    write.shutdown().await.unwrap();

    assert!(matches!(
        frame_rx.recv().await,
        Some(ControlStreamFrame::Data { stream_id: 1, bytes }) if bytes == b"data"
    ));
    assert!(matches!(
        frame_rx.recv().await,
        Some(ControlStreamFrame::Fin { stream_id: 1 })
    ));
}

#[tokio::test]
async fn runtime_close_fails_pending_and_subsequent_writes() {
    let runtime = ControlStreamRuntime::new(true, Arc::new(NeverSender));
    let mut pending_write = ControlStreamWrite::new(1, runtime.inner.clone());
    assert!(matches!(
        poll_write_once(&mut pending_write, b"pending"),
        Poll::Pending
    ));

    runtime.close_all(p2p_err!(P2pErrorCode::Interrupted, "runtime stopped"));
    assert!(matches!(
        poll_write_once(&mut pending_write, b"pending"),
        Poll::Ready(Err(_))
    ));

    let mut later_write = ControlStreamWrite::new(3, runtime.inner.clone());
    assert!(later_write.write_all(b"later").await.is_err());
    assert!(later_write.flush().await.is_err());
    assert!(later_write.shutdown().await.is_err());
}

#[tokio::test]
async fn partial_reads_advance_an_offset_without_front_drain() {
    let (tx, rx) = ControlStreamRuntime::inbound_channel();
    let bytes = (0u8..32).collect::<Vec<_>>();
    tx.send(InboundItem::Data(bytes.clone())).await.unwrap();
    tx.send(InboundItem::Fin).await.unwrap();
    let mut read = ControlStreamRead::new(rx);

    for (index, expected) in bytes.iter().enumerate() {
        let mut one = [0u8; 1];
        read.read_exact(&mut one).await.unwrap();
        assert_eq!(one[0], *expected);
        if index + 1 < bytes.len() {
            assert_eq!(read.pending.len(), bytes.len());
            assert_eq!(read.pending_offset, index + 1);
        }
    }
    assert!(read.pending.is_empty());
    assert_eq!(read.pending_offset, 0);
    let mut eof = [0u8; 1];
    assert_eq!(read.read(&mut eof).await.unwrap(), 0);
}

#[tokio::test]
async fn control_stream_rejects_oversized_outer_data() {
    let runtime = ControlStreamRuntime::new(true, Arc::new(NoopSender));
    let err = runtime
        .on_data(vec![0u8; MAX_CONTROL_DATA_FRAME_SIZE + 1])
        .await
        .err()
        .unwrap();
    assert_eq!(err.code(), P2pErrorCode::InvalidData);
    let err = runtime
        .open(TunnelPurpose::from_value(&9u16).unwrap())
        .await
        .err()
        .unwrap();
    assert_eq!(err.code(), P2pErrorCode::InvalidData);
}

#[tokio::test]
async fn control_stream_canceled_open_cleans_pending_and_ignores_late_response() {
    let (capture_tx, mut capture_rx) = mpsc::channel(1);
    let runtime = ControlStreamRuntime::new(true, Arc::new(CaptureSender { tx: capture_tx }));
    let opener = runtime.clone();
    let task = runtime::task::spawn(async move {
        opener.open(TunnelPurpose::from_value(&9u16).unwrap()).await
    });

    assert!(capture_rx.recv().await.is_some());
    task.abort();
    let _ = task.await;
    assert!(runtime.inner.pending_opens.lock().unwrap().is_empty());

    runtime
        .handle_open_resp(1, TunnelCommandResult::Success)
        .unwrap();
    assert!(runtime.inner.streams.lock().unwrap().is_empty());
}

#[tokio::test]
async fn control_stream_write_splits_below_outer_limit() {
    let (capture_tx, mut capture_rx) = mpsc::channel(4);
    let runtime = ControlStreamRuntime::new(true, Arc::new(CaptureSender { tx: capture_tx }));
    let mut write = ControlStreamWrite::new(1, runtime.inner.clone());
    let written = write
        .write(&vec![1u8; MAX_CONTROL_DATA_FRAME_SIZE * 2])
        .await
        .unwrap();
    assert_eq!(written, CONTROL_STREAM_WRITE_CHUNK);
    assert!(capture_rx.recv().await.unwrap().len() <= MAX_CONTROL_DATA_FRAME_SIZE);
}
