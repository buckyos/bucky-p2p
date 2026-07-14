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
const CONTROL_STREAM_QUEUE_TERMINAL_RESERVE: usize = 1;
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
    local_stream_id_parity: u32,
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
    pending_offset: usize,
}

pub(crate) struct ControlStreamWrite {
    stream_id: u32,
    inner: Arc<ControlStreamRuntimeInner>,
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
            local_stream_id_parity: first & 1,
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
        let (tx, rx) = Self::inbound_channel();
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
                    self.inner.clone(),
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
            ControlStreamFrame::Data { stream_id, bytes } => {
                self.deliver(stream_id, bytes).await
            }
            ControlStreamFrame::Fin { stream_id } => self.finish_stream(stream_id),
            ControlStreamFrame::Reset { stream_id, reason } => {
                self.reset_stream(stream_id, reason.into_p2p_error("control stream reset"))
            }
            ControlStreamFrame::Window { .. } => Ok(()),
        }
    }

    async fn handle_open(&self, stream_id: u32, purpose: TunnelPurpose) -> P2pResult<()> {
        if !self.is_peer_stream_id(stream_id) {
            self.send_frame(ControlStreamFrame::OpenResp {
                stream_id,
                result: TunnelCommandResult::InvalidParam,
            })
            .await?;
            return Ok(());
        }
        if self
            .inner
            .pending_opens
            .lock()
            .unwrap()
            .contains_key(&stream_id)
            || self.inner.streams.lock().unwrap().contains_key(&stream_id)
        {
            self.send_frame(ControlStreamFrame::OpenResp {
                stream_id,
                result: TunnelCommandResult::ConflictLost,
            })
            .await?;
            return Ok(());
        }
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
        let (tx, rx) = Self::inbound_channel();
        let inserted = {
            let mut streams = self.inner.streams.lock().unwrap();
            if streams.contains_key(&stream_id) {
                false
            } else {
                streams.insert(stream_id, tx);
                true
            }
        };
        if !inserted {
            self.send_frame(ControlStreamFrame::OpenResp {
                stream_id,
                result: TunnelCommandResult::ConflictLost,
            })
            .await?;
            return Ok(());
        }
        self.send_frame(ControlStreamFrame::OpenResp {
            stream_id,
            result: TunnelCommandResult::Success,
        })
        .await?;
        let read: TunnelStreamRead = Box::pin(ControlStreamRead::new(rx));
        let write: TunnelStreamWrite = Box::pin(ControlStreamWrite::new(
            stream_id,
            self.inner.clone(),
        ));
        runtime::task::spawn(async move {
            callback(Ok((purpose, read, write))).await;
        });
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

    async fn deliver(&self, stream_id: u32, bytes: Vec<u8>) -> P2pResult<()> {
        enum DeliveryFailure {
            Overflow,
            ReaderClosed,
        }

        let failure = {
            let mut streams = self.inner.streams.lock().unwrap();
            let Some(tx) = streams.get(&stream_id).cloned() else {
                return Ok(());
            };
            if tx.capacity() <= CONTROL_STREAM_QUEUE_TERMINAL_RESERVE {
                streams.remove(&stream_id);
                let _ = tx.try_send(InboundItem::Reset(
                    P2pErrorCode::OutOfLimit,
                    "control stream inbound queue full".to_owned(),
                ));
                Some(DeliveryFailure::Overflow)
            } else {
                match tx.try_send(InboundItem::Data(bytes)) {
                    Ok(()) => None,
                    Err(mpsc::error::TrySendError::Full(_)) => {
                        streams.remove(&stream_id);
                        let _ = tx.try_send(InboundItem::Reset(
                            P2pErrorCode::OutOfLimit,
                            "control stream inbound queue full".to_owned(),
                        ));
                        Some(DeliveryFailure::Overflow)
                    }
                    Err(mpsc::error::TrySendError::Closed(_)) => {
                        streams.remove(&stream_id);
                        Some(DeliveryFailure::ReaderClosed)
                    }
                }
            }
        };
        let Some(failure) = failure else {
            return Ok(());
        };
        let reason = match failure {
            DeliveryFailure::Overflow => TunnelCommandResult::AcceptQueueFull,
            DeliveryFailure::ReaderClosed => TunnelCommandResult::Interrupted,
        };
        self.send_frame(ControlStreamFrame::Reset { stream_id, reason })
            .await
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
        loop {
            let stream_id = self.inner.next_stream_id.fetch_add(2, Ordering::SeqCst);
            if stream_id == 0 {
                continue;
            }
            if !self
                .inner
                .pending_opens
                .lock()
                .unwrap()
                .contains_key(&stream_id)
                && !self.inner.streams.lock().unwrap().contains_key(&stream_id)
            {
                return stream_id;
            }
        }
    }

    fn is_peer_stream_id(&self, stream_id: u32) -> bool {
        stream_id != 0 && stream_id & 1 != self.inner.local_stream_id_parity
    }

    fn inbound_channel() -> (mpsc::Sender<InboundItem>, mpsc::Receiver<InboundItem>) {
        mpsc::channel(CONTROL_STREAM_QUEUE_CAPACITY + CONTROL_STREAM_QUEUE_TERMINAL_RESERVE)
    }
}

impl ControlStreamRead {
    fn new(rx: mpsc::Receiver<InboundItem>) -> Self {
        Self {
            rx,
            pending: Vec::new(),
            pending_offset: 0,
        }
    }
}

impl tokio::io::AsyncRead for ControlStreamRead {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.pending_offset < self.pending.len() {
            let start = self.pending_offset;
            let len = (self.pending.len() - start).min(buf.remaining());
            buf.put_slice(&self.pending[start..start + len]);
            self.pending_offset += len;
            if self.pending_offset == self.pending.len() {
                self.pending.clear();
                self.pending_offset = 0;
            }
            return Poll::Ready(Ok(()));
        }
        match Pin::new(&mut self.rx).poll_recv(cx) {
            Poll::Ready(Some(InboundItem::Data(bytes))) => {
                let len = bytes.len().min(buf.remaining());
                buf.put_slice(&bytes[..len]);
                if len < bytes.len() {
                    self.pending = bytes;
                    self.pending_offset = len;
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
    fn new(stream_id: u32, inner: Arc<ControlStreamRuntimeInner>) -> Self {
        Self {
            stream_id,
            inner,
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
        if self.inner.closed.load(Ordering::SeqCst) {
            self.pending_write = None;
            self.pending_write_len = 0;
            self.closed = true;
            return Poll::Ready(Err(self.closed_error()));
        }
        let Some(send) = self.pending_write.as_mut() else {
            return Poll::Ready(Ok(0));
        };
        match send.as_mut().poll(cx) {
            Poll::Ready(Ok(())) => {
                self.pending_write = None;
                let len = self.pending_write_len;
                self.pending_write_len = 0;
                if self.inner.closed.load(Ordering::SeqCst) {
                    self.closed = true;
                    Poll::Ready(Err(self.closed_error()))
                } else {
                    Poll::Ready(Ok(len))
                }
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

    fn closed_error(&self) -> io::Error {
        let message = self
            .inner
            .close_reason
            .lock()
            .unwrap()
            .as_ref()
            .map(|(_, msg)| msg.clone())
            .unwrap_or_else(|| "control stream closed".to_owned());
        io::Error::new(io::ErrorKind::BrokenPipe, message)
    }
}

impl tokio::io::AsyncWrite for ControlStreamWrite {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        src: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        if this.closed || this.inner.closed.load(Ordering::SeqCst) {
            this.closed = true;
            return Poll::Ready(Err(this.closed_error()));
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
        this.pending_write = Some(this.inner.sender.send(payload));
        this.poll_pending_write(cx)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.pending_write.is_some() {
            match self.poll_pending_write(cx)? {
                Poll::Ready(_) => {}
                Poll::Pending => return Poll::Pending,
            }
        }
        if self.inner.closed.load(Ordering::SeqCst) {
            self.closed = true;
            return Poll::Ready(Err(self.closed_error()));
        }
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.pending_write.is_some() {
            match self.poll_pending_write(cx)? {
                Poll::Ready(_) => {}
                Poll::Pending => return Poll::Pending,
            }
        }
        if self.inner.closed.load(Ordering::SeqCst) {
            self.pending_shutdown = None;
            self.closed = true;
            return Poll::Ready(Err(self.closed_error()));
        }
        if !self.closed {
            if self.pending_shutdown.is_none() {
                let payload = self.encode_frame(ControlStreamFrame::Fin {
                    stream_id: self.stream_id,
                })?;
                self.pending_shutdown = Some(self.inner.sender.send(payload));
            }
            if let Some(send) = self.pending_shutdown.as_mut() {
                match send.as_mut().poll(cx) {
                    Poll::Ready(Ok(())) => {
                        self.pending_shutdown = None;
                        if self.inner.closed.load(Ordering::SeqCst) {
                            self.closed = true;
                            return Poll::Ready(Err(self.closed_error()));
                        }
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
mod tests;
