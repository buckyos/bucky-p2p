use crate::error::{P2pErrorCode, P2pResult, p2p_err};
use crate::networks::{TunnelStreamRead, TunnelStreamWrite};
use tokio::sync::{Mutex as AsyncMutex, mpsc};

use std::sync::Arc;

use super::{TtpDatagram, TtpStreamMeta};

#[async_trait::async_trait]
pub trait TtpListener: Send + Sync + 'static {
    async fn accept(&self) -> P2pResult<(TtpStreamMeta, TunnelStreamRead, TunnelStreamWrite)>;
}

pub type TtpListenerRef = Arc<dyn TtpListener>;

#[async_trait::async_trait]
pub trait TtpDatagramListener: Send + Sync + 'static {
    async fn accept(&self) -> P2pResult<TtpDatagram>;
}

pub type TtpDatagramListenerRef = Arc<dyn TtpDatagramListener>;

#[async_trait::async_trait]
pub trait TtpPortListener: Send + Sync + 'static {
    async fn listen_stream(&self, vport: u16) -> P2pResult<TtpListenerRef>;
    async fn unlisten_stream(&self, vport: u16) -> P2pResult<()>;

    async fn listen_datagram(&self, vport: u16) -> P2pResult<TtpDatagramListenerRef>;
    async fn unlisten_datagram(&self, vport: u16) -> P2pResult<()>;
}

pub(crate) struct QueueTtpListener {
    rx: AsyncMutex<
        mpsc::UnboundedReceiver<P2pResult<(TtpStreamMeta, TunnelStreamRead, TunnelStreamWrite)>>,
    >,
}

impl QueueTtpListener {
    pub(crate) fn new(
        rx: mpsc::UnboundedReceiver<
            P2pResult<(TtpStreamMeta, TunnelStreamRead, TunnelStreamWrite)>,
        >,
    ) -> TtpListenerRef {
        Arc::new(Self {
            rx: AsyncMutex::new(rx),
        })
    }
}

#[async_trait::async_trait]
impl TtpListener for QueueTtpListener {
    async fn accept(&self) -> P2pResult<(TtpStreamMeta, TunnelStreamRead, TunnelStreamWrite)> {
        let mut rx = self.rx.lock().await;
        match rx.recv().await {
            Some(result) => result,
            None => Err(p2p_err!(
                P2pErrorCode::Interrupted,
                "ttp stream listener closed"
            )),
        }
    }
}

pub(crate) struct QueueTtpDatagramListener {
    rx: AsyncMutex<mpsc::UnboundedReceiver<P2pResult<TtpDatagram>>>,
}

impl QueueTtpDatagramListener {
    pub(crate) fn new(
        rx: mpsc::UnboundedReceiver<P2pResult<TtpDatagram>>,
    ) -> TtpDatagramListenerRef {
        Arc::new(Self {
            rx: AsyncMutex::new(rx),
        })
    }
}

#[async_trait::async_trait]
impl TtpDatagramListener for QueueTtpDatagramListener {
    async fn accept(&self) -> P2pResult<TtpDatagram> {
        let mut rx = self.rx.lock().await;
        match rx.recv().await {
            Some(result) => result,
            None => Err(p2p_err!(
                P2pErrorCode::Interrupted,
                "ttp datagram listener closed"
            )),
        }
    }
}
