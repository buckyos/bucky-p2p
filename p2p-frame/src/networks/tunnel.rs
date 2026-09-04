use crate::endpoint::{Endpoint, Protocol};
use crate::error::{P2pError, P2pErrorCode, P2pResult};
use crate::p2p_identity::P2pId;
use crate::runtime;
use crate::types::{TunnelCandidateId, TunnelId};
use bucky_raw_codec::{RawConvertTo, RawDecode, RawEncode, RawFrom};
use std::fmt;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

pub type TunnelStreamRead = Pin<Box<dyn runtime::AsyncRead + Send + Unpin + 'static>>;
pub type TunnelStreamWrite = Pin<Box<dyn runtime::AsyncWrite + Send + Unpin + 'static>>;
pub type TunnelDatagramRead = Pin<Box<dyn runtime::AsyncRead + Send + Unpin + 'static>>;
pub type TunnelDatagramWrite = Pin<Box<dyn runtime::AsyncWrite + Send + Unpin + 'static>>;
pub type IncomingStream = (TunnelPurpose, TunnelStreamRead, TunnelStreamWrite);
pub type IncomingDatagram = (TunnelPurpose, TunnelDatagramRead);
pub type IncomingControlStream = (TunnelPurpose, TunnelStreamRead, TunnelStreamWrite);
pub type IncomingStreamCallbackFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;
pub type IncomingDatagramCallbackFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;
pub type IncomingControlStreamCallbackFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;
pub type IncomingStreamCallback =
    Arc<dyn Fn(P2pResult<IncomingStream>) -> IncomingStreamCallbackFuture + Send + Sync + 'static>;
pub type IncomingDatagramCallback = Arc<
    dyn Fn(P2pResult<IncomingDatagram>) -> IncomingDatagramCallbackFuture + Send + Sync + 'static,
>;
pub type IncomingControlStreamCallback = Arc<
    dyn Fn(P2pResult<IncomingControlStream>) -> IncomingControlStreamCallbackFuture
        + Send
        + Sync
        + 'static,
>;

#[derive(Clone, Debug, Eq, Hash, PartialEq, RawEncode, RawDecode)]
pub struct TunnelPurpose(Vec<u8>);

impl TunnelPurpose {
    fn codec_error(err: bucky_raw_codec::CodecError) -> P2pError {
        P2pError::new(P2pErrorCode::RawCodecError, err.to_string())
    }

    pub fn from_bytes(raw: Vec<u8>) -> Self {
        Self(raw)
    }

    pub fn from_value<T>(value: &T) -> P2pResult<Self>
    where
        T: RawEncode,
    {
        value.to_vec().map(Self).map_err(Self::codec_error)
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_slice()
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }

    pub fn decode_as<T>(&self) -> P2pResult<T>
    where
        for<'de> T: RawFrom<'de, T>,
    {
        T::clone_from_slice(self.0.as_slice()).map_err(Self::codec_error)
    }
}

impl fmt::Display for TunnelPurpose {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x")?;
        for byte in &self.0 {
            write!(f, "{:02x}", byte)?;
        }
        Ok(())
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, RawEncode, RawDecode)]
pub enum TunnelCommandResult {
    Success = 0,
    PortNotListen = 1,
    ListenerClosed = 2,
    AcceptQueueFull = 3,
    ConflictLost = 4,
    LeaseMismatch = 5,
    Retired = 6,
    ProtocolError = 7,
    Timeout = 8,
    Interrupted = 9,
    InvalidParam = 10,
    InternalError = 11,
}

impl TunnelCommandResult {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Success),
            1 => Some(Self::PortNotListen),
            2 => Some(Self::ListenerClosed),
            3 => Some(Self::AcceptQueueFull),
            4 => Some(Self::ConflictLost),
            5 => Some(Self::LeaseMismatch),
            6 => Some(Self::Retired),
            7 => Some(Self::ProtocolError),
            8 => Some(Self::Timeout),
            9 => Some(Self::Interrupted),
            10 => Some(Self::InvalidParam),
            11 => Some(Self::InternalError),
            _ => None,
        }
    }

    pub fn into_p2p_error(self, context: impl Into<String>) -> P2pError {
        let code = match self {
            Self::Success => P2pErrorCode::Ok,
            Self::PortNotListen => P2pErrorCode::PortNotListen,
            Self::ListenerClosed => P2pErrorCode::Interrupted,
            Self::AcceptQueueFull => P2pErrorCode::Reject,
            Self::ConflictLost => P2pErrorCode::Conflict,
            Self::LeaseMismatch => P2pErrorCode::Reject,
            Self::Retired => P2pErrorCode::Interrupted,
            Self::ProtocolError => P2pErrorCode::InvalidData,
            Self::Timeout => P2pErrorCode::Timeout,
            Self::Interrupted => P2pErrorCode::Interrupted,
            Self::InvalidParam => P2pErrorCode::InvalidParam,
            Self::InternalError => P2pErrorCode::Unknown,
        };
        P2pError::new(code, context.into())
    }
}

pub trait ListenVPorts: Send + Sync + 'static {
    fn is_listen(&self, purpose: &TunnelPurpose) -> bool;
}

pub type ListenVPortsRef = Arc<dyn ListenVPorts>;

pub struct AllowAllListenVPorts;

impl ListenVPorts for AllowAllListenVPorts {
    fn is_listen(&self, _purpose: &TunnelPurpose) -> bool {
        true
    }
}

pub fn allow_all_listen_vports() -> ListenVPortsRef {
    Arc::new(AllowAllListenVPorts)
}

pub fn allow_all_tunnel_purposes() -> ListenVPortsRef {
    allow_all_listen_vports()
}

pub struct ListenPurposeRegistry<L> {
    listeners: std::sync::RwLock<std::collections::HashMap<TunnelPurpose, Arc<L>>>,
}

impl<L> ListenPurposeRegistry<L> {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            listeners: std::sync::RwLock::new(std::collections::HashMap::new()),
        })
    }

    pub fn as_listen_vports_ref(self: &Arc<Self>) -> ListenVPortsRef
    where
        L: Send + Sync + 'static,
    {
        self.clone()
    }

    pub fn contains(&self, purpose: &TunnelPurpose) -> bool {
        self.listeners.read().unwrap().contains_key(purpose)
    }

    pub fn insert(&self, purpose: TunnelPurpose, listener: Arc<L>) -> Option<Arc<L>> {
        self.listeners.write().unwrap().insert(purpose, listener)
    }

    pub fn get(&self, purpose: &TunnelPurpose) -> Option<Arc<L>> {
        self.listeners.read().unwrap().get(purpose).cloned()
    }

    pub fn remove(&self, purpose: &TunnelPurpose) -> Option<Arc<L>> {
        self.listeners.write().unwrap().remove(purpose)
    }
}

impl<L> ListenVPorts for ListenPurposeRegistry<L>
where
    L: Send + Sync + 'static,
{
    fn is_listen(&self, purpose: &TunnelPurpose) -> bool {
        self.contains(purpose)
    }
}

pub struct ListenVPortRegistry<L> {
    listeners: std::sync::RwLock<std::collections::HashMap<u16, Arc<L>>>,
}

impl<L> ListenVPortRegistry<L> {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            listeners: std::sync::RwLock::new(std::collections::HashMap::new()),
        })
    }

    pub fn as_listen_vports_ref(self: &Arc<Self>) -> ListenVPortsRef
    where
        L: Send + Sync + 'static,
    {
        self.clone()
    }

    pub fn contains(&self, vport: u16) -> bool {
        self.listeners.read().unwrap().contains_key(&vport)
    }

    pub fn insert(&self, vport: u16, listener: Arc<L>) -> Option<Arc<L>> {
        self.listeners.write().unwrap().insert(vport, listener)
    }

    pub fn get(&self, vport: u16) -> Option<Arc<L>> {
        self.listeners.read().unwrap().get(&vport).cloned()
    }

    pub fn remove(&self, vport: u16) -> Option<Arc<L>> {
        self.listeners.write().unwrap().remove(&vport)
    }
}

impl<L> ListenVPorts for ListenVPortRegistry<L>
where
    L: Send + Sync + 'static,
{
    fn is_listen(&self, purpose: &TunnelPurpose) -> bool {
        purpose
            .decode_as::<u16>()
            .ok()
            .map(|vport| self.contains(vport))
            .unwrap_or(false)
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum TunnelForm {
    Active,
    Passive,
    Proxy,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum TunnelState {
    Connecting,
    Connected,
    Closed,
    Error,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TunnelActivityLifecycle {
    Live,
    Retired,
}

struct TunnelActivityState {
    lifecycle: TunnelActivityLifecycle,
    pending_open_count: usize,
    work_instance_num: usize,
    latest_active_at: Instant,
}

pub(crate) struct TunnelActivity {
    state: Mutex<TunnelActivityState>,
}

impl TunnelActivity {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(TunnelActivityState {
                lifecycle: TunnelActivityLifecycle::Live,
                pending_open_count: 0,
                work_instance_num: 0,
                latest_active_at: Instant::now(),
            }),
        })
    }

    fn interrupted_error() -> P2pError {
        P2pError::new(
            P2pErrorCode::Interrupted,
            "tunnel candidate retired".to_owned(),
        )
    }

    pub(crate) fn ensure_live(&self) -> P2pResult<()> {
        if self.state.lock().unwrap().lifecycle == TunnelActivityLifecycle::Live {
            Ok(())
        } else {
            Err(Self::interrupted_error())
        }
    }

    pub(crate) fn note_activity(&self) {
        let mut state = self.state.lock().unwrap();
        if state.lifecycle == TunnelActivityLifecycle::Live {
            state.latest_active_at = Instant::now();
        }
    }

    pub(crate) fn begin_pending(self: &Arc<Self>) -> P2pResult<PendingTunnelActivity> {
        let mut state = self.state.lock().unwrap();
        if state.lifecycle == TunnelActivityLifecycle::Retired {
            return Err(Self::interrupted_error());
        }
        state.pending_open_count += 1;
        state.latest_active_at = Instant::now();
        Ok(PendingTunnelActivity {
            activity: self.clone(),
            active: true,
        })
    }

    fn acquire_work_instances(
        self: &Arc<Self>,
        work_instance_num: usize,
    ) -> P2pResult<Vec<TunnelWorkActivity>> {
        let mut state = self.state.lock().unwrap();
        if state.lifecycle == TunnelActivityLifecycle::Retired {
            return Err(Self::interrupted_error());
        }
        state.work_instance_num += work_instance_num;
        state.latest_active_at = Instant::now();
        Ok((0..work_instance_num)
            .map(|_| TunnelWorkActivity {
                activity: self.clone(),
                active: true,
            })
            .collect())
    }

    pub(crate) fn track_stream_result(
        self: &Arc<Self>,
        result: P2pResult<(TunnelPurpose, TunnelStreamRead, TunnelStreamWrite)>,
    ) -> P2pResult<(TunnelPurpose, TunnelStreamRead, TunnelStreamWrite)> {
        match result {
            Ok((purpose, read, write)) => {
                let mut leases = self.acquire_work_instances(2)?;
                let write_lease = leases.pop().unwrap();
                let read_lease = leases.pop().unwrap();
                Ok((
                    purpose,
                    Self::tracked_read(read, read_lease),
                    Self::tracked_write(write, write_lease),
                ))
            }
            Err(err) => {
                self.note_activity();
                Err(err)
            }
        }
    }

    pub(crate) fn track_datagram_result(
        self: &Arc<Self>,
        result: P2pResult<(TunnelPurpose, TunnelDatagramRead)>,
    ) -> P2pResult<(TunnelPurpose, TunnelDatagramRead)> {
        match result {
            Ok((purpose, read)) => {
                let mut leases = self.acquire_work_instances(1)?;
                Ok((purpose, Self::tracked_read(read, leases.pop().unwrap())))
            }
            Err(err) => {
                self.note_activity();
                Err(err)
            }
        }
    }

    pub(crate) fn try_retire_idle(&self, now: Instant, idle_timeout: Duration) -> bool {
        let mut state = self.state.lock().unwrap();
        if state.lifecycle != TunnelActivityLifecycle::Live
            || state.pending_open_count != 0
            || state.work_instance_num != 0
            || now
                .checked_duration_since(state.latest_active_at)
                .map(|elapsed| elapsed <= idle_timeout)
                .unwrap_or(true)
        {
            return false;
        }
        state.lifecycle = TunnelActivityLifecycle::Retired;
        true
    }

    pub(crate) fn retire(&self) {
        self.state.lock().unwrap().lifecycle = TunnelActivityLifecycle::Retired;
    }

    fn tracked_read(read: TunnelStreamRead, lease: TunnelWorkActivity) -> TunnelStreamRead {
        Box::pin(ActivityTrackedRead {
            inner: read,
            _lease: lease,
        })
    }

    fn tracked_write(write: TunnelStreamWrite, lease: TunnelWorkActivity) -> TunnelStreamWrite {
        Box::pin(ActivityTrackedWrite {
            inner: write,
            _lease: lease,
        })
    }
}

pub(crate) struct PendingTunnelActivity {
    activity: Arc<TunnelActivity>,
    active: bool,
}

impl PendingTunnelActivity {
    pub(crate) fn promote_stream(
        self,
        read: TunnelStreamRead,
        write: TunnelStreamWrite,
    ) -> P2pResult<(TunnelStreamRead, TunnelStreamWrite)> {
        let mut leases = self.promote(2)?;
        let write_lease = leases.pop().unwrap();
        let read_lease = leases.pop().unwrap();
        Ok((
            TunnelActivity::tracked_read(read, read_lease),
            TunnelActivity::tracked_write(write, write_lease),
        ))
    }

    pub(crate) fn promote_datagram(
        self,
        write: TunnelDatagramWrite,
    ) -> P2pResult<TunnelDatagramWrite> {
        let mut leases = self.promote(1)?;
        Ok(TunnelActivity::tracked_write(write, leases.pop().unwrap()))
    }

    pub(crate) fn promote_datagram_read(
        self,
        read: TunnelDatagramRead,
    ) -> P2pResult<TunnelDatagramRead> {
        let mut leases = self.promote(1)?;
        Ok(TunnelActivity::tracked_read(read, leases.pop().unwrap()))
    }

    fn promote(mut self, work_instance_num: usize) -> P2pResult<Vec<TunnelWorkActivity>> {
        let mut state = self.activity.state.lock().unwrap();
        debug_assert!(state.pending_open_count > 0);
        if state.pending_open_count > 0 {
            state.pending_open_count -= 1;
        }
        self.active = false;
        if state.lifecycle == TunnelActivityLifecycle::Retired {
            return Err(TunnelActivity::interrupted_error());
        }
        state.work_instance_num += work_instance_num;
        state.latest_active_at = Instant::now();
        Ok((0..work_instance_num)
            .map(|_| TunnelWorkActivity {
                activity: self.activity.clone(),
                active: true,
            })
            .collect())
    }
}

impl Drop for PendingTunnelActivity {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let mut state = self.activity.state.lock().unwrap();
        debug_assert!(state.pending_open_count > 0);
        if state.pending_open_count > 0 {
            state.pending_open_count -= 1;
        }
        state.latest_active_at = Instant::now();
    }
}

struct TunnelWorkActivity {
    activity: Arc<TunnelActivity>,
    active: bool,
}

impl Drop for TunnelWorkActivity {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let mut state = self.activity.state.lock().unwrap();
        debug_assert!(state.work_instance_num > 0);
        if state.work_instance_num > 0 {
            state.work_instance_num -= 1;
        }
        state.latest_active_at = Instant::now();
        self.active = false;
    }
}

struct ActivityTrackedRead {
    inner: TunnelStreamRead,
    _lease: TunnelWorkActivity,
}

impl runtime::AsyncRead for ActivityTrackedRead {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut runtime::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        self.inner.as_mut().poll_read(cx, buf)
    }
}

struct ActivityTrackedWrite {
    inner: TunnelStreamWrite,
    _lease: TunnelWorkActivity,
}

impl runtime::AsyncWrite for ActivityTrackedWrite {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.inner.as_mut().poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.inner.as_mut().poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.inner.as_mut().poll_shutdown(cx)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        self.inner.as_mut().poll_write_vectored(cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }
}

#[async_trait::async_trait]
pub trait Tunnel: Send + Sync + 'static {
    fn tunnel_id(&self) -> TunnelId;
    fn candidate_id(&self) -> TunnelCandidateId;
    fn form(&self) -> TunnelForm;
    fn is_reverse(&self) -> bool;
    fn protocol(&self) -> Protocol;

    fn local_id(&self) -> P2pId;
    fn remote_id(&self) -> P2pId;

    fn local_ep(&self) -> Option<Endpoint>;
    fn remote_ep(&self) -> Option<Endpoint>;

    fn state(&self) -> TunnelState;
    fn is_closed(&self) -> bool;

    fn close(&self) -> P2pResult<()> {
        Ok(())
    }

    fn try_retire_idle(&self, _now: Instant, _idle_timeout: Duration) -> bool {
        false
    }

    async fn listen_stream(
        &self,
        vports: ListenVPortsRef,
        callback: IncomingStreamCallback,
    ) -> P2pResult<()>;
    async fn listen_datagram(
        &self,
        vports: ListenVPortsRef,
        callback: IncomingDatagramCallback,
    ) -> P2pResult<()>;
    async fn listen_control_stream(
        &self,
        _purposes: ListenVPortsRef,
        _callback: IncomingControlStreamCallback,
    ) -> P2pResult<()> {
        Err(P2pError::new(
            P2pErrorCode::NotSupport,
            "control stream is not supported".to_owned(),
        ))
    }

    async fn open_stream(
        &self,
        purpose: TunnelPurpose,
    ) -> P2pResult<(TunnelStreamRead, TunnelStreamWrite)>;

    async fn open_datagram(&self, purpose: TunnelPurpose) -> P2pResult<TunnelDatagramWrite>;
    async fn open_control_stream(
        &self,
        _purpose: TunnelPurpose,
    ) -> P2pResult<(TunnelStreamRead, TunnelStreamWrite)> {
        Err(P2pError::new(
            P2pErrorCode::NotSupport,
            "control stream is not supported".to_owned(),
        ))
    }
}

pub type TunnelRef = Arc<dyn Tunnel>;

#[cfg(test)]
mod activity_tests {
    use super::*;

    fn stream_handles() -> (TunnelStreamRead, TunnelStreamWrite) {
        let (stream, _peer) = tokio::io::duplex(64);
        let (read, write) = tokio::io::split(stream);
        (Box::pin(read), Box::pin(write))
    }

    #[test]
    fn idle_retirement_uses_strict_timeout_boundary() {
        let activity = TunnelActivity::new();
        let timeout = Duration::from_secs(30);
        let base = Instant::now();
        activity.state.lock().unwrap().latest_active_at = base;

        assert!(!activity.try_retire_idle(base + timeout, timeout));
        assert!(activity.try_retire_idle(
            base + timeout + Duration::from_nanos(1),
            timeout
        ));
    }

    #[test]
    fn pending_and_each_live_half_block_idle_retirement() {
        let activity = TunnelActivity::new();
        let pending = activity.begin_pending().unwrap();
        let now = Instant::now() + Duration::from_secs(60);
        assert!(!activity.try_retire_idle(now, Duration::ZERO));

        let (read, write) = stream_handles();
        let (read, write) = pending.promote_stream(read, write).unwrap();
        assert!(!activity.try_retire_idle(now, Duration::ZERO));
        drop(read);
        assert!(!activity.try_retire_idle(now, Duration::ZERO));

        let before_final_drop = Instant::now();
        drop(write);
        let latest_active_at = activity.state.lock().unwrap().latest_active_at;
        assert!(latest_active_at >= before_final_drop);
        assert!(!activity.try_retire_idle(latest_active_at, Duration::ZERO));
        assert!(activity.try_retire_idle(
            latest_active_at + Duration::from_nanos(1),
            Duration::ZERO
        ));
    }

    #[test]
    fn retired_tunnel_rejects_pending_promotion() {
        let activity = TunnelActivity::new();
        let pending = activity.begin_pending().unwrap();
        activity.retire();
        let (read, write) = stream_handles();

        let err = pending.promote_stream(read, write).err().unwrap();
        assert_eq!(err.code(), P2pErrorCode::Interrupted);
        let state = activity.state.lock().unwrap();
        assert_eq!(state.pending_open_count, 0);
        assert_eq!(state.work_instance_num, 0);
    }
}
