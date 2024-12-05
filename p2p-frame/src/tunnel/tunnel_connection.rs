use std::fmt::{Display, Formatter};
use std::future::Future;
use std::sync::{Arc, Mutex};
use as_any::AsAny;
use bucky_time::bucky_time_now;
use futures::FutureExt;
use crate::endpoint::Endpoint;
use crate::error::{bdt_err, P2pErrorCode, P2pResult};
use crate::p2p_identity::P2pId;
use crate::runtime;
use crate::types::{IncreaseId, TempSeq};

pub trait TunnelListenPorts: 'static + Send + Sync {
    fn is_listen(&self, port: u16) -> bool;
}
pub type TunnelListenPortsRef = Arc<dyn TunnelListenPorts>;

#[derive(Debug, Clone, Copy, Ord, PartialOrd, PartialEq, Eq)]
pub enum TunnelType {
    IDLE,
    TUNNEL,
    STREAM(u16),
    ERROR
}

#[derive(Copy, Clone, Eq, PartialEq)]
pub enum SocketType {
    TCP,
    UDP
}

impl TunnelType {
    pub fn is_tunnel(&self) -> bool {
        matches!(self, TunnelType::IDLE)
    }

    pub fn is_stream(&self) -> bool {
        matches!(self, TunnelType::STREAM(_))
    }

    pub fn get_vport(&self) -> Option<u16> {
        match self {
            TunnelType::STREAM(vport) => Some(*vport),
            _ => None,
        }
    }
}

struct TunnelStatState {
    pub work_instance_num: u32,
    pub latest_active_time: u64,
}

pub struct TunnelStat {
    state: Mutex<TunnelStatState>
}
pub type TunnelStatRef = Arc<TunnelStat>;

impl TunnelStat {
    pub fn new() -> TunnelStatRef {
        Arc::new(TunnelStat {
            state: Mutex::new(TunnelStatState {
                work_instance_num: 0,
                latest_active_time: 0,
            })
        })
    }

    pub fn increase_work_instance(&self) {
        let mut state = self.state.lock().unwrap();
        state.work_instance_num += 1;
        state.latest_active_time = bucky_time_now();
    }

    pub fn decrease_work_instance(&self) {
        let mut state = self.state.lock().unwrap();
        state.work_instance_num = state.work_instance_num - 1;
        state.latest_active_time = bucky_time_now();
    }

    pub fn get_work_instance_num(&self) -> u32 {
        let state = self.state.lock().unwrap();
        state.work_instance_num
    }

    pub fn get_latest_active_time(&self) -> u64 {
        let state = self.state.lock().unwrap();
        state.latest_active_time
    }
}
#[async_trait::async_trait]
pub trait TunnelStream: 'static + Send + runtime::AsyncWrite + runtime::AsyncRead + Unpin {
    fn port(&self) -> u16;
    fn session_id(&self) -> IncreaseId;
    fn sequence(&self) -> TempSeq;
    fn remote_identity_id(&self) -> P2pId;
    fn local_identity_id(&self) -> P2pId;
    fn remote_endpoint(&self) -> Endpoint;
    fn local_endpoint(&self) -> Endpoint;
    async fn close(&mut self) -> P2pResult<()>;
}

#[async_trait::async_trait]
pub trait TunnelDatagramSend: 'static + Send + runtime::AsyncWrite {
    fn sequence(&self) -> TempSeq;
    fn remote_identity_id(&self) -> P2pId;
    fn local_identity_id(&self) -> P2pId;
    fn remote_endpoint(&self) -> Endpoint;
    fn local_endpoint(&self) -> Endpoint;
    async fn close(&mut self) -> P2pResult<()>;
}

#[async_trait::async_trait]
pub trait TunnelDatagramRecv: 'static + Send + runtime::AsyncRead {
    fn sequence(&self) -> TempSeq;
    fn remote_identity_id(&self) -> P2pId;
    fn local_identity_id(&self) -> P2pId;
    fn remote_endpoint(&self) -> Endpoint;
    fn local_endpoint(&self) -> Endpoint;
    async fn close(&mut self) -> P2pResult<()>;
}

pub enum TunnelInstance {
    Stream(Box<dyn TunnelStream>),
    Datagram(Box<dyn TunnelDatagramRecv>),
    ReverseStream(Box<dyn TunnelStream>),
    ReverseDatagram(Box<dyn TunnelDatagramSend>),
}

impl Display for TunnelInstance {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            TunnelInstance::Stream(stream) => write!(f, "Stream(seq {:?} session {} port {})", stream.sequence(), stream.session_id(), stream.port()),
            TunnelInstance::Datagram(datagram) => write!(f, "Datagram({:?})", datagram.sequence()),
            TunnelInstance::ReverseStream(stream) => write!(f, "ReverseStream(seq {:?} session {} port {})", stream.sequence(), stream.session_id(), stream.port()),
            TunnelInstance::ReverseDatagram(datagram) => write!(f, "ReverseDatagram({:?})", datagram.sequence()),
        }
    }
}
#[async_trait::async_trait]
pub trait TunnelConnection: AsAny + Send + Sync {
    fn get_sequence(&self) -> TempSeq;
    fn socket_type(&self) -> SocketType;
    fn is_idle(&self) -> bool;
    fn tunnel_stat(&self) -> TunnelStatRef;
    async fn connect_stream(&self, vport: u16, session_id: IncreaseId) -> P2pResult<Box<dyn TunnelStream>>;
    async fn connect_datagram(&self) -> P2pResult<Box<dyn TunnelDatagramSend>>;
    async fn connect_reverse_stream(&self, vport: u16, session_id: IncreaseId) -> P2pResult<Box<dyn TunnelStream>>;
    async fn connect_reverse_datagram(&self) -> P2pResult<Box<dyn TunnelDatagramRecv>>;
    async fn open_stream(&self, vport: u16, session_id: IncreaseId) -> P2pResult<Box<dyn TunnelStream>>;
    async fn open_datagram(&self) -> P2pResult<Box<dyn TunnelDatagramSend>>;
    async fn accept_instance(&self) -> P2pResult<TunnelInstance>;
    async fn shutdown(&self) -> P2pResult<()>;
}

pub async fn select_successful<T, E, F>(futures: Vec<F>) -> P2pResult<T>
where
    F: Future<Output=Result<T, E>> + Unpin,
{
    let mut futures = futures.into_iter().map(FutureExt::fuse).collect::<Vec<_>>();

    while futures.len() > 0 {
        let select_all = futures::future::select_all(futures);
        match select_all.await {
            (Ok(result), _index, _remaining) => {
                return Ok(result);
            },
            (Err(_), _index, remaining) => {
                futures = remaining;
            },
        }
    };
    Err(bdt_err!(P2pErrorCode::ConnectFailed, "connect failed"))
}
