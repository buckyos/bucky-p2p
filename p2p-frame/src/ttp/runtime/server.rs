use std::sync::Arc;

use crate::endpoint::{Endpoint, Protocol};
use crate::error::P2pResult;
use crate::networks::{
    NetManagerRef, TunnelDatagramWrite, TunnelPurpose, TunnelStreamRead, TunnelStreamWrite,
    ValidateResult,
};
use crate::p2p_identity::{P2pId, P2pIdentityRef};
use crate::types::{TunnelCandidateId, TunnelId};

use super::super::client::TtpConnector;
use super::super::listener::{
    TtpIncomingControlStreamCallback, TtpIncomingDatagramCallback, TtpIncomingStreamCallback,
    TtpPortListener,
};
use super::super::types::{TtpDatagramMeta, TtpStreamMeta, TtpTarget};
use super::handle::TtpRuntime;

pub struct TtpServer {
    runtime: TtpRuntime,
}

pub type TtpServerRef = Arc<TtpServer>;

#[derive(Debug, Clone)]
pub struct TtpIncomingTunnelValidateContext {
    pub local_id: P2pId,
    pub remote_id: P2pId,
    pub protocol: Protocol,
    pub tunnel_id: TunnelId,
    pub candidate_id: TunnelCandidateId,
    pub local_ep: Option<Endpoint>,
    pub remote_ep: Option<Endpoint>,
}

#[async_trait::async_trait]
pub trait TtpIncomingTunnelValidator: Send + Sync + 'static {
    async fn validate(&self, ctx: &TtpIncomingTunnelValidateContext) -> P2pResult<ValidateResult>;
}

pub type TtpIncomingTunnelValidatorRef = Arc<dyn TtpIncomingTunnelValidator>;

pub struct AllowAllTtpIncomingTunnelValidator;

#[async_trait::async_trait]
impl TtpIncomingTunnelValidator for AllowAllTtpIncomingTunnelValidator {
    async fn validate(&self, _ctx: &TtpIncomingTunnelValidateContext) -> P2pResult<ValidateResult> {
        Ok(ValidateResult::Accept)
    }
}

pub fn allow_all_ttp_incoming_tunnel_validator() -> TtpIncomingTunnelValidatorRef {
    Arc::new(AllowAllTtpIncomingTunnelValidator)
}

impl TtpServer {
    pub fn new(
        local_identity: P2pIdentityRef,
        net_manager: NetManagerRef,
    ) -> P2pResult<TtpServerRef> {
        Self::new_with_incoming_tunnel_validator(
            local_identity,
            net_manager,
            allow_all_ttp_incoming_tunnel_validator(),
        )
    }

    pub fn new_with_incoming_tunnel_validator(
        local_identity: P2pIdentityRef,
        net_manager: NetManagerRef,
        incoming_tunnel_validator: TtpIncomingTunnelValidatorRef,
    ) -> P2pResult<TtpServerRef> {
        let runtime = TtpRuntime::new(local_identity, net_manager, incoming_tunnel_validator)?;
        Ok(Self::new_with_runtime(runtime))
    }

    pub fn new_with_runtime(runtime: TtpRuntime) -> TtpServerRef {
        Arc::new(Self { runtime })
    }

    pub fn runtime(&self) -> TtpRuntime {
        self.runtime.clone()
    }

    fn get_existing_tunnel(&self, target: &TtpTarget) -> P2pResult<crate::networks::TunnelRef> {
        self.runtime.core().get_existing_tunnel(target)
    }
}

#[cfg(test)]
impl TtpServer {
    pub(crate) fn has_cached_tunnel_for_test(&self, target: &TtpTarget) -> bool {
        self.runtime.core().has_cached_tunnel_for_test(target)
    }

    pub(crate) fn cache_snapshot_for_test(
        &self,
        target: &TtpTarget,
    ) -> Vec<super::handle::TtpCacheTunnelSnapshot> {
        self.runtime.core().cache_snapshot_for_test(target)
    }

    pub(crate) fn accept_progress_for_test(&self) -> usize {
        self.runtime.core().accept_progress_for_test()
    }
}

#[async_trait::async_trait]
impl TtpPortListener for TtpServer {
    async fn listen_stream(
        &self,
        purpose: TunnelPurpose,
        callback: TtpIncomingStreamCallback,
    ) -> P2pResult<()> {
        self.runtime.core().listen_stream(purpose, callback)
    }

    async fn unlisten_stream(&self, purpose: &TunnelPurpose) -> P2pResult<()> {
        self.runtime.core().unlisten_stream(purpose);
        Ok(())
    }

    async fn listen_control_stream(
        &self,
        purpose: TunnelPurpose,
        callback: TtpIncomingControlStreamCallback,
    ) -> P2pResult<()> {
        self.runtime.core().listen_control_stream(purpose, callback)
    }

    async fn unlisten_control_stream(&self, purpose: &TunnelPurpose) -> P2pResult<()> {
        self.runtime.core().unlisten_control_stream(purpose);
        Ok(())
    }

    async fn listen_datagram(
        &self,
        purpose: TunnelPurpose,
        callback: TtpIncomingDatagramCallback,
    ) -> P2pResult<()> {
        self.runtime.core().listen_datagram(purpose, callback)
    }

    async fn unlisten_datagram(&self, purpose: &TunnelPurpose) -> P2pResult<()> {
        self.runtime.core().unlisten_datagram(purpose);
        Ok(())
    }
}

#[async_trait::async_trait]
impl TtpConnector for TtpServer {
    async fn open_stream(
        &self,
        target: &TtpTarget,
        purpose: TunnelPurpose,
    ) -> P2pResult<(TtpStreamMeta, TunnelStreamRead, TunnelStreamWrite)> {
        let tunnel = self.get_existing_tunnel(target)?;
        let (read, write) = tunnel.open_stream(purpose.clone()).await?;
        Ok((
            TtpStreamMeta {
                local_ep: tunnel.local_ep(),
                remote_ep: tunnel.remote_ep().or(Some(target.remote_ep)),
                local_id: tunnel.local_id(),
                remote_id: tunnel.remote_id(),
                remote_name: target.remote_name.clone(),
                purpose,
            },
            read,
            write,
        ))
    }

    async fn open_control_stream(
        &self,
        target: &TtpTarget,
        purpose: TunnelPurpose,
    ) -> P2pResult<(TtpStreamMeta, TunnelStreamRead, TunnelStreamWrite)> {
        let tunnel = self.get_existing_tunnel(target)?;
        let (read, write) = tunnel.open_control_stream(purpose.clone()).await?;
        Ok((
            TtpStreamMeta {
                local_ep: tunnel.local_ep(),
                remote_ep: tunnel.remote_ep().or(Some(target.remote_ep)),
                local_id: tunnel.local_id(),
                remote_id: tunnel.remote_id(),
                remote_name: target.remote_name.clone(),
                purpose,
            },
            read,
            write,
        ))
    }

    async fn open_datagram(
        &self,
        target: &TtpTarget,
        purpose: TunnelPurpose,
    ) -> P2pResult<(TtpDatagramMeta, TunnelDatagramWrite)> {
        let tunnel = self.get_existing_tunnel(target)?;
        let write = tunnel.open_datagram(purpose.clone()).await?;
        Ok((
            TtpDatagramMeta {
                local_ep: tunnel.local_ep(),
                remote_ep: tunnel.remote_ep().or(Some(target.remote_ep)),
                local_id: tunnel.local_id(),
                remote_id: tunnel.remote_id(),
                remote_name: target.remote_name.clone(),
                purpose,
            },
            write,
        ))
    }
}
