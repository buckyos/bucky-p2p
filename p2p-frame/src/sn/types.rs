use std::any::Any;
use std::pin::Pin;
use std::task::{Context, Poll};
use sfo_cmd_server::{CmdTunnel, CmdTunnelRead, CmdTunnelWrite, PeerId};
use sfo_cmd_server::client::ClassifiedCmdTunnel;
use sfo_cmd_server::errors::{into_cmd_err, CmdErrorCode, CmdResult};
use tokio::io::ReadBuf;
use crate::endpoint::{Endpoint, Protocol};
use crate::error::P2pErrorCode;
use crate::p2p_connection::{P2pConnectionRef, P2pRead, P2pWrite};

#[derive(Clone, Debug)]
pub struct PingSessionResp {
    pub from: Endpoint,
    pub err: P2pErrorCode,
    pub endpoints: Vec<Endpoint>
}

pub struct SnTunnel {
    conn: P2pConnectionRef,
}

impl SnTunnel {
    pub(crate) fn new(conn: P2pConnectionRef) -> Self {
        Self {
            conn,
        }
    }

    pub fn p2p_connection(&self) -> &P2pConnectionRef {
        &self.conn
    }
}

impl CmdTunnel for SnTunnel {
    fn get_remote_peer_id(&self) -> PeerId {
        PeerId::from(self.conn.remote_id().as_slice())
    }

    fn split(&self) -> CmdResult<(Box<dyn CmdTunnelRead>, Box<dyn CmdTunnelWrite>)> {
        let (read, write) = self.conn.split().map_err(into_cmd_err!(CmdErrorCode::Failed))?;
        Ok((Box::new(SnTunnelRead::new(read)), Box::new(SnTunnelWrite::new(write))))
    }

    fn unsplit(&self, mut read: Box<dyn CmdTunnelRead>, mut write: Box<dyn CmdTunnelWrite>) -> CmdResult<()> {
        let read = unsafe { std::mem::transmute::<_, &mut dyn Any>(read.get_any_mut())};
        if let Some(read) = read.downcast_mut::<SnTunnelRead>() {
            if let Some(read) = read.take() {
                let write = unsafe { std::mem::transmute::<_, &mut dyn Any>(write.get_any_mut())};
                if let Some(write) = write.downcast_mut::<SnTunnelWrite>() {
                    if let Some(write) = write.take() {
                        self.conn.unsplit(read, write);
                    }
                }
            }
        }
        Ok(())
    }
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

impl ClassifiedCmdTunnel<SnTunnelClassification> for SnTunnel {
    fn get_classification(&self) -> SnTunnelClassification {
        SnTunnelClassification::new(Some(self.conn.local()), self.conn.remote())
    }
}

pub(crate) struct SnTunnelRead {
    read: Option<Box<dyn P2pRead>>
}

impl SnTunnelRead {
    pub fn new(read: Box<dyn P2pRead>) -> Self {
        Self {
            read: Some(read),
        }
    }

    pub fn take(&mut self) -> Option<Box<dyn P2pRead>> {
        self.read.take()
    }
}

impl CmdTunnelRead for SnTunnelRead {
    fn get_any(&self) -> &dyn Any {
        self
    }

    fn get_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl tokio::io::AsyncRead for SnTunnelRead {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        if let Some(read) = this.read.as_mut() {
            Pin::new(read).poll_read(cx, buf)
        } else {
            Poll::Ready(Ok(()))
        }
    }
}

pub(crate) struct SnTunnelWrite {
    write: Option<Box<dyn P2pWrite>>
}

impl SnTunnelWrite {
    pub fn new(write: Box<dyn P2pWrite>) -> Self {
        Self {
            write: Some(write),
        }
    }

    pub fn take(&mut self) -> Option<Box<dyn P2pWrite>> {
        self.write.take()
    }
}

impl CmdTunnelWrite for SnTunnelWrite {
    fn get_any(&self) -> &dyn Any {
        self
    }

    fn get_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl tokio::io::AsyncWrite for SnTunnelWrite {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
        let this = self.get_mut();
        if let Some(write) = this.write.as_mut() {
            Pin::new(write).poll_write(cx, buf)
        } else {
            Poll::Ready(Ok(0))
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        if let Some(write) = this.write.as_mut() {
            Pin::new(write).poll_flush(cx)
        } else {
            Poll::Ready(Ok(()))
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        if let Some(write) = this.write.as_mut() {
            Pin::new(write).poll_shutdown(cx)
        } else {
            Poll::Ready(Ok(()))
        }
    }
}

pub type SnCmdHeader = sfo_cmd_server::CmdHeader<u16, u8>;
pub type CmdTunnelId = sfo_cmd_server::TunnelId;
