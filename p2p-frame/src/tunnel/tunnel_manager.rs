use crate::endpoint::{Endpoint, EndpointArea, Protocol, is_non_lan_ipv4_addr};
use crate::error::{P2pErrorCode, P2pResult, into_p2p_err, p2p_err};
use crate::executor::{Executor, SpawnHandle};
use crate::p2p_identity::{P2pId, P2pIdentityCertFactoryRef, P2pIdentityCertRef, P2pIdentityRef};
use crate::pn::pn_virtual_endpoint;
use crate::runtime;
use crate::sn::client::SNClientServiceRef;
use crate::sn::protocol::v0::{SnCalled, TunnelType};
use crate::types::{TunnelCandidateId, TunnelId, TunnelIdGenerator};
use async_named_locker::Locker;
use bucky_time::bucky_time_now;
use notify_future::Notify;
use std::collections::HashMap;
use std::future::Future;
use std::net::SocketAddr;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::Notify as AsyncNotify;

use futures::stream::{FuturesUnordered, StreamExt};

use super::{ConnectDirection, DeviceFinderRef, P2pConnectionInfo, P2pConnectionInfoCacheRef};
use crate::networks::{
    IncomingTunnelCallback, ListenVPortsRef, NetManagerRef, TunnelCommandResult,
    TunnelConnectIntent, TunnelDatagramRead, TunnelDatagramWrite, TunnelForm, TunnelListenerInfo,
    TunnelNetworkRef, TunnelRef, TunnelState, TunnelStreamRead, TunnelStreamWrite,
};

const HEDGED_REVERSE_DELAY: Duration = Duration::from_millis(300);
const NAT_HEDGED_REVERSE_DELAY: Duration = HEDGED_REVERSE_DELAY;
const HOUSEKEEPING_INTERVAL: Duration = Duration::from_secs(60);
const PROXY_UPGRADE_INITIAL_INTERVAL: Duration = Duration::from_secs(300);
const PROXY_UPGRADE_MAX_INTERVAL: Duration = Duration::from_secs(2 * 60 * 60);
const PROXY_UPGRADE_SHORT_INTERVALS: [Duration; 4] = [
    Duration::from_secs(15),
    Duration::from_secs(30),
    Duration::from_secs(60),
    Duration::from_secs(120),
];

async fn race_with_delay<T, FD, FR>(
    direct_future: FD,
    reverse_delay: Duration,
    reverse_future: FR,
) -> (P2pResult<T>, bool)
where
    T: Send,
    FD: Future<Output = P2pResult<T>> + Send,
    FR: Future<Output = P2pResult<T>> + Send,
{
    let delayed_reverse = async move {
        runtime::sleep(reverse_delay).await;
        reverse_future.await
    };
    tokio::pin!(direct_future);
    tokio::pin!(delayed_reverse);

    tokio::select! {
        direct_result = &mut direct_future => {
            match direct_result {
                Ok(value) => (Ok(value), true),
                Err(err) => match delayed_reverse.await {
                    Ok(value) => (Ok(value), false),
                    Err(_) => (Err(err), false),
                }
            }
        }
        reverse_result = &mut delayed_reverse => {
            match reverse_result {
                Ok(value) => (Ok(value), false),
                Err(err) => match direct_future.await {
                    Ok(value) => (Ok(value), true),
                    Err(_) => (Err(err), true),
                }
            }
        }
    }
}

struct TunnelEntry {
    tunnel: TunnelRef,
    updated_at: Instant,
    published: bool,
}

type TunnelEntries = Vec<TunnelEntry>;

#[derive(Clone)]
struct EndpointScore {
    last_success_at: u64,
    fail_count: u32,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct EndpointScoreKey {
    protocol: crate::endpoint::Protocol,
    addr: SocketAddr,
}

impl EndpointScoreKey {
    fn new(ep: &Endpoint) -> Self {
        Self {
            protocol: ep.protocol(),
            addr: *ep.addr(),
        }
    }
}

type ReverseWaitKey = (P2pId, TunnelId);

struct ReverseWaitRegistration<'a> {
    manager: &'a TunnelManager,
    remote_id: P2pId,
    tunnel_id: TunnelId,
    active: bool,
}

impl<'a> ReverseWaitRegistration<'a> {
    fn register(
        manager: &'a TunnelManager,
        remote_id: P2pId,
        tunnel_id: TunnelId,
        waiter: Notify<P2pResult<TunnelRef>>,
    ) -> Self {
        manager.add_reverse_waiter(remote_id.clone(), tunnel_id, waiter);
        Self {
            manager,
            remote_id,
            tunnel_id,
            active: true,
        }
    }

    fn dismiss(&mut self) {
        self.active = false;
    }
}

impl Drop for ReverseWaitRegistration<'_> {
    fn drop(&mut self) {
        if self.active {
            self.manager
                .take_reverse_waiter(&self.remote_id, &self.tunnel_id);
        }
    }
}

#[derive(Clone, Copy)]
struct ProxyUpgradeState {
    next_attempt_at: Instant,
    retry_interval: Duration,
    short_retry_index: usize,
    in_progress: bool,
}

impl ProxyUpgradeState {
    fn new(now: Instant, _initial_interval: Duration) -> Self {
        let retry_interval = PROXY_UPGRADE_SHORT_INTERVALS[0];
        Self {
            next_attempt_at: now + retry_interval,
            retry_interval,
            short_retry_index: 0,
            in_progress: false,
        }
    }
}

fn advance_proxy_upgrade_retry(
    upgrade: &mut ProxyUpgradeState,
    initial_interval: Duration,
) -> Duration {
    if upgrade.short_retry_index + 1 < PROXY_UPGRADE_SHORT_INTERVALS.len() {
        upgrade.short_retry_index += 1;
        upgrade.retry_interval = PROXY_UPGRADE_SHORT_INTERVALS[upgrade.short_retry_index];
    } else if upgrade.short_retry_index + 1 == PROXY_UPGRADE_SHORT_INTERVALS.len() {
        upgrade.short_retry_index += 1;
        upgrade.retry_interval = initial_interval;
    } else {
        upgrade.retry_interval = upgrade
            .retry_interval
            .saturating_mul(2)
            .min(PROXY_UPGRADE_MAX_INTERVAL);
    }
    upgrade.retry_interval
}

struct ManagerState {
    endpoint_scores: HashMap<EndpointScoreKey, EndpointScore>,
    pending_reverse_waiters: HashMap<ReverseWaitKey, Notify<P2pResult<TunnelRef>>>,
    proxy_upgrade_states: HashMap<P2pId, ProxyUpgradeState>,
}

pub struct TunnelManager {
    self_weak: std::sync::Weak<TunnelManager>,
    local_identity: P2pIdentityRef,
    device_finder: Option<DeviceFinderRef>,
    net_manager: NetManagerRef,
    sn_service: Option<SNClientServiceRef>,
    cert_factory: P2pIdentityCertFactoryRef,
    pn_network: Option<TunnelNetworkRef>,
    conn_info_cache: P2pConnectionInfoCacheRef,
    gen_id: Arc<TunnelIdGenerator>,
    conn_timeout: Duration,
    proxy_upgrade_initial_interval: Duration,
    tunnels: RwLock<HashMap<P2pId, TunnelEntries>>,
    state: Mutex<ManagerState>,
    subscriptions: Mutex<Vec<IncomingTunnelCallback>>,
    cleanup_task: SpawnHandle<()>,
    proxy_upgrade_notify: Arc<AsyncNotify>,
    proxy_upgrade_task: SpawnHandle<()>,
}

pub type TunnelManagerRef = Arc<TunnelManager>;

impl TunnelManager {
    pub fn new(
        local_identity: P2pIdentityRef,
        device_finder: Option<DeviceFinderRef>,
        net_manager: NetManagerRef,
        sn_service: Option<SNClientServiceRef>,
        cert_factory: P2pIdentityCertFactoryRef,
        pn_network: Option<TunnelNetworkRef>,
        conn_info_cache: P2pConnectionInfoCacheRef,
        gen_id: Arc<TunnelIdGenerator>,
        conn_timeout: Duration,
        idle_timeout: Duration,
        proxy_upgrade_initial_interval: Duration,
    ) -> P2pResult<TunnelManagerRef> {
        let sn_service_for_listener = sn_service.clone();
        let proxy_upgrade_notify = Arc::new(AsyncNotify::new());
        let manager = Arc::new_cyclic(|weak: &std::sync::Weak<Self>| Self {
            self_weak: weak.clone(),
            local_identity: local_identity.clone(),
            device_finder: device_finder.clone(),
            net_manager: net_manager.clone(),
            sn_service: sn_service.clone(),
            cert_factory: cert_factory.clone(),
            pn_network: pn_network.clone(),
            conn_info_cache: conn_info_cache.clone(),
            gen_id: gen_id.clone(),
            conn_timeout,
            proxy_upgrade_initial_interval,
            tunnels: RwLock::new(HashMap::new()),
            state: Mutex::new(ManagerState {
                endpoint_scores: HashMap::new(),
                pending_reverse_waiters: HashMap::new(),
                proxy_upgrade_states: HashMap::new(),
            }),
            subscriptions: Mutex::new(Vec::new()),
            cleanup_task: Executor::spawn_with_handle({
                let weak = weak.clone();
                async move {
                    loop {
                        runtime::sleep(HOUSEKEEPING_INTERVAL).await;
                        let Some(manager) = weak.upgrade() else {
                            break;
                        };
                        manager.cleanup_closed_tunnels(idle_timeout).await;
                    }
                }
            })
            .unwrap(),
            proxy_upgrade_notify: proxy_upgrade_notify.clone(),
            proxy_upgrade_task: Executor::spawn_with_handle({
                let weak = weak.clone();
                async move {
                    loop {
                        let wait_duration = match weak.upgrade() {
                            Some(manager) => manager.next_proxy_upgrade_wait(),
                            None => break,
                        };

                        match wait_duration {
                            Some(delay) => {
                                tokio::select! {
                                    _ = runtime::sleep(delay) => {}
                                    _ = proxy_upgrade_notify.notified() => {}
                                }
                            }
                            None => {
                                proxy_upgrade_notify.notified().await;
                            }
                        }

                        let Some(manager) = weak.upgrade() else {
                            break;
                        };
                        manager.process_proxy_upgrade_attempts().await;
                    }
                }
            })
            .unwrap(),
        });
        let weak = Arc::downgrade(&manager);
        manager.net_manager.register_incoming_tunnel_subscriber(
            local_identity.get_id(),
            Arc::new(move |result| {
                let Some(manager) = weak.upgrade() else {
                    return Box::pin(async { false });
                };
                let tunnel = match result {
                    Ok(tunnel) => tunnel,
                    Err(err) => {
                        log::warn!("receive incoming tunnel failed: {:?}", err);
                        return Box::pin(async { true });
                    }
                };
                Box::pin(async move {
                    if let Err(err) = manager.on_incoming_tunnel(tunnel).await {
                        log::warn!("register incoming tunnel failed: {:?}", err);
                    }
                    true
                })
            }),
        )?;
        if let Some(sn_service) = sn_service_for_listener {
            let weak = Arc::downgrade(&manager);
            sn_service.set_listener(move |called: SnCalled| {
                let weak = weak.clone();
                async move {
                    Executor::spawn(async move {
                        if let Some(manager) = weak.upgrade() {
                            if let Err(err) = manager.on_sn_called(called).await {
                                log::warn!("handle network reverse sn call failed: {:?}", err);
                            }
                        }
                    });
                    Ok(())
                }
            });
        }
        Ok(manager)
    }

    pub fn subscribe(&self, callback: IncomingTunnelCallback) {
        self.subscriptions.lock().unwrap().push(callback);
    }

    pub fn get_tunnel(&self, remote_id: &P2pId) -> Option<TunnelRef> {
        self.get_tunnel_with_filter(remote_id, |_| true, true)
    }

    fn has_available_tunnel_id(&self, remote_id: &P2pId, tunnel_id: TunnelId) -> bool {
        let mut tunnels = self.tunnels.write().unwrap();
        let Some(entries) = tunnels.get_mut(remote_id) else {
            return false;
        };
        entries.retain(|entry| is_tunnel_available(entry.tunnel.as_ref()));
        let exists = entries
            .iter()
            .any(|entry| entry.tunnel.tunnel_id() == tunnel_id);
        if entries.is_empty() {
            tunnels.remove(remote_id);
        }
        exists
    }

    fn get_non_proxy_tunnel(&self, remote_id: &P2pId) -> Option<TunnelRef> {
        self.get_tunnel_with_filter(
            remote_id,
            |entry| entry.tunnel.form() != TunnelForm::Proxy,
            false,
        )
    }

    fn get_tunnel_with_filter<F>(
        &self,
        remote_id: &P2pId,
        filter: F,
        allow_hidden_fallback: bool,
    ) -> Option<TunnelRef>
    where
        F: Fn(&TunnelEntry) -> bool,
    {
        let (selected, should_track_proxy_upgrade) = {
            let mut tunnels = self.tunnels.write().unwrap();
            let entries = tunnels.get_mut(remote_id)?;
            let mut removed_unavailable = Vec::new();
            entries.retain(|entry| {
                let keep = is_tunnel_available(entry.tunnel.as_ref());
                if !keep {
                    removed_unavailable.push(format!(
                        "tunnel_id={:?} candidate_id={:?} form={:?} protocol={:?} reverse={} published={} state={:?} is_closed={}",
                        entry.tunnel.tunnel_id(),
                        entry.tunnel.candidate_id(),
                        entry.tunnel.form(),
                        entry.tunnel.protocol(),
                        entry.tunnel.is_reverse(),
                        entry.published,
                        entry.tunnel.state(),
                        entry.tunnel.is_closed()
                    ));
                }
                keep
            });
            if !removed_unavailable.is_empty() {
                log::warn!(
                    "get tunnel local={} remote={} dropped unavailable entries [{}]",
                    self.local_identity.get_id(),
                    remote_id,
                    removed_unavailable.join("; ")
                );
            }
            let published = entries
                .iter()
                .filter(|entry| filter(entry))
                .filter(|entry| entry.published);
            let selected = Self::select_preferred_tunnel_entry(published)
                .or_else(|| {
                    if !allow_hidden_fallback {
                        return None;
                    }
                    Self::select_preferred_tunnel_entry(
                        entries.iter().filter(|entry| filter(entry)),
                    )
                })
                .map(|entry| entry.tunnel.clone());
            let should_track_proxy_upgrade = Self::only_published_proxy_candidates(entries);
            if entries.is_empty() {
                tunnels.remove(remote_id);
            }
            (selected, should_track_proxy_upgrade)
        };
        if should_track_proxy_upgrade {
            self.track_proxy_upgrade_if_missing(remote_id);
        }
        selected
    }

    fn select_preferred_tunnel_entry<'a, I>(entries: I) -> Option<&'a TunnelEntry>
    where
        I: IntoIterator<Item = &'a TunnelEntry>,
    {
        let mut latest_non_proxy = None;
        let mut latest_proxy = None;
        for entry in entries {
            if entry.tunnel.form() == TunnelForm::Proxy {
                if latest_proxy
                    .map(|latest: &TunnelEntry| entry.updated_at > latest.updated_at)
                    .unwrap_or(true)
                {
                    latest_proxy = Some(entry);
                }
            } else if latest_non_proxy
                .map(|latest: &TunnelEntry| entry.updated_at > latest.updated_at)
                .unwrap_or(true)
            {
                latest_non_proxy = Some(entry);
            }
        }
        latest_non_proxy.or(latest_proxy)
    }

    fn only_published_proxy_candidates(entries: &[TunnelEntry]) -> bool {
        !entries.is_empty()
            && entries
                .iter()
                .all(|entry| entry.tunnel.form() == TunnelForm::Proxy)
            && entries.iter().any(|entry| entry.published)
    }

    fn next_candidate_id(&self) -> TunnelCandidateId {
        TunnelCandidateId::from(self.gen_id.generate().value())
    }

    pub async fn open_tunnel(&self, remote: &P2pIdentityCertRef) -> P2pResult<TunnelRef> {
        self.open_known_tunnel(
            remote.endpoints(),
            &remote.get_id(),
            Some(remote.get_name()),
        )
        .await
    }

    pub async fn open_tunnel_from_id(&self, remote_id: &P2pId) -> P2pResult<TunnelRef> {
        if let Some(tunnel) = self.get_tunnel(remote_id) {
            return Ok(tunnel);
        }

        if let Some(device_finder) = self.device_finder.as_ref() {
            match device_finder.get_identity_cert(remote_id).await {
                Ok(remote) => return self.open_tunnel(&remote).await,
                Err(e) => {
                    log::warn!(
                        "device_finder lookup failed for {}, try proxy: {}",
                        remote_id,
                        e
                    );
                }
            }
        }

        self.open_proxy_path(
            remote_id,
            None,
            TunnelConnectIntent::active_logical(self.gen_id.generate()),
        )
        .await
    }

    pub async fn open_direct_tunnel(
        &self,
        remote_eps: Vec<Endpoint>,
        remote_id: &P2pId,
    ) -> P2pResult<TunnelRef> {
        if let Some(tunnel) = self.get_tunnel(remote_id) {
            return Ok(tunnel);
        }
        self.open_known_tunnel(remote_eps, remote_id, None).await
    }

    async fn on_incoming_tunnel(&self, tunnel: TunnelRef) -> P2pResult<()> {
        let remote_id = tunnel.remote_id();
        let tunnel_id = tunnel.tunnel_id();
        log::debug!(
            "incoming tunnel remote={} tunnel_id={:?} form={:?} reverse={} protocol={:?} local_ep={:?} remote_ep={:?}",
            remote_id,
            tunnel_id,
            tunnel.form(),
            tunnel.is_reverse(),
            tunnel.protocol(),
            tunnel.local_ep(),
            tunnel.remote_ep()
        );
        if tunnel.is_reverse() {
            let Some(waiter) = self.take_reverse_waiter(&remote_id, &tunnel_id) else {
                log::debug!(
                    "incoming tunnel remote={} tunnel_id={:?} no reverse waiter, close tunnel",
                    remote_id,
                    tunnel_id
                );
                tunnel.close()?;
                return Ok(());
            };
            log::debug!(
                "incoming tunnel remote={} tunnel_id={:?} matched reverse waiter",
                remote_id,
                tunnel_id
            );
            waiter.notify(Ok(tunnel));
            return Ok(());
        }
        let tunnel = self.register_tunnel(tunnel).await?;
        self.publish_registered_tunnel(&tunnel)?;
        log::debug!(
            "incoming tunnel remote={} tunnel_id={:?} ignore reverse waiter because reverse=false",
            remote_id,
            tunnel_id
        );
        Ok(())
    }

    async fn open_known_tunnel(
        &self,
        remote_eps: Vec<Endpoint>,
        remote_id: &P2pId,
        remote_name: Option<String>,
    ) -> P2pResult<TunnelRef> {
        self.open_known_tunnel_with_options(remote_eps, remote_id, remote_name, true, true)
            .await
    }

    async fn open_known_tunnel_with_options(
        &self,
        remote_eps: Vec<Endpoint>,
        remote_id: &P2pId,
        remote_name: Option<String>,
        reuse_existing_proxy: bool,
        allow_proxy_fallback: bool,
    ) -> P2pResult<TunnelRef> {
        log::debug!(
            "open tunnel remote={} name={:?} eps_count={} eps={:?} has_sn={} has_pn={} reuse_existing_proxy={} allow_proxy_fallback={}",
            remote_id,
            remote_name,
            remote_eps.len(),
            remote_eps,
            self.sn_service.is_some(),
            self.pn_network.is_some(),
            reuse_existing_proxy,
            allow_proxy_fallback
        );
        if remote_eps.is_empty() {
            return Err(p2p_err!(
                P2pErrorCode::InvalidParam,
                "remote endpoints empty"
            ));
        }

        let lock_name = format!("network-tunnel-{}", remote_id);
        let _guard = Locker::get_locker(lock_name).await;
        let logical_tunnel_id = self.gen_id.generate();

        let existing_tunnel = if reuse_existing_proxy {
            self.get_tunnel(remote_id)
        } else {
            self.get_non_proxy_tunnel(remote_id)
        };
        if let Some(tunnel) = existing_tunnel {
            return Ok(tunnel);
        }

        let mut last_err = None;
        let mut tried_reverse = false;
        if let Some(info) = self.conn_info_cache.get(remote_id).await {
            match info.direct {
                ConnectDirection::Direct => {
                    if remote_eps.iter().any(|ep| ep == &info.remote_ep) {
                        match self
                            .open_direct_path(
                                vec![info.remote_ep],
                                remote_id,
                                remote_name.clone(),
                                TunnelConnectIntent::active_logical(logical_tunnel_id),
                            )
                            .await
                        {
                            Ok(tunnel) => return Ok(tunnel),
                            Err(err) => last_err = Some(err),
                        }
                    }
                }
                ConnectDirection::Reverse => {
                    match self.open_reverse_path(remote_id, logical_tunnel_id).await {
                        Ok(tunnel) => return Ok(tunnel),
                        Err(err) => last_err = Some(err),
                    }
                }
                ConnectDirection::Proxy => {
                    if allow_proxy_fallback {
                        match self
                            .open_proxy_path(
                                remote_id,
                                remote_name.clone(),
                                TunnelConnectIntent::active_logical(logical_tunnel_id),
                            )
                            .await
                        {
                            Ok(tunnel) => return Ok(tunnel),
                            Err(err) => last_err = Some(err),
                        }
                    }
                }
            }
        }

        let direct_eps = self
            .preferred_direct_endpoints(Some(remote_id), remote_eps.as_slice())
            .await;
        log::debug!(
            "open tunnel remote={} preferred direct eps {:?}",
            remote_id,
            direct_eps
        );
        if !direct_eps.is_empty() && self.sn_service.is_some() {
            tried_reverse = true;
            let reverse_delay = Self::hedged_reverse_delay_for_endpoints(direct_eps.as_slice());
            log::debug!(
                "open tunnel remote={} tunnel_id={:?} start hedged direct+reverse delay_ms={}",
                remote_id,
                logical_tunnel_id,
                reverse_delay.as_millis()
            );
            let (result, direct_won) = race_with_delay(
                self.open_direct_path(
                    direct_eps.clone(),
                    remote_id,
                    remote_name.clone(),
                    TunnelConnectIntent::active_logical(logical_tunnel_id),
                ),
                reverse_delay,
                self.open_reverse_path(remote_id, logical_tunnel_id),
            )
            .await;
            match result {
                Ok(tunnel) => {
                    log::debug!(
                        "open tunnel remote={} hedged success winner={}",
                        remote_id,
                        if direct_won { "direct" } else { "reverse" }
                    );
                    return Ok(tunnel);
                }
                Err(err) => {
                    log::warn!(
                        "open tunnel remote={} hedged failed winner={} code={:?} msg={}",
                        remote_id,
                        if direct_won { "direct" } else { "reverse" },
                        err.code(),
                        err.msg()
                    );
                    last_err = Some(err);
                    if direct_won {
                        log::debug!(
                            "hedged connect remote {} direct failed after reverse",
                            remote_id
                        );
                    } else {
                        log::debug!(
                            "hedged connect remote {} reverse failed after direct",
                            remote_id
                        );
                    }
                }
            }
        } else {
            match self
                .open_direct_path(
                    direct_eps.clone(),
                    remote_id,
                    remote_name.clone(),
                    TunnelConnectIntent::active_logical(logical_tunnel_id),
                )
                .await
            {
                Ok(tunnel) => return Ok(tunnel),
                Err(err) => last_err = Some(err),
            }
        }

        if !tried_reverse {
            match self.open_reverse_path(remote_id, logical_tunnel_id).await {
                Ok(tunnel) => return Ok(tunnel),
                Err(err) => last_err = Some(err),
            }
        }

        if allow_proxy_fallback {
            match self
                .open_proxy_path(
                    remote_id,
                    remote_name,
                    TunnelConnectIntent::active_logical(logical_tunnel_id),
                )
                .await
            {
                Ok(tunnel) => return Ok(tunnel),
                Err(err) => last_err = Some(err),
            }
        }

        Err(last_err
            .unwrap_or_else(|| p2p_err!(P2pErrorCode::NotFound, "no tunnel network matched")))
    }

    async fn open_direct_path(
        &self,
        remote_eps: Vec<Endpoint>,
        remote_id: &P2pId,
        remote_name: Option<String>,
        intent: TunnelConnectIntent,
    ) -> P2pResult<TunnelRef> {
        let mut last_err = None;
        let local_identity = self.local_identity.clone();
        let connect_remote_id = remote_id.clone();
        let manager_weak = self.self_weak.clone();
        let mut connect_futures = FuturesUnordered::new();

        log::debug!(
            "direct path start remote={} tunnel_id={:?} reverse={} eps_count={} eps={:?}",
            remote_id,
            intent.tunnel_id,
            intent.is_reverse,
            remote_eps.len(),
            remote_eps
        );

        for remote_ep in remote_eps {
            let network = match self.net_manager.get_network(remote_ep.protocol()) {
                Ok(network) => network,
                Err(err) => {
                    last_err = Some(err);
                    continue;
                }
            };
            let local_identity = local_identity.clone();
            let connect_remote_id = connect_remote_id.clone();
            let remote_name = remote_name.clone();
            let remote_id = remote_id.clone();
            let manager_weak = manager_weak.clone();
            let intent = Self::intent_for_candidate(
                &remote_ep,
                TunnelConnectIntent {
                    candidate_id: self.next_candidate_id(),
                    ..intent
                },
                self.sn_service.is_some(),
            );
            connect_futures.push(Box::pin(async move {
                log::debug!(
                    "direct path attempt remote={} tunnel_id={:?} candidate_id={:?} ep={:?}",
                    connect_remote_id,
                    intent.tunnel_id,
                    intent.candidate_id,
                    remote_ep
                );
                match network
                    .create_tunnel_with_intent(
                        &local_identity,
                        &remote_ep,
                        &connect_remote_id,
                        remote_name,
                        intent,
                    )
                    .await
                {
                    Ok(tunnel) => {
                        if let Some(manager) = manager_weak.upgrade() {
                            manager.on_direct_connect_result(&remote_ep, true);
                            match manager.register_tunnel_and_publish(tunnel).await {
                                Ok(tunnel) => {
                                    manager
                                        .conn_info_cache
                                        .add(
                                            remote_id,
                                            P2pConnectionInfo {
                                                direct: ConnectDirection::Direct,
                                                local_ep: tunnel.local_ep().unwrap_or_default(),
                                                remote_ep,
                                            },
                                        )
                                        .await;
                                    log::debug!(
                                        "direct path success remote={} tunnel_id={:?} candidate_id={:?} ep={:?}",
                                        connect_remote_id,
                                        intent.tunnel_id,
                                        intent.candidate_id,
                                        remote_ep
                                    );
                                    Ok((remote_ep, tunnel))
                                }
                                Err(err) => Err(err),
                            }
                        } else {
                            Err(p2p_err!(
                                P2pErrorCode::Interrupted,
                                "tunnel manager dropped during direct connect"
                            ))
                        }
                    }
                    Err(err) => {
                        if let Some(manager) = manager_weak.upgrade() {
                            manager.on_direct_connect_result(&remote_ep, false);
                        }
                        log::debug!(
                            "direct path failed remote={} tunnel_id={:?} candidate_id={:?} ep={:?} code={:?} msg={}",
                            connect_remote_id,
                            intent.tunnel_id,
                            intent.candidate_id,
                            remote_ep,
                            err.code(),
                            err.msg()
                        );
                        Err(err)
                    }
                }
            }));
        }

        while let Some(result) = connect_futures.next().await {
            match result {
                Ok((remote_ep, tunnel)) => {
                    log::debug!(
                        "direct path selected remote={} ep={:?}",
                        remote_id,
                        remote_ep
                    );
                    if !connect_futures.is_empty() {
                        Executor::spawn_ok(async move {
                            while let Some(result) = connect_futures.next().await {
                                if let Err(err) = result {
                                    log::trace!(
                                        "direct path background candidate failed: {:?}",
                                        err
                                    );
                                }
                            }
                        });
                    }
                    return Ok(tunnel);
                }
                Err(err) => {
                    last_err = Some(err);
                }
            }
        }

        Err(last_err
            .unwrap_or_else(|| p2p_err!(P2pErrorCode::ConnectFailed, "direct connect failed")))
    }

    fn hedged_reverse_delay_for_endpoints(_endpoints: &[Endpoint]) -> Duration {
        HEDGED_REVERSE_DELAY
    }

    fn udp_punch_enabled_for_candidate(endpoint: &Endpoint) -> bool {
        endpoint.protocol() == Protocol::Quic
            && endpoint.get_area() == EndpointArea::ServerReflexive
            && is_non_lan_ipv4_addr(endpoint.addr())
            && endpoint.addr().port() != 0
    }

    fn intent_for_candidate(
        endpoint: &Endpoint,
        intent: TunnelConnectIntent,
        sn_service_available: bool,
    ) -> TunnelConnectIntent {
        if sn_service_available && Self::udp_punch_enabled_for_candidate(endpoint) {
            intent.set_udp_punch_enabled(true)
        } else {
            intent
        }
    }

    fn push_unique_endpoint(endpoints: &mut Vec<Endpoint>, ep: Endpoint) {
        if !endpoints.contains(&ep) {
            endpoints.push(ep);
        }
    }

    fn collect_reverse_endpoints_from_sources(
        listener_entries: Vec<(crate::endpoint::Protocol, Vec<TunnelListenerInfo>)>,
        wan_endpoints: Vec<Endpoint>,
    ) -> Vec<Endpoint> {
        let mut endpoints = Vec::new();
        let mut mapping_ports = Vec::new();

        for (protocol, listeners) in listener_entries {
            for listener in listeners {
                if !listener.local.addr().ip().is_unspecified() {
                    Self::push_unique_endpoint(&mut endpoints, listener.local);
                }
                if let Some(port) = listener.mapping_port {
                    mapping_ports.push((protocol, port));
                }
            }
        }

        for wan_ep in wan_endpoints {
            Self::push_unique_endpoint(&mut endpoints, wan_ep);
            for (protocol, port) in mapping_ports.iter() {
                let mut mapped = Endpoint::from((*protocol, wan_ep.addr().ip(), *port));
                mapped.set_area(crate::endpoint::EndpointArea::Mapped);
                Self::push_unique_endpoint(&mut endpoints, mapped);
            }
        }

        endpoints
    }

    fn reverse_endpoints(&self) -> Vec<Endpoint> {
        let wan_endpoints = self
            .sn_service
            .as_ref()
            .map(|sn_service| sn_service.get_wan_ip_list())
            .unwrap_or_default();
        Self::collect_reverse_endpoints_from_sources(
            self.net_manager.listener_info_entries(),
            wan_endpoints,
        )
    }

    async fn open_reverse_path(
        &self,
        remote_id: &P2pId,
        tunnel_id: TunnelId,
    ) -> P2pResult<TunnelRef> {
        let sn_service = self
            .sn_service
            .as_ref()
            .ok_or_else(|| p2p_err!(P2pErrorCode::NotSupport, "sn service not configured"))?;
        let (notify, waiter) = Notify::new();
        let mut waiter_registration =
            ReverseWaitRegistration::register(self, remote_id.clone(), tunnel_id, notify);
        log::debug!(
            "reverse path start remote={} tunnel_id={:?} add waiter",
            remote_id,
            tunnel_id
        );
        let reverse_endpoints = self.reverse_endpoints();
        let reverse_endpoint_arg = if reverse_endpoints.is_empty() {
            None
        } else {
            Some(reverse_endpoints.as_slice())
        };
        let call_result = sn_service
            .call(
                tunnel_id,
                reverse_endpoint_arg,
                remote_id,
                TunnelType::Stream,
                vec![],
            )
            .await;
        if let Err(err) = call_result {
            log::warn!(
                "reverse path remote={} tunnel_id={:?} sn call failed code={:?} msg={}",
                remote_id,
                tunnel_id,
                err.code(),
                err.msg()
            );
            return Err(err);
        }

        log::debug!(
            "reverse path remote={} tunnel_id={:?} sn call ok, waiting incoming tunnel timeout_ms={}",
            remote_id,
            tunnel_id,
            self.conn_timeout.as_millis()
        );
        let tunnel = runtime::timeout(self.conn_timeout, waiter).await;
        if tunnel.is_err() || matches!(tunnel, Ok(Err(_))) {
            log::debug!(
                "reverse path remote={} tunnel_id={:?} waiter cleanup after timeout/error",
                remote_id,
                tunnel_id
            );
        }
        let tunnel = tunnel.map_err(into_p2p_err!(P2pErrorCode::Timeout))??;
        waiter_registration.dismiss();
        let tunnel = self.register_tunnel(tunnel).await?;
        self.publish_registered_tunnel(&tunnel)?;
        log::debug!(
            "reverse path remote={} tunnel_id={:?} incoming tunnel ready local_ep={:?} remote_ep={:?}",
            remote_id,
            tunnel_id,
            tunnel.local_ep(),
            tunnel.remote_ep()
        );
        self.conn_info_cache
            .add(
                remote_id.clone(),
                P2pConnectionInfo {
                    direct: ConnectDirection::Reverse,
                    local_ep: tunnel.local_ep().unwrap_or_default(),
                    remote_ep: tunnel.remote_ep().unwrap_or_default(),
                },
            )
            .await;
        Ok(tunnel)
    }

    async fn open_proxy_path(
        &self,
        remote_id: &P2pId,
        remote_name: Option<String>,
        intent: TunnelConnectIntent,
    ) -> P2pResult<TunnelRef> {
        let pn_network = self
            .pn_network
            .as_ref()
            .ok_or_else(|| p2p_err!(P2pErrorCode::NotSupport, "proxy client not configured"))?;
        let tunnel = pn_network
            .create_tunnel_with_intent(
                &self.local_identity,
                &pn_virtual_endpoint(),
                remote_id,
                remote_name,
                intent,
            )
            .await?;
        let tunnel = self.register_tunnel_and_publish(tunnel).await?;
        self.conn_info_cache
            .add(
                remote_id.clone(),
                P2pConnectionInfo {
                    direct: ConnectDirection::Proxy,
                    local_ep: tunnel.local_ep().unwrap_or_default(),
                    remote_ep: tunnel.remote_ep().unwrap_or_default(),
                },
            )
            .await;
        Ok(tunnel)
    }

    async fn preferred_direct_endpoints(
        &self,
        remote_id: Option<&P2pId>,
        endpoints: &[Endpoint],
    ) -> Vec<Endpoint> {
        let preferred_ep = if let Some(remote_id) = remote_id {
            self.conn_info_cache
                .get(remote_id)
                .await
                .map(|info| info.remote_ep)
        } else {
            None
        };
        let state = self.state.lock().unwrap();
        let mut ranked: Vec<(i64, usize, Endpoint)> = endpoints
            .iter()
            .enumerate()
            .map(|(idx, ep)| {
                let mut score: i64 = 0;
                if preferred_ep
                    .map(|preferred| preferred == *ep)
                    .unwrap_or(false)
                {
                    score += 10_000;
                }
                if ep.is_static_wan() {
                    score += 500;
                }
                if let Some(stat) = state.endpoint_scores.get(&EndpointScoreKey::new(ep)) {
                    if stat.last_success_at > 0 {
                        score += 2_000;
                    }
                    score -= (stat.fail_count.min(20) as i64) * 300;
                }
                (score, idx, *ep)
            })
            .collect();
        ranked.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        ranked.into_iter().map(|(_, _, ep)| ep).collect()
    }

    fn on_direct_connect_result(&self, endpoint: &Endpoint, success: bool) {
        let mut state = self.state.lock().unwrap();
        let stat = state
            .endpoint_scores
            .entry(EndpointScoreKey::new(endpoint))
            .or_insert(EndpointScore {
                last_success_at: 0,
                fail_count: 0,
            });
        if success {
            stat.last_success_at = bucky_time_now();
            stat.fail_count = 0;
        } else {
            stat.fail_count = stat.fail_count.saturating_add(1);
        }
    }

    fn add_reverse_waiter(
        &self,
        remote_id: P2pId,
        tunnel_id: TunnelId,
        waiter: Notify<P2pResult<TunnelRef>>,
    ) {
        self.state
            .lock()
            .unwrap()
            .pending_reverse_waiters
            .insert((remote_id, tunnel_id), waiter);
    }

    fn take_reverse_waiter(
        &self,
        remote_id: &P2pId,
        tunnel_id: &TunnelId,
    ) -> Option<Notify<P2pResult<TunnelRef>>> {
        self.state
            .lock()
            .unwrap()
            .pending_reverse_waiters
            .remove(&(remote_id.clone(), *tunnel_id))
    }

    fn has_reverse_waiter(&self, remote_id: &P2pId, tunnel_id: &TunnelId) -> bool {
        self.state
            .lock()
            .unwrap()
            .pending_reverse_waiters
            .contains_key(&(remote_id.clone(), *tunnel_id))
    }

    fn track_proxy_upgrade(&self, remote_id: &P2pId) {
        let now = Instant::now();
        let mut state = self.state.lock().unwrap();
        state
            .proxy_upgrade_states
            .entry(remote_id.clone())
            .and_modify(|entry| {
                *entry = ProxyUpgradeState::new(now, self.proxy_upgrade_initial_interval);
            })
            .or_insert_with(|| ProxyUpgradeState::new(now, self.proxy_upgrade_initial_interval));
        drop(state);
        self.proxy_upgrade_notify.notify_one();
    }

    fn track_proxy_upgrade_if_missing(&self, remote_id: &P2pId) {
        let now = Instant::now();
        let mut state = self.state.lock().unwrap();
        if state.proxy_upgrade_states.contains_key(remote_id) {
            return;
        }
        state.proxy_upgrade_states.insert(
            remote_id.clone(),
            ProxyUpgradeState::new(now, self.proxy_upgrade_initial_interval),
        );
        drop(state);
        self.proxy_upgrade_notify.notify_one();
    }

    fn clear_proxy_upgrade(&self, remote_id: &P2pId) {
        self.state
            .lock()
            .unwrap()
            .proxy_upgrade_states
            .remove(remote_id);
        self.proxy_upgrade_notify.notify_one();
    }

    fn next_proxy_upgrade_wait(&self) -> Option<Duration> {
        let now = Instant::now();
        let state = self.state.lock().unwrap();
        state
            .proxy_upgrade_states
            .values()
            .filter(|upgrade| !upgrade.in_progress)
            .map(|upgrade| upgrade.next_attempt_at.saturating_duration_since(now))
            .min()
    }

    fn collect_due_proxy_upgrades(&self) -> Vec<P2pId> {
        let now = Instant::now();
        let mut state = self.state.lock().unwrap();
        let mut due = Vec::new();
        for (remote_id, upgrade) in state.proxy_upgrade_states.iter_mut() {
            if !upgrade.in_progress && upgrade.next_attempt_at <= now {
                upgrade.in_progress = true;
                due.push(remote_id.clone());
            }
        }
        due
    }

    fn on_proxy_upgrade_failed(&self, remote_id: &P2pId) -> Option<Duration> {
        let mut state = self.state.lock().unwrap();
        let upgrade = state.proxy_upgrade_states.get_mut(remote_id)?;
        upgrade.in_progress = false;
        advance_proxy_upgrade_retry(upgrade, self.proxy_upgrade_initial_interval);
        upgrade.next_attempt_at = Instant::now() + upgrade.retry_interval;
        let next_interval = upgrade.retry_interval;
        drop(state);
        self.proxy_upgrade_notify.notify_one();
        Some(next_interval)
    }

    async fn process_proxy_upgrade_attempts(&self) {
        let due = self.collect_due_proxy_upgrades();
        if due.is_empty() {
            return;
        }

        for remote_id in due {
            let weak = self.self_weak.clone();
            Executor::spawn_ok(async move {
                let Some(manager) = weak.upgrade() else {
                    return;
                };
                manager.attempt_proxy_upgrade(remote_id).await;
            });
        }
    }

    async fn attempt_proxy_upgrade(&self, remote_id: P2pId) {
        match self.try_upgrade_proxy_tunnel(&remote_id).await {
            Ok(tunnel) => {
                self.clear_proxy_upgrade(&remote_id);
                self.close_proxy_tunnels_after_upgrade(&remote_id, &tunnel)
                    .await;
                log::info!(
                    "proxy upgrade succeeded remote={} form={:?} protocol={:?}",
                    remote_id,
                    tunnel.form(),
                    tunnel.protocol()
                );
            }
            Err(err) => {
                let next_interval = self.on_proxy_upgrade_failed(&remote_id);
                log::warn!(
                    "proxy upgrade failed remote={} code={:?} msg={} next_retry_secs={:?}",
                    remote_id,
                    err.code(),
                    err.msg(),
                    next_interval.map(|interval| interval.as_secs())
                );
            }
        }
    }

    async fn try_upgrade_proxy_tunnel(&self, remote_id: &P2pId) -> P2pResult<TunnelRef> {
        if let Some(tunnel) = self.get_non_proxy_tunnel(remote_id) {
            return Ok(tunnel);
        }

        if let Some(device_finder) = self.device_finder.as_ref() {
            match device_finder.get_identity_cert(remote_id).await {
                Ok(remote) if !remote.endpoints().is_empty() => {
                    return self
                        .open_known_tunnel_with_options(
                            remote.endpoints().clone(),
                            remote_id,
                            Some(remote.get_name()),
                            false,
                            false,
                        )
                        .await;
                }
                Ok(_) => {
                    log::debug!(
                        "proxy upgrade remote={} skipped direct path because endpoints are empty",
                        remote_id
                    );
                }
                Err(err) => {
                    log::warn!(
                        "proxy upgrade device lookup failed for {}: {}",
                        remote_id,
                        err
                    );
                }
            }
        }

        let tunnel = self
            .open_reverse_path(remote_id, self.gen_id.generate())
            .await?;
        Ok(tunnel)
    }

    async fn close_proxy_tunnels_after_upgrade(&self, remote_id: &P2pId, tunnel: &TunnelRef) {
        if tunnel.form() == TunnelForm::Proxy {
            return;
        }

        let proxy_tunnels = {
            let mut tunnels = self.tunnels.write().unwrap();
            let Some(entries) = tunnels.get_mut(remote_id) else {
                return;
            };
            let mut proxy_tunnels = Vec::new();
            entries.retain(|entry| {
                let is_proxy = entry.tunnel.form() == TunnelForm::Proxy;
                if is_proxy {
                    proxy_tunnels.push(entry.tunnel.clone());
                }
                !is_proxy
            });
            if entries.is_empty() {
                tunnels.remove(remote_id);
            }
            proxy_tunnels
        };

        for proxy in proxy_tunnels {
            log::debug!(
                "close proxy tunnel after upgrade remote={} proxy_tunnel_id={:?} proxy_candidate_id={:?} upgraded_tunnel_id={:?} upgraded_candidate_id={:?} upgraded_form={:?}",
                remote_id,
                proxy.tunnel_id(),
                proxy.candidate_id(),
                tunnel.tunnel_id(),
                tunnel.candidate_id(),
                tunnel.form()
            );
            if let Err(err) = proxy.close() {
                log::warn!(
                    "close proxy tunnel after upgrade failed remote={} tunnel_id={:?} candidate_id={:?} code={:?} msg={}",
                    remote_id,
                    proxy.tunnel_id(),
                    proxy.candidate_id(),
                    err.code(),
                    err.msg()
                );
            }
        }
    }

    async fn on_sn_called(&self, called: SnCalled) -> P2pResult<()> {
        let cert = self.cert_factory.create(&called.peer_info)?;
        let remote_id = cert.get_id();
        log::debug!(
            "sn called remote={} seq={} reverse_eps_count={} reverse_eps={:?} cert_eps={:?}",
            remote_id,
            called.seq.value(),
            called.reverse_endpoint_array.len(),
            called.reverse_endpoint_array,
            cert.endpoints()
        );
        if self.has_available_tunnel_id(&remote_id, called.tunnel_id) {
            log::debug!(
                "sn called remote={} tunnel_id={:?} already exists, skip reverse dial",
                remote_id,
                called.tunnel_id
            );
            return Ok(());
        }

        let mut eps = if self
            .sn_service
            .as_ref()
            .map(|service| service.is_same_lan(&called.reverse_endpoint_array))
            .unwrap_or(false)
        {
            let mut eps = cert.endpoints().clone();
            for ep in called.reverse_endpoint_array.iter() {
                if !eps.contains(ep) {
                    eps.push(*ep);
                }
            }
            eps
        } else {
            let mut eps = called.reverse_endpoint_array.clone();
            for ep in cert.endpoints().iter() {
                if !eps.contains(ep) {
                    eps.push(*ep);
                }
            }
            eps
        };
        eps = self
            .preferred_direct_endpoints(Some(&remote_id), eps.as_slice())
            .await;
        log::debug!(
            "sn called remote={} resolved reverse direct eps {:?}",
            remote_id,
            eps
        );
        let _guard = Locker::get_locker(format!("network-tunnel-{}", remote_id)).await;
        if self.has_available_tunnel_id(&remote_id, called.tunnel_id) {
            log::debug!(
                "sn called remote={} tunnel_id={:?} became available before dial",
                remote_id,
                called.tunnel_id
            );
            return Ok(());
        }
        match self
            .open_direct_path(
                eps,
                &remote_id,
                Some(cert.get_name()),
                TunnelConnectIntent::reverse_logical(called.tunnel_id),
            )
            .await
        {
            Ok(_) => {
                log::debug!("sn called remote={} reverse direct dial success", remote_id);
            }
            Err(err) => {
                log::warn!(
                    "sn called remote={} reverse direct dial failed code={:?} msg={}",
                    remote_id,
                    err.code(),
                    err.msg()
                );
                return Err(err);
            }
        }
        Ok(())
    }

    async fn register_tunnel(&self, tunnel: TunnelRef) -> P2pResult<TunnelRef> {
        let remote_id = tunnel.remote_id();
        log::debug!(
            "register tunnel local={:?} remote={} tunnel_id={:?} candidate_id={:?} form={:?} reverse={} protocol={:?} local_ep={:?} remote_ep={:?}",
            self.local_identity.get_id(),
            remote_id,
            tunnel.tunnel_id(),
            tunnel.candidate_id(),
            tunnel.form(),
            tunnel.is_reverse(),
            tunnel.protocol(),
            tunnel.local_ep(),
            tunnel.remote_ep()
        );
        if remote_id.is_default() {
            log::debug!(
                "register tunnel remote={} publish-only default_remote=true",
                remote_id,
            );
            return Ok(tunnel);
        }

        let _guard = Locker::get_locker(format!("network-register-{}", remote_id)).await;
        let remote_id_for_log = remote_id.clone();
        let duplicate_same_arc = {
            let mut tunnels = self.tunnels.write().unwrap();
            let entries = tunnels.entry(remote_id).or_default();
            entries.retain(|entry| is_tunnel_available(entry.tunnel.as_ref()));
            if let Some(entry) = entries.iter_mut().find(|entry| {
                entry.tunnel.tunnel_id() == tunnel.tunnel_id()
                    && entry.tunnel.candidate_id() == tunnel.candidate_id()
            }) {
                Some(Arc::ptr_eq(&entry.tunnel, &tunnel))
            } else {
                entries.push(TunnelEntry {
                    tunnel: tunnel.clone(),
                    updated_at: Instant::now(),
                    published: false,
                });
                None
            }
        };

        if let Some(same_arc) = duplicate_same_arc {
            log::warn!(
                "register tunnel local={} remote={} duplicate tunnel_id={:?} candidate_id={:?} form={:?} protocol={:?} reverse={} state={:?} is_closed={} same_arc={}",
                self.local_identity.get_id(),
                remote_id_for_log,
                tunnel.tunnel_id(),
                tunnel.candidate_id(),
                tunnel.form(),
                tunnel.protocol(),
                tunnel.is_reverse(),
                tunnel.state(),
                tunnel.is_closed(),
                same_arc
            );
            if !same_arc {
                let _ = tunnel.close();
            }
            return Err(p2p_err!(
                P2pErrorCode::AlreadyExists,
                "tunnel candidate already registered"
            ));
        }

        log::debug!(
            "register tunnel remote={} stored tunnel_id={:?} candidate_id={:?} form={:?} protocol={:?} published={}",
            remote_id_for_log,
            tunnel.tunnel_id(),
            tunnel.candidate_id(),
            tunnel.form(),
            tunnel.protocol(),
            self.is_registered_tunnel_published(&remote_id_for_log, &tunnel)
        );
        if tunnel.form() == TunnelForm::Proxy {
            self.track_proxy_upgrade(&remote_id_for_log);
        } else {
            self.clear_proxy_upgrade(&remote_id_for_log);
        }
        Ok(tunnel)
    }

    async fn register_tunnel_and_publish(&self, tunnel: TunnelRef) -> P2pResult<TunnelRef> {
        let tunnel = self.register_tunnel(tunnel).await?;
        self.publish_registered_tunnel(&tunnel)?;
        Ok(tunnel)
    }

    fn publish_registered_tunnel(&self, tunnel: &TunnelRef) -> P2pResult<()> {
        let mut should_publish = false;
        {
            let mut tunnels = self.tunnels.write().unwrap();
            if let Some(entries) = tunnels.get_mut(&tunnel.remote_id()) {
                if let Some(entry) = entries.iter_mut().find(|entry| {
                    entry.tunnel.tunnel_id() == tunnel.tunnel_id()
                        && entry.tunnel.candidate_id() == tunnel.candidate_id()
                }) {
                    if entry.published {
                        return Ok(());
                    }
                    entry.published = true;
                    entry.updated_at = Instant::now();
                    should_publish = true;
                }
            }
        }
        if !should_publish {
            log::warn!(
                "skip publish unregistered tunnel remote={} tunnel_id={:?} candidate_id={:?} form={:?}",
                tunnel.remote_id(),
                tunnel.tunnel_id(),
                tunnel.candidate_id(),
                tunnel.form()
            );
            return Err(p2p_err!(
                P2pErrorCode::NotFound,
                "publish unregistered tunnel remote={} tunnel_id={:?} candidate_id={:?}",
                tunnel.remote_id(),
                tunnel.tunnel_id(),
                tunnel.candidate_id()
            ));
        }
        self.publish_tunnel(tunnel.clone())
    }

    fn publish_tunnel(&self, tunnel: TunnelRef) -> P2pResult<()> {
        log::debug!(
            "publish tunnel remote={} form={:?} protocol={:?}",
            tunnel.remote_id(),
            tunnel.form(),
            tunnel.protocol()
        );
        let subscriptions = self.subscriptions.lock().unwrap().clone();
        for callback in subscriptions {
            let tunnel = tunnel.clone();
            Executor::spawn_ok(async move {
                callback(Ok(tunnel)).await;
            });
        }
        Ok(())
    }

    fn is_registered_tunnel_published(&self, remote_id: &P2pId, tunnel: &TunnelRef) -> bool {
        self.tunnels
            .read()
            .unwrap()
            .get(remote_id)
            .and_then(|entries| {
                entries.iter().find(|entry| {
                    entry.tunnel.tunnel_id() == tunnel.tunnel_id()
                        && entry.tunnel.candidate_id() == tunnel.candidate_id()
                })
            })
            .map(|entry| entry.published)
            .unwrap_or(false)
    }

    async fn cleanup_closed_tunnels(&self, _idle_timeout: Duration) {
        let mut removed = Vec::new();
        let mut proxy_only_remotes = Vec::new();
        {
            let mut tunnels = self.tunnels.write().unwrap();
            tunnels.retain(|remote_id, entries| {
                entries.retain(|entry| {
                    let keep = is_tunnel_available(entry.tunnel.as_ref());
                    if !keep {
                        removed.push(entry.tunnel.clone());
                    }
                    keep
                });
                if Self::only_published_proxy_candidates(entries) {
                    proxy_only_remotes.push(remote_id.clone());
                }
                !entries.is_empty()
            });
        }
        for remote_id in proxy_only_remotes {
            self.track_proxy_upgrade_if_missing(&remote_id);
        }
        for tunnel in removed {
            let _ = tunnel.close();
        }
    }
}

impl Drop for TunnelManager {
    fn drop(&mut self) {
        let tunnels = self
            .tunnels
            .write()
            .unwrap()
            .drain()
            .flat_map(|(_, entries)| entries.into_iter().map(|entry| entry.tunnel))
            .collect::<Vec<_>>();
        self.net_manager
            .unregister_incoming_tunnel_subscriber(&self.local_identity.get_id());
        self.cleanup_task.abort();
        self.proxy_upgrade_task.abort();
        for tunnel in tunnels {
            let _ = tunnel.close();
        }
    }
}

fn is_tunnel_available(tunnel: &dyn crate::networks::Tunnel) -> bool {
    !tunnel.is_closed() && tunnel.state() == TunnelState::Connected
}

#[cfg(test)]
mod nat_strategy_tests {
    use super::*;
    use crate::endpoint::{EndpointArea, Protocol};
    use crate::error::P2pError;
    use crate::p2p_identity::{
        EncodedP2pIdentity, EncodedP2pIdentityCert, P2pIdentity, P2pIdentityCert,
        P2pIdentityCertFactory, P2pIdentityCertRef, P2pIdentityRef, P2pIdentitySignType,
        P2pSignature,
    };
    use std::sync::atomic::AtomicUsize;
    use tokio::sync::mpsc;

    const TEST_SUBSCRIPTION_CHANNEL_CAPACITY: usize = 8;

    struct SelectionTunnel {
        form: TunnelForm,
    }

    struct TestIdentity {
        id: P2pId,
        name: String,
    }

    impl TestIdentity {
        fn new(id: u8, name: &str) -> P2pIdentityRef {
            Arc::new(Self {
                id: P2pId::from(vec![id; 32]),
                name: name.to_owned(),
            })
        }
    }

    impl P2pIdentity for TestIdentity {
        fn get_identity_cert(&self) -> P2pResult<P2pIdentityCertRef> {
            Err(p2p_err!(P2pErrorCode::NotSupport, "test identity cert"))
        }

        fn get_id(&self) -> P2pId {
            self.id.clone()
        }

        fn get_name(&self) -> String {
            self.name.clone()
        }

        fn sign_type(&self) -> P2pIdentitySignType {
            P2pIdentitySignType::Ed25519
        }

        fn sign(&self, _message: &[u8]) -> P2pResult<P2pSignature> {
            Err(p2p_err!(P2pErrorCode::NotSupport, "test sign"))
        }

        fn get_encoded_identity(&self) -> P2pResult<EncodedP2pIdentity> {
            Ok(Vec::new())
        }

        fn endpoints(&self) -> Vec<Endpoint> {
            Vec::new()
        }

        fn update_endpoints(&self, _eps: Vec<Endpoint>) -> P2pIdentityRef {
            Arc::new(Self {
                id: self.id.clone(),
                name: self.name.clone(),
            })
        }
    }

    struct TestCertFactory;

    impl P2pIdentityCertFactory for TestCertFactory {
        fn create(&self, _cert: &EncodedP2pIdentityCert) -> P2pResult<P2pIdentityCertRef> {
            Err(p2p_err!(P2pErrorCode::NotSupport, "test cert factory"))
        }
    }

    struct ClosableReverseTunnel {
        tunnel_id: TunnelId,
        candidate_id: TunnelCandidateId,
        form: TunnelForm,
        is_reverse: bool,
        local_id: P2pId,
        remote_id: P2pId,
        state: Mutex<TunnelState>,
        close_count: AtomicUsize,
    }

    impl ClosableReverseTunnel {
        fn new(
            tunnel_id: TunnelId,
            candidate_id: TunnelCandidateId,
            local_id: P2pId,
            remote_id: P2pId,
        ) -> Arc<Self> {
            Self::new_with_form(
                tunnel_id,
                candidate_id,
                TunnelForm::Passive,
                true,
                local_id,
                remote_id,
            )
        }

        fn new_with_form(
            tunnel_id: TunnelId,
            candidate_id: TunnelCandidateId,
            form: TunnelForm,
            is_reverse: bool,
            local_id: P2pId,
            remote_id: P2pId,
        ) -> Arc<Self> {
            Arc::new(Self {
                tunnel_id,
                candidate_id,
                form,
                is_reverse,
                local_id,
                remote_id,
                state: Mutex::new(TunnelState::Connected),
                close_count: AtomicUsize::new(0),
            })
        }

        fn close_count(&self) -> usize {
            self.close_count.load(Ordering::SeqCst)
        }
    }

    #[async_trait::async_trait]
    impl crate::networks::Tunnel for ClosableReverseTunnel {
        fn tunnel_id(&self) -> TunnelId {
            self.tunnel_id
        }

        fn candidate_id(&self) -> TunnelCandidateId {
            self.candidate_id
        }

        fn form(&self) -> TunnelForm {
            self.form
        }

        fn is_reverse(&self) -> bool {
            self.is_reverse
        }

        fn protocol(&self) -> Protocol {
            Protocol::Tcp
        }

        fn local_id(&self) -> P2pId {
            self.local_id.clone()
        }

        fn remote_id(&self) -> P2pId {
            self.remote_id.clone()
        }

        fn local_ep(&self) -> Option<Endpoint> {
            None
        }

        fn remote_ep(&self) -> Option<Endpoint> {
            None
        }

        fn state(&self) -> TunnelState {
            *self.state.lock().unwrap()
        }

        fn is_closed(&self) -> bool {
            self.state() == TunnelState::Closed
        }

        fn close(&self) -> P2pResult<()> {
            self.close_count.fetch_add(1, Ordering::SeqCst);
            *self.state.lock().unwrap() = TunnelState::Closed;
            Ok(())
        }

        async fn listen_stream(
            &self,
            _vports: ListenVPortsRef,
            _callback: crate::networks::IncomingStreamCallback,
        ) -> P2pResult<()> {
            Ok(())
        }

        async fn listen_datagram(
            &self,
            _vports: ListenVPortsRef,
            _callback: crate::networks::IncomingDatagramCallback,
        ) -> P2pResult<()> {
            Ok(())
        }

        async fn open_stream(
            &self,
            _purpose: crate::networks::TunnelPurpose,
        ) -> P2pResult<(TunnelStreamRead, TunnelStreamWrite)> {
            Err(p2p_err!(P2pErrorCode::NotSupport, "test tunnel"))
        }

        async fn open_datagram(
            &self,
            _purpose: crate::networks::TunnelPurpose,
        ) -> P2pResult<TunnelDatagramWrite> {
            Err(p2p_err!(P2pErrorCode::NotSupport, "test tunnel"))
        }
    }

    fn test_manager(local_identity: P2pIdentityRef) -> TunnelManagerRef {
        Executor::init();
        TunnelManager::new(
            local_identity,
            None,
            crate::networks::NetManager::new(
                Vec::new(),
                crate::tls::DefaultTlsServerCertResolver::new(),
            )
            .unwrap(),
            None,
            Arc::new(TestCertFactory),
            None,
            crate::tunnel::DefaultP2pConnectionInfoCache::new(),
            Arc::new(TunnelIdGenerator::new()),
            Duration::from_millis(200),
            Duration::from_secs(30),
            PROXY_UPGRADE_INITIAL_INTERVAL,
        )
        .unwrap()
    }

    fn subscribe_tunnels(manager: &TunnelManagerRef) -> mpsc::Receiver<P2pResult<TunnelRef>> {
        let (tx, rx) = mpsc::channel(TEST_SUBSCRIPTION_CHANNEL_CAPACITY);
        manager.subscribe(Arc::new(move |result| {
            let tx = tx.clone();
            Box::pin(async move {
                let _ = tx.send(result).await;
            })
        }));
        rx
    }

    async fn recv_tunnel(rx: &mut mpsc::Receiver<P2pResult<TunnelRef>>) -> P2pResult<TunnelRef> {
        match rx.recv().await {
            Some(result) => result,
            None => Err(p2p_err!(
                P2pErrorCode::Interrupted,
                "tunnel subscription closed"
            )),
        }
    }

    #[async_trait::async_trait]
    impl crate::networks::Tunnel for SelectionTunnel {
        fn tunnel_id(&self) -> TunnelId {
            TunnelId::from(1)
        }

        fn candidate_id(&self) -> TunnelCandidateId {
            TunnelCandidateId::from(1)
        }

        fn form(&self) -> TunnelForm {
            self.form
        }

        fn is_reverse(&self) -> bool {
            false
        }

        fn protocol(&self) -> Protocol {
            Protocol::Tcp
        }

        fn local_id(&self) -> P2pId {
            P2pId::default()
        }

        fn remote_id(&self) -> P2pId {
            P2pId::default()
        }

        fn local_ep(&self) -> Option<Endpoint> {
            None
        }

        fn remote_ep(&self) -> Option<Endpoint> {
            None
        }

        fn state(&self) -> TunnelState {
            TunnelState::Connected
        }

        fn is_closed(&self) -> bool {
            false
        }

        fn close(&self) -> P2pResult<()> {
            Ok(())
        }

        async fn listen_stream(
            &self,
            _vports: ListenVPortsRef,
            _callback: crate::networks::IncomingStreamCallback,
        ) -> P2pResult<()> {
            Ok(())
        }

        async fn listen_datagram(
            &self,
            _vports: ListenVPortsRef,
            _callback: crate::networks::IncomingDatagramCallback,
        ) -> P2pResult<()> {
            Ok(())
        }

        async fn open_stream(
            &self,
            _purpose: crate::networks::TunnelPurpose,
        ) -> P2pResult<(TunnelStreamRead, TunnelStreamWrite)> {
            Err(p2p_err!(P2pErrorCode::NotSupport, "selection tunnel"))
        }

        async fn open_datagram(
            &self,
            _purpose: crate::networks::TunnelPurpose,
        ) -> P2pResult<TunnelDatagramWrite> {
            Err(p2p_err!(P2pErrorCode::NotSupport, "selection tunnel"))
        }
    }

    fn endpoint(protocol: Protocol, port: u16) -> Endpoint {
        endpoint_with_ip(protocol, [8, 8, 8, 8], port)
    }

    fn endpoint_with_ip(protocol: Protocol, ip: [u8; 4], port: u16) -> Endpoint {
        Endpoint::from((protocol, (ip, port).into()))
    }

    #[test]
    fn nat_udp_candidates_use_short_reverse_delay_by_default() {
        let ep = endpoint(Protocol::Quic, 22000);

        assert_eq!(
            TunnelManager::hedged_reverse_delay_for_endpoints(&[ep]),
            NAT_HEDGED_REVERSE_DELAY
        );
    }

    #[test]
    fn static_wan_candidates_use_same_reverse_delay() {
        let mut ep = endpoint(Protocol::Quic, 22001);
        ep.set_area(EndpointArea::Wan);

        assert_eq!(
            TunnelManager::hedged_reverse_delay_for_endpoints(&[ep]),
            HEDGED_REVERSE_DELAY
        );
    }

    #[test]
    fn endpoint_score_key_isolated_by_protocol() {
        let tcp = endpoint(Protocol::Tcp, 22002);
        let quic = endpoint(Protocol::Quic, 22002);

        assert_ne!(EndpointScoreKey::new(&tcp), EndpointScoreKey::new(&quic));
    }

    #[test]
    fn select_preferred_tunnel_entry_prefers_non_proxy_over_newer_proxy() {
        let non_proxy: TunnelRef = Arc::new(SelectionTunnel {
            form: TunnelForm::Active,
        });
        let proxy: TunnelRef = Arc::new(SelectionTunnel {
            form: TunnelForm::Proxy,
        });
        let entries = vec![
            TunnelEntry {
                tunnel: non_proxy,
                updated_at: Instant::now(),
                published: true,
            },
            TunnelEntry {
                tunnel: proxy,
                updated_at: Instant::now() + Duration::from_secs(1),
                published: true,
            },
        ];

        let selected = TunnelManager::select_preferred_tunnel_entry(entries.iter()).unwrap();

        assert_eq!(selected.tunnel.form(), TunnelForm::Active);
    }

    #[test]
    fn udp_punch_candidate_policy_requires_server_reflexive_quic_non_lan_ipv4_endpoint() {
        let mut server_reflexive_quic = endpoint(Protocol::Quic, 22003);
        server_reflexive_quic.set_area(EndpointArea::ServerReflexive);
        let tcp = endpoint(Protocol::Tcp, 22004);
        let mut public_wan_area_quic = endpoint(Protocol::Quic, 22005);
        public_wan_area_quic.set_area(EndpointArea::Wan);
        let mut mapped_quic = endpoint(Protocol::Quic, 22009);
        mapped_quic.set_area(EndpointArea::Mapped);
        let public_lan_area_quic = endpoint(Protocol::Quic, 22010);
        let private_quic = endpoint_with_ip(Protocol::Quic, [192, 168, 1, 10], 22006);
        let loopback_quic = endpoint_with_ip(Protocol::Quic, [127, 0, 0, 1], 22007);
        let mut zero_port_quic = endpoint(Protocol::Quic, 0);
        zero_port_quic.set_area(EndpointArea::ServerReflexive);
        let mut ipv6_quic = Endpoint::from((
            Protocol::Quic,
            "[2001:4860:4860::8888]:22008"
                .parse::<SocketAddr>()
                .unwrap(),
        ));
        ipv6_quic.set_area(EndpointArea::ServerReflexive);

        assert!(TunnelManager::udp_punch_enabled_for_candidate(
            &server_reflexive_quic
        ));
        assert!(!TunnelManager::udp_punch_enabled_for_candidate(
            &public_lan_area_quic
        ));
        assert!(!TunnelManager::udp_punch_enabled_for_candidate(
            &public_wan_area_quic
        ));
        assert!(!TunnelManager::udp_punch_enabled_for_candidate(
            &mapped_quic
        ));
        assert!(!TunnelManager::udp_punch_enabled_for_candidate(&tcp));
        assert!(!TunnelManager::udp_punch_enabled_for_candidate(
            &private_quic
        ));
        assert!(!TunnelManager::udp_punch_enabled_for_candidate(
            &loopback_quic
        ));
        assert!(!TunnelManager::udp_punch_enabled_for_candidate(
            &zero_port_quic
        ));
        assert!(!TunnelManager::udp_punch_enabled_for_candidate(&ipv6_quic));
    }

    #[test]
    fn quic_nat_candidate_enables_udp_punch_only_when_sn_is_available() {
        let mut server_reflexive_quic = endpoint(Protocol::Quic, 22006);
        server_reflexive_quic.set_area(EndpointArea::ServerReflexive);
        let lan_quic = endpoint(Protocol::Quic, 22007);
        let active = TunnelConnectIntent::active_logical(TunnelId::from(31));
        let reverse = TunnelConnectIntent::reverse_logical(TunnelId::from(32));

        let no_sn = TunnelManager::intent_for_candidate(&server_reflexive_quic, active, false);
        assert!(!no_sn.udp_punch_enabled);

        let non_server_reflexive = TunnelManager::intent_for_candidate(&lan_quic, active, true);
        assert!(!non_server_reflexive.udp_punch_enabled);

        let active = TunnelManager::intent_for_candidate(&server_reflexive_quic, active, true);
        assert_eq!(active.tunnel_id, TunnelId::from(31));
        assert!(!active.is_reverse);
        assert!(active.udp_punch_enabled);

        let reverse = TunnelManager::intent_for_candidate(&server_reflexive_quic, reverse, true);
        assert_eq!(reverse.tunnel_id, TunnelId::from(32));
        assert!(reverse.is_reverse);
        assert!(reverse.udp_punch_enabled);
    }

    #[tokio::test]
    async fn reverse_without_waiter_closes_without_publish() {
        let local = TestIdentity::new(1, "local-no-reverse-waiter");
        let remote = TestIdentity::new(2, "remote-no-reverse-waiter");
        let manager = test_manager(local.clone());
        let mut sub = subscribe_tunnels(&manager);
        let tunnel_id = TunnelId::from(51);
        let (notify, _waiter) = Notify::new();
        let registration =
            ReverseWaitRegistration::register(manager.as_ref(), remote.get_id(), tunnel_id, notify);
        drop(registration);

        let late = ClosableReverseTunnel::new(
            tunnel_id,
            TunnelCandidateId::from(5101),
            local.get_id(),
            remote.get_id(),
        );

        manager.on_incoming_tunnel(late.clone()).await.unwrap();

        assert_eq!(late.close_count(), 1);
        assert!(manager.get_tunnel(&remote.get_id()).is_none());
        assert!(
            runtime::timeout(Duration::from_millis(50), recv_tunnel(&mut sub))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn reverse_without_waiter_closes_fresh_tunnel_id_without_x509() {
        let local = TestIdentity::new(3, "local-no-waiter-reverse");
        let remote = TestIdentity::new(4, "remote-no-waiter-reverse");
        let manager = test_manager(local.clone());
        let mut sub = subscribe_tunnels(&manager);
        let fresh_tunnel_id = TunnelId::from(53);

        let fresh = ClosableReverseTunnel::new(
            fresh_tunnel_id,
            TunnelCandidateId::from(5301),
            local.get_id(),
            remote.get_id(),
        );
        let fresh_ref: TunnelRef = fresh.clone();

        manager.on_incoming_tunnel(fresh_ref.clone()).await.unwrap();

        assert_eq!(fresh.close_count(), 1);
        assert!(manager.get_tunnel(&remote.get_id()).is_none());
        assert!(
            runtime::timeout(Duration::from_millis(50), recv_tunnel(&mut sub))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn publish_registered_tunnel_skips_unregistered_candidate() {
        let local = TestIdentity::new(5, "local-publish-unregistered");
        let remote = TestIdentity::new(6, "remote-publish-unregistered");
        let manager = test_manager(local.clone());
        let mut sub = subscribe_tunnels(&manager);
        let tunnel = ClosableReverseTunnel::new_with_form(
            TunnelId::from(55),
            TunnelCandidateId::from(5501),
            TunnelForm::Active,
            false,
            local.get_id(),
            remote.get_id(),
        );
        let tunnel_ref: TunnelRef = tunnel.clone();

        assert!(manager.publish_registered_tunnel(&tunnel_ref).is_err());

        assert!(manager.get_tunnel(&remote.get_id()).is_none());
        assert!(
            runtime::timeout(Duration::from_millis(50), recv_tunnel(&mut sub))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn register_tunnel_duplicate_candidate_fails_and_closes_new_tunnel() {
        let local = TestIdentity::new(7, "local-duplicate-candidate");
        let remote = TestIdentity::new(8, "remote-duplicate-candidate");
        let manager = test_manager(local.clone());
        let tunnel_id = TunnelId::from(57);
        let candidate_id = TunnelCandidateId::from(5701);
        let first = ClosableReverseTunnel::new_with_form(
            tunnel_id,
            candidate_id,
            TunnelForm::Active,
            false,
            local.get_id(),
            remote.get_id(),
        );
        let second = ClosableReverseTunnel::new_with_form(
            tunnel_id,
            candidate_id,
            TunnelForm::Active,
            false,
            local.get_id(),
            remote.get_id(),
        );

        manager.register_tunnel(first.clone()).await.unwrap();
        let err = manager.register_tunnel(second.clone()).await.err().unwrap();

        assert_eq!(err.code(), P2pErrorCode::AlreadyExists);
        assert_eq!(first.close_count(), 0);
        assert_eq!(second.close_count(), 1);
    }

    #[test]
    fn proxy_upgrade_short_window_falls_back_to_capped_backoff() {
        let mut upgrade = ProxyUpgradeState::new(Instant::now(), PROXY_UPGRADE_INITIAL_INTERVAL);
        let mut intervals = Vec::new();
        for _ in 0..5 {
            intervals.push(advance_proxy_upgrade_retry(
                &mut upgrade,
                PROXY_UPGRADE_INITIAL_INTERVAL,
            ));
        }

        assert_eq!(
            &intervals[..4],
            &[
                PROXY_UPGRADE_SHORT_INTERVALS[1],
                PROXY_UPGRADE_SHORT_INTERVALS[2],
                PROXY_UPGRADE_SHORT_INTERVALS[3],
                PROXY_UPGRADE_INITIAL_INTERVAL,
            ]
        );
        assert_eq!(
            intervals[4],
            PROXY_UPGRADE_INITIAL_INTERVAL.saturating_mul(2)
        );
    }

    #[test]
    fn reverse_endpoint_collection_includes_listener_wan_and_mapped_candidates() {
        let local = endpoint(Protocol::Quic, 4433);
        let mut wan = endpoint(Protocol::Quic, 55000);
        wan.set_area(EndpointArea::Wan);

        let endpoints = TunnelManager::collect_reverse_endpoints_from_sources(
            vec![(
                Protocol::Quic,
                vec![TunnelListenerInfo {
                    local,
                    mapping_port: Some(7000),
                }],
            )],
            vec![wan],
        );
        let mut mapped = Endpoint::from((Protocol::Quic, wan.addr().ip(), 7000));
        mapped.set_area(EndpointArea::Mapped);

        assert!(endpoints.contains(&local));
        assert!(endpoints.contains(&wan));
        assert!(endpoints.contains(&mapped));
        let mapped_area = endpoints
            .iter()
            .find(|ep| ep.addr() == mapped.addr() && ep.protocol() == mapped.protocol())
            .map(|ep| ep.get_area());
        assert_eq!(mapped_area, Some(EndpointArea::Mapped));
    }
}

#[cfg(all(test, feature = "x509"))]
mod tests {
    use super::*;
    use crate::endpoint::{EndpointArea, Protocol};
    use crate::error::P2pError;
    use crate::executor::Executor;
    use crate::networks::{IncomingTunnelCallback, TunnelListenerInfo, TunnelNetwork};
    use crate::networks::{TcpTunnelListener, TcpTunnelNetwork, TcpTunnelRegistry, Tunnel};
    use crate::sn::protocol::v0::{SnCalled, TunnelType};
    use crate::tls::{DefaultTlsServerCertResolver, TlsServerCertResolver};
    use crate::tunnel::DefaultP2pConnectionInfoCache;
    use crate::types::{Sequence, TunnelCandidateId, TunnelIdGenerator};
    use crate::x509::{X509IdentityCertFactory, X509IdentityFactory, generate_rsa_x509_identity};
    use sfo_reuseport::{ServerRuntime, ServerRuntimeConfig};
    use std::sync::Once;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::mpsc;

    static TLS_INIT: Once = Once::new();
    static TEST_TUNNEL_ID_SEQ: AtomicUsize = AtomicUsize::new(1);
    const TEST_CHANNEL_CAPACITY: usize = 8;

    fn subscribe_tunnels(manager: &TunnelManagerRef) -> mpsc::Receiver<P2pResult<TunnelRef>> {
        let (tx, rx) = mpsc::channel(TEST_CHANNEL_CAPACITY);
        manager.subscribe(Arc::new(move |result| {
            let tx = tx.clone();
            Box::pin(async move {
                let _ = tx.send(result).await;
            })
        }));
        rx
    }

    async fn recv_tunnel(rx: &mut mpsc::Receiver<P2pResult<TunnelRef>>) -> P2pResult<TunnelRef> {
        match rx.recv().await {
            Some(result) => result,
            None => Err(p2p_err!(
                P2pErrorCode::Interrupted,
                "tunnel subscription closed"
            )),
        }
    }

    struct StaticDeviceFinder {
        devices: HashMap<P2pId, P2pIdentityCertRef>,
    }

    #[async_trait::async_trait]
    impl crate::networks::DeviceFinder for StaticDeviceFinder {
        async fn get_identity_cert(&self, device_id: &P2pId) -> P2pResult<P2pIdentityCertRef> {
            self.devices
                .get(device_id)
                .cloned()
                .ok_or_else(|| p2p_err!(P2pErrorCode::NotFound, "device not found"))
        }
    }

    fn init_tls_once() {
        TLS_INIT.call_once(|| {
            Executor::init();
            crate::tls::init_tls(Arc::new(X509IdentityFactory));
        });
    }

    fn new_identity(name: &str) -> P2pIdentityRef {
        Arc::new(generate_rsa_x509_identity(Some(name.to_owned())).unwrap())
    }

    fn next_test_tunnel_id() -> TunnelId {
        TunnelId::from(TEST_TUNNEL_ID_SEQ.fetch_add(1, Ordering::SeqCst) as u32)
    }

    fn loopback_tcp_ep() -> Endpoint {
        Endpoint::from((Protocol::Tcp, "127.0.0.1:0".parse().unwrap()))
    }

    fn ignore_incoming() -> IncomingTunnelCallback {
        Arc::new(|_| Box::pin(async {}))
    }

    fn ext_ep(protocol: u8, port: u16) -> Endpoint {
        Endpoint::from((Protocol::Ext(protocol), ([127, 0, 0, 1], port).into()))
    }

    fn static_wan_ext_ep(protocol: u8, port: u16) -> Endpoint {
        let mut ep = ext_ep(protocol, port);
        ep.set_area(EndpointArea::Wan);
        ep
    }

    fn new_test_manager(
        local_identity: P2pIdentityRef,
        devices: HashMap<P2pId, P2pIdentityCertRef>,
        pn_network: Option<TunnelNetworkRef>,
    ) -> TunnelManagerRef {
        new_test_manager_with_networks(local_identity, devices, pn_network, vec![])
    }

    fn new_test_manager_with_networks(
        local_identity: P2pIdentityRef,
        devices: HashMap<P2pId, P2pIdentityCertRef>,
        pn_network: Option<TunnelNetworkRef>,
        networks: Vec<crate::networks::TunnelNetworkRef>,
    ) -> TunnelManagerRef {
        TunnelManager::new(
            local_identity,
            Some(Arc::new(StaticDeviceFinder { devices })),
            crate::networks::NetManager::new(networks, DefaultTlsServerCertResolver::new())
                .unwrap(),
            None,
            Arc::new(X509IdentityCertFactory),
            pn_network,
            DefaultP2pConnectionInfoCache::new(),
            Arc::new(TunnelIdGenerator::new()),
            Duration::from_secs(1),
            Duration::from_secs(30),
            PROXY_UPGRADE_INITIAL_INTERVAL,
        )
        .unwrap()
    }

    fn make_sn_called(
        remote_identity: &P2pIdentityRef,
        tunnel_id: TunnelId,
        reverse_endpoint_array: Vec<Endpoint>,
    ) -> SnCalled {
        SnCalled {
            seq: Sequence::from(1),
            sn_peer_id: P2pId::default(),
            to_peer_id: P2pId::default(),
            reverse_endpoint_array,
            active_pn_list: vec![],
            peer_info: remote_identity
                .get_identity_cert()
                .unwrap()
                .get_encoded_cert()
                .unwrap(),
            tunnel_id,
            call_send_time: 0,
            call_type: TunnelType::Stream,
            payload: vec![],
        }
    }

    #[derive(Clone)]
    struct MockDialBehavior {
        delay: Duration,
        result: Result<(), P2pErrorCode>,
    }

    struct MockDialNetwork {
        protocol: Protocol,
        local_id: P2pId,
        behaviors: Mutex<HashMap<Endpoint, MockDialBehavior>>,
        started_at: Instant,
        start_offsets: Mutex<HashMap<Endpoint, Duration>>,
        intents: Mutex<HashMap<Endpoint, TunnelConnectIntent>>,
        call_count: AtomicUsize,
    }

    impl MockDialNetwork {
        fn new(
            protocol: Protocol,
            local_id: P2pId,
            behaviors: HashMap<Endpoint, MockDialBehavior>,
        ) -> Arc<Self> {
            Arc::new(Self {
                protocol,
                local_id,
                behaviors: Mutex::new(behaviors),
                started_at: Instant::now(),
                start_offsets: Mutex::new(HashMap::new()),
                intents: Mutex::new(HashMap::new()),
                call_count: AtomicUsize::new(0),
            })
        }

        fn start_offset(&self, endpoint: &Endpoint) -> Option<Duration> {
            self.start_offsets.lock().unwrap().get(endpoint).copied()
        }

        fn call_count(&self) -> usize {
            self.call_count.load(Ordering::SeqCst)
        }

        fn intent_for(&self, endpoint: &Endpoint) -> Option<TunnelConnectIntent> {
            self.intents.lock().unwrap().get(endpoint).copied()
        }
    }

    struct MockProxyNetwork {
        local_id: P2pId,
        result: Result<(), P2pErrorCode>,
    }

    impl MockProxyNetwork {
        fn new(local_id: P2pId, result: Result<(), P2pErrorCode>) -> Arc<Self> {
            Arc::new(Self { local_id, result })
        }
    }

    #[async_trait::async_trait]
    impl TunnelNetwork for MockProxyNetwork {
        fn protocol(&self) -> Protocol {
            Protocol::Ext(255)
        }

        fn is_udp(&self) -> bool {
            false
        }

        async fn listen(
            &self,
            _local: &Endpoint,
            _out: Option<Endpoint>,
            _mapping_port: Option<u16>,
            _on_incoming_tunnel: IncomingTunnelCallback,
        ) -> P2pResult<()> {
            Err(p2p_err!(
                P2pErrorCode::NotSupport,
                "mock proxy listen not supported"
            ))
        }

        async fn close_all_listener(&self) -> P2pResult<()> {
            Ok(())
        }

        fn listener_infos(&self) -> Vec<TunnelListenerInfo> {
            vec![]
        }

        async fn create_tunnel_with_intent(
            &self,
            _local_identity: &P2pIdentityRef,
            _remote: &Endpoint,
            remote_id: &P2pId,
            _remote_name: Option<String>,
            intent: TunnelConnectIntent,
        ) -> P2pResult<TunnelRef> {
            match self.result {
                Ok(()) => Ok(Arc::new(MockTunnel {
                    tunnel_id: intent.tunnel_id,
                    candidate_id: intent.candidate_id,
                    form: TunnelForm::Proxy,
                    local_id: self.local_id.clone(),
                    remote_id: remote_id.clone(),
                    state: TunnelState::Connected,
                    is_reverse: false,
                })),
                Err(code) => Err(P2pError::new(
                    code,
                    format!("mock proxy connect failed for {remote_id}"),
                )),
            }
        }

        async fn create_tunnel_with_local_ep_and_intent(
            &self,
            local_identity: &P2pIdentityRef,
            local_ep: &Endpoint,
            remote: &Endpoint,
            remote_id: &P2pId,
            remote_name: Option<String>,
            intent: TunnelConnectIntent,
        ) -> P2pResult<TunnelRef> {
            self.create_tunnel_with_intent(local_identity, remote, remote_id, remote_name, intent)
                .await
        }
    }

    #[async_trait::async_trait]
    impl TunnelNetwork for MockDialNetwork {
        fn protocol(&self) -> Protocol {
            self.protocol
        }

        fn is_udp(&self) -> bool {
            false
        }

        async fn listen(
            &self,
            _local: &Endpoint,
            _out: Option<Endpoint>,
            _mapping_port: Option<u16>,
            _on_incoming_tunnel: IncomingTunnelCallback,
        ) -> P2pResult<()> {
            Err(p2p_err!(
                P2pErrorCode::NotSupport,
                "mock listen not supported"
            ))
        }

        async fn close_all_listener(&self) -> P2pResult<()> {
            Ok(())
        }

        fn listener_infos(&self) -> Vec<TunnelListenerInfo> {
            vec![]
        }

        async fn create_tunnel_with_intent(
            &self,
            _local_identity: &P2pIdentityRef,
            remote: &Endpoint,
            remote_id: &P2pId,
            _remote_name: Option<String>,
            intent: TunnelConnectIntent,
        ) -> P2pResult<TunnelRef> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            self.start_offsets
                .lock()
                .unwrap()
                .insert(*remote, self.started_at.elapsed());
            self.intents.lock().unwrap().insert(*remote, intent);

            let behavior = self
                .behaviors
                .lock()
                .unwrap()
                .get(remote)
                .cloned()
                .ok_or_else(|| {
                    p2p_err!(P2pErrorCode::NotFound, "missing mock endpoint {remote}")
                })?;
            runtime::sleep(behavior.delay).await;

            match behavior.result {
                Ok(()) => Ok(Arc::new(MockTunnel {
                    tunnel_id: intent.tunnel_id,
                    candidate_id: intent.candidate_id,
                    form: TunnelForm::Active,
                    local_id: self.local_id.clone(),
                    remote_id: remote_id.clone(),
                    state: TunnelState::Connected,
                    is_reverse: intent.is_reverse,
                })),
                Err(code) => Err(P2pError::new(
                    code,
                    format!("mock connect failed for {remote}"),
                )),
            }
        }

        async fn create_tunnel_with_local_ep_and_intent(
            &self,
            local_identity: &P2pIdentityRef,
            _local_ep: &Endpoint,
            remote: &Endpoint,
            remote_id: &P2pId,
            remote_name: Option<String>,
            intent: TunnelConnectIntent,
        ) -> P2pResult<TunnelRef> {
            self.create_tunnel_with_intent(local_identity, remote, remote_id, remote_name, intent)
                .await
        }
    }

    struct MockTunnel {
        tunnel_id: TunnelId,
        candidate_id: TunnelCandidateId,
        form: TunnelForm,
        local_id: P2pId,
        remote_id: P2pId,
        state: TunnelState,
        is_reverse: bool,
    }

    #[async_trait::async_trait]
    impl crate::networks::Tunnel for MockTunnel {
        fn tunnel_id(&self) -> TunnelId {
            self.tunnel_id
        }

        fn candidate_id(&self) -> TunnelCandidateId {
            self.candidate_id
        }

        fn form(&self) -> TunnelForm {
            self.form
        }

        fn is_reverse(&self) -> bool {
            self.is_reverse
        }

        fn protocol(&self) -> Protocol {
            Protocol::Tcp
        }

        fn local_id(&self) -> P2pId {
            self.local_id.clone()
        }

        fn remote_id(&self) -> P2pId {
            self.remote_id.clone()
        }

        fn local_ep(&self) -> Option<Endpoint> {
            Some(loopback_tcp_ep())
        }

        fn remote_ep(&self) -> Option<Endpoint> {
            Some(loopback_tcp_ep())
        }

        fn state(&self) -> TunnelState {
            self.state
        }

        fn is_closed(&self) -> bool {
            self.state == TunnelState::Closed
        }

        fn close(&self) -> P2pResult<()> {
            Ok(())
        }

        async fn listen_stream(
            &self,
            _vports: crate::networks::ListenVPortsRef,
            _callback: crate::networks::IncomingStreamCallback,
        ) -> P2pResult<()> {
            Ok(())
        }

        async fn listen_datagram(
            &self,
            _vports: crate::networks::ListenVPortsRef,
            _callback: crate::networks::IncomingDatagramCallback,
        ) -> P2pResult<()> {
            Ok(())
        }

        async fn open_stream(
            &self,
            _purpose: crate::networks::TunnelPurpose,
        ) -> P2pResult<(TunnelStreamRead, TunnelStreamWrite)> {
            Err(p2p_err!(P2pErrorCode::NotSupport, "mock tunnel"))
        }

        async fn open_datagram(
            &self,
            _purpose: crate::networks::TunnelPurpose,
        ) -> P2pResult<TunnelDatagramWrite> {
            Err(p2p_err!(P2pErrorCode::NotSupport, "mock tunnel"))
        }
    }

    struct TrackableTunnel {
        tunnel_id: TunnelId,
        candidate_id: TunnelCandidateId,
        form: TunnelForm,
        is_reverse: bool,
        protocol: Protocol,
        local_id: P2pId,
        remote_id: P2pId,
        local_ep: Option<Endpoint>,
        remote_ep: Option<Endpoint>,
        state: Mutex<TunnelState>,
        close_count: AtomicUsize,
    }

    impl TrackableTunnel {
        fn new(
            form: TunnelForm,
            local_id: P2pId,
            remote_id: P2pId,
            state: TunnelState,
        ) -> Arc<Self> {
            let tunnel_id = next_test_tunnel_id();
            Self::new_with_ids(
                tunnel_id,
                TunnelCandidateId::from(tunnel_id.value()),
                form,
                false,
                local_id,
                remote_id,
                state,
            )
        }

        fn new_with_ids(
            tunnel_id: TunnelId,
            candidate_id: TunnelCandidateId,
            form: TunnelForm,
            is_reverse: bool,
            local_id: P2pId,
            remote_id: P2pId,
            state: TunnelState,
        ) -> Arc<Self> {
            Arc::new(Self {
                tunnel_id,
                candidate_id,
                form,
                is_reverse,
                protocol: Protocol::Tcp,
                local_id,
                remote_id,
                local_ep: Some(loopback_tcp_ep()),
                remote_ep: Some(loopback_tcp_ep()),
                state: Mutex::new(state),
                close_count: AtomicUsize::new(0),
            })
        }

        fn close_count(&self) -> usize {
            self.close_count.load(Ordering::SeqCst)
        }
    }

    #[async_trait::async_trait]
    impl crate::networks::Tunnel for TrackableTunnel {
        fn tunnel_id(&self) -> TunnelId {
            self.tunnel_id
        }

        fn candidate_id(&self) -> TunnelCandidateId {
            self.candidate_id
        }

        fn form(&self) -> TunnelForm {
            self.form
        }

        fn is_reverse(&self) -> bool {
            self.is_reverse
        }

        fn protocol(&self) -> Protocol {
            self.protocol
        }

        fn local_id(&self) -> P2pId {
            self.local_id.clone()
        }

        fn remote_id(&self) -> P2pId {
            self.remote_id.clone()
        }

        fn local_ep(&self) -> Option<Endpoint> {
            self.local_ep
        }

        fn remote_ep(&self) -> Option<Endpoint> {
            self.remote_ep
        }

        fn state(&self) -> TunnelState {
            *self.state.lock().unwrap()
        }

        fn is_closed(&self) -> bool {
            self.state() == TunnelState::Closed
        }

        fn close(&self) -> P2pResult<()> {
            self.close_count.fetch_add(1, Ordering::SeqCst);
            *self.state.lock().unwrap() = TunnelState::Closed;
            Ok(())
        }

        async fn listen_stream(
            &self,
            _vports: crate::networks::ListenVPortsRef,
            _callback: crate::networks::IncomingStreamCallback,
        ) -> P2pResult<()> {
            Ok(())
        }

        async fn listen_datagram(
            &self,
            _vports: crate::networks::ListenVPortsRef,
            _callback: crate::networks::IncomingDatagramCallback,
        ) -> P2pResult<()> {
            Ok(())
        }

        async fn open_stream(
            &self,
            _purpose: crate::networks::TunnelPurpose,
        ) -> P2pResult<(TunnelStreamRead, TunnelStreamWrite)> {
            Err(p2p_err!(P2pErrorCode::NotSupport, "trackable tunnel"))
        }

        async fn open_datagram(
            &self,
            _purpose: crate::networks::TunnelPurpose,
        ) -> P2pResult<TunnelDatagramWrite> {
            Err(p2p_err!(P2pErrorCode::NotSupport, "trackable tunnel"))
        }
    }

    #[tokio::test]
    async fn incoming_tunnel_is_broadcast_to_subscription() {
        init_tls_once();

        let caller_identity = new_identity("caller");
        let callee_identity = new_identity("callee");
        let caller_cert = caller_identity.get_identity_cert().unwrap();

        let callee_net_manager = crate::networks::NetManager::new(
            vec![Arc::new(TcpTunnelNetwork::new(
                DefaultTlsServerCertResolver::new(),
                Arc::new(X509IdentityCertFactory),
                Duration::from_secs(3),
                Duration::from_millis(200),
                Duration::from_secs(5),
                ServerRuntime::start(ServerRuntimeConfig::default()).unwrap(),
            ))],
            DefaultTlsServerCertResolver::new(),
        )
        .unwrap();

        let callee_manager = TunnelManager::new(
            callee_identity.clone(),
            Some(Arc::new(StaticDeviceFinder {
                devices: HashMap::from([(caller_identity.get_id(), caller_cert.clone())]),
            })),
            callee_net_manager,
            None,
            Arc::new(X509IdentityCertFactory),
            None,
            DefaultP2pConnectionInfoCache::new(),
            Arc::new(TunnelIdGenerator::new()),
            Duration::from_secs(3),
            Duration::from_secs(30),
            PROXY_UPGRADE_INITIAL_INTERVAL,
        )
        .unwrap();
        let mut callee_sub = subscribe_tunnels(&callee_manager);

        let caller_resolver = DefaultTlsServerCertResolver::new();
        caller_resolver
            .add_server_identity(caller_identity.clone())
            .await
            .unwrap();
        let caller_network = Arc::new(TcpTunnelNetwork::new(
            caller_resolver,
            Arc::new(X509IdentityCertFactory),
            Duration::from_secs(3),
            Duration::from_millis(200),
            Duration::from_secs(5),
            ServerRuntime::start(ServerRuntimeConfig::default()).unwrap(),
        ));
        caller_network
            .listen(&loopback_tcp_ep(), None, None, ignore_incoming())
            .await
            .unwrap();

        let server_resolver = DefaultTlsServerCertResolver::new();
        server_resolver
            .add_server_identity(callee_identity.clone())
            .await
            .unwrap();
        let (server_incoming_tx, mut server_incoming_rx) = mpsc::channel(TEST_CHANNEL_CAPACITY);
        let server_incoming: IncomingTunnelCallback = Arc::new(move |result| {
            let server_incoming_tx = server_incoming_tx.clone();
            Box::pin(async move {
                let _ = server_incoming_tx.send(result).await;
            })
        });
        let server_listener = TcpTunnelListener::new(
            server_resolver,
            Arc::new(X509IdentityCertFactory),
            TcpTunnelRegistry::new(),
            Duration::from_secs(3),
            Duration::from_millis(200),
            Duration::from_secs(5),
            ServerRuntime::start(ServerRuntimeConfig::default()).unwrap(),
            TEST_CHANNEL_CAPACITY,
            server_incoming,
            TEST_CHANNEL_CAPACITY,
        );
        server_listener
            .start(loopback_tcp_ep(), None, None, false)
            .await
            .unwrap();

        let _opened = caller_network
            .create_tunnel(
                &caller_identity,
                &server_listener.bound_local(),
                &callee_identity.get_id(),
                Some(callee_identity.get_name()),
            )
            .await
            .unwrap();

        let accepted = server_incoming_rx.recv().await.unwrap().unwrap();
        let accepted = callee_manager.register_tunnel(accepted).await.unwrap();
        callee_manager.publish_registered_tunnel(&accepted).unwrap();

        let subscribed = recv_tunnel(&mut callee_sub).await.unwrap();
        assert_eq!(subscribed.remote_id(), caller_identity.get_id());
        assert!(
            callee_manager
                .tunnels
                .read()
                .unwrap()
                .contains_key(&caller_identity.get_id())
        );
    }

    #[tokio::test]
    async fn incoming_tunnel_notifies_reverse_waiter() {
        init_tls_once();

        let local_identity = new_identity("local-reverse");
        let remote_identity = new_identity("remote-reverse");
        let manager = new_test_manager(local_identity.clone(), HashMap::new(), None);
        let mut sub = subscribe_tunnels(&manager);
        let tunnel_id = TunnelId::from(42);
        let tunnel: TunnelRef = Arc::new(MockTunnel {
            tunnel_id,
            candidate_id: TunnelCandidateId::from(1),
            form: TunnelForm::Active,
            local_id: local_identity.get_id(),
            remote_id: remote_identity.get_id(),
            state: TunnelState::Connected,
            is_reverse: true,
        });

        let (notify, waiter) = Notify::new();
        manager.add_reverse_waiter(remote_identity.get_id(), tunnel_id, notify);

        manager.on_incoming_tunnel(tunnel.clone()).await.unwrap();

        let notified = runtime::timeout(Duration::from_secs(1), waiter)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(notified.remote_id(), remote_identity.get_id());
        assert!(manager.get_tunnel(&remote_identity.get_id()).is_none());
        assert!(
            !manager
                .tunnels
                .read()
                .unwrap()
                .contains_key(&remote_identity.get_id())
        );
        assert!(
            runtime::timeout(Duration::from_millis(100), recv_tunnel(&mut sub))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn non_reverse_incoming_tunnel_does_not_notify_reverse_waiter() {
        init_tls_once();

        let local_identity = new_identity("local-non-reverse");
        let remote_identity = new_identity("remote-non-reverse");
        let manager = new_test_manager(local_identity.clone(), HashMap::new(), None);
        let tunnel_id = TunnelId::from(43);
        let tunnel: TunnelRef = Arc::new(MockTunnel {
            tunnel_id,
            candidate_id: TunnelCandidateId::from(2),
            form: TunnelForm::Active,
            local_id: local_identity.get_id(),
            remote_id: remote_identity.get_id(),
            state: TunnelState::Connected,
            is_reverse: false,
        });

        let (notify, waiter) = Notify::new();
        manager.add_reverse_waiter(remote_identity.get_id(), tunnel_id, notify);

        manager.on_incoming_tunnel(tunnel).await.unwrap();

        assert!(
            runtime::timeout(Duration::from_millis(100), waiter)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn reverse_wait_registration_drop_cleans_up_waiter() {
        init_tls_once();

        let local_identity = new_identity("local-reverse-wait-drop");
        let remote_identity = new_identity("remote-reverse-wait-drop");
        let manager = new_test_manager(local_identity, HashMap::new(), None);
        let tunnel_id = TunnelId::from(44);
        let (notify, _waiter) = Notify::new();

        let registration = ReverseWaitRegistration::register(
            manager.as_ref(),
            remote_identity.get_id(),
            tunnel_id,
            notify,
        );
        assert!(manager.has_reverse_waiter(&remote_identity.get_id(), &tunnel_id));

        drop(registration);

        assert!(!manager.has_reverse_waiter(&remote_identity.get_id(), &tunnel_id));
    }

    #[tokio::test]
    async fn late_reverse_tunnel_after_waiter_timeout_is_closed_not_published() {
        init_tls_once();

        let local_identity = new_identity("local-late-reverse-close");
        let remote_identity = new_identity("remote-late-reverse-close");
        let manager = new_test_manager(local_identity.clone(), HashMap::new(), None);
        let mut sub = subscribe_tunnels(&manager);
        let tunnel_id = TunnelId::from(45);
        let (notify, _waiter) = Notify::new();
        let registration = ReverseWaitRegistration::register(
            manager.as_ref(),
            remote_identity.get_id(),
            tunnel_id,
            notify,
        );
        drop(registration);

        let late_reverse = TrackableTunnel::new_with_ids(
            tunnel_id,
            TunnelCandidateId::from(4501),
            TunnelForm::Passive,
            true,
            local_identity.get_id(),
            remote_identity.get_id(),
            TunnelState::Connected,
        );

        manager
            .on_incoming_tunnel(late_reverse.clone())
            .await
            .unwrap();

        assert_eq!(late_reverse.close_count(), 1);
        assert!(late_reverse.is_closed());
        assert!(manager.get_tunnel(&remote_identity.get_id()).is_none());
        assert!(
            !manager
                .tunnels
                .read()
                .unwrap()
                .contains_key(&remote_identity.get_id())
        );
        assert!(
            runtime::timeout(Duration::from_millis(100), recv_tunnel(&mut sub))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn reverse_without_waiter_closes_new_tunnel_id() {
        init_tls_once();

        let local_identity = new_identity("local-no-waiter-reverse-new-id");
        let remote_identity = new_identity("remote-no-waiter-reverse-new-id");
        let manager = new_test_manager(local_identity.clone(), HashMap::new(), None);
        let mut sub = subscribe_tunnels(&manager);
        let fresh_tunnel_id = TunnelId::from(47);

        let fresh_reverse = TrackableTunnel::new_with_ids(
            fresh_tunnel_id,
            TunnelCandidateId::from(4701),
            TunnelForm::Passive,
            true,
            local_identity.get_id(),
            remote_identity.get_id(),
            TunnelState::Connected,
        );
        let fresh_ref: TunnelRef = fresh_reverse.clone();

        manager.on_incoming_tunnel(fresh_ref.clone()).await.unwrap();

        assert_eq!(fresh_reverse.close_count(), 1);
        assert!(manager.get_tunnel(&remote_identity.get_id()).is_none());
        assert!(
            runtime::timeout(Duration::from_millis(100), recv_tunnel(&mut sub))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn get_tunnel_removes_unavailable_entry() {
        init_tls_once();

        let local_identity = new_identity("local-get-tunnel");
        let remote_identity = new_identity("remote-get-tunnel");
        let manager = new_test_manager(local_identity.clone(), HashMap::new(), None);
        let tunnel = TrackableTunnel::new(
            TunnelForm::Active,
            local_identity.get_id(),
            remote_identity.get_id(),
            TunnelState::Closed,
        );
        manager.tunnels.write().unwrap().insert(
            remote_identity.get_id(),
            vec![TunnelEntry {
                tunnel,
                updated_at: Instant::now(),
                published: true,
            }],
        );

        assert!(manager.get_tunnel(&remote_identity.get_id()).is_none());
        assert!(
            !manager
                .tunnels
                .read()
                .unwrap()
                .contains_key(&remote_identity.get_id())
        );
    }

    #[tokio::test]
    async fn register_tunnel_keeps_multiple_live_tunnels_for_same_logical_remote() {
        init_tls_once();

        let local_identity = new_identity("local-register-existing");
        let remote_identity = new_identity("remote-register-existing");
        let manager = new_test_manager(local_identity.clone(), HashMap::new(), None);
        let logical_tunnel_id = TunnelId::from(501);
        let existing = TrackableTunnel::new_with_ids(
            logical_tunnel_id,
            TunnelCandidateId::from(11),
            TunnelForm::Active,
            false,
            local_identity.get_id(),
            remote_identity.get_id(),
            TunnelState::Connected,
        );
        let existing_ref: TunnelRef = existing.clone();
        manager
            .register_tunnel_and_publish(existing_ref.clone())
            .await
            .unwrap();

        let replacement = TrackableTunnel::new_with_ids(
            logical_tunnel_id,
            TunnelCandidateId::from(12),
            TunnelForm::Active,
            false,
            local_identity.get_id(),
            remote_identity.get_id(),
            TunnelState::Connected,
        );
        let replacement_ref: TunnelRef = replacement.clone();

        let returned = manager
            .register_tunnel_and_publish(replacement_ref.clone())
            .await
            .unwrap();

        assert!(Arc::ptr_eq(&returned, &replacement_ref));
        assert!(Arc::ptr_eq(
            &manager.get_tunnel(&remote_identity.get_id()).unwrap(),
            &replacement_ref,
        ));
        assert_eq!(
            manager
                .tunnels
                .read()
                .unwrap()
                .get(&remote_identity.get_id())
                .map(|entries| entries.len()),
            Some(2)
        );
        assert_eq!(existing.close_count(), 0);
        assert_eq!(replacement.close_count(), 0);
    }

    #[tokio::test]
    async fn register_tunnel_publishes_all_non_reverse_candidates() {
        init_tls_once();

        let local_identity = new_identity("local-publish-all");
        let remote_identity = new_identity("remote-publish-all");
        let manager = new_test_manager(local_identity.clone(), HashMap::new(), None);
        let mut sub = subscribe_tunnels(&manager);
        let logical_tunnel_id = TunnelId::from(601);

        let first = TrackableTunnel::new_with_ids(
            logical_tunnel_id,
            TunnelCandidateId::from(21),
            TunnelForm::Active,
            false,
            local_identity.get_id(),
            remote_identity.get_id(),
            TunnelState::Connected,
        );
        let second = TrackableTunnel::new_with_ids(
            logical_tunnel_id,
            TunnelCandidateId::from(22),
            TunnelForm::Passive,
            false,
            local_identity.get_id(),
            remote_identity.get_id(),
            TunnelState::Connected,
        );

        manager
            .register_tunnel_and_publish(first.clone())
            .await
            .unwrap();
        manager
            .register_tunnel_and_publish(second.clone())
            .await
            .unwrap();

        let first_published = recv_tunnel(&mut sub).await.unwrap();
        let second_published = recv_tunnel(&mut sub).await.unwrap();
        assert_eq!(first_published.tunnel_id(), logical_tunnel_id);
        assert_eq!(second_published.tunnel_id(), logical_tunnel_id);
        assert_ne!(
            first_published.candidate_id(),
            second_published.candidate_id()
        );
        assert_eq!(
            manager
                .tunnels
                .read()
                .unwrap()
                .get(&remote_identity.get_id())
                .map(|entries| entries.iter().filter(|entry| entry.published).count()),
            Some(2)
        );
    }

    #[tokio::test]
    async fn register_local_reverse_tunnel_without_waiter_is_published() {
        init_tls_once();

        let local_identity = new_identity("local-publish-reverse-without-waiter");
        let remote_identity = new_identity("remote-publish-reverse-without-waiter");
        let manager = new_test_manager(local_identity.clone(), HashMap::new(), None);
        let mut sub = subscribe_tunnels(&manager);
        let logical_tunnel_id = TunnelId::from(651);
        let reverse = TrackableTunnel::new_with_ids(
            logical_tunnel_id,
            TunnelCandidateId::from(23),
            TunnelForm::Active,
            true,
            local_identity.get_id(),
            remote_identity.get_id(),
            TunnelState::Connected,
        );

        let reverse_ref: TunnelRef = reverse.clone();
        let returned = manager
            .register_tunnel_and_publish(reverse_ref.clone())
            .await
            .unwrap();
        let published = recv_tunnel(&mut sub).await.unwrap();

        assert!(Arc::ptr_eq(&returned, &reverse_ref));
        assert!(Arc::ptr_eq(&published, &reverse_ref));
    }

    #[tokio::test]
    async fn reverse_incoming_tunnel_closes_without_waiter_after_waiter_consumed() {
        init_tls_once();

        let local_identity = new_identity("local-reverse-after-waiter");
        let remote_identity = new_identity("remote-reverse-after-waiter");
        let manager = new_test_manager(local_identity.clone(), HashMap::new(), None);
        let mut sub = subscribe_tunnels(&manager);
        let logical_tunnel_id = TunnelId::from(701);

        let first = TrackableTunnel::new_with_ids(
            logical_tunnel_id,
            TunnelCandidateId::from(31),
            TunnelForm::Passive,
            true,
            local_identity.get_id(),
            remote_identity.get_id(),
            TunnelState::Connected,
        );
        let second = TrackableTunnel::new_with_ids(
            logical_tunnel_id,
            TunnelCandidateId::from(32),
            TunnelForm::Passive,
            true,
            local_identity.get_id(),
            remote_identity.get_id(),
            TunnelState::Connected,
        );

        let (notify, waiter) = Notify::new();
        manager.add_reverse_waiter(remote_identity.get_id(), logical_tunnel_id, notify);

        manager.on_incoming_tunnel(first.clone()).await.unwrap();
        let notified = runtime::timeout(Duration::from_secs(1), waiter)
            .await
            .unwrap()
            .unwrap();
        let first_ref: TunnelRef = first.clone();
        assert!(Arc::ptr_eq(&notified, &first_ref));
        assert!(
            runtime::timeout(Duration::from_millis(100), recv_tunnel(&mut sub))
                .await
                .is_err()
        );

        manager.on_incoming_tunnel(second.clone()).await.unwrap();
        assert_eq!(first.close_count(), 0);
        assert_eq!(second.close_count(), 1);
        assert!(
            runtime::timeout(Duration::from_millis(100), recv_tunnel(&mut sub))
                .await
                .is_err()
        );
        assert_eq!(
            manager
                .tunnels
                .read()
                .unwrap()
                .get(&remote_identity.get_id())
                .map(|entries| entries.len()),
            None
        );
    }

    #[tokio::test]
    async fn reverse_tunnel_is_published_after_waiter_completion() {
        init_tls_once();

        let local_identity = new_identity("local-reverse-waiter-publish");
        let remote_identity = new_identity("remote-reverse-waiter-publish");
        let manager = new_test_manager(local_identity.clone(), HashMap::new(), None);
        let mut sub = subscribe_tunnels(&manager);
        let logical_tunnel_id = TunnelId::from(711);
        let reverse = TrackableTunnel::new_with_ids(
            logical_tunnel_id,
            TunnelCandidateId::from(33),
            TunnelForm::Passive,
            true,
            local_identity.get_id(),
            remote_identity.get_id(),
            TunnelState::Connected,
        );

        let (notify, waiter) = Notify::new();
        manager.add_reverse_waiter(remote_identity.get_id(), logical_tunnel_id, notify);

        manager.on_incoming_tunnel(reverse.clone()).await.unwrap();
        let notified = runtime::timeout(Duration::from_secs(1), waiter)
            .await
            .unwrap()
            .unwrap();
        let notified = manager.register_tunnel(notified).await.unwrap();
        manager.publish_registered_tunnel(&notified).unwrap();

        let published = recv_tunnel(&mut sub).await.unwrap();
        let reverse_ref: TunnelRef = reverse.clone();
        assert!(Arc::ptr_eq(&published, &reverse_ref));
    }

    #[tokio::test]
    async fn get_tunnel_prefers_latest_live_tunnel_for_same_remote() {
        init_tls_once();

        let local_identity = new_identity("local-latest-live");
        let remote_identity = new_identity("remote-latest-live");
        let manager = new_test_manager(local_identity.clone(), HashMap::new(), None);

        let first = TrackableTunnel::new(
            TunnelForm::Active,
            local_identity.get_id(),
            remote_identity.get_id(),
            TunnelState::Connected,
        );
        let second = TrackableTunnel::new(
            TunnelForm::Passive,
            local_identity.get_id(),
            remote_identity.get_id(),
            TunnelState::Connected,
        );

        manager.register_tunnel(first.clone()).await.unwrap();
        runtime::sleep(Duration::from_millis(1)).await;
        manager.register_tunnel(second.clone()).await.unwrap();

        let selected = manager.get_tunnel(&remote_identity.get_id()).unwrap();
        let second_ref: TunnelRef = second.clone();
        assert!(Arc::ptr_eq(&selected, &second_ref));
        assert_eq!(
            manager
                .tunnels
                .read()
                .unwrap()
                .get(&remote_identity.get_id())
                .map(|entries| entries.len()),
            Some(2)
        );
    }

    #[tokio::test]
    async fn get_tunnel_prefers_non_proxy_over_newer_proxy() {
        init_tls_once();

        let local_identity = new_identity("local-non-proxy-preferred");
        let remote_identity = new_identity("remote-non-proxy-preferred");
        let manager = new_test_manager(local_identity.clone(), HashMap::new(), None);

        let direct = TrackableTunnel::new(
            TunnelForm::Active,
            local_identity.get_id(),
            remote_identity.get_id(),
            TunnelState::Connected,
        );
        let proxy = TrackableTunnel::new(
            TunnelForm::Proxy,
            local_identity.get_id(),
            remote_identity.get_id(),
            TunnelState::Connected,
        );

        manager
            .register_tunnel_and_publish(direct.clone())
            .await
            .unwrap();
        runtime::sleep(Duration::from_millis(1)).await;
        manager
            .register_tunnel_and_publish(proxy.clone())
            .await
            .unwrap();

        let selected = manager.get_tunnel(&remote_identity.get_id()).unwrap();
        let direct_ref: TunnelRef = direct.clone();
        assert!(Arc::ptr_eq(&selected, &direct_ref));
        assert_eq!(selected.form(), TunnelForm::Active);
        assert_eq!(
            manager
                .tunnels
                .read()
                .unwrap()
                .get(&remote_identity.get_id())
                .map(|entries| entries.len()),
            Some(2)
        );
    }

    #[tokio::test]
    async fn get_tunnel_retracks_proxy_upgrade_after_non_proxy_drops() {
        init_tls_once();

        let local_identity = new_identity("local-proxy-retrack");
        let remote_identity = new_identity("remote-proxy-retrack");
        let manager = new_test_manager(local_identity.clone(), HashMap::new(), None);

        let proxy = TrackableTunnel::new(
            TunnelForm::Proxy,
            local_identity.get_id(),
            remote_identity.get_id(),
            TunnelState::Connected,
        );
        let direct = TrackableTunnel::new(
            TunnelForm::Active,
            local_identity.get_id(),
            remote_identity.get_id(),
            TunnelState::Connected,
        );

        manager
            .register_tunnel_and_publish(proxy.clone())
            .await
            .unwrap();
        assert!(
            manager
                .state
                .lock()
                .unwrap()
                .proxy_upgrade_states
                .contains_key(&remote_identity.get_id())
        );

        manager
            .register_tunnel_and_publish(direct.clone())
            .await
            .unwrap();
        assert!(
            !manager
                .state
                .lock()
                .unwrap()
                .proxy_upgrade_states
                .contains_key(&remote_identity.get_id())
        );

        *direct.state.lock().unwrap() = TunnelState::Closed;

        let selected = manager.get_tunnel(&remote_identity.get_id()).unwrap();
        let proxy_ref: TunnelRef = proxy.clone();
        assert!(Arc::ptr_eq(&selected, &proxy_ref));
        assert_eq!(selected.form(), TunnelForm::Proxy);

        let state = manager.state.lock().unwrap();
        let upgrade = state
            .proxy_upgrade_states
            .get(&remote_identity.get_id())
            .copied()
            .unwrap();
        assert_eq!(upgrade.retry_interval, PROXY_UPGRADE_SHORT_INTERVALS[0]);
        assert_eq!(upgrade.short_retry_index, 0);
        assert!(!upgrade.in_progress);
    }

    #[tokio::test]
    async fn get_tunnel_prefers_published_candidate_over_hidden_reverse() {
        init_tls_once();

        let local_identity = new_identity("local-published-preferred");
        let remote_identity = new_identity("remote-published-preferred");
        let manager = new_test_manager(local_identity.clone(), HashMap::new(), None);
        let logical_tunnel_id = TunnelId::from(801);

        let reverse_hidden = TrackableTunnel::new_with_ids(
            logical_tunnel_id,
            TunnelCandidateId::from(41),
            TunnelForm::Passive,
            true,
            local_identity.get_id(),
            remote_identity.get_id(),
            TunnelState::Connected,
        );
        let published = TrackableTunnel::new_with_ids(
            logical_tunnel_id,
            TunnelCandidateId::from(42),
            TunnelForm::Active,
            false,
            local_identity.get_id(),
            remote_identity.get_id(),
            TunnelState::Connected,
        );

        manager
            .register_tunnel(reverse_hidden.clone())
            .await
            .unwrap();
        manager
            .register_tunnel_and_publish(published.clone())
            .await
            .unwrap();

        let selected = manager.get_tunnel(&remote_identity.get_id()).unwrap();
        let published_ref: TunnelRef = published.clone();
        assert!(Arc::ptr_eq(&selected, &published_ref));
    }

    #[tokio::test]
    async fn register_tunnel_proxy_without_replacement_is_registered_and_published() {
        init_tls_once();

        let local_identity = new_identity("local-register-proxy");
        let remote_identity = new_identity("remote-register-proxy");
        let manager = new_test_manager(local_identity.clone(), HashMap::new(), None);
        let mut sub = subscribe_tunnels(&manager);
        let proxy = TrackableTunnel::new(
            TunnelForm::Proxy,
            local_identity.get_id(),
            remote_identity.get_id(),
            TunnelState::Connected,
        );
        let proxy_ref: TunnelRef = proxy.clone();

        let returned = manager
            .register_tunnel_and_publish(proxy_ref)
            .await
            .unwrap();
        let published = recv_tunnel(&mut sub).await.unwrap();

        assert_eq!(returned.form(), TunnelForm::Proxy);
        assert_eq!(published.remote_id(), remote_identity.get_id());
        assert!(manager.get_tunnel(&remote_identity.get_id()).is_some());
        assert_eq!(
            manager
                .tunnels
                .read()
                .unwrap()
                .get(&remote_identity.get_id())
                .map(|entries| entries.len()),
            Some(1)
        );
        assert_eq!(proxy.close_count(), 0);
    }

    #[tokio::test]
    async fn publish_registered_tunnel_is_idempotent() {
        init_tls_once();

        let local_identity = new_identity("local-publish-idempotent");
        let remote_identity = new_identity("remote-publish-idempotent");
        let manager = new_test_manager(local_identity.clone(), HashMap::new(), None);
        let mut sub = subscribe_tunnels(&manager);
        let tunnel = TrackableTunnel::new(
            TunnelForm::Active,
            local_identity.get_id(),
            remote_identity.get_id(),
            TunnelState::Connected,
        );
        let tunnel_ref: TunnelRef = tunnel.clone();

        manager.register_tunnel(tunnel_ref.clone()).await.unwrap();
        manager.publish_registered_tunnel(&tunnel_ref).unwrap();
        manager.publish_registered_tunnel(&tunnel_ref).unwrap();

        let published = recv_tunnel(&mut sub).await.unwrap();
        assert!(Arc::ptr_eq(&published, &tunnel_ref));
        assert!(
            runtime::timeout(Duration::from_millis(100), recv_tunnel(&mut sub))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn duplicate_published_candidate_registration_fails_without_republish() {
        init_tls_once();

        let local_identity = new_identity("local-duplicate-published");
        let remote_identity = new_identity("remote-duplicate-published");
        let manager = new_test_manager(local_identity.clone(), HashMap::new(), None);
        let mut sub = subscribe_tunnels(&manager);
        let tunnel_id = TunnelId::from(901);
        let candidate_id = TunnelCandidateId::from(1);
        let first = TrackableTunnel::new_with_ids(
            tunnel_id,
            candidate_id,
            TunnelForm::Proxy,
            false,
            local_identity.get_id(),
            remote_identity.get_id(),
            TunnelState::Connected,
        );
        let replacement = TrackableTunnel::new_with_ids(
            tunnel_id,
            candidate_id,
            TunnelForm::Proxy,
            false,
            local_identity.get_id(),
            remote_identity.get_id(),
            TunnelState::Connected,
        );

        manager
            .register_tunnel_and_publish(first.clone())
            .await
            .unwrap();
        let first_published = recv_tunnel(&mut sub).await.unwrap();
        let first_ref: TunnelRef = first.clone();
        assert!(Arc::ptr_eq(&first_published, &first_ref));

        let err = manager
            .register_tunnel_and_publish(replacement.clone())
            .await
            .err()
            .unwrap();

        assert_eq!(err.code(), P2pErrorCode::AlreadyExists);
        assert_eq!(first.close_count(), 0);
        assert_eq!(replacement.close_count(), 1);
        assert!(
            runtime::timeout(Duration::from_millis(100), recv_tunnel(&mut sub))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn sn_called_skips_reverse_dial_when_tunnel_id_exists() {
        init_tls_once();

        let local_identity = new_identity("local-sn-called-existing-id");
        let remote_identity = new_identity("remote-sn-called-existing-id");
        let remote_ep = ext_ep(11, 18011);
        let network = MockDialNetwork::new(
            Protocol::Ext(11),
            local_identity.get_id(),
            HashMap::from([(
                remote_ep,
                MockDialBehavior {
                    delay: Duration::from_millis(1),
                    result: Ok(()),
                },
            )]),
        );
        let manager = new_test_manager_with_networks(
            local_identity.clone(),
            HashMap::new(),
            None,
            vec![network.clone()],
        );
        let tunnel_id = TunnelId::from(901);
        let existing = TrackableTunnel::new_with_ids(
            tunnel_id,
            TunnelCandidateId::from(1),
            TunnelForm::Active,
            false,
            local_identity.get_id(),
            remote_identity.get_id(),
            TunnelState::Connected,
        );
        manager.register_tunnel_and_publish(existing).await.unwrap();

        manager
            .on_sn_called(make_sn_called(&remote_identity, tunnel_id, vec![remote_ep]))
            .await
            .unwrap();

        assert_eq!(network.call_count(), 0);
    }

    #[tokio::test]
    async fn sn_called_dials_reverse_when_existing_tunnel_has_different_id() {
        init_tls_once();

        let local_identity = new_identity("local-sn-called-new-id");
        let remote_identity = new_identity("remote-sn-called-new-id");
        let remote_ep = ext_ep(12, 18012);
        let network = MockDialNetwork::new(
            Protocol::Ext(12),
            local_identity.get_id(),
            HashMap::from([(
                remote_ep,
                MockDialBehavior {
                    delay: Duration::from_millis(1),
                    result: Ok(()),
                },
            )]),
        );
        let manager = new_test_manager_with_networks(
            local_identity.clone(),
            HashMap::new(),
            None,
            vec![network.clone()],
        );
        let existing = TrackableTunnel::new_with_ids(
            TunnelId::from(902),
            TunnelCandidateId::from(1),
            TunnelForm::Active,
            false,
            local_identity.get_id(),
            remote_identity.get_id(),
            TunnelState::Connected,
        );
        manager.register_tunnel_and_publish(existing).await.unwrap();

        let new_tunnel_id = TunnelId::from(903);
        manager
            .on_sn_called(make_sn_called(
                &remote_identity,
                new_tunnel_id,
                vec![remote_ep],
            ))
            .await
            .unwrap();

        assert_eq!(network.call_count(), 1);
        assert!(manager.has_available_tunnel_id(&remote_identity.get_id(), new_tunnel_id));
    }

    #[tokio::test]
    async fn sn_called_reverse_quic_candidate_keeps_udp_punch_disabled_without_sn_service() {
        init_tls_once();

        let local_identity = new_identity("local-sn-called-quic-punch");
        let remote_identity = new_identity("remote-sn-called-quic-punch");
        let remote_ep = Endpoint::from((Protocol::Quic, "127.0.0.1:18013".parse().unwrap()));
        let network = MockDialNetwork::new(
            Protocol::Quic,
            local_identity.get_id(),
            HashMap::from([(
                remote_ep,
                MockDialBehavior {
                    delay: Duration::from_millis(1),
                    result: Ok(()),
                },
            )]),
        );
        let manager = new_test_manager_with_networks(
            local_identity.clone(),
            HashMap::new(),
            None,
            vec![network.clone()],
        );

        let tunnel_id = TunnelId::from(904);
        manager
            .on_sn_called(make_sn_called(&remote_identity, tunnel_id, vec![remote_ep]))
            .await
            .unwrap();

        let intent = network.intent_for(&remote_ep).unwrap();
        assert_eq!(intent.tunnel_id, tunnel_id);
        assert!(intent.is_reverse);
        assert!(!intent.udp_punch_enabled);
    }

    #[tokio::test]
    async fn hedged_race_prefers_reverse_when_direct_is_slow() {
        init_tls_once();

        let (result, direct_won) = race_with_delay(
            async {
                runtime::sleep(Duration::from_millis(80)).await;
                Ok::<_, crate::error::P2pError>(1u8)
            },
            Duration::from_millis(10),
            async {
                runtime::sleep(Duration::from_millis(20)).await;
                Ok::<_, crate::error::P2pError>(2u8)
            },
        )
        .await;

        assert_eq!(result.unwrap(), 2);
        assert!(!direct_won);
    }

    #[tokio::test]
    async fn hedged_race_falls_back_to_reverse_after_direct_error() {
        init_tls_once();

        let (result, direct_won) = race_with_delay(
            async { Err::<u8, _>(p2p_err!(P2pErrorCode::ConnectFailed, "direct failed")) },
            Duration::from_millis(10),
            async {
                runtime::sleep(Duration::from_millis(5)).await;
                Ok::<_, crate::error::P2pError>(9u8)
            },
        )
        .await;

        assert_eq!(result.unwrap(), 9);
        assert!(!direct_won);
    }

    #[tokio::test]
    async fn open_direct_path_prefers_fastest_successful_endpoint() {
        init_tls_once();

        let local_identity = new_identity("local-fastest");
        let remote_identity = new_identity("remote-fastest");
        let slow_ep = Endpoint::from((Protocol::Ext(1), "127.0.0.1:11001".parse().unwrap()));
        let fast_ep = Endpoint::from((Protocol::Ext(1), "127.0.0.1:11002".parse().unwrap()));
        let network = MockDialNetwork::new(
            Protocol::Ext(1),
            local_identity.get_id(),
            HashMap::from([
                (
                    slow_ep,
                    MockDialBehavior {
                        delay: Duration::from_millis(80),
                        result: Ok(()),
                    },
                ),
                (
                    fast_ep,
                    MockDialBehavior {
                        delay: Duration::from_millis(10),
                        result: Ok(()),
                    },
                ),
            ]),
        );
        let manager = new_test_manager_with_networks(
            local_identity,
            HashMap::new(),
            None,
            vec![network.clone()],
        );

        let started = Instant::now();
        let tunnel = manager
            .open_direct_tunnel(vec![slow_ep, fast_ep], &remote_identity.get_id())
            .await
            .unwrap();

        assert_eq!(tunnel.remote_id(), remote_identity.get_id());
        assert!(started.elapsed() < Duration::from_millis(50));
        assert_eq!(network.call_count(), 2);
        let slow_start = network.start_offset(&slow_ep).unwrap();
        let fast_start = network.start_offset(&fast_ep).unwrap();
        let start_gap = slow_start.abs_diff(fast_start);
        assert!(start_gap < Duration::from_millis(20));

        let info = manager
            .conn_info_cache
            .get(&remote_identity.get_id())
            .await
            .unwrap();
        assert_eq!(info.remote_ep, fast_ep);
        assert_eq!(info.direct, ConnectDirection::Direct);
    }

    #[tokio::test]
    async fn open_tunnel_from_id_uses_device_finder_once_for_live_tunnel() {
        init_tls_once();

        let local_identity = new_identity("local-open-from-id");
        let remote_identity = new_identity("remote-open-from-id");
        let remote_ep = Endpoint::from((Protocol::Ext(3), "127.0.0.1:13001".parse().unwrap()));
        let remote_cert = remote_identity
            .get_identity_cert()
            .unwrap()
            .update_endpoints(vec![remote_ep]);
        let network = MockDialNetwork::new(
            Protocol::Ext(3),
            local_identity.get_id(),
            HashMap::from([(
                remote_ep,
                MockDialBehavior {
                    delay: Duration::from_millis(10),
                    result: Ok(()),
                },
            )]),
        );
        let manager = new_test_manager_with_networks(
            local_identity,
            HashMap::from([(remote_identity.get_id(), remote_cert)]),
            None,
            vec![network.clone()],
        );

        let first = manager
            .open_tunnel_from_id(&remote_identity.get_id())
            .await
            .unwrap();
        let second = manager
            .open_tunnel_from_id(&remote_identity.get_id())
            .await
            .unwrap();

        assert_eq!(first.remote_id(), remote_identity.get_id());
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(network.call_count(), 1);
    }

    #[tokio::test]
    async fn open_direct_tunnel_rejects_empty_endpoints() {
        init_tls_once();

        let manager = new_test_manager(new_identity("local-empty-eps"), HashMap::new(), None);

        let err = manager
            .open_direct_tunnel(vec![], &P2pId::default())
            .await
            .err()
            .unwrap();

        assert_eq!(err.code(), P2pErrorCode::InvalidParam);
    }

    #[tokio::test]
    async fn open_direct_path_falls_back_when_faster_endpoint_fails() {
        init_tls_once();

        let local_identity = new_identity("local-fallback");
        let remote_identity = new_identity("remote-fallback");
        let fail_ep = Endpoint::from((Protocol::Ext(2), "127.0.0.1:12001".parse().unwrap()));
        let success_ep = Endpoint::from((Protocol::Ext(2), "127.0.0.1:12002".parse().unwrap()));
        let network = MockDialNetwork::new(
            Protocol::Ext(2),
            local_identity.get_id(),
            HashMap::from([
                (
                    fail_ep,
                    MockDialBehavior {
                        delay: Duration::from_millis(10),
                        result: Err(P2pErrorCode::ConnectFailed),
                    },
                ),
                (
                    success_ep,
                    MockDialBehavior {
                        delay: Duration::from_millis(80),
                        result: Ok(()),
                    },
                ),
            ]),
        );
        let manager = new_test_manager_with_networks(
            local_identity,
            HashMap::new(),
            None,
            vec![network.clone()],
        );

        let started = Instant::now();
        let tunnel = manager
            .open_direct_tunnel(vec![fail_ep, success_ep], &remote_identity.get_id())
            .await
            .unwrap();

        assert_eq!(tunnel.remote_id(), remote_identity.get_id());
        assert!(started.elapsed() < Duration::from_millis(120));
        assert_eq!(network.call_count(), 2);

        let info = manager
            .conn_info_cache
            .get(&remote_identity.get_id())
            .await
            .unwrap();
        assert_eq!(info.remote_ep, success_ep);
        assert_eq!(info.direct, ConnectDirection::Direct);
    }

    #[tokio::test]
    async fn cleanup_closed_tunnels_removes_dead_entries() {
        init_tls_once();

        let local_identity = new_identity("local-cleanup");
        let live_identity = new_identity("remote-live-cleanup");
        let dead_identity = new_identity("remote-dead-cleanup");
        let manager = new_test_manager(local_identity.clone(), HashMap::new(), None);
        let live_tunnel = TrackableTunnel::new(
            TunnelForm::Active,
            local_identity.get_id(),
            live_identity.get_id(),
            TunnelState::Connected,
        );
        let dead_tunnel = TrackableTunnel::new(
            TunnelForm::Active,
            local_identity.get_id(),
            dead_identity.get_id(),
            TunnelState::Closed,
        );
        manager.tunnels.write().unwrap().insert(
            live_identity.get_id(),
            vec![TunnelEntry {
                tunnel: live_tunnel.clone(),
                updated_at: Instant::now(),
                published: true,
            }],
        );
        manager.tunnels.write().unwrap().insert(
            dead_identity.get_id(),
            vec![TunnelEntry {
                tunnel: dead_tunnel.clone(),
                updated_at: Instant::now(),
                published: true,
            }],
        );

        manager.cleanup_closed_tunnels(Duration::from_secs(0)).await;

        assert!(manager.get_tunnel(&live_identity.get_id()).is_some());
        assert!(manager.get_tunnel(&dead_identity.get_id()).is_none());
        assert_eq!(live_tunnel.close_count(), 0);
        assert_eq!(dead_tunnel.close_count(), 1);
    }

    #[tokio::test]
    async fn cleanup_closed_tunnels_retracks_proxy_upgrade_when_proxy_remains() {
        init_tls_once();

        let local_identity = new_identity("local-cleanup-proxy-retrack");
        let remote_identity = new_identity("remote-cleanup-proxy-retrack");
        let manager = new_test_manager(local_identity.clone(), HashMap::new(), None);
        let proxy = TrackableTunnel::new(
            TunnelForm::Proxy,
            local_identity.get_id(),
            remote_identity.get_id(),
            TunnelState::Connected,
        );
        let direct = TrackableTunnel::new(
            TunnelForm::Active,
            local_identity.get_id(),
            remote_identity.get_id(),
            TunnelState::Closed,
        );

        manager
            .register_tunnel_and_publish(proxy.clone())
            .await
            .unwrap();
        manager
            .register_tunnel_and_publish(direct.clone())
            .await
            .unwrap();
        assert!(
            !manager
                .state
                .lock()
                .unwrap()
                .proxy_upgrade_states
                .contains_key(&remote_identity.get_id())
        );

        manager.cleanup_closed_tunnels(Duration::from_secs(0)).await;

        let selected = manager.get_tunnel(&remote_identity.get_id()).unwrap();
        let proxy_ref: TunnelRef = proxy.clone();
        assert!(Arc::ptr_eq(&selected, &proxy_ref));
        assert_eq!(direct.close_count(), 1);
        assert!(
            manager
                .state
                .lock()
                .unwrap()
                .proxy_upgrade_states
                .contains_key(&remote_identity.get_id())
        );
    }

    #[tokio::test]
    async fn drop_closes_held_tunnels() {
        init_tls_once();

        let local_identity = new_identity("local-drop-close");
        let remote_identity = new_identity("remote-drop-close");
        let manager = new_test_manager(local_identity.clone(), HashMap::new(), None);
        let tunnel = TrackableTunnel::new(
            TunnelForm::Active,
            local_identity.get_id(),
            remote_identity.get_id(),
            TunnelState::Connected,
        );

        manager
            .register_tunnel_and_publish(tunnel.clone())
            .await
            .unwrap();

        drop(manager);

        assert_eq!(tunnel.close_count(), 1);
        assert!(tunnel.is_closed());
    }

    #[tokio::test]
    async fn open_direct_tunnel_single_endpoint_success() {
        init_tls_once();

        let local_identity = new_identity("local-direct-single");
        let remote_identity = new_identity("remote-direct-single");
        let remote_ep = Endpoint::from((Protocol::Ext(4), "127.0.0.1:14001".parse().unwrap()));
        let network = MockDialNetwork::new(
            Protocol::Ext(4),
            local_identity.get_id(),
            HashMap::from([(
                remote_ep,
                MockDialBehavior {
                    delay: Duration::from_millis(10),
                    result: Ok(()),
                },
            )]),
        );
        let manager = new_test_manager_with_networks(
            local_identity,
            HashMap::new(),
            None,
            vec![network.clone()],
        );

        let tunnel = manager
            .open_direct_tunnel(vec![remote_ep], &remote_identity.get_id())
            .await
            .unwrap();

        assert_eq!(tunnel.remote_id(), remote_identity.get_id());

        let info = manager
            .conn_info_cache
            .get(&remote_identity.get_id())
            .await
            .unwrap();
        assert_eq!(info.direct, ConnectDirection::Direct);
    }

    #[tokio::test]
    async fn open_tunnel_fails_with_no_network_and_no_proxy() {
        init_tls_once();

        let local_identity = new_identity("local-no-nets");
        let remote_identity = new_identity("remote-no-nets");

        let manager = new_test_manager(local_identity, HashMap::new(), None);

        let err = manager
            .open_proxy_path(
                &remote_identity.get_id(),
                None,
                TunnelConnectIntent::active_logical(TunnelId::from(8888)),
            )
            .await
            .err()
            .unwrap();

        assert_eq!(err.code(), P2pErrorCode::NotSupport);
    }

    #[tokio::test]
    async fn open_proxy_path_without_pn_network_fails() {
        init_tls_once();

        let local_identity = new_identity("local-no-pn");
        let remote_identity = new_identity("remote-no-pn");
        let manager = new_test_manager(local_identity.clone(), HashMap::new(), None);

        let err = manager
            .open_proxy_path(
                &remote_identity.get_id(),
                None,
                TunnelConnectIntent::active_logical(TunnelId::from(999)),
            )
            .await
            .err()
            .unwrap();

        assert_eq!(err.code(), P2pErrorCode::NotSupport);
    }

    #[tokio::test]
    async fn conn_info_cache_direct_preferred_on_reconnect() {
        init_tls_once();

        let local_identity = new_identity("local-cache-reuse");
        let remote_identity = new_identity("remote-cache-reuse");
        let cached_ep = Endpoint::from((Protocol::Ext(8), "127.0.0.1:18001".parse().unwrap()));
        let new_ep = Endpoint::from((Protocol::Ext(8), "127.0.0.1:18002".parse().unwrap()));
        let network = MockDialNetwork::new(
            Protocol::Ext(8),
            local_identity.get_id(),
            HashMap::from([
                (
                    cached_ep,
                    MockDialBehavior {
                        delay: Duration::from_millis(10),
                        result: Ok(()),
                    },
                ),
                (
                    new_ep,
                    MockDialBehavior {
                        delay: Duration::from_millis(10),
                        result: Ok(()),
                    },
                ),
            ]),
        );
        let manager = new_test_manager_with_networks(
            local_identity,
            HashMap::new(),
            None,
            vec![network.clone()],
        );

        manager
            .conn_info_cache
            .add(
                remote_identity.get_id(),
                P2pConnectionInfo {
                    direct: ConnectDirection::Direct,
                    local_ep: loopback_tcp_ep(),
                    remote_ep: cached_ep,
                },
            )
            .await;

        let tunnel = manager
            .open_direct_tunnel(vec![cached_ep, new_ep], &remote_identity.get_id())
            .await
            .unwrap();

        assert_eq!(tunnel.remote_id(), remote_identity.get_id());

        let info = manager
            .conn_info_cache
            .get(&remote_identity.get_id())
            .await
            .unwrap();
        assert_eq!(info.remote_ep, cached_ep);
    }

    #[tokio::test]
    async fn reverse_path_fails_without_sn_service_when_cached() {
        init_tls_once();

        let local_identity = new_identity("local-no-sn");
        let remote_identity = new_identity("remote-no-sn");
        let remote_ep = Endpoint::from((Protocol::Ext(9), "127.0.0.1:19001".parse().unwrap()));
        let manager = new_test_manager_with_networks(local_identity, HashMap::new(), None, vec![]);

        manager
            .conn_info_cache
            .add(
                remote_identity.get_id(),
                P2pConnectionInfo {
                    direct: ConnectDirection::Reverse,
                    local_ep: loopback_tcp_ep(),
                    remote_ep,
                },
            )
            .await;

        let err = manager
            .open_direct_tunnel(vec![remote_ep], &remote_identity.get_id())
            .await
            .err()
            .unwrap();

        assert_eq!(err.code(), P2pErrorCode::NotSupport);
    }

    #[tokio::test]
    async fn register_proxy_tunnel_tracks_upgrade_attempt() {
        init_tls_once();

        let local_identity = new_identity("local-proxy-track");
        let remote_identity = new_identity("remote-proxy-track");
        let manager = new_test_manager(local_identity.clone(), HashMap::new(), None);
        let proxy = TrackableTunnel::new(
            TunnelForm::Proxy,
            local_identity.get_id(),
            remote_identity.get_id(),
            TunnelState::Connected,
        );

        manager.register_tunnel_and_publish(proxy).await.unwrap();

        let state = manager.state.lock().unwrap();
        let upgrade = state
            .proxy_upgrade_states
            .get(&remote_identity.get_id())
            .copied()
            .unwrap();
        assert_eq!(upgrade.retry_interval, PROXY_UPGRADE_SHORT_INTERVALS[0]);
        assert_eq!(upgrade.short_retry_index, 0);
        assert!(!upgrade.in_progress);
        assert!(upgrade.next_attempt_at > Instant::now());
    }

    #[tokio::test]
    async fn track_proxy_upgrade_uses_configured_initial_interval() {
        init_tls_once();

        let local_identity = new_identity("local-proxy-track-custom");
        let remote_identity = new_identity("remote-proxy-track-custom");
        let configured_interval = Duration::from_secs(17);
        let manager = TunnelManager::new(
            local_identity.clone(),
            Some(Arc::new(StaticDeviceFinder {
                devices: HashMap::new(),
            })),
            crate::networks::NetManager::new(vec![], DefaultTlsServerCertResolver::new()).unwrap(),
            None,
            Arc::new(X509IdentityCertFactory),
            None,
            DefaultP2pConnectionInfoCache::new(),
            Arc::new(TunnelIdGenerator::new()),
            Duration::from_secs(1),
            Duration::from_secs(30),
            configured_interval,
        )
        .unwrap();
        let proxy = TrackableTunnel::new(
            TunnelForm::Proxy,
            local_identity.get_id(),
            remote_identity.get_id(),
            TunnelState::Connected,
        );

        manager.register_tunnel_and_publish(proxy).await.unwrap();

        let state = manager.state.lock().unwrap();
        let upgrade = state
            .proxy_upgrade_states
            .get(&remote_identity.get_id())
            .copied()
            .unwrap();
        assert_eq!(upgrade.retry_interval, PROXY_UPGRADE_SHORT_INTERVALS[0]);
        assert_eq!(upgrade.short_retry_index, 0);
        assert!(upgrade.next_attempt_at <= Instant::now() + PROXY_UPGRADE_SHORT_INTERVALS[0]);
    }

    #[tokio::test]
    async fn open_proxy_path_tracks_upgrade_attempt_for_stored_proxy() {
        init_tls_once();

        let local_identity = new_identity("local-open-proxy-track");
        let remote_identity = new_identity("remote-open-proxy-track");
        let proxy_network = MockProxyNetwork::new(local_identity.get_id(), Ok(()))
            as crate::networks::TunnelNetworkRef;
        let manager = new_test_manager(local_identity.clone(), HashMap::new(), Some(proxy_network));

        let proxy = manager
            .open_proxy_path(
                &remote_identity.get_id(),
                None,
                TunnelConnectIntent::active_logical(TunnelId::from(1201)),
            )
            .await
            .unwrap();

        assert_eq!(proxy.form(), TunnelForm::Proxy);
        assert_eq!(
            manager
                .get_tunnel(&remote_identity.get_id())
                .unwrap()
                .form(),
            TunnelForm::Proxy
        );
        assert!(
            manager
                .state
                .lock()
                .unwrap()
                .proxy_upgrade_states
                .contains_key(&remote_identity.get_id())
        );
    }

    #[tokio::test]
    async fn successful_proxy_upgrade_promotes_connection_to_direct() {
        init_tls_once();

        let local_identity = new_identity("local-proxy-upgrade-success");
        let remote_identity = new_identity("remote-proxy-upgrade-success");
        let remote_ep = Endpoint::from((Protocol::Ext(10), "127.0.0.1:20001".parse().unwrap()));
        let remote_cert = remote_identity
            .get_identity_cert()
            .unwrap()
            .update_endpoints(vec![remote_ep]);
        let network = MockDialNetwork::new(
            Protocol::Ext(10),
            local_identity.get_id(),
            HashMap::from([(
                remote_ep,
                MockDialBehavior {
                    delay: Duration::from_millis(10),
                    result: Ok(()),
                },
            )]),
        );
        let manager = new_test_manager_with_networks(
            local_identity.clone(),
            HashMap::from([(remote_identity.get_id(), remote_cert)]),
            None,
            vec![network.clone()],
        );
        let proxy = TrackableTunnel::new(
            TunnelForm::Proxy,
            local_identity.get_id(),
            remote_identity.get_id(),
            TunnelState::Connected,
        );

        manager
            .register_tunnel_and_publish(proxy.clone())
            .await
            .unwrap();
        manager
            .attempt_proxy_upgrade(remote_identity.get_id())
            .await;

        let info = manager
            .conn_info_cache
            .get(&remote_identity.get_id())
            .await
            .unwrap();
        assert_eq!(info.direct, ConnectDirection::Direct);
        assert_eq!(info.remote_ep, remote_ep);
        assert!(manager.get_tunnel(&remote_identity.get_id()).is_some());
        assert_eq!(proxy.close_count(), 1);
        assert_eq!(
            manager
                .tunnels
                .read()
                .unwrap()
                .get(&remote_identity.get_id())
                .map(|entries| entries.len()),
            Some(1)
        );
        assert_eq!(network.call_count(), 1);
        assert!(
            !manager
                .state
                .lock()
                .unwrap()
                .proxy_upgrade_states
                .contains_key(&remote_identity.get_id())
        );
    }

    #[tokio::test]
    async fn stored_proxy_upgrade_dials_direct_instead_of_reusing_proxy() {
        init_tls_once();

        let local_identity = new_identity("local-stored-proxy-upgrade");
        let remote_identity = new_identity("remote-stored-proxy-upgrade");
        let remote_ep = Endpoint::from((Protocol::Ext(11), "127.0.0.1:20011".parse().unwrap()));
        let remote_cert = remote_identity
            .get_identity_cert()
            .unwrap()
            .update_endpoints(vec![remote_ep]);
        let direct_network = MockDialNetwork::new(
            Protocol::Ext(11),
            local_identity.get_id(),
            HashMap::from([(
                remote_ep,
                MockDialBehavior {
                    delay: Duration::from_millis(10),
                    result: Ok(()),
                },
            )]),
        );
        let proxy_network = MockProxyNetwork::new(local_identity.get_id(), Ok(()))
            as crate::networks::TunnelNetworkRef;
        let manager = new_test_manager_with_networks(
            local_identity.clone(),
            HashMap::from([(remote_identity.get_id(), remote_cert)]),
            Some(proxy_network),
            vec![direct_network.clone()],
        );

        manager
            .open_proxy_path(
                &remote_identity.get_id(),
                None,
                TunnelConnectIntent::active_logical(TunnelId::from(1202)),
            )
            .await
            .unwrap();
        assert_eq!(
            manager
                .get_tunnel(&remote_identity.get_id())
                .unwrap()
                .form(),
            TunnelForm::Proxy
        );

        manager
            .attempt_proxy_upgrade(remote_identity.get_id())
            .await;

        let info = manager
            .conn_info_cache
            .get(&remote_identity.get_id())
            .await
            .unwrap();
        assert_eq!(info.direct, ConnectDirection::Direct);
        assert_eq!(info.remote_ep, remote_ep);
        assert_eq!(direct_network.call_count(), 1);
        assert_eq!(
            manager
                .get_tunnel(&remote_identity.get_id())
                .unwrap()
                .form(),
            TunnelForm::Active
        );
        assert_eq!(
            manager
                .tunnels
                .read()
                .unwrap()
                .get(&remote_identity.get_id())
                .map(|entries| entries.len()),
            Some(1)
        );
        assert!(
            !manager
                .state
                .lock()
                .unwrap()
                .proxy_upgrade_states
                .contains_key(&remote_identity.get_id())
        );
    }

    #[tokio::test]
    async fn failed_proxy_upgrade_uses_exponential_backoff_with_cap() {
        init_tls_once();

        let local_identity = new_identity("local-proxy-upgrade-fail");
        let remote_identity = new_identity("remote-proxy-upgrade-fail");
        let manager = new_test_manager(local_identity, HashMap::new(), None);

        manager.track_proxy_upgrade(&remote_identity.get_id());
        for _ in 0..12 {
            manager
                .attempt_proxy_upgrade(remote_identity.get_id())
                .await;
        }

        let state = manager.state.lock().unwrap();
        let upgrade = state
            .proxy_upgrade_states
            .get(&remote_identity.get_id())
            .copied()
            .unwrap();
        assert_eq!(upgrade.retry_interval, PROXY_UPGRADE_MAX_INTERVAL);
        assert!(!upgrade.in_progress);
        assert!(upgrade.next_attempt_at > Instant::now());
    }

    #[tokio::test]
    async fn nat_quic_candidates_use_short_reverse_delay() {
        init_tls_once();

        let nat_ep = ext_ep(12, 21001);

        assert_eq!(
            TunnelManager::hedged_reverse_delay_for_endpoints(&[nat_ep]),
            NAT_HEDGED_REVERSE_DELAY
        );
    }

    #[tokio::test]
    async fn direct_quic_nat_candidate_keeps_udp_punch_disabled_without_sn_service() {
        init_tls_once();

        let local_identity = new_identity("local-direct-quic-punch");
        let remote_identity = new_identity("remote-direct-quic-punch");
        let remote_ep = Endpoint::from((Protocol::Quic, "127.0.0.1:21021".parse().unwrap()));
        let network = MockDialNetwork::new(
            Protocol::Quic,
            local_identity.get_id(),
            HashMap::from([(
                remote_ep,
                MockDialBehavior {
                    delay: Duration::from_millis(1),
                    result: Ok(()),
                },
            )]),
        );
        let manager = new_test_manager_with_networks(
            local_identity,
            HashMap::new(),
            None,
            vec![network.clone()],
        );

        manager
            .open_direct_path(
                vec![remote_ep],
                &remote_identity.get_id(),
                None,
                TunnelConnectIntent::active_logical(TunnelId::from(1203)),
            )
            .await
            .unwrap();

        let intent = network.intent_for(&remote_ep).unwrap();
        assert!(!intent.is_reverse);
        assert!(!intent.udp_punch_enabled);
    }

    #[tokio::test]
    async fn static_wan_direct_uses_same_reverse_delay() {
        init_tls_once();

        let wan_ep = static_wan_ext_ep(13, 21011);

        assert_eq!(
            TunnelManager::hedged_reverse_delay_for_endpoints(&[wan_ep]),
            HEDGED_REVERSE_DELAY
        );
    }

    #[tokio::test]
    async fn endpoint_score_isolated_by_protocol() {
        init_tls_once();

        let local_identity = new_identity("local-score-protocol");
        let manager = new_test_manager(local_identity, HashMap::new(), None);
        let tcp_ep = Endpoint::from((Protocol::Tcp, "127.0.0.1:22001".parse().unwrap()));
        let quic_ep = Endpoint::from((Protocol::Quic, "127.0.0.1:22001".parse().unwrap()));

        manager.on_direct_connect_result(&tcp_ep, false);
        let preferred = manager
            .preferred_direct_endpoints(None, &[tcp_ep, quic_ep])
            .await;

        assert_eq!(preferred[0], quic_ep);
    }

    #[tokio::test]
    async fn proxy_upgrade_starts_with_short_nat_retry_window() {
        init_tls_once();

        let local_identity = new_identity("local-proxy-short-window");
        let remote_identity = new_identity("remote-proxy-short-window");
        let manager = new_test_manager(local_identity, HashMap::new(), None);

        manager.track_proxy_upgrade(&remote_identity.get_id());

        let state = manager.state.lock().unwrap();
        let upgrade = state
            .proxy_upgrade_states
            .get(&remote_identity.get_id())
            .copied()
            .unwrap();
        assert_eq!(upgrade.retry_interval, PROXY_UPGRADE_SHORT_INTERVALS[0]);
        assert!(upgrade.next_attempt_at <= Instant::now() + PROXY_UPGRADE_SHORT_INTERVALS[0]);
    }

    #[tokio::test]
    async fn proxy_upgrade_short_window_falls_back_to_capped_backoff() {
        init_tls_once();

        let local_identity = new_identity("local-proxy-short-to-backoff");
        let remote_identity = new_identity("remote-proxy-short-to-backoff");
        let manager = new_test_manager(local_identity, HashMap::new(), None);

        manager.track_proxy_upgrade(&remote_identity.get_id());
        let mut intervals = Vec::new();
        for _ in 0..5 {
            let interval = manager
                .on_proxy_upgrade_failed(&remote_identity.get_id())
                .unwrap();
            intervals.push(interval);
        }

        assert_eq!(
            &intervals[..4],
            &[
                PROXY_UPGRADE_SHORT_INTERVALS[1],
                PROXY_UPGRADE_SHORT_INTERVALS[2],
                PROXY_UPGRADE_SHORT_INTERVALS[3],
                PROXY_UPGRADE_INITIAL_INTERVAL,
            ]
        );
        assert_eq!(
            intervals[4],
            PROXY_UPGRADE_INITIAL_INTERVAL.saturating_mul(2)
        );
    }
}
