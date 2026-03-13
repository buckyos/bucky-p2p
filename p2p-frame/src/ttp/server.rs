use crate::error::{P2pErrorCode, P2pResult, p2p_err};
use crate::networks::{
    NetManagerRef, TunnelAcceptor, TunnelDatagramWrite, TunnelRef, TunnelStreamRead,
    TunnelStreamWrite,
};
use crate::p2p_identity::{P2pId, P2pIdentityRef};
use crate::runtime;
use std::sync::{Arc, Mutex};
use tokio::task::JoinHandle;

use super::client::{TtpConnector, is_tunnel_available, match_target};
use super::listener::{TtpDatagramListenerRef, TtpListenerRef, TtpPortListener};
use super::runtime::TtpRuntime;
use super::{TtpDatagramMeta, TtpStreamMeta, TtpTarget};

pub struct TtpServer {
    local_identity: P2pIdentityRef,
    net_manager: NetManagerRef,
    runtime: Arc<TtpRuntime>,
    tunnels: Mutex<Vec<TunnelRef>>,
    accept_task: Mutex<Option<JoinHandle<()>>>,
}

pub type TtpServerRef = Arc<TtpServer>;

impl TtpServer {
    pub fn new(
        local_identity: P2pIdentityRef,
        net_manager: NetManagerRef,
    ) -> P2pResult<TtpServerRef> {
        let acceptor = net_manager.register_tunnel_acceptor(local_identity.get_id())?;
        let server = Arc::new(Self {
            local_identity,
            net_manager,
            runtime: TtpRuntime::new(),
            tunnels: Mutex::new(Vec::new()),
            accept_task: Mutex::new(None),
        });
        server.start_accept_loop(acceptor);
        Ok(server)
    }

    fn start_accept_loop(self: &Arc<Self>, mut acceptor: TunnelAcceptor) {
        let weak = Arc::downgrade(self);
        let handle = runtime::task::spawn(async move {
            loop {
                let tunnel = match acceptor.accept_tunnel().await {
                    Ok(tunnel) => tunnel,
                    Err(err) => {
                        log::debug!("ttp server accept tunnel finished: {:?}", err);
                        break;
                    }
                };

                let Some(server) = weak.upgrade() else {
                    break;
                };

                if let Err(err) = server.runtime.attach_tunnel(tunnel.clone()).await {
                    log::warn!("ttp attach incoming tunnel failed: {:?}", err);
                    continue;
                }
                server.remember_tunnel(tunnel);
            }
        });
        *self.accept_task.lock().unwrap() = Some(handle);
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
        vport: u16,
    ) -> P2pResult<(TtpStreamMeta, TunnelStreamRead, TunnelStreamWrite)> {
        let tunnel = self.get_existing_tunnel_by_id(remote_id)?;
        let (read, write) = tunnel.open_stream(vport).await?;
        Ok((
            TtpStreamMeta {
                local_ep: tunnel.local_ep(),
                remote_ep: tunnel.remote_ep(),
                local_id: tunnel.local_id(),
                remote_id: tunnel.remote_id(),
                remote_name,
                vport,
            },
            read,
            write,
        ))
    }
}

#[async_trait::async_trait]
impl TtpPortListener for TtpServer {
    async fn listen_stream(&self, vport: u16) -> P2pResult<TtpListenerRef> {
        self.runtime.listen_stream(vport)
    }

    async fn unlisten_stream(&self, vport: u16) -> P2pResult<()> {
        self.runtime.unlisten_stream(vport);
        Ok(())
    }

    async fn listen_datagram(&self, vport: u16) -> P2pResult<TtpDatagramListenerRef> {
        self.runtime.listen_datagram(vport)
    }

    async fn unlisten_datagram(&self, vport: u16) -> P2pResult<()> {
        self.runtime.unlisten_datagram(vport);
        Ok(())
    }
}

#[async_trait::async_trait]
impl TtpConnector for TtpServer {
    async fn open_stream(
        &self,
        target: &TtpTarget,
        vport: u16,
    ) -> P2pResult<(TtpStreamMeta, TunnelStreamRead, TunnelStreamWrite)> {
        let tunnel = self.get_existing_tunnel(target)?;
        let (read, write) = tunnel.open_stream(vport).await?;
        Ok((
            TtpStreamMeta {
                local_ep: tunnel.local_ep(),
                remote_ep: tunnel.remote_ep().or(Some(target.remote_ep)),
                local_id: tunnel.local_id(),
                remote_id: tunnel.remote_id(),
                remote_name: target.remote_name.clone(),
                vport,
            },
            read,
            write,
        ))
    }

    async fn open_datagram(
        &self,
        target: &TtpTarget,
        vport: u16,
    ) -> P2pResult<(TtpDatagramMeta, TunnelDatagramWrite)> {
        let tunnel = self.get_existing_tunnel(target)?;
        let write = tunnel.open_datagram(vport).await?;
        Ok((
            TtpDatagramMeta {
                local_ep: tunnel.local_ep(),
                remote_ep: tunnel.remote_ep().or(Some(target.remote_ep)),
                local_id: tunnel.local_id(),
                remote_id: tunnel.remote_id(),
                remote_name: target.remote_name.clone(),
                vport,
            },
            write,
        ))
    }
}

impl Drop for TtpServer {
    fn drop(&mut self) {
        self.net_manager
            .unregister_tunnel_acceptor(&self.local_identity.get_id());
        if let Some(task) = self.accept_task.lock().unwrap().take() {
            task.abort();
        }
    }
}
