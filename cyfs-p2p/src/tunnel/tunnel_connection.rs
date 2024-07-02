use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::net::Shutdown;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;
use as_any::AsAny;
use bucky_objects::DeviceId;
use bucky_raw_codec::RawEncode;
use callback_result::{CallbackWaiter, SingleCallbackWaiter};
use notify_future::NotifyFuture;
use crate::protocol::{AckTunnel, Package, PackageCmdCode, PackageHeader, SynTunnel};
use crate::sockets::{NetManagerRef};
use crate::{IncreaseId, LocalDeviceRef, MixAesKey, runtime, TempSeq};
use crate::error::{bdt_err, BdtErrorCode, BdtResult, into_bdt_err};
use crate::history::keystore::{FoundKey, Keystore};
use crate::protocol::v0::{AckStream, SynStream};

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

#[async_trait::async_trait]
pub trait TunnelStream: 'static + Send + runtime::AsyncWrite + runtime::AsyncRead {
    fn port(&self) -> u16;
    async fn close(&mut self) -> BdtResult<()>;
}

#[async_trait::async_trait]
pub trait TunnelDatagramSend: 'static + Send + runtime::AsyncWrite {
    async fn close(&mut self) -> BdtResult<()>;
}

#[async_trait::async_trait]
pub trait TunnelDatagramRecv: 'static + Send + runtime::AsyncRead {
    async fn close(&mut self) -> BdtResult<()>;
}

pub enum TunnelInstance {
    Stream(Box<dyn TunnelStream>),
    Datagram(Box<dyn TunnelDatagramRecv>),
}

#[async_trait::async_trait]
pub(crate) trait TunnelConnection: AsAny + Send + Sync {
    fn socket_type(&self) -> SocketType;
    fn is_idle(&self) -> bool;
    async fn connect(&self) -> BdtResult<()>;
    async fn open_stream(&self, vport: u16, session_id: IncreaseId) -> BdtResult<Box<dyn TunnelStream>>;
    async fn open_datagram(&self) -> BdtResult<Box<dyn TunnelDatagramSend>>;
    async fn accept_instance(&self) -> BdtResult<TunnelInstance>;
    async fn shutdown(&self) -> BdtResult<()>;
}
