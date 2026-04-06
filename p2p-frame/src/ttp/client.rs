use crate::error::{P2pErrorCode, P2pResult, p2p_err};
use crate::networks::{
    NetManagerRef, TunnelDatagramWrite, TunnelPurpose, TunnelRef, TunnelState, TunnelStreamRead,
    TunnelStreamWrite,
};
use crate::p2p_identity::{P2pId, P2pIdentityRef};
use crate::runtime;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::listener::{TtpDatagramListenerRef, TtpListenerRef, TtpPortListener};
use super::runtime::TtpRuntime;
use super::{TtpDatagramMeta, TtpStreamMeta, TtpTarget};

#[async_trait::async_trait]
pub trait TtpConnector: Send + Sync + 'static {
    async fn open_stream(
        &self,
        target: &TtpTarget,
        purpose: TunnelPurpose,
    ) -> P2pResult<(TtpStreamMeta, TunnelStreamRead, TunnelStreamWrite)>;
    async fn open_datagram(
        &self,
        target: &TtpTarget,
        purpose: TunnelPurpose,
    ) -> P2pResult<(TtpDatagramMeta, TunnelDatagramWrite)>;
}

pub struct TtpClient {
    local_identity: P2pIdentityRef,
    net_manager: NetManagerRef,
    runtime: Arc<TtpRuntime>,
    tunnels: Mutex<Vec<TunnelRef>>,
    maintained_targets: Mutex<Vec<TtpTarget>>,
    maintain_started: AtomicBool,
}

pub type TtpClientRef = Arc<TtpClient>;

impl TtpClient {
    pub fn new(local_identity: P2pIdentityRef, net_manager: NetManagerRef) -> TtpClientRef {
        Arc::new(Self {
            local_identity,
            net_manager,
            runtime: TtpRuntime::new(),
            tunnels: Mutex::new(Vec::new()),
            maintained_targets: Mutex::new(Vec::new()),
            maintain_started: AtomicBool::new(false),
        })
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

    fn remember_tunnel(&self, tunnel: TunnelRef) {
        let mut tunnels = self.tunnels.lock().unwrap();
        tunnels.retain(|existing| {
            is_tunnel_available(existing.as_ref()) && !Arc::ptr_eq(existing, &tunnel)
        });
        tunnels.push(tunnel);
    }

    fn latest_available_tunnel(&self) -> Option<TunnelRef> {
        let mut tunnels = self.tunnels.lock().unwrap();
        tunnels.retain(|tunnel| is_tunnel_available(tunnel.as_ref()));
        tunnels.last().cloned()
    }

    pub fn local_id(&self) -> P2pId {
        self.local_identity.get_id()
    }

    pub async fn open_stream_on_latest_tunnel(
        &self,
        purpose: TunnelPurpose,
    ) -> P2pResult<(TtpStreamMeta, TunnelStreamRead, TunnelStreamWrite)> {
        let tunnel = self
            .latest_available_tunnel()
            .ok_or_else(|| p2p_err!(P2pErrorCode::NotFound, "no cached ttp tunnel available"))?;
        let (read, write) = tunnel.open_stream(purpose.clone()).await?;
        Ok((
            TtpStreamMeta {
                local_ep: tunnel.local_ep(),
                remote_ep: tunnel.remote_ep(),
                local_id: tunnel.local_id(),
                remote_id: tunnel.remote_id(),
                remote_name: None,
                purpose,
            },
            read,
            write,
        ))
    }

    pub async fn connect_server(self: &TtpClientRef, target: TtpTarget) -> P2pResult<()> {
        self.get_or_create_tunnel(&target).await?;

        {
            let mut targets = self.maintained_targets.lock().unwrap();
            let already_exists = targets
                .iter()
                .any(|t| t.remote_ep == target.remote_ep && t.remote_id == target.remote_id);
            if !already_exists {
                targets.push(target);
            }
        }

        if !self.maintain_started.swap(true, Ordering::SeqCst) {
            self.start_maintain_loop();
        }

        Ok(())
    }

    fn start_maintain_loop(self: &TtpClientRef) {
        let client = Arc::downgrade(self);
        runtime::task::spawn(async move {
            loop {
                runtime::sleep(Duration::from_secs(60)).await;

                let Some(client) = client.upgrade() else {
                    break;
                };

                let targets = client.maintained_targets.lock().unwrap().clone();
                for target in &targets {
                    if let Err(e) = client.get_or_create_tunnel(target).await {
                        log::warn!("maintain tunnel to {:?} failed: {}", target.remote_ep, e);
                    }
                }
            }
        });
    }

    async fn get_or_create_tunnel(&self, target: &TtpTarget) -> P2pResult<TunnelRef> {
        if let Some(tunnel) = self.find_existing_tunnel(target) {
            return Ok(tunnel);
        }

        let network = self.net_manager.get_network(target.remote_ep.protocol())?;
        let tunnel = if let Some(local_ep) = target.local_ep.as_ref() {
            network
                .create_tunnel_with_local_ep(
                    &self.local_identity,
                    local_ep,
                    &target.remote_ep,
                    &target.remote_id,
                    target.remote_name.clone(),
                )
                .await?
        } else {
            network
                .create_tunnel(
                    &self.local_identity,
                    &target.remote_ep,
                    &target.remote_id,
                    target.remote_name.clone(),
                )
                .await?
        };

        self.runtime.attach_tunnel(tunnel.clone()).await?;
        self.remember_tunnel(tunnel.clone());
        Ok(tunnel)
    }
}

#[async_trait::async_trait]
impl TtpPortListener for TtpClient {
    async fn listen_stream(&self, purpose: TunnelPurpose) -> P2pResult<TtpListenerRef> {
        self.runtime.listen_stream(purpose)
    }

    async fn unlisten_stream(&self, purpose: &TunnelPurpose) -> P2pResult<()> {
        self.runtime.unlisten_stream(purpose);
        Ok(())
    }

    async fn listen_datagram(&self, purpose: TunnelPurpose) -> P2pResult<TtpDatagramListenerRef> {
        self.runtime.listen_datagram(purpose)
    }

    async fn unlisten_datagram(&self, purpose: &TunnelPurpose) -> P2pResult<()> {
        self.runtime.unlisten_datagram(purpose);
        Ok(())
    }
}

#[async_trait::async_trait]
impl TtpConnector for TtpClient {
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

pub(crate) fn is_tunnel_available(tunnel: &dyn crate::networks::Tunnel) -> bool {
    !tunnel.is_closed() && tunnel.state() == TunnelState::Connected
}

pub(crate) fn match_target(tunnel: &dyn crate::networks::Tunnel, target: &TtpTarget) -> bool {
    if tunnel.remote_id() != target.remote_id {
        return false;
    }

    if let Some(local_ep) = target.local_ep {
        if tunnel.local_ep().map(|ep| ep != local_ep).unwrap_or(false) {
            return false;
        }
    }

    tunnel
        .remote_ep()
        .map(|ep| ep == target.remote_ep)
        .unwrap_or(true)
}
