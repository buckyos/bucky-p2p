use crate::endpoint::Endpoint;
use crate::error::{P2pErrorCode, P2pResult, p2p_err};
use crate::networks::{
    NetManagerRef, TunnelDatagramWrite, TunnelPurpose, TunnelRef, TunnelState, TunnelStreamRead,
    TunnelStreamWrite,
};
use crate::p2p_identity::{P2pId, P2pIdentityRef};
use crate::runtime;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::listener::{
    TtpIncomingControlStreamCallback, TtpIncomingDatagramCallback, TtpIncomingStreamCallback,
    TtpPortListener,
};
use super::runtime::TtpRuntime;
use super::{TtpDatagramMeta, TtpStreamMeta, TtpTarget};

#[async_trait::async_trait]
pub trait TtpConnector: Send + Sync + 'static {
    async fn open_stream(
        &self,
        target: &TtpTarget,
        purpose: TunnelPurpose,
    ) -> P2pResult<(TtpStreamMeta, TunnelStreamRead, TunnelStreamWrite)>;
    async fn open_control_stream(
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
    tunnels: Mutex<HashMap<P2pId, TunnelRef>>,
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
            tunnels: Mutex::new(HashMap::new()),
            maintained_targets: Mutex::new(Vec::new()),
            maintain_started: AtomicBool::new(false),
        })
    }

    fn find_existing_tunnel(&self, target: &TtpTarget) -> Option<TunnelRef> {
        find_existing_tunnel_in(&self.tunnels, target)
    }

    pub(crate) fn remember_tunnel(&self, tunnel: TunnelRef) {
        remember_tunnel_in(&self.tunnels, tunnel);
    }

    pub fn local_id(&self) -> P2pId {
        self.local_identity.get_id()
    }

    pub(crate) fn configured_server_id(&self) -> P2pResult<Option<P2pId>> {
        let targets = self.maintained_targets.lock().unwrap();
        let Some(first) = targets.first() else {
            return Ok(None);
        };
        if targets
            .iter()
            .any(|target| target.remote_id != first.remote_id)
        {
            return Err(p2p_err!(
                P2pErrorCode::InvalidParam,
                "multiple ttp server targets configured"
            ));
        }
        Ok(Some(first.remote_id.clone()))
    }

    #[cfg(test)]
    pub(crate) fn remember_server_target_for_test(&self, target: TtpTarget) {
        self.maintained_targets.lock().unwrap().push(target);
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
        get_or_create_tunnel_for(
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
impl TtpPortListener for TtpClient {
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

pub(crate) fn is_tunnel_available(tunnel: &dyn crate::networks::Tunnel) -> bool {
    !tunnel.is_closed() && tunnel.state() == TunnelState::Connected
}

pub(crate) fn remember_tunnel_in(tunnels: &Mutex<HashMap<P2pId, TunnelRef>>, tunnel: TunnelRef) {
    let mut tunnels = tunnels.lock().unwrap();
    tunnels.retain(|_, existing| is_tunnel_available(existing.as_ref()));
    tunnels.insert(tunnel.remote_id(), tunnel);
}

pub(crate) fn find_existing_tunnel_in(
    tunnels: &Mutex<HashMap<P2pId, TunnelRef>>,
    target: &TtpTarget,
) -> Option<TunnelRef> {
    let mut tunnels = tunnels.lock().unwrap();
    tunnels.retain(|_, tunnel| is_tunnel_available(tunnel.as_ref()));
    tunnels
        .get(&target.remote_id)
        .filter(|tunnel| match_target(tunnel.as_ref(), target))
        .cloned()
}

pub(crate) async fn get_or_create_tunnel_for(
    local_identity: &P2pIdentityRef,
    net_manager: &NetManagerRef,
    runtime: &Arc<TtpRuntime>,
    tunnels: &Mutex<HashMap<P2pId, TunnelRef>>,
    target: &TtpTarget,
) -> P2pResult<TunnelRef> {
    if let Some(tunnel) = find_existing_tunnel_in(tunnels, target) {
        return Ok(tunnel);
    }

    let network = net_manager.get_network(target.remote_ep.protocol())?;
    let tunnel = if let Some(local_ep) = target.local_ep.as_ref() {
        network
            .create_tunnel_with_local_ep(
                local_identity,
                local_ep,
                &target.remote_ep,
                &target.remote_id,
                target.remote_name.clone(),
            )
            .await?
    } else {
        network
            .create_tunnel(
                local_identity,
                &target.remote_ep,
                &target.remote_id,
                target.remote_name.clone(),
            )
            .await?
    };

    runtime.attach_tunnel(tunnel.clone()).await?;
    remember_tunnel_in(tunnels, tunnel.clone());
    Ok(tunnel)
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

    target.remote_ep == Endpoint::default()
        || tunnel
            .remote_ep()
            .map(|ep| ep == target.remote_ep)
            .unwrap_or(true)
}
