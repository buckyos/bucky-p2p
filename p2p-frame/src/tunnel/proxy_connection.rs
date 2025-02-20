use std::any::Any;
use std::cell::UnsafeCell;
use std::pin::Pin;
use std::sync::Mutex;
use std::task::{Context, Poll};
use tokio::io::ReadBuf;
use crate::endpoint::Endpoint;
use crate::error::{p2p_err, P2pErrorCode, P2pResult};
use crate::p2p_connection::{P2pConnection, P2pRead, P2pWrite};
use crate::p2p_identity::P2pId;
use crate::pn::{PnTunnelRead, PnTunnelWrite};
use crate::runtime;

pub struct ProxyConnectionRead {
    read: Box<dyn PnTunnelRead>,
    local_id: P2pId,
}

impl ProxyConnectionRead {
    pub fn new(read: Box<dyn PnTunnelRead>,
               local_id: P2pId,) -> Self {
        Self {
            read,
            local_id,
        }
    }
}

impl P2pRead for ProxyConnectionRead {
    fn remote(&self) -> Endpoint {
        Endpoint::default()
    }

    fn local(&self) -> Endpoint {
        Endpoint::default()
    }

    fn remote_id(&self) -> P2pId {
        self.read.remote_id()
    }

    fn local_id(&self) -> P2pId {
        self.local_id.clone()
    }
}

impl runtime::AsyncRead for ProxyConnectionRead {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.read).poll_read(cx, buf)
    }
}

pub struct ProxyConnectionWrite {
    write: Box<dyn PnTunnelWrite>,
    local_id: P2pId,
}

impl ProxyConnectionWrite {
    pub fn new(write: Box<dyn PnTunnelWrite>,
               local_id: P2pId,) -> Self {
        Self {
            write,
            local_id,
        }
    }
}

impl runtime::AsyncWrite for ProxyConnectionWrite {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.write).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.write).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.write).poll_shutdown(cx)
    }
}

impl P2pWrite for ProxyConnectionWrite {
    fn remote(&self) -> Endpoint {
        Endpoint::default()
    }

    fn local(&self) -> Endpoint {
        Endpoint::default()
    }

    fn remote_id(&self) -> P2pId {
        self.write.remote_id()
    }

    fn local_id(&self) -> P2pId {
        self.local_id.clone()
    }
}

