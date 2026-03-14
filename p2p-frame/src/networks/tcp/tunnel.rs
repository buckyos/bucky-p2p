use super::connection::{TcpTlsConnection, connect_with_optional_local};
use super::protocol::{
    ClaimConnAck, ClaimConnAckResult, ClaimConnReq, ControlConnReady, ControlConnReadyResult,
    DataConnReady, DataConnReadyResult, OpenDataConnReq, PingCmd, PongCmd, ReadDone, TcpChannelId,
    TcpChannelKind, TcpConnId, TcpConnectionHello, TcpConnectionRole, TcpControlCmd, TcpLeaseSeq,
    TcpRequestId, WriteFin, read_raw_frame, write_raw_frame,
};
use crate::endpoint::Endpoint;
use crate::error::{P2pError, P2pErrorCode, P2pResult, p2p_err};
use crate::executor::Executor;
use crate::networks::{
    ListenVPortsRef, Tunnel, TunnelDatagramRead, TunnelDatagramWrite, TunnelForm, TunnelPurpose,
    TunnelState, TunnelStreamRead, TunnelStreamWrite,
};
use crate::p2p_identity::{P2pId, P2pIdentityCertFactoryRef, P2pIdentityRef};
use crate::runtime;
use crate::types::{Timestamp, TunnelCandidateId, TunnelId};
use rand::random;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::task::{Context, Poll};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::{Mutex as AsyncMutex, Notify, mpsc, oneshot};

const FIRST_LISTEN_WAIT_TIMEOUT: Duration = Duration::from_millis(300);
const FIRST_LISTEN_WAIT_NOT_STARTED: u8 = 0;
const FIRST_LISTEN_WAIT_IN_PROGRESS: u8 = 1;
const FIRST_LISTEN_WAIT_DONE: u8 = 2;
const TCP_LOCAL_ID_HIGH_BIT: u32 = 1u32 << 31;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TcpIncomingControlDecision {
    Accept,
    ProtocolError,
    TunnelConflictLost,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LocalTunnelPhase {
    PassiveReady,
    Connected,
    Closed,
    Error,
}

struct AcceptedChannel {
    purpose: TunnelPurpose,
    lease: Arc<Mutex<LeaseContext>>,
}

struct ClaimReqProcessing {
    response: TcpControlCmd,
    accepted: Option<(TcpChannelKind, AcceptedChannel)>,
    local_conflict_channel: Option<TcpChannelId>,
}

type PendingDrainKey = (TcpConnId, TcpLeaseSeq);

struct PendingDrainFacts {
    write_fin: Option<WriteFin>,
    read_done: Option<ReadDone>,
    updated_at: Instant,
}

struct ClaimingState {
    lease_seq: TcpLeaseSeq,
    channel_id: TcpChannelId,
    claim_nonce: u64,
    kind: TcpChannelKind,
    purpose: TunnelPurpose,
}

#[derive(Clone)]
struct RecentClaimRecord {
    channel_id: TcpChannelId,
    lease_seq: TcpLeaseSeq,
    claim_nonce: u64,
    kind: TcpChannelKind,
    purpose: TunnelPurpose,
    response: RecentClaimResponse,
}

#[derive(Clone, Copy)]
enum RecentClaimResponse {
    Ack,
    Err(ClaimConnAckResult),
}

enum DataConnEntryState {
    FirstClaimPending,
    Idle,
    Claiming(ClaimingState),
    Bound(Arc<Mutex<LeaseContext>>),
    Draining(Arc<Mutex<LeaseContext>>),
    Retired,
}

struct DataConnInner {
    committed_lease_seq: TcpLeaseSeq,
    state: DataConnEntryState,
    no_reuse: bool,
    recent_claim: Option<RecentClaimRecord>,
    pending_fin: Option<WriteFin>,
    pending_read_done: Option<ReadDone>,
}

struct DataConnEntry {
    conn_id: TcpConnId,
    created_by_local: bool,
    stream: Mutex<Option<runtime::TlsStream<runtime::TcpStream>>>,
    local_ep: Endpoint,
    remote_ep: Endpoint,
    local_id: P2pId,
    local_name: String,
    remote_id: P2pId,
    remote_name: String,
    inner: Mutex<DataConnInner>,
}

struct LeaseContext {
    tunnel: Weak<TcpTunnel>,
    entry: Weak<DataConnEntry>,
    conn_id: TcpConnId,
    lease_seq: TcpLeaseSeq,
    channel_id: TcpChannelId,
    kind: TcpChannelKind,
    purpose: TunnelPurpose,
    local_initiator: bool,
    read: Option<runtime::ReadHalf<runtime::TlsStream<runtime::TcpStream>>>,
    write: Option<runtime::WriteHalf<runtime::TlsStream<runtime::TcpStream>>>,
    tx_bytes: u64,
    rx_bytes: u64,
    peer_final_tx_bytes: Option<u64>,
    pending_peer_read_done_final_rx_bytes: Option<u64>,
    local_write_fin_sent: bool,
    peer_write_fin_received: bool,
    local_read_done_sent: bool,
    peer_read_done_received: bool,
    read_released: bool,
    write_released: bool,
    retired: bool,
    no_reuse: bool,
    hidden_zero_read: bool,
    last_drain_progress_at: Option<Instant>,
}

struct TcpLeaseRead {
    lease: Arc<Mutex<LeaseContext>>,
}

struct TcpLeaseWrite {
    lease: Arc<Mutex<LeaseContext>>,
}

struct TcpTunnelState {
    phase: LocalTunnelPhase,
    last_recv_at: Instant,
    last_pong_at: Instant,
    data_conns: HashMap<TcpConnId, Arc<DataConnEntry>>,
    pending_drains: HashMap<PendingDrainKey, PendingDrainFacts>,
    pending_open_requests: HashMap<TcpRequestId, oneshot::Sender<P2pResult<Arc<DataConnEntry>>>>,
    pending_claims: HashMap<TcpChannelId, oneshot::Sender<P2pResult<Arc<Mutex<LeaseContext>>>>>,
}

#[derive(Clone)]
pub(crate) struct TcpTunnelConnector {
    pub cert_factory: P2pIdentityCertFactoryRef,
    pub local_identity: P2pIdentityRef,
    pub local_ep: Endpoint,
    pub remote_ep: Arc<Mutex<Endpoint>>,
    pub remote_id: P2pId,
    pub remote_name: Option<String>,
    pub timeout: Duration,
    pub tunnel_id: TunnelId,
    pub candidate_id: TunnelCandidateId,
}

pub(crate) struct TcpTunnel {
    tunnel_id: TunnelId,
    candidate_id: TunnelCandidateId,
    form: TunnelForm,
    is_reverse: bool,
    local_id: P2pId,
    remote_id: P2pId,
    local_ep: Endpoint,
    remote_ep: Endpoint,
    control_write: Arc<AsyncMutex<runtime::WriteHalf<runtime::TlsStream<runtime::TcpStream>>>>,
    connector: TcpTunnelConnector,
    self_weak: Mutex<Weak<TcpTunnel>>,
    state: Mutex<TcpTunnelState>,
    closed: AtomicBool,
    next_conn_seq: AtomicU32,
    next_channel_seq: AtomicU32,
    next_request_seq: AtomicU32,
    next_ping_seq: AtomicU64,
    stream_rx: AsyncMutex<mpsc::UnboundedReceiver<P2pResult<AcceptedChannel>>>,
    stream_tx: mpsc::UnboundedSender<P2pResult<AcceptedChannel>>,
    datagram_rx: AsyncMutex<mpsc::UnboundedReceiver<P2pResult<AcceptedChannel>>>,
    datagram_tx: mpsc::UnboundedSender<P2pResult<AcceptedChannel>>,
    stream_accept_limit: AtomicU64,
    stream_accept_pending: AtomicU64,
    datagram_accept_limit: AtomicU64,
    datagram_accept_pending: AtomicU64,
    stream_vports: std::sync::RwLock<Option<ListenVPortsRef>>,
    stream_first_listen_wait_state: AtomicU8,
    stream_vports_notify: Notify,
    datagram_vports: std::sync::RwLock<Option<ListenVPortsRef>>,
    datagram_first_listen_wait_state: AtomicU8,
    datagram_vports_notify: Notify,
    heartbeat_interval: Duration,
    heartbeat_timeout: Duration,
    unclaimed_data_conn_budget: AtomicU64,
    connected_notify: Notify,
    #[cfg(test)]
    reverse_open_delay: Mutex<Duration>,
    #[cfg(test)]
    claim_req_delay: Mutex<Duration>,
    #[cfg(test)]
    suppress_pong: AtomicBool,
    #[cfg(test)]
    suppress_ping: AtomicBool,
    #[cfg(test)]
    fail_next_control_send: AtomicBool,
}

impl DataConnEntry {
    fn new(connection: TcpTlsConnection, conn_id: TcpConnId, created_by_local: bool) -> Arc<Self> {
        Arc::new(Self {
            conn_id,
            created_by_local,
            stream: Mutex::new(Some(connection.stream)),
            local_ep: connection.local_ep,
            remote_ep: connection.remote_ep,
            local_id: connection.local_id,
            local_name: connection.local_name,
            remote_id: connection.remote_id,
            remote_name: connection.remote_name,
            inner: Mutex::new(DataConnInner {
                committed_lease_seq: TcpLeaseSeq::default(),
                state: DataConnEntryState::FirstClaimPending,
                no_reuse: false,
                recent_claim: None,
                pending_fin: None,
                pending_read_done: None,
            }),
        })
    }

    fn take_stream(&self) -> P2pResult<runtime::TlsStream<runtime::TcpStream>> {
        self.stream
            .lock()
            .unwrap()
            .take()
            .ok_or_else(|| p2p_err!(P2pErrorCode::ErrorState, "data connection stream missing"))
    }

    fn put_stream(&self, stream: runtime::TlsStream<runtime::TcpStream>) {
        *self.stream.lock().unwrap() = Some(stream);
    }

    fn drop_stream(&self) {
        let _ = self.stream.lock().unwrap().take();
    }
}

impl LeaseContext {
    fn can_send_read_done(&self) -> bool {
        self.peer_write_fin_received
            && self.peer_final_tx_bytes == Some(self.rx_bytes)
            && !self.local_read_done_sent
    }

    fn ready_for_idle(&self) -> bool {
        self.local_write_fin_sent
            && self.peer_write_fin_received
            && self.local_read_done_sent
            && self.peer_read_done_received
            && self.read_released
            && self.write_released
            && !self.retired
            && !self.no_reuse
    }

    fn ready_for_close(&self) -> bool {
        self.read_released && self.write_released && (self.retired || self.no_reuse)
    }
}

impl TcpLeaseRead {
    fn after_read(lease: Arc<Mutex<LeaseContext>>, read_len: usize, eof: bool) {
        let mut retire = false;
        let mut send_read_done = false;
        let mut refresh_drain_timeout = false;
        {
            let mut inner = lease.lock().unwrap();
            if inner.retired {
                return;
            }
            if read_len > 0 {
                inner.rx_bytes += read_len as u64;
                if inner.hidden_zero_read {
                    retire = true;
                } else if let Some(peer_final) = inner.peer_final_tx_bytes {
                    if inner.rx_bytes > peer_final {
                        retire = true;
                    } else if inner.can_send_read_done() {
                        send_read_done = true;
                    } else if inner.peer_write_fin_received && inner.rx_bytes < peer_final {
                        refresh_drain_timeout = true;
                    }
                }
            } else if eof {
                retire = true;
            }
        }
        if retire {
            TcpTunnel::retire_lease_arc(&lease, P2pErrorCode::Interrupted);
        } else if send_read_done {
            TcpTunnel::spawn_send_read_done(lease);
        } else if refresh_drain_timeout {
            TcpTunnel::note_drain_progress(&lease);
        }
    }
}

impl runtime::AsyncRead for TcpLeaseRead {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let mut lease = self.lease.lock().unwrap();
        let before = buf.filled().len();
        let result = if let Some(read) = lease.read.as_mut() {
            Pin::new(read).poll_read(cx, buf)
        } else {
            Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "tcp lease read released",
            )))
        };
        drop(lease);

        match result {
            Poll::Ready(Ok(())) => {
                let read_len = buf.filled().len().saturating_sub(before);
                Self::after_read(self.lease.clone(), read_len, read_len == 0);
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(err)) => {
                TcpTunnel::retire_lease_arc(&self.lease, P2pErrorCode::IoError);
                Poll::Ready(Err(err))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl Drop for TcpLeaseRead {
    fn drop(&mut self) {
        TcpTunnel::release_read_half(self.lease.clone());
    }
}

impl runtime::AsyncWrite for TcpLeaseWrite {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let mut lease = self.lease.lock().unwrap();
        let result = if let Some(write) = lease.write.as_mut() {
            Pin::new(write).poll_write(cx, buf)
        } else {
            Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "tcp lease write released",
            )))
        };
        match result {
            Poll::Ready(Ok(n)) => {
                lease.tx_bytes += n as u64;
                Poll::Ready(Ok(n))
            }
            Poll::Ready(Err(err)) => {
                drop(lease);
                TcpTunnel::retire_lease_arc(&self.lease, P2pErrorCode::IoError);
                Poll::Ready(Err(err))
            }
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let mut lease = self.lease.lock().unwrap();
        let result = if let Some(write) = lease.write.as_mut() {
            Pin::new(write).poll_flush(cx)
        } else {
            Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "tcp lease write released",
            )))
        };
        if let Poll::Ready(Err(_)) = &result {
            drop(lease);
            TcpTunnel::retire_lease_arc(&self.lease, P2pErrorCode::IoError);
        }
        result
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let mut lease = self.lease.lock().unwrap();
        let result = if let Some(write) = lease.write.as_mut() {
            Pin::new(write).poll_shutdown(cx)
        } else {
            Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "tcp lease write released",
            )))
        };
        if let Poll::Ready(Err(_)) = &result {
            drop(lease);
            TcpTunnel::retire_lease_arc(&self.lease, P2pErrorCode::IoError);
        }
        result
    }
}

impl Drop for TcpLeaseWrite {
    fn drop(&mut self) {
        TcpTunnel::release_write_half(self.lease.clone());
    }
}

impl TcpTunnelConnector {
    fn connect_local_ep(&self) -> Endpoint {
        let mut ep = Endpoint::default_tcp(&self.local_ep);
        ep.mut_addr().set_ip(self.local_ep.addr().ip());
        ep.mut_addr().set_port(0);
        ep
    }

    pub async fn open_data_connection(
        &self,
        conn_id: TcpConnId,
        open_request_id: Option<TcpRequestId>,
    ) -> P2pResult<TcpTlsConnection> {
        let hello = TcpConnectionHello {
            role: TcpConnectionRole::Data,
            tunnel_id: self.tunnel_id,
            candidate_id: self.candidate_id,
            is_reverse: false,
            conn_id: Some(conn_id),
            open_request_id,
        };
        let local_ep = self.connect_local_ep();
        let remote_ep = *self.remote_ep.lock().unwrap();
        connect_with_optional_local(
            self.cert_factory.clone(),
            &self.local_identity,
            Some(&local_ep),
            &remote_ep,
            &self.remote_id,
            self.remote_name.clone(),
            self.timeout,
            &hello,
        )
        .await
    }
}

impl TcpTunnel {
    pub(crate) fn tunnel_id(&self) -> TunnelId {
        self.tunnel_id
    }

    pub(crate) fn candidate_id(&self) -> TunnelCandidateId {
        self.candidate_id
    }

    #[cfg(test)]
    pub(crate) fn set_data_remote_ep_for_test(&self, remote_ep: Endpoint) {
        *self.connector.remote_ep.lock().unwrap() = remote_ep;
    }

    #[cfg(test)]
    pub(crate) fn set_reverse_open_delay_for_test(&self, delay: Duration) {
        *self.reverse_open_delay.lock().unwrap() = delay;
    }

    #[cfg(test)]
    pub(crate) fn set_claim_req_delay_for_test(&self, delay: Duration) {
        *self.claim_req_delay.lock().unwrap() = delay;
    }

    #[cfg(test)]
    pub(crate) fn set_suppress_pong_for_test(&self, suppress: bool) {
        self.suppress_pong.store(suppress, Ordering::SeqCst);
    }

    #[cfg(test)]
    pub(crate) fn set_suppress_ping_for_test(&self, suppress: bool) {
        self.suppress_ping.store(suppress, Ordering::SeqCst);
    }

    #[cfg(test)]
    pub(crate) fn fail_next_control_send_for_test(&self) {
        self.fail_next_control_send.store(true, Ordering::SeqCst);
    }

    #[cfg(test)]
    pub(crate) fn set_unclaimed_data_conn_budget_for_test(&self, budget: usize) {
        self.unclaimed_data_conn_budget
            .store(budget as u64, Ordering::SeqCst);
    }

    #[cfg(test)]
    pub(crate) fn set_accept_queue_limit_for_test(&self, kind: TcpChannelKind, limit: usize) {
        match kind {
            TcpChannelKind::Stream => self
                .stream_accept_limit
                .store(limit as u64, Ordering::SeqCst),
            TcpChannelKind::Datagram => self
                .datagram_accept_limit
                .store(limit as u64, Ordering::SeqCst),
        }
    }

    pub(crate) async fn accept_control_connection(
        mut control: TcpTlsConnection,
        connector: TcpTunnelConnector,
        tunnel_id: TunnelId,
        candidate_id: TunnelCandidateId,
        is_reverse: bool,
        decision: TcpIncomingControlDecision,
        heartbeat_interval: Duration,
        heartbeat_timeout: Duration,
    ) -> P2pResult<Option<Arc<Self>>> {
        let result = match decision {
            TcpIncomingControlDecision::Accept => ControlConnReadyResult::Success,
            TcpIncomingControlDecision::ProtocolError => ControlConnReadyResult::ProtocolError,
            TcpIncomingControlDecision::TunnelConflictLost => {
                ControlConnReadyResult::TunnelConflictLost
            }
        };
        write_raw_frame(
            &mut control.stream,
            &ControlConnReady {
                tunnel_id,
                candidate_id,
                result,
            },
        )
        .await?;
        if result != ControlConnReadyResult::Success {
            let _ = runtime::AsyncWriteExt::shutdown(&mut control.stream).await;
            return Ok(None);
        }

        Ok(Some(Self::new(
            control,
            connector,
            tunnel_id,
            candidate_id,
            TunnelForm::Passive,
            is_reverse,
            heartbeat_interval,
            heartbeat_timeout,
            LocalTunnelPhase::PassiveReady,
        )))
    }

    pub(crate) fn new(
        control: TcpTlsConnection,
        connector: TcpTunnelConnector,
        tunnel_id: TunnelId,
        candidate_id: TunnelCandidateId,
        form: TunnelForm,
        is_reverse: bool,
        heartbeat_interval: Duration,
        heartbeat_timeout: Duration,
        initial_phase: LocalTunnelPhase,
    ) -> Arc<Self> {
        let (control_read, control_write) = runtime::split(control.stream);
        let (stream_tx, stream_rx) = mpsc::unbounded_channel();
        let (datagram_tx, datagram_rx) = mpsc::unbounded_channel();
        let tunnel = Arc::new(Self {
            tunnel_id,
            candidate_id,
            form,
            is_reverse,
            local_id: control.local_id,
            remote_id: control.remote_id,
            local_ep: control.local_ep,
            remote_ep: control.remote_ep,
            control_write: Arc::new(AsyncMutex::new(control_write)),
            connector,
            self_weak: Mutex::new(Weak::new()),
            state: Mutex::new(TcpTunnelState {
                phase: initial_phase,
                last_recv_at: Instant::now(),
                last_pong_at: Instant::now(),
                data_conns: HashMap::new(),
                pending_drains: HashMap::new(),
                pending_open_requests: HashMap::new(),
                pending_claims: HashMap::new(),
            }),
            closed: AtomicBool::new(false),
            next_conn_seq: AtomicU32::new(1),
            next_channel_seq: AtomicU32::new(1),
            next_request_seq: AtomicU32::new(1),
            next_ping_seq: AtomicU64::new(1),
            stream_rx: AsyncMutex::new(stream_rx),
            stream_tx,
            datagram_rx: AsyncMutex::new(datagram_rx),
            datagram_tx,
            stream_accept_limit: AtomicU64::new(16),
            stream_accept_pending: AtomicU64::new(0),
            datagram_accept_limit: AtomicU64::new(16),
            datagram_accept_pending: AtomicU64::new(0),
            stream_vports: std::sync::RwLock::new(None),
            stream_first_listen_wait_state: AtomicU8::new(FIRST_LISTEN_WAIT_NOT_STARTED),
            stream_vports_notify: Notify::new(),
            datagram_vports: std::sync::RwLock::new(None),
            datagram_first_listen_wait_state: AtomicU8::new(FIRST_LISTEN_WAIT_NOT_STARTED),
            datagram_vports_notify: Notify::new(),
            heartbeat_interval,
            heartbeat_timeout,
            unclaimed_data_conn_budget: AtomicU64::new(8),
            connected_notify: Notify::new(),
            #[cfg(test)]
            reverse_open_delay: Mutex::new(Duration::from_millis(0)),
            #[cfg(test)]
            claim_req_delay: Mutex::new(Duration::from_millis(0)),
            #[cfg(test)]
            suppress_pong: AtomicBool::new(false),
            #[cfg(test)]
            suppress_ping: AtomicBool::new(false),
            #[cfg(test)]
            fail_next_control_send: AtomicBool::new(false),
        });
        *tunnel.self_weak.lock().unwrap() = Arc::downgrade(&tunnel);
        Self::start_loops(tunnel.clone(), control_read);
        if initial_phase == LocalTunnelPhase::Connected && form == TunnelForm::Active {
            tunnel.spawn_initial_ping();
        }
        tunnel
    }

    fn start_loops(
        this: Arc<Self>,
        control_read: runtime::ReadHalf<runtime::TlsStream<runtime::TcpStream>>,
    ) {
        let recv_tunnel = this.clone();
        let _ = Executor::spawn(async move {
            recv_tunnel.control_recv_loop(control_read).await;
        });
        let heartbeat_tunnel = this.clone();
        let _ = Executor::spawn(async move {
            heartbeat_tunnel.heartbeat_loop().await;
        });
    }

    fn external_state(phase: LocalTunnelPhase) -> TunnelState {
        match phase {
            LocalTunnelPhase::PassiveReady => TunnelState::Connecting,
            LocalTunnelPhase::Connected => TunnelState::Connected,
            LocalTunnelPhase::Closed => TunnelState::Closed,
            LocalTunnelPhase::Error => TunnelState::Error,
        }
    }

    fn now_ts() -> Timestamp {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }

    fn initial_ping_needed(&self) -> bool {
        self.form == TunnelForm::Active
            && self.state.lock().unwrap().phase == LocalTunnelPhase::Connected
    }

    fn spawn_initial_ping(self: &Arc<Self>) {
        let this = self.clone();
        let _ = Executor::spawn(async move {
            let _ = this.send_ping().await;
        });
    }

    async fn send_control(&self, cmd: &TcpControlCmd) -> P2pResult<()> {
        #[cfg(test)]
        if self.fail_next_control_send.swap(false, Ordering::SeqCst) {
            return Err(p2p_err!(
                P2pErrorCode::Interrupted,
                "injected control send failure"
            ));
        }
        let mut write = self.control_write.lock().await;
        write_raw_frame(&mut *write, cmd).await
    }

    async fn send_ping(&self) -> P2pResult<()> {
        let seq = self.next_ping_seq.fetch_add(1, Ordering::SeqCst);
        self.send_control(&TcpControlCmd::Ping(PingCmd {
            seq,
            send_time: Self::now_ts(),
        }))
        .await
    }

    fn promote_connected(&self) {
        let should_notify = {
            let mut state = self.state.lock().unwrap();
            if state.phase == LocalTunnelPhase::PassiveReady {
                state.phase = LocalTunnelPhase::Connected;
                true
            } else {
                false
            }
        };
        if should_notify {
            self.connected_notify.notify_waiters();
        }
    }

    async fn wait_until_connected(&self) -> P2pResult<()> {
        loop {
            let phase = self.state.lock().unwrap().phase;
            match phase {
                LocalTunnelPhase::Connected => return Ok(()),
                LocalTunnelPhase::Closed => {
                    return Err(p2p_err!(P2pErrorCode::Interrupted, "tunnel closed"));
                }
                LocalTunnelPhase::Error => {
                    return Err(p2p_err!(P2pErrorCode::ErrorState, "tunnel error"));
                }
                LocalTunnelPhase::PassiveReady => {
                    self.connected_notify.notified().await;
                }
            }
        }
    }

    async fn close_impl(&self, phase: LocalTunnelPhase, reason: P2pErrorCode) -> P2pResult<()> {
        if self.closed.swap(true, Ordering::SeqCst) {
            return Ok(());
        }

        let (pending_open_requests, pending_claims, data_conns) = {
            let mut state = self.state.lock().unwrap();
            state.phase = phase;
            state.pending_drains.clear();
            (
                std::mem::take(&mut state.pending_open_requests),
                std::mem::take(&mut state.pending_claims),
                state.data_conns.values().cloned().collect::<Vec<_>>(),
            )
        };
        self.connected_notify.notify_waiters();

        for (_, tx) in pending_open_requests {
            let _ = tx.send(Err(p2p_err!(reason, "tunnel closed")));
        }
        for (_, tx) in pending_claims {
            let _ = tx.send(Err(p2p_err!(reason, "tunnel closed")));
        }

        for entry in data_conns {
            let maybe_lease = {
                let mut inner = entry.inner.lock().unwrap();
                inner.no_reuse = true;
                match &inner.state {
                    DataConnEntryState::Bound(lease) | DataConnEntryState::Draining(lease) => {
                        Some(lease.clone())
                    }
                    DataConnEntryState::Claiming(_)
                    | DataConnEntryState::FirstClaimPending
                    | DataConnEntryState::Idle => {
                        inner.state = DataConnEntryState::Retired;
                        None
                    }
                    DataConnEntryState::Retired => None,
                }
            };
            if let Some(lease) = maybe_lease {
                let mut lease_inner = lease.lock().unwrap();
                lease_inner.no_reuse = true;
                if lease_inner.ready_for_close() {
                    drop(lease_inner);
                    Self::retire_lease_arc(&lease, reason);
                }
            } else {
                entry.drop_stream();
            }
        }

        let _ = self.stream_tx.send(Err(p2p_err!(reason, "tunnel closed")));
        let _ = self
            .datagram_tx
            .send(Err(p2p_err!(reason, "tunnel closed")));

        {
            let mut rx = self.stream_rx.lock().await;
            rx.close();
        }
        {
            let mut rx = self.datagram_rx.lock().await;
            rx.close();
        }

        let mut write = self.control_write.lock().await;
        let _ = runtime::AsyncWriteExt::shutdown(&mut *write).await;
        Ok(())
    }

    async fn control_recv_loop(
        self: Arc<Self>,
        mut control_read: runtime::ReadHalf<runtime::TlsStream<runtime::TcpStream>>,
    ) {
        loop {
            let cmd = read_raw_frame::<_, TcpControlCmd>(&mut control_read).await;
            let cmd = match cmd {
                Ok(cmd) => cmd,
                Err(err) => {
                    log::warn!(
                        "tcp tunnel {} recv loop stopped: {:?}",
                        self.tunnel_id.value(),
                        err
                    );
                    let _ = self
                        .close_impl(LocalTunnelPhase::Closed, P2pErrorCode::Interrupted)
                        .await;
                    break;
                }
            };
            {
                let mut state = self.state.lock().unwrap();
                state.last_recv_at = Instant::now();
            }
            self.promote_connected();
            if let Err(err) = self.handle_control_cmd(cmd).await {
                log::warn!(
                    "tcp tunnel {} handle cmd error {:?}",
                    self.tunnel_id.value(),
                    err
                );
                let _ = self
                    .close_impl(LocalTunnelPhase::Error, P2pErrorCode::Interrupted)
                    .await;
                break;
            }
        }
    }

    async fn handle_control_cmd(self: &Arc<Self>, cmd: TcpControlCmd) -> P2pResult<()> {
        match cmd {
            TcpControlCmd::Ping(ping) => {
                #[cfg(test)]
                if self.suppress_pong.load(Ordering::SeqCst) {
                    return Ok(());
                }
                self.send_control(&TcpControlCmd::Pong(PongCmd {
                    seq: ping.seq,
                    send_time: ping.send_time,
                }))
                .await?;
            }
            TcpControlCmd::Pong(_) => {
                self.state.lock().unwrap().last_pong_at = Instant::now();
            }
            TcpControlCmd::OpenDataConnReq(req) => {
                self.spawn_reverse_data_connection(req.request_id);
            }
            TcpControlCmd::ClaimConnReq(req) => {
                self.handle_claim_req(req).await?;
            }
            TcpControlCmd::ClaimConnAck(ack) => {
                self.handle_claim_ack(ack).await?;
            }
            TcpControlCmd::WriteFin(fin) => {
                self.handle_write_fin(fin)?;
            }
            TcpControlCmd::ReadDone(done) => {
                self.handle_read_done(done)?;
            }
        }
        Ok(())
    }

    fn next_local_conn_id(&self) -> TcpConnId {
        TcpConnId::from(self.next_local_id_value(&self.next_conn_seq))
    }

    fn next_local_channel_id(&self) -> TcpChannelId {
        TcpChannelId::from(self.next_local_id_value(&self.next_channel_seq))
    }

    fn next_local_request_id(&self) -> TcpRequestId {
        TcpRequestId::from(self.next_local_id_value(&self.next_request_seq))
    }

    fn next_local_id_value(&self, counter: &AtomicU32) -> u32 {
        loop {
            let seq = counter.fetch_add(1, Ordering::SeqCst) & !TCP_LOCAL_ID_HIGH_BIT;
            if seq == 0 {
                continue;
            }

            return match self.form {
                TunnelForm::Active | TunnelForm::Proxy => seq,
                TunnelForm::Passive => TCP_LOCAL_ID_HIGH_BIT | seq,
            };
        }
    }

    fn next_lease_seq(current: TcpLeaseSeq) -> TcpLeaseSeq {
        let next = current.value().wrapping_add(1);
        TcpLeaseSeq::from(if next == 0 { 1 } else { next })
    }

    fn owns_id_high_bit(&self) -> u32 {
        match self.form {
            TunnelForm::Active | TunnelForm::Proxy => 0,
            TunnelForm::Passive => 1,
        }
    }

    fn is_local_conn_creator(&self, conn_id: TcpConnId) -> bool {
        (conn_id.value() >> 31) == self.owns_id_high_bit()
    }

    fn register_entry(&self, entry: Arc<DataConnEntry>) {
        self.state
            .lock()
            .unwrap()
            .data_conns
            .insert(entry.conn_id, entry);
    }

    fn remove_entry(&self, conn_id: TcpConnId) {
        self.state.lock().unwrap().data_conns.remove(&conn_id);
    }

    fn get_entry(&self, conn_id: TcpConnId) -> Option<Arc<DataConnEntry>> {
        self.state.lock().unwrap().data_conns.get(&conn_id).cloned()
    }

    fn idle_pool_len(&self) -> usize {
        let entries = self
            .state
            .lock()
            .unwrap()
            .data_conns
            .values()
            .cloned()
            .collect::<Vec<_>>();
        entries
            .into_iter()
            .filter(|entry| matches!(entry.inner.lock().unwrap().state, DataConnEntryState::Idle))
            .count()
    }

    #[cfg(test)]
    pub(crate) fn idle_pool_len_for_test(&self) -> usize {
        self.idle_pool_len()
    }

    #[cfg(test)]
    pub(crate) fn data_conn_states_for_test(&self) -> Vec<String> {
        let entries = self
            .state
            .lock()
            .unwrap()
            .data_conns
            .values()
            .cloned()
            .collect::<Vec<_>>();
        entries
            .into_iter()
            .map(|entry| {
                let inner = entry.inner.lock().unwrap();
                let state = match &inner.state {
                    DataConnEntryState::FirstClaimPending => "FirstClaimPending",
                    DataConnEntryState::Idle => "Idle",
                    DataConnEntryState::Claiming(_) => "Claiming",
                    DataConnEntryState::Bound(_) => "Bound",
                    DataConnEntryState::Draining(_) => "Draining",
                    DataConnEntryState::Retired => "Retired",
                };
                let lease_debug = match &inner.state {
                    DataConnEntryState::Bound(lease) | DataConnEntryState::Draining(lease) => {
                        let lease = lease.lock().unwrap();
                        format!(
                            " tx={} rx={} local_fin={} peer_fin={} local_done={} peer_done={} read_rel={} write_rel={} peer_final={:?} read_some={} write_some={}",
                            lease.tx_bytes,
                            lease.rx_bytes,
                            lease.local_write_fin_sent,
                            lease.peer_write_fin_received,
                            lease.local_read_done_sent,
                            lease.peer_read_done_received,
                            lease.read_released,
                            lease.write_released,
                            lease.peer_final_tx_bytes,
                            lease.read.is_some(),
                            lease.write.is_some(),
                        )
                    }
                    _ => String::new(),
                };
                format!(
                    "conn={} state={} lease={}{}",
                    entry.conn_id, state, inner.committed_lease_seq, lease_debug
                )
            })
            .collect()
    }

    fn claim_result_to_error(result: ClaimConnAckResult) -> P2pErrorCode {
        match result {
            ClaimConnAckResult::Success => P2pErrorCode::Ok,
            ClaimConnAckResult::ConflictLost => P2pErrorCode::Conflict,
            ClaimConnAckResult::Retired => P2pErrorCode::Interrupted,
            ClaimConnAckResult::VportNotFound => P2pErrorCode::PortNotListen,
            ClaimConnAckResult::LeaseMismatch
            | ClaimConnAckResult::NotIdle
            | ClaimConnAckResult::ProtocolError
            | ClaimConnAckResult::ListenerClosed
            | ClaimConnAckResult::AcceptQueueFull => P2pErrorCode::Reject,
        }
    }

    fn claim_result_to_p2p_error(result: ClaimConnAckResult) -> P2pError {
        P2pError::new(
            Self::claim_result_to_error(result),
            format!("claim rejected: {:?}", result),
        )
    }

    fn claim_record_matches(req: &ClaimConnReq, record: &RecentClaimRecord) -> bool {
        req.channel_id == record.channel_id
            && req.lease_seq == record.lease_seq
            && req.claim_nonce == record.claim_nonce
            && req.kind == record.kind
            && req.purpose == record.purpose
    }

    fn claim_response_cmd(req: &ClaimConnReq, response: RecentClaimResponse) -> TcpControlCmd {
        let result = match response {
            RecentClaimResponse::Ack => ClaimConnAckResult::Success as u8,
            RecentClaimResponse::Err(result) => result as u8,
        };
        TcpControlCmd::ClaimConnAck(ClaimConnAck {
            channel_id: req.channel_id,
            conn_id: req.conn_id,
            lease_seq: req.lease_seq,
            result,
        })
    }

    fn send_pending_claim_result(
        &self,
        channel_id: TcpChannelId,
        result: P2pResult<Arc<Mutex<LeaseContext>>>,
    ) {
        if let Some(tx) = self
            .state
            .lock()
            .unwrap()
            .pending_claims
            .remove(&channel_id)
        {
            let _ = tx.send(result);
        }
    }

    fn send_pending_open_request_result(
        &self,
        request_id: TcpRequestId,
        result: P2pResult<Arc<DataConnEntry>>,
    ) {
        if let Some(tx) = self
            .state
            .lock()
            .unwrap()
            .pending_open_requests
            .remove(&request_id)
        {
            let _ = tx.send(result);
        }
    }

    fn insert_pending_write_fin(&self, fin: WriteFin) -> P2pResult<()> {
        let key = (fin.conn_id, fin.lease_seq);
        let updated_at;
        {
            let mut state = self.state.lock().unwrap();
            let entry = state
                .pending_drains
                .entry(key)
                .or_insert_with(|| PendingDrainFacts {
                    write_fin: None,
                    read_done: None,
                    updated_at: Instant::now(),
                });
            if let Some(existing) = &entry.write_fin {
                if existing.channel_id != fin.channel_id
                    || existing.final_tx_bytes != fin.final_tx_bytes
                {
                    return Err(p2p_err!(
                        P2pErrorCode::Interrupted,
                        "conflicting pending write fin"
                    ));
                }
                return Ok(());
            }
            entry.write_fin = Some(fin);
            entry.updated_at = Instant::now();
            updated_at = entry.updated_at;
        }
        if let Some(tunnel) = self.self_weak.lock().unwrap().upgrade() {
            tunnel.schedule_pending_drain_timeout(key, updated_at);
        }
        Ok(())
    }

    fn insert_pending_read_done(&self, done: ReadDone) -> P2pResult<()> {
        let key = (done.conn_id, done.lease_seq);
        let updated_at;
        {
            let mut state = self.state.lock().unwrap();
            let entry = state
                .pending_drains
                .entry(key)
                .or_insert_with(|| PendingDrainFacts {
                    write_fin: None,
                    read_done: None,
                    updated_at: Instant::now(),
                });
            if let Some(existing) = &entry.read_done {
                if existing.channel_id != done.channel_id
                    || existing.final_rx_bytes != done.final_rx_bytes
                {
                    return Err(p2p_err!(
                        P2pErrorCode::Interrupted,
                        "conflicting pending read done"
                    ));
                }
                return Ok(());
            }
            entry.read_done = Some(done);
            entry.updated_at = Instant::now();
            updated_at = entry.updated_at;
        }
        if let Some(tunnel) = self.self_weak.lock().unwrap().upgrade() {
            tunnel.schedule_pending_drain_timeout(key, updated_at);
        }
        Ok(())
    }

    fn take_pending_drain(
        &self,
        conn_id: TcpConnId,
        lease_seq: TcpLeaseSeq,
    ) -> Option<PendingDrainFacts> {
        self.state
            .lock()
            .unwrap()
            .pending_drains
            .remove(&(conn_id, lease_seq))
    }

    fn clear_pending_drain_for_entry(&self, conn_id: TcpConnId, lease_seq: TcpLeaseSeq) {
        self.state
            .lock()
            .unwrap()
            .pending_drains
            .remove(&(conn_id, lease_seq));
    }

    fn drain_timeout(&self) -> Duration {
        std::cmp::max(
            self.heartbeat_timeout + self.heartbeat_timeout,
            Duration::from_secs(2),
        )
    }

    fn schedule_pending_drain_timeout(self: &Arc<Self>, key: PendingDrainKey, updated_at: Instant) {
        let this = self.clone();
        let timeout = this.drain_timeout();
        let _ = Executor::spawn(async move {
            runtime::sleep(timeout).await;
            let should_retire = {
                let state = this.state.lock().unwrap();
                matches!(
                    state.pending_drains.get(&key),
                    Some(pending) if pending.updated_at == updated_at
                )
            };
            if !should_retire {
                return;
            }
            if let Some(entry) = this.get_entry(key.0) {
                this.retire_plain_entry(&entry, P2pErrorCode::Interrupted);
            } else {
                this.clear_pending_drain_for_entry(key.0, key.1);
            }
        });
    }

    fn note_drain_progress(lease: &Arc<Mutex<LeaseContext>>) {
        let (tunnel, observed_at, timeout_active) = {
            let mut inner = lease.lock().unwrap();
            let timeout_active = inner
                .peer_final_tx_bytes
                .map(|peer_final| inner.peer_write_fin_received && inner.rx_bytes < peer_final)
                .unwrap_or(false);
            if !timeout_active {
                inner.last_drain_progress_at = None;
                return;
            }
            let now = Instant::now();
            inner.last_drain_progress_at = Some(now);
            (inner.tunnel.upgrade(), now, timeout_active)
        };
        if !timeout_active {
            return;
        }
        let Some(tunnel) = tunnel else {
            return;
        };
        let timeout = tunnel.drain_timeout();
        let lease = lease.clone();
        let _ = Executor::spawn(async move {
            runtime::sleep(timeout).await;
            let should_retire = {
                let inner = lease.lock().unwrap();
                matches!(
                    (inner.last_drain_progress_at, inner.peer_final_tx_bytes),
                    (Some(last_progress), Some(peer_final))
                        if last_progress == observed_at
                            && inner.peer_write_fin_received
                            && inner.rx_bytes < peer_final
                            && !inner.retired
                )
            };
            if should_retire {
                TcpTunnel::retire_lease_arc(&lease, P2pErrorCode::Interrupted);
            }
        });
    }

    fn reserve_accept_slot(&self, kind: TcpChannelKind) -> Result<(), ClaimConnAckResult> {
        let (sender_closed, pending, limit) = match kind {
            TcpChannelKind::Stream => (
                self.stream_tx.is_closed(),
                &self.stream_accept_pending,
                &self.stream_accept_limit,
            ),
            TcpChannelKind::Datagram => (
                self.datagram_tx.is_closed(),
                &self.datagram_accept_pending,
                &self.datagram_accept_limit,
            ),
        };
        if sender_closed {
            return Err(ClaimConnAckResult::ListenerClosed);
        }
        loop {
            let cur = pending.load(Ordering::SeqCst);
            let max = limit.load(Ordering::SeqCst);
            if cur >= max {
                return Err(ClaimConnAckResult::AcceptQueueFull);
            }
            if pending
                .compare_exchange(cur, cur + 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                return Ok(());
            }
        }
    }

    fn current_listen_vports(&self, kind: TcpChannelKind) -> Option<ListenVPortsRef> {
        match kind {
            TcpChannelKind::Stream => self.stream_vports.read().unwrap().clone(),
            TcpChannelKind::Datagram => self.datagram_vports.read().unwrap().clone(),
        }
    }

    async fn wait_first_listen_if_needed(&self, kind: TcpChannelKind) {
        if self.current_listen_vports(kind).is_some() {
            return;
        }

        let (wait_state, notify) = match kind {
            TcpChannelKind::Stream => (
                &self.stream_first_listen_wait_state,
                &self.stream_vports_notify,
            ),
            TcpChannelKind::Datagram => (
                &self.datagram_first_listen_wait_state,
                &self.datagram_vports_notify,
            ),
        };

        loop {
            match wait_state.load(Ordering::SeqCst) {
                FIRST_LISTEN_WAIT_DONE => return,
                FIRST_LISTEN_WAIT_IN_PROGRESS => {
                    let _ = runtime::timeout(FIRST_LISTEN_WAIT_TIMEOUT, async {
                        loop {
                            if self.is_closed()
                                || self.current_listen_vports(kind).is_some()
                                || wait_state.load(Ordering::SeqCst)
                                    != FIRST_LISTEN_WAIT_IN_PROGRESS
                            {
                                break;
                            }
                            notify.notified().await;
                        }
                    })
                    .await;
                    return;
                }
                FIRST_LISTEN_WAIT_NOT_STARTED => {
                    if wait_state
                        .compare_exchange(
                            FIRST_LISTEN_WAIT_NOT_STARTED,
                            FIRST_LISTEN_WAIT_IN_PROGRESS,
                            Ordering::SeqCst,
                            Ordering::SeqCst,
                        )
                        .is_err()
                    {
                        continue;
                    }

                    let _ = runtime::timeout(FIRST_LISTEN_WAIT_TIMEOUT, async {
                        loop {
                            if self.is_closed() || self.current_listen_vports(kind).is_some() {
                                break;
                            }
                            notify.notified().await;
                        }
                    })
                    .await;
                    wait_state.store(FIRST_LISTEN_WAIT_DONE, Ordering::SeqCst);
                    notify.notify_waiters();
                    return;
                }
                _ => return,
            }
        }
    }

    async fn check_accept_target(
        &self,
        kind: TcpChannelKind,
        purpose: &TunnelPurpose,
    ) -> Result<(), ClaimConnAckResult> {
        let (sender_closed, pending, limit) = match kind {
            TcpChannelKind::Stream => (
                self.stream_tx.is_closed(),
                &self.stream_accept_pending,
                &self.stream_accept_limit,
            ),
            TcpChannelKind::Datagram => (
                self.datagram_tx.is_closed(),
                &self.datagram_accept_pending,
                &self.datagram_accept_limit,
            ),
        };
        if sender_closed {
            return Err(ClaimConnAckResult::ListenerClosed);
        }
        self.wait_first_listen_if_needed(kind).await;
        let vports = self
            .current_listen_vports(kind)
            .ok_or(ClaimConnAckResult::ListenerClosed)?;
        if !vports.is_listen(purpose) {
            return Err(ClaimConnAckResult::VportNotFound);
        }
        if pending.load(Ordering::SeqCst) >= limit.load(Ordering::SeqCst) {
            return Err(ClaimConnAckResult::AcceptQueueFull);
        }
        Ok(())
    }

    fn has_listener_registration(&self, kind: TcpChannelKind) -> bool {
        match kind {
            TcpChannelKind::Stream => self.stream_vports.read().unwrap().is_some(),
            TcpChannelKind::Datagram => self.datagram_vports.read().unwrap().is_some(),
        }
    }

    fn release_accept_slot(&self, kind: TcpChannelKind) {
        let pending = match kind {
            TcpChannelKind::Stream => &self.stream_accept_pending,
            TcpChannelKind::Datagram => &self.datagram_accept_pending,
        };
        pending.fetch_sub(1, Ordering::SeqCst);
    }

    fn start_first_claim_timeout(self: &Arc<Self>, entry: Arc<DataConnEntry>) {
        let this = self.clone();
        let _ = Executor::spawn(async move {
            runtime::sleep(this.heartbeat_timeout).await;
            let should_retire = {
                let inner = entry.inner.lock().unwrap();
                matches!(inner.state, DataConnEntryState::FirstClaimPending)
            };
            if should_retire {
                this.retire_plain_entry(&entry, P2pErrorCode::Timeout);
            }
        });
    }

    fn clear_pending_drains_for_conn(&self, conn_id: TcpConnId) {
        self.state
            .lock()
            .unwrap()
            .pending_drains
            .retain(|(pending_conn_id, _), _| *pending_conn_id != conn_id);
    }

    fn retire_plain_entry(&self, entry: &Arc<DataConnEntry>, reason: P2pErrorCode) {
        let pending_channel = {
            let mut inner = entry.inner.lock().unwrap();
            let pending = match &inner.state {
                DataConnEntryState::Claiming(claiming) => Some(claiming.channel_id),
                _ => None,
            };
            inner.state = DataConnEntryState::Retired;
            pending
        };
        entry.drop_stream();
        self.clear_pending_drains_for_conn(entry.conn_id);
        self.remove_entry(entry.conn_id);
        if let Some(channel_id) = pending_channel {
            self.send_pending_claim_result(channel_id, Err(p2p_err!(reason, "tcp claim retired")));
        }
    }

    fn retire_lease_arc(lease: &Arc<Mutex<LeaseContext>>, reason: P2pErrorCode) {
        let (tunnel, entry, conn_id, channel_id, lease_seq) = {
            let mut inner = lease.lock().unwrap();
            if inner.retired {
                return;
            }
            inner.retired = true;
            inner.read.take();
            inner.write.take();
            (
                inner.tunnel.upgrade(),
                inner.entry.upgrade(),
                inner.conn_id,
                inner.channel_id,
                inner.lease_seq,
            )
        };
        if let Some(entry) = entry {
            if let Some(tunnel) = tunnel {
                {
                    let mut inner = entry.inner.lock().unwrap();
                    inner.state = DataConnEntryState::Retired;
                }
                tunnel.clear_pending_drain_for_entry(conn_id, lease_seq);
                tunnel.remove_entry(conn_id);
                tunnel.send_pending_claim_result(
                    channel_id,
                    Err(p2p_err!(reason, "tcp lease retired")),
                );
            }
        }
    }

    fn maybe_finalize_lease(lease: &Arc<Mutex<LeaseContext>>) {
        let (entry, tunnel, stream_to_idle, should_retire, conn_id) = {
            let mut inner = lease.lock().unwrap();
            let entry = inner.entry.upgrade();
            let tunnel = inner.tunnel.upgrade();
            let conn_id = inner.conn_id;
            if inner.ready_for_idle() {
                if inner.read.is_none() || inner.write.is_none() {
                    return;
                }
                let read = inner.read.take().unwrap();
                let write = inner.write.take().unwrap();
                (entry, tunnel, Some(read.unsplit(write)), false, conn_id)
            } else if inner.ready_for_close() {
                inner.retired = true;
                inner.read.take();
                inner.write.take();
                (entry, tunnel, None, true, conn_id)
            } else {
                return;
            }
        };

        let Some(entry) = entry else {
            return;
        };
        let Some(tunnel) = tunnel else {
            return;
        };

        if should_retire {
            {
                let mut inner = entry.inner.lock().unwrap();
                inner.state = DataConnEntryState::Retired;
            }
            tunnel.remove_entry(conn_id);
            return;
        }

        if let Some(stream) = stream_to_idle {
            entry.put_stream(stream);
            let mut inner = entry.inner.lock().unwrap();
            if matches!(
                inner.state,
                DataConnEntryState::Bound(_) | DataConnEntryState::Draining(_)
            ) {
                inner.state = DataConnEntryState::Idle;
            }
        }
    }

    fn release_read_half(lease: Arc<Mutex<LeaseContext>>) {
        let mut send_read_done = false;
        let mut retire = false;
        {
            let mut inner = lease.lock().unwrap();
            if inner.read_released {
                return;
            }
            inner.read_released = true;
            if inner.peer_write_fin_received {
                if inner.peer_final_tx_bytes == Some(inner.rx_bytes) {
                    if inner.can_send_read_done() {
                        send_read_done = true;
                    }
                } else {
                    retire = true;
                }
            }
        }
        if retire {
            Self::retire_lease_arc(&lease, P2pErrorCode::Interrupted);
        } else if send_read_done {
            Self::spawn_send_read_done(lease.clone());
        }
        Self::maybe_finalize_lease(&lease);
    }

    fn release_write_half(lease: Arc<Mutex<LeaseContext>>) {
        let should_send_fin = {
            let mut inner = lease.lock().unwrap();
            if inner.write_released {
                return;
            }
            inner.write_released = true;
            !inner.local_write_fin_sent
        };
        if should_send_fin {
            Self::spawn_send_write_fin(lease.clone());
        }
        Self::maybe_finalize_lease(&lease);
    }

    fn spawn_send_write_fin(lease: Arc<Mutex<LeaseContext>>) {
        let (tunnel, cmd, should_mark_draining, should_retire) = {
            let mut inner = lease.lock().unwrap();
            if inner.local_write_fin_sent || inner.retired {
                return;
            }
            inner.local_write_fin_sent = true;
            let mut should_retire = false;
            if let Some(pending_final_rx) = inner.pending_peer_read_done_final_rx_bytes.take() {
                if pending_final_rx == inner.tx_bytes {
                    inner.peer_read_done_received = true;
                } else {
                    should_retire = true;
                }
            }
            (
                inner.tunnel.upgrade(),
                WriteFin {
                    channel_id: inner.channel_id,
                    conn_id: inner.conn_id,
                    lease_seq: inner.lease_seq,
                    final_tx_bytes: inner.tx_bytes,
                },
                inner.entry.upgrade(),
                should_retire,
            )
        };
        if let Some(entry) = should_mark_draining {
            let mut entry_inner = entry.inner.lock().unwrap();
            if matches!(entry_inner.state, DataConnEntryState::Bound(_)) {
                entry_inner.state = DataConnEntryState::Draining(lease.clone());
            }
        }
        if should_retire {
            TcpTunnel::retire_lease_arc(&lease, P2pErrorCode::Interrupted);
            return;
        }
        let Some(tunnel) = tunnel else {
            return;
        };
        let _ = Executor::spawn(async move {
            if let Err(err) = tunnel.send_control(&TcpControlCmd::WriteFin(cmd)).await {
                log::warn!(
                    "tcp tunnel {} send WriteFin failed: {:?}",
                    tunnel.tunnel_id.value(),
                    err
                );
                TcpTunnel::retire_lease_arc(&lease, P2pErrorCode::Interrupted);
            } else {
                TcpTunnel::maybe_finalize_lease(&lease);
            }
        });
    }

    fn spawn_send_read_done(lease: Arc<Mutex<LeaseContext>>) {
        let (tunnel, cmd, should_mark_draining) = {
            let mut inner = lease.lock().unwrap();
            if !inner.can_send_read_done() || inner.retired {
                return;
            }
            inner.local_read_done_sent = true;
            (
                inner.tunnel.upgrade(),
                ReadDone {
                    channel_id: inner.channel_id,
                    conn_id: inner.conn_id,
                    lease_seq: inner.lease_seq,
                    final_rx_bytes: inner.rx_bytes,
                },
                inner.entry.upgrade(),
            )
        };
        if let Some(entry) = should_mark_draining {
            let mut entry_inner = entry.inner.lock().unwrap();
            if matches!(entry_inner.state, DataConnEntryState::Bound(_)) {
                entry_inner.state = DataConnEntryState::Draining(lease.clone());
            }
        }
        let Some(tunnel) = tunnel else {
            return;
        };
        let _ = Executor::spawn(async move {
            if let Err(err) = tunnel.send_control(&TcpControlCmd::ReadDone(cmd)).await {
                log::warn!(
                    "tcp tunnel {} send ReadDone failed: {:?}",
                    tunnel.tunnel_id.value(),
                    err
                );
                TcpTunnel::retire_lease_arc(&lease, P2pErrorCode::Interrupted);
            } else {
                TcpTunnel::maybe_finalize_lease(&lease);
            }
        });
    }

    fn spawn_hidden_zero_read_observer(lease: Arc<Mutex<LeaseContext>>) {
        let _ = Executor::spawn(async move {
            loop {
                let mut read = {
                    let mut inner = lease.lock().unwrap();
                    if inner.retired {
                        drop(inner);
                        TcpTunnel::maybe_finalize_lease(&lease);
                        return;
                    }
                    match inner.read.take() {
                        Some(read) => read,
                        None => return,
                    }
                };

                let mut buf = [0u8; 2048];
                let result = runtime::timeout(
                    Duration::from_millis(100),
                    runtime::AsyncReadExt::read(&mut read, &mut buf),
                )
                .await;

                let mut retire = false;
                let mut send_read_done = false;
                let mut refresh_drain_timeout = false;
                {
                    let mut inner = lease.lock().unwrap();
                    inner.read = Some(read);
                    if inner.retired {
                        drop(inner);
                        TcpTunnel::maybe_finalize_lease(&lease);
                        return;
                    }
                    match result {
                        Ok(Ok(0)) => {
                            retire = true;
                        }
                        Ok(Ok(n)) => {
                            inner.rx_bytes += n as u64;
                            retire = n > 0;
                        }
                        Ok(Err(_)) => {
                            retire = true;
                        }
                        Err(_) => {
                            if inner.can_send_read_done() {
                                send_read_done = true;
                            }
                        }
                    }

                    if inner.ready_for_idle() || inner.ready_for_close() {
                        drop(inner);
                        TcpTunnel::maybe_finalize_lease(&lease);
                        return;
                    }

                    if !retire {
                        if let Some(peer_final) = inner.peer_final_tx_bytes {
                            if inner.rx_bytes > peer_final {
                                retire = true;
                            } else if inner.can_send_read_done() {
                                send_read_done = true;
                            } else if inner.peer_write_fin_received && inner.rx_bytes < peer_final {
                                refresh_drain_timeout = true;
                            }
                        }
                    }
                }

                if retire {
                    TcpTunnel::retire_lease_arc(&lease, P2pErrorCode::Interrupted);
                    return;
                }
                if send_read_done {
                    TcpTunnel::spawn_send_read_done(lease.clone());
                    TcpTunnel::maybe_finalize_lease(&lease);
                    return;
                }
                if refresh_drain_timeout {
                    TcpTunnel::note_drain_progress(&lease);
                }
            }
        });
    }

    fn schedule_finalize_check(lease: Arc<Mutex<LeaseContext>>) {
        let _ = Executor::spawn(async move {
            for _ in 0..20 {
                runtime::sleep(Duration::from_millis(100)).await;
                TcpTunnel::maybe_finalize_lease(&lease);
                let done = {
                    let inner = lease.lock().unwrap();
                    if inner.retired {
                        true
                    } else if let Some(entry) = inner.entry.upgrade() {
                        let entry_inner = entry.inner.lock().unwrap();
                        matches!(
                            entry_inner.state,
                            DataConnEntryState::Idle | DataConnEntryState::Retired
                        )
                    } else {
                        true
                    }
                };
                if done {
                    break;
                }
            }
        });
    }

    fn make_stream_channel(
        &self,
        lease: Arc<Mutex<LeaseContext>>,
    ) -> (TunnelStreamRead, TunnelStreamWrite) {
        (
            Box::pin(TcpLeaseRead {
                lease: lease.clone(),
            }),
            Box::pin(TcpLeaseWrite { lease }),
        )
    }

    fn make_datagram_write(&self, lease: Arc<Mutex<LeaseContext>>) -> TunnelDatagramWrite {
        Box::pin(TcpLeaseWrite { lease })
    }

    fn make_datagram_read(&self, lease: Arc<Mutex<LeaseContext>>) -> TunnelDatagramRead {
        Box::pin(TcpLeaseRead { lease })
    }

    fn bind_entry_to_lease(
        self: &Arc<Self>,
        entry: &Arc<DataConnEntry>,
        channel_id: TcpChannelId,
        lease_seq: TcpLeaseSeq,
        kind: TcpChannelKind,
        purpose: TunnelPurpose,
        local_initiator: bool,
    ) -> P2pResult<Arc<Mutex<LeaseContext>>> {
        let stream = entry.take_stream()?;
        let (read, write) = runtime::split(stream);
        let (read_released, write_released, hidden_zero_read, auto_write_fin_zero) =
            match (kind, local_initiator) {
                (TcpChannelKind::Stream, _) => (false, false, false, false),
                (TcpChannelKind::Datagram, true) => (true, false, true, false),
                (TcpChannelKind::Datagram, false) => (false, true, false, true),
            };

        let lease = Arc::new(Mutex::new(LeaseContext {
            tunnel: self.self_weak.lock().unwrap().clone(),
            entry: Arc::downgrade(entry),
            conn_id: entry.conn_id,
            lease_seq,
            channel_id,
            kind,
            purpose,
            local_initiator,
            read: Some(read),
            write: Some(write),
            tx_bytes: 0,
            rx_bytes: 0,
            peer_final_tx_bytes: None,
            pending_peer_read_done_final_rx_bytes: None,
            local_write_fin_sent: false,
            peer_write_fin_received: false,
            local_read_done_sent: false,
            peer_read_done_received: false,
            read_released,
            write_released,
            retired: false,
            no_reuse: false,
            hidden_zero_read,
            last_drain_progress_at: None,
        }));

        let mut retire = false;
        let mut send_read_done = false;
        let pending_fin;
        let pending_read_done;
        let pending_drain;
        {
            let mut inner = entry.inner.lock().unwrap();
            inner.committed_lease_seq = lease_seq;
            inner.state = DataConnEntryState::Bound(lease.clone());
            pending_fin = inner.pending_fin.take();
            pending_read_done = inner.pending_read_done.take();
        }
        pending_drain = self.take_pending_drain(entry.conn_id, lease_seq);
        let should_mark_draining = pending_fin.is_some()
            || pending_read_done.is_some()
            || pending_drain
                .as_ref()
                .map(|pending| pending.write_fin.is_some() || pending.read_done.is_some())
                .unwrap_or(false);

        if let Some(fin) = pending_fin {
            if fin.lease_seq == lease_seq && fin.channel_id == channel_id {
                let mut lease_inner = lease.lock().unwrap();
                if lease_inner.hidden_zero_read && fin.final_tx_bytes != 0 {
                    retire = true;
                }
                lease_inner.peer_write_fin_received = true;
                lease_inner.peer_final_tx_bytes = Some(fin.final_tx_bytes);
                if lease_inner.read_released && lease_inner.rx_bytes != fin.final_tx_bytes {
                    retire = true;
                } else if lease_inner.can_send_read_done() {
                    send_read_done = true;
                }
            } else {
                retire = true;
            }
        }

        if let Some(done) = pending_read_done {
            if done.lease_seq == lease_seq && done.channel_id == channel_id {
                let mut lease_inner = lease.lock().unwrap();
                lease_inner.pending_peer_read_done_final_rx_bytes = Some(done.final_rx_bytes);
            } else {
                retire = true;
            }
        }

        if let Some(pending) = pending_drain {
            if let Some(fin) = pending.write_fin {
                if fin.channel_id != channel_id {
                    retire = true;
                } else {
                    let mut lease_inner = lease.lock().unwrap();
                    if lease_inner.hidden_zero_read && fin.final_tx_bytes != 0 {
                        retire = true;
                    }
                    lease_inner.peer_write_fin_received = true;
                    lease_inner.peer_final_tx_bytes = Some(fin.final_tx_bytes);
                    if lease_inner.read_released && lease_inner.rx_bytes != fin.final_tx_bytes {
                        retire = true;
                    } else if lease_inner.can_send_read_done() {
                        send_read_done = true;
                    }
                }
            }
            if let Some(done) = pending.read_done {
                if done.channel_id != channel_id {
                    retire = true;
                } else {
                    let mut lease_inner = lease.lock().unwrap();
                    lease_inner.pending_peer_read_done_final_rx_bytes = Some(done.final_rx_bytes);
                }
            }
        }

        if should_mark_draining {
            let mut inner = entry.inner.lock().unwrap();
            if matches!(inner.state, DataConnEntryState::Bound(_)) {
                inner.state = DataConnEntryState::Draining(lease.clone());
            }
        }

        if hidden_zero_read {
            Self::spawn_hidden_zero_read_observer(lease.clone());
        }
        if auto_write_fin_zero {
            Self::spawn_send_write_fin(lease.clone());
        }
        if retire {
            Self::retire_lease_arc(&lease, P2pErrorCode::Interrupted);
        } else if send_read_done {
            Self::spawn_send_read_done(lease.clone());
        }
        Self::note_drain_progress(&lease);

        Ok(lease)
    }

    async fn handle_claim_ack(self: &Arc<Self>, ack: ClaimConnAck) -> P2pResult<()> {
        let result = ClaimConnAckResult::try_from(ack.result).map_err(|_| {
            p2p_err!(
                P2pErrorCode::InvalidData,
                "invalid claim ack result: {}",
                ack.result
            )
        })?;
        let has_waiter = self
            .state
            .lock()
            .unwrap()
            .pending_claims
            .contains_key(&ack.channel_id);
        if !has_waiter {
            return Ok(());
        }
        let Some(entry) = self.get_entry(ack.conn_id) else {
            self.send_pending_claim_result(
                ack.channel_id,
                Err(p2p_err!(P2pErrorCode::NotFound, "claim ack conn not found")),
            );
            return Ok(());
        };

        if result != ClaimConnAckResult::Success {
            let should_update = {
                let inner = entry.inner.lock().unwrap();
                matches!(
                    &inner.state,
                    DataConnEntryState::Claiming(claiming)
                        if claiming.channel_id == ack.channel_id && claiming.lease_seq == ack.lease_seq
                )
            };
            if !should_update {
                return Ok(());
            }

            {
                let mut inner = entry.inner.lock().unwrap();
                if matches!(result, ClaimConnAckResult::ConflictLost) {
                    if !matches!(
                        inner.state,
                        DataConnEntryState::Bound(_) | DataConnEntryState::Draining(_)
                    ) {
                        inner.state = if entry.created_by_local
                            && inner.committed_lease_seq == TcpLeaseSeq::default()
                        {
                            DataConnEntryState::FirstClaimPending
                        } else {
                            DataConnEntryState::Idle
                        };
                    }
                } else {
                    inner.state = if entry.created_by_local
                        && inner.committed_lease_seq == TcpLeaseSeq::default()
                    {
                        DataConnEntryState::FirstClaimPending
                    } else {
                        DataConnEntryState::Idle
                    };
                }
            }

            self.send_pending_claim_result(
                ack.channel_id,
                Err(Self::claim_result_to_p2p_error(result)),
            );
            return Ok(());
        }

        let (kind, purpose, channel_id, lease_seq) = {
            let inner = entry.inner.lock().unwrap();
            match &inner.state {
                DataConnEntryState::Claiming(claiming)
                    if claiming.channel_id == ack.channel_id
                        && claiming.lease_seq == ack.lease_seq =>
                {
                    (
                        claiming.kind,
                        claiming.purpose.clone(),
                        claiming.channel_id,
                        claiming.lease_seq,
                    )
                }
                _ => return Ok(()),
            }
        };

        let lease = self.bind_entry_to_lease(&entry, channel_id, lease_seq, kind, purpose, true)?;
        self.send_pending_claim_result(ack.channel_id, Ok(lease));
        Ok(())
    }

    fn handle_write_fin(&self, fin: WriteFin) -> P2pResult<()> {
        let Some(entry) = self.get_entry(fin.conn_id) else {
            return Err(p2p_err!(
                P2pErrorCode::InvalidData,
                "write fin conn not found: {:?}",
                fin.conn_id
            ));
        };
        {
            let mut inner = entry.inner.lock().unwrap();
            match &inner.state {
                DataConnEntryState::FirstClaimPending
                | DataConnEntryState::Idle
                | DataConnEntryState::Retired
                    if fin.lease_seq > inner.committed_lease_seq =>
                {
                    drop(inner);
                    self.retire_plain_entry(&entry, P2pErrorCode::Interrupted);
                    return Ok(());
                }
                _ => {}
            }
            if let DataConnEntryState::Claiming(claiming) = &inner.state {
                if claiming.lease_seq == fin.lease_seq {
                    if claiming.channel_id != fin.channel_id {
                        drop(inner);
                        self.retire_plain_entry(&entry, P2pErrorCode::Interrupted);
                        return Ok(());
                    }
                    if self.insert_pending_write_fin(fin.clone()).is_err() {
                        drop(inner);
                        self.retire_plain_entry(&entry, P2pErrorCode::Interrupted);
                        return Ok(());
                    }
                    match &inner.pending_fin {
                        Some(existing)
                            if existing.channel_id != fin.channel_id
                                || existing.final_tx_bytes != fin.final_tx_bytes =>
                        {
                            drop(inner);
                            self.retire_plain_entry(&entry, P2pErrorCode::Interrupted);
                            return Ok(());
                        }
                        Some(_) => return Ok(()),
                        None => {
                            inner.pending_fin = Some(fin);
                            return Ok(());
                        }
                    }
                }
            }
        }
        let lease = {
            let mut inner = entry.inner.lock().unwrap();
            match &mut inner.state {
                DataConnEntryState::Bound(lease) | DataConnEntryState::Draining(lease) => {
                    lease.clone()
                }
                _ => return Ok(()),
            }
        };

        let mut retire = false;
        let mut send_read_done = false;
        let mut refresh_drain_timeout = false;
        {
            let mut lease_inner = lease.lock().unwrap();
            if lease_inner.lease_seq != fin.lease_seq {
                if fin.lease_seq < lease_inner.lease_seq {
                    return Ok(());
                }
                retire = true;
            } else if lease_inner.channel_id != fin.channel_id {
                retire = true;
            } else if lease_inner.peer_write_fin_received {
                if lease_inner.peer_final_tx_bytes != Some(fin.final_tx_bytes) {
                    retire = true;
                }
            } else {
                if lease_inner.hidden_zero_read && fin.final_tx_bytes != 0 {
                    retire = true;
                }
                lease_inner.peer_write_fin_received = true;
                lease_inner.peer_final_tx_bytes = Some(fin.final_tx_bytes);
                if lease_inner.read_released && lease_inner.rx_bytes != fin.final_tx_bytes {
                    retire = true;
                } else if lease_inner.can_send_read_done() {
                    send_read_done = true;
                } else if lease_inner.rx_bytes < fin.final_tx_bytes {
                    refresh_drain_timeout = true;
                }
            }
        }

        {
            let mut inner = entry.inner.lock().unwrap();
            if matches!(inner.state, DataConnEntryState::Bound(_)) {
                inner.state = DataConnEntryState::Draining(lease.clone());
            }
        }
        if retire {
            Self::retire_lease_arc(&lease, P2pErrorCode::Interrupted);
        } else if send_read_done {
            Self::spawn_send_read_done(lease.clone());
        } else if refresh_drain_timeout {
            Self::note_drain_progress(&lease);
        }
        Self::maybe_finalize_lease(&lease);
        Ok(())
    }

    fn handle_read_done(&self, done: ReadDone) -> P2pResult<()> {
        let Some(entry) = self.get_entry(done.conn_id) else {
            return Err(p2p_err!(
                P2pErrorCode::InvalidData,
                "read done conn not found: {:?}",
                done.conn_id
            ));
        };
        {
            let mut inner = entry.inner.lock().unwrap();
            match &inner.state {
                DataConnEntryState::FirstClaimPending
                | DataConnEntryState::Idle
                | DataConnEntryState::Retired
                    if done.lease_seq > inner.committed_lease_seq =>
                {
                    drop(inner);
                    self.retire_plain_entry(&entry, P2pErrorCode::Interrupted);
                    return Ok(());
                }
                _ => {}
            }
            if let DataConnEntryState::Claiming(claiming) = &inner.state {
                if claiming.lease_seq == done.lease_seq {
                    if claiming.channel_id != done.channel_id {
                        drop(inner);
                        self.retire_plain_entry(&entry, P2pErrorCode::Interrupted);
                        return Ok(());
                    }
                    if self.insert_pending_read_done(done.clone()).is_err() {
                        drop(inner);
                        self.retire_plain_entry(&entry, P2pErrorCode::Interrupted);
                        return Ok(());
                    }
                    match &inner.pending_read_done {
                        Some(existing)
                            if existing.channel_id != done.channel_id
                                || existing.final_rx_bytes != done.final_rx_bytes =>
                        {
                            drop(inner);
                            self.retire_plain_entry(&entry, P2pErrorCode::Interrupted);
                            return Ok(());
                        }
                        Some(_) => return Ok(()),
                        None => {
                            inner.pending_read_done = Some(done);
                            return Ok(());
                        }
                    }
                }
            }
        }
        let lease = {
            let inner = entry.inner.lock().unwrap();
            match &inner.state {
                DataConnEntryState::Bound(lease) | DataConnEntryState::Draining(lease) => {
                    lease.clone()
                }
                _ => return Ok(()),
            }
        };

        let mut retire = false;
        {
            let mut lease_inner = lease.lock().unwrap();
            if lease_inner.lease_seq != done.lease_seq {
                if done.lease_seq < lease_inner.lease_seq {
                    return Ok(());
                }
                retire = true;
            } else if lease_inner.channel_id != done.channel_id {
                retire = true;
            } else if !lease_inner.local_write_fin_sent {
                match lease_inner.pending_peer_read_done_final_rx_bytes {
                    Some(value) if value != done.final_rx_bytes => retire = true,
                    Some(_) => return Ok(()),
                    None => {
                        lease_inner.pending_peer_read_done_final_rx_bytes =
                            Some(done.final_rx_bytes);
                        let mut inner = entry.inner.lock().unwrap();
                        if matches!(inner.state, DataConnEntryState::Bound(_)) {
                            inner.state = DataConnEntryState::Draining(lease.clone());
                        }
                        return Ok(());
                    }
                }
            } else if done.final_rx_bytes != lease_inner.tx_bytes {
                retire = true;
            } else {
                lease_inner.peer_read_done_received = true;
            }
        }

        if retire {
            Self::retire_lease_arc(&lease, P2pErrorCode::Interrupted);
        } else {
            Self::maybe_finalize_lease(&lease);
            Self::schedule_finalize_check(lease);
        }
        Ok(())
    }

    async fn process_claim_req(
        self: &Arc<Self>,
        req: ClaimConnReq,
    ) -> P2pResult<ClaimReqProcessing> {
        let Some(entry) = self.get_entry(req.conn_id) else {
            return Ok(ClaimReqProcessing {
                response: Self::claim_response_cmd(
                    &req,
                    RecentClaimResponse::Err(ClaimConnAckResult::ProtocolError),
                ),
                accepted: None,
                local_conflict_channel: None,
            });
        };

        enum ClaimDecision {
            Replay(RecentClaimResponse),
            Accept,
            Err(ClaimConnAckResult),
            ConflictWin,
            ConflictLose { local_channel_id: TcpChannelId },
        }

        enum ClaimDecisionPlan {
            Ready(ClaimDecision),
            CheckThen(ClaimDecision),
        }

        let decision_plan = {
            let inner = entry.inner.lock().unwrap();
            if let Some(record) = inner.recent_claim.as_ref() {
                if record.lease_seq == req.lease_seq {
                    if Self::claim_record_matches(&req, record) {
                        ClaimDecisionPlan::Ready(ClaimDecision::Replay(record.response))
                    } else {
                        ClaimDecisionPlan::Ready(ClaimDecision::Err(
                            ClaimConnAckResult::ProtocolError,
                        ))
                    }
                } else {
                    match &inner.state {
                        DataConnEntryState::Retired => ClaimDecisionPlan::Ready(
                            ClaimDecision::Err(ClaimConnAckResult::Retired),
                        ),
                        DataConnEntryState::FirstClaimPending => {
                            if req.lease_seq != TcpLeaseSeq::from(1) {
                                ClaimDecisionPlan::Ready(ClaimDecision::Err(
                                    ClaimConnAckResult::LeaseMismatch,
                                ))
                            } else if entry.created_by_local {
                                ClaimDecisionPlan::Ready(ClaimDecision::Err(
                                    ClaimConnAckResult::ProtocolError,
                                ))
                            } else {
                                ClaimDecisionPlan::CheckThen(ClaimDecision::Accept)
                            }
                        }
                        DataConnEntryState::Idle => {
                            if req.lease_seq != Self::next_lease_seq(inner.committed_lease_seq) {
                                ClaimDecisionPlan::Ready(ClaimDecision::Err(
                                    ClaimConnAckResult::LeaseMismatch,
                                ))
                            } else {
                                ClaimDecisionPlan::CheckThen(ClaimDecision::Accept)
                            }
                        }
                        DataConnEntryState::Claiming(claiming) => {
                            if req.lease_seq != claiming.lease_seq {
                                ClaimDecisionPlan::Ready(ClaimDecision::Err(
                                    ClaimConnAckResult::LeaseMismatch,
                                ))
                            } else {
                                let local_wins = if claiming.claim_nonce == req.claim_nonce {
                                    self.local_id.as_slice() > self.remote_id.as_slice()
                                } else {
                                    claiming.claim_nonce > req.claim_nonce
                                };
                                if local_wins {
                                    ClaimDecisionPlan::CheckThen(ClaimDecision::ConflictWin)
                                } else {
                                    ClaimDecisionPlan::CheckThen(ClaimDecision::ConflictLose {
                                        local_channel_id: claiming.channel_id,
                                    })
                                }
                            }
                        }
                        DataConnEntryState::Bound(_) | DataConnEntryState::Draining(_) => {
                            ClaimDecisionPlan::Ready(ClaimDecision::Err(
                                ClaimConnAckResult::NotIdle,
                            ))
                        }
                    }
                }
            } else {
                match &inner.state {
                    DataConnEntryState::Retired => {
                        ClaimDecisionPlan::Ready(ClaimDecision::Err(ClaimConnAckResult::Retired))
                    }
                    DataConnEntryState::FirstClaimPending => {
                        if req.lease_seq != TcpLeaseSeq::from(1) {
                            ClaimDecisionPlan::Ready(ClaimDecision::Err(
                                ClaimConnAckResult::LeaseMismatch,
                            ))
                        } else if entry.created_by_local {
                            ClaimDecisionPlan::Ready(ClaimDecision::Err(
                                ClaimConnAckResult::ProtocolError,
                            ))
                        } else {
                            ClaimDecisionPlan::CheckThen(ClaimDecision::Accept)
                        }
                    }
                    DataConnEntryState::Idle => {
                        if req.lease_seq != Self::next_lease_seq(inner.committed_lease_seq) {
                            ClaimDecisionPlan::Ready(ClaimDecision::Err(
                                ClaimConnAckResult::LeaseMismatch,
                            ))
                        } else {
                            ClaimDecisionPlan::CheckThen(ClaimDecision::Accept)
                        }
                    }
                    DataConnEntryState::Claiming(claiming) => {
                        if req.lease_seq != claiming.lease_seq {
                            ClaimDecisionPlan::Ready(ClaimDecision::Err(
                                ClaimConnAckResult::LeaseMismatch,
                            ))
                        } else {
                            let local_wins = if claiming.claim_nonce == req.claim_nonce {
                                self.local_id.as_slice() > self.remote_id.as_slice()
                            } else {
                                claiming.claim_nonce > req.claim_nonce
                            };
                            if local_wins {
                                ClaimDecisionPlan::CheckThen(ClaimDecision::ConflictWin)
                            } else {
                                ClaimDecisionPlan::CheckThen(ClaimDecision::ConflictLose {
                                    local_channel_id: claiming.channel_id,
                                })
                            }
                        }
                    }
                    DataConnEntryState::Bound(_) | DataConnEntryState::Draining(_) => {
                        ClaimDecisionPlan::Ready(ClaimDecision::Err(ClaimConnAckResult::NotIdle))
                    }
                }
            }
        };

        let decision = match decision_plan {
            ClaimDecisionPlan::Ready(decision) => decision,
            ClaimDecisionPlan::CheckThen(on_success) => {
                match self.check_accept_target(req.kind, &req.purpose).await {
                    Ok(()) => on_success,
                    Err(reason) => ClaimDecision::Err(reason),
                }
            }
        };

        let make_record = |response| RecentClaimRecord {
            channel_id: req.channel_id,
            lease_seq: req.lease_seq,
            claim_nonce: req.claim_nonce,
            kind: req.kind,
            purpose: req.purpose.clone(),
            response,
        };

        match decision {
            ClaimDecision::Replay(response) => Ok(ClaimReqProcessing {
                response: Self::claim_response_cmd(&req, response),
                accepted: None,
                local_conflict_channel: None,
            }),
            ClaimDecision::Err(result) => {
                entry.inner.lock().unwrap().recent_claim =
                    Some(make_record(RecentClaimResponse::Err(result)));
                Ok(ClaimReqProcessing {
                    response: Self::claim_response_cmd(&req, RecentClaimResponse::Err(result)),
                    accepted: None,
                    local_conflict_channel: None,
                })
            }
            ClaimDecision::ConflictWin => {
                entry.inner.lock().unwrap().recent_claim = Some(make_record(
                    RecentClaimResponse::Err(ClaimConnAckResult::ConflictLost),
                ));
                Ok(ClaimReqProcessing {
                    response: Self::claim_response_cmd(
                        &req,
                        RecentClaimResponse::Err(ClaimConnAckResult::ConflictLost),
                    ),
                    accepted: None,
                    local_conflict_channel: None,
                })
            }
            ClaimDecision::ConflictLose { local_channel_id } => {
                if let Err(reason) = self.reserve_accept_slot(req.kind) {
                    entry.inner.lock().unwrap().recent_claim =
                        Some(make_record(RecentClaimResponse::Err(reason)));
                    return Ok(ClaimReqProcessing {
                        response: Self::claim_response_cmd(&req, RecentClaimResponse::Err(reason)),
                        accepted: None,
                        local_conflict_channel: Some(local_channel_id),
                    });
                }
                let lease = self.bind_entry_to_lease(
                    &entry,
                    req.channel_id,
                    req.lease_seq,
                    req.kind,
                    req.purpose.clone(),
                    false,
                )?;
                entry.inner.lock().unwrap().recent_claim =
                    Some(make_record(RecentClaimResponse::Ack));
                Ok(ClaimReqProcessing {
                    response: Self::claim_response_cmd(&req, RecentClaimResponse::Ack),
                    accepted: Some((
                        req.kind,
                        AcceptedChannel {
                            purpose: req.purpose.clone(),
                            lease,
                        },
                    )),
                    local_conflict_channel: Some(local_channel_id),
                })
            }
            ClaimDecision::Accept => {
                if let Err(reason) = self.reserve_accept_slot(req.kind) {
                    entry.inner.lock().unwrap().recent_claim =
                        Some(make_record(RecentClaimResponse::Err(reason)));
                    return Ok(ClaimReqProcessing {
                        response: Self::claim_response_cmd(&req, RecentClaimResponse::Err(reason)),
                        accepted: None,
                        local_conflict_channel: None,
                    });
                }
                let lease = self.bind_entry_to_lease(
                    &entry,
                    req.channel_id,
                    req.lease_seq,
                    req.kind,
                    req.purpose.clone(),
                    false,
                )?;
                entry.inner.lock().unwrap().recent_claim =
                    Some(make_record(RecentClaimResponse::Ack));
                Ok(ClaimReqProcessing {
                    response: Self::claim_response_cmd(&req, RecentClaimResponse::Ack),
                    accepted: Some((
                        req.kind,
                        AcceptedChannel {
                            purpose: req.purpose.clone(),
                            lease,
                        },
                    )),
                    local_conflict_channel: None,
                })
            }
        }
    }

    async fn handle_claim_req(self: &Arc<Self>, req: ClaimConnReq) -> P2pResult<()> {
        #[cfg(test)]
        {
            let delay = *self.claim_req_delay.lock().unwrap();
            if !delay.is_zero() {
                runtime::sleep(delay).await;
            }
        }
        let processing = self.process_claim_req(req).await?;
        if let Some(local_channel_id) = processing.local_conflict_channel {
            self.send_pending_claim_result(
                local_channel_id,
                Err(p2p_err!(P2pErrorCode::Conflict, "claim conflict lost")),
            );
        }
        if let Some((kind, accepted)) = processing.accepted {
            self.enqueue_accepted(kind, accepted)?;
        }
        self.send_control(&processing.response).await
    }

    #[cfg(test)]
    pub(crate) fn first_conn_id_for_test(&self) -> Option<TcpConnId> {
        self.state.lock().unwrap().data_conns.keys().copied().next()
    }

    #[cfg(test)]
    pub(crate) fn current_lease_for_test(
        &self,
        conn_id: TcpConnId,
    ) -> Option<(TcpChannelId, TcpLeaseSeq, u64, u64)> {
        let entry = self.get_entry(conn_id)?;
        let inner = entry.inner.lock().unwrap();
        let lease = match &inner.state {
            DataConnEntryState::Bound(lease) | DataConnEntryState::Draining(lease) => lease.clone(),
            _ => return None,
        };
        let lease = lease.lock().unwrap();
        Some((
            lease.channel_id,
            lease.lease_seq,
            lease.tx_bytes,
            lease.rx_bytes,
        ))
    }

    #[cfg(test)]
    pub(crate) fn current_claiming_for_test(
        &self,
        conn_id: TcpConnId,
    ) -> Option<(
        TcpChannelId,
        TcpLeaseSeq,
        u64,
        TcpChannelKind,
        TunnelPurpose,
    )> {
        let entry = self.get_entry(conn_id)?;
        let inner = entry.inner.lock().unwrap();
        match &inner.state {
            DataConnEntryState::Claiming(claiming) => Some((
                claiming.channel_id,
                claiming.lease_seq,
                claiming.claim_nonce,
                claiming.kind,
                claiming.purpose.clone(),
            )),
            _ => None,
        }
    }

    #[cfg(test)]
    pub(crate) async fn create_data_connection_for_test(self: &Arc<Self>) -> P2pResult<TcpConnId> {
        let entry = self.create_data_connection(None).await?;
        Ok(entry.conn_id)
    }

    #[cfg(test)]
    pub(crate) fn start_claim_for_test(
        &self,
        conn_id: TcpConnId,
        kind: TcpChannelKind,
        purpose: TunnelPurpose,
        claim_nonce: u64,
    ) -> P2pResult<(TcpChannelId, TcpLeaseSeq)> {
        let entry = self
            .get_entry(conn_id)
            .ok_or_else(|| p2p_err!(P2pErrorCode::NotFound, "conn not found"))?;
        let channel_id = self.next_local_channel_id();
        let lease_seq = {
            let mut inner = entry.inner.lock().unwrap();
            let expected_next = Self::next_lease_seq(inner.committed_lease_seq);
            match &inner.state {
                DataConnEntryState::FirstClaimPending if entry.created_by_local => {
                    inner.state = DataConnEntryState::Claiming(ClaimingState {
                        lease_seq: TcpLeaseSeq::from(1),
                        channel_id,
                        claim_nonce,
                        kind,
                        purpose: purpose.clone(),
                    });
                    TcpLeaseSeq::from(1)
                }
                DataConnEntryState::Idle => {
                    inner.state = DataConnEntryState::Claiming(ClaimingState {
                        lease_seq: expected_next,
                        channel_id,
                        claim_nonce,
                        kind,
                        purpose,
                    });
                    expected_next
                }
                _ => {
                    return Err(p2p_err!(
                        P2pErrorCode::ErrorState,
                        "connection not claimable"
                    ));
                }
            }
        };
        Ok((channel_id, lease_seq))
    }

    #[cfg(test)]
    pub(crate) fn register_pending_claim_for_test(
        &self,
        channel_id: TcpChannelId,
    ) -> oneshot::Receiver<P2pResult<()>> {
        let (tx, rx) = oneshot::channel();
        let (inner_tx, inner_rx) = oneshot::channel();
        self.state
            .lock()
            .unwrap()
            .pending_claims
            .insert(channel_id, inner_tx);
        let _ = Executor::spawn(async move {
            let mapped = match inner_rx.await {
                Ok(Ok(_)) => Ok(()),
                Ok(Err(err)) => Err(err),
                Err(_) => Err(p2p_err!(
                    P2pErrorCode::Interrupted,
                    "test pending claim waiter dropped"
                )),
            };
            let _ = tx.send(mapped);
        });
        rx
    }

    #[cfg(test)]
    pub(crate) async fn simulate_claim_req_for_test(
        self: &Arc<Self>,
        req: ClaimConnReq,
    ) -> P2pResult<TcpControlCmd> {
        let processing = self.process_claim_req(req).await?;
        if let Some(local_channel_id) = processing.local_conflict_channel {
            self.send_pending_claim_result(
                local_channel_id,
                Err(p2p_err!(P2pErrorCode::Conflict, "claim conflict lost")),
            );
        }
        if let Some((kind, accepted)) = processing.accepted {
            self.enqueue_accepted(kind, accepted)?;
        }
        Ok(processing.response)
    }

    #[cfg(test)]
    pub(crate) fn simulate_write_fin_for_test(&self, fin: WriteFin) {
        self.handle_write_fin(fin).unwrap();
    }

    #[cfg(test)]
    pub(crate) fn simulate_read_done_for_test(&self, done: ReadDone) {
        self.handle_read_done(done).unwrap();
    }

    #[cfg(test)]
    pub(crate) async fn simulate_control_cmd_for_test(
        self: &Arc<Self>,
        cmd: TcpControlCmd,
    ) -> P2pResult<()> {
        if let Err(err) = self.handle_control_cmd(cmd).await {
            let _ = self
                .close_impl(LocalTunnelPhase::Error, P2pErrorCode::Interrupted)
                .await;
            return Err(err);
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) async fn write_hidden_bytes_for_test(
        &self,
        conn_id: TcpConnId,
        data: &[u8],
    ) -> P2pResult<()> {
        let entry = self
            .get_entry(conn_id)
            .ok_or_else(|| p2p_err!(P2pErrorCode::NotFound, "conn not found"))?;
        let lease = {
            let inner = entry.inner.lock().unwrap();
            match &inner.state {
                DataConnEntryState::Bound(lease) | DataConnEntryState::Draining(lease) => {
                    lease.clone()
                }
                _ => return Err(p2p_err!(P2pErrorCode::ErrorState, "conn not bound")),
            }
        };
        let mut taken = {
            let mut lease_inner = lease.lock().unwrap();
            let write = lease_inner
                .write
                .take()
                .ok_or_else(|| p2p_err!(P2pErrorCode::ErrorState, "hidden write missing"))?;
            (write, lease_inner.tx_bytes)
        };
        runtime::AsyncWriteExt::write_all(&mut taken.0, data)
            .await
            .map_err(|_| p2p_err!(P2pErrorCode::IoError, "hidden write failed"))?;
        runtime::AsyncWriteExt::flush(&mut taken.0)
            .await
            .map_err(|_| p2p_err!(P2pErrorCode::IoError, "hidden flush failed"))?;
        let mut lease_inner = lease.lock().unwrap();
        lease_inner.tx_bytes = taken.1 + data.len() as u64;
        lease_inner.write = Some(taken.0);
        Ok(())
    }

    #[cfg(test)]
    pub(crate) async fn simulate_claim_ack_for_test(
        self: &Arc<Self>,
        ack: ClaimConnAck,
    ) -> P2pResult<()> {
        self.handle_claim_ack(ack).await
    }

    #[cfg(test)]
    pub(crate) async fn simulate_claim_error_for_test(
        self: &Arc<Self>,
        channel_id: TcpChannelId,
        conn_id: TcpConnId,
        lease_seq: TcpLeaseSeq,
        result: ClaimConnAckResult,
    ) -> P2pResult<()> {
        self.handle_claim_ack(ClaimConnAck {
            channel_id,
            conn_id,
            lease_seq,
            result: result as u8,
        })
        .await
    }

    #[cfg(test)]
    pub(crate) async fn close_accept_queue_for_test(&self, kind: TcpChannelKind) {
        match kind {
            TcpChannelKind::Stream => self.stream_rx.lock().await.close(),
            TcpChannelKind::Datagram => self.datagram_rx.lock().await.close(),
        }
    }

    fn enqueue_accepted(&self, kind: TcpChannelKind, accepted: AcceptedChannel) -> P2pResult<()> {
        match kind {
            TcpChannelKind::Stream => self.stream_tx.send(Ok(accepted)).map_err(|_| {
                self.release_accept_slot(TcpChannelKind::Stream);
                p2p_err!(P2pErrorCode::Interrupted, "stream accept queue closed")
            }),
            TcpChannelKind::Datagram => self.datagram_tx.send(Ok(accepted)).map_err(|_| {
                self.release_accept_slot(TcpChannelKind::Datagram);
                p2p_err!(P2pErrorCode::Interrupted, "datagram accept queue closed")
            }),
        }
    }

    fn find_claimable_entry(&self) -> Option<Arc<DataConnEntry>> {
        let entries = self
            .state
            .lock()
            .unwrap()
            .data_conns
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for entry in entries {
            let inner = entry.inner.lock().unwrap();
            match &inner.state {
                DataConnEntryState::FirstClaimPending if entry.created_by_local => {
                    return Some(entry.clone());
                }
                DataConnEntryState::Idle => return Some(entry.clone()),
                _ => {}
            }
        }
        None
    }

    fn unclaimed_entry_count(&self) -> usize {
        let entries = self
            .state
            .lock()
            .unwrap()
            .data_conns
            .values()
            .cloned()
            .collect::<Vec<_>>();
        entries
            .into_iter()
            .filter(|entry| {
                matches!(
                    entry.inner.lock().unwrap().state,
                    DataConnEntryState::FirstClaimPending | DataConnEntryState::Idle
                )
            })
            .count()
    }

    fn allow_late_open_request_connection(&self) -> bool {
        self.unclaimed_entry_count()
            < self.unclaimed_data_conn_budget.load(Ordering::SeqCst) as usize
    }

    #[cfg(test)]
    pub(crate) fn data_conn_count_for_test(&self) -> usize {
        self.state.lock().unwrap().data_conns.len()
    }

    #[cfg(test)]
    pub(crate) fn pending_drain_count_for_test(&self) -> usize {
        self.state.lock().unwrap().pending_drains.len()
    }

    async fn create_data_connection(
        self: &Arc<Self>,
        open_request_id: Option<TcpRequestId>,
    ) -> P2pResult<Arc<DataConnEntry>> {
        let conn_id = self.next_local_conn_id();
        let mut connection = self
            .connector
            .open_data_connection(conn_id, open_request_id)
            .await?;
        let ready = runtime::timeout(
            self.connector.timeout,
            read_raw_frame::<_, DataConnReady>(&mut connection.stream),
        )
        .await
        .map_err(|_| p2p_err!(P2pErrorCode::Timeout, "wait data conn ready timeout"))??;
        if ready.conn_id != conn_id || ready.candidate_id != self.candidate_id {
            return Err(p2p_err!(
                P2pErrorCode::InvalidData,
                "data ready tunnel candidate mismatch"
            ));
        }
        if ready.result != DataConnReadyResult::Success {
            return Err(p2p_err!(
                P2pErrorCode::Reject,
                "data conn ready failed: {:?}",
                ready.result
            ));
        }
        match runtime::timeout(
            Duration::ZERO,
            read_raw_frame::<_, DataConnReady>(&mut connection.stream),
        )
        .await
        {
            Ok(Ok(_)) => {
                return Err(p2p_err!(
                    P2pErrorCode::InvalidData,
                    "duplicate data conn ready"
                ));
            }
            Ok(Err(err)) => {
                return Err(p2p_err!(
                    P2pErrorCode::InvalidData,
                    "unexpected bytes after data conn ready: {:?}",
                    err
                ));
            }
            Err(_) => {}
        }

        let entry = DataConnEntry::new(connection, conn_id, true);
        self.register_entry(entry.clone());
        self.start_first_claim_timeout(entry.clone());
        Ok(entry)
    }

    fn spawn_reverse_data_connection(self: &Arc<Self>, request_id: TcpRequestId) {
        let this = self.clone();
        let _ = Executor::spawn(async move {
            #[cfg(test)]
            {
                let delay = *this.reverse_open_delay.lock().unwrap();
                if !delay.is_zero() {
                    runtime::sleep(delay).await;
                }
            }
            if let Err(err) = this.create_data_connection(Some(request_id)).await {
                log::warn!(
                    "tcp tunnel {} reverse data connection failed: {:?}",
                    this.tunnel_id.value(),
                    err
                );
            }
        });
    }

    async fn request_remote_data_connection(self: &Arc<Self>) -> P2pResult<Arc<DataConnEntry>> {
        let request_id = self.next_local_request_id();
        let (tx, rx) = oneshot::channel();
        self.state
            .lock()
            .unwrap()
            .pending_open_requests
            .insert(request_id, tx);

        if let Err(err) = self
            .send_control(&TcpControlCmd::OpenDataConnReq(OpenDataConnReq {
                request_id,
            }))
            .await
        {
            self.state
                .lock()
                .unwrap()
                .pending_open_requests
                .remove(&request_id);
            return Err(err);
        }

        match runtime::timeout(self.connector.timeout, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(p2p_err!(
                P2pErrorCode::Interrupted,
                "reverse data connection waiter dropped"
            )),
            Err(_) => {
                self.state
                    .lock()
                    .unwrap()
                    .pending_open_requests
                    .remove(&request_id);
                Err(p2p_err!(
                    P2pErrorCode::Timeout,
                    "wait reverse data connection timeout"
                ))
            }
        }
    }

    #[cfg(test)]
    pub(crate) async fn request_reverse_data_connection_for_test(
        self: &Arc<Self>,
    ) -> P2pResult<()> {
        let _ = self.request_remote_data_connection().await?;
        Ok(())
    }

    pub(crate) async fn on_incoming_data_connection(
        self: &Arc<Self>,
        hello: TcpConnectionHello,
        mut connection: TcpTlsConnection,
    ) -> P2pResult<()> {
        self.promote_connected();
        if hello.role != TcpConnectionRole::Data || hello.conn_id.is_none() {
            write_raw_frame(
                &mut connection.stream,
                &DataConnReady {
                    conn_id: hello.conn_id.unwrap_or_default(),
                    candidate_id: hello.candidate_id,
                    result: DataConnReadyResult::ProtocolError,
                },
            )
            .await?;
            return Err(p2p_err!(P2pErrorCode::InvalidParam, "invalid data hello"));
        }
        let conn_id = hello.conn_id.unwrap();
        if self.get_entry(conn_id).is_some() {
            write_raw_frame(
                &mut connection.stream,
                &DataConnReady {
                    conn_id,
                    candidate_id: hello.candidate_id,
                    result: DataConnReadyResult::ConnIdConflict,
                },
            )
            .await?;
            return Err(p2p_err!(P2pErrorCode::Conflict, "conn_id conflict"));
        }

        if let Some(request_id) = hello.open_request_id {
            let has_pending_request = self
                .state
                .lock()
                .unwrap()
                .pending_open_requests
                .contains_key(&request_id);
            if !has_pending_request && !self.allow_late_open_request_connection() {
                write_raw_frame(
                    &mut connection.stream,
                    &DataConnReady {
                        conn_id,
                        candidate_id: hello.candidate_id,
                        result: DataConnReadyResult::InternalError,
                    },
                )
                .await?;
                return Err(p2p_err!(
                    P2pErrorCode::OutOfLimit,
                    "late reverse data connection over budget"
                ));
            }
        }

        write_raw_frame(
            &mut connection.stream,
            &DataConnReady {
                conn_id,
                candidate_id: hello.candidate_id,
                result: DataConnReadyResult::Success,
            },
        )
        .await?;

        let entry = DataConnEntry::new(connection, conn_id, self.is_local_conn_creator(conn_id));
        self.register_entry(entry.clone());
        self.start_first_claim_timeout(entry.clone());
        if let Some(request_id) = hello.open_request_id {
            self.send_pending_open_request_result(request_id, Ok(entry));
        }
        Ok(())
    }

    async fn claim_entry(
        self: &Arc<Self>,
        entry: Arc<DataConnEntry>,
        kind: TcpChannelKind,
        purpose: TunnelPurpose,
    ) -> P2pResult<Arc<Mutex<LeaseContext>>> {
        let channel_id = self.next_local_channel_id();
        let (lease_seq, claim_nonce) = {
            let mut inner = entry.inner.lock().unwrap();
            let expected_next = Self::next_lease_seq(inner.committed_lease_seq);
            match &inner.state {
                DataConnEntryState::FirstClaimPending if entry.created_by_local => {
                    let claim_nonce = random::<u64>();
                    inner.state = DataConnEntryState::Claiming(ClaimingState {
                        lease_seq: TcpLeaseSeq::from(1),
                        channel_id,
                        claim_nonce,
                        kind,
                        purpose: purpose.clone(),
                    });
                    (TcpLeaseSeq::from(1), claim_nonce)
                }
                DataConnEntryState::Idle => {
                    let claim_nonce = random::<u64>();
                    inner.state = DataConnEntryState::Claiming(ClaimingState {
                        lease_seq: expected_next,
                        channel_id,
                        claim_nonce,
                        kind,
                        purpose: purpose.clone(),
                    });
                    (expected_next, claim_nonce)
                }
                _ => {
                    return Err(p2p_err!(
                        P2pErrorCode::ErrorState,
                        "connection not claimable"
                    ));
                }
            }
        };

        let (tx, rx) = oneshot::channel();
        self.state
            .lock()
            .unwrap()
            .pending_claims
            .insert(channel_id, tx);
        if let Err(err) = self
            .send_control(&TcpControlCmd::ClaimConnReq(ClaimConnReq {
                channel_id,
                kind,
                purpose,
                conn_id: entry.conn_id,
                lease_seq,
                claim_nonce,
            }))
            .await
        {
            self.state
                .lock()
                .unwrap()
                .pending_claims
                .remove(&channel_id);
            {
                let mut inner = entry.inner.lock().unwrap();
                inner.state = if entry.created_by_local
                    && inner.committed_lease_seq == TcpLeaseSeq::default()
                {
                    DataConnEntryState::FirstClaimPending
                } else {
                    DataConnEntryState::Idle
                };
            }
            return Err(err);
        }

        match runtime::timeout(self.heartbeat_timeout, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(p2p_err!(P2pErrorCode::Interrupted, "claim waiter dropped")),
            Err(_) => {
                self.state
                    .lock()
                    .unwrap()
                    .pending_claims
                    .remove(&channel_id);
                self.retire_plain_entry(&entry, P2pErrorCode::Timeout);
                Err(p2p_err!(P2pErrorCode::Timeout, "claim timeout"))
            }
        }
    }

    async fn open_channel(
        self: &Arc<Self>,
        kind: TcpChannelKind,
        purpose: TunnelPurpose,
    ) -> P2pResult<Arc<Mutex<LeaseContext>>> {
        self.wait_until_connected().await?;
        for _ in 0..4 {
            let entry = match self.find_claimable_entry() {
                Some(entry) => entry,
                None => match self.create_data_connection(None).await {
                    Ok(entry) => entry,
                    Err(_) => self.request_remote_data_connection().await?,
                },
            };
            match self.claim_entry(entry, kind, purpose.clone()).await {
                Ok(lease) => return Ok(lease),
                Err(err) if err.code() == P2pErrorCode::Conflict => continue,
                Err(err) => return Err(err),
            }
        }
        Err(p2p_err!(P2pErrorCode::Conflict, "claim retries exhausted"))
    }

    async fn heartbeat_loop(self: Arc<Self>) {
        loop {
            runtime::sleep(self.heartbeat_interval).await;
            if self.closed.load(Ordering::SeqCst) {
                break;
            }
            let phase = self.state.lock().unwrap().phase;
            if phase != LocalTunnelPhase::Connected {
                continue;
            }

            let should_ping = {
                let state = self.state.lock().unwrap();
                state.last_recv_at.elapsed() >= self.heartbeat_interval
            };
            if should_ping {
                #[cfg(test)]
                if self.suppress_ping.load(Ordering::SeqCst) {
                    continue;
                }
                if let Err(err) = self.send_ping().await {
                    log::warn!(
                        "tcp tunnel {} heartbeat send failed: {:?}",
                        self.tunnel_id.value(),
                        err
                    );
                    let _ = self
                        .close_impl(LocalTunnelPhase::Error, P2pErrorCode::Interrupted)
                        .await;
                    break;
                }
            }
            let timed_out = {
                let state = self.state.lock().unwrap();
                state.last_pong_at.elapsed() >= self.heartbeat_timeout
                    && state.last_recv_at.elapsed() >= self.heartbeat_timeout
            };
            if timed_out {
                let _ = self
                    .close_impl(LocalTunnelPhase::Error, P2pErrorCode::Timeout)
                    .await;
                break;
            }
        }
    }
}

#[async_trait::async_trait]
impl Tunnel for TcpTunnel {
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

    fn protocol(&self) -> crate::endpoint::Protocol {
        crate::endpoint::Protocol::Tcp
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
        Self::external_state(self.state.lock().unwrap().phase)
    }

    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    async fn close(&self) -> P2pResult<()> {
        self.close_impl(LocalTunnelPhase::Closed, P2pErrorCode::Interrupted)
            .await
    }

    async fn listen_stream(&self, vports: ListenVPortsRef) -> P2pResult<()> {
        *self.stream_vports.write().unwrap() = Some(vports);
        self.stream_vports_notify.notify_waiters();
        Ok(())
    }

    async fn listen_datagram(&self, vports: ListenVPortsRef) -> P2pResult<()> {
        *self.datagram_vports.write().unwrap() = Some(vports);
        self.datagram_vports_notify.notify_waiters();
        Ok(())
    }

    async fn open_stream(
        &self,
        purpose: TunnelPurpose,
    ) -> P2pResult<(TunnelStreamRead, TunnelStreamWrite)> {
        if self.is_closed() {
            return Err(p2p_err!(P2pErrorCode::ErrorState, "tunnel closed"));
        }
        let this = self
            .self_weak
            .lock()
            .unwrap()
            .upgrade()
            .ok_or_else(|| p2p_err!(P2pErrorCode::ErrorState, "tunnel dropped"))?;
        let lease = this.open_channel(TcpChannelKind::Stream, purpose).await?;
        Ok(self.make_stream_channel(lease))
    }

    async fn accept_stream(
        &self,
    ) -> P2pResult<(TunnelPurpose, TunnelStreamRead, TunnelStreamWrite)> {
        if self.is_closed() {
            return Err(p2p_err!(P2pErrorCode::ErrorState, "tunnel closed"));
        }
        if !self.has_listener_registration(TcpChannelKind::Stream) {
            return Err(p2p_err!(
                P2pErrorCode::Interrupted,
                "tcp accept requires listen before accept"
            ));
        }
        let mut rx = self.stream_rx.lock().await;
        let accepted = rx
            .recv()
            .await
            .ok_or_else(|| p2p_err!(P2pErrorCode::Interrupted, "stream accept queue closed"))??;
        self.release_accept_slot(TcpChannelKind::Stream);
        let (read, write) = self.make_stream_channel(accepted.lease);
        Ok((accepted.purpose, read, write))
    }

    async fn open_datagram(&self, purpose: TunnelPurpose) -> P2pResult<TunnelDatagramWrite> {
        if self.is_closed() {
            return Err(p2p_err!(P2pErrorCode::ErrorState, "tunnel closed"));
        }
        let this = self
            .self_weak
            .lock()
            .unwrap()
            .upgrade()
            .ok_or_else(|| p2p_err!(P2pErrorCode::ErrorState, "tunnel dropped"))?;
        let lease = this.open_channel(TcpChannelKind::Datagram, purpose).await?;
        Ok(self.make_datagram_write(lease))
    }

    async fn accept_datagram(&self) -> P2pResult<(TunnelPurpose, TunnelDatagramRead)> {
        if self.is_closed() {
            return Err(p2p_err!(P2pErrorCode::ErrorState, "tunnel closed"));
        }
        if !self.has_listener_registration(TcpChannelKind::Datagram) {
            return Err(p2p_err!(
                P2pErrorCode::Interrupted,
                "tcp accept requires listen before accept"
            ));
        }
        let mut rx = self.datagram_rx.lock().await;
        let accepted = rx
            .recv()
            .await
            .ok_or_else(|| p2p_err!(P2pErrorCode::Interrupted, "datagram accept queue closed"))??;
        self.release_accept_slot(TcpChannelKind::Datagram);
        Ok((accepted.purpose, self.make_datagram_read(accepted.lease)))
    }
}

impl Drop for TcpTunnel {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::SeqCst);
        let mut state = self.state.lock().unwrap();
        if matches!(
            state.phase,
            LocalTunnelPhase::Connected | LocalTunnelPhase::PassiveReady
        ) {
            state.phase = LocalTunnelPhase::Closed;
        }
        state.pending_open_requests.clear();
        state.pending_claims.clear();
        state.data_conns.clear();
    }
}
