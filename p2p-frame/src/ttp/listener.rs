use crate::error::P2pResult;
use crate::networks::{TunnelPurpose, TunnelStreamRead, TunnelStreamWrite};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use super::{TtpDatagram, TtpStreamMeta};

pub type TtpIncomingStream = (TtpStreamMeta, TunnelStreamRead, TunnelStreamWrite);
pub type TtpIncomingStreamCallbackFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;
pub type TtpIncomingDatagramCallbackFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;
pub type TtpIncomingStreamCallback = Arc<
    dyn Fn(P2pResult<TtpIncomingStream>) -> TtpIncomingStreamCallbackFuture + Send + Sync + 'static,
>;
pub type TtpIncomingDatagramCallback = Arc<
    dyn Fn(P2pResult<TtpDatagram>) -> TtpIncomingDatagramCallbackFuture + Send + Sync + 'static,
>;

#[async_trait::async_trait]
pub trait TtpPortListener: Send + Sync + 'static {
    async fn listen_stream(
        &self,
        purpose: TunnelPurpose,
        callback: TtpIncomingStreamCallback,
    ) -> P2pResult<()>;
    async fn unlisten_stream(&self, purpose: &TunnelPurpose) -> P2pResult<()>;

    async fn listen_datagram(
        &self,
        purpose: TunnelPurpose,
        callback: TtpIncomingDatagramCallback,
    ) -> P2pResult<()>;
    async fn unlisten_datagram(&self, purpose: &TunnelPurpose) -> P2pResult<()>;
}
