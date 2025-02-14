mod pn_client;

use std::sync::Arc;
pub use pn_client::*;
use crate::error::P2pResult;
use crate::p2p_identity::P2pId;
use crate::runtime;
use crate::types::TunnelId;

pub trait PnTunnelRead: 'static + Send + runtime::AsyncRead + Unpin {
    fn tunnel_id(&self) -> TunnelId;
    fn remote_id(&self) -> P2pId;
}

pub trait PnTunnelWrite: 'static + Send + runtime::AsyncWrite + Unpin {
    fn tunnel_id(&self) -> TunnelId;
    fn remote_id(&self) -> P2pId;
}

#[async_trait::async_trait]
pub trait PnClient: 'static + Send + Sync {
    async fn accept(&self) -> P2pResult<(Box<dyn PnTunnelRead>, Box<dyn PnTunnelWrite>)>;
    async fn connect(&self, tunnel_id: TunnelId, to: P2pId) -> P2pResult<(Box<dyn PnTunnelRead>, Box<dyn PnTunnelWrite>)>;
}
pub type PnClientRef = Arc<dyn PnClient>;
