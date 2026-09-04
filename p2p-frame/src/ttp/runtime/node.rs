use std::sync::Arc;

use crate::error::P2pResult;
use crate::networks::{
    NetManagerRef, TunnelDatagramWrite, TunnelPurpose, TunnelStreamRead, TunnelStreamWrite,
};
use crate::p2p_identity::P2pIdentityRef;

use super::super::client::TtpConnector;
use super::super::listener::{
    TtpIncomingControlStreamCallback, TtpIncomingDatagramCallback, TtpIncomingStreamCallback,
    TtpPortListener,
};
use super::super::{TtpDatagramMeta, TtpStreamMeta, TtpTarget};
use super::handle::TtpRuntime;
use super::server::allow_all_ttp_incoming_tunnel_validator;

pub struct TtpNode {
    runtime: TtpRuntime,
}

pub type TtpNodeRef = Arc<TtpNode>;

impl TtpNode {
    pub fn new(
        local_identity: P2pIdentityRef,
        net_manager: NetManagerRef,
    ) -> P2pResult<TtpNodeRef> {
        let runtime = TtpRuntime::new(
            local_identity,
            net_manager,
            allow_all_ttp_incoming_tunnel_validator(),
        )?;
        Ok(Self::new_with_runtime(runtime))
    }

    pub fn new_with_runtime(runtime: TtpRuntime) -> TtpNodeRef {
        Arc::new(Self { runtime })
    }

    pub fn runtime(&self) -> TtpRuntime {
        self.runtime.clone()
    }
}

#[async_trait::async_trait]
impl TtpPortListener for TtpNode {
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
impl TtpConnector for TtpNode {
    async fn open_stream(
        &self,
        target: &TtpTarget,
        purpose: TunnelPurpose,
    ) -> P2pResult<(TtpStreamMeta, TunnelStreamRead, TunnelStreamWrite)> {
        let tunnel = self.runtime.core().get_or_create_tunnel(target).await?;
        let (read, write) = tunnel.open_stream(purpose.clone()).await?;
        Ok((
            TtpStreamMeta {
                local_ep: tunnel.local_ep().or(target.local_ep),
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
        let tunnel = self.runtime.core().get_or_create_tunnel(target).await?;
        let (read, write) = tunnel.open_control_stream(purpose.clone()).await?;
        Ok((
            TtpStreamMeta {
                local_ep: tunnel.local_ep().or(target.local_ep),
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
        let tunnel = self.runtime.core().get_or_create_tunnel(target).await?;
        let write = tunnel.open_datagram(purpose.clone()).await?;
        Ok((
            TtpDatagramMeta {
                local_ep: tunnel.local_ep().or(target.local_ep),
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
