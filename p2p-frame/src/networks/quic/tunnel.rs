use crate::endpoint::{Endpoint, Protocol};
use crate::error::{P2pError, P2pErrorCode, P2pResult, into_p2p_err, p2p_err};
use crate::executor::Executor;
use crate::networks::{
    ListenVPortsRef, Tunnel, TunnelCommand, TunnelCommandBody, TunnelCommandResult,
    TunnelDatagramRead, TunnelDatagramWrite, TunnelForm, TunnelPurpose, TunnelState,
    TunnelStreamRead, TunnelStreamWrite, read_tunnel_command_body, read_tunnel_command_header,
    write_tunnel_command,
};
use crate::p2p_identity::P2pId;
use crate::runtime;
use crate::types::{Timestamp, TunnelCandidateId, TunnelId};
use bucky_raw_codec::{RawDecode, RawEncode};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock, Weak};
use std::time::{Duration, Instant};
use tokio::sync::{Mutex as AsyncMutex, Notify, mpsc, oneshot};

const COMMAND_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const COMMAND_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
const COMMAND_HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(15);

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, RawEncode, RawDecode)]
enum TunnelChannelKind {
    Stream = 1,
    Datagram = 2,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, RawEncode, RawDecode)]
enum QuicTunnelCommandId {
    Hello = 1,
    HelloResp = 2,
    Ping = 3,
    Pong = 4,
    OpenChannelReq = 5,
    OpenChannelResp = 6,
}

impl QuicTunnelCommandId {
    fn from_u8(value: u8) -> Option<Self> {
        match value {
            x if x == Self::Hello as u8 => Some(Self::Hello),
            x if x == Self::HelloResp as u8 => Some(Self::HelloResp),
            x if x == Self::Ping as u8 => Some(Self::Ping),
            x if x == Self::Pong as u8 => Some(Self::Pong),
            x if x == Self::OpenChannelReq as u8 => Some(Self::OpenChannelReq),
            x if x == Self::OpenChannelResp as u8 => Some(Self::OpenChannelResp),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, RawEncode, RawDecode)]
struct TunnelHello {
    tunnel_id: TunnelId,
    candidate_id: TunnelCandidateId,
    is_reverse: bool,
}

impl TunnelCommandBody for TunnelHello {
    const COMMAND_ID: u8 = QuicTunnelCommandId::Hello as u8;
}

#[derive(Clone, Debug, RawEncode, RawDecode)]
struct TunnelHelloResp {
    tunnel_id: TunnelId,
    candidate_id: TunnelCandidateId,
}

impl TunnelCommandBody for TunnelHelloResp {
    const COMMAND_ID: u8 = QuicTunnelCommandId::HelloResp as u8;
}

#[derive(Clone, Debug, RawEncode, RawDecode)]
struct TunnelPing {
    seq: u64,
    send_time: Timestamp,
}

impl TunnelCommandBody for TunnelPing {
    const COMMAND_ID: u8 = QuicTunnelCommandId::Ping as u8;
}

#[derive(Clone, Debug, RawEncode, RawDecode)]
struct TunnelPong {
    seq: u64,
    send_time: Timestamp,
}

impl TunnelCommandBody for TunnelPong {
    const COMMAND_ID: u8 = QuicTunnelCommandId::Pong as u8;
}

#[derive(Clone, Debug, RawEncode, RawDecode)]
struct TunnelOpenChannelReq {
    request_id: u64,
    kind: TunnelChannelKind,
    purpose: TunnelPurpose,
}

impl TunnelCommandBody for TunnelOpenChannelReq {
    const COMMAND_ID: u8 = QuicTunnelCommandId::OpenChannelReq as u8;
}

#[derive(Clone, Debug, RawEncode, RawDecode)]
struct TunnelOpenChannelResp {
    request_id: u64,
    result: TunnelCommandResult,
}

impl TunnelCommandBody for TunnelOpenChannelResp {
    const COMMAND_ID: u8 = QuicTunnelCommandId::OpenChannelResp as u8;
}

struct QuicAcceptedStream {
    purpose: TunnelPurpose,
    read: quinn::RecvStream,
    write: quinn::SendStream,
}

struct QuicAcceptedDatagram {
    purpose: TunnelPurpose,
    read: quinn::RecvStream,
}

struct QuicTunnelState {
    last_cmd_recv_at: Instant,
    last_pong_at: Instant,
    pending_open_requests: HashMap<u64, oneshot::Sender<P2pResult<()>>>,
}

pub(crate) struct QuicTunnel {
    socket: quinn::Connection,
    tunnel_id: TunnelId,
    candidate_id: TunnelCandidateId,
    form: TunnelForm,
    is_reverse: bool,
    local_id: P2pId,
    remote_id: P2pId,
    local_ep: Endpoint,
    remote_ep: Endpoint,
    command_write: AsyncMutex<quinn::SendStream>,
    state: Mutex<QuicTunnelState>,
    self_weak: Weak<QuicTunnel>,
    closed: AtomicBool,
    close_notify: Notify,
    next_request_id: AtomicU64,
    next_ping_seq: AtomicU64,
    bi_accept_started: AtomicBool,
    uni_accept_started: AtomicBool,
    stream_vports: RwLock<Option<ListenVPortsRef>>,
    datagram_vports: RwLock<Option<ListenVPortsRef>>,
    stream_rx: AsyncMutex<mpsc::UnboundedReceiver<QuicAcceptedStream>>,
    stream_tx: mpsc::UnboundedSender<QuicAcceptedStream>,
    datagram_rx: AsyncMutex<mpsc::UnboundedReceiver<QuicAcceptedDatagram>>,
    datagram_tx: mpsc::UnboundedSender<QuicAcceptedDatagram>,
}

impl QuicTunnel {
    fn kind_name(kind: TunnelChannelKind) -> &'static str {
        match kind {
            TunnelChannelKind::Stream => "stream",
            TunnelChannelKind::Datagram => "datagram",
        }
    }

    fn log_ctx(&self) -> String {
        format!(
            "tunnel_id={} candidate_id={:?} form={:?} reverse={} local_id={} local_ep={} remote_id={} remote_ep={}",
            self.tunnel_id,
            self.candidate_id,
            self.form,
            self.is_reverse,
            self.local_id,
            self.local_ep,
            self.remote_id,
            self.remote_ep
        )
    }

    pub(crate) async fn connect(
        socket: quinn::Connection,
        tunnel_id: TunnelId,
        candidate_id: TunnelCandidateId,
        is_reverse: bool,
        local_id: P2pId,
        remote_id: P2pId,
        local_ep: Endpoint,
        remote_ep: Endpoint,
    ) -> P2pResult<Arc<Self>> {
        log::debug!(
            "quic tunnel connect start tunnel_id={} candidate_id={:?} reverse={} local_id={} local_ep={} remote_id={} remote_ep={}",
            tunnel_id,
            candidate_id,
            is_reverse,
            local_id,
            local_ep,
            remote_id,
            remote_ep
        );
        let (command_read, mut command_write) =
            Self::init_command_stream(&socket, remote_ep, true).await?;
        Self::write_command(
            &mut command_write,
            TunnelHello {
                tunnel_id,
                candidate_id,
                is_reverse,
            },
        )
        .await?;
        let mut command_read = command_read;
        let hello_resp = Self::read_command::<_, TunnelHelloResp>(
            &mut command_read,
            QuicTunnelCommandId::HelloResp,
            "quic hello resp",
        )
        .await?;
        if hello_resp.tunnel_id != tunnel_id || hello_resp.candidate_id != candidate_id {
            log::warn!(
                "quic tunnel connect hello resp mismatch tunnel_id={} candidate_id={:?} resp_tunnel_id={} resp_candidate_id={:?} local_id={} local_ep={} remote_id={} remote_ep={}",
                tunnel_id,
                candidate_id,
                hello_resp.tunnel_id,
                hello_resp.candidate_id,
                local_id,
                local_ep,
                remote_id,
                remote_ep
            );
            return Err(p2p_err!(
                P2pErrorCode::InvalidData,
                "quic hello resp tunnel key mismatch"
            ));
        }
        let tunnel = Self::new_inner(
            socket,
            tunnel_id,
            candidate_id,
            TunnelForm::Active,
            is_reverse,
            local_id,
            remote_id,
            local_ep,
            remote_ep,
            command_read,
            command_write,
        );
        log::debug!("quic tunnel connect established {}", tunnel.log_ctx());
        Ok(tunnel)
    }

    pub(crate) async fn accept(
        socket: quinn::Connection,
        local_id: P2pId,
        remote_id: P2pId,
        local_ep: Endpoint,
        remote_ep: Endpoint,
    ) -> P2pResult<Arc<Self>> {
        log::debug!(
            "quic tunnel accept start local_id={} local_ep={} remote_id={} remote_ep={}",
            local_id,
            local_ep,
            remote_id,
            remote_ep
        );
        let (command_read, mut command_write) =
            Self::init_command_stream(&socket, remote_ep, false).await?;
        let mut command_read = command_read;
        let hello = Self::read_command::<_, TunnelHello>(
            &mut command_read,
            QuicTunnelCommandId::Hello,
            "quic hello",
        )
        .await?;
        Self::write_command(
            &mut command_write,
            TunnelHelloResp {
                tunnel_id: hello.tunnel_id,
                candidate_id: hello.candidate_id,
            },
        )
        .await?;
        let tunnel = Self::new_inner(
            socket,
            hello.tunnel_id,
            hello.candidate_id,
            TunnelForm::Passive,
            hello.is_reverse,
            local_id,
            remote_id,
            local_ep,
            remote_ep,
            command_read,
            command_write,
        );
        log::debug!("quic tunnel accept established {}", tunnel.log_ctx());
        Ok(tunnel)
    }

    fn new_inner(
        socket: quinn::Connection,
        tunnel_id: TunnelId,
        candidate_id: TunnelCandidateId,
        form: TunnelForm,
        is_reverse: bool,
        local_id: P2pId,
        remote_id: P2pId,
        local_ep: Endpoint,
        remote_ep: Endpoint,
        command_read: quinn::RecvStream,
        command_write: quinn::SendStream,
    ) -> Arc<Self> {
        let (stream_tx, stream_rx) = mpsc::unbounded_channel();
        let (datagram_tx, datagram_rx) = mpsc::unbounded_channel();
        let tunnel = Arc::new_cyclic(|weak| Self {
            socket,
            tunnel_id,
            candidate_id,
            form,
            is_reverse,
            local_id,
            remote_id,
            local_ep,
            remote_ep,
            command_write: AsyncMutex::new(command_write),
            state: Mutex::new(QuicTunnelState {
                last_cmd_recv_at: Instant::now(),
                last_pong_at: Instant::now(),
                pending_open_requests: HashMap::new(),
            }),
            self_weak: weak.clone(),
            closed: AtomicBool::new(false),
            close_notify: Notify::new(),
            next_request_id: AtomicU64::new(1),
            next_ping_seq: AtomicU64::new(1),
            bi_accept_started: AtomicBool::new(false),
            uni_accept_started: AtomicBool::new(false),
            stream_vports: RwLock::new(None),
            datagram_vports: RwLock::new(None),
            stream_rx: AsyncMutex::new(stream_rx),
            stream_tx,
            datagram_rx: AsyncMutex::new(datagram_rx),
            datagram_tx,
        });
        Self::start_loops(tunnel.clone(), command_read);
        tunnel
    }

    async fn init_command_stream(
        socket: &quinn::Connection,
        remote_ep: Endpoint,
        active: bool,
    ) -> P2pResult<(quinn::RecvStream, quinn::SendStream)> {
        if active {
            log::debug!("quic tunnel open command stream remote_ep={}", remote_ep);
            let (send, recv) = socket.open_bi().await.map_err(into_p2p_err!(
                P2pErrorCode::ConnectFailed,
                "quic open command stream to {} failed",
                remote_ep
            ))?;
            Ok((recv, send))
        } else {
            log::debug!("quic tunnel accept command stream remote_ep={}", remote_ep);
            let (send, recv) = socket.accept_bi().await.map_err(into_p2p_err!(
                P2pErrorCode::IoError,
                "quic accept command stream from {} failed",
                remote_ep
            ))?;
            Ok((recv, send))
        }
    }

    fn start_loops(this: Arc<Self>, command_read: quinn::RecvStream) {
        let recv_tunnel = this.clone();
        Executor::spawn_ok(async move {
            recv_tunnel.command_recv_loop(command_read).await;
        });

        let heartbeat_tunnel = this.clone();
        Executor::spawn_ok(async move {
            heartbeat_tunnel.heartbeat_loop().await;
        });
    }

    fn start_bi_accept_loop_if_needed(&self) {
        if self
            .bi_accept_started
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }

        let Some(this) = self.self_weak.upgrade() else {
            self.bi_accept_started.store(false, Ordering::SeqCst);
            return;
        };
        Executor::spawn_ok(async move {
            this.bi_accept_loop().await;
        });
    }

    fn start_uni_accept_loop_if_needed(&self) {
        if self
            .uni_accept_started
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }

        let Some(this) = self.self_weak.upgrade() else {
            self.uni_accept_started.store(false, Ordering::SeqCst);
            return;
        };
        Executor::spawn_ok(async move {
            this.uni_accept_loop().await;
        });
    }

    fn close_with_error(&self, code: P2pErrorCode, message: String) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        log::warn!(
            "quic tunnel closing {} code={:?} message={}",
            self.log_ctx(),
            code,
            message
        );
        self.socket.close(0_u32.into(), message.as_bytes());
        let pending = {
            let mut state = self.state.lock().unwrap();
            state.pending_open_requests.drain().collect::<Vec<_>>()
        };
        for (_, tx) in pending {
            let _ = tx.send(Err(P2pError::new(code, message.clone())));
        }
        self.close_notify.notify_waiters();
    }

    async fn send_command<T>(&self, body: T) -> P2pResult<()>
    where
        T: TunnelCommandBody,
    {
        if self.is_closed() {
            return Err(p2p_err!(P2pErrorCode::Interrupted, "quic tunnel closed"));
        }
        let cmd = TunnelCommand::<T>::new(body)?;
        let mut write = self.command_write.lock().await;
        write_tunnel_command(&mut *write, &cmd).await
    }

    async fn write_command<T>(write: &mut quinn::SendStream, body: T) -> P2pResult<()>
    where
        T: TunnelCommandBody,
    {
        let cmd = TunnelCommand::<T>::new(body)?;
        write_tunnel_command(write, &cmd).await
    }

    async fn read_command<R, T>(
        read: &mut R,
        expected_id: QuicTunnelCommandId,
        label: &str,
    ) -> P2pResult<T>
    where
        R: runtime::AsyncRead + Unpin,
        T: TunnelCommandBody,
    {
        let header = read_tunnel_command_header(read)
            .await
            .map_err(into_p2p_err!(
                P2pErrorCode::InvalidData,
                "{} read header failed",
                label
            ))?;
        if header.command_id != expected_id as u8 {
            return Err(p2p_err!(
                P2pErrorCode::InvalidData,
                "{} unexpected command id {} expect {}",
                label,
                header.command_id,
                expected_id as u8
            ));
        }
        Ok(read_tunnel_command_body::<_, T>(read, header).await?.body)
    }

    async fn write_channel_command<W, T>(write: &mut W, body: T) -> P2pResult<()>
    where
        W: runtime::AsyncWrite + Unpin,
        T: TunnelCommandBody,
    {
        let cmd = TunnelCommand::<T>::new(body)?;
        write_tunnel_command(write, &cmd).await
    }

    async fn read_channel_command<R, T>(
        read: &mut R,
        expected_id: QuicTunnelCommandId,
        label: &str,
    ) -> P2pResult<T>
    where
        R: runtime::AsyncRead + Unpin,
        T: TunnelCommandBody,
    {
        let header = read_tunnel_command_header(read).await?;
        let Some(command_id) = QuicTunnelCommandId::from_u8(header.command_id) else {
            return Err(p2p_err!(
                P2pErrorCode::InvalidData,
                "unknown quic {} command id {}",
                label,
                header.command_id
            ));
        };
        if command_id != expected_id {
            return Err(p2p_err!(
                P2pErrorCode::InvalidData,
                "unexpected quic {} command id {:?}, expect {:?}",
                label,
                command_id,
                expected_id
            ));
        }
        Ok(read_tunnel_command_body::<_, T>(read, header).await?.body)
    }

    fn handle_decode_error(&self, message: &str, err: crate::error::P2pError) {
        self.close_with_error(P2pErrorCode::InvalidData, format!("{}: {:?}", message, err));
    }

    fn listen_vports(&self, kind: TunnelChannelKind) -> Option<ListenVPortsRef> {
        match kind {
            TunnelChannelKind::Stream => self.stream_vports.read().unwrap().clone(),
            TunnelChannelKind::Datagram => self.datagram_vports.read().unwrap().clone(),
        }
    }

    fn next_request_id(&self) -> u64 {
        self.next_request_id.fetch_add(1, Ordering::SeqCst)
    }

    fn record_command_recv(&self) {
        self.state.lock().unwrap().last_cmd_recv_at = Instant::now();
    }

    fn record_pong(&self) {
        self.state.lock().unwrap().last_pong_at = Instant::now();
    }

    async fn command_recv_loop(self: Arc<Self>, mut command_read: quinn::RecvStream) {
        log::debug!("quic command recv loop start {}", self.log_ctx());
        loop {
            if self.is_closed() {
                break;
            }
            let header = match read_tunnel_command_header(&mut command_read).await {
                Ok(header) => header,
                Err(err) => {
                    self.close_with_error(
                        P2pErrorCode::Interrupted,
                        format!("quic command recv failed: {:?}", err),
                    );
                    break;
                }
            };
            self.record_command_recv();
            let Some(command_id) = QuicTunnelCommandId::from_u8(header.command_id) else {
                self.close_with_error(
                    P2pErrorCode::InvalidData,
                    format!("unknown quic tunnel command id {}", header.command_id),
                );
                break;
            };
            match command_id {
                QuicTunnelCommandId::Hello | QuicTunnelCommandId::HelloResp => {
                    self.close_with_error(
                        P2pErrorCode::InvalidData,
                        "quic command stream should not carry tunnel hello after init".to_owned(),
                    );
                    break;
                }
                QuicTunnelCommandId::Ping => {
                    let ping =
                        match read_tunnel_command_body::<_, TunnelPing>(&mut command_read, header)
                            .await
                        {
                            Ok(command) => command.body,
                            Err(err) => {
                                self.handle_decode_error("quic decode ping failed", err);
                                break;
                            }
                        };
                    let _ = self
                        .send_command(TunnelPong {
                            seq: ping.seq,
                            send_time: ping.send_time,
                        })
                        .await;
                }
                QuicTunnelCommandId::Pong => {
                    if let Err(err) =
                        read_tunnel_command_body::<_, TunnelPong>(&mut command_read, header).await
                    {
                        self.handle_decode_error("quic decode pong failed", err);
                        break;
                    }
                    self.record_pong();
                }
                QuicTunnelCommandId::OpenChannelReq => {
                    self.close_with_error(
                        P2pErrorCode::InvalidData,
                        "quic command stream should not carry OpenChannelReq".to_owned(),
                    );
                    break;
                }
                QuicTunnelCommandId::OpenChannelResp => {
                    let resp = match read_tunnel_command_body::<_, TunnelOpenChannelResp>(
                        &mut command_read,
                        header,
                    )
                    .await
                    {
                        Ok(command) => command.body,
                        Err(err) => {
                            self.handle_decode_error("quic decode open channel resp failed", err);
                            break;
                        }
                    };
                    if let Some(tx) = self
                        .state
                        .lock()
                        .unwrap()
                        .pending_open_requests
                        .remove(&resp.request_id)
                    {
                        log::debug!(
                            "quic open resp {} request_id={} result={:?}",
                            self.log_ctx(),
                            resp.request_id,
                            resp.result
                        );
                        let result = if resp.result == TunnelCommandResult::Success {
                            Ok(())
                        } else {
                            Err(resp.result.into_p2p_error(format!(
                                "quic open channel rejected request {} result {:?}",
                                resp.request_id, resp.result
                            )))
                        };
                        let _ = tx.send(result);
                    } else {
                        log::warn!(
                            "quic open resp missing pending request {} request_id={} result={:?}",
                            self.log_ctx(),
                            resp.request_id,
                            resp.result
                        );
                    }
                }
            }
        }
        log::debug!("quic command recv loop stop {}", self.log_ctx());
    }

    async fn heartbeat_loop(self: Arc<Self>) {
        log::debug!("quic heartbeat loop start {}", self.log_ctx());
        loop {
            runtime::sleep(COMMAND_HEARTBEAT_INTERVAL).await;
            if self.is_closed() {
                break;
            }
            let (last_cmd_recv_at, last_pong_at) = {
                let state = self.state.lock().unwrap();
                (state.last_cmd_recv_at, state.last_pong_at)
            };
            let now = Instant::now();
            if now.duration_since(last_cmd_recv_at) > COMMAND_HEARTBEAT_TIMEOUT
                && now.duration_since(last_pong_at) > COMMAND_HEARTBEAT_TIMEOUT
            {
                log::warn!(
                    "quic heartbeat timeout {} last_cmd_recv_elapsed_ms={} last_pong_elapsed_ms={}",
                    self.log_ctx(),
                    now.duration_since(last_cmd_recv_at).as_millis(),
                    now.duration_since(last_pong_at).as_millis()
                );
                self.close_with_error(
                    P2pErrorCode::Interrupted,
                    "quic command heartbeat timeout".to_owned(),
                );
                break;
            }
            if now.duration_since(last_cmd_recv_at) >= COMMAND_HEARTBEAT_INTERVAL {
                let seq = self.next_ping_seq.fetch_add(1, Ordering::SeqCst);
                if let Err(err) = self.send_command(TunnelPing { seq, send_time: 0 }).await {
                    log::debug!(
                        "quic heartbeat ping send failed {} seq={} err={:?}",
                        self.log_ctx(),
                        seq,
                        err
                    );
                    break;
                }
            }
        }
        log::debug!("quic heartbeat loop stop {}", self.log_ctx());
    }

    async fn bi_accept_loop(self: Arc<Self>) {
        log::debug!("quic bi accept loop start {}", self.log_ctx());
        loop {
            if self.is_closed() {
                break;
            }
            let accepted = self.socket.accept_bi().await;
            let (send, recv) = match accepted {
                Ok(stream) => stream,
                Err(err) => {
                    self.close_with_error(
                        P2pErrorCode::Interrupted,
                        format!("quic accept bi failed: {:?}", err),
                    );
                    break;
                }
            };
            log::debug!("quic bi accepted raw stream {}", self.log_ctx());
            let tunnel = self.clone();
            Executor::spawn_ok(async move {
                tunnel.handle_incoming_stream_open(send, recv).await;
            });
        }
        log::debug!("quic bi accept loop stop {}", self.log_ctx());
    }

    async fn uni_accept_loop(self: Arc<Self>) {
        log::debug!("quic uni accept loop start {}", self.log_ctx());
        loop {
            if self.is_closed() {
                break;
            }
            let recv = match self.socket.accept_uni().await {
                Ok(recv) => recv,
                Err(err) => {
                    self.close_with_error(
                        P2pErrorCode::Interrupted,
                        format!("quic accept uni failed: {:?}", err),
                    );
                    break;
                }
            };
            log::debug!("quic uni accepted raw stream {}", self.log_ctx());
            let tunnel = self.clone();
            Executor::spawn_ok(async move {
                tunnel.handle_incoming_datagram_open(recv).await;
            });
        }
        log::debug!("quic uni accept loop stop {}", self.log_ctx());
    }

    fn incoming_open_result(
        &self,
        kind: TunnelChannelKind,
        purpose: &TunnelPurpose,
    ) -> TunnelCommandResult {
        let Some(vports) = self.listen_vports(kind) else {
            return TunnelCommandResult::ListenerClosed;
        };

        if !vports.is_listen(purpose) {
            return TunnelCommandResult::PortNotListen;
        }

        TunnelCommandResult::Success
    }

    async fn handle_incoming_stream_open(
        self: Arc<Self>,
        mut send: quinn::SendStream,
        mut recv: quinn::RecvStream,
    ) {
        let req = match Self::read_channel_command::<_, TunnelOpenChannelReq>(
            &mut recv,
            QuicTunnelCommandId::OpenChannelReq,
            "stream open req",
        )
        .await
        {
            Ok(req) => req,
            Err(err) => {
                self.close_with_error(
                    P2pErrorCode::InvalidData,
                    format!("quic decode stream open req failed: {:?}", err),
                );
                return;
            }
        };
        log::debug!(
            "quic incoming stream open req {} request_id={} kind={:?} purpose={}",
            self.log_ctx(),
            req.request_id,
            req.kind,
            req.purpose
        );

        let result = if req.kind != TunnelChannelKind::Stream {
            TunnelCommandResult::InvalidParam
        } else {
            self.incoming_open_result(req.kind, &req.purpose)
        };

        if result != TunnelCommandResult::Success {
            log::warn!(
                "quic incoming stream open rejected {} request_id={} purpose={} result={:?}",
                self.log_ctx(),
                req.request_id,
                req.purpose,
                result
            );
            let _ = Self::write_channel_command(
                &mut send,
                TunnelOpenChannelResp {
                    request_id: req.request_id,
                    result,
                },
            )
            .await;
            return;
        }

        if self.stream_tx.is_closed() {
            log::warn!(
                "quic incoming stream open queue closed {} request_id={} purpose={}",
                self.log_ctx(),
                req.request_id,
                req.purpose
            );
            let _ = Self::write_channel_command(
                &mut send,
                TunnelOpenChannelResp {
                    request_id: req.request_id,
                    result: TunnelCommandResult::ListenerClosed,
                },
            )
            .await;
            return;
        }

        if Self::write_channel_command(
            &mut send,
            TunnelOpenChannelResp {
                request_id: req.request_id,
                result: TunnelCommandResult::Success,
            },
        )
        .await
        .is_err()
        {
            log::warn!(
                "quic incoming stream open ack write failed {} request_id={} purpose={}",
                self.log_ctx(),
                req.request_id,
                req.purpose
            );
            return;
        }

        if self
            .stream_tx
            .send(QuicAcceptedStream {
                purpose: req.purpose.clone(),
                read: recv,
                write: send,
            })
            .is_err()
        {
            log::warn!(
                "quic incoming stream open deliver failed {} request_id={} purpose={}",
                self.log_ctx(),
                req.request_id,
                req.purpose
            );
            return;
        }
        log::debug!(
            "quic incoming stream open accepted {} request_id={} purpose={}",
            self.log_ctx(),
            req.request_id,
            req.purpose
        );
    }

    async fn handle_incoming_datagram_open(self: Arc<Self>, mut recv: quinn::RecvStream) {
        let req = match Self::read_channel_command::<_, TunnelOpenChannelReq>(
            &mut recv,
            QuicTunnelCommandId::OpenChannelReq,
            "datagram open req",
        )
        .await
        {
            Ok(req) => req,
            Err(err) => {
                self.close_with_error(
                    P2pErrorCode::InvalidData,
                    format!("quic decode datagram open req failed: {:?}", err),
                );
                return;
            }
        };
        log::debug!(
            "quic incoming datagram open req {} request_id={} kind={:?} purpose={}",
            self.log_ctx(),
            req.request_id,
            req.kind,
            req.purpose
        );

        let mut result = if req.kind != TunnelChannelKind::Datagram {
            TunnelCommandResult::InvalidParam
        } else {
            self.incoming_open_result(req.kind, &req.purpose)
        };

        if result == TunnelCommandResult::Success
            && self
                .datagram_tx
                .send(QuicAcceptedDatagram {
                    purpose: req.purpose.clone(),
                    read: recv,
                })
                .is_err()
        {
            result = TunnelCommandResult::ListenerClosed;
        }

        if result != TunnelCommandResult::Success {
            log::warn!(
                "quic incoming datagram open rejected {} request_id={} purpose={} result={:?}",
                self.log_ctx(),
                req.request_id,
                req.purpose,
                result
            );
        } else {
            log::debug!(
                "quic incoming datagram open accepted {} request_id={} purpose={}",
                self.log_ctx(),
                req.request_id,
                req.purpose
            );
        }
        let _ = self
            .send_command(TunnelOpenChannelResp {
                request_id: req.request_id,
                result,
            })
            .await;
    }

    fn begin_open_channel(&self) -> (u64, oneshot::Receiver<P2pResult<()>>) {
        let request_id = self.next_request_id();
        let (tx, rx) = oneshot::channel();
        self.state
            .lock()
            .unwrap()
            .pending_open_requests
            .insert(request_id, tx);
        (request_id, rx)
    }

    async fn wait_open_channel(
        &self,
        request_id: u64,
        rx: oneshot::Receiver<P2pResult<()>>,
    ) -> P2pResult<()> {
        let result = match runtime::timeout(COMMAND_REQUEST_TIMEOUT, async move { rx.await }).await
        {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => {
                self.state
                    .lock()
                    .unwrap()
                    .pending_open_requests
                    .remove(&request_id);
                return Err(p2p_err!(
                    P2pErrorCode::Interrupted,
                    "quic open channel interrupted"
                ));
            }
            Err(_) => {
                self.state
                    .lock()
                    .unwrap()
                    .pending_open_requests
                    .remove(&request_id);
                self.close_with_error(
                    P2pErrorCode::Timeout,
                    format!("quic open datagram timeout request {}", request_id),
                );
                return Err(p2p_err!(P2pErrorCode::Timeout, "quic open channel timeout"));
            }
        }?;
        self.state
            .lock()
            .unwrap()
            .pending_open_requests
            .remove(&request_id);
        let _ = result;
        log::debug!(
            "quic open {} completed {} request_id={}",
            Self::kind_name(TunnelChannelKind::Datagram),
            self.log_ctx(),
            request_id
        );
        Ok(())
    }
}

#[async_trait::async_trait]
impl Tunnel for QuicTunnel {
    fn tunnel_id(&self) -> TunnelId {
        self.tunnel_id
    }

    fn candidate_id(&self) -> TunnelCandidateId {
        self.candidate_id
    }

    fn form(&self) -> TunnelForm {
        self.form
    }

    fn is_reverse(&self) -> bool {
        self.is_reverse
    }

    fn protocol(&self) -> Protocol {
        Protocol::Quic
    }

    fn local_id(&self) -> P2pId {
        self.local_id.clone()
    }

    fn remote_id(&self) -> P2pId {
        self.remote_id.clone()
    }

    fn local_ep(&self) -> Option<Endpoint> {
        Some(self.local_ep)
    }

    fn remote_ep(&self) -> Option<Endpoint> {
        Some(self.remote_ep)
    }

    fn state(&self) -> TunnelState {
        if self.is_closed() || self.socket.close_reason().is_some() {
            TunnelState::Closed
        } else {
            TunnelState::Connected
        }
    }

    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst) || self.socket.close_reason().is_some()
    }

    async fn close(&self) -> P2pResult<()> {
        self.close_with_error(P2pErrorCode::Interrupted, "tunnel closed".to_owned());
        Ok(())
    }

    async fn listen_stream(&self, vports: ListenVPortsRef) -> P2pResult<()> {
        *self.stream_vports.write().unwrap() = Some(vports);
        log::debug!("quic listen stream registered {}", self.log_ctx());
        self.start_bi_accept_loop_if_needed();
        Ok(())
    }

    async fn listen_datagram(&self, vports: ListenVPortsRef) -> P2pResult<()> {
        *self.datagram_vports.write().unwrap() = Some(vports);
        log::debug!("quic listen datagram registered {}", self.log_ctx());
        self.start_uni_accept_loop_if_needed();
        Ok(())
    }

    async fn open_stream(
        &self,
        purpose: TunnelPurpose,
    ) -> P2pResult<(TunnelStreamRead, TunnelStreamWrite)> {
        let purpose_for_log = purpose.clone();
        log::debug!(
            "quic open stream start {} purpose={}",
            self.log_ctx(),
            purpose_for_log
        );
        let (mut send, recv) = match self.socket.open_bi().await {
            Ok(stream) => stream,
            Err(err) => {
                log::warn!(
                    "quic open stream open_bi failed {} purpose={} err={:?}",
                    self.log_ctx(),
                    purpose_for_log,
                    err
                );
                return Err(into_p2p_err!(
                    P2pErrorCode::ConnectFailed,
                    "quic to {} open bi failed",
                    self.remote_ep
                )(err));
            }
        };
        let request_id = self.next_request_id();
        if let Err(err) = Self::write_channel_command(
            &mut send,
            TunnelOpenChannelReq {
                request_id,
                kind: TunnelChannelKind::Stream,
                purpose,
            },
        )
        .await
        {
            log::warn!(
                "quic open stream write req failed {} request_id={} purpose={} err={:?}",
                self.log_ctx(),
                request_id,
                purpose_for_log,
                err
            );
            return Err(err);
        }
        log::debug!(
            "quic open stream req sent {} request_id={} purpose={}",
            self.log_ctx(),
            request_id,
            purpose_for_log
        );
        let mut recv = recv;
        let resp = runtime::timeout(
            COMMAND_REQUEST_TIMEOUT,
            Self::read_channel_command::<_, TunnelOpenChannelResp>(
                &mut recv,
                QuicTunnelCommandId::OpenChannelResp,
                "stream open resp",
            ),
        )
        .await
        .map_err(|_| {
            log::warn!(
                "quic open stream wait resp timeout {} request_id={} purpose={}",
                self.log_ctx(),
                request_id,
                purpose_for_log
            );
            self.close_with_error(
                P2pErrorCode::Timeout,
                format!(
                    "quic open stream timeout request_id={} purpose={}",
                    request_id, purpose_for_log
                ),
            );
            p2p_err!(P2pErrorCode::Timeout, "quic open stream timeout")
        })??;
        if resp.request_id != request_id {
            log::warn!(
                "quic open stream resp mismatch {} request_id={} resp_request_id={} purpose={}",
                self.log_ctx(),
                request_id,
                resp.request_id,
                purpose_for_log
            );
            return Err(p2p_err!(
                P2pErrorCode::InvalidData,
                "quic open stream resp request_id mismatch"
            ));
        }
        if resp.result != TunnelCommandResult::Success {
            log::warn!(
                "quic open stream rejected {} request_id={} purpose={} result={:?}",
                self.log_ctx(),
                request_id,
                purpose_for_log,
                resp.result
            );
            return Err(resp.result.into_p2p_error(format!(
                "quic open stream rejected request {} result {:?}",
                request_id, resp.result
            )));
        }
        log::debug!(
            "quic open stream success {} request_id={} purpose={}",
            self.log_ctx(),
            request_id,
            purpose_for_log
        );
        Ok((Box::pin(recv), Box::pin(send)))
    }

    async fn accept_stream(
        &self,
    ) -> P2pResult<(TunnelPurpose, TunnelStreamRead, TunnelStreamWrite)> {
        if self.is_closed() {
            return Err(p2p_err!(P2pErrorCode::Interrupted, "quic tunnel closed"));
        }
        if self.listen_vports(TunnelChannelKind::Stream).is_none() {
            return Err(p2p_err!(
                P2pErrorCode::Interrupted,
                "quic accept requires listen before accept"
            ));
        }
        let mut rx = self.stream_rx.lock().await;
        let closed = self.close_notify.notified();
        tokio::pin!(closed);
        tokio::select! {
            accepted = rx.recv() => {
                let accepted = accepted.ok_or_else(|| p2p_err!(P2pErrorCode::Interrupted, "quic stream accept queue closed"))?;
                log::debug!(
                    "quic accept stream dequeued {} purpose={}",
                    self.log_ctx(),
                    accepted.purpose
                );
                Ok((accepted.purpose, Box::pin(accepted.read), Box::pin(accepted.write)))
            }
            _ = &mut closed => Err(p2p_err!(P2pErrorCode::Interrupted, "quic tunnel closed")),
        }
    }

    async fn open_datagram(&self, purpose: TunnelPurpose) -> P2pResult<TunnelDatagramWrite> {
        let purpose_for_log = purpose.clone();
        log::debug!(
            "quic open datagram start {} purpose={}",
            self.log_ctx(),
            purpose_for_log
        );
        let mut send = match self.socket.open_uni().await {
            Ok(send) => send,
            Err(err) => {
                log::warn!(
                    "quic open datagram open_uni failed {} purpose={} err={:?}",
                    self.log_ctx(),
                    purpose_for_log,
                    err
                );
                return Err(into_p2p_err!(
                    P2pErrorCode::ConnectFailed,
                    "quic to {} open uni failed",
                    self.remote_ep
                )(err));
            }
        };
        let (request_id, rx) = self.begin_open_channel();
        if let Err(err) = Self::write_channel_command(
            &mut send,
            TunnelOpenChannelReq {
                request_id,
                kind: TunnelChannelKind::Datagram,
                purpose,
            },
        )
        .await
        {
            log::warn!(
                "quic open datagram write req failed {} request_id={} purpose={} err={:?}",
                self.log_ctx(),
                request_id,
                purpose_for_log,
                err
            );
            self.state
                .lock()
                .unwrap()
                .pending_open_requests
                .remove(&request_id);
            return Err(err);
        }
        log::debug!(
            "quic open datagram req sent {} request_id={} purpose={}",
            self.log_ctx(),
            request_id,
            purpose_for_log
        );
        self.wait_open_channel(request_id, rx).await?;
        Ok(Box::pin(send))
    }

    async fn accept_datagram(&self) -> P2pResult<(TunnelPurpose, TunnelDatagramRead)> {
        if self.is_closed() {
            return Err(p2p_err!(P2pErrorCode::Interrupted, "quic tunnel closed"));
        }
        if self.listen_vports(TunnelChannelKind::Datagram).is_none() {
            return Err(p2p_err!(
                P2pErrorCode::Interrupted,
                "quic accept requires listen before accept"
            ));
        }
        let mut rx = self.datagram_rx.lock().await;
        let closed = self.close_notify.notified();
        tokio::pin!(closed);
        tokio::select! {
            accepted = rx.recv() => {
                let accepted = accepted.ok_or_else(|| p2p_err!(P2pErrorCode::Interrupted, "quic datagram accept queue closed"))?;
                log::debug!(
                    "quic accept datagram dequeued {} purpose={}",
                    self.log_ctx(),
                    accepted.purpose
                );
                Ok((accepted.purpose, Box::pin(accepted.read)))
            }
            _ = &mut closed => Err(p2p_err!(P2pErrorCode::Interrupted, "quic tunnel closed")),
        }
    }
}
