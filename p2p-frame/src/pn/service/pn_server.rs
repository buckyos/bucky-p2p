use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::io;
use std::num::NonZeroU32;
use std::ops::Bound::{Excluded, Unbounded};
use std::pin::Pin;
use std::sync::{
    Arc, Mutex, Weak,
    atomic::{self, AtomicBool},
};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use sfo_io::{
    LimitStream, SfoSpeedStat, SpeedLimitSession, SpeedLimiter, SpeedLimiterRef, SpeedStat,
    SpeedTracker, StatStream,
};
use tokio::{
    io::{ReadBuf, copy_bidirectional},
    sync::Notify,
};

use crate::endpoint::Endpoint;
use crate::error::{P2pError, P2pErrorCode, P2pResult, into_p2p_err, p2p_err};
use crate::executor::Executor;
use crate::networks::{
    TunnelCommand, TunnelCommandBody, TunnelCommandResult, TunnelPurpose, TunnelStreamRead,
    TunnelStreamWrite, ValidateResult, read_tunnel_command_body, read_tunnel_command_header,
    write_tunnel_command,
};
use crate::p2p_identity::P2pId;
use crate::pn::{
    PROXY_SERVICE, PnChannelKind, ProxyControlOpenReq, ProxyControlOpenResp, ProxyOpenReq,
    ProxyOpenResp,
};
use crate::runtime;
use crate::ttp::{TtpConnector, TtpPortListener, TtpServer, TtpServerRef, TtpTarget};

const PN_OPEN_TIMEOUT: Duration = Duration::from_secs(5);
const PN_TRAFFIC_CLEANUP_MAX_WAIT: Duration = Duration::from_secs(60 * 60);
const PN_TRAFFIC_CLEANUP_BATCH_SIZE: usize = 64;
const PN_TRAFFIC_RETENTION_MAX: Duration = Duration::from_secs(2_592_000);

struct ProxyStream<R, W> {
    read: R,
    write: W,
}

impl<R, W> ProxyStream<R, W> {
    fn new(read: R, write: W) -> Self {
        Self { read, write }
    }
}

impl<R, W> Unpin for ProxyStream<R, W>
where
    R: Unpin,
    W: Unpin,
{
}

impl<R, W> tokio::io::AsyncRead for ProxyStream<R, W>
where
    R: runtime::AsyncRead + Unpin,
    W: Unpin,
{
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.as_mut().get_mut().read).poll_read(cx, buf)
    }
}

impl<R, W> tokio::io::AsyncWrite for ProxyStream<R, W>
where
    R: Unpin,
    W: runtime::AsyncWrite + Unpin,
{
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.as_mut().get_mut().write).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.as_mut().get_mut().write).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.as_mut().get_mut().write).poll_shutdown(cx)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PnTrafficLimitConfig {
    pub tx_rate: Option<NonZeroU32>,
    pub tx_weight: Option<NonZeroU32>,
    pub rx_rate: Option<NonZeroU32>,
    pub rx_weight: Option<NonZeroU32>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PnUserTrafficSnapshot {
    pub tx_bytes: u64,
    pub tx_speed: u64,
    pub rx_bytes: u64,
    pub rx_speed: u64,
    pub tx_delta_bytes: u64,
    pub rx_delta_bytes: u64,
}

#[derive(Default)]
struct PnTrafficSnapshotBaseline {
    tx_bytes: u64,
    rx_bytes: u64,
}

struct PnUserTrafficEntry {
    tx_limiter: SpeedLimiterRef,
    rx_limiter: SpeedLimiterRef,
    stat: Arc<SfoSpeedStat>,
    snapshot_baseline: Mutex<PnTrafficSnapshotBaseline>,
}

impl PnUserTrafficEntry {
    fn new() -> Self {
        Self {
            tx_limiter: SpeedLimiter::new(None, None, None),
            rx_limiter: SpeedLimiter::new(None, None, None),
            stat: Arc::new(SfoSpeedStat::new()),
            snapshot_baseline: Mutex::new(PnTrafficSnapshotBaseline::default()),
        }
    }

    fn snapshot(&self) -> PnUserTrafficSnapshot {
        self.read_snapshot(true)
    }

    fn set_limit(&self, config: PnTrafficLimitConfig) {
        self.tx_limiter
            .set_limit(config.tx_rate, config.tx_weight);
        self.rx_limiter
            .set_limit(config.rx_rate, config.rx_weight);
    }

    fn peek_snapshot(&self) -> PnUserTrafficSnapshot {
        self.read_snapshot(false)
    }

    fn read_snapshot(&self, consume: bool) -> PnUserTrafficSnapshot {
        let mut baseline = self.snapshot_baseline.lock().unwrap();
        let tx_bytes = self.stat.get_read_sum_size();
        let rx_bytes = self.stat.get_write_sum_size();
        let snapshot = PnUserTrafficSnapshot {
            tx_bytes,
            tx_speed: self.stat.get_read_speed(),
            rx_bytes,
            rx_speed: self.stat.get_write_speed(),
            tx_delta_bytes: tx_bytes.saturating_sub(baseline.tx_bytes),
            rx_delta_bytes: rx_bytes.saturating_sub(baseline.rx_bytes),
        };
        if consume {
            baseline.tx_bytes = tx_bytes;
            baseline.rx_bytes = rx_bytes;
        }
        snapshot
    }
}

struct PnTrafficSession {
    source_read_limit: SpeedLimitSession,
    source_write_limit: SpeedLimitSession,
    source_tracker: Arc<dyn SpeedTracker>,
    target_tracker: Arc<dyn SpeedTracker>,
    lifecycle: PnTrafficSessionGuard,
}

struct PnTrafficUserState {
    entry: Arc<PnUserTrafficEntry>,
    active_sessions: usize,
    idle_deadline: Option<Instant>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct PnTrafficCleanupDeadline {
    deadline: Instant,
    user: P2pId,
}

#[derive(Default)]
struct PnTrafficManagerState {
    users: BTreeMap<P2pId, PnTrafficUserState>,
    limit_configs: BTreeMap<P2pId, PnTrafficLimitConfig>,
    retention: Duration,
    cleanup_deadlines: BTreeSet<PnTrafficCleanupDeadline>,
    shutdown: bool,
}

struct PnTrafficManagerShared {
    state: Mutex<PnTrafficManagerState>,
    cleanup_wakeup: Notify,
}

enum PnTrafficCleanupAction {
    Shutdown,
    WaitForNotification,
    WaitForDeadline(Duration),
    Yield,
}

struct PnTrafficSessionParticipant {
    user: P2pId,
    entry: Arc<PnUserTrafficEntry>,
}

struct PnTrafficSessionGuard {
    traffic_manager: Weak<PnTrafficManager>,
    source: PnTrafficSessionParticipant,
    target: Option<PnTrafficSessionParticipant>,
}

impl Drop for PnTrafficSessionGuard {
    fn drop(&mut self) {
        let Some(traffic_manager) = self.traffic_manager.upgrade() else {
            return;
        };
        traffic_manager.end_session(&self.source, self.target.as_ref());
    }
}

struct PnTrafficManager {
    shared: Arc<PnTrafficManagerShared>,
    cleanup_task: Mutex<Option<crate::executor::SpawnHandle<()>>>,
}

pub struct PnUserTrafficSnapshotIter {
    traffic_manager: Arc<PnTrafficManager>,
    next_after: Option<P2pId>,
}

impl Iterator for PnUserTrafficSnapshotIter {
    type Item = (P2pId, PnUserTrafficSnapshot);

    fn next(&mut self) -> Option<Self::Item> {
        let (user, entry) = self
            .traffic_manager
            .next_entry(self.next_after.as_ref())?;
        self.next_after = Some(user.clone());
        Some((user, entry.snapshot()))
    }
}

impl PnTrafficManager {
    fn retention_deadline(now: Instant, retention: Duration) -> Option<Instant> {
        now.checked_add(retention)
    }

    fn new() -> Arc<Self> {
        Arc::new(Self {
            shared: Arc::new(PnTrafficManagerShared {
                state: Mutex::new(PnTrafficManagerState::default()),
                cleanup_wakeup: Notify::new(),
            }),
            cleanup_task: Mutex::new(None),
        })
    }

    fn start_cleanup_task(&self) -> P2pResult<()> {
        let mut cleanup_task = self.cleanup_task.lock().unwrap();
        if cleanup_task.is_some() {
            return Ok(());
        }

        let state = self.shared.state.lock().unwrap();
        if state.shutdown {
            return Ok(());
        }

        let shared = self.shared.clone();
        let task = Executor::spawn_with_handle(async move {
            Self::run_cleanup_task(shared).await;
        })?;
        *cleanup_task = Some(task);
        Ok(())
    }

    async fn run_cleanup_task(shared: Arc<PnTrafficManagerShared>) {
        loop {
            let notified = shared.cleanup_wakeup.notified();
            tokio::pin!(notified);
            match Self::cleanup_action(&shared) {
                PnTrafficCleanupAction::Shutdown => return,
                PnTrafficCleanupAction::WaitForNotification => notified.await,
                PnTrafficCleanupAction::WaitForDeadline(wait) => {
                    tokio::select! {
                        _ = runtime::sleep(wait) => {}
                        _ = &mut notified => {}
                    }
                }
                PnTrafficCleanupAction::Yield => tokio::task::yield_now().await,
            }
        }
    }

    fn cleanup_action(shared: &PnTrafficManagerShared) -> PnTrafficCleanupAction {
        let mut state = shared.state.lock().unwrap();
        if state.shutdown {
            return PnTrafficCleanupAction::Shutdown;
        }

        let now = Instant::now();
        let mut processed = 0;
        while processed < PN_TRAFFIC_CLEANUP_BATCH_SIZE {
            let Some(next_deadline) = state.cleanup_deadlines.first().cloned() else {
                break;
            };
            if next_deadline.deadline > now {
                break;
            }

            state.cleanup_deadlines.remove(&next_deadline);
            processed += 1;
            let remove = state
                .users
                .get(&next_deadline.user)
                .is_some_and(|user_state| {
                    user_state.active_sessions == 0
                        && user_state.idle_deadline == Some(next_deadline.deadline)
                });
            if remove {
                state.users.remove(&next_deadline.user);
            }
        }

        let Some(next_deadline) = state.cleanup_deadlines.first() else {
            return PnTrafficCleanupAction::WaitForNotification;
        };
        if next_deadline.deadline <= now {
            return PnTrafficCleanupAction::Yield;
        }

        PnTrafficCleanupAction::WaitForDeadline(
            next_deadline
                .deadline
                .saturating_duration_since(now)
                .min(PN_TRAFFIC_CLEANUP_MAX_WAIT),
        )
    }

    fn iter(self: &Arc<Self>) -> PnUserTrafficSnapshotIter {
        PnUserTrafficSnapshotIter {
            traffic_manager: self.clone(),
            next_after: None,
        }
    }

    fn next_entry(
        &self,
        after: Option<&P2pId>,
    ) -> Option<(P2pId, Arc<PnUserTrafficEntry>)> {
        let state = self.shared.state.lock().unwrap();
        let (user, entry) = match after {
            Some(after) => state.users.range((Excluded(after), Unbounded)).next(),
            None => state.users.first_key_value(),
        }?;
        Some((user.clone(), entry.entry.clone()))
    }

    fn acquire_user(
        state: &mut PnTrafficManagerState,
        user: &P2pId,
    ) -> (Arc<PnUserTrafficEntry>, bool) {
        if let Some(user_state) = state.users.get_mut(user) {
            let canceled_deadline = user_state.idle_deadline.take().map(|deadline| {
                PnTrafficCleanupDeadline {
                    deadline,
                    user: user.clone(),
                }
            });
            user_state.active_sessions += 1;
            let entry = user_state.entry.clone();
            let canceled = canceled_deadline
                .as_ref()
                .is_some_and(|deadline| state.cleanup_deadlines.remove(deadline));
            return (entry, canceled);
        }

        let entry = Arc::new(PnUserTrafficEntry::new());
        if let Some(config) = state.limit_configs.get(user).copied() {
            entry.set_limit(config);
        }
        state.users.insert(
            user.clone(),
            PnTrafficUserState {
                entry: entry.clone(),
                active_sessions: 1,
                idle_deadline: None,
            },
        );
        (entry, false)
    }

    fn begin_session(
        self: &Arc<Self>,
        source_user: &P2pId,
        target_user: &P2pId,
    ) -> PnTrafficSession {
        let mut state = self.shared.state.lock().unwrap();
        let shutdown = state.shutdown;
        let (source_entry, target_entry, schedule_changed) = if shutdown {
            let source_entry = Arc::new(PnUserTrafficEntry::new());
            let target_entry = if source_user == target_user {
                source_entry.clone()
            } else {
                Arc::new(PnUserTrafficEntry::new())
            };
            (source_entry, target_entry, false)
        } else {
            let (source_entry, source_canceled) =
                Self::acquire_user(&mut state, source_user);
            let (target_entry, target_canceled) = if source_user == target_user {
                (source_entry.clone(), false)
            } else {
                Self::acquire_user(&mut state, target_user)
            };
            (
                source_entry,
                target_entry,
                source_canceled || target_canceled,
            )
        };
        drop(state);
        if schedule_changed {
            self.shared.cleanup_wakeup.notify_one();
        }

        let lifecycle = PnTrafficSessionGuard {
            traffic_manager: if shutdown {
                Weak::new()
            } else {
                Arc::downgrade(self)
            },
            source: PnTrafficSessionParticipant {
                user: source_user.clone(),
                entry: source_entry.clone(),
            },
            target: (source_user != target_user).then(|| PnTrafficSessionParticipant {
                user: target_user.clone(),
                entry: target_entry.clone(),
            }),
        };
        PnTrafficSession {
            source_read_limit: source_entry.tx_limiter.new_limit_session(),
            source_write_limit: source_entry.rx_limiter.new_limit_session(),
            source_tracker: source_entry.stat.clone(),
            target_tracker: target_entry.stat.clone(),
            lifecycle,
        }
    }

    fn end_session(
        &self,
        source: &PnTrafficSessionParticipant,
        target: Option<&PnTrafficSessionParticipant>,
    ) {
        let mut state = self.shared.state.lock().unwrap();
        let mut schedule_changed = Self::release_user(&mut state, source);
        if let Some(target) = target {
            schedule_changed |= Self::release_user(&mut state, target);
        }
        drop(state);
        if schedule_changed {
            self.shared.cleanup_wakeup.notify_one();
        }
    }

    fn release_user(
        state: &mut PnTrafficManagerState,
        participant: &PnTrafficSessionParticipant,
    ) -> bool {
        let became_idle = match state.users.get_mut(&participant.user) {
            Some(user_state) if Arc::ptr_eq(&user_state.entry, &participant.entry) => {
                if user_state.active_sessions == 0 {
                    false
                } else {
                    user_state.active_sessions -= 1;
                    user_state.active_sessions == 0
                }
            }
            _ => false,
        };
        if !became_idle {
            return false;
        }

        if state.retention.is_zero() || state.shutdown {
            state.users.remove(&participant.user);
            return false;
        }

        let Some(deadline) = Self::retention_deadline(Instant::now(), state.retention) else {
            state.users.remove(&participant.user);
            return false;
        };
        let Some(user_state) = state.users.get_mut(&participant.user) else {
            return false;
        };
        user_state.idle_deadline = Some(deadline);
        state.cleanup_deadlines.insert(PnTrafficCleanupDeadline {
            deadline,
            user: participant.user.clone(),
        })
    }

    fn set_retention(&self, retention: Duration) {
        let mut state = self.shared.state.lock().unwrap();
        if !state.shutdown {
            state.retention = retention.min(PN_TRAFFIC_RETENTION_MAX);
        }
    }

    fn set_user_limit(&self, user: P2pId, config: PnTrafficLimitConfig) {
        let mut state = self.shared.state.lock().unwrap();
        if state.shutdown {
            return;
        }
        state.limit_configs.insert(user.clone(), config);
        if let Some(user_state) = state.users.get(&user) {
            user_state.entry.set_limit(config);
        }
    }

    fn snapshot(&self, user: &P2pId) -> Option<PnUserTrafficSnapshot> {
        let entry = self
            .shared
            .state
            .lock()
            .unwrap()
            .users
            .get(user)
            .map(|user_state| user_state.entry.clone())?;
        Some(entry.snapshot())
    }

    fn peek_snapshot(&self, user: &P2pId) -> Option<PnUserTrafficSnapshot> {
        let entry = self
            .shared
            .state
            .lock()
            .unwrap()
            .users
            .get(user)
            .map(|user_state| user_state.entry.clone())?;
        Some(entry.peek_snapshot())
    }

    fn shutdown(&self) {
        let cleanup_task = {
            let mut cleanup_task = self.cleanup_task.lock().unwrap();
            let mut state = self.shared.state.lock().unwrap();
            state.shutdown = true;
            state.users.clear();
            state.limit_configs.clear();
            state.cleanup_deadlines.clear();
            state.retention = Duration::ZERO;
            cleanup_task.take()
        };
        self.shared.cleanup_wakeup.notify_one();
        drop(cleanup_task);
    }
}

impl Drop for PnTrafficManager {
    fn drop(&mut self) {
        self.shutdown();
    }
}

pub struct PnServer {
    ttp_server: TtpServerRef,
    service: PnServiceRef,
    started: AtomicBool,
    stopped: AtomicBool,
    accept_task: Mutex<Option<crate::executor::SpawnHandle<()>>>,
}

pub type PnServerRef = Arc<PnServer>;
pub type PnServiceRef = Arc<PnService>;

#[derive(Clone, Debug)]
pub struct PnConnectionValidateContext {
    pub from: P2pId,
    pub to: P2pId,
    pub tunnel_id: crate::types::TunnelId,
    pub kind: crate::pn::PnChannelKind,
    pub purpose: TunnelPurpose,
    pub is_control: bool,
}

#[async_trait::async_trait]
pub trait PnConnectionValidator: Send + Sync + 'static {
    async fn validate(&self, ctx: &PnConnectionValidateContext) -> P2pResult<ValidateResult>;
}

pub type PnConnectionValidatorRef = Arc<dyn PnConnectionValidator>;

pub struct AllowAllPnConnectionValidator;

#[async_trait::async_trait]
impl PnConnectionValidator for AllowAllPnConnectionValidator {
    async fn validate(&self, _ctx: &PnConnectionValidateContext) -> P2pResult<ValidateResult> {
        Ok(ValidateResult::Accept)
    }
}

pub fn allow_all_pn_connection_validator() -> PnConnectionValidatorRef {
    Arc::new(AllowAllPnConnectionValidator)
}

pub struct RejectAllPnConnectionValidator;

#[async_trait::async_trait]
impl PnConnectionValidator for RejectAllPnConnectionValidator {
    async fn validate(&self, _ctx: &PnConnectionValidateContext) -> P2pResult<ValidateResult> {
        Ok(ValidateResult::Reject(
            "pn assigned target policy not configured".to_owned(),
        ))
    }
}

pub fn reject_all_pn_connection_validator() -> PnConnectionValidatorRef {
    Arc::new(RejectAllPnConnectionValidator)
}

#[async_trait::async_trait]
pub trait PnAssignedTargetPolicy: Send + Sync + 'static {
    async fn is_assigned_target(&self, target: &P2pId) -> P2pResult<bool>;
}

pub type PnAssignedTargetPolicyRef = Arc<dyn PnAssignedTargetPolicy>;

struct AssignedTargetConnectionValidator {
    policy: PnAssignedTargetPolicyRef,
}

#[async_trait::async_trait]
impl PnConnectionValidator for AssignedTargetConnectionValidator {
    async fn validate(&self, ctx: &PnConnectionValidateContext) -> P2pResult<ValidateResult> {
        if self.policy.is_assigned_target(&ctx.to).await? {
            Ok(ValidateResult::Accept)
        } else {
            Ok(ValidateResult::Reject(format!(
                "target {} is not assigned to this pn server",
                ctx.to
            )))
        }
    }
}

pub fn assigned_target_pn_connection_validator(
    policy: PnAssignedTargetPolicyRef,
) -> PnConnectionValidatorRef {
    Arc::new(AssignedTargetConnectionValidator { policy })
}

#[async_trait::async_trait]
pub trait PnTargetStreamFactory: Send + Sync + 'static {
    async fn open_target_stream(
        &self,
        target: &P2pId,
    ) -> P2pResult<(TunnelStreamRead, TunnelStreamWrite)>;
}

pub type PnTargetStreamFactoryRef = Arc<dyn PnTargetStreamFactory>;

#[async_trait::async_trait]
impl PnTargetStreamFactory for TtpServer {
    async fn open_target_stream(
        &self,
        target: &P2pId,
    ) -> P2pResult<(TunnelStreamRead, TunnelStreamWrite)> {
        let ttp_target = TtpTarget {
            local_ep: None,
            remote_ep: Endpoint::default(),
            remote_id: target.clone(),
            remote_name: Some(target.to_string()),
        };
        let (_meta, read, write) = self
            .open_stream(
                &ttp_target,
                crate::networks::TunnelPurpose::from_value(&PROXY_SERVICE.to_string()).unwrap(),
            )
            .await?;
        Ok((read, write))
    }
}

pub struct PnService {
    target_stream_factory: PnTargetStreamFactoryRef,
    connection_validator: PnConnectionValidatorRef,
    traffic_manager: Arc<PnTrafficManager>,
    relay_sessions: Mutex<HashSet<PnRelaySessionKey>>,
}

#[derive(Clone, Eq, Hash, PartialEq)]
struct PnRelaySessionKey {
    tunnel_id: crate::types::TunnelId,
    from: P2pId,
    to: P2pId,
}

impl PnService {
    pub fn new(
        target_stream_factory: PnTargetStreamFactoryRef,
        connection_validator: PnConnectionValidatorRef,
    ) -> PnServiceRef {
        Self::new_with_traffic_manager(
            target_stream_factory,
            connection_validator,
            PnTrafficManager::new(),
        )
    }

    fn new_with_traffic_manager(
        target_stream_factory: PnTargetStreamFactoryRef,
        connection_validator: PnConnectionValidatorRef,
        traffic_manager: Arc<PnTrafficManager>,
    ) -> PnServiceRef {
        Arc::new(Self {
            target_stream_factory,
            connection_validator,
            traffic_manager,
            relay_sessions: Mutex::new(HashSet::new()),
        })
    }

    fn has_relay_session(
        &self,
        tunnel_id: crate::types::TunnelId,
        from: &P2pId,
        to: &P2pId,
    ) -> bool {
        let sessions = self.relay_sessions.lock().unwrap();
        sessions.contains(&PnRelaySessionKey {
            tunnel_id,
            from: from.clone(),
            to: to.clone(),
        }) || sessions.contains(&PnRelaySessionKey {
            tunnel_id,
            from: to.clone(),
            to: from.clone(),
        })
    }

    fn register_relay_session(&self, tunnel_id: crate::types::TunnelId, from: &P2pId, to: &P2pId) {
        self.relay_sessions
            .lock()
            .unwrap()
            .insert(PnRelaySessionKey {
                tunnel_id,
                from: from.clone(),
                to: to.clone(),
            });
    }

    fn unregister_relay_session(
        &self,
        tunnel_id: crate::types::TunnelId,
        from: &P2pId,
        to: &P2pId,
    ) {
        let mut sessions = self.relay_sessions.lock().unwrap();
        sessions.remove(&PnRelaySessionKey {
            tunnel_id,
            from: from.clone(),
            to: to.clone(),
        });
        sessions.remove(&PnRelaySessionKey {
            tunnel_id,
            from: to.clone(),
            to: from.clone(),
        });
    }

    async fn validate_proxy_open_req(&self, req: &ProxyOpenReq) -> P2pResult<()> {
        if self.has_relay_session(req.tunnel_id, &req.from, &req.to) {
            return Ok(());
        }
        let ctx = PnConnectionValidateContext {
            from: req.from.clone(),
            to: req.to.clone(),
            tunnel_id: req.tunnel_id,
            kind: req.kind,
            purpose: req.purpose.clone(),
            is_control: false,
        };
        match self.connection_validator.validate(&ctx).await? {
            ValidateResult::Accept => Ok(()),
            ValidateResult::Reject(reason) => Err(p2p_err!(
                P2pErrorCode::PermissionDenied,
                "pn connection validate failed from={} to={} kind={:?} purpose={} reason={}",
                req.from,
                req.to,
                req.kind,
                req.purpose,
                reason
            )),
        }
    }

    async fn validate_proxy_control_open_req(&self, req: &ProxyControlOpenReq) -> P2pResult<()> {
        let ctx = PnConnectionValidateContext {
            from: req.from.clone(),
            to: req.to.clone(),
            tunnel_id: req.tunnel_id,
            kind: PnChannelKind::Stream,
            purpose: TunnelPurpose::from_value(&PROXY_SERVICE.to_string())?,
            is_control: true,
        };
        match self.connection_validator.validate(&ctx).await? {
            ValidateResult::Accept => Ok(()),
            ValidateResult::Reject(reason) => Err(p2p_err!(
                P2pErrorCode::PermissionDenied,
                "pn control connection validate failed from={} to={} reason={}",
                req.from,
                req.to,
                reason
            )),
        }
    }

    async fn handle_proxy_open_req(
        self: &Arc<Self>,
        from: P2pId,
        mut req: ProxyOpenReq,
        read: TunnelStreamRead,
        write: TunnelStreamWrite,
    ) {
        req.from = from.clone();
        log::debug!(
            "pn server open recv tunnel_id={:?} from={} to={} kind={:?} requested_purpose={} proxy_service={}",
            req.tunnel_id,
            req.from,
            req.to,
            req.kind,
            req.purpose,
            PROXY_SERVICE
        );

        let mut source_write = write;
        let open_result = match self.validate_proxy_open_req(&req).await {
            Ok(()) => runtime::timeout(
                PN_OPEN_TIMEOUT,
                self.target_stream_factory.open_target_stream(&req.to),
            )
            .await
            .map_err(into_p2p_err!(P2pErrorCode::Timeout, "pn open timeout"))
            .and_then(|ret| ret),
            Err(err) => Err(err),
        };

        let open_result = match open_result {
            Ok((mut target_read, mut target_write)) => {
                log::debug!(
                    "pn server open upstream connected tunnel_id={:?} target={} requested_purpose={} proxy_service={}",
                    req.tunnel_id,
                    req.to,
                    req.purpose,
                    PROXY_SERVICE
                );
                let bridge_ready = async {
                    write_proxy_command(&mut target_write, req.clone()).await?;
                    let resp = runtime::timeout(
                        PN_OPEN_TIMEOUT,
                        read_proxy_command::<_, ProxyOpenResp>(&mut target_read),
                    )
                    .await
                    .map_err(into_p2p_err!(P2pErrorCode::Timeout, "pn open timeout"))??;
                    if resp.tunnel_id != req.tunnel_id {
                        return Err(p2p_err!(
                            P2pErrorCode::InvalidData,
                            "pn open response tunnel id mismatch"
                        ));
                    }
                    let result = TunnelCommandResult::from_u8(resp.result).ok_or_else(|| {
                        p2p_err!(
                            P2pErrorCode::InvalidData,
                            "invalid pn open result {}",
                            resp.result
                        )
                    })?;
                    Ok((result, resp, target_read, target_write))
                }
                .await;

                match bridge_ready {
                    Ok((result, resp, target_read, target_write)) => {
                        log::debug!(
                            "pn server open upstream resp tunnel_id={:?} target={} kind={:?} requested_purpose={} proxy_service={} result={:?}",
                            req.tunnel_id,
                            req.to,
                            req.kind,
                            req.purpose,
                            PROXY_SERVICE,
                            result
                        );
                        let _ = write_proxy_command(
                            &mut source_write,
                            ProxyOpenResp {
                                tunnel_id: req.tunnel_id,
                                result: result as u8,
                            },
                        )
                        .await;
                        if result == TunnelCommandResult::Success {
                            self.register_relay_session(req.tunnel_id, &req.from, &req.to);
                            Ok((target_read, target_write))
                        } else {
                            Err(result.into_p2p_error(format!(
                                "pn open rejected kind {:?} purpose {}",
                                req.kind, req.purpose
                            )))
                        }
                    }
                    Err(err) => {
                        log::warn!(
                            "pn server open upstream failed tunnel_id={:?} target={} kind={:?} requested_purpose={} proxy_service={} code={:?} msg={}",
                            req.tunnel_id,
                            req.to,
                            req.kind,
                            req.purpose,
                            PROXY_SERVICE,
                            err.code(),
                            err.msg()
                        );
                        let _ = write_proxy_command(
                            &mut source_write,
                            ProxyOpenResp {
                                tunnel_id: req.tunnel_id,
                                result: result_from_error(&err) as u8,
                            },
                        )
                        .await;
                        Err(err)
                    }
                }
            }
            Err(err) => {
                log::warn!(
                    "pn server open target failed tunnel_id={:?} from={} to={} kind={:?} requested_purpose={} proxy_service={} code={:?} msg={}",
                    req.tunnel_id,
                    req.from,
                    req.to,
                    req.kind,
                    req.purpose,
                    PROXY_SERVICE,
                    err.code(),
                    err.msg()
                );
                let _ = write_proxy_command(
                    &mut source_write,
                    ProxyOpenResp {
                        tunnel_id: req.tunnel_id,
                        result: result_from_error(&err) as u8,
                    },
                )
                .await;
                Err(err)
            }
        };

        if let Ok((target_read, target_write)) = open_result {
            log::debug!(
                "pn server bridge start tunnel_id={:?} from={} to={} kind={:?} requested_purpose={} proxy_service={}",
                req.tunnel_id,
                from,
                req.to,
                req.kind,
                req.purpose,
                PROXY_SERVICE
            );
            let PnTrafficSession {
                source_read_limit,
                source_write_limit,
                source_tracker,
                target_tracker,
                lifecycle: _traffic_lifecycle,
            } = self.traffic_manager.begin_session(&req.from, &req.to);
            let source_stream = ProxyStream::new(read, source_write);
            let source_stream = LimitStream::new(
                source_stream,
                source_read_limit,
                source_write_limit,
            );
            let mut source_stream = StatStream::new_with_tracker(source_stream, source_tracker);
            let target_stream = ProxyStream::new(target_read, target_write);
            let mut target_stream = StatStream::new_with_tracker(target_stream, target_tracker);
            let _ = copy_bidirectional(&mut source_stream, &mut target_stream).await;
            if let Some(snapshot) = self.traffic_manager.peek_snapshot(&req.from) {
                log::debug!(
                    "pn server bridge stop tunnel_id={:?} from={} to={} tx_bytes={} rx_bytes={} tx_speed={} rx_speed={}",
                    req.tunnel_id,
                    req.from,
                    req.to,
                    snapshot.tx_bytes,
                    snapshot.rx_bytes,
                    snapshot.tx_speed,
                    snapshot.rx_speed
                );
            }
            if let Some(snapshot) = self.traffic_manager.peek_snapshot(&req.to) {
                log::debug!(
                    "pn server bridge target stop tunnel_id={:?} target={} tx_bytes={} rx_bytes={} tx_speed={} rx_speed={}",
                    req.tunnel_id,
                    req.to,
                    snapshot.tx_bytes,
                    snapshot.rx_bytes,
                    snapshot.tx_speed,
                    snapshot.rx_speed
                );
            }
        }
    }

    async fn handle_proxy_control_open_req(
        self: &Arc<Self>,
        from: P2pId,
        mut req: ProxyControlOpenReq,
        read: TunnelStreamRead,
        write: TunnelStreamWrite,
    ) {
        req.from = from.clone();
        log::debug!(
            "pn server control open recv tunnel_id={:?} from={} to={} proxy_service={}",
            req.tunnel_id,
            req.from,
            req.to,
            PROXY_SERVICE
        );

        let mut source_write = write;
        let open_result = match self.validate_proxy_control_open_req(&req).await {
            Ok(()) => runtime::timeout(
                PN_OPEN_TIMEOUT,
                self.target_stream_factory.open_target_stream(&req.to),
            )
            .await
            .map_err(into_p2p_err!(
                P2pErrorCode::Timeout,
                "pn control open timeout"
            ))
            .and_then(|ret| ret),
            Err(err) => Err(err),
        };

        let open_result = match open_result {
            Ok((mut target_read, mut target_write)) => {
                let bridge_ready = async {
                    write_proxy_command(&mut target_write, req.clone()).await?;
                    let resp = runtime::timeout(
                        PN_OPEN_TIMEOUT,
                        read_proxy_command::<_, ProxyControlOpenResp>(&mut target_read),
                    )
                    .await
                    .map_err(into_p2p_err!(
                        P2pErrorCode::Timeout,
                        "pn control open timeout"
                    ))??;
                    if resp.tunnel_id != req.tunnel_id {
                        return Err(p2p_err!(
                            P2pErrorCode::InvalidData,
                            "pn control open response tunnel id mismatch"
                        ));
                    }
                    let result = TunnelCommandResult::from_u8(resp.result).ok_or_else(|| {
                        p2p_err!(
                            P2pErrorCode::InvalidData,
                            "invalid pn control open result {}",
                            resp.result
                        )
                    })?;
                    Ok((result, resp, target_read, target_write))
                }
                .await;

                match bridge_ready {
                    Ok((result, resp, target_read, target_write)) => {
                        let _ = write_proxy_command(
                            &mut source_write,
                            ProxyControlOpenResp {
                                tunnel_id: resp.tunnel_id,
                                result: result as u8,
                            },
                        )
                        .await;
                        if result == TunnelCommandResult::Success {
                            Ok((target_read, target_write))
                        } else {
                            Err(result.into_p2p_error("pn control open rejected".to_owned()))
                        }
                    }
                    Err(err) => {
                        let _ = write_proxy_command(
                            &mut source_write,
                            ProxyControlOpenResp {
                                tunnel_id: req.tunnel_id,
                                result: result_from_error(&err) as u8,
                            },
                        )
                        .await;
                        Err(err)
                    }
                }
            }
            Err(err) => {
                let _ = write_proxy_command(
                    &mut source_write,
                    ProxyControlOpenResp {
                        tunnel_id: req.tunnel_id,
                        result: result_from_error(&err) as u8,
                    },
                )
                .await;
                Err(err)
            }
        };

        if let Ok((target_read, target_write)) = open_result {
            log::debug!(
                "pn server control bridge start tunnel_id={:?} from={} to={} proxy_service={}",
                req.tunnel_id,
                from,
                req.to,
                PROXY_SERVICE
            );
            let PnTrafficSession {
                source_read_limit,
                source_write_limit,
                source_tracker,
                target_tracker,
                lifecycle: _traffic_lifecycle,
            } = self.traffic_manager.begin_session(&req.from, &req.to);
            let source_stream = ProxyStream::new(read, source_write);
            let source_stream = LimitStream::new(
                source_stream,
                source_read_limit,
                source_write_limit,
            );
            let mut source_stream = StatStream::new_with_tracker(source_stream, source_tracker);
            let target_stream = ProxyStream::new(target_read, target_write);
            let mut target_stream = StatStream::new_with_tracker(target_stream, target_tracker);
            let _ = copy_bidirectional(&mut source_stream, &mut target_stream).await;
            self.unregister_relay_session(req.tunnel_id, &req.from, &req.to);
            log::debug!(
                "pn server control bridge stop tunnel_id={:?} from={} to={} proxy_service={}",
                req.tunnel_id,
                req.from,
                req.to,
                PROXY_SERVICE
            );
        }
    }

    pub async fn handle_proxy_connection(
        self: &Arc<Self>,
        from: P2pId,
        mut read: TunnelStreamRead,
        write: TunnelStreamWrite,
    ) {
        let header = match read_tunnel_command_header(&mut read).await {
            Ok(v) => v,
            Err(e) => {
                log::warn!("read pn control frame failed from {}: {:?}", from, e);
                return;
            }
        };

        if header.command_id == ProxyOpenReq::COMMAND_ID {
            let command_id = header.command_id;
            let data_len = header.data_len;
            if let Ok(req) = read_tunnel_command_body::<_, ProxyOpenReq>(&mut read, header).await {
                log::debug!(
                    "pn server data connection control frame from={} command_id={} data_len={}",
                    from,
                    command_id,
                    data_len
                );
                self.handle_proxy_open_req(from, req.body, read, write)
                    .await;
            }
        } else if header.command_id == ProxyControlOpenReq::COMMAND_ID {
            let command_id = header.command_id;
            let data_len = header.data_len;
            if let Ok(req) =
                read_tunnel_command_body::<_, ProxyControlOpenReq>(&mut read, header).await
            {
                log::debug!(
                    "pn server control connection frame from={} command_id={} data_len={}",
                    from,
                    command_id,
                    data_len
                );
                self.handle_proxy_control_open_req(from, req.body, read, write)
                    .await;
            }
        }
    }
}

impl PnServer {
    pub fn new(ttp_server: TtpServerRef) -> PnServerRef {
        Self::new_with_connection_validator(ttp_server, allow_all_pn_connection_validator())
    }

    pub fn new_with_assigned_target_policy(
        ttp_server: TtpServerRef,
        assigned_target_policy: PnAssignedTargetPolicyRef,
    ) -> PnServerRef {
        Self::new_with_connection_validator(
            ttp_server,
            assigned_target_pn_connection_validator(assigned_target_policy),
        )
    }

    pub fn new_with_connection_validator(
        ttp_server: TtpServerRef,
        connection_validator: PnConnectionValidatorRef,
    ) -> PnServerRef {
        let target_stream_factory: PnTargetStreamFactoryRef = ttp_server.clone();
        let service = PnService::new(target_stream_factory, connection_validator);
        Arc::new(Self {
            ttp_server,
            service,
            started: AtomicBool::new(false),
            stopped: AtomicBool::new(false),
            accept_task: Mutex::new(None),
        })
    }

    pub fn set_user_traffic_limit(&self, user: P2pId, config: PnTrafficLimitConfig) {
        self.service.traffic_manager.set_user_limit(user, config);
    }

    /// Sets retention for users that become idle in the future.
    ///
    /// Values above 30 days are clamped to 30 days; existing idle deadlines are
    /// unchanged.
    pub fn set_user_traffic_retention(&self, retention: Duration) {
        self.service.traffic_manager.set_retention(retention);
    }

    pub fn get_user_traffic_snapshot(&self, user: &P2pId) -> Option<PnUserTrafficSnapshot> {
        self.service.traffic_manager.snapshot(user)
    }

    pub fn iter_user_traffic_snapshots(&self) -> PnUserTrafficSnapshotIter {
        self.service.traffic_manager.iter()
    }

    pub async fn start(self: &Arc<Self>) -> P2pResult<()> {
        if self
            .started
            .compare_exchange(
                false,
                true,
                atomic::Ordering::SeqCst,
                atomic::Ordering::SeqCst,
            )
            .is_err()
        {
            return Ok(());
        }

        if let Err(err) = self.service.traffic_manager.start_cleanup_task() {
            self.started.store(false, atomic::Ordering::SeqCst);
            return Err(err);
        }

        let weak = Arc::downgrade(self);
        let callback = Arc::new(move |accepted: P2pResult<crate::ttp::TtpIncomingStream>| {
            let weak = weak.clone();
            Box::pin(async move {
                let Some(server) = weak.upgrade() else {
                    return;
                };
                if server.is_stopped() {
                    return;
                }
                let service = server.service.clone();
                let (meta, read, write) = match accepted {
                    Ok(accepted) => accepted,
                    Err(err) => {
                        log::warn!("pn server accept stopped: {:?}", err);
                        return;
                    }
                };
                Executor::spawn(async move {
                    service
                        .handle_proxy_connection(meta.remote_id, read, write)
                        .await;
                });
            }) as crate::ttp::TtpIncomingStreamCallbackFuture
        });

        match self
            .ttp_server
            .listen_stream(
                crate::networks::TunnelPurpose::from_value(&PROXY_SERVICE.to_string()).unwrap(),
                callback,
            )
            .await
        {
            Ok(()) => {}
            Err(err) => {
                self.started.store(false, atomic::Ordering::SeqCst);
                return Err(err);
            }
        };
        Ok(())
    }

    pub fn stop(&self) {
        self.stopped.store(true, atomic::Ordering::Relaxed);
        self.abort_accept_task();
        self.service.traffic_manager.shutdown();
    }

    pub fn is_stopped(&self) -> bool {
        self.stopped.load(atomic::Ordering::Relaxed)
    }

    fn abort_accept_task(&self) {
        if let Some(task) = self.accept_task.lock().unwrap().take() {
            task.abort();
        }
    }
}

impl Drop for PnServer {
    fn drop(&mut self) {
        self.abort_accept_task();
        self.service.traffic_manager.shutdown();
    }
}

fn result_from_error(err: &P2pError) -> TunnelCommandResult {
    match err.code() {
        P2pErrorCode::PortNotListen => TunnelCommandResult::PortNotListen,
        P2pErrorCode::Timeout => TunnelCommandResult::Timeout,
        P2pErrorCode::Interrupted | P2pErrorCode::NotFound => TunnelCommandResult::Interrupted,
        P2pErrorCode::InvalidParam | P2pErrorCode::PermissionDenied | P2pErrorCode::Reject => {
            TunnelCommandResult::InvalidParam
        }
        P2pErrorCode::InvalidData => TunnelCommandResult::ProtocolError,
        _ => TunnelCommandResult::InternalError,
    }
}

async fn write_proxy_command<T>(write: &mut TunnelStreamWrite, body: T) -> P2pResult<()>
where
    T: TunnelCommandBody,
{
    let command = TunnelCommand::new(body)?;
    write_tunnel_command(write, &command).await
}

async fn read_proxy_command<R, T>(read: &mut R) -> P2pResult<T>
where
    R: runtime::AsyncRead + Unpin,
    T: TunnelCommandBody,
{
    let header = read_tunnel_command_header(read).await?;
    let command = read_tunnel_command_body::<_, T>(read, header).await?;
    Ok(command.body)
}

#[cfg(test)]
mod tests {
    mod traffic_manager_tests;

    use super::*;
    use crate::endpoint::{Endpoint, Protocol};
    use crate::error::p2p_err;
    use crate::executor::Executor;
    use crate::networks::{
        IncomingTunnelCallback, NetManager, Tunnel, TunnelCommand, TunnelListenerInfo,
        TunnelNetwork, TunnelNetworkRef, TunnelState,
    };
    use crate::p2p_identity::{
        EncodedP2pIdentity, P2pIdentity, P2pIdentityCertRef, P2pIdentityRef, P2pSignature,
    };
    use crate::pn::PnChannelKind;
    use crate::tls::DefaultTlsServerCertResolver;
    use crate::ttp::TtpServer;
    use crate::types::{TunnelCandidateId, TunnelId};
    use std::num::NonZeroU32;
    use std::sync::Mutex as StdMutex;
    use std::time::Instant;
    use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream, ReadHalf, WriteHalf, split};
    use tokio::sync::{Mutex as AsyncMutex, mpsc, oneshot};

    const TEST_CHANNEL_CAPACITY: usize = 8;
    use tokio::time::{Duration, timeout};

    struct DummyIdentity {
        id: P2pId,
        name: String,
        endpoints: Vec<Endpoint>,
    }

    impl P2pIdentity for DummyIdentity {
        fn get_identity_cert(&self) -> P2pResult<P2pIdentityCertRef> {
            Err(p2p_err!(P2pErrorCode::NotSupport, "unused in test"))
        }

        fn get_id(&self) -> P2pId {
            self.id.clone()
        }

        fn get_name(&self) -> String {
            self.name.clone()
        }

        fn sign_type(&self) -> crate::p2p_identity::P2pIdentitySignType {
            crate::p2p_identity::P2pIdentitySignType::Rsa
        }

        fn sign(&self, _message: &[u8]) -> P2pResult<P2pSignature> {
            Ok(vec![])
        }

        fn get_encoded_identity(&self) -> P2pResult<EncodedP2pIdentity> {
            Ok(vec![])
        }

        fn endpoints(&self) -> Vec<Endpoint> {
            self.endpoints.clone()
        }

        fn update_endpoints(&self, eps: Vec<Endpoint>) -> P2pIdentityRef {
            Arc::new(Self {
                id: self.id.clone(),
                name: self.name.clone(),
                endpoints: eps,
            })
        }
    }

    struct FakeTunnel {
        tunnel_id: TunnelId,
        candidate_id: TunnelCandidateId,
        local_id: P2pId,
        remote_id: P2pId,
        local_ep: Endpoint,
        remote_ep: Endpoint,
        incoming_rx: Arc<
            AsyncMutex<
                mpsc::Receiver<(
                    crate::networks::TunnelPurpose,
                    TunnelStreamRead,
                    TunnelStreamWrite,
                )>,
            >,
        >,
        opened_tx: Option<
            mpsc::Sender<(
                crate::networks::TunnelPurpose,
                ReadHalf<DuplexStream>,
                WriteHalf<DuplexStream>,
            )>,
        >,
        attached_tx: StdMutex<Option<oneshot::Sender<()>>>,
    }

    impl FakeTunnel {
        fn new(
            local_id: P2pId,
            remote_id: P2pId,
            local_ep: Endpoint,
            remote_ep: Endpoint,
        ) -> (
            Arc<Self>,
            mpsc::Sender<(
                crate::networks::TunnelPurpose,
                TunnelStreamRead,
                TunnelStreamWrite,
            )>,
            oneshot::Receiver<()>,
        ) {
            let (incoming_tx, incoming_rx) = mpsc::channel(TEST_CHANNEL_CAPACITY);
            let (attached_tx, attached_rx) = oneshot::channel();
            (
                Arc::new(Self {
                    tunnel_id: TunnelId::from(1),
                    candidate_id: TunnelCandidateId::from(1),
                    local_id,
                    remote_id,
                    local_ep,
                    remote_ep,
                    incoming_rx: Arc::new(AsyncMutex::new(incoming_rx)),
                    opened_tx: None,
                    attached_tx: StdMutex::new(Some(attached_tx)),
                }),
                incoming_tx,
                attached_rx,
            )
        }

        fn new_with_open_stream(
            local_id: P2pId,
            remote_id: P2pId,
            local_ep: Endpoint,
            remote_ep: Endpoint,
        ) -> (
            Arc<Self>,
            mpsc::Receiver<(
                crate::networks::TunnelPurpose,
                ReadHalf<DuplexStream>,
                WriteHalf<DuplexStream>,
            )>,
            oneshot::Receiver<()>,
        ) {
            let (opened_tx, opened_rx) = mpsc::channel(TEST_CHANNEL_CAPACITY);
            let (attached_tx, attached_rx) = oneshot::channel();
            (
                Arc::new(Self {
                    tunnel_id: TunnelId::from(2),
                    candidate_id: TunnelCandidateId::from(2),
                    local_id,
                    remote_id,
                    local_ep,
                    remote_ep,
                    incoming_rx: Arc::new(AsyncMutex::new(mpsc::channel(TEST_CHANNEL_CAPACITY).1)),
                    opened_tx: Some(opened_tx),
                    attached_tx: StdMutex::new(Some(attached_tx)),
                }),
                opened_rx,
                attached_rx,
            )
        }
    }

    #[async_trait::async_trait]
    impl Tunnel for FakeTunnel {
        fn tunnel_id(&self) -> TunnelId {
            self.tunnel_id
        }

        fn candidate_id(&self) -> TunnelCandidateId {
            self.candidate_id
        }

        fn form(&self) -> crate::networks::TunnelForm {
            crate::networks::TunnelForm::Active
        }

        fn is_reverse(&self) -> bool {
            false
        }

        fn protocol(&self) -> Protocol {
            self.local_ep.protocol()
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
            TunnelState::Connected
        }

        fn is_closed(&self) -> bool {
            false
        }

        fn close(&self) -> P2pResult<()> {
            Ok(())
        }

        async fn listen_stream(
            &self,
            _vports: crate::networks::ListenVPortsRef,
            callback: crate::networks::IncomingStreamCallback,
        ) -> P2pResult<()> {
            if let Some(tx) = self.attached_tx.lock().unwrap().take() {
                let _ = tx.send(());
            }
            let incoming_rx = self.incoming_rx.clone();
            tokio::spawn(async move {
                let mut incoming_rx = incoming_rx.lock().await;
                while let Some((purpose, read, write)) = incoming_rx.recv().await {
                    callback(Ok((purpose, read, write))).await;
                }
            });
            Ok(())
        }

        async fn listen_datagram(
            &self,
            _vports: crate::networks::ListenVPortsRef,
            _callback: crate::networks::IncomingDatagramCallback,
        ) -> P2pResult<()> {
            Ok(())
        }

        async fn open_stream(
            &self,
            purpose: crate::networks::TunnelPurpose,
        ) -> P2pResult<(TunnelStreamRead, TunnelStreamWrite)> {
            let ((local_read, local_write), (remote_read, remote_write)) = make_stream_pair();
            if let Some(opened_tx) = &self.opened_tx {
                opened_tx
                    .try_send((purpose, remote_read, remote_write))
                    .map_err(|err| {
                        p2p_err!(
                            P2pErrorCode::OutOfLimit,
                            "open stream observer queue failed: {}",
                            err
                        )
                    })?;
                Ok((local_read, local_write))
            } else {
                Err(p2p_err!(P2pErrorCode::NotSupport, "unused in test"))
            }
        }

        async fn open_datagram(
            &self,
            _purpose: crate::networks::TunnelPurpose,
        ) -> P2pResult<crate::networks::TunnelDatagramWrite> {
            Err(p2p_err!(P2pErrorCode::NotSupport, "unused in test"))
        }
    }

    struct FakeTunnelNetwork {
        protocol: Protocol,
        rx: AsyncMutex<Option<mpsc::Receiver<P2pResult<crate::networks::TunnelRef>>>>,
        tx: mpsc::Sender<P2pResult<crate::networks::TunnelRef>>,
        infos: Mutex<Vec<TunnelListenerInfo>>,
    }

    impl FakeTunnelNetwork {
        fn new(protocol: Protocol) -> Arc<Self> {
            let (tx, rx) = mpsc::channel(TEST_CHANNEL_CAPACITY);
            Arc::new(Self {
                protocol,
                rx: AsyncMutex::new(Some(rx)),
                tx,
                infos: Mutex::new(Vec::new()),
            })
        }

        fn push_tunnel(&self, tunnel: crate::networks::TunnelRef) -> P2pResult<()> {
            self.tx.try_send(Ok(tunnel)).map_err(|err| {
                p2p_err!(
                    P2pErrorCode::OutOfLimit,
                    "test tunnel queue failed: {}",
                    err
                )
            })
        }
    }

    #[async_trait::async_trait]
    impl TunnelNetwork for FakeTunnelNetwork {
        fn protocol(&self) -> Protocol {
            self.protocol
        }

        fn is_udp(&self) -> bool {
            self.protocol == Protocol::Quic
        }

        async fn listen(
            &self,
            local: &Endpoint,
            _out: Option<Endpoint>,
            mapping_port: Option<u16>,
            on_incoming_tunnel: IncomingTunnelCallback,
        ) -> P2pResult<()> {
            *self.infos.lock().unwrap() = vec![TunnelListenerInfo {
                local: *local,
                mapping_port,
            }];
            let mut rx =
                self.rx.lock().await.take().ok_or_else(|| {
                    p2p_err!(P2pErrorCode::ErrorState, "fake listener already used")
                })?;
            Executor::spawn_ok(async move {
                loop {
                    match rx.recv().await {
                        Some(result) => on_incoming_tunnel(result).await,
                        None => break,
                    }
                }
            });
            Ok(())
        }

        async fn close_all_listener(&self) -> P2pResult<()> {
            Ok(())
        }

        fn listener_infos(&self) -> Vec<TunnelListenerInfo> {
            self.infos.lock().unwrap().clone()
        }

        async fn create_tunnel_with_intent(
            &self,
            _local_identity: &P2pIdentityRef,
            _remote: &Endpoint,
            _remote_id: &P2pId,
            _remote_name: Option<String>,
            _intent: crate::networks::TunnelConnectIntent,
        ) -> P2pResult<crate::networks::TunnelRef> {
            Err(p2p_err!(P2pErrorCode::NotSupport, "unused in test"))
        }

        async fn create_tunnel_with_local_ep_and_intent(
            &self,
            _local_identity: &P2pIdentityRef,
            _local_ep: &Endpoint,
            _remote: &Endpoint,
            _remote_id: &P2pId,
            _remote_name: Option<String>,
            _intent: crate::networks::TunnelConnectIntent,
        ) -> P2pResult<crate::networks::TunnelRef> {
            Err(p2p_err!(P2pErrorCode::NotSupport, "unused in test"))
        }
    }

    fn test_identity(local_ep: Endpoint) -> P2pIdentityRef {
        Arc::new(DummyIdentity {
            id: P2pId::from(vec![1u8; 32]),
            name: "pn-server-test".to_owned(),
            endpoints: vec![local_ep],
        })
    }

    fn make_stream_pair() -> (
        (TunnelStreamRead, TunnelStreamWrite),
        (ReadHalf<DuplexStream>, WriteHalf<DuplexStream>),
    ) {
        let (test_end, tunnel_end) = tokio::io::duplex(256);
        let (test_read, test_write) = split(test_end);
        let (tunnel_read, tunnel_write) = split(tunnel_end);
        (
            (Box::pin(tunnel_read), Box::pin(tunnel_write)),
            (test_read, test_write),
        )
    }

    struct FakeTargetStreamFactory {
        target_stream: StdMutex<Option<(TunnelStreamRead, TunnelStreamWrite)>>,
        opened_target: StdMutex<Option<P2pId>>,
    }

    impl FakeTargetStreamFactory {
        fn new(target_stream: (TunnelStreamRead, TunnelStreamWrite)) -> Arc<Self> {
            Arc::new(Self {
                target_stream: StdMutex::new(Some(target_stream)),
                opened_target: StdMutex::new(None),
            })
        }

        fn opened_target(&self) -> Option<P2pId> {
            self.opened_target.lock().unwrap().clone()
        }
    }

    #[async_trait::async_trait]
    impl PnTargetStreamFactory for FakeTargetStreamFactory {
        async fn open_target_stream(
            &self,
            target: &P2pId,
        ) -> P2pResult<(TunnelStreamRead, TunnelStreamWrite)> {
            *self.opened_target.lock().unwrap() = Some(target.clone());
            self.target_stream
                .lock()
                .unwrap()
                .take()
                .ok_or_else(|| p2p_err!(P2pErrorCode::Interrupted, "target stream already opened"))
        }
    }

    struct TestPnConnectionValidator {
        decision: ValidateResult,
        last_ctx: StdMutex<Option<PnConnectionValidateContext>>,
    }

    impl TestPnConnectionValidator {
        fn new(decision: ValidateResult) -> Arc<Self> {
            Arc::new(Self {
                decision,
                last_ctx: StdMutex::new(None),
            })
        }

        fn last_ctx(&self) -> Option<PnConnectionValidateContext> {
            self.last_ctx.lock().unwrap().clone()
        }
    }

    #[async_trait::async_trait]
    impl PnConnectionValidator for TestPnConnectionValidator {
        async fn validate(&self, ctx: &PnConnectionValidateContext) -> P2pResult<ValidateResult> {
            *self.last_ctx.lock().unwrap() = Some(ctx.clone());
            match &self.decision {
                ValidateResult::Accept => Ok(ValidateResult::Accept),
                ValidateResult::Reject(reason) => Ok(ValidateResult::Reject(reason.clone())),
            }
        }
    }

    fn test_connection_validate_context() -> PnConnectionValidateContext {
        PnConnectionValidateContext {
            from: P2pId::from(vec![2u8; 32]),
            to: P2pId::from(vec![3u8; 32]),
            tunnel_id: TunnelId::from(24),
            kind: PnChannelKind::Stream,
            purpose: crate::networks::TunnelPurpose::from_value(&2000u16).unwrap(),
            is_control: false,
        }
    }

    #[tokio::test]
    async fn default_pn_connection_validator_allows_connections() {
        let result = allow_all_pn_connection_validator()
            .validate(&test_connection_validate_context())
            .await
            .unwrap();

        assert!(matches!(result, ValidateResult::Accept));
    }

    #[tokio::test]
    async fn reject_all_pn_connection_validator_rejects_connections() {
        let result = reject_all_pn_connection_validator()
            .validate(&test_connection_validate_context())
            .await
            .unwrap();

        assert!(matches!(result, ValidateResult::Reject(_)));
    }

    #[tokio::test]
    async fn pn_service_uses_injected_target_stream_factory() {
        let source_id = P2pId::from(vec![2u8; 32]);
        let target_id = P2pId::from(vec![3u8; 32]);
        let ((service_target_read, service_target_write), (mut target_read, mut target_write)) =
            make_stream_pair();
        let target_stream_factory =
            FakeTargetStreamFactory::new((service_target_read, service_target_write));
        let service = PnService::new(
            target_stream_factory.clone(),
            TestPnConnectionValidator::new(ValidateResult::Accept),
        );

        let ((service_source_read, service_source_write), (mut source_read, mut source_write)) =
            make_stream_pair();
        let req = ProxyOpenReq {
            tunnel_id: TunnelId::from(24),
            from: P2pId::default(),
            to: target_id.clone(),
            kind: PnChannelKind::Stream,
            purpose: crate::networks::TunnelPurpose::from_value(&2000u16).unwrap(),
        };

        let service_task = tokio::spawn({
            let service = service.clone();
            let source_id = source_id.clone();
            let req = req.clone();
            async move {
                service
                    .handle_proxy_open_req(
                        source_id,
                        req,
                        service_source_read,
                        service_source_write,
                    )
                    .await;
            }
        });

        let target_header = read_tunnel_command_header(&mut target_read).await.unwrap();
        let target_req =
            read_tunnel_command_body::<_, ProxyOpenReq>(&mut target_read, target_header)
                .await
                .unwrap()
                .body;
        assert_eq!(
            target_stream_factory.opened_target(),
            Some(target_id.clone())
        );
        assert_eq!(target_req.tunnel_id, req.tunnel_id);
        assert_eq!(target_req.from, source_id);
        assert_eq!(target_req.to, target_id);

        let resp = TunnelCommand::new(ProxyOpenResp {
            tunnel_id: req.tunnel_id,
            result: TunnelCommandResult::Success as u8,
        })
        .unwrap();
        write_tunnel_command(&mut target_write, &resp)
            .await
            .unwrap();

        let source_header = read_tunnel_command_header(&mut source_read).await.unwrap();
        let source_resp =
            read_tunnel_command_body::<_, ProxyOpenResp>(&mut source_read, source_header)
                .await
                .unwrap()
                .body;
        assert_eq!(source_resp.tunnel_id, req.tunnel_id);
        assert_eq!(source_resp.result, TunnelCommandResult::Success as u8);

        source_write.write_all(b"ping").await.unwrap();
        let mut ping_buf = [0u8; 4];
        target_read.read_exact(&mut ping_buf).await.unwrap();
        assert_eq!(&ping_buf, b"ping");

        target_write.write_all(b"pong").await.unwrap();
        let mut pong_buf = [0u8; 4];
        source_read.read_exact(&mut pong_buf).await.unwrap();
        assert_eq!(&pong_buf, b"pong");

        drop(source_write);
        drop(target_write);
        drop(source_read);
        drop(target_read);
        timeout(Duration::from_secs(1), service_task)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn pn_service_rejects_proxy_open_when_validator_rejects() {
        let source_id = P2pId::from(vec![2u8; 32]);
        let target_id = P2pId::from(vec![3u8; 32]);
        let ((service_target_read, service_target_write), _) = make_stream_pair();
        let target_stream_factory =
            FakeTargetStreamFactory::new((service_target_read, service_target_write));
        let validator = TestPnConnectionValidator::new(ValidateResult::Reject(
            "source peer is not allowed".to_owned(),
        ));
        let service = PnService::new(target_stream_factory.clone(), validator.clone());

        let ((service_source_read, service_source_write), (mut source_read, _source_write)) =
            make_stream_pair();
        let req = ProxyOpenReq {
            tunnel_id: TunnelId::from(25),
            from: P2pId::default(),
            to: target_id.clone(),
            kind: PnChannelKind::Stream,
            purpose: crate::networks::TunnelPurpose::from_value(&2001u16).unwrap(),
        };

        service
            .handle_proxy_open_req(
                source_id.clone(),
                req.clone(),
                service_source_read,
                service_source_write,
            )
            .await;

        assert_eq!(target_stream_factory.opened_target(), None);

        let ctx = validator.last_ctx().unwrap();
        assert_eq!(ctx.from, source_id);
        assert_eq!(ctx.to, target_id);
        assert_eq!(ctx.tunnel_id, req.tunnel_id);
        assert_eq!(ctx.kind, req.kind);
        assert_eq!(ctx.purpose, req.purpose);
        assert!(!ctx.is_control);

        let source_header = read_tunnel_command_header(&mut source_read).await.unwrap();
        let source_resp =
            read_tunnel_command_body::<_, ProxyOpenResp>(&mut source_read, source_header)
                .await
                .unwrap()
                .body;
        assert_eq!(source_resp.tunnel_id, req.tunnel_id);
        assert_eq!(source_resp.result, TunnelCommandResult::InvalidParam as u8);
    }

    #[tokio::test]
    async fn pn_service_rejects_proxy_control_open_when_validator_rejects() {
        let source_id = P2pId::from(vec![10u8; 32]);
        let target_id = P2pId::from(vec![11u8; 32]);
        let ((service_target_read, service_target_write), _) = make_stream_pair();
        let target_stream_factory =
            FakeTargetStreamFactory::new((service_target_read, service_target_write));
        let validator = TestPnConnectionValidator::new(ValidateResult::Reject(
            "control not allowed".to_owned(),
        ));
        let service = PnService::new(target_stream_factory.clone(), validator.clone());

        let ((service_source_read, service_source_write), (mut source_read, _source_write)) =
            make_stream_pair();
        let req = ProxyControlOpenReq {
            tunnel_id: TunnelId::from(29),
            from: P2pId::default(),
            to: target_id.clone(),
        };

        service
            .handle_proxy_control_open_req(
                source_id.clone(),
                req.clone(),
                service_source_read,
                service_source_write,
            )
            .await;

        assert_eq!(target_stream_factory.opened_target(), None);
        let ctx = validator.last_ctx().unwrap();
        assert_eq!(ctx.from, source_id);
        assert_eq!(ctx.to, target_id);
        assert_eq!(ctx.tunnel_id, req.tunnel_id);
        assert_eq!(ctx.kind, PnChannelKind::Stream);
        assert_eq!(
            ctx.purpose,
            crate::networks::TunnelPurpose::from_value(&PROXY_SERVICE.to_string()).unwrap()
        );
        assert!(ctx.is_control);

        let source_header = read_tunnel_command_header(&mut source_read).await.unwrap();
        let source_resp =
            read_tunnel_command_body::<_, ProxyControlOpenResp>(&mut source_read, source_header)
                .await
                .unwrap()
                .body;
        assert_eq!(source_resp.tunnel_id, req.tunnel_id);
        assert_eq!(source_resp.result, TunnelCommandResult::InvalidParam as u8);
    }

    #[tokio::test]
    async fn pn_service_tracks_and_limits_control_bridge_bytes() {
        let source_id = P2pId::from(vec![12u8; 32]);
        let target_id = P2pId::from(vec![13u8; 32]);
        let ((service_target_read, service_target_write), (mut target_read, mut target_write)) =
            make_stream_pair();
        let target_stream_factory =
            FakeTargetStreamFactory::new((service_target_read, service_target_write));
        let traffic_manager = PnTrafficManager::new();
        traffic_manager.set_user_limit(
            source_id.clone(),
            PnTrafficLimitConfig {
                tx_rate: Some(NonZeroU32::new(10).unwrap()),
                tx_weight: Some(NonZeroU32::new(1).unwrap()),
                rx_rate: None,
                rx_weight: None,
            },
        );
        let service = PnService::new_with_traffic_manager(
            target_stream_factory,
            TestPnConnectionValidator::new(ValidateResult::Accept),
            traffic_manager.clone(),
        );

        let ((service_source_read, service_source_write), (mut source_read, mut source_write)) =
            make_stream_pair();
        let req = ProxyControlOpenReq {
            tunnel_id: TunnelId::from(30),
            from: P2pId::default(),
            to: target_id.clone(),
        };

        let service_task = tokio::spawn({
            let service = service.clone();
            let source_id = source_id.clone();
            let req = req.clone();
            async move {
                service
                    .handle_proxy_control_open_req(
                        source_id,
                        req,
                        service_source_read,
                        service_source_write,
                    )
                    .await;
            }
        });

        let target_header = read_tunnel_command_header(&mut target_read).await.unwrap();
        let target_req =
            read_tunnel_command_body::<_, ProxyControlOpenReq>(&mut target_read, target_header)
                .await
                .unwrap()
                .body;
        assert_eq!(target_req.from, source_id);
        assert_eq!(target_req.to, target_id);

        let resp = TunnelCommand::new(ProxyControlOpenResp {
            tunnel_id: req.tunnel_id,
            result: TunnelCommandResult::Success as u8,
        })
        .unwrap();
        write_tunnel_command(&mut target_write, &resp)
            .await
            .unwrap();

        let source_header = read_tunnel_command_header(&mut source_read).await.unwrap();
        let source_resp =
            read_tunnel_command_body::<_, ProxyControlOpenResp>(&mut source_read, source_header)
                .await
                .unwrap()
                .body;
        assert_eq!(source_resp.result, TunnelCommandResult::Success as u8);

        let payload = vec![b'c'; 20];
        source_write.write_all(payload.as_slice()).await.unwrap();
        let started_at = Instant::now();
        let mut received = vec![0u8; payload.len()];
        target_read
            .read_exact(received.as_mut_slice())
            .await
            .unwrap();
        assert_eq!(received, payload);
        assert!(
            started_at.elapsed() >= Duration::from_millis(700),
            "expected source control limit to delay bridge"
        );

        let reply = b"control";
        target_write.write_all(reply).await.unwrap();
        let mut reply_buf = vec![0u8; reply.len()];
        source_read
            .read_exact(reply_buf.as_mut_slice())
            .await
            .unwrap();
        assert_eq!(reply_buf, reply);

        let source_snapshot = traffic_manager.snapshot(&source_id).unwrap();
        assert_eq!(source_snapshot.tx_bytes, payload.len() as u64);
        assert_eq!(source_snapshot.rx_bytes, reply.len() as u64);
        let target_snapshot = traffic_manager.snapshot(&target_id).unwrap();
        assert_eq!(target_snapshot.tx_bytes, reply.len() as u64);
        assert_eq!(target_snapshot.rx_bytes, payload.len() as u64);

        drop(source_write);
        drop(target_write);
        drop(source_read);
        drop(target_read);
        timeout(Duration::from_secs(3), service_task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(traffic_manager.snapshot(&source_id), None);
        assert_eq!(traffic_manager.snapshot(&target_id), None);
    }

    #[tokio::test]
    async fn pn_service_tracks_user_traffic_with_normalized_source_id() {
        let source_id = P2pId::from(vec![4u8; 32]);
        let forged_source_id = P2pId::from(vec![9u8; 32]);
        let target_id = P2pId::from(vec![5u8; 32]);
        let ((service_target_read, service_target_write), (mut target_read, mut target_write)) =
            make_stream_pair();
        let target_stream_factory =
            FakeTargetStreamFactory::new((service_target_read, service_target_write));
        let traffic_manager = PnTrafficManager::new();
        let service = PnService::new_with_traffic_manager(
            target_stream_factory,
            TestPnConnectionValidator::new(ValidateResult::Accept),
            traffic_manager.clone(),
        );

        let ((service_source_read, service_source_write), (mut source_read, mut source_write)) =
            make_stream_pair();
        let req = ProxyOpenReq {
            tunnel_id: TunnelId::from(26),
            from: forged_source_id.clone(),
            to: target_id.clone(),
            kind: PnChannelKind::Stream,
            purpose: crate::networks::TunnelPurpose::from_value(&2002u16).unwrap(),
        };

        let service_task = tokio::spawn({
            let service = service.clone();
            let source_id = source_id.clone();
            let req = req.clone();
            async move {
                service
                    .handle_proxy_open_req(
                        source_id,
                        req,
                        service_source_read,
                        service_source_write,
                    )
                    .await;
            }
        });

        let target_header = read_tunnel_command_header(&mut target_read).await.unwrap();
        let target_req =
            read_tunnel_command_body::<_, ProxyOpenReq>(&mut target_read, target_header)
                .await
                .unwrap()
                .body;
        assert_eq!(target_req.from, source_id);
        assert_ne!(target_req.from, forged_source_id);

        let resp = TunnelCommand::new(ProxyOpenResp {
            tunnel_id: req.tunnel_id,
            result: TunnelCommandResult::Success as u8,
        })
        .unwrap();
        write_tunnel_command(&mut target_write, &resp)
            .await
            .unwrap();

        let source_header = read_tunnel_command_header(&mut source_read).await.unwrap();
        let source_resp =
            read_tunnel_command_body::<_, ProxyOpenResp>(&mut source_read, source_header)
                .await
                .unwrap()
                .body;
        assert_eq!(source_resp.result, TunnelCommandResult::Success as u8);

        let source_payload = b"ping";
        source_write.write_all(source_payload).await.unwrap();
        let mut ping_buf = [0u8; 4];
        target_read.read_exact(&mut ping_buf).await.unwrap();
        assert_eq!(&ping_buf, source_payload);

        let target_payload = b"answer";
        target_write.write_all(target_payload).await.unwrap();
        let mut pong_buf = [0u8; 6];
        source_read.read_exact(&mut pong_buf).await.unwrap();
        assert_eq!(&pong_buf, target_payload);

        let snapshot = traffic_manager.snapshot(&source_id).unwrap();
        assert_eq!(
            snapshot,
            PnUserTrafficSnapshot {
                tx_bytes: 4,
                tx_speed: 0,
                rx_bytes: 6,
                rx_speed: 0,
                tx_delta_bytes: 4,
                rx_delta_bytes: 6,
            }
        );
        let target_snapshot = traffic_manager.snapshot(&target_id).unwrap();
        assert_eq!(
            target_snapshot,
            PnUserTrafficSnapshot {
                tx_bytes: 6,
                tx_speed: 0,
                rx_bytes: 4,
                rx_speed: 0,
                tx_delta_bytes: 6,
                rx_delta_bytes: 4,
            }
        );
        assert_eq!(traffic_manager.snapshot(&forged_source_id), None);

        drop(source_write);
        drop(target_write);
        drop(source_read);
        drop(target_read);
        timeout(Duration::from_secs(1), service_task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(traffic_manager.snapshot(&source_id), None);
        assert_eq!(traffic_manager.snapshot(&target_id), None);
        assert_eq!(traffic_manager.snapshot(&forged_source_id), None);
    }

    #[tokio::test]
    async fn pn_service_applies_user_rate_limit() {
        let source_id = P2pId::from(vec![6u8; 32]);
        let target_id = P2pId::from(vec![7u8; 32]);
        let ((service_target_read, service_target_write), (mut target_read, mut target_write)) =
            make_stream_pair();
        let target_stream_factory =
            FakeTargetStreamFactory::new((service_target_read, service_target_write));
        let traffic_manager = PnTrafficManager::new();
        traffic_manager.set_user_limit(
            source_id.clone(),
            PnTrafficLimitConfig {
                tx_rate: Some(NonZeroU32::new(10).unwrap()),
                tx_weight: Some(NonZeroU32::new(1).unwrap()),
                rx_rate: None,
                rx_weight: None,
            },
        );
        let service = PnService::new_with_traffic_manager(
            target_stream_factory,
            TestPnConnectionValidator::new(ValidateResult::Accept),
            traffic_manager.clone(),
        );

        let ((service_source_read, service_source_write), (mut source_read, mut source_write)) =
            make_stream_pair();
        let req = ProxyOpenReq {
            tunnel_id: TunnelId::from(27),
            from: source_id.clone(),
            to: target_id,
            kind: PnChannelKind::Stream,
            purpose: crate::networks::TunnelPurpose::from_value(&2003u16).unwrap(),
        };

        let service_task = tokio::spawn({
            let service = service.clone();
            let source_id = source_id.clone();
            let req = req.clone();
            async move {
                service
                    .handle_proxy_open_req(
                        source_id,
                        req,
                        service_source_read,
                        service_source_write,
                    )
                    .await;
            }
        });

        let target_header = read_tunnel_command_header(&mut target_read).await.unwrap();
        let _target_req =
            read_tunnel_command_body::<_, ProxyOpenReq>(&mut target_read, target_header)
                .await
                .unwrap()
                .body;

        let resp = TunnelCommand::new(ProxyOpenResp {
            tunnel_id: req.tunnel_id,
            result: TunnelCommandResult::Success as u8,
        })
        .unwrap();
        write_tunnel_command(&mut target_write, &resp)
            .await
            .unwrap();

        let source_header = read_tunnel_command_header(&mut source_read).await.unwrap();
        let source_resp =
            read_tunnel_command_body::<_, ProxyOpenResp>(&mut source_read, source_header)
                .await
                .unwrap()
                .body;
        assert_eq!(source_resp.result, TunnelCommandResult::Success as u8);

        let payload = vec![b'x'; 20];
        source_write.write_all(payload.as_slice()).await.unwrap();

        let started_at = Instant::now();
        let mut received = vec![0u8; payload.len()];
        target_read
            .read_exact(received.as_mut_slice())
            .await
            .unwrap();
        let elapsed = started_at.elapsed();
        assert_eq!(received, payload);
        assert!(
            elapsed >= Duration::from_millis(700),
            "expected tx limit to delay bridge, got {:?}",
            elapsed
        );

        let snapshot = traffic_manager.snapshot(&source_id).unwrap();
        assert_eq!(snapshot.tx_bytes, payload.len() as u64);
        assert_eq!(snapshot.rx_bytes, 0);
        let target_snapshot = traffic_manager.snapshot(&req.to).unwrap();
        assert_eq!(target_snapshot.tx_bytes, 0);
        assert_eq!(target_snapshot.rx_bytes, payload.len() as u64);

        drop(source_write);
        drop(target_write);
        drop(source_read);
        drop(target_read);
        timeout(Duration::from_secs(3), service_task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(traffic_manager.snapshot(&source_id), None);
        assert_eq!(traffic_manager.snapshot(&req.to), None);
    }

    #[tokio::test]
    async fn pn_service_target_user_limit_does_not_throttle_bridge() {
        let source_id = P2pId::from(vec![8u8; 32]);
        let target_id = P2pId::from(vec![9u8; 32]);
        let ((service_target_read, service_target_write), (mut target_read, mut target_write)) =
            make_stream_pair();
        let target_stream_factory =
            FakeTargetStreamFactory::new((service_target_read, service_target_write));
        let traffic_manager = PnTrafficManager::new();
        traffic_manager.set_user_limit(
            target_id.clone(),
            PnTrafficLimitConfig {
                tx_rate: Some(NonZeroU32::new(10).unwrap()),
                tx_weight: Some(NonZeroU32::new(1).unwrap()),
                rx_rate: None,
                rx_weight: None,
            },
        );
        let service = PnService::new_with_traffic_manager(
            target_stream_factory,
            TestPnConnectionValidator::new(ValidateResult::Accept),
            traffic_manager.clone(),
        );

        let ((service_source_read, service_source_write), (mut source_read, mut source_write)) =
            make_stream_pair();
        let req = ProxyOpenReq {
            tunnel_id: TunnelId::from(28),
            from: source_id.clone(),
            to: target_id.clone(),
            kind: PnChannelKind::Stream,
            purpose: crate::networks::TunnelPurpose::from_value(&2004u16).unwrap(),
        };

        let service_task = tokio::spawn({
            let service = service.clone();
            let source_id = source_id.clone();
            let req = req.clone();
            async move {
                service
                    .handle_proxy_open_req(
                        source_id,
                        req,
                        service_source_read,
                        service_source_write,
                    )
                    .await;
            }
        });

        let target_header = read_tunnel_command_header(&mut target_read).await.unwrap();
        let _target_req =
            read_tunnel_command_body::<_, ProxyOpenReq>(&mut target_read, target_header)
                .await
                .unwrap()
                .body;

        let resp = TunnelCommand::new(ProxyOpenResp {
            tunnel_id: req.tunnel_id,
            result: TunnelCommandResult::Success as u8,
        })
        .unwrap();
        write_tunnel_command(&mut target_write, &resp)
            .await
            .unwrap();

        let source_header = read_tunnel_command_header(&mut source_read).await.unwrap();
        let source_resp =
            read_tunnel_command_body::<_, ProxyOpenResp>(&mut source_read, source_header)
                .await
                .unwrap()
                .body;
        assert_eq!(source_resp.result, TunnelCommandResult::Success as u8);

        let payload = vec![b'y'; 20];
        target_write.write_all(payload.as_slice()).await.unwrap();

        let started_at = Instant::now();
        let mut received = vec![0u8; payload.len()];
        source_read
            .read_exact(received.as_mut_slice())
            .await
            .unwrap();
        let elapsed = started_at.elapsed();
        assert_eq!(received, payload);
        assert!(
            elapsed < Duration::from_millis(300),
            "target user limit should not throttle bridge, got {:?}",
            elapsed
        );

        let source_snapshot = traffic_manager.snapshot(&source_id).unwrap();
        assert_eq!(source_snapshot.tx_bytes, 0);
        assert_eq!(source_snapshot.rx_bytes, payload.len() as u64);
        let target_snapshot = traffic_manager.snapshot(&target_id).unwrap();
        assert_eq!(target_snapshot.tx_bytes, payload.len() as u64);
        assert_eq!(target_snapshot.rx_bytes, 0);

        drop(source_write);
        drop(target_write);
        drop(source_read);
        drop(target_read);
        timeout(Duration::from_secs(1), service_task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(traffic_manager.snapshot(&source_id), None);
        assert_eq!(traffic_manager.snapshot(&target_id), None);
    }

    #[tokio::test]
    async fn pn_server_listens_and_bridges_proxy_stream() {
        let local_ep = Endpoint::from((Protocol::Quic, "127.0.0.1:23101".parse().unwrap()));
        let source_ep = Endpoint::from((Protocol::Quic, "127.0.0.1:23102".parse().unwrap()));
        let target_ep = Endpoint::from((Protocol::Quic, "127.0.0.1:23103".parse().unwrap()));
        let identity = test_identity(local_ep);
        let source_id = P2pId::from(vec![2u8; 32]);
        let target_id = P2pId::from(vec![3u8; 32]);

        let fake_network = FakeTunnelNetwork::new(Protocol::Quic);
        let net_manager = NetManager::new(
            vec![fake_network.clone() as TunnelNetworkRef],
            DefaultTlsServerCertResolver::new(),
        )
        .unwrap();
        net_manager.listen(&[local_ep], None).await.unwrap();
        let ttp_server = TtpServer::new(identity.clone(), net_manager).unwrap();

        let pn_server = PnServer::new_with_connection_validator(
            ttp_server.clone(),
            TestPnConnectionValidator::new(ValidateResult::Accept),
        );
        pn_server.start().await.unwrap();

        let (target_tunnel, mut target_open_rx, target_attached) = FakeTunnel::new_with_open_stream(
            identity.get_id(),
            target_id.clone(),
            local_ep,
            target_ep,
        );
        fake_network.push_tunnel(target_tunnel).unwrap();
        timeout(Duration::from_secs(1), target_attached)
            .await
            .unwrap()
            .unwrap();

        let (source_tunnel, source_stream_tx, source_attached) =
            FakeTunnel::new(identity.get_id(), source_id.clone(), local_ep, source_ep);
        fake_network.push_tunnel(source_tunnel).unwrap();
        timeout(Duration::from_secs(1), source_attached)
            .await
            .unwrap()
            .unwrap();

        let ((server_read, server_write), (mut source_read, mut source_write)) = make_stream_pair();
        source_stream_tx
            .try_send((
                crate::networks::TunnelPurpose::from_value(&PROXY_SERVICE.to_string()).unwrap(),
                server_read,
                server_write,
            ))
            .map_err(|err| {
                p2p_err!(
                    P2pErrorCode::OutOfLimit,
                    "test source stream queue failed: {}",
                    err
                )
            })
            .unwrap();

        let req = ProxyOpenReq {
            tunnel_id: TunnelId::from(42),
            from: source_id.clone(),
            to: target_id.clone(),
            kind: PnChannelKind::Stream,
            purpose: crate::networks::TunnelPurpose::from_value(&2000u16).unwrap(),
        };
        let command = TunnelCommand::new(req.clone()).unwrap();
        write_tunnel_command(&mut source_write, &command)
            .await
            .unwrap();

        let (purpose, mut target_read, mut target_write) =
            timeout(Duration::from_secs(1), target_open_rx.recv())
                .await
                .unwrap()
                .unwrap();
        assert_eq!(
            purpose,
            crate::networks::TunnelPurpose::from_value(&PROXY_SERVICE.to_string()).unwrap()
        );

        let target_header = read_tunnel_command_header(&mut target_read).await.unwrap();
        let target_req =
            read_tunnel_command_body::<_, ProxyOpenReq>(&mut target_read, target_header)
                .await
                .unwrap()
                .body;
        assert_eq!(target_req.tunnel_id, req.tunnel_id);
        assert_eq!(target_req.from, source_id);
        assert_eq!(target_req.to, target_id);

        let resp = TunnelCommand::new(ProxyOpenResp {
            tunnel_id: req.tunnel_id,
            result: TunnelCommandResult::Success as u8,
        })
        .unwrap();
        write_tunnel_command(&mut target_write, &resp)
            .await
            .unwrap();

        let source_header = read_tunnel_command_header(&mut source_read).await.unwrap();
        let source_resp =
            read_tunnel_command_body::<_, ProxyOpenResp>(&mut source_read, source_header)
                .await
                .unwrap()
                .body;
        assert_eq!(source_resp.tunnel_id, req.tunnel_id);
        assert_eq!(source_resp.result, TunnelCommandResult::Success as u8);

        source_write.write_all(b"ping").await.unwrap();
        let mut ping_buf = [0u8; 4];
        target_read.read_exact(&mut ping_buf).await.unwrap();
        assert_eq!(&ping_buf, b"ping");

        target_write.write_all(b"pong").await.unwrap();
        let mut pong_buf = [0u8; 4];
        source_read.read_exact(&mut pong_buf).await.unwrap();
        assert_eq!(&pong_buf, b"pong");
    }
}
