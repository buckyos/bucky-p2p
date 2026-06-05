use std::collections::HashMap;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::task::{Context, Poll};
use std::time::Duration;

use bucky_raw_codec::{RawConvertTo, RawDecode, RawEncode, RawFrom};
use tokio::sync::{mpsc, oneshot};

use crate::error::{P2pError, P2pErrorCode, P2pResult, into_p2p_err, p2p_err};
use crate::networks::{
    IncomingControlStreamCallback, ListenVPortsRef, TunnelCommandResult, TunnelPurpose,
    TunnelStreamRead, TunnelStreamWrite,
};
use crate::runtime;

pub(crate) const MAX_CONTROL_DATA_FRAME_SIZE: usize = 64 * 1024;
const CONTROL_STREAM_WRITE_CHUNK: usize = 60 * 1024;
const CONTROL_STREAM_QUEUE_CAPACITY: usize = 1024;
const CONTROL_STREAM_OPEN_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) type ControlDataSenderFuture = Pin<Box<dyn Future<Output = P2pResult<()>> + Send>>;

pub(crate) trait ControlDataSender: Send + Sync + 'static {
    fn send(&self, payload: Vec<u8>) -> ControlDataSenderFuture;
}

pub(crate) type ControlDataSenderRef = Arc<dyn ControlDataSender>;

impl ControlDataSender for mpsc::Sender<Vec<u8>> {
    fn send(&self, payload: Vec<u8>) -> ControlDataSenderFuture {
        let sender = self.clone();
        Box::pin(async move {
            sender.send(payload).await.map_err(|_| {
                p2p_err!(
                    P2pErrorCode::Interrupted,
                    "control data sender channel closed"
                )
            })
        })
    }
}

#[derive(Clone, Debug, RawEncode, RawDecode)]
enum ControlStreamFrame {
    Open {
        stream_id: u32,
        purpose: TunnelPurpose,
    },
    OpenResp {
        stream_id: u32,
        result: TunnelCommandResult,
    },
    Data {
        stream_id: u32,
        bytes: Vec<u8>,
    },
    Fin {
        stream_id: u32,
    },
    Reset {
        stream_id: u32,
        reason: TunnelCommandResult,
    },
    Window {
        stream_id: u32,
        credit: u32,
    },
}

enum InboundItem {
    Data(Vec<u8>),
    Fin,
    Reset(P2pErrorCode, String),
}

struct PendingOpen {
    purpose: TunnelPurpose,
    inbound_tx: mpsc::Sender<InboundItem>,
    waiter: oneshot::Sender<P2pResult<()>>,
}

struct PendingOpenCleanup {
    inner: Arc<ControlStreamRuntimeInner>,
    stream_id: u32,
}

impl Drop for PendingOpenCleanup {
    fn drop(&mut self) {
        self.inner
            .pending_opens
            .lock()
            .unwrap()
            .remove(&self.stream_id);
    }
}

struct ControlStreamRuntimeInner {
    sender: ControlDataSenderRef,
    streams: Mutex<HashMap<u32, mpsc::Sender<InboundItem>>>,
    pending_opens: Mutex<HashMap<u32, PendingOpen>>,
    listener: RwLock<Option<(ListenVPortsRef, IncomingControlStreamCallback)>>,
    next_stream_id: AtomicU32,
    closed: AtomicBool,
    close_reason: Mutex<Option<(P2pErrorCode, String)>>,
}

#[derive(Clone)]
pub(crate) struct ControlStreamRuntime {
    inner: Arc<ControlStreamRuntimeInner>,
}

pub(crate) struct ControlStreamRead {
    rx: mpsc::Receiver<InboundItem>,
    pending: Vec<u8>,
    closed: Option<P2pError>,
}

pub(crate) struct ControlStreamWrite {
    stream_id: u32,
    sender: ControlDataSenderRef,
    pending_write: Option<ControlDataSenderFuture>,
    pending_write_len: usize,
    pending_shutdown: Option<ControlDataSenderFuture>,
    closed: bool,
}

impl ControlStreamRuntime {
    pub(crate) fn new(is_initiator: bool, sender: ControlDataSenderRef) -> Self {
        let first = if is_initiator { 1 } else { 2 };
        let inner = Arc::new(ControlStreamRuntimeInner {
            sender,
            streams: Mutex::new(HashMap::new()),
            pending_opens: Mutex::new(HashMap::new()),
            listener: RwLock::new(None),
            next_stream_id: AtomicU32::new(first),
            closed: AtomicBool::new(false),
            close_reason: Mutex::new(None),
        });
        Self { inner }
    }

    pub(crate) async fn listen(
        &self,
        purposes: ListenVPortsRef,
        callback: IncomingControlStreamCallback,
    ) -> P2pResult<()> {
        self.check_open()?;
        *self.inner.listener.write().unwrap() = Some((purposes, callback));
        Ok(())
    }

    pub(crate) async fn open(
        &self,
        purpose: TunnelPurpose,
    ) -> P2pResult<(TunnelStreamRead, TunnelStreamWrite)> {
        self.check_open()?;
        let stream_id = self.alloc_stream_id();
        let (tx, rx) = mpsc::channel(CONTROL_STREAM_QUEUE_CAPACITY);
        let (waiter_tx, waiter_rx) = oneshot::channel();
        self.inner.pending_opens.lock().unwrap().insert(
            stream_id,
            PendingOpen {
                purpose: purpose.clone(),
                inbound_tx: tx,
                waiter: waiter_tx,
            },
        );
        let _cleanup = PendingOpenCleanup {
            inner: self.inner.clone(),
            stream_id,
        };
        self.send_frame(ControlStreamFrame::Open { stream_id, purpose })
            .await?;
        match runtime::timeout(CONTROL_STREAM_OPEN_TIMEOUT, waiter_rx).await {
            Ok(Ok(Ok(()))) => {
                let read: TunnelStreamRead = Box::pin(ControlStreamRead::new(rx));
                let write: TunnelStreamWrite = Box::pin(ControlStreamWrite::new(
                    stream_id,
                    self.inner.sender.clone(),
                ));
                Ok((read, write))
            }
            Ok(Ok(Err(err))) => Err(err),
            Ok(Err(_)) => Err(p2p_err!(
                P2pErrorCode::Interrupted,
                "control stream open canceled"
            )),
            Err(_) => Err(p2p_err!(
                P2pErrorCode::Timeout,
                "control stream open timeout"
            )),
        }
    }

    pub(crate) async fn on_data(&self, payload: Vec<u8>) -> P2pResult<()> {
        if payload.len() > MAX_CONTROL_DATA_FRAME_SIZE {
            let err = p2p_err!(
                P2pErrorCode::InvalidData,
                "control data payload exceeds 64KiB"
            );
            self.close_all(p2p_err!(
                P2pErrorCode::InvalidData,
                "control data payload exceeds 64KiB"
            ));
            return Err(err);
        }
        self.check_open()?;
        let frame = ControlStreamFrame::clone_from_slice(payload.as_slice())
            .map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
        self.handle_frame(frame).await
    }

    pub(crate) fn close_all(&self, reason: P2pError) {
        if self.inner.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        let code = reason.code();
        let msg = reason.msg().to_owned();
        *self.inner.close_reason.lock().unwrap() = Some((code, msg.clone()));
        for (_, tx) in self.inner.streams.lock().unwrap().drain() {
            let _ = tx.try_send(InboundItem::Reset(code, msg.clone()));
        }
        for (_, pending) in self.inner.pending_opens.lock().unwrap().drain() {
            let _ = pending.waiter.send(Err(P2pError::new(code, msg.clone())));
            let _ = pending
                .inbound_tx
                .try_send(InboundItem::Reset(code, msg.clone()));
        }
        *self.inner.listener.write().unwrap() = None;
    }

    async fn handle_frame(&self, frame: ControlStreamFrame) -> P2pResult<()> {
        match frame {
            ControlStreamFrame::Open { stream_id, purpose } => {
                self.handle_open(stream_id, purpose).await
            }
            ControlStreamFrame::OpenResp { stream_id, result } => {
                self.handle_open_resp(stream_id, result)
            }
            ControlStreamFrame::Data { stream_id, bytes } => self.deliver(stream_id, bytes),
            ControlStreamFrame::Fin { stream_id } => self.finish_stream(stream_id),
            ControlStreamFrame::Reset { stream_id, reason } => {
                self.reset_stream(stream_id, reason.into_p2p_error("control stream reset"))
            }
            ControlStreamFrame::Window { .. } => Ok(()),
        }
    }

    async fn handle_open(&self, stream_id: u32, purpose: TunnelPurpose) -> P2pResult<()> {
        let listener = self.inner.listener.read().unwrap().clone();
        let Some((purposes, callback)) = listener else {
            self.send_frame(ControlStreamFrame::OpenResp {
                stream_id,
                result: TunnelCommandResult::ListenerClosed,
            })
            .await?;
            return Ok(());
        };
        if !purposes.is_listen(&purpose) {
            self.send_frame(ControlStreamFrame::OpenResp {
                stream_id,
                result: TunnelCommandResult::PortNotListen,
            })
            .await?;
            return Ok(());
        }
        let (tx, rx) = mpsc::channel(CONTROL_STREAM_QUEUE_CAPACITY);
        self.inner.streams.lock().unwrap().insert(stream_id, tx);
        self.send_frame(ControlStreamFrame::OpenResp {
            stream_id,
            result: TunnelCommandResult::Success,
        })
        .await?;
        let read: TunnelStreamRead = Box::pin(ControlStreamRead::new(rx));
        let write: TunnelStreamWrite = Box::pin(ControlStreamWrite::new(
            stream_id,
            self.inner.sender.clone(),
        ));
        callback(Ok((purpose, read, write))).await;
        Ok(())
    }

    fn handle_open_resp(&self, stream_id: u32, result: TunnelCommandResult) -> P2pResult<()> {
        let pending = self.inner.pending_opens.lock().unwrap().remove(&stream_id);
        let Some(pending) = pending else {
            return Ok(());
        };
        if result == TunnelCommandResult::Success {
            if pending.waiter.send(Ok(())).is_ok() {
                self.inner
                    .streams
                    .lock()
                    .unwrap()
                    .insert(stream_id, pending.inbound_tx);
            }
        } else {
            let _ = pending.waiter.send(Err(result.into_p2p_error(format!(
                "control stream open rejected purpose {}",
                pending.purpose
            ))));
        }
        Ok(())
    }

    fn deliver(&self, stream_id: u32, bytes: Vec<u8>) -> P2pResult<()> {
        let tx = self.inner.streams.lock().unwrap().get(&stream_id).cloned();
        let Some(tx) = tx else {
            return Ok(());
        };
        tx.try_send(InboundItem::Data(bytes)).map_err(|_| {
            p2p_err!(
                P2pErrorCode::OutOfLimit,
                "control stream inbound queue full"
            )
        })
    }

    fn finish_stream(&self, stream_id: u32) -> P2pResult<()> {
        if let Some(tx) = self.inner.streams.lock().unwrap().remove(&stream_id) {
            let _ = tx.try_send(InboundItem::Fin);
        }
        Ok(())
    }

    fn reset_stream(&self, stream_id: u32, reason: P2pError) -> P2pResult<()> {
        if let Some(tx) = self.inner.streams.lock().unwrap().remove(&stream_id) {
            let _ = tx.try_send(InboundItem::Reset(reason.code(), reason.msg().to_owned()));
        }
        Ok(())
    }

    async fn send_frame(&self, frame: ControlStreamFrame) -> P2pResult<()> {
        let payload = frame
            .to_vec()
            .map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
        if payload.len() > MAX_CONTROL_DATA_FRAME_SIZE {
            return Err(p2p_err!(
                P2pErrorCode::OutOfLimit,
                "control stream frame exceeds 64KiB"
            ));
        }
        if let Err(err) = self.inner.sender.send(payload).await {
            log::warn!(
                "control stream data sender failed code={:?} msg={}",
                err.code(),
                err.msg()
            );
            self.close_all(P2pError::new(err.code(), err.msg().to_owned()));
            return Err(err);
        }
        Ok(())
    }

    fn check_open(&self) -> P2pResult<()> {
        if self.inner.closed.load(Ordering::SeqCst) {
            let reason = self.inner.close_reason.lock().unwrap();
            return Err(reason
                .as_ref()
                .map(|(code, msg)| P2pError::new(*code, msg.clone()))
                .unwrap_or_else(|| {
                    p2p_err!(P2pErrorCode::Interrupted, "control stream runtime closed")
                }));
        }
        Ok(())
    }

    fn alloc_stream_id(&self) -> u32 {
        self.inner.next_stream_id.fetch_add(2, Ordering::SeqCst)
    }
}

impl ControlStreamRead {
    fn new(rx: mpsc::Receiver<InboundItem>) -> Self {
        Self {
            rx,
            pending: Vec::new(),
            closed: None,
        }
    }
}

impl tokio::io::AsyncRead for ControlStreamRead {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if let Some(err) = self.closed.take() {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::Interrupted,
                err.to_string(),
            )));
        }
        if !self.pending.is_empty() {
            let len = self.pending.len().min(buf.remaining());
            buf.put_slice(&self.pending[..len]);
            self.pending.drain(..len);
            return Poll::Ready(Ok(()));
        }
        match Pin::new(&mut self.rx).poll_recv(cx) {
            Poll::Ready(Some(InboundItem::Data(bytes))) => {
                let len = bytes.len().min(buf.remaining());
                buf.put_slice(&bytes[..len]);
                if len < bytes.len() {
                    self.pending.extend_from_slice(&bytes[len..]);
                }
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Some(InboundItem::Fin)) | Poll::Ready(None) => Poll::Ready(Ok(())),
            Poll::Ready(Some(InboundItem::Reset(_code, msg))) => {
                Poll::Ready(Err(io::Error::new(io::ErrorKind::Interrupted, msg)))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl ControlStreamWrite {
    fn new(stream_id: u32, sender: ControlDataSenderRef) -> Self {
        Self {
            stream_id,
            sender,
            pending_write: None,
            pending_write_len: 0,
            pending_shutdown: None,
            closed: false,
        }
    }

    fn encode_frame(&self, frame: ControlStreamFrame) -> io::Result<Vec<u8>> {
        let payload = frame
            .to_vec()
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err.to_string()))?;
        if payload.len() > MAX_CONTROL_DATA_FRAME_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "control stream frame exceeds 64KiB",
            ));
        }
        Ok(payload)
    }

    fn poll_pending_write(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        let Some(send) = self.pending_write.as_mut() else {
            return Poll::Ready(Ok(0));
        };
        match send.as_mut().poll(cx) {
            Poll::Ready(Ok(())) => {
                self.pending_write = None;
                let len = self.pending_write_len;
                self.pending_write_len = 0;
                Poll::Ready(Ok(len))
            }
            Poll::Ready(Err(err)) => {
                self.pending_write = None;
                self.pending_write_len = 0;
                self.closed = true;
                Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    err.to_string(),
                )))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl tokio::io::AsyncWrite for ControlStreamWrite {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        src: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        if this.closed {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "control stream closed",
            )));
        }
        if this.pending_write.is_some() {
            return this.poll_pending_write(cx);
        }
        if src.is_empty() {
            return Poll::Ready(Ok(0));
        }
        let len = src.len().min(CONTROL_STREAM_WRITE_CHUNK);
        let payload = this.encode_frame(ControlStreamFrame::Data {
            stream_id: this.stream_id,
            bytes: src[..len].to_vec(),
        })?;
        this.pending_write_len = len;
        this.pending_write = Some(this.sender.send(payload));
        this.poll_pending_write(cx)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.pending_write.is_some() {
            match self.poll_pending_write(cx)? {
                Poll::Ready(_) => {}
                Poll::Pending => return Poll::Pending,
            }
        }
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if !self.closed {
            if self.pending_shutdown.is_none() {
                let payload = self.encode_frame(ControlStreamFrame::Fin {
                    stream_id: self.stream_id,
                })?;
                self.pending_shutdown = Some(self.sender.send(payload));
            }
            if let Some(send) = self.pending_shutdown.as_mut() {
                match send.as_mut().poll(cx) {
                    Poll::Ready(Ok(())) => {
                        self.pending_shutdown = None;
                        self.closed = true;
                    }
                    Poll::Ready(Err(err)) => {
                        self.pending_shutdown = None;
                        self.closed = true;
                        return Poll::Ready(Err(io::Error::new(
                            io::ErrorKind::BrokenPipe,
                            err.to_string(),
                        )));
                    }
                    Poll::Pending => return Poll::Pending,
                }
            }
        }
        Poll::Ready(Ok(()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::networks::allow_all_listen_vports;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    struct NoopSender;

    impl ControlDataSender for NoopSender {
        fn send(&self, _payload: Vec<u8>) -> ControlDataSenderFuture {
            Box::pin(async { Ok(()) })
        }
    }

    struct CaptureSender {
        tx: mpsc::Sender<usize>,
    }

    impl ControlDataSender for CaptureSender {
        fn send(&self, payload: Vec<u8>) -> ControlDataSenderFuture {
            let tx = self.tx.clone();
            Box::pin(async move {
                assert!(payload.len() <= MAX_CONTROL_DATA_FRAME_SIZE);
                tx.send(payload.len())
                    .await
                    .map_err(|_| p2p_err!(P2pErrorCode::Interrupted, "capture sender closed"))
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
        let (capture_tx, mut capture_rx) = mpsc::channel::<usize>(1);
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
        let (capture_tx, mut capture_rx) = mpsc::channel::<usize>(4);
        let runtime = ControlStreamRuntime::new(true, Arc::new(CaptureSender { tx: capture_tx }));
        let mut write = ControlStreamWrite::new(1, runtime.inner.sender.clone());
        let written = write
            .write(&vec![1u8; MAX_CONTROL_DATA_FRAME_SIZE * 2])
            .await
            .unwrap();
        assert_eq!(written, CONTROL_STREAM_WRITE_CHUNK);
        assert!(capture_rx.recv().await.unwrap() <= MAX_CONTROL_DATA_FRAME_SIZE);
    }
}
