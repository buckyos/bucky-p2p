use crate::endpoint::{Endpoint, Protocol};
use crate::error::P2pResult;
use crate::p2p_identity::{P2pId, P2pIdentityRef};
use crate::types::{TunnelCandidateId, TunnelId};
use std::sync::Arc;

use super::{TunnelListenerRef, TunnelRef};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct TunnelListenerInfo {
    pub local: Endpoint,
    pub mapping_port: Option<u16>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Default)]
pub struct TunnelConnectIntent {
    pub tunnel_id: TunnelId,
    pub candidate_id: TunnelCandidateId,
    pub is_reverse: bool,
    pub udp_punch_enabled: bool,
}

impl TunnelConnectIntent {
    pub fn active_logical(tunnel_id: TunnelId) -> Self {
        Self::active(tunnel_id, TunnelCandidateId::default())
    }

    pub fn active(tunnel_id: TunnelId, candidate_id: TunnelCandidateId) -> Self {
        Self {
            tunnel_id,
            candidate_id,
            is_reverse: false,
            udp_punch_enabled: false,
        }
    }

    pub fn reverse_logical(tunnel_id: TunnelId) -> Self {
        Self::reverse(tunnel_id, TunnelCandidateId::default())
    }

    pub fn reverse(tunnel_id: TunnelId, candidate_id: TunnelCandidateId) -> Self {
        Self {
            tunnel_id,
            candidate_id,
            is_reverse: true,
            udp_punch_enabled: false,
        }
    }

    pub fn set_udp_punch_enabled(mut self, udp_punch_enabled: bool) -> Self {
        self.udp_punch_enabled = udp_punch_enabled;
        self
    }
}

#[async_trait::async_trait]
pub trait TunnelNetwork: Send + Sync + 'static {
    fn protocol(&self) -> Protocol;
    fn is_udp(&self) -> bool;
    fn set_reuse_address(&self, _reuse_address: bool) {}

    async fn listen(
        &self,
        local: &Endpoint,
        out: Option<Endpoint>,
        mapping_port: Option<u16>,
    ) -> P2pResult<TunnelListenerRef>;

    async fn close_all_listener(&self) -> P2pResult<()>;
    fn listeners(&self) -> Vec<TunnelListenerRef>;
    fn listener_infos(&self) -> Vec<TunnelListenerInfo>;

    async fn create_tunnel(
        &self,
        local_identity: &P2pIdentityRef,
        remote: &Endpoint,
        remote_id: &P2pId,
        remote_name: Option<String>,
    ) -> P2pResult<TunnelRef> {
        self.create_tunnel_with_intent(
            local_identity,
            remote,
            remote_id,
            remote_name,
            TunnelConnectIntent::default(),
        )
        .await
    }

    async fn create_tunnel_with_intent(
        &self,
        local_identity: &P2pIdentityRef,
        remote: &Endpoint,
        remote_id: &P2pId,
        remote_name: Option<String>,
        intent: TunnelConnectIntent,
    ) -> P2pResult<TunnelRef>;

    async fn create_tunnel_with_local_ep(
        &self,
        local_identity: &P2pIdentityRef,
        local_ep: &Endpoint,
        remote: &Endpoint,
        remote_id: &P2pId,
        remote_name: Option<String>,
    ) -> P2pResult<TunnelRef> {
        self.create_tunnel_with_local_ep_and_intent(
            local_identity,
            local_ep,
            remote,
            remote_id,
            remote_name,
            TunnelConnectIntent::default(),
        )
        .await
    }

    async fn create_tunnel_with_local_ep_and_intent(
        &self,
        local_identity: &P2pIdentityRef,
        local_ep: &Endpoint,
        remote: &Endpoint,
        remote_id: &P2pId,
        remote_name: Option<String>,
        intent: TunnelConnectIntent,
    ) -> P2pResult<TunnelRef>;
}

pub type TunnelNetworkRef = Arc<dyn TunnelNetwork>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tunnel_connect_intent_controls_udp_punch_per_connection_with_default_off() {
        let intent = TunnelConnectIntent::active_logical(TunnelId::from(7));
        assert!(!intent.udp_punch_enabled);

        let intent = intent.set_udp_punch_enabled(true);
        assert!(intent.udp_punch_enabled);
        assert_eq!(intent.tunnel_id, TunnelId::from(7));
        assert!(!intent.is_reverse);
    }
}
