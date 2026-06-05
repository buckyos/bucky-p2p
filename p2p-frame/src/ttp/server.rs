use crate::error::{P2pErrorCode, P2pResult, p2p_err};
use crate::networks::{
    NetManagerRef, TunnelDatagramWrite, TunnelPurpose, TunnelRef, TunnelStreamRead,
    TunnelStreamWrite,
};
use crate::p2p_identity::{P2pId, P2pIdentityRef};
use std::sync::{Arc, Mutex};

use super::client::{TtpConnector, is_tunnel_available, match_target};
use super::listener::{
    TtpIncomingControlStreamCallback, TtpIncomingDatagramCallback, TtpIncomingStreamCallback,
    TtpPortListener,
};
use super::runtime::TtpRuntime;
use super::{TtpDatagramMeta, TtpStreamMeta, TtpTarget};

pub struct TtpServer {
    local_identity: P2pIdentityRef,
    net_manager: NetManagerRef,
    runtime: Arc<TtpRuntime>,
    tunnels: Mutex<Vec<TunnelRef>>,
}

pub type TtpServerRef = Arc<TtpServer>;

impl TtpServer {
    pub fn new(
        local_identity: P2pIdentityRef,
        net_manager: NetManagerRef,
    ) -> P2pResult<TtpServerRef> {
        let server = Arc::new(Self {
            local_identity,
            net_manager,
            runtime: TtpRuntime::new(),
            tunnels: Mutex::new(Vec::new()),
        });
        server.register_accept_callback()?;
        Ok(server)
    }

    fn register_accept_callback(self: &Arc<Self>) -> P2pResult<()> {
        let weak = Arc::downgrade(self);
        self.net_manager.register_incoming_tunnel_subscriber(
            self.local_identity.get_id(),
            Arc::new(move |result| {
                let Some(server) = weak.upgrade() else {
                    return Box::pin(async { false });
                };
                let tunnel = match result {
                    Ok(tunnel) => tunnel,
                    Err(err) => {
                        log::debug!("ttp server accept tunnel failed: {:?}", err);
                        return Box::pin(async { true });
                    }
                };

                Box::pin(async move {
                    log::debug!(
                        "ttp server accepted incoming tunnel local={} remote={} protocol={:?} tunnel_id={:?} candidate_id={:?}",
                        tunnel.local_id(),
                        tunnel.remote_id(),
                        tunnel.protocol(),
                        tunnel.tunnel_id(),
                        tunnel.candidate_id()
                    );
                    if let Err(err) = server.runtime.attach_tunnel(tunnel.clone()).await {
                        log::warn!(
                            "ttp attach incoming tunnel failed local={} remote={} protocol={:?} tunnel_id={:?} candidate_id={:?} err={:?}",
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
                        "ttp attach incoming tunnel success local={} remote={} protocol={:?} tunnel_id={:?} candidate_id={:?}",
                        tunnel.local_id(),
                        tunnel.remote_id(),
                        tunnel.protocol(),
                        tunnel.tunnel_id(),
                        tunnel.candidate_id()
                    );
                    server.remember_tunnel(tunnel);
                    true
                })
            }),
        )
    }

    fn remember_tunnel(&self, tunnel: TunnelRef) {
        let mut tunnels = self.tunnels.lock().unwrap();
        tunnels.retain(|existing| {
            is_tunnel_available(existing.as_ref()) && !Arc::ptr_eq(existing, &tunnel)
        });
        tunnels.push(tunnel);
    }

    fn find_existing_tunnel(&self, target: &TtpTarget) -> Option<TunnelRef> {
        let mut tunnels = self.tunnels.lock().unwrap();
        tunnels.retain(|tunnel| is_tunnel_available(tunnel.as_ref()));
        tunnels
            .iter()
            .rev()
            .find(|tunnel| match_target(tunnel.as_ref(), target))
            .cloned()
    }

    fn get_existing_tunnel(&self, target: &TtpTarget) -> P2pResult<TunnelRef> {
        self.find_existing_tunnel(target).ok_or_else(|| {
            p2p_err!(
                P2pErrorCode::NotFound,
                "ttp server has no incoming tunnel for {} {}",
                target.remote_id,
                target.remote_ep
            )
        })
    }

    fn get_existing_tunnel_by_id(&self, remote_id: &P2pId) -> P2pResult<TunnelRef> {
        let mut tunnels = self.tunnels.lock().unwrap();
        tunnels.retain(|tunnel| is_tunnel_available(tunnel.as_ref()));
        tunnels
            .iter()
            .rev()
            .find(|tunnel| tunnel.remote_id() == *remote_id)
            .cloned()
            .ok_or_else(|| {
                p2p_err!(
                    P2pErrorCode::NotFound,
                    "ttp server has no incoming tunnel for {}",
                    remote_id
                )
            })
    }

    pub async fn open_stream_by_id(
        &self,
        remote_id: &P2pId,
        remote_name: Option<String>,
        purpose: TunnelPurpose,
    ) -> P2pResult<(TtpStreamMeta, TunnelStreamRead, TunnelStreamWrite)> {
        let tunnel = self.get_existing_tunnel_by_id(remote_id)?;
        let (read, write) = tunnel.open_stream(purpose.clone()).await?;
        Ok((
            TtpStreamMeta {
                local_ep: tunnel.local_ep(),
                remote_ep: tunnel.remote_ep(),
                local_id: tunnel.local_id(),
                remote_id: tunnel.remote_id(),
                remote_name,
                purpose,
            },
            read,
            write,
        ))
    }

    pub async fn open_control_stream_by_id(
        &self,
        remote_id: &P2pId,
        remote_name: Option<String>,
        purpose: TunnelPurpose,
    ) -> P2pResult<(TtpStreamMeta, TunnelStreamRead, TunnelStreamWrite)> {
        let tunnel = self.get_existing_tunnel_by_id(remote_id)?;
        let (read, write) = tunnel.open_control_stream(purpose.clone()).await?;
        Ok((
            TtpStreamMeta {
                local_ep: tunnel.local_ep(),
                remote_ep: tunnel.remote_ep(),
                local_id: tunnel.local_id(),
                remote_id: tunnel.remote_id(),
                remote_name,
                purpose,
            },
            read,
            write,
        ))
    }
}

#[async_trait::async_trait]
impl TtpPortListener for TtpServer {
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

impl Drop for TtpServer {
    fn drop(&mut self) {
        self.net_manager
            .unregister_incoming_tunnel_subscriber(&self.local_identity.get_id());
    }
}
