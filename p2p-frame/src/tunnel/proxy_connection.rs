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
    read: Option<Box<dyn PnTunnelRead>>,
}

impl ProxyConnectionRead {
    pub fn new(read: Box<dyn PnTunnelRead>) -> Self {
        Self {
            read: Some(read)
        }
    }

    pub fn take(&mut self) -> Option<Box<dyn PnTunnelRead>> {
        self.read.take()
    }
}
impl P2pRead for ProxyConnectionRead {
    fn get_any(&self) -> &dyn Any {
        self
    }

    fn get_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl runtime::AsyncRead for ProxyConnectionRead {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        let read = self.read.as_mut().unwrap();
        Pin::new(read).poll_read(cx, buf)
    }
}

pub struct ProxyConnectionWrite {
    write: Option<Box<dyn PnTunnelWrite>>
}

impl ProxyConnectionWrite {
    pub fn new(write: Box<dyn PnTunnelWrite>) -> Self {
        Self {
            write: Some(write)
        }
    }

    pub fn take(&mut self) -> Option<Box<dyn PnTunnelWrite>> {
        self.write.take()
    }
}

impl runtime::AsyncWrite for ProxyConnectionWrite {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
        Pin::new(self.write.as_mut().unwrap()).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(self.write.as_mut().unwrap()).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(self.write.as_mut().unwrap()).poll_shutdown(cx)
    }
}

impl P2pWrite for ProxyConnectionWrite {
    fn get_any(&self) -> &dyn Any {
        self
    }

    fn get_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

struct ProxyConnectionState {
    read: Option<Box<dyn PnTunnelRead>>,
    write: Option<Box<dyn PnTunnelWrite>>
}

pub struct ProxyConnection {
    state: Mutex<ProxyConnectionState>,
    remote_id: P2pId,
    local_id: P2pId,
}

impl ProxyConnection {
    pub fn new(remote_id: P2pId, local_id: P2pId, read: Box<dyn PnTunnelRead>, write: Box<dyn PnTunnelWrite>) -> Self {
        Self {
            state: Mutex::new(ProxyConnectionState {
                read: Some(read),
                write: Some(write)
            }),
            remote_id,
            local_id
        }
    }
}

impl Drop for ProxyConnection {
    fn drop(&mut self) {
        log::info!("proxy connection drop remote_id: {}, local_id: {}", self.remote_id, self.local_id);
    }
}

impl P2pConnection for ProxyConnection {
    fn is_stream(&self) -> bool {
        true
    }

    fn remote(&self) -> Endpoint {
        Endpoint::default()
    }

    fn local(&self) -> Endpoint {
        Endpoint::default()
    }

    fn remote_id(&self) -> P2pId {
        self.remote_id.clone()
    }

    fn local_id(&self) -> P2pId {
        self.local_id.clone()
    }

    fn split(&self) -> P2pResult<(Box<dyn P2pRead>, Box<dyn P2pWrite>)> {
        let (read, write) = {
            let mut state = self.state.lock().unwrap();
            if state.read.is_none() || state.write.is_none() {
                return Err(p2p_err!(P2pErrorCode::Failed, "proxy connection split failed"));
            }
            (state.read.take().unwrap(), state.write.take().unwrap())
        };
        Ok((Box::new(ProxyConnectionRead::new(read)), Box::new(ProxyConnectionWrite::new(write))))
    }

    fn unsplit(&self, mut read: Box<dyn P2pRead>, mut write: Box<dyn P2pWrite>) {
        let mut read = unsafe { std::mem::transmute::<_, &mut dyn Any>(read.get_any_mut())};
        if let Some(read) = read.downcast_mut::<ProxyConnectionRead>() {
            if let Some(read) = read.take() {
                let mut write = unsafe { std::mem::transmute::<_, &mut dyn Any>(write.get_any_mut())};
                if let Some(write) = write.downcast_mut::<ProxyConnectionWrite>() {
                    if let Some(write) = write.take() {
                        let mut state = self.state.lock().unwrap();
                        state.read = Some(read);
                        state.write = Some(write);
                    }
                }
            }
        }
    }
}
