use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, RwLock, Weak};
use std::time::{Duration, Instant};
use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

use tokio::sync::Notify;

use crate::endpoint::{Endpoint, Protocol};
use crate::error::{P2pError, P2pErrorCode, P2pResult, p2p_err};
use crate::networks::{
    ListenVPortsRef, Tunnel, TunnelCommandResult, TunnelDatagramRead, TunnelDatagramWrite,
    TunnelForm, TunnelPurpose, TunnelState, TunnelStreamRead, TunnelStreamWrite,
    validate_server_name,
};
use crate::p2p_identity::{P2pId, P2pIdentityCertFactoryRef, P2pIdentityRef};
use crate::pn::{PnChannelKind, ProxyOpenReq, ProxyOpenResp};
use crate::runtime;
use crate::tls::{DefaultTlsServerCertResolver, TlsServerCertResolver};
use crate::types::{TunnelCandidateId, TunnelId};
use rustls::ServerConfig;
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
use rustls::version::TLS13;

use super::pn_client::{PnShared, read_pn_command, write_pn_command};

const FIRST_LISTEN_WAIT_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(300);
pub(super) const DEFAULT_PN_TUNNEL_IDLE_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const FIRST_LISTEN_WAIT_NOT_STARTED: u8 = 0;
const FIRST_LISTEN_WAIT_IN_PROGRESS: u8 = 1;
const FIRST_LISTEN_WAIT_DONE: u8 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PnProxyStreamSecurityMode {
    Disabled,
    TlsRequired,
}

impl PnProxyStreamSecurityMode {
    pub(super) fn to_atomic(self) -> u8 {
        match self {
            Self::Disabled => 0,
            Self::TlsRequired => 1,
        }
    }

    pub(super) fn from_atomic(value: u8) -> Self {
        match value {
            1 => Self::TlsRequired,
            _ => Self::Disabled,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PnTunnelOptions {
    pub stream_security_mode: PnProxyStreamSecurityMode,
}

impl Default for PnTunnelOptions {
    fn default() -> Self {
        Self {
            stream_security_mode: PnProxyStreamSecurityMode::Disabled,
        }
    }
}

#[derive(Clone)]
pub(super) struct PnTlsContext {
    pub(super) local_identity: P2pIdentityRef,
    pub(super) cert_factory: P2pIdentityCertFactoryRef,
}

struct ProxyTlsIo {
    read: TunnelStreamRead,
    write: TunnelStreamWrite,
}

impl ProxyTlsIo {
    fn new(read: TunnelStreamRead, write: TunnelStreamWrite) -> Self {
        Self { read, write }
    }
}

impl tokio::io::AsyncRead for ProxyTlsIo {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.read).poll_read(cx, buf)
    }
}

impl tokio::io::AsyncWrite for ProxyTlsIo {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.write).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.write).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.write).poll_shutdown(cx)
    }
}

struct PassivePnChannel {
    request: ProxyOpenReq,
    read: TunnelStreamRead,
    write: TunnelStreamWrite,
}

pub(super) struct RejectedPassivePnChannel {
    pub(super) request: ProxyOpenReq,
    pub(super) read: TunnelStreamRead,
    pub(super) write: TunnelStreamWrite,
    pub(super) error: P2pError,
}

impl std::fmt::Debug for RejectedPassivePnChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RejectedPassivePnChannel")
            .field("tunnel_id", &self.request.tunnel_id)
            .field("from", &self.request.from)
            .field("to", &self.request.to)
            .field("kind", &self.request.kind)
            .field("purpose", &self.request.purpose)
            .field("error", &self.error)
            .finish()
    }
}

struct PnInboundChannels {
    stream_channels: Mutex<VecDeque<PassivePnChannel>>,
    datagram_channels: Mutex<VecDeque<PassivePnChannel>>,
    channel_notify: Notify,
}

impl PnInboundChannels {
    fn new() -> Self {
        Self {
            stream_channels: Mutex::new(VecDeque::new()),
            datagram_channels: Mutex::new(VecDeque::new()),
            channel_notify: Notify::new(),
        }
    }

    fn with_first(first: PassivePnChannel) -> Self {
        let mut stream_channels = VecDeque::new();
        let mut datagram_channels = VecDeque::new();
        match first.request.kind {
            PnChannelKind::Stream => stream_channels.push_back(first),
            PnChannelKind::Datagram => datagram_channels.push_back(first),
        }
        Self {
            stream_channels: Mutex::new(stream_channels),
            datagram_channels: Mutex::new(datagram_channels),
            channel_notify: Notify::new(),
        }
    }

    fn push(&self, channel: PassivePnChannel) {
        match channel.request.kind {
            PnChannelKind::Stream => self.stream_channels.lock().unwrap().push_back(channel),
            PnChannelKind::Datagram => self.datagram_channels.lock().unwrap().push_back(channel),
        }
        self.channel_notify.notify_waiters();
    }

    fn pop(&self, kind: PnChannelKind) -> Option<PassivePnChannel> {
        match kind {
            PnChannelKind::Stream => self.stream_channels.lock().unwrap().pop_front(),
            PnChannelKind::Datagram => self.datagram_channels.lock().unwrap().pop_front(),
        }
    }

    fn drain(&self) {
        self.stream_channels.lock().unwrap().clear();
        self.datagram_channels.lock().unwrap().clear();
        self.channel_notify.notify_waiters();
    }

    fn drain_all(&self) -> Vec<PassivePnChannel> {
        let mut drained = Vec::new();
        drained.extend(self.stream_channels.lock().unwrap().drain(..));
        drained.extend(self.datagram_channels.lock().unwrap().drain(..));
        self.channel_notify.notify_waiters();
        drained
    }
}

#[derive(Clone, Copy)]
enum PnTunnelCloseReason {
    Manual,
    IdleTimeout,
}

struct PnTunnelLifecycleState {
    active_channels: usize,
    pending_channels: usize,
    queued_channels: usize,
    zero_since: Option<Instant>,
    generation: u64,
    close_reason: Option<PnTunnelCloseReason>,
}

impl PnTunnelLifecycleState {
    fn new(queued_channels: usize) -> Self {
        Self {
            active_channels: 0,
            pending_channels: 0,
            queued_channels,
            zero_since: if queued_channels == 0 {
                Some(Instant::now())
            } else {
                None
            },
            generation: 0,
            close_reason: None,
        }
    }

    fn is_open(&self) -> bool {
        self.close_reason.is_none()
    }

    fn total_channels(&self) -> usize {
        self.active_channels + self.pending_channels + self.queued_channels
    }

    fn refresh_idle_state(&mut self) -> Option<(Instant, u64)> {
        if !self.is_open() {
            return None;
        }
        if self.total_channels() == 0 {
            if self.zero_since.is_none() {
                self.zero_since = Some(Instant::now());
                self.generation = self.generation.wrapping_add(1);
                return Some((self.zero_since.unwrap(), self.generation));
            }
        } else if self.zero_since.take().is_some() {
            self.generation = self.generation.wrapping_add(1);
        }
        None
    }
}

struct PnChannelLease {
    tunnel: Weak<PnTunnel>,
}

impl Drop for PnChannelLease {
    fn drop(&mut self) {
        if let Some(tunnel) = self.tunnel.upgrade() {
            tunnel.release_active_channel();
        }
    }
}

struct PnPendingChannel {
    tunnel: Weak<PnTunnel>,
    released: bool,
}

impl PnPendingChannel {
    fn new(tunnel: Weak<PnTunnel>) -> Self {
        Self {
            tunnel,
            released: false,
        }
    }

    fn into_active(mut self, tunnel: &PnTunnel) -> P2pResult<Arc<PnChannelLease>> {
        self.released = true;
        tunnel.pending_to_active_channel()
    }
}

impl Drop for PnPendingChannel {
    fn drop(&mut self) {
        if self.released {
            return;
        }
        if let Some(tunnel) = self.tunnel.upgrade() {
            tunnel.release_pending_channel();
        }
    }
}

struct LeaseRead {
    inner: TunnelStreamRead,
    _lease: Arc<PnChannelLease>,
}

impl tokio::io::AsyncRead for LeaseRead {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

struct LeaseWrite {
    inner: TunnelStreamWrite,
    _lease: Arc<PnChannelLease>,
}

impl tokio::io::AsyncWrite for LeaseWrite {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

enum PnTunnelRole {
    Active { network: Arc<PnShared> },
    Passive { network: Option<Arc<PnShared>> },
}

pub struct PnTunnel {
    self_weak: Weak<PnTunnel>,
    tunnel_id: TunnelId,
    candidate_id: TunnelCandidateId,
    local_id: P2pId,
    remote_id: P2pId,
    role: PnTunnelRole,
    inbound_channels: PnInboundChannels,
    tls_context: Option<PnTlsContext>,
    stream_security_mode: PnProxyStreamSecurityMode,
    stream_vports: RwLock<Option<ListenVPortsRef>>,
    stream_first_listen_wait_state: AtomicU8,
    stream_vports_notify: Notify,
    datagram_vports: RwLock<Option<ListenVPortsRef>>,
    datagram_first_listen_wait_state: AtomicU8,
    datagram_vports_notify: Notify,
    lifecycle: Mutex<PnTunnelLifecycleState>,
    idle_timeout: Mutex<Option<Duration>>,
    closed: AtomicBool,
}

impl PnTunnel {
    pub(super) fn new_active(
        tunnel_id: TunnelId,
        candidate_id: TunnelCandidateId,
        local_id: P2pId,
        remote_id: P2pId,
        network: Arc<PnShared>,
        stream_security_mode: PnProxyStreamSecurityMode,
    ) -> Arc<Self> {
        let tls_context = network.tls_context();
        let idle_timeout = network.tunnel_idle_timeout();
        let tunnel = Arc::new_cyclic(|self_weak| Self {
            self_weak: self_weak.clone(),
            tunnel_id,
            candidate_id,
            local_id,
            remote_id,
            tls_context,
            stream_security_mode,
            role: PnTunnelRole::Active { network },
            inbound_channels: PnInboundChannels::new(),
            stream_vports: RwLock::new(None),
            stream_first_listen_wait_state: AtomicU8::new(FIRST_LISTEN_WAIT_NOT_STARTED),
            stream_vports_notify: Notify::new(),
            datagram_vports: RwLock::new(None),
            datagram_first_listen_wait_state: AtomicU8::new(FIRST_LISTEN_WAIT_NOT_STARTED),
            datagram_vports_notify: Notify::new(),
            lifecycle: Mutex::new(PnTunnelLifecycleState::new(0)),
            idle_timeout: Mutex::new(idle_timeout),
            closed: AtomicBool::new(false),
        });
        tunnel.schedule_idle_check_if_needed();
        tunnel
    }

    pub(super) fn new_passive(
        local_id: P2pId,
        request: ProxyOpenReq,
        read: TunnelStreamRead,
        write: TunnelStreamWrite,
        network: Option<Arc<PnShared>>,
        tls_context: Option<PnTlsContext>,
        stream_security_mode: PnProxyStreamSecurityMode,
    ) -> Arc<Self> {
        let idle_timeout = network
            .as_ref()
            .map(|network| network.tunnel_idle_timeout())
            .unwrap_or(Some(DEFAULT_PN_TUNNEL_IDLE_TIMEOUT));
        let tunnel = Arc::new_cyclic(|self_weak| Self {
            self_weak: self_weak.clone(),
            tunnel_id: request.tunnel_id,
            candidate_id: TunnelCandidateId::from(request.tunnel_id.value()),
            local_id,
            remote_id: request.from.clone(),
            tls_context,
            stream_security_mode,
            role: PnTunnelRole::Passive { network },
            inbound_channels: PnInboundChannels::with_first(PassivePnChannel {
                request,
                read,
                write,
            }),
            stream_vports: RwLock::new(None),
            stream_first_listen_wait_state: AtomicU8::new(FIRST_LISTEN_WAIT_NOT_STARTED),
            stream_vports_notify: Notify::new(),
            datagram_vports: RwLock::new(None),
            datagram_first_listen_wait_state: AtomicU8::new(FIRST_LISTEN_WAIT_NOT_STARTED),
            datagram_vports_notify: Notify::new(),
            lifecycle: Mutex::new(PnTunnelLifecycleState::new(1)),
            idle_timeout: Mutex::new(idle_timeout),
            closed: AtomicBool::new(false),
        });
        tunnel.schedule_idle_check_if_needed();
        tunnel
    }

    fn current_listen_vports(&self, kind: PnChannelKind) -> Option<ListenVPortsRef> {
        match kind {
            PnChannelKind::Stream => self.stream_vports.read().unwrap().clone(),
            PnChannelKind::Datagram => self.datagram_vports.read().unwrap().clone(),
        }
    }

    fn role_name(&self) -> &'static str {
        match &self.role {
            PnTunnelRole::Active { .. } => "active",
            PnTunnelRole::Passive { .. } => "passive",
        }
    }

    pub fn stream_security_mode(&self) -> PnProxyStreamSecurityMode {
        self.stream_security_mode
    }

    pub(super) fn is_closed_flag(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    fn shared(&self) -> Option<Arc<PnShared>> {
        match &self.role {
            PnTunnelRole::Active { network } => Some(network.clone()),
            PnTunnelRole::Passive {
                network: Some(network),
                ..
            } => Some(network.clone()),
            PnTunnelRole::Passive { network: None, .. } => None,
        }
    }

    fn schedule_idle_check_if_needed(self: &Arc<Self>) {
        let schedule = {
            let state = self.lifecycle.lock().unwrap();
            match (state.zero_since, *self.idle_timeout.lock().unwrap()) {
                (Some(zero_since), Some(timeout)) if state.is_open() => {
                    Some((zero_since, timeout, state.generation))
                }
                _ => None,
            }
        };
        if let Some((zero_since, timeout, generation)) = schedule {
            self.schedule_idle_check(zero_since, timeout, generation);
        }
    }

    fn schedule_idle_check(&self, zero_since: Instant, timeout: Duration, generation: u64) {
        let Some(this) = self.self_weak.upgrade() else {
            return;
        };
        let delay = zero_since
            .checked_add(timeout)
            .and_then(|deadline| deadline.checked_duration_since(Instant::now()))
            .unwrap_or(Duration::ZERO);
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                runtime::sleep(delay).await;
                this.close_if_idle(generation).await;
            });
        }
    }

    async fn close_if_idle(&self, generation: u64) {
        let drained = {
            let mut state = self.lifecycle.lock().unwrap();
            let Some(timeout) = *self.idle_timeout.lock().unwrap() else {
                return;
            };
            let Some(zero_since) = state.zero_since else {
                return;
            };
            let should_close = state.is_open()
                && state.generation == generation
                && state.total_channels() == 0
                && zero_since + timeout <= Instant::now();
            if should_close {
                self.close_lifecycle_locked(&mut state, PnTunnelCloseReason::IdleTimeout)
            } else {
                None
            }
        };
        if let Some(drained) = drained {
            self.finish_close(drained).await;
        }
    }

    fn schedule_from_refresh(&self, refresh: Option<(Instant, u64)>) {
        if let Some((zero_since, generation)) = refresh {
            if let Some(timeout) = *self.idle_timeout.lock().unwrap() {
                self.schedule_idle_check(zero_since, timeout, generation);
            }
        }
    }

    fn begin_pending_channel(&self) -> P2pResult<PnPendingChannel> {
        let mut state = self.lifecycle.lock().unwrap();
        if !state.is_open() {
            return Err(p2p_err!(P2pErrorCode::ErrorState, "pn tunnel closed"));
        }
        state.pending_channels += 1;
        state.refresh_idle_state();
        Ok(PnPendingChannel::new(self.self_weak.clone()))
    }

    fn pending_to_active_channel(&self) -> P2pResult<Arc<PnChannelLease>> {
        let (refresh, result) = {
            let mut state = self.lifecycle.lock().unwrap();
            debug_assert!(state.pending_channels > 0);
            state.pending_channels = state.pending_channels.saturating_sub(1);
            if state.is_open() {
                state.active_channels += 1;
                (
                    state.refresh_idle_state(),
                    Ok(Arc::new(PnChannelLease {
                        tunnel: self.self_weak.clone(),
                    })),
                )
            } else {
                (
                    state.refresh_idle_state(),
                    Err(p2p_err!(P2pErrorCode::Interrupted, "pn tunnel closed")),
                )
            }
        };
        self.schedule_from_refresh(refresh);
        result
    }

    fn release_pending_channel(&self) {
        let refresh = {
            let mut state = self.lifecycle.lock().unwrap();
            state.pending_channels = state.pending_channels.saturating_sub(1);
            state.refresh_idle_state()
        };
        self.schedule_from_refresh(refresh);
    }

    fn release_active_channel(&self) {
        let refresh = {
            let mut state = self.lifecycle.lock().unwrap();
            state.active_channels = state.active_channels.saturating_sub(1);
            state.refresh_idle_state()
        };
        self.schedule_from_refresh(refresh);
    }

    fn wrap_stream_with_lease(
        &self,
        read: TunnelStreamRead,
        write: TunnelStreamWrite,
        lease: Arc<PnChannelLease>,
    ) -> (TunnelStreamRead, TunnelStreamWrite) {
        (
            Box::pin(LeaseRead {
                inner: read,
                _lease: lease.clone(),
            }),
            Box::pin(LeaseWrite {
                inner: write,
                _lease: lease,
            }),
        )
    }

    fn wrap_read_with_lease(
        &self,
        read: TunnelStreamRead,
        lease: Arc<PnChannelLease>,
    ) -> TunnelStreamRead {
        Box::pin(LeaseRead {
            inner: read,
            _lease: lease,
        })
    }

    fn wrap_write_with_lease(
        &self,
        write: TunnelStreamWrite,
        lease: Arc<PnChannelLease>,
    ) -> TunnelStreamWrite {
        Box::pin(LeaseWrite {
            inner: write,
            _lease: lease,
        })
    }

    fn close_lifecycle_locked(
        &self,
        state: &mut PnTunnelLifecycleState,
        reason: PnTunnelCloseReason,
    ) -> Option<Vec<PassivePnChannel>> {
        if !state.is_open() {
            return None;
        }
        state.close_reason = Some(reason);
        state.queued_channels = 0;
        state.zero_since = None;
        state.generation = state.generation.wrapping_add(1);
        self.closed.store(true, Ordering::SeqCst);
        Some(self.inbound_channels.drain_all())
    }

    async fn finish_close(&self, drained: Vec<PassivePnChannel>) {
        if let Some(shared) = self.shared() {
            shared.unregister_tunnel(
                &PnShared::tunnel_key(self.remote_id.clone(), self.tunnel_id),
                self,
            );
        }

        self.inbound_channels.channel_notify.notify_waiters();
        self.stream_vports_notify.notify_waiters();
        self.datagram_vports_notify.notify_waiters();

        for channel in drained {
            self.send_drained_channel_rejection(channel).await;
        }
    }

    async fn close_with_reason(&self, reason: PnTunnelCloseReason) -> P2pResult<()> {
        let drained = {
            let mut state = self.lifecycle.lock().unwrap();
            self.close_lifecycle_locked(&mut state, reason)
        };
        if let Some(drained) = drained {
            self.finish_close(drained).await;
        }
        Ok(())
    }

    async fn send_drained_channel_rejection(&self, channel: PassivePnChannel) {
        let mut write = channel.write;
        let _ = write_pn_command(
            &mut write,
            ProxyOpenResp {
                tunnel_id: channel.request.tunnel_id,
                result: TunnelCommandResult::Interrupted as u8,
            },
        )
        .await;
    }

    #[cfg(test)]
    fn lifecycle_counts_for_test(&self) -> (usize, usize, usize) {
        let state = self.lifecycle.lock().unwrap();
        (
            state.active_channels,
            state.pending_channels,
            state.queued_channels,
        )
    }

    #[cfg(test)]
    fn set_idle_timeout_for_test(&self, timeout: Duration) {
        *self.idle_timeout.lock().unwrap() = Some(timeout);
    }

    #[cfg(test)]
    async fn trigger_idle_timeout_for_test(&self) {
        let generation = {
            let mut state = self.lifecycle.lock().unwrap();
            let timeout = self.idle_timeout.lock().unwrap().unwrap_or(Duration::ZERO);
            if state.total_channels() == 0 && state.is_open() {
                state.zero_since = Some(Instant::now() - timeout);
                state.generation = state.generation.wrapping_add(1);
                Some(state.generation)
            } else {
                None
            }
        };
        if let Some(generation) = generation {
            self.close_if_idle(generation).await;
        }
    }

    pub(super) fn push_passive_channel(
        &self,
        request: ProxyOpenReq,
        read: TunnelStreamRead,
        write: TunnelStreamWrite,
    ) -> Result<(), RejectedPassivePnChannel> {
        {
            let mut state = self.lifecycle.lock().unwrap();
            if !state.is_open() {
                return Err(RejectedPassivePnChannel {
                    request,
                    read,
                    write,
                    error: p2p_err!(P2pErrorCode::ErrorState, "pn tunnel closed"),
                });
            }
            state.queued_channels += 1;
            state.refresh_idle_state();
            self.inbound_channels.push(PassivePnChannel {
                request,
                read,
                write,
            });
        }
        Ok(())
    }

    async fn wait_first_listen_if_needed(&self, kind: PnChannelKind) {
        if self.current_listen_vports(kind).is_some() {
            return;
        }

        log::debug!(
            "pn passive wait first listen local={} remote={} kind={:?}",
            self.local_id,
            self.remote_id,
            kind
        );

        let (wait_state, notify) = match kind {
            PnChannelKind::Stream => (
                &self.stream_first_listen_wait_state,
                &self.stream_vports_notify,
            ),
            PnChannelKind::Datagram => (
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
                            if self.closed.load(Ordering::SeqCst)
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
                    let listen_ready = self.current_listen_vports(kind).is_some();
                    log::debug!(
                        "pn passive wait first listen done local={} remote={} kind={:?} listen_ready={} closed={}",
                        self.local_id,
                        self.remote_id,
                        kind,
                        listen_ready,
                        self.closed.load(Ordering::SeqCst)
                    );
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
                            if self.closed.load(Ordering::SeqCst)
                                || self.current_listen_vports(kind).is_some()
                            {
                                break;
                            }
                            notify.notified().await;
                        }
                    })
                    .await;
                    wait_state.store(FIRST_LISTEN_WAIT_DONE, Ordering::SeqCst);
                    notify.notify_waiters();
                    let listen_ready = self.current_listen_vports(kind).is_some();
                    log::debug!(
                        "pn passive first listen window finished local={} remote={} kind={:?} listen_ready={} closed={}",
                        self.local_id,
                        self.remote_id,
                        kind,
                        listen_ready,
                        self.closed.load(Ordering::SeqCst)
                    );
                    return;
                }
                _ => return,
            }
        }
    }

    async fn take_passive_channel(
        &self,
        kind: PnChannelKind,
    ) -> P2pResult<(
        ProxyOpenReq,
        TunnelStreamRead,
        TunnelStreamWrite,
        PnPendingChannel,
    )> {
        loop {
            let notified = self.inbound_channels.channel_notify.notified();
            if let Some(channel) = {
                let mut state = self.lifecycle.lock().unwrap();
                if !state.is_open() {
                    return Err(p2p_err!(P2pErrorCode::Interrupted, "pn tunnel closed"));
                }
                let channel = self.inbound_channels.pop(kind);
                if channel.is_some() {
                    state.queued_channels = state.queued_channels.saturating_sub(1);
                    state.pending_channels += 1;
                    state.refresh_idle_state();
                }
                channel
            } {
                return Ok((
                    channel.request,
                    channel.read,
                    channel.write,
                    PnPendingChannel::new(self.self_weak.clone()),
                ));
            }
            if self.closed.load(Ordering::SeqCst) {
                return Err(p2p_err!(P2pErrorCode::Interrupted, "pn tunnel closed"));
            }
            notified.await;
        }
    }

    async fn accept_passive_stream(
        &self,
        expected_kind: PnChannelKind,
    ) -> P2pResult<(ProxyOpenReq, TunnelStreamRead, TunnelStreamWrite)> {
        let stream_tls_required = expected_kind == PnChannelKind::Stream
            && self.stream_security_mode == PnProxyStreamSecurityMode::TlsRequired;
        let (req, read, mut write, pending) = self.take_passive_channel(expected_kind).await?;

        let (result, post_error) = if req.to != self.local_id {
            (
                TunnelCommandResult::InvalidParam,
                Some(p2p_err!(
                    P2pErrorCode::InvalidParam,
                    "pn request target mismatch"
                )),
            )
        } else if stream_tls_required && self.tls_context.is_none() {
            (
                TunnelCommandResult::InternalError,
                Some(p2p_err!(
                    P2pErrorCode::NotSupport,
                    "pn tunnel tls context unavailable"
                )),
            )
        } else {
            self.wait_first_listen_if_needed(expected_kind).await;

            if let Some(listened) = self.current_listen_vports(expected_kind) {
                if listened.is_listen(&req.purpose) {
                    log::debug!(
                        "pn passive accept allow local={} remote={} kind={:?} purpose={}",
                        self.local_id,
                        self.remote_id,
                        req.kind,
                        req.purpose
                    );
                    (TunnelCommandResult::Success, None)
                } else {
                    log::warn!(
                        "pn passive reject port-not-listen local={} remote={} kind={:?} purpose={} expected_kind={:?}",
                        self.local_id,
                        self.remote_id,
                        req.kind,
                        req.purpose,
                        expected_kind
                    );
                    (
                        TunnelCommandResult::PortNotListen,
                        Some(TunnelCommandResult::PortNotListen.into_p2p_error(format!(
                            "pn open rejected kind {:?} purpose {}",
                            req.kind, req.purpose
                        ))),
                    )
                }
            } else {
                log::warn!(
                    "pn passive reject listener-closed local={} remote={} kind={:?} purpose={} expected_kind={:?}",
                    self.local_id,
                    self.remote_id,
                    req.kind,
                    req.purpose,
                    expected_kind
                );
                (
                    TunnelCommandResult::ListenerClosed,
                    Some(p2p_err!(
                        P2pErrorCode::Interrupted,
                        "pn accept requires listen before accept"
                    )),
                )
            }
        };

        if self.closed.load(Ordering::SeqCst) {
            return Err(p2p_err!(P2pErrorCode::Interrupted, "pn tunnel closed"));
        }

        if let Err(err) = write_pn_command(
            &mut write,
            ProxyOpenResp {
                tunnel_id: req.tunnel_id,
                result: result as u8,
            },
        )
        .await
        {
            return Err(err);
        }

        if let Some(err) = post_error {
            log::debug!(
                "pn passive accept finished with error local={} remote={} kind={:?} purpose={} code={:?} msg={}",
                self.local_id,
                self.remote_id,
                req.kind,
                req.purpose,
                err.code(),
                err.msg()
            );
            return Err(err);
        }

        log::debug!(
            "pn passive accept success local={} remote={} kind={:?} purpose={} tls_mode={:?}",
            self.local_id,
            self.remote_id,
            req.kind,
            req.purpose,
            self.stream_security_mode
        );
        if stream_tls_required {
            let tls_context = self
                .tls_context
                .clone()
                .ok_or_else(|| p2p_err!(P2pErrorCode::NotSupport, "pn tunnel tls unavailable"))?;
            match wrap_stream_with_server_tls(read, write, &tls_context, &req.from).await {
                Ok((read, write)) => {
                    let lease = pending.into_active(self)?;
                    let (read, write) = self.wrap_stream_with_lease(read, write, lease);
                    Ok((req, read, write))
                }
                Err(err) => Err(err),
            }
        } else {
            let lease = pending.into_active(self)?;
            let (read, write) = self.wrap_stream_with_lease(read, write, lease);
            Ok((req, read, write))
        }
    }
}

#[async_trait::async_trait]
impl Tunnel for PnTunnel {
    fn tunnel_id(&self) -> TunnelId {
        self.tunnel_id
    }

    fn candidate_id(&self) -> TunnelCandidateId {
        self.candidate_id
    }

    fn form(&self) -> TunnelForm {
        TunnelForm::Proxy
    }

    fn is_reverse(&self) -> bool {
        false
    }

    fn protocol(&self) -> Protocol {
        Protocol::Ext(1)
    }

    fn local_id(&self) -> P2pId {
        self.local_id.clone()
    }

    fn remote_id(&self) -> P2pId {
        self.remote_id.clone()
    }

    fn local_ep(&self) -> Option<Endpoint> {
        None
    }

    fn remote_ep(&self) -> Option<Endpoint> {
        None
    }

    fn state(&self) -> TunnelState {
        if self.closed.load(Ordering::SeqCst) {
            TunnelState::Closed
        } else {
            TunnelState::Connected
        }
    }

    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    async fn close(&self) -> P2pResult<()> {
        self.close_with_reason(PnTunnelCloseReason::Manual).await
    }

    async fn listen_stream(&self, vports: ListenVPortsRef) -> P2pResult<()> {
        *self.stream_vports.write().unwrap() = Some(vports);
        log::debug!(
            "pn tunnel listen stream local={} remote={} role={} tunnel_id={:?}",
            self.local_id,
            self.remote_id,
            self.role_name(),
            self.tunnel_id
        );
        self.stream_vports_notify.notify_waiters();
        Ok(())
    }

    async fn listen_datagram(&self, vports: ListenVPortsRef) -> P2pResult<()> {
        *self.datagram_vports.write().unwrap() = Some(vports);
        log::debug!(
            "pn tunnel listen datagram local={} remote={} role={} tunnel_id={:?}",
            self.local_id,
            self.remote_id,
            self.role_name(),
            self.tunnel_id
        );
        self.datagram_vports_notify.notify_waiters();
        Ok(())
    }

    async fn open_stream(
        &self,
        purpose: TunnelPurpose,
    ) -> P2pResult<(TunnelStreamRead, TunnelStreamWrite)> {
        let pending = self.begin_pending_channel()?;
        let network = match &self.role {
            PnTunnelRole::Active { network } => network.clone(),
            PnTunnelRole::Passive {
                network: Some(network),
                ..
            } => network.clone(),
            PnTunnelRole::Passive { network: None, .. } => {
                return Err(p2p_err!(
                    P2pErrorCode::NotSupport,
                    "pn tunnel has no proxy network"
                ));
            }
        };

        let (read, write) = match network
            .open_channel(
                self.tunnel_id,
                self.remote_id.clone(),
                PnChannelKind::Stream,
                purpose,
            )
            .await
        {
            Ok(channel) => channel,
            Err(err) => return Err(err),
        };
        if self.stream_security_mode == PnProxyStreamSecurityMode::TlsRequired {
            let Some(tls_context) = self.tls_context.clone() else {
                return Err(p2p_err!(
                    P2pErrorCode::NotSupport,
                    "pn tunnel tls unavailable"
                ));
            };
            match wrap_stream_with_client_tls(read, write, &tls_context, &self.remote_id).await {
                Ok((read, write)) => {
                    let lease = pending.into_active(self)?;
                    Ok(self.wrap_stream_with_lease(read, write, lease))
                }
                Err(err) => Err(err),
            }
        } else {
            let lease = pending.into_active(self)?;
            Ok(self.wrap_stream_with_lease(read, write, lease))
        }
    }

    async fn accept_stream(
        &self,
    ) -> P2pResult<(TunnelPurpose, TunnelStreamRead, TunnelStreamWrite)> {
        let (req, read, write) = self.accept_passive_stream(PnChannelKind::Stream).await?;
        Ok((req.purpose, read, write))
    }

    async fn open_datagram(&self, purpose: TunnelPurpose) -> P2pResult<TunnelDatagramWrite> {
        let pending = self.begin_pending_channel()?;
        let network = match &self.role {
            PnTunnelRole::Active { network } => network.clone(),
            PnTunnelRole::Passive {
                network: Some(network),
                ..
            } => network.clone(),
            PnTunnelRole::Passive { network: None, .. } => {
                return Err(p2p_err!(
                    P2pErrorCode::NotSupport,
                    "pn tunnel has no proxy network"
                ));
            }
        };
        let (_read, write) = match network
            .open_channel(
                self.tunnel_id,
                self.remote_id.clone(),
                PnChannelKind::Datagram,
                purpose,
            )
            .await
        {
            Ok(channel) => channel,
            Err(err) => return Err(err),
        };
        let lease = pending.into_active(self)?;
        Ok(self.wrap_write_with_lease(write, lease))
    }

    async fn accept_datagram(&self) -> P2pResult<(TunnelPurpose, TunnelDatagramRead)> {
        let (req, read, _write) = self.accept_passive_stream(PnChannelKind::Datagram).await?;
        Ok((req.purpose, read))
    }
}

async fn wrap_stream_with_client_tls(
    read: TunnelStreamRead,
    write: TunnelStreamWrite,
    tls_context: &PnTlsContext,
    remote_id: &P2pId,
) -> P2pResult<(TunnelStreamRead, TunnelStreamWrite)> {
    let client_config = rustls::ClientConfig::builder_with_provider(crate::tls::provider().into())
        .with_protocol_versions(&[&TLS13])
        .unwrap()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(crate::tls::TlsServerCertVerifier::new(
            tls_context.cert_factory.clone(),
            remote_id.clone(),
        )))
        .with_client_auth_cert(
            vec![CertificateDer::from(
                tls_context
                    .local_identity
                    .get_identity_cert()?
                    .get_encoded_cert()?,
            )],
            PrivatePkcs8KeyDer::from(tls_context.local_identity.get_encoded_identity()?).into(),
        )
        .map_err(crate::error::into_p2p_err!(P2pErrorCode::TlsError))?;
    let tls_connector = runtime::TlsConnector::from(Arc::new(client_config));
    let stream = ProxyTlsIo::new(read, write);
    let tls_stream = tls_connector
        .connect(
            validate_server_name(remote_id.to_string())
                .try_into()
                .unwrap(),
            stream,
        )
        .await
        .map_err(crate::error::into_p2p_err!(
            P2pErrorCode::TlsError,
            "pn proxy tls client handshake failed"
        ))?;
    let (read, write) = runtime::split(runtime::TlsStream::from(tls_stream));
    Ok((Box::pin(read), Box::pin(write)))
}

async fn wrap_stream_with_server_tls(
    read: TunnelStreamRead,
    write: TunnelStreamWrite,
    tls_context: &PnTlsContext,
    remote_id: &P2pId,
) -> P2pResult<(TunnelStreamRead, TunnelStreamWrite)> {
    let resolver = DefaultTlsServerCertResolver::new();
    resolver
        .add_server_identity(tls_context.local_identity.clone())
        .await?;
    let mut server_config = ServerConfig::builder_with_provider(crate::tls::provider().into())
        .with_protocol_versions(&[&TLS13])
        .unwrap()
        .with_client_cert_verifier(Arc::new(crate::tls::TlsClientCertVerifier::new(
            tls_context.cert_factory.clone(),
        )))
        .with_cert_resolver(resolver.clone().get_resolves_server_cert());
    server_config.key_log = Arc::new(rustls::KeyLogFile::new());
    let acceptor = runtime::TlsAcceptor::from(Arc::new(server_config));
    let stream = ProxyTlsIo::new(read, write);
    let tls_stream = acceptor
        .accept(stream)
        .await
        .map_err(crate::error::into_p2p_err!(
            P2pErrorCode::TlsError,
            "pn proxy tls server handshake failed"
        ))?;
    let (_, tls_conn) = tls_stream.get_ref();
    let cert = tls_conn
        .peer_certificates()
        .ok_or_else(|| p2p_err!(P2pErrorCode::CertError, "no cert"))?;
    if cert.is_empty() {
        return Err(p2p_err!(P2pErrorCode::CertError, "no cert"));
    }
    let remote_cert = tls_context
        .cert_factory
        .create(&cert[0].as_ref().to_vec())?;
    if remote_cert.get_id() != *remote_id {
        return Err(p2p_err!(
            P2pErrorCode::CertError,
            "pn proxy tls client id mismatch expected={} actual={}",
            remote_id,
            remote_cert.get_id()
        ));
    }
    let (read, write) = runtime::split(runtime::TlsStream::from(tls_stream));
    Ok((Box::pin(read), Box::pin(write)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::networks::ListenVPortRegistry;
    use crate::networks::NetManager;
    use crate::networks::allow_all_listen_vports;
    use crate::p2p_identity::{
        EncodedP2pIdentity, EncodedP2pIdentityCert, P2pIdentity, P2pIdentityCert,
        P2pIdentityCertFactory, P2pIdentityCertRef, P2pIdentityFactory, P2pIdentityRef,
        P2pSignature, P2pSn,
    };
    use crate::tls::{DefaultTlsServerCertResolver, init_tls};
    use crate::types::{TunnelCandidateId, TunnelId};
    #[cfg(feature = "x509")]
    use crate::x509::{
        X509IdentityCertFactory, X509IdentityFactory, generate_ed25519_x509_identity,
    };
    use sha2::{Digest, Sha256};
    use std::sync::Once;
    use tokio::io::{AsyncReadExt, AsyncWriteExt, split};
    use tokio::time::timeout;

    fn test_p2p_id(byte: u8) -> P2pId {
        P2pId::from(vec![byte; 32])
    }

    fn purpose_of(vport: u16) -> TunnelPurpose {
        TunnelPurpose::from_value(&vport).unwrap()
    }

    static INIT_TLS_ONCE: Once = Once::new();

    #[derive(Clone)]
    struct FakeTlsIdentity {
        key: Vec<u8>,
    }

    impl FakeTlsIdentity {
        fn new(key: Vec<u8>) -> Self {
            Self { key }
        }

        fn id(&self) -> P2pId {
            P2pId::from(self.key.clone())
        }

        fn name(&self) -> String {
            self.id().to_string()
        }
    }

    struct FakeTlsCert {
        key: Vec<u8>,
    }

    impl FakeTlsCert {
        fn new(key: Vec<u8>) -> Self {
            Self { key }
        }

        fn id(&self) -> P2pId {
            P2pId::from(self.key.clone())
        }

        fn name(&self) -> String {
            self.id().to_string()
        }
    }

    struct FakeTlsIdentityFactory;

    impl P2pIdentityFactory for FakeTlsIdentityFactory {
        fn create(&self, id: &EncodedP2pIdentity) -> P2pResult<P2pIdentityRef> {
            Ok(Arc::new(FakeTlsIdentity::new(id.clone())))
        }
    }

    struct FakeTlsCertFactory;

    impl P2pIdentityCertFactory for FakeTlsCertFactory {
        fn create(&self, cert: &EncodedP2pIdentityCert) -> P2pResult<P2pIdentityCertRef> {
            Ok(Arc::new(FakeTlsCert::new(cert.clone())))
        }
    }

    fn fake_signature(key: &[u8], message: &[u8]) -> Vec<u8> {
        let mut hasher = Sha256::new();
        hasher.update(key);
        hasher.update(message);
        hasher.finalize().to_vec()
    }

    impl P2pIdentity for FakeTlsIdentity {
        fn get_identity_cert(&self) -> P2pResult<P2pIdentityCertRef> {
            Ok(Arc::new(FakeTlsCert::new(self.key.clone())))
        }

        fn get_id(&self) -> P2pId {
            self.id()
        }

        fn get_name(&self) -> String {
            self.name()
        }

        fn sign_type(&self) -> crate::p2p_identity::P2pIdentitySignType {
            crate::p2p_identity::P2pIdentitySignType::Ed25519
        }

        fn sign(&self, message: &[u8]) -> P2pResult<P2pSignature> {
            Ok(fake_signature(self.key.as_slice(), message))
        }

        fn get_encoded_identity(&self) -> P2pResult<EncodedP2pIdentity> {
            Ok(self.key.clone())
        }

        fn endpoints(&self) -> Vec<Endpoint> {
            vec![]
        }

        fn update_endpoints(&self, _eps: Vec<Endpoint>) -> P2pIdentityRef {
            Arc::new(self.clone())
        }
    }

    impl P2pIdentityCert for FakeTlsCert {
        fn get_id(&self) -> P2pId {
            self.id()
        }

        fn get_name(&self) -> String {
            self.name()
        }

        fn sign_type(&self) -> crate::p2p_identity::P2pIdentitySignType {
            crate::p2p_identity::P2pIdentitySignType::Ed25519
        }

        fn verify(&self, message: &[u8], sign: &P2pSignature) -> bool {
            fake_signature(self.key.as_slice(), message) == *sign
        }

        fn verify_cert(&self, name: &str) -> bool {
            crate::networks::parse_server_name(name) == self.name()
        }

        fn get_encoded_cert(&self) -> P2pResult<EncodedP2pIdentityCert> {
            Ok(self.key.clone())
        }

        fn endpoints(&self) -> Vec<Endpoint> {
            vec![]
        }

        fn sn_list(&self) -> Vec<P2pSn> {
            vec![]
        }

        fn update_endpoints(&self, _eps: Vec<Endpoint>) -> P2pIdentityCertRef {
            Arc::new(Self::new(self.key.clone()))
        }
    }

    #[cfg(not(feature = "x509"))]
    fn init_fake_tls() {
        INIT_TLS_ONCE.call_once(|| {
            init_tls(Arc::new(FakeTlsIdentityFactory));
        });
    }

    #[cfg(feature = "x509")]
    fn tls_context(byte: u8) -> (P2pIdentityRef, PnTlsContext) {
        INIT_TLS_ONCE.call_once(|| {
            init_tls(Arc::new(X509IdentityFactory));
        });
        let identity: P2pIdentityRef =
            Arc::new(generate_ed25519_x509_identity(Some(format!("pn-tls-{byte}"))).unwrap());
        let cert_factory: P2pIdentityCertFactoryRef = Arc::new(X509IdentityCertFactory);
        (
            identity.clone(),
            PnTlsContext {
                local_identity: identity,
                cert_factory,
            },
        )
    }

    #[cfg(not(feature = "x509"))]
    fn tls_context(_byte: u8) -> (P2pIdentityRef, PnTlsContext) {
        init_fake_tls();
        let identity: P2pIdentityRef = Arc::new(FakeTlsIdentity::new(vec![42; 32]));
        let cert_factory: P2pIdentityCertFactoryRef = Arc::new(FakeTlsCertFactory);
        (
            identity.clone(),
            PnTlsContext {
                local_identity: identity,
                cert_factory,
            },
        )
    }

    fn passive_tunnel(
        kind: PnChannelKind,
        vport: u16,
        stream_security_mode: PnProxyStreamSecurityMode,
    ) -> (Arc<PnTunnel>, TunnelStreamRead, TunnelStreamWrite) {
        let local_id = test_p2p_id(1);
        let remote_id = test_p2p_id(2);
        let request = ProxyOpenReq {
            tunnel_id: TunnelId::from(42),
            from: remote_id,
            to: local_id.clone(),
            kind,
            purpose: purpose_of(vport),
        };
        let (local, remote) = tokio::io::duplex(1024);
        let (local_read, local_write) = split(local);
        let (remote_read, remote_write) = split(remote);
        (
            PnTunnel::new_passive(
                local_id,
                request,
                Box::pin(local_read),
                Box::pin(local_write),
                None,
                None,
                stream_security_mode,
            ),
            Box::pin(remote_read),
            Box::pin(remote_write),
        )
    }

    #[tokio::test]
    async fn passive_stream_accept_returns_channel_and_success_response() {
        let (tunnel, mut peer_read, mut peer_write) = passive_tunnel(
            PnChannelKind::Stream,
            1001,
            PnProxyStreamSecurityMode::Disabled,
        );
        tunnel
            .listen_stream(allow_all_listen_vports())
            .await
            .unwrap();

        let (purpose, mut read, mut write) = tunnel.accept_stream().await.unwrap();
        assert_eq!(purpose, purpose_of(1001));

        let resp = read_pn_command::<_, ProxyOpenResp>(&mut peer_read)
            .await
            .unwrap();
        assert_eq!(resp.tunnel_id, TunnelId::from(42));
        assert_eq!(resp.result, TunnelCommandResult::Success as u8);

        peer_write.write_all(b"ping").await.unwrap();
        let mut buf = [0u8; 4];
        read.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"ping");

        write.write_all(b"pong").await.unwrap();
        let mut buf = [0u8; 4];
        peer_read.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"pong");
    }

    #[tokio::test]
    async fn passive_tunnel_accepts_multiple_stream_channels() {
        let (tunnel, mut first_peer_read, _first_peer_write) = passive_tunnel(
            PnChannelKind::Stream,
            1001,
            PnProxyStreamSecurityMode::Disabled,
        );
        tunnel
            .listen_stream(allow_all_listen_vports())
            .await
            .unwrap();

        let (purpose, _read, _write) = tunnel.accept_stream().await.unwrap();
        assert_eq!(purpose, purpose_of(1001));
        let first_resp = read_pn_command::<_, ProxyOpenResp>(&mut first_peer_read)
            .await
            .unwrap();
        assert_eq!(first_resp.result, TunnelCommandResult::Success as u8);

        let (local, remote) = tokio::io::duplex(1024);
        let (local_read, local_write) = split(local);
        let (mut second_peer_read, _second_peer_write) = split(remote);
        tunnel
            .push_passive_channel(
                ProxyOpenReq {
                    tunnel_id: TunnelId::from(42),
                    from: test_p2p_id(2),
                    to: test_p2p_id(1),
                    kind: PnChannelKind::Stream,
                    purpose: purpose_of(1002),
                },
                Box::pin(local_read),
                Box::pin(local_write),
            )
            .unwrap();

        let (purpose, _read, _write) = tunnel.accept_stream().await.unwrap();
        assert_eq!(purpose, purpose_of(1002));
        let second_resp = read_pn_command::<_, ProxyOpenResp>(&mut second_peer_read)
            .await
            .unwrap();
        assert_eq!(second_resp.result, TunnelCommandResult::Success as u8);
    }

    #[tokio::test]
    async fn active_tunnel_accepts_inbound_stream_channel() {
        let local_identity: P2pIdentityRef = Arc::new(FakeTlsIdentity::new(vec![1; 32]));
        let local_id = local_identity.get_id();
        let remote_id = test_p2p_id(2);
        let tunnel = PnTunnel::new_active(
            TunnelId::from(42),
            TunnelCandidateId::from(42),
            local_id.clone(),
            remote_id.clone(),
            PnShared::new_for_test(local_identity),
            PnProxyStreamSecurityMode::Disabled,
        );
        tunnel
            .listen_stream(allow_all_listen_vports())
            .await
            .unwrap();

        let (local, remote) = tokio::io::duplex(1024);
        let (local_read, local_write) = split(local);
        let (mut peer_read, _peer_write) = split(remote);
        tunnel
            .push_passive_channel(
                ProxyOpenReq {
                    tunnel_id: TunnelId::from(42),
                    from: remote_id,
                    to: local_id,
                    kind: PnChannelKind::Stream,
                    purpose: purpose_of(1003),
                },
                Box::pin(local_read),
                Box::pin(local_write),
            )
            .unwrap();

        let (purpose, _read, _write) = tunnel.accept_stream().await.unwrap();
        assert_eq!(purpose, purpose_of(1003));
        let resp = read_pn_command::<_, ProxyOpenResp>(&mut peer_read)
            .await
            .unwrap();
        assert_eq!(resp.result, TunnelCommandResult::Success as u8);
    }

    #[tokio::test]
    async fn pn_tunnel_idle_timeout_waits_for_queued_and_active_channel() {
        let (tunnel, mut peer_read, _peer_write) = passive_tunnel(
            PnChannelKind::Stream,
            1004,
            PnProxyStreamSecurityMode::Disabled,
        );
        tunnel.set_idle_timeout_for_test(std::time::Duration::ZERO);

        assert_eq!(tunnel.lifecycle_counts_for_test(), (0, 0, 1));
        tunnel.trigger_idle_timeout_for_test().await;
        assert!(!tunnel.is_closed());

        tunnel
            .listen_stream(allow_all_listen_vports())
            .await
            .unwrap();
        let (_purpose, read, write) = tunnel.accept_stream().await.unwrap();
        let resp = read_pn_command::<_, ProxyOpenResp>(&mut peer_read)
            .await
            .unwrap();
        assert_eq!(resp.result, TunnelCommandResult::Success as u8);
        assert_eq!(tunnel.lifecycle_counts_for_test(), (1, 0, 0));

        tunnel.trigger_idle_timeout_for_test().await;
        assert!(!tunnel.is_closed());

        drop(read);
        drop(write);
        assert_eq!(tunnel.lifecycle_counts_for_test(), (0, 0, 0));
        tunnel.trigger_idle_timeout_for_test().await;
        assert!(tunnel.is_closed());
        assert_eq!(tunnel.state(), TunnelState::Closed);
    }

    #[tokio::test]
    async fn pn_tunnel_idle_close_wakes_pending_accept() {
        let local_identity: P2pIdentityRef = Arc::new(FakeTlsIdentity::new(vec![3; 32]));
        let local_id = local_identity.get_id();
        let tunnel = PnTunnel::new_active(
            TunnelId::from(43),
            TunnelCandidateId::from(43),
            local_id,
            test_p2p_id(4),
            PnShared::new_for_test(local_identity),
            PnProxyStreamSecurityMode::Disabled,
        );
        tunnel.set_idle_timeout_for_test(std::time::Duration::ZERO);

        let accept_task = tokio::spawn({
            let tunnel = tunnel.clone();
            async move { tunnel.accept_stream().await }
        });
        assert!(
            timeout(std::time::Duration::from_millis(20), async {
                while !accept_task.is_finished() {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .is_err()
        );

        tunnel.trigger_idle_timeout_for_test().await;
        let err = accept_task.await.unwrap().err().unwrap();
        assert_eq!(err.code(), P2pErrorCode::Interrupted);
    }

    #[tokio::test]
    async fn pn_tunnel_idle_close_rejects_later_open_immediately() {
        let local_identity: P2pIdentityRef = Arc::new(FakeTlsIdentity::new(vec![5; 32]));
        let local_id = local_identity.get_id();
        let tunnel = PnTunnel::new_active(
            TunnelId::from(44),
            TunnelCandidateId::from(44),
            local_id,
            test_p2p_id(6),
            PnShared::new_for_test(local_identity),
            PnProxyStreamSecurityMode::Disabled,
        );
        tunnel.set_idle_timeout_for_test(std::time::Duration::ZERO);
        tunnel.trigger_idle_timeout_for_test().await;

        let err = tunnel.open_stream(purpose_of(1005)).await.err().unwrap();
        assert_eq!(err.code(), P2pErrorCode::ErrorState);
        let err = tunnel.open_datagram(purpose_of(1006)).await.err().unwrap();
        assert_eq!(err.code(), P2pErrorCode::ErrorState);
    }

    #[tokio::test]
    async fn pn_tunnel_idle_close_allows_recreate_for_same_key() {
        let local_identity: P2pIdentityRef = Arc::new(FakeTlsIdentity::new(vec![7; 32]));
        let local_id = local_identity.get_id();
        let shared = PnShared::new_for_test(local_identity);
        let remote_id = test_p2p_id(8);
        let tunnel_id = TunnelId::from(45);
        let tunnel = PnTunnel::new_active(
            tunnel_id,
            TunnelCandidateId::from(45),
            local_id.clone(),
            remote_id.clone(),
            shared.clone(),
            PnProxyStreamSecurityMode::Disabled,
        );
        let key = PnShared::tunnel_key(remote_id.clone(), tunnel_id);
        shared.register_tunnel(key.clone(), &tunnel);
        assert!(shared.get_tunnel(&key).is_some());

        tunnel.set_idle_timeout_for_test(std::time::Duration::ZERO);
        tunnel.trigger_idle_timeout_for_test().await;

        assert!(shared.get_tunnel(&key).is_none());

        let request = ProxyOpenReq {
            tunnel_id,
            from: remote_id.clone(),
            to: local_id,
            kind: PnChannelKind::Stream,
            purpose: purpose_of(1007),
        };
        let (local, _remote) = tokio::io::duplex(1024);
        let (local_read, local_write) = split(local);
        let recreated = PnTunnel::new_passive(
            shared.local_id(),
            request,
            Box::pin(local_read),
            Box::pin(local_write),
            Some(shared.clone()),
            None,
            PnProxyStreamSecurityMode::Disabled,
        );
        shared.register_tunnel(key.clone(), &recreated);
        let registered = shared.get_tunnel(&key).unwrap();
        assert!(Arc::ptr_eq(&registered, &recreated));
        assert!(!Arc::ptr_eq(&registered, &tunnel));
    }

    #[tokio::test]
    async fn pn_tunnel_close_does_not_unregister_replacement_for_same_key() {
        let local_identity: P2pIdentityRef = Arc::new(FakeTlsIdentity::new(vec![9; 32]));
        let local_id = local_identity.get_id();
        let shared = PnShared::new_for_test(local_identity);
        let remote_id = test_p2p_id(10);
        let tunnel_id = TunnelId::from(46);
        let key = PnShared::tunnel_key(remote_id.clone(), tunnel_id);

        let old = PnTunnel::new_active(
            tunnel_id,
            TunnelCandidateId::from(46),
            local_id.clone(),
            remote_id.clone(),
            shared.clone(),
            PnProxyStreamSecurityMode::Disabled,
        );
        let replacement = PnTunnel::new_active(
            tunnel_id,
            TunnelCandidateId::from(46),
            local_id,
            remote_id,
            shared.clone(),
            PnProxyStreamSecurityMode::Disabled,
        );

        shared.register_tunnel(key.clone(), &old);
        shared.register_tunnel(key.clone(), &replacement);
        old.close().await.unwrap();

        let registered = shared.get_tunnel(&key).unwrap();
        assert!(Arc::ptr_eq(&registered, &replacement));
    }

    #[tokio::test]
    async fn pn_tunnel_closed_push_returns_channel_for_recreate() {
        let local_identity: P2pIdentityRef = Arc::new(FakeTlsIdentity::new(vec![11; 32]));
        let local_id = local_identity.get_id();
        let remote_id = test_p2p_id(12);
        let tunnel = PnTunnel::new_active(
            TunnelId::from(47),
            TunnelCandidateId::from(47),
            local_id.clone(),
            remote_id.clone(),
            PnShared::new_for_test(local_identity),
            PnProxyStreamSecurityMode::Disabled,
        );
        tunnel.close().await.unwrap();

        let request = ProxyOpenReq {
            tunnel_id: TunnelId::from(47),
            from: remote_id,
            to: local_id,
            kind: PnChannelKind::Stream,
            purpose: purpose_of(1008),
        };
        let (local, _remote) = tokio::io::duplex(1024);
        let (local_read, local_write) = split(local);

        let rejected = tunnel
            .push_passive_channel(request.clone(), Box::pin(local_read), Box::pin(local_write))
            .err()
            .unwrap();

        assert_eq!(rejected.error.code(), P2pErrorCode::ErrorState);
        assert_eq!(rejected.request.tunnel_id, request.tunnel_id);
        assert_eq!(rejected.request.purpose, request.purpose);
    }

    #[tokio::test]
    async fn pn_tunnel_pending_guard_drop_releases_pending_channel() {
        let local_identity: P2pIdentityRef = Arc::new(FakeTlsIdentity::new(vec![13; 32]));
        let local_id = local_identity.get_id();
        let tunnel = PnTunnel::new_active(
            TunnelId::from(48),
            TunnelCandidateId::from(48),
            local_id,
            test_p2p_id(14),
            PnShared::new_for_test(local_identity),
            PnProxyStreamSecurityMode::Disabled,
        );

        let pending = tunnel.begin_pending_channel().unwrap();
        assert_eq!(tunnel.lifecycle_counts_for_test(), (0, 1, 0));

        drop(pending);
        assert_eq!(tunnel.lifecycle_counts_for_test(), (0, 0, 0));

        tunnel.set_idle_timeout_for_test(std::time::Duration::ZERO);
        tunnel.trigger_idle_timeout_for_test().await;
        assert!(tunnel.is_closed());
    }

    #[tokio::test]
    async fn pn_tunnel_close_rejects_pending_to_active() {
        let local_identity: P2pIdentityRef = Arc::new(FakeTlsIdentity::new(vec![15; 32]));
        let local_id = local_identity.get_id();
        let tunnel = PnTunnel::new_active(
            TunnelId::from(49),
            TunnelCandidateId::from(49),
            local_id,
            test_p2p_id(16),
            PnShared::new_for_test(local_identity),
            PnProxyStreamSecurityMode::Disabled,
        );

        let pending = tunnel.begin_pending_channel().unwrap();
        tunnel.close().await.unwrap();

        let err = pending.into_active(&tunnel).err().unwrap();
        assert_eq!(err.code(), P2pErrorCode::Interrupted);
        assert_eq!(tunnel.lifecycle_counts_for_test(), (0, 0, 0));
        assert!(tunnel.is_closed());
    }

    #[tokio::test]
    async fn pn_tunnel_closed_pending_to_active_consumes_only_its_guard() {
        let local_identity: P2pIdentityRef = Arc::new(FakeTlsIdentity::new(vec![19; 32]));
        let local_id = local_identity.get_id();
        let tunnel = PnTunnel::new_active(
            TunnelId::from(51),
            TunnelCandidateId::from(51),
            local_id,
            test_p2p_id(20),
            PnShared::new_for_test(local_identity),
            PnProxyStreamSecurityMode::Disabled,
        );

        let first = tunnel.begin_pending_channel().unwrap();
        let second = tunnel.begin_pending_channel().unwrap();
        assert_eq!(tunnel.lifecycle_counts_for_test(), (0, 2, 0));

        tunnel.close().await.unwrap();
        let err = first.into_active(&tunnel).err().unwrap();
        assert_eq!(err.code(), P2pErrorCode::Interrupted);
        assert_eq!(tunnel.lifecycle_counts_for_test(), (0, 1, 0));

        drop(second);
        assert_eq!(tunnel.lifecycle_counts_for_test(), (0, 0, 0));
    }

    #[tokio::test]
    async fn pn_shared_concurrent_same_key_dispatch_creates_one_tunnel() {
        let local_identity: P2pIdentityRef = Arc::new(FakeTlsIdentity::new(vec![17; 32]));
        let local_id = local_identity.get_id();
        let shared = PnShared::new_for_test(local_identity);
        let remote_id = test_p2p_id(18);
        let tunnel_id = TunnelId::from(50);
        let key = PnShared::tunnel_key(remote_id.clone(), tunnel_id);

        let make_channel = |vport| -> (ProxyOpenReq, TunnelStreamRead, TunnelStreamWrite) {
            let request = ProxyOpenReq {
                tunnel_id,
                from: remote_id.clone(),
                to: local_id.clone(),
                kind: PnChannelKind::Stream,
                purpose: purpose_of(vport),
            };
            let (local, _remote) = tokio::io::duplex(1024);
            let (local_read, local_write) = split(local);
            (request, Box::pin(local_read), Box::pin(local_write))
        };

        let (req1, read1, write1) = make_channel(1009);
        let (req2, read2, write2) = make_channel(1010);
        let first = tokio::spawn({
            let shared = shared.clone();
            let key = key.clone();
            async move { shared.dispatch_or_create_passive_tunnel(key, req1, read1, write1) }
        });
        let second = tokio::spawn({
            let shared = shared.clone();
            let key = key.clone();
            async move { shared.dispatch_or_create_passive_tunnel(key, req2, read2, write2) }
        });

        let first = first.await.unwrap();
        let second = second.await.unwrap();
        let created = matches!(
            first,
            super::super::pn_client::PassiveTunnelDispatch::Created(_)
        ) as usize
            + matches!(
                second,
                super::super::pn_client::PassiveTunnelDispatch::Created(_)
            ) as usize;

        assert_eq!(created, 1);
        let registered = shared.get_tunnel(&key).unwrap();
        assert_eq!(registered.lifecycle_counts_for_test(), (0, 0, 2));
    }

    #[tokio::test]
    async fn passive_datagram_accept_returns_read_and_success_response() {
        let (tunnel, mut peer_read, mut peer_write) = passive_tunnel(
            PnChannelKind::Datagram,
            2002,
            PnProxyStreamSecurityMode::Disabled,
        );
        tunnel
            .listen_datagram(allow_all_listen_vports())
            .await
            .unwrap();

        let (purpose, mut read) = tunnel.accept_datagram().await.unwrap();
        assert_eq!(purpose, purpose_of(2002));

        let resp = read_pn_command::<_, ProxyOpenResp>(&mut peer_read)
            .await
            .unwrap();
        assert_eq!(resp.tunnel_id, TunnelId::from(42));
        assert_eq!(resp.result, TunnelCommandResult::Success as u8);

        peer_write.write_all(b"data").await.unwrap();
        let mut buf = [0u8; 4];
        read.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"data");
    }

    #[tokio::test]
    async fn passive_stream_accept_rejects_unlistened_port() {
        let (tunnel, mut peer_read, _peer_write) = passive_tunnel(
            PnChannelKind::Stream,
            3003,
            PnProxyStreamSecurityMode::Disabled,
        );
        let empty_vports = ListenVPortRegistry::<()>::new();
        tunnel
            .listen_stream(empty_vports.as_listen_vports_ref())
            .await
            .unwrap();

        let err = tunnel.accept_stream().await.err().unwrap();
        assert_eq!(err.code(), P2pErrorCode::PortNotListen);

        let resp = read_pn_command::<_, ProxyOpenResp>(&mut peer_read)
            .await
            .unwrap();
        assert_eq!(resp.result, TunnelCommandResult::PortNotListen as u8);
    }

    #[tokio::test]
    async fn passive_stream_accept_requires_listen_first() {
        let (tunnel, mut peer_read, _peer_write) = passive_tunnel(
            PnChannelKind::Stream,
            4004,
            PnProxyStreamSecurityMode::Disabled,
        );

        let err = tunnel.accept_stream().await.err().unwrap();
        assert_eq!(err.code(), P2pErrorCode::Interrupted);

        let resp = read_pn_command::<_, ProxyOpenResp>(&mut peer_read)
            .await
            .unwrap();
        assert_eq!(resp.result, TunnelCommandResult::ListenerClosed as u8);
    }

    #[tokio::test]
    async fn passive_stream_accept_waits_for_late_listen() {
        let (tunnel, mut peer_read, mut peer_write) = passive_tunnel(
            PnChannelKind::Stream,
            5005,
            PnProxyStreamSecurityMode::Disabled,
        );

        let mut pending_accept = Box::pin({
            let tunnel = tunnel.clone();
            async move { tunnel.accept_stream().await }
        });

        assert!(
            timeout(FIRST_LISTEN_WAIT_TIMEOUT / 3, &mut pending_accept)
                .await
                .is_err()
        );

        tunnel
            .listen_stream(allow_all_listen_vports())
            .await
            .unwrap();

        let (purpose, mut read, mut write) = pending_accept.await.unwrap();
        assert_eq!(purpose, purpose_of(5005));

        let resp = read_pn_command::<_, ProxyOpenResp>(&mut peer_read)
            .await
            .unwrap();
        assert_eq!(resp.result, TunnelCommandResult::Success as u8);

        peer_write.write_all(b"late").await.unwrap();
        let mut buf = [0u8; 4];
        read.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"late");

        write.write_all(b"sync").await.unwrap();
        let mut buf = [0u8; 4];
        peer_read.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"sync");
    }

    #[tokio::test]
    async fn passive_stream_wait_is_independent_from_datagram_listen() {
        let (tunnel, mut peer_read, _peer_write) = passive_tunnel(
            PnChannelKind::Stream,
            5006,
            PnProxyStreamSecurityMode::Disabled,
        );

        let mut pending_accept = Box::pin({
            let tunnel = tunnel.clone();
            async move { tunnel.accept_stream().await }
        });

        tunnel
            .listen_datagram(allow_all_listen_vports())
            .await
            .unwrap();

        assert!(
            timeout(FIRST_LISTEN_WAIT_TIMEOUT / 3, &mut pending_accept)
                .await
                .is_err()
        );

        tunnel
            .listen_stream(allow_all_listen_vports())
            .await
            .unwrap();

        let (purpose, _read, _write) = pending_accept.await.unwrap();
        assert_eq!(purpose, purpose_of(5006));

        let resp = read_pn_command::<_, ProxyOpenResp>(&mut peer_read)
            .await
            .unwrap();
        assert_eq!(resp.result, TunnelCommandResult::Success as u8);
    }

    #[tokio::test]
    async fn passive_stream_accept_wraps_stream_with_tls_when_requested() {
        let (server_identity, server_tls) = tls_context(11);
        let (client_identity, client_tls) = tls_context(12);
        let request = ProxyOpenReq {
            tunnel_id: TunnelId::from(52),
            from: client_identity.get_id(),
            to: server_identity.get_id(),
            kind: PnChannelKind::Stream,
            purpose: purpose_of(6001),
        };
        let (local, remote) = tokio::io::duplex(4096);
        let (local_read, local_write) = split(local);
        let (remote_read, remote_write) = split(remote);
        let tunnel = PnTunnel::new_passive(
            server_identity.get_id(),
            request.clone(),
            Box::pin(local_read),
            Box::pin(local_write),
            None,
            Some(server_tls),
            PnProxyStreamSecurityMode::TlsRequired,
        );
        tunnel
            .listen_stream(allow_all_listen_vports())
            .await
            .unwrap();

        let accept_task = tokio::spawn({
            let tunnel = tunnel.clone();
            async move { tunnel.accept_stream().await }
        });

        let mut peer_read = Box::pin(remote_read) as TunnelStreamRead;
        let peer_write = Box::pin(remote_write) as TunnelStreamWrite;

        let resp = read_pn_command::<_, ProxyOpenResp>(&mut peer_read)
            .await
            .unwrap();
        assert_eq!(resp.result, TunnelCommandResult::Success as u8);

        let (mut client_read, mut client_write) = wrap_stream_with_client_tls(
            peer_read,
            peer_write,
            &client_tls,
            &server_identity.get_id(),
        )
        .await
        .unwrap();

        let (purpose, mut server_read, mut server_write) = accept_task.await.unwrap().unwrap();
        assert_eq!(purpose, purpose_of(6001));

        client_write.write_all(b"ping").await.unwrap();
        let mut ping_buf = [0u8; 4];
        server_read.read_exact(&mut ping_buf).await.unwrap();
        assert_eq!(&ping_buf, b"ping");

        server_write.write_all(b"pong").await.unwrap();
        let mut pong_buf = [0u8; 4];
        client_read.read_exact(&mut pong_buf).await.unwrap();
        assert_eq!(&pong_buf, b"pong");
    }

    #[tokio::test]
    async fn client_tls_wrapper_rejects_wrong_remote_identity() {
        let (server_identity, server_tls) = tls_context(21);
        let (client_identity, client_tls) = tls_context(22);
        let wrong_remote_id = test_p2p_id(23);
        let (server_side, client_side) = tokio::io::duplex(4096);
        let (server_read, server_write) = split(server_side);
        let (client_read, client_write) = split(client_side);

        let server_task = tokio::spawn(async move {
            wrap_stream_with_server_tls(
                Box::pin(server_read),
                Box::pin(server_write),
                &server_tls,
                &client_identity.get_id(),
            )
            .await
        });

        let client_result = wrap_stream_with_client_tls(
            Box::pin(client_read),
            Box::pin(client_write),
            &client_tls,
            &wrong_remote_id,
        )
        .await;
        assert!(client_result.is_err());

        let server_result = server_task.await.unwrap();
        assert!(server_result.is_err());
        assert_ne!(server_identity.get_id(), wrong_remote_id);
    }

    #[tokio::test]
    async fn passive_datagram_ignores_tls_mode_and_returns_read_and_success_response() {
        let (tunnel, mut peer_read, mut peer_write) = passive_tunnel(
            PnChannelKind::Datagram,
            6002,
            PnProxyStreamSecurityMode::TlsRequired,
        );
        tunnel
            .listen_datagram(allow_all_listen_vports())
            .await
            .unwrap();

        let (purpose, mut read) = tunnel.accept_datagram().await.unwrap();
        assert_eq!(purpose, purpose_of(6002));

        let resp = read_pn_command::<_, ProxyOpenResp>(&mut peer_read)
            .await
            .unwrap();
        assert_eq!(resp.result, TunnelCommandResult::Success as u8);

        peer_write.write_all(b"data").await.unwrap();
        let mut buf = [0u8; 4];
        read.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"data");
    }

    #[tokio::test]
    async fn create_tunnel_with_options_sets_tls_mode_without_local_datagram_rejection() {
        let (local_identity, _tls) = tls_context(41);
        let net_manager = NetManager::new(vec![], DefaultTlsServerCertResolver::new()).unwrap();
        let ttp_client = crate::ttp::TtpClient::new(local_identity.clone(), net_manager);
        let pn_client = super::super::pn_client::PnClient::new_with_tls_material(
            ttp_client,
            local_identity.clone(),
            Arc::new(FakeTlsCertFactory),
        );
        let remote = Endpoint::from((Protocol::Ext(1), "0.0.0.0:0".parse().unwrap()));
        let remote_id = test_p2p_id(42);
        let tunnel = pn_client
            .create_tunnel_with_options(
                &local_identity,
                &remote,
                &remote_id,
                None,
                crate::networks::TunnelConnectIntent::default(),
                PnTunnelOptions {
                    stream_security_mode: PnProxyStreamSecurityMode::TlsRequired,
                },
            )
            .await
            .unwrap();

        assert_eq!(
            tunnel.stream_security_mode(),
            PnProxyStreamSecurityMode::TlsRequired
        );
        let registered = pn_client
            .get_tunnel_for_test(&remote_id, tunnel.tunnel_id())
            .unwrap();
        assert!(Arc::ptr_eq(&registered, &tunnel));
        if let Err(err) = tunnel.open_datagram(purpose_of(6003)).await {
            assert_ne!(err.code(), P2pErrorCode::NotSupport);
        }
    }

    #[tokio::test]
    async fn pn_client_tunnel_idle_timeout_config_applies_to_new_tunnel() {
        let (local_identity, _tls) = tls_context(51);
        let net_manager = NetManager::new(vec![], DefaultTlsServerCertResolver::new()).unwrap();
        let ttp_client = crate::ttp::TtpClient::new(local_identity.clone(), net_manager);
        let pn_client = super::super::pn_client::PnClient::new(ttp_client);
        let remote = Endpoint::from((Protocol::Ext(1), "0.0.0.0:0".parse().unwrap()));

        pn_client.set_tunnel_idle_timeout(None);
        assert_eq!(pn_client.tunnel_idle_timeout(), None);

        let tunnel = pn_client
            .create_tunnel_with_options(
                &local_identity,
                &remote,
                &test_p2p_id(52),
                None,
                crate::networks::TunnelConnectIntent::default(),
                PnTunnelOptions::default(),
            )
            .await
            .unwrap();

        tunnel.trigger_idle_timeout_for_test().await;
        assert!(!tunnel.is_closed());

        pn_client.set_tunnel_idle_timeout(Some(std::time::Duration::ZERO));
        let tunnel = pn_client
            .create_tunnel_with_options(
                &local_identity,
                &remote,
                &test_p2p_id(53),
                None,
                crate::networks::TunnelConnectIntent::default(),
                PnTunnelOptions::default(),
            )
            .await
            .unwrap();

        tunnel.trigger_idle_timeout_for_test().await;
        assert!(tunnel.is_closed());
    }
}
