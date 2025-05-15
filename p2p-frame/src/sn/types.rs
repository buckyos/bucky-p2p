use std::any::Any;
use std::pin::Pin;
use std::task::{Context, Poll};
use sfo_cmd_server::{CmdTunnel, CmdTunnelRead, CmdTunnelWrite, PeerId};
use sfo_cmd_server::client::{ClassifiedCmdTunnel, ClassifiedCmdTunnelRead, ClassifiedCmdTunnelWrite};
use sfo_cmd_server::errors::{into_cmd_err, CmdErrorCode, CmdResult};
use tokio::io::ReadBuf;
use crate::endpoint::{Endpoint, Protocol};
use crate::error::P2pErrorCode;
use crate::p2p_connection::{P2pConnection, P2pRead, P2pReadHalf, P2pWrite, P2pWriteHalf};
use crate::p2p_identity::P2pId;

#[derive(Clone, Debug)]
pub struct PingSessionResp {
    pub from: Endpoint,
    pub err: P2pErrorCode,
    pub endpoints: Vec<Endpoint>
}

#[derive(Clone, Debug, Hash, Eq)]
pub struct SnTunnelClassification {
    pub local_ep: Option<Endpoint>,
    pub remote_ep: Endpoint,
}

impl SnTunnelClassification {
    pub fn new(local_ep: Option<Endpoint>, remote_ep: Endpoint) -> Self {
        Self {
            local_ep,
            remote_ep,
        }
    }
}

impl PartialEq<Self> for SnTunnelClassification {
    fn eq(&self, other: &Self) -> bool {
        if let Some(local_ep) = other.local_ep.as_ref() {
            self.local_ep == other.local_ep && self.remote_ep == other.remote_ep
        } else {
            self.remote_ep == other.remote_ep
        }
    }
}

pub struct SnTunnelRead {
    read: P2pReadHalf,
}

impl SnTunnelRead {
    pub fn new(read: P2pReadHalf) -> Self {
        Self {
            read,
        }
    }

    pub fn remote(&self) -> Endpoint {
        self.read.remote()
    }

    pub fn local(&self) -> Endpoint {
        self.read.local()
    }

    pub fn remote_id(&self) -> P2pId {
        self.read.remote_id()
    }

    pub fn local_id(&self) -> P2pId {
        self.read.local_id()
    }
}

impl CmdTunnelRead<()> for SnTunnelRead {
    fn get_remote_peer_id(&self) -> PeerId {
        PeerId::from(self.read.remote_id().as_slice())
    }
}

impl ClassifiedCmdTunnelRead<SnTunnelClassification, ()> for SnTunnelRead {
    fn get_classification(&self) -> SnTunnelClassification {
        SnTunnelClassification::new(Some(self.read.local()), self.read.remote())
    }
}

impl tokio::io::AsyncRead for SnTunnelRead {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        Pin::new(this.read.as_mut()).poll_read(cx, buf)
    }
}

pub struct SnTunnelWrite {
    write: P2pWriteHalf,
}

impl SnTunnelWrite {
    pub fn new(write: P2pWriteHalf, ) -> Self {
        Self {
            write,
        }
    }

    pub fn remote(&self) -> Endpoint {
        self.write.remote()
    }

    pub fn local(&self) -> Endpoint {
        self.write.local()
    }

    pub fn remote_id(&self) -> P2pId {
        self.write.remote_id()
    }

    pub fn local_id(&self) -> P2pId {
        self.write.local_id()
    }
}

impl CmdTunnelWrite<()> for SnTunnelWrite {
    fn get_remote_peer_id(&self) -> PeerId {
        PeerId::from(self.write.remote_id().as_slice())
    }
}

impl ClassifiedCmdTunnelWrite<SnTunnelClassification, ()> for SnTunnelWrite {
    fn get_classification(&self) -> SnTunnelClassification {
        SnTunnelClassification::new(Some(self.write.local()), self.write.remote())
    }
}
impl tokio::io::AsyncWrite for SnTunnelWrite {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
        let this = self.get_mut();
        Pin::new(this.write.as_mut()).poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        Pin::new(this.write.as_mut()).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        Pin::new(this.write.as_mut()).poll_shutdown(cx)
    }
}

pub type SnCmdHeader = sfo_cmd_server::CmdHeader<u16, u8>;
pub type CmdTunnelId = sfo_cmd_server::TunnelId;
