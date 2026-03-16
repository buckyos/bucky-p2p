use crate::endpoint::{Endpoint, Protocol};
use crate::error::P2pErrorCode;
use crate::networks::{TunnelStreamRead, TunnelStreamWrite};
use crate::p2p_identity::P2pId;
use sfo_cmd_server::client::{
    ClassifiedCmdTunnel, ClassifiedCmdTunnelRead, ClassifiedCmdTunnelWrite,
};
use sfo_cmd_server::{CmdTunnel, CmdTunnelRead, CmdTunnelWrite, PeerId};
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::ReadBuf;

#[derive(Clone, Debug)]
pub struct PingSessionResp {
    pub from: Endpoint,
    pub err: P2pErrorCode,
    pub endpoints: Vec<Endpoint>,
}

pub const SN_CMD_SERVICE: &str = "sn_service";

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
    read: TunnelStreamRead,
    prefix: Option<(Vec<u8>, usize)>,
    local: Endpoint,
    remote: Endpoint,
    local_id: P2pId,
    remote_id: P2pId,
}

impl SnTunnelRead {
    pub fn new(
        read: TunnelStreamRead,
        local: Endpoint,
        remote: Endpoint,
        local_id: P2pId,
        remote_id: P2pId,
    ) -> Self {
        Self {
            read,
            prefix: None,
            local,
            remote,
            local_id,
            remote_id,
        }
    }

    pub fn new_with_prefix(
        read: TunnelStreamRead,
        local: Endpoint,
        remote: Endpoint,
        local_id: P2pId,
        remote_id: P2pId,
        prefix: Vec<u8>,
    ) -> Self {
        Self {
            read,
            prefix: Some((prefix, 0)),
            local,
            remote,
            local_id,
            remote_id,
        }
    }

    pub fn remote(&self) -> Endpoint {
        self.remote
    }

    pub fn local(&self) -> Endpoint {
        self.local
    }

    pub fn remote_id(&self) -> P2pId {
        self.remote_id.clone()
    }

    pub fn local_id(&self) -> P2pId {
        self.local_id.clone()
    }
}

impl CmdTunnelRead<()> for SnTunnelRead {
    fn get_local_peer_id(&self) -> PeerId {
        PeerId::from(self.local_id.as_slice())
    }

    fn get_remote_peer_id(&self) -> PeerId {
        PeerId::from(self.remote_id.as_slice())
    }
}

impl ClassifiedCmdTunnelRead<SnTunnelClassification, ()> for SnTunnelRead {
    fn get_classification(&self) -> SnTunnelClassification {
        SnTunnelClassification::new(Some(self.local), self.remote)
    }
}

impl tokio::io::AsyncRead for SnTunnelRead {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        if let Some((prefix, offset)) = self.prefix.as_mut() {
            if *offset < prefix.len() {
                let remain = &prefix[*offset..];
                let n = std::cmp::min(remain.len(), buf.remaining());
                if n > 0 {
                    buf.put_slice(&remain[..n]);
                    *offset += n;
                }
                if *offset == prefix.len() {
                    self.prefix = None;
                }
                return Poll::Ready(Ok(()));
            }
            self.prefix = None;
        }
        self.read.as_mut().poll_read(cx, buf)
    }
}

pub struct SnTunnelWrite {
    write: TunnelStreamWrite,
    local: Endpoint,
    remote: Endpoint,
    local_id: P2pId,
    remote_id: P2pId,
}

impl SnTunnelWrite {
    pub fn new(
        write: TunnelStreamWrite,
        local: Endpoint,
        remote: Endpoint,
        local_id: P2pId,
        remote_id: P2pId,
    ) -> Self {
        Self {
            write,
            local,
            remote,
            local_id,
            remote_id,
        }
    }

    pub fn remote(&self) -> Endpoint {
        self.remote
    }

    pub fn local(&self) -> Endpoint {
        self.local
    }

    pub fn remote_id(&self) -> P2pId {
        self.remote_id.clone()
    }

    pub fn local_id(&self) -> P2pId {
        self.local_id.clone()
    }
}

impl CmdTunnelWrite<()> for SnTunnelWrite {
    fn get_local_peer_id(&self) -> PeerId {
        PeerId::from(self.local_id.as_slice())
    }

    fn get_remote_peer_id(&self) -> PeerId {
        PeerId::from(self.remote_id.as_slice())
    }
}

impl ClassifiedCmdTunnelWrite<SnTunnelClassification, ()> for SnTunnelWrite {
    fn get_classification(&self) -> SnTunnelClassification {
        SnTunnelClassification::new(Some(self.local), self.remote)
    }
}
impl tokio::io::AsyncWrite for SnTunnelWrite {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let this = self.get_mut();
        this.write.as_mut().poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        this.write.as_mut().poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        this.write.as_mut().poll_shutdown(cx)
    }
}

pub type SnCmdHeader = sfo_cmd_server::CmdHeader<u16, u8>;
pub type CmdTunnelId = sfo_cmd_server::TunnelId;
