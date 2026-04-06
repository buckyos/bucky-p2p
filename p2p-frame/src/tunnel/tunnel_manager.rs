use crate::endpoint::Endpoint;
use crate::error::{P2pError, P2pErrorCode, P2pResult, into_p2p_err, p2p_err};
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
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

use futures::stream::{FuturesUnordered, StreamExt};

use super::{ConnectDirection, DeviceFinderRef, P2pConnectionInfo, P2pConnectionInfoCacheRef};
use crate::networks::{
    ListenVPortsRef, NetManagerRef, TunnelCommandResult, TunnelConnectIntent, TunnelDatagramRead,
    TunnelDatagramWrite, TunnelForm, TunnelNetworkRef, TunnelRef, TunnelState, TunnelStreamRead,
    TunnelStreamWrite,
};

const HEDGED_REVERSE_DELAY: Duration = Duration::from_millis(2000);

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

pub struct TunnelSubscription {
    rx: mpsc::UnboundedReceiver<P2pResult<TunnelRef>>,
}

impl TunnelSubscription {
    pub async fn accept_tunnel(&mut self) -> P2pResult<TunnelRef> {
        match self.rx.recv().await {
            Some(result) => result,
            None => Err(p2p_err!(
                P2pErrorCode::Interrupted,
                "tunnel subscription closed"
            )),
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

type ReverseWaitKey = (P2pId, TunnelId);

struct ManagerState {
    endpoint_scores: HashMap<Endpoint, EndpointScore>,
    pending_reverse_waiters: HashMap<ReverseWaitKey, Notify<P2pResult<TunnelRef>>>,
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
    tunnels: RwLock<HashMap<P2pId, TunnelEntries>>,
    state: Mutex<ManagerState>,
    subscriptions: Mutex<Vec<mpsc::UnboundedSender<P2pResult<TunnelRef>>>>,
    incoming_task: SpawnHandle<()>,
    proxy_incoming_task: Option<SpawnHandle<()>>,
    cleanup_task: SpawnHandle<()>,
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
    ) -> P2pResult<TunnelManagerRef> {
        let mut incoming_acceptor =
            net_manager.register_tunnel_acceptor(local_identity.get_id())?;
        let sn_service_for_listener = sn_service.clone();
        let pn_network_for_listener = pn_network.clone();
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
            tunnels: RwLock::new(HashMap::new()),
            state: Mutex::new(ManagerState {
                endpoint_scores: HashMap::new(),
                pending_reverse_waiters: HashMap::new(),
            }),
            subscriptions: Mutex::new(Vec::new()),
            incoming_task: Executor::spawn_with_handle({
                let weak = weak.clone();
                async move {
                    loop {
                        match incoming_acceptor.accept_tunnel().await {
                            Ok(tunnel) => {
                                let Some(manager) = weak.upgrade() else {
                                    break;
                                };
                                if let Err(err) = manager.on_incoming_tunnel(tunnel).await {
                                    log::warn!("register incoming tunnel failed: {:?}", err);
                                }
                            }
                            Err(err) => {
                                log::warn!("receive incoming tunnel failed: {:?}", err);
                                break;
                            }
                        }
                    }
                }
            })
            .unwrap(),
            proxy_incoming_task: pn_network_for_listener.as_ref().and_then(|pn_network| {
                let listeners = pn_network.listeners();
                if listeners.is_empty() {
                    log::warn!(
                        "proxy incoming disabled local_id={} protocol={:?} reason=no pn listener",
                        local_identity.get_id(),
                        pn_network.protocol()
                    );
                    return None;
                }
                log::debug!(
                    "proxy incoming enabled local_id={} protocol={:?} listener_count={}",
                    local_identity.get_id(),
                    pn_network.protocol(),
                    listeners.len()
                );
                let listener = listeners.into_iter().next()?;
                let weak = weak.clone();
                Some(
                    Executor::spawn_with_handle(async move {
                        loop {
                            match listener.accept_tunnel().await {
                                Ok(tunnel) => {
                                    let Some(manager) = weak.upgrade() else {
                                        break;
                                    };
                                    if let Err(err) =
                                        manager.register_tunnel(tunnel, false, true).await
                                    {
                                        log::warn!(
                                            "register incoming proxy tunnel failed: {:?}",
                                            err
                                        );
                                    }
                                }
                                Err(err) => {
                                    log::warn!("receive incoming proxy tunnel failed: {:?}", err);
                                    break;
                                }
                            }
                        }
                    })
                    .unwrap(),
                )
            }),
            cleanup_task: Executor::spawn_with_handle({
                let weak = weak.clone();
                async move {
                    loop {
                        runtime::sleep(Duration::from_secs(120)).await;
                        let Some(manager) = weak.upgrade() else {
                            break;
                        };
                        manager.cleanup_closed_tunnels(idle_timeout).await;
                    }
                }
            })
            .unwrap(),
        });
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

    pub fn subscribe(&self) -> TunnelSubscription {
        let (tx, rx) = mpsc::unbounded_channel();
        self.subscriptions.lock().unwrap().push(tx);
        TunnelSubscription { rx }
    }

    pub fn get_tunnel(&self, remote_id: &P2pId) -> Option<TunnelRef> {
        let mut tunnels = self.tunnels.write().unwrap();
        let entries = tunnels.get_mut(remote_id)?;
        entries.retain(|entry| is_tunnel_available(entry.tunnel.as_ref()));
        let selected = entries
            .iter()
            .filter(|entry| entry.published)
            .max_by_key(|entry| entry.updated_at)
            .or_else(|| entries.iter().max_by_key(|entry| entry.updated_at))
            .map(|entry| entry.tunnel.clone());
        if entries.is_empty() {
            tunnels.remove(remote_id);
        }
        selected
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
        let tunnel = self.register_tunnel(tunnel, false, false).await?;
        if !tunnel.is_reverse() {
            self.publish_registered_tunnel(&tunnel);
            log::debug!(
                "incoming tunnel remote={} tunnel_id={:?} ignore reverse waiter because reverse=false",
                remote_id,
                tunnel_id
            );
            return Ok(());
        }
        if let Some(waiter) = self.take_reverse_waiter(&remote_id, &tunnel_id) {
            log::debug!(
                "incoming tunnel remote={} tunnel_id={:?} matched reverse waiter",
                remote_id,
                tunnel_id
            );
            waiter.notify(Ok(tunnel));
        } else {
            self.publish_registered_tunnel(&tunnel);
            log::debug!(
                "incoming tunnel remote={} tunnel_id={:?} no reverse waiter, publish tunnel",
                remote_id,
                tunnel_id
            );
        }
        Ok(())
    }

    async fn open_known_tunnel(
        &self,
        remote_eps: Vec<Endpoint>,
        remote_id: &P2pId,
        remote_name: Option<String>,
    ) -> P2pResult<TunnelRef> {
        log::debug!(
            "open tunnel remote={} name={:?} eps_count={} eps={:?} has_sn={} has_pn={}",
            remote_id,
            remote_name,
            remote_eps.len(),
            remote_eps,
            self.sn_service.is_some(),
            self.pn_network.is_some()
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

        if let Some(tunnel) = self.get_tunnel(remote_id) {
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
            log::debug!(
                "open tunnel remote={} tunnel_id={:?} start hedged direct+reverse delay_ms={}",
                remote_id,
                logical_tunnel_id,
                HEDGED_REVERSE_DELAY.as_millis()
            );
            let (result, direct_won) = race_with_delay(
                self.open_direct_path(
                    direct_eps.clone(),
                    remote_id,
                    remote_name.clone(),
                    TunnelConnectIntent::active_logical(logical_tunnel_id),
                ),
                HEDGED_REVERSE_DELAY,
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
            let intent = TunnelConnectIntent {
                candidate_id: self.next_candidate_id(),
                ..intent
            };
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
                            let should_publish = manager.should_publish_tunnel(&tunnel);
                            match manager.register_tunnel(tunnel, true, should_publish).await {
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
        log::debug!(
            "reverse path start remote={} tunnel_id={:?} add waiter",
            remote_id,
            tunnel_id
        );
        self.add_reverse_waiter(remote_id.clone(), tunnel_id, notify);
        let call_result = sn_service
            .call(tunnel_id, None, remote_id, TunnelType::Stream, vec![])
            .await;
        if let Err(err) = call_result {
            log::warn!(
                "reverse path remote={} tunnel_id={:?} sn call failed code={:?} msg={}",
                remote_id,
                tunnel_id,
                err.code(),
                err.msg()
            );
            self.take_reverse_waiter(remote_id, &tunnel_id);
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
            self.take_reverse_waiter(remote_id, &tunnel_id);
        }
        let tunnel = tunnel.map_err(into_p2p_err!(P2pErrorCode::Timeout))??;
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
        let should_publish = self.should_publish_tunnel(&tunnel);
        let tunnel = self.register_tunnel(tunnel, true, should_publish).await?;
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
                if let Some(stat) = state.endpoint_scores.get(ep) {
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
            .entry(*endpoint)
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

    fn should_publish_tunnel(&self, tunnel: &TunnelRef) -> bool {
        !tunnel.is_reverse() || !self.has_reverse_waiter(&tunnel.remote_id(), &tunnel.tunnel_id())
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
        if let Some(tunnel) = self.get_tunnel(&remote_id) {
            if is_tunnel_available(tunnel.as_ref()) {
                log::debug!("sn called remote={} reuse existing tunnel", remote_id);
                return Ok(());
            }
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
        if let Some(tunnel) = self.get_tunnel(&remote_id) {
            if is_tunnel_available(tunnel.as_ref()) {
                log::debug!(
                    "sn called remote={} tunnel became available before dial",
                    remote_id
                );
                return Ok(());
            }
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

    async fn register_tunnel(
        &self,
        tunnel: TunnelRef,
        close_replaced: bool,
        publish: bool,
    ) -> P2pResult<TunnelRef> {
        let remote_id = tunnel.remote_id();
        log::debug!(
            "register tunnel local={:?} remote={} tunnel_id={:?} candidate_id={:?} form={:?} reverse={} protocol={:?} close_replaced={} publish={} local_ep={:?} remote_ep={:?}",
            self.local_identity.get_id(),
            remote_id,
            tunnel.tunnel_id(),
            tunnel.candidate_id(),
            tunnel.form(),
            tunnel.is_reverse(),
            tunnel.protocol(),
            close_replaced,
            publish,
            tunnel.local_ep(),
            tunnel.remote_ep()
        );
        if remote_id.is_default() || (!close_replaced && tunnel.form() == TunnelForm::Proxy) {
            log::debug!(
                "register tunnel remote={} publish-only default_or_proxy={} ",
                remote_id,
                remote_id.is_default() || tunnel.form() == TunnelForm::Proxy
            );
            if publish {
                self.publish_tunnel(tunnel.clone());
            }
            return Ok(tunnel);
        }

        let _guard = Locker::get_locker(format!("network-register-{}", remote_id)).await;
        let remote_id_for_log = remote_id.clone();
        let replaced = {
            let mut tunnels = self.tunnels.write().unwrap();
            let entries = tunnels.entry(remote_id).or_default();
            entries.retain(|entry| is_tunnel_available(entry.tunnel.as_ref()));
            if let Some(entry) = entries.iter_mut().find(|entry| {
                entry.tunnel.tunnel_id() == tunnel.tunnel_id()
                    && entry.tunnel.candidate_id() == tunnel.candidate_id()
            }) {
                let replaced = std::mem::replace(&mut entry.tunnel, tunnel.clone());
                entry.updated_at = Instant::now();
                entry.published |= publish;
                Some(replaced)
            } else {
                entries.push(TunnelEntry {
                    tunnel: tunnel.clone(),
                    updated_at: Instant::now(),
                    published: publish,
                });
                None
            }
        };

        if let Some(old) = replaced {
            log::debug!(
                "register tunnel remote={} replaced old tunnel_id={:?} candidate_id={:?} form={:?} protocol={:?}",
                remote_id_for_log,
                old.tunnel_id(),
                old.candidate_id(),
                old.form(),
                old.protocol()
            );
            if !Arc::ptr_eq(&old, &tunnel) {
                let _ = old.close().await;
            }
        }

        log::debug!(
            "register tunnel remote={} stored tunnel_id={:?} candidate_id={:?} form={:?} protocol={:?} published={}",
            remote_id_for_log,
            tunnel.tunnel_id(),
            tunnel.candidate_id(),
            tunnel.form(),
            tunnel.protocol(),
            publish
        );
        if publish {
            self.publish_tunnel(tunnel.clone());
        }
        Ok(tunnel)
    }

    fn publish_registered_tunnel(&self, tunnel: &TunnelRef) {
        {
            let mut tunnels = self.tunnels.write().unwrap();
            if let Some(entries) = tunnels.get_mut(&tunnel.remote_id()) {
                if let Some(entry) = entries.iter_mut().find(|entry| {
                    entry.tunnel.tunnel_id() == tunnel.tunnel_id()
                        && entry.tunnel.candidate_id() == tunnel.candidate_id()
                }) {
                    if entry.published {
                        return;
                    }
                    entry.published = true;
                    entry.updated_at = Instant::now();
                }
            }
        }
        self.publish_tunnel(tunnel.clone());
    }

    fn publish_tunnel(&self, tunnel: TunnelRef) {
        log::debug!(
            "publish tunnel remote={} form={:?} protocol={:?}",
            tunnel.remote_id(),
            tunnel.form(),
            tunnel.protocol()
        );
        let mut subscriptions = self.subscriptions.lock().unwrap();
        subscriptions.retain(|tx| tx.send(Ok(tunnel.clone())).is_ok());
    }

    async fn cleanup_closed_tunnels(&self, _idle_timeout: Duration) {
        let mut removed = Vec::new();
        {
            let mut tunnels = self.tunnels.write().unwrap();
            tunnels.retain(|_, entries| {
                entries.retain(|entry| {
                    let keep = is_tunnel_available(entry.tunnel.as_ref());
                    if !keep {
                        removed.push(entry.tunnel.clone());
                    }
                    keep
                });
                !entries.is_empty()
            });
        }
        for tunnel in removed {
            let _ = tunnel.close().await;
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
            .unregister_tunnel_acceptor(&self.local_identity.get_id());
        self.incoming_task.abort();
        if let Some(task) = self.proxy_incoming_task.take() {
            task.abort();
        }
        self.cleanup_task.abort();
        for tunnel in tunnels {
            let _ = Executor::block_on(tunnel.close());
        }
    }
}

fn is_tunnel_available(tunnel: &dyn crate::networks::Tunnel) -> bool {
    !tunnel.is_closed() && tunnel.state() == TunnelState::Connected
}

#[cfg(all(test, feature = "x509"))]
mod tests {
    use super::*;
    use crate::endpoint::Protocol;
    use crate::executor::Executor;
    use crate::networks::{TcpTunnelListener, TcpTunnelNetwork, TcpTunnelRegistry, Tunnel};
    use crate::networks::{TunnelListener, TunnelListenerInfo, TunnelNetwork};
    use crate::tls::{DefaultTlsServerCertResolver, TlsServerCertResolver};
    use crate::tunnel::DefaultP2pConnectionInfoCache;
    use crate::types::{TunnelCandidateId, TunnelIdGenerator};
    use crate::x509::{X509IdentityCertFactory, X509IdentityFactory, generate_rsa_x509_identity};
    use std::sync::Once;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TLS_INIT: Once = Once::new();
    static TEST_TUNNEL_ID_SEQ: AtomicUsize = AtomicUsize::new(1);

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
        )
        .unwrap()
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
                call_count: AtomicUsize::new(0),
            })
        }

        fn start_offset(&self, endpoint: &Endpoint) -> Option<Duration> {
            self.start_offsets.lock().unwrap().get(endpoint).copied()
        }

        fn call_count(&self) -> usize {
            self.call_count.load(Ordering::SeqCst)
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
        ) -> P2pResult<crate::networks::TunnelListenerRef> {
            Err(p2p_err!(
                P2pErrorCode::NotSupport,
                "mock proxy listen not supported"
            ))
        }

        async fn close_all_listener(&self) -> P2pResult<()> {
            Ok(())
        }

        fn listeners(&self) -> Vec<crate::networks::TunnelListenerRef> {
            vec![]
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
        ) -> P2pResult<crate::networks::TunnelListenerRef> {
            Err(p2p_err!(
                P2pErrorCode::NotSupport,
                "mock listen not supported"
            ))
        }

        async fn close_all_listener(&self) -> P2pResult<()> {
            Ok(())
        }

        fn listeners(&self) -> Vec<crate::networks::TunnelListenerRef> {
            vec![]
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
            TunnelForm::Active
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

        async fn close(&self) -> P2pResult<()> {
            Ok(())
        }

        async fn listen_stream(&self, _vports: crate::networks::ListenVPortsRef) -> P2pResult<()> {
            Ok(())
        }

        async fn listen_datagram(
            &self,
            _vports: crate::networks::ListenVPortsRef,
        ) -> P2pResult<()> {
            Ok(())
        }

        async fn open_stream(
            &self,
            _purpose: crate::networks::TunnelPurpose,
        ) -> P2pResult<(TunnelStreamRead, TunnelStreamWrite)> {
            Err(p2p_err!(P2pErrorCode::NotSupport, "mock tunnel"))
        }

        async fn accept_stream(
            &self,
        ) -> P2pResult<(
            crate::networks::TunnelPurpose,
            TunnelStreamRead,
            TunnelStreamWrite,
        )> {
            Err(p2p_err!(P2pErrorCode::NotSupport, "mock tunnel"))
        }

        async fn open_datagram(
            &self,
            _purpose: crate::networks::TunnelPurpose,
        ) -> P2pResult<TunnelDatagramWrite> {
            Err(p2p_err!(P2pErrorCode::NotSupport, "mock tunnel"))
        }

        async fn accept_datagram(
            &self,
        ) -> P2pResult<(crate::networks::TunnelPurpose, TunnelDatagramRead)> {
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

        async fn close(&self) -> P2pResult<()> {
            self.close_count.fetch_add(1, Ordering::SeqCst);
            *self.state.lock().unwrap() = TunnelState::Closed;
            Ok(())
        }

        async fn listen_stream(&self, _vports: crate::networks::ListenVPortsRef) -> P2pResult<()> {
            Ok(())
        }

        async fn listen_datagram(
            &self,
            _vports: crate::networks::ListenVPortsRef,
        ) -> P2pResult<()> {
            Ok(())
        }

        async fn open_stream(
            &self,
            _purpose: crate::networks::TunnelPurpose,
        ) -> P2pResult<(TunnelStreamRead, TunnelStreamWrite)> {
            Err(p2p_err!(P2pErrorCode::NotSupport, "trackable tunnel"))
        }

        async fn accept_stream(
            &self,
        ) -> P2pResult<(
            crate::networks::TunnelPurpose,
            TunnelStreamRead,
            TunnelStreamWrite,
        )> {
            Err(p2p_err!(P2pErrorCode::NotSupport, "trackable tunnel"))
        }

        async fn open_datagram(
            &self,
            _purpose: crate::networks::TunnelPurpose,
        ) -> P2pResult<TunnelDatagramWrite> {
            Err(p2p_err!(P2pErrorCode::NotSupport, "trackable tunnel"))
        }

        async fn accept_datagram(
            &self,
        ) -> P2pResult<(crate::networks::TunnelPurpose, TunnelDatagramRead)> {
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
        )
        .unwrap();
        let mut callee_sub = callee_manager.subscribe();

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
        ));
        caller_network
            .listen(&loopback_tcp_ep(), None, None)
            .await
            .unwrap();

        let server_resolver = DefaultTlsServerCertResolver::new();
        server_resolver
            .add_server_identity(callee_identity.clone())
            .await
            .unwrap();
        let server_listener = TcpTunnelListener::new(
            server_resolver,
            Arc::new(X509IdentityCertFactory),
            TcpTunnelRegistry::new(),
            Duration::from_secs(3),
            Duration::from_millis(200),
            Duration::from_secs(5),
        );
        server_listener
            .bind(loopback_tcp_ep(), None, None, false)
            .await
            .unwrap();
        server_listener.start();

        let _opened = caller_network
            .create_tunnel(
                &caller_identity,
                &server_listener.bound_local(),
                &callee_identity.get_id(),
                Some(callee_identity.get_name()),
            )
            .await
            .unwrap();

        let accepted = server_listener.accept_tunnel().await.unwrap();
        callee_manager
            .register_tunnel(accepted, false, true)
            .await
            .unwrap();

        let subscribed = callee_sub.accept_tunnel().await.unwrap();
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
        let mut sub = manager.subscribe();
        let tunnel_id = TunnelId::from(42);
        let tunnel: TunnelRef = Arc::new(MockTunnel {
            tunnel_id,
            candidate_id: TunnelCandidateId::from(1),
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
        assert!(manager.get_tunnel(&remote_identity.get_id()).is_some());
        assert!(
            runtime::timeout(Duration::from_millis(100), sub.accept_tunnel())
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
            .register_tunnel(existing_ref.clone(), false, true)
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
            .register_tunnel(replacement_ref.clone(), true, true)
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
        let mut sub = manager.subscribe();
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
            .register_tunnel(first.clone(), false, true)
            .await
            .unwrap();
        manager
            .register_tunnel(second.clone(), false, true)
            .await
            .unwrap();

        let first_published = sub.accept_tunnel().await.unwrap();
        let second_published = sub.accept_tunnel().await.unwrap();
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
        let mut sub = manager.subscribe();
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
        let should_publish = manager.should_publish_tunnel(&reverse_ref);
        let returned = manager
            .register_tunnel(reverse_ref.clone(), false, should_publish)
            .await
            .unwrap();
        let published = sub.accept_tunnel().await.unwrap();

        assert!(should_publish);
        assert!(Arc::ptr_eq(&returned, &reverse_ref));
        assert!(Arc::ptr_eq(&published, &reverse_ref));
    }

    #[tokio::test]
    async fn reverse_incoming_tunnel_publishes_after_waiter_consumed() {
        init_tls_once();

        let local_identity = new_identity("local-reverse-after-waiter");
        let remote_identity = new_identity("remote-reverse-after-waiter");
        let manager = new_test_manager(local_identity.clone(), HashMap::new(), None);
        let mut sub = manager.subscribe();
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
            runtime::timeout(Duration::from_millis(100), sub.accept_tunnel())
                .await
                .is_err()
        );

        manager.on_incoming_tunnel(second.clone()).await.unwrap();
        let published = sub.accept_tunnel().await.unwrap();
        let second_ref: TunnelRef = second.clone();
        assert!(Arc::ptr_eq(&published, &second_ref));
        assert_eq!(first.close_count(), 0);
        assert_eq!(second.close_count(), 0);
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

        manager
            .register_tunnel(first.clone(), false, false)
            .await
            .unwrap();
        runtime::sleep(Duration::from_millis(1)).await;
        manager
            .register_tunnel(second.clone(), false, false)
            .await
            .unwrap();

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
            .register_tunnel(reverse_hidden.clone(), false, false)
            .await
            .unwrap();
        manager
            .register_tunnel(published.clone(), false, true)
            .await
            .unwrap();

        let selected = manager.get_tunnel(&remote_identity.get_id()).unwrap();
        let published_ref: TunnelRef = published.clone();
        assert!(Arc::ptr_eq(&selected, &published_ref));
    }

    #[tokio::test]
    async fn register_tunnel_proxy_without_replacement_is_only_published() {
        init_tls_once();

        let local_identity = new_identity("local-register-proxy");
        let remote_identity = new_identity("remote-register-proxy");
        let manager = new_test_manager(local_identity.clone(), HashMap::new(), None);
        let mut sub = manager.subscribe();
        let proxy = TrackableTunnel::new(
            TunnelForm::Proxy,
            local_identity.get_id(),
            remote_identity.get_id(),
            TunnelState::Connected,
        );
        let proxy_ref: TunnelRef = proxy.clone();

        let returned = manager
            .register_tunnel(proxy_ref, false, true)
            .await
            .unwrap();
        let published = sub.accept_tunnel().await.unwrap();

        assert_eq!(returned.form(), TunnelForm::Proxy);
        assert_eq!(published.remote_id(), remote_identity.get_id());
        assert!(manager.get_tunnel(&remote_identity.get_id()).is_none());
        assert_eq!(proxy.close_count(), 0);
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
            .register_tunnel(tunnel.clone(), false, true)
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
}
