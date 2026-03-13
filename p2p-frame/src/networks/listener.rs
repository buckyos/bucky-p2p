use crate::error::P2pResult;
use std::sync::Arc;

use super::TunnelRef;

#[async_trait::async_trait]
pub trait TunnelListener: Send + Sync + 'static {
    async fn accept_tunnel(&self) -> P2pResult<TunnelRef>;
}

pub type TunnelListenerRef = Arc<dyn TunnelListener>;
