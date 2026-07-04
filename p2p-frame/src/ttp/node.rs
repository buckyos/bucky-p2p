use crate::error::P2pResult;
use crate::networks::{
    NetManagerRef, TunnelDatagramWrite, TunnelPurpose, TunnelRef, TunnelStreamRead,
    TunnelStreamWrite,
};
use crate::p2p_identity::P2pIdentityRef;
use std::sync::{Arc, Mutex};

use super::client::{
    TtpConnector, TtpTunnelCache, get_or_create_tunnel_for_multi, remember_tunnel_in_multi,
};
use super::listener::{
    TtpIncomingControlStreamCallback, TtpIncomingDatagramCallback, TtpIncomingStreamCallback,
    TtpPortListener,
};
use super::runtime::TtpRuntime;
use super::{TtpDatagramMeta, TtpStreamMeta, TtpTarget};

pub struct TtpNode {
    local_identity: P2pIdentityRef,
    net_manager: NetManagerRef,
    runtime: Arc<TtpRuntime>,
    tunnels: Mutex<TtpTunnelCache>,
}

pub type TtpNodeRef = Arc<TtpNode>;

impl TtpNode {
    pub fn new(
        local_identity: P2pIdentityRef,
        net_manager: NetManagerRef,
    ) -> P2pResult<TtpNodeRef> {
        let node = Arc::new(Self {
            local_identity,
            net_manager,
            runtime: TtpRuntime::new(),
            tunnels: Mutex::new(TtpTunnelCache::new()),
        });
        node.register_accept_callback()?;
        Ok(node)
    }

    fn register_accept_callback(self: &Arc<Self>) -> P2pResult<()> {
        let weak = Arc::downgrade(self);
        self.net_manager.register_incoming_tunnel_subscriber(
            self.local_identity.get_id(),
            Arc::new(move |result| {
                let Some(node) = weak.upgrade() else {
                    return Box::pin(async { false });
                };
                let tunnel = match result {
                    Ok(tunnel) => tunnel,
                    Err(err) => {
                        log::debug!("ttp node accept tunnel failed: {:?}", err);
                        return Box::pin(async { true });
                    }
                };

                Box::pin(async move {
                    log::debug!(
                        "ttp node accepted incoming tunnel local={} remote={} protocol={:?} tunnel_id={:?} candidate_id={:?}",
                        tunnel.local_id(),
                        tunnel.remote_id(),
                        tunnel.protocol(),
                        tunnel.tunnel_id(),
                        tunnel.candidate_id()
                    );
                    if let Err(err) = node.runtime.attach_tunnel(tunnel.clone()).await {
                        log::warn!(
                            "ttp node attach incoming tunnel failed local={} remote={} protocol={:?} tunnel_id={:?} candidate_id={:?} err={:?}",
                            tunnel.local_id(),
                            tunnel.remote_id(),
                            tunnel.protocol(),
                            tunnel.tunnel_id(),
                            tunnel.candidate_id(),
                            err
                        );
                        return true;
                    }
                    log::debug!(
                        "ttp node attach incoming tunnel success local={} remote={} protocol={:?} tunnel_id={:?} candidate_id={:?}",
                        tunnel.local_id(),
                        tunnel.remote_id(),
                        tunnel.protocol(),
                        tunnel.tunnel_id(),
                        tunnel.candidate_id()
                    );
                    node.remember_tunnel(tunnel);
                    true
                })
            }),
        )
    }

    fn remember_tunnel(&self, tunnel: TunnelRef) {
        remember_tunnel_in_multi(&self.tunnels, tunnel);
    }

    async fn get_or_create_tunnel(&self, target: &TtpTarget) -> P2pResult<TunnelRef> {
        get_or_create_tunnel_for_multi(
            &self.local_identity,
            &self.net_manager,
            &self.runtime,
            &self.tunnels,
            target,
        )
        .await
    }
}

#[async_trait::async_trait]
impl TtpPortListener for TtpNode {
    async fn listen_stream(
        &self,
        purpose: TunnelPurpose,
        callback: TtpIncomingStreamCallback,
    ) -> P2pResult<()> {
        self.runtime.listen_stream(purpose, callback)
    }

    async fn unlisten_stream(&self, purpose: &TunnelPurpose) -> P2pResult<()> {
        self.runtime.unlisten_stream(purpose);
        Ok(())
    }

    async fn listen_control_stream(
        &self,
        purpose: TunnelPurpose,
        callback: TtpIncomingControlStreamCallback,
    ) -> P2pResult<()> {
        self.runtime.listen_control_stream(purpose, callback)
    }

    async fn unlisten_control_stream(&self, purpose: &TunnelPurpose) -> P2pResult<()> {
        self.runtime.unlisten_control_stream(purpose);
        Ok(())
    }

    async fn listen_datagram(
        &self,
        purpose: TunnelPurpose,
        callback: TtpIncomingDatagramCallback,
    ) -> P2pResult<()> {
        self.runtime.listen_datagram(purpose, callback)
    }

    async fn unlisten_datagram(&self, purpose: &TunnelPurpose) -> P2pResult<()> {
        self.runtime.unlisten_datagram(purpose);
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
        let tunnel = self.get_or_create_tunnel(target).await?;
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
        let tunnel = self.get_or_create_tunnel(target).await?;
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
        let tunnel = self.get_or_create_tunnel(target).await?;
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

impl Drop for TtpNode {
    fn drop(&mut self) {
        self.net_manager
            .unregister_incoming_tunnel_subscriber(&self.local_identity.get_id());
    }
}
