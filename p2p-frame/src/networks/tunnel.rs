use crate::endpoint::{Endpoint, Protocol};
use crate::error::{P2pError, P2pErrorCode, P2pResult};
use crate::p2p_identity::P2pId;
use crate::runtime;
use crate::types::{TunnelCandidateId, TunnelId};
use bucky_raw_codec::{RawDecode, RawEncode};
use std::pin::Pin;
use std::sync::Arc;

pub type TunnelStreamRead = Pin<Box<dyn runtime::AsyncRead + Send + Unpin + 'static>>;
pub type TunnelStreamWrite = Pin<Box<dyn runtime::AsyncWrite + Send + Unpin + 'static>>;
pub type TunnelDatagramRead = Pin<Box<dyn runtime::AsyncRead + Send + Unpin + 'static>>;
pub type TunnelDatagramWrite = Pin<Box<dyn runtime::AsyncWrite + Send + Unpin + 'static>>;

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
    fn is_listen(&self, vport: u16) -> bool;
}

pub type ListenVPortsRef = Arc<dyn ListenVPorts>;

pub struct AllowAllListenVPorts;

impl ListenVPorts for AllowAllListenVPorts {
    fn is_listen(&self, _vport: u16) -> bool {
        true
    }
}

pub fn allow_all_listen_vports() -> ListenVPortsRef {
    Arc::new(AllowAllListenVPorts)
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
    fn is_listen(&self, vport: u16) -> bool {
        self.contains(vport)
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

    async fn close(&self) -> P2pResult<()>;

    async fn listen_stream(&self, vports: ListenVPortsRef) -> P2pResult<()>;
    async fn listen_datagram(&self, vports: ListenVPortsRef) -> P2pResult<()>;

    async fn open_stream(&self, vport: u16) -> P2pResult<(TunnelStreamRead, TunnelStreamWrite)>;
    async fn accept_stream(&self) -> P2pResult<(u16, TunnelStreamRead, TunnelStreamWrite)>;

    async fn open_datagram(&self, vport: u16) -> P2pResult<TunnelDatagramWrite>;
    async fn accept_datagram(&self) -> P2pResult<(u16, TunnelDatagramRead)>;
}

pub type TunnelRef = Arc<dyn Tunnel>;
