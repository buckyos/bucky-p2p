use crate::endpoint::{Endpoint, Protocol};
use crate::error::P2pResult;
use crate::p2p_identity::P2pId;
use crate::types::{TunnelCandidateId, TunnelId};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct IncomingTunnelValidateContext {
    pub local_id: P2pId,
    pub remote_id: P2pId,
    pub protocol: Protocol,
    pub tunnel_id: TunnelId,
    pub candidate_id: TunnelCandidateId,
    pub is_reverse: bool,
    pub local_ep: Option<Endpoint>,
    pub remote_ep: Option<Endpoint>,
}

#[derive(Debug)]
pub enum ValidateResult {
    Accept,
    Reject(String),
}

#[async_trait::async_trait]
pub trait IncomingTunnelValidator: Send + Sync + 'static {
    async fn validate(&self, ctx: &IncomingTunnelValidateContext) -> P2pResult<ValidateResult>;
}

pub type IncomingTunnelValidatorRef = Arc<dyn IncomingTunnelValidator>;

pub struct AllowAllIncomingTunnelValidator;

#[async_trait::async_trait]
impl IncomingTunnelValidator for AllowAllIncomingTunnelValidator {
    async fn validate(&self, _ctx: &IncomingTunnelValidateContext) -> P2pResult<ValidateResult> {
        Ok(ValidateResult::Accept)
    }
}

pub fn allow_all_incoming_tunnel_validator() -> IncomingTunnelValidatorRef {
    Arc::new(AllowAllIncomingTunnelValidator)
}
