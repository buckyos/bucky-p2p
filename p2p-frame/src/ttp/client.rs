use crate::endpoint::Endpoint;
use crate::error::{P2pErrorCode, P2pResult, p2p_err};
use crate::networks::{
    NetManagerRef, TunnelDatagramWrite, TunnelPurpose, TunnelRef, TunnelState, TunnelStreamRead,
    TunnelStreamWrite,
};
use crate::p2p_identity::{P2pId, P2pIdentityRef};
use crate::runtime;
use std::collections::HashMap;
use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

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
    tunnels: Arc<Mutex<HashMap<P2pId, TtpClientTunnelEntries>>>,
    maintained_targets: Mutex<Vec<TtpTarget>>,
    maintain_started: AtomicBool,
    idle_started: AtomicBool,
    idle_timeout: Duration,
}

pub type TtpClientRef = Arc<TtpClient>;

const DEFAULT_TTP_CLIENT_IDLE_TIMEOUT: Duration = Duration::from_secs(5 * 60);

struct TtpClientTunnelEntry {
    tunnel: TunnelRef,
    maintained: bool,
    last_used: Instant,
    leases: usize,
}

type TtpClientTunnelEntries = HashMap<TtpClientTunnelKey, TtpClientTunnelEntry>;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct TtpClientTunnelKey {
    local_ep: Option<Endpoint>,
    remote_ep: Option<Endpoint>,
}

type TtpClientTunnelLease = Arc<TtpClientTunnelLeaseInner>;

struct TtpClientTunnelLeaseInner {
    tunnels: Arc<Mutex<HashMap<P2pId, TtpClientTunnelEntries>>>,
    remote_id: P2pId,
    key: TtpClientTunnelKey,
}

impl Drop for TtpClientTunnelLeaseInner {
    fn drop(&mut self) {
        let mut tunnels = self.tunnels.lock().unwrap();
        if let Some(entry) = tunnels
            .get_mut(&self.remote_id)
            .and_then(|entries| entries.get_mut(&self.key))
        {
            entry.leases = entry.leases.saturating_sub(1);
            entry.last_used = Instant::now();
        }
    }
}

struct LeasedRead {
    inner: TunnelStreamRead,
    _lease: TtpClientTunnelLease,
}

impl runtime::AsyncRead for LeasedRead {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut runtime::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

struct LeasedWrite {
    inner: TunnelStreamWrite,
    _lease: TtpClientTunnelLease,
}

impl runtime::AsyncWrite for LeasedWrite {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

impl TtpClient {
    pub fn new(local_identity: P2pIdentityRef, net_manager: NetManagerRef) -> TtpClientRef {
        Self::new_with_idle_timeout(local_identity, net_manager, DEFAULT_TTP_CLIENT_IDLE_TIMEOUT)
    }

    fn new_with_idle_timeout(
        local_identity: P2pIdentityRef,
        net_manager: NetManagerRef,
        idle_timeout: Duration,
    ) -> TtpClientRef {
        Arc::new(Self {
            local_identity,
            net_manager,
            runtime: TtpRuntime::new(),
            tunnels: Arc::new(Mutex::new(HashMap::new())),
            maintained_targets: Mutex::new(Vec::new()),
            maintain_started: AtomicBool::new(false),
            idle_started: AtomicBool::new(false),
            idle_timeout,
        })
    }

    fn find_existing_tunnel(&self, target: &TtpTarget) -> Option<TunnelRef> {
        let now = Instant::now();
        let maintained_targets = self.maintained_targets.lock().unwrap().clone();
        let mut tunnels = self.tunnels.lock().unwrap();
        self.release_idle_tunnels_locked(&mut tunnels, now, &maintained_targets);
        tunnels
            .get_mut(&target.remote_id)?
            .values_mut()
            .find(|entry| match_target(entry.tunnel.as_ref(), target))
            .map(|entry| {
                entry.last_used = now;
                entry.tunnel.clone()
            })
    }

    pub(crate) fn remember_tunnel(&self, tunnel: TunnelRef) {
        self.remember_tunnel_entry(tunnel);
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

    #[cfg(test)]
    pub(crate) fn new_with_idle_timeout_for_test(
        local_identity: P2pIdentityRef,
        net_manager: NetManagerRef,
        idle_timeout: Duration,
    ) -> TtpClientRef {
        Self::new_with_idle_timeout(local_identity, net_manager, idle_timeout)
    }

    #[cfg(test)]
    pub(crate) fn release_idle_tunnels_for_test(&self) {
        let maintained_targets = self.maintained_targets.lock().unwrap().clone();
        let mut tunnels = self.tunnels.lock().unwrap();
        self.release_idle_tunnels_locked(&mut tunnels, Instant::now(), &maintained_targets);
    }

    pub fn remove_server(&self, target: &TtpTarget) -> P2pResult<()> {
        {
            let mut targets = self.maintained_targets.lock().unwrap();
            targets.retain(|configured| {
                configured.remote_id != target.remote_id || configured.remote_ep != target.remote_ep
            });
        }
        self.refresh_maintained_cache_state(&target.remote_id);
        Ok(())
    }

    pub async fn connect_server(self: &TtpClientRef, target: TtpTarget) -> P2pResult<()> {
        self.get_or_create_tunnel(&target).await?;

        {
            let mut targets = self.maintained_targets.lock().unwrap();
            let already_exists = targets
                .iter()
                .any(|t| t.remote_ep == target.remote_ep && t.remote_id == target.remote_id);
            if !already_exists {
                targets.push(target.clone());
            }
        }
        self.refresh_maintained_cache_state(&target.remote_id);

        if !self.maintain_started.swap(true, Ordering::SeqCst) {
            self.start_maintain_loop();
        }
        self.start_idle_loop();

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
        self.remember_tunnel_entry(tunnel.clone());
        Ok(tunnel)
    }

    fn remember_tunnel_entry(&self, tunnel: TunnelRef) {
        let maintained_targets = self.maintained_targets.lock().unwrap().clone();
        let maintained = maintained_targets
            .iter()
            .any(|target| match_target(tunnel.as_ref(), target));
        let mut tunnels = self.tunnels.lock().unwrap();
        self.release_idle_tunnels_locked(&mut tunnels, Instant::now(), &maintained_targets);
        let key = TtpClientTunnelKey::from_tunnel(tunnel.as_ref());
        tunnels.entry(tunnel.remote_id()).or_default().insert(
            key,
            TtpClientTunnelEntry {
                tunnel,
                maintained,
                last_used: Instant::now(),
                leases: 0,
            },
        );
    }

    fn refresh_maintained_cache_state(&self, remote_id: &P2pId) {
        let targets = self.maintained_targets.lock().unwrap().clone();
        let mut tunnels = self.tunnels.lock().unwrap();
        if let Some(entries) = tunnels.get_mut(remote_id) {
            for entry in entries.values_mut() {
                entry.maintained = targets
                    .iter()
                    .any(|target| match_target(entry.tunnel.as_ref(), target));
                entry.last_used = Instant::now();
            }
        }
    }

    fn acquire_lease(&self, target: &TtpTarget) -> Option<TtpClientTunnelLease> {
        let mut tunnels = self.tunnels.lock().unwrap();
        let mut lease_key = None;
        let entries = tunnels.get_mut(&target.remote_id)?;
        for (key, entry) in entries.iter_mut() {
            if match_target(entry.tunnel.as_ref(), target) {
                entry.leases += 1;
                entry.last_used = Instant::now();
                lease_key = Some(key.clone());
                break;
            }
        }
        Some(Arc::new(TtpClientTunnelLeaseInner {
            tunnels: self.tunnels.clone(),
            remote_id: target.remote_id.clone(),
            key: lease_key?,
        }))
    }

    fn release_idle_tunnels_locked(
        &self,
        tunnels: &mut HashMap<P2pId, TtpClientTunnelEntries>,
        now: Instant,
        maintained_targets: &[TtpTarget],
    ) {
        let idle_timeout = self.idle_timeout;
        tunnels.retain(|_, entries| {
            entries.retain(|_, entry| {
                entry.maintained = maintained_targets
                    .iter()
                    .any(|target| match_target(entry.tunnel.as_ref(), target));
                is_tunnel_available(entry.tunnel.as_ref())
                    && (entry.maintained
                        || entry.leases > 0
                        || now.duration_since(entry.last_used) < idle_timeout)
            });
            !entries.is_empty()
        });
    }

    fn start_idle_loop(self: &TtpClientRef) {
        if self.idle_started.swap(true, Ordering::SeqCst) {
            return;
        }

        let client = Arc::downgrade(self);
        let interval = self.idle_timeout.min(Duration::from_secs(60));
        runtime::task::spawn(async move {
            loop {
                runtime::sleep(interval).await;

                let Some(client) = client.upgrade() else {
                    break;
                };

                let maintained_targets = client.maintained_targets.lock().unwrap().clone();
                let mut tunnels = client.tunnels.lock().unwrap();
                client.release_idle_tunnels_locked(
                    &mut tunnels,
                    Instant::now(),
                    &maintained_targets,
                );
            }
        });
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
        let lease = self.acquire_lease(target).ok_or_else(|| {
            p2p_err!(
                P2pErrorCode::ErrorState,
                "ttp client tunnel cache missing for lease"
            )
        })?;
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
            Box::pin(LeasedRead {
                inner: read,
                _lease: lease.clone(),
            }) as TunnelStreamRead,
            Box::pin(LeasedWrite {
                inner: write,
                _lease: lease,
            }) as TunnelStreamWrite,
        ))
    }

    async fn open_control_stream(
        &self,
        target: &TtpTarget,
        purpose: TunnelPurpose,
    ) -> P2pResult<(TtpStreamMeta, TunnelStreamRead, TunnelStreamWrite)> {
        let tunnel = self.get_or_create_tunnel(target).await?;
        let lease = self.acquire_lease(target).ok_or_else(|| {
            p2p_err!(
                P2pErrorCode::ErrorState,
                "ttp client tunnel cache missing for lease"
            )
        })?;
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
            Box::pin(LeasedRead {
                inner: read,
                _lease: lease.clone(),
            }) as TunnelStreamRead,
            Box::pin(LeasedWrite {
                inner: write,
                _lease: lease,
            }) as TunnelStreamWrite,
        ))
    }

    async fn open_datagram(
        &self,
        target: &TtpTarget,
        purpose: TunnelPurpose,
    ) -> P2pResult<(TtpDatagramMeta, TunnelDatagramWrite)> {
        let tunnel = self.get_or_create_tunnel(target).await?;
        let lease = self.acquire_lease(target).ok_or_else(|| {
            p2p_err!(
                P2pErrorCode::ErrorState,
                "ttp client tunnel cache missing for lease"
            )
        })?;
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
            Box::pin(LeasedWrite {
                inner: write,
                _lease: lease,
            }) as TunnelDatagramWrite,
        ))
    }
}

impl TtpClientTunnelKey {
    fn from_tunnel(tunnel: &dyn crate::networks::Tunnel) -> Self {
        Self {
            local_ep: tunnel.local_ep(),
            remote_ep: tunnel.remote_ep(),
        }
    }
}

pub(crate) fn is_tunnel_available(tunnel: &dyn crate::networks::Tunnel) -> bool {
    !tunnel.is_closed() && tunnel.state() == TunnelState::Connected
}

pub(crate) type TtpTunnelCache = HashMap<P2pId, TtpTunnelCacheEntries>;
pub(crate) type TtpTunnelCacheEntries = HashMap<TtpTunnelCacheKey, TunnelRef>;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct TtpTunnelCacheKey {
    local_ep: Option<Endpoint>,
    remote_ep: Option<Endpoint>,
}

impl TtpTunnelCacheKey {
    fn from_tunnel(tunnel: &dyn crate::networks::Tunnel) -> Self {
        Self {
            local_ep: tunnel.local_ep(),
            remote_ep: tunnel.remote_ep(),
        }
    }
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

pub(crate) fn remember_tunnel_in_multi(tunnels: &Mutex<TtpTunnelCache>, tunnel: TunnelRef) {
    let mut tunnels = tunnels.lock().unwrap();
    tunnels.retain(|_, entries| {
        entries.retain(|_, existing| is_tunnel_available(existing.as_ref()));
        !entries.is_empty()
    });
    let key = TtpTunnelCacheKey::from_tunnel(tunnel.as_ref());
    tunnels
        .entry(tunnel.remote_id())
        .or_default()
        .insert(key, tunnel);
}

pub(crate) fn find_existing_tunnel_in_multi(
    tunnels: &Mutex<TtpTunnelCache>,
    target: &TtpTarget,
) -> Option<TunnelRef> {
    let mut tunnels = tunnels.lock().unwrap();
    tunnels.retain(|_, entries| {
        entries.retain(|_, tunnel| is_tunnel_available(tunnel.as_ref()));
        !entries.is_empty()
    });
    tunnels
        .get_mut(&target.remote_id)?
        .values()
        .find(|tunnel| match_target(tunnel.as_ref(), target))
        .cloned()
}

pub(crate) async fn get_or_create_tunnel_for_multi(
    local_identity: &P2pIdentityRef,
    net_manager: &NetManagerRef,
    runtime: &Arc<TtpRuntime>,
    tunnels: &Mutex<TtpTunnelCache>,
    target: &TtpTarget,
) -> P2pResult<TunnelRef> {
    if let Some(tunnel) = find_existing_tunnel_in_multi(tunnels, target) {
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
    remember_tunnel_in_multi(tunnels, tunnel.clone());
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
