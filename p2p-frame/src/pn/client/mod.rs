mod pn_client;

use crate::error::P2pResult;
use crate::p2p_identity::P2pId;
use crate::runtime;
use crate::types::TunnelId;
pub use pn_client::*;
use std::sync::Arc;

pub trait PnTunnelRead: 'static + Send + runtime::AsyncRead + Unpin {
    fn tunnel_id(&self) -> TunnelId;
    fn remote_id(&self) -> P2pId;
    fn remote_name(&self) -> String;
}

pub trait PnTunnelWrite: 'static + Send + runtime::AsyncWrite + Unpin {
    fn tunnel_id(&self) -> TunnelId;
    fn remote_id(&self) -> P2pId;
    fn remote_name(&self) -> String;
}

#[async_trait::async_trait]
pub trait PnClient: 'static + Send + Sync {
    async fn accept(&self) -> P2pResult<(Box<dyn PnTunnelRead>, Box<dyn PnTunnelWrite>)>;
    async fn connect(
        &self,
        tunnel_id: TunnelId,
        to: P2pId,
        to_name: Option<String>,
    ) -> P2pResult<(Box<dyn PnTunnelRead>, Box<dyn PnTunnelWrite>)>;
}
pub type PnClientRef = Arc<dyn PnClient>;
