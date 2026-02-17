use super::{
    P2pConnectionFactory, ReverseResult, ReverseWaiterCache, SessionSnCall, Tunnel,
    TunnelConnection, TunnelConnectionRead, TunnelConnectionRef, TunnelConnectionWrite,
    TunnelListenPorts, TunnelSession, TunnelState,
};
use crate::endpoint::Endpoint;
use crate::error::{P2pErrorCode, P2pResult, into_p2p_err, p2p_err};
use crate::executor::{Executor, SpawnHandle};
use crate::p2p_connection::{P2pConnection, P2pConnectionInfoCacheRef};
use crate::p2p_identity::{
    P2pId, P2pIdentityCertCacheRef, P2pIdentityCertFactoryRef, P2pIdentityCertRef, P2pIdentityRef,
};
use crate::pn::PnClientRef;
use crate::protocol::v0::{
    AckDatagram, AckReverseDatagram, AckReverseSession, AckSession, SnCalled, SynReverseSession,
    SynSession, TunnelType,
};
use crate::protocol::{Package, PackageCmdCode};
use crate::runtime;
use crate::sn::client::SNClientServiceRef;
use crate::sockets::NetManagerRef;
use crate::tunnel::proxy_connection::{ProxyConnectionRead, ProxyConnectionWrite};
use crate::tunnel::tunnel_listener::{NewTunnelEvent, TunnelListenerRef};
use crate::types::{SessionId, TunnelId, TunnelIdGenerator};
use async_named_locker::Locker;
use bucky_raw_codec::{RawConvertTo, RawFrom};
use bucky_time::bucky_time_now;
use notify_future::Notify;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, RwLock, Weak};
use std::time::Duration;
use tokio::io::AsyncWriteExt;

struct TunnelsState<F: P2pConnectionFactory> {
    tunnels: HashMap<TunnelId, Arc<Tunnel<F>>>,
    pending_tunnels: HashMap<TunnelId, Notify<ReverseResult>>,
    endpoint_scores: HashMap<Endpoint, EndpointScore>,
}

#[derive(Clone)]
struct EndpointScore {
    // Last successful direct-connect timestamp (microseconds).
    last_success_at: u64,
    // Consecutive failures used for temporary down-ranking.
    fail_count: u32,
}

impl<F: P2pConnectionFactory> TunnelsState<F> {
    pub fn get_idle_tunnel(&mut self) -> Option<Arc<Tunnel<F>>> {
        for (_, tunnel) in self.tunnels.iter() {
            if tunnel.is_idle() {
                return Some(tunnel.clone());
            }
        }
        None
    }

    pub fn tunnel_exist(&mut self, seq: TunnelId) -> bool {
        self.tunnels.contains_key(&seq)
    }

    pub fn get_tunnel(&self, tunnel_id: TunnelId) -> Option<Arc<Tunnel<F>>> {
        self.tunnels.get(&tunnel_id).map(|v| v.clone())
    }

    pub fn remove_tunnel(&mut self, seq: TunnelId) {
        self.tunnels.remove(&seq);
    }

    pub fn add_pending_future(&mut self, seq: TunnelId, future: Notify<ReverseResult>) {
        self.pending_tunnels.insert(seq, future);
    }

    pub fn try_pop_pending_future(&mut self, seq: TunnelId) -> Option<Notify<ReverseResult>> {
        self.pending_tunnels.remove(&seq)
    }

    pub fn is_exist_pending_future(&self, seq: TunnelId) -> bool {
        self.pending_tunnels.contains_key(&seq)
    }
}

#[callback_trait::callback_trait]
pub trait NewSessionEvent: Send + Sync + 'static {
    async fn on_new_session(
        &self,
        session: SynSession,
        read: TunnelConnectionRead,
        write: TunnelConnectionWrite,
    ) -> P2pResult<()>;
}

struct Tunnels<F: P2pConnectionFactory> {
    listener: Arc<dyn NewSessionEvent>,
    state: Mutex<TunnelsState<F>>,
}

impl<F: P2pConnectionFactory> Drop for Tunnels<F> {
    fn drop(&mut self) {}
}
impl<F: P2pConnectionFactory> Tunnels<F> {
    pub fn new(listener: impl NewSessionEvent) -> Arc<Self> {
        Arc::new(Self {
            listener: Arc::new(listener),
            state: Mutex::new(TunnelsState {
                tunnels: Default::default(),
                pending_tunnels: Default::default(),
                endpoint_scores: Default::default(),
            }),
        })
    }

    pub fn get_idle_tunnel(&self) -> Option<Arc<Tunnel<F>>> {
        let mut state = self.state.lock().unwrap();
        state.get_idle_tunnel()
    }

    pub fn add_tunnel(self: &Arc<Self>, tunnel: Tunnel<F>) -> bool {
        let tunnel = Arc::new(tunnel);
        {
            let mut state = self.state.lock().unwrap();
            if state.tunnels.contains_key(&tunnel.get_tunnel_id()) {
                return false;
            }
            state.tunnels.insert(tunnel.get_tunnel_id(), tunnel.clone());
        }
        let this = self.clone();
        let _ = Executor::spawn(async move {
            loop {
                match tunnel.accept_session().await {
                    Ok(instance) => {
                        match instance {
                            TunnelSession::Forward((session, read, write)) => {
                                // log::info!("accept stream tunnel {:?} stream_id {} port {} remote_id {} remote_ep {} local_id {} local_ep {}",
                                //     stream.tunnel_id(),
                                //     stream.session_id(),
                                //     stream.port(),
                                //     stream.remote_identity_id().to_string(),
                                //     stream.remote_endpoint().to_string(),
                                //     stream.local_identity_id().to_string(),
                                //     stream.local_endpoint().to_string());

                                if let Err(e) =
                                    this.listener.on_new_session(session, read, write).await
                                {
                                    log::error!("accept tunnel error: {:?}", e);
                                }
                            }
                            TunnelSession::Reverse(_stream) => {
                                log::error!("reverse tunnel not support");
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        log::error!("accept tunnel error: {:?}", e);
                        break;
                    }
                }
            }
            let mut state = this.state.lock().unwrap();
            state.tunnels.remove(&tunnel.get_tunnel_id());
        });
        true
    }

    pub fn get_tunnel(&self, tunnel_id: TunnelId) -> Option<Arc<Tunnel<F>>> {
        let state = self.state.lock().unwrap();
        state.get_tunnel(tunnel_id)
    }

    pub fn tunnel_exist(&self, seq: TunnelId) -> bool {
        let mut state = self.state.lock().unwrap();
        state.tunnel_exist(seq)
    }

    pub fn remove_tunnel(&self, seq: TunnelId) {
        let mut state = self.state.lock().unwrap();
        state.remove_tunnel(seq);
    }

    pub fn try_pop_pending_future(&self, seq: TunnelId) -> Option<Notify<ReverseResult>> {
        let mut state = self.state.lock().unwrap();
        state.try_pop_pending_future(seq)
    }

    pub fn add_pending_future(&self, seq: TunnelId, future: Notify<ReverseResult>) {
        let mut state = self.state.lock().unwrap();
        state.add_pending_future(seq, future);
    }

    pub fn is_exist_pending_future(&self, seq: TunnelId) -> bool {
        let state = self.state.lock().unwrap();
        state.is_exist_pending_future(seq)
    }

    pub fn preferred_direct_endpoints(
        &self,
        endpoints: &[Endpoint],
        preferred_ep: Option<&Endpoint>,
    ) -> Vec<Endpoint> {
        let state = self.state.lock().unwrap();
        let mut ranked: Vec<(i64, usize, Endpoint)> = endpoints
            .iter()
            .enumerate()
            .map(|(idx, ep)| {
                let mut score: i64 = 0;
                // Strongly prefer the endpoint that succeeded most recently in conn_info_cache.
                if preferred_ep
                    .map(|preferred| preferred == ep)
                    .unwrap_or(false)
                {
                    score += 10_000;
                }
                // Slightly prefer WAN/mapped endpoints outside LAN scenario.
                if ep.is_static_wan() {
                    score += 500;
                }
                if let Some(stat) = state.endpoint_scores.get(ep) {
                    if stat.last_success_at > 0 {
                        score += 2_000;
                    }
                    // Penalize repeatedly failing endpoints, with an upper bound.
                    score -= (stat.fail_count.min(20) as i64) * 300;
                }
                (score, idx, *ep)
            })
            .collect();
        ranked.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        ranked.into_iter().map(|(_, _, ep)| ep).collect()
    }

    pub fn on_direct_connect_result(&self, endpoint: &Endpoint, success: bool) {
        let mut state = self.state.lock().unwrap();
        let stat = state
            .endpoint_scores
            .entry(*endpoint)
            .or_insert(EndpointScore {
                last_success_at: 0,
                fail_count: 0,
            });
        if success {
            // Success immediately resets failure penalty.
            stat.last_success_at = bucky_time_now();
            stat.fail_count = 0;
        } else {
            stat.fail_count = stat.fail_count.saturating_add(1);
        }
    }

    pub async fn close_all_tunnel(&self) {
        let mut state = self.state.lock().unwrap();
        state.tunnels.clear();
        state.pending_tunnels.clear();
    }
}

impl<F: P2pConnectionFactory> ReverseWaiterCache for Tunnels<F> {
    fn add_reverse_waiter(&self, sequence: TunnelId, future: Notify<ReverseResult>) {
        self.add_pending_future(sequence, future);
    }

    fn remove_reverse_waiter(&self, sequence: TunnelId) {
        self.try_pop_pending_future(sequence);
    }

    fn preferred_direct_endpoints(
        &self,
        endpoints: &[Endpoint],
        preferred_ep: Option<&Endpoint>,
    ) -> Vec<Endpoint> {
        self.preferred_direct_endpoints(endpoints, preferred_ep)
    }

    fn on_direct_connect_result(&self, endpoint: &Endpoint, success: bool) {
        self.on_direct_connect_result(endpoint, success)
    }
}

struct ListenPorts {
    listen_ports: Mutex<HashSet<u16>>,
}

impl ListenPorts {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            listen_ports: Mutex::new(Default::default()),
        })
    }

    pub fn add_listen_port(&self, port: u16) {
        let mut listen_ports = self.listen_ports.lock().unwrap();
        listen_ports.insert(port);
    }

    pub fn remove_listen_port(&self, port: u16) {
        let mut listen_ports = self.listen_ports.lock().unwrap();
        listen_ports.remove(&port);
    }
}

impl TunnelListenPorts for ListenPorts {
    fn is_listen(&self, port: u16) -> bool {
        let listen_ports = self.listen_ports.lock().unwrap();
        listen_ports.contains(&port)
    }
}

#[async_trait::async_trait]
pub trait DeviceFinder: 'static + Send + Sync {
    async fn get_identity_cert(&self, device_id: &P2pId) -> P2pResult<P2pIdentityCertRef>;
}
pub type DeviceFinderRef = Arc<dyn DeviceFinder>;

pub struct DefaultDeviceFinder {
    cert_cache: P2pIdentityCertCacheRef,
    sn_service: SNClientServiceRef,
    cert_factory: P2pIdentityCertFactoryRef,
    query_cache: mini_moka::sync::Cache<P2pId, u64>,
}

impl DefaultDeviceFinder {
    pub fn new(
        sn_service: SNClientServiceRef,
        cert_factory: P2pIdentityCertFactoryRef,
        cert_cache: P2pIdentityCertCacheRef,
        interval: Duration,
    ) -> Arc<Self> {
        Arc::new(Self {
            cert_cache,
            sn_service,
            cert_factory,
            query_cache: mini_moka::sync::Cache::builder()
                .time_to_live(interval)
                .build(),
        })
    }
}

#[async_trait::async_trait]
impl DeviceFinder for DefaultDeviceFinder {
    async fn get_identity_cert(&self, device_id: &P2pId) -> P2pResult<P2pIdentityCertRef> {
        if let Some(device) = self.cert_cache.get(device_id).await {
            return Ok(device);
        }

        if self.query_cache.contains_key(device_id) {
            return Err(p2p_err!(P2pErrorCode::NotFound, "device not found"));
        }

        let resp = self.sn_service.query(device_id).await?;
        log::info!("query device {} resp {:?}", device_id, resp);
        if resp.peer_info.is_none() {
            return Err(p2p_err!(P2pErrorCode::NotFound, "device not found"));
        }
        let device = resp.peer_info.unwrap();
        let mut device = self.cert_factory.create(&device)?;
        if !resp.end_point_array.is_empty() {
            let mut eps = device.endpoints();
            for wan_ep in resp.end_point_array.iter() {
                let mut has = false;
                for ep in eps.iter() {
                    if ep.protocol() == wan_ep.protocol() && ep.addr() == wan_ep.addr() {
                        has = true;
                        break;
                    }
                }
                if !has {
                    eps.push(wan_ep.clone());
                }
            }

            device = device.update_endpoints(eps);
        }
        self.cert_cache.add(device_id, &device).await;
        Ok(device)
    }
}

struct NewTunnelListener<F: P2pConnectionFactory> {
    manager: Weak<TunnelManager<F>>,
}

impl<F: P2pConnectionFactory> NewTunnelListener<F> {
    pub fn new(manager: Arc<TunnelManager<F>>) -> Self {
        Self {
            manager: Arc::downgrade(&manager),
        }
    }
}

#[async_trait::async_trait]
impl<F: P2pConnectionFactory> NewTunnelEvent for NewTunnelListener<F> {
    async fn on_new_tunnel(
        &self,
        session: SynSession,
        conn: TunnelConnectionRef,
        read: TunnelConnectionRead,
        write: TunnelConnectionWrite,
    ) -> P2pResult<()> {
        if let Some(manager) = self.manager.upgrade() {
            manager
                .on_new_tunnel(session, Some(conn), read, write)
                .await?;
        }
        Ok(())
    }

    async fn on_new_reverse_tunnel(
        &self,
        session: SynReverseSession,
        conn: TunnelConnectionRef,
        read: TunnelConnectionRead,
        write: TunnelConnectionWrite,
    ) -> P2pResult<()> {
        if let Some(manager) = self.manager.upgrade() {
            manager
                .on_new_reverse_tunnel(session, conn, read, write)
                .await?;
        }
        Ok(())
    }

    async fn on_sn_called(&self, called: SnCalled) -> P2pResult<()> {
        if let Some(manager) = self.manager.upgrade() {
            manager.on_sn_called(called).await?;
        }
        Ok(())
    }
}

#[callback_trait::callback_trait]
pub trait TunnelManagerEvent: Send + Sync + 'static {
    async fn on_new_session(
        &self,
        session_id: SessionId,
        vport: u16,
        read: TunnelConnectionRead,
        write: TunnelConnectionWrite,
    ) -> P2pResult<()>;
}

pub struct TunnelManager<F: P2pConnectionFactory> {
    tunnels: Arc<RwLock<HashMap<P2pId, Arc<Tunnels<F>>>>>,
    sn_service: SNClientServiceRef,
    local_identity: P2pIdentityRef,
    protocol_version: u8,
    conn_timeout: Duration,
    idle_timeout: Duration,
    gen_id: Arc<TunnelIdGenerator>,
    listen_ports: Arc<ListenPorts>,
    clear_handle: SpawnHandle<()>,
    device_finder: DeviceFinderRef,
    cert_factory: P2pIdentityCertFactoryRef,
    sn_calling: Mutex<HashMap<P2pId, HashSet<TunnelId>>>,
    pn_client: Option<PnClientRef>,
    p2p_factory: Arc<F>,
    tunnel_listener: TunnelListenerRef,
    conn_info_cache: P2pConnectionInfoCacheRef,
    listener: Mutex<Option<Arc<dyn TunnelManagerEvent>>>,
}
pub type TunnelManagerRef<F> = Arc<TunnelManager<F>>;

impl<F: P2pConnectionFactory> TunnelManager<F> {
    pub(crate) fn new(
        sn_service: SNClientServiceRef,
        local_identity: P2pIdentityRef,
        device_finder: DeviceFinderRef,
        cert_factory: P2pIdentityCertFactoryRef,
        pn_client: Option<PnClientRef>,
        gen_id: Arc<TunnelIdGenerator>,
        p2p_factory: F,
        tunnel_listener: TunnelListenerRef,
        conn_info_cache: P2pConnectionInfoCacheRef,
        protocol_version: u8,
        conn_timeout: Duration,
        idle_timeout: Duration,
    ) -> Arc<Self> {
        let tunnels = Arc::new(RwLock::new(HashMap::<P2pId, Arc<Tunnels<F>>>::new()));
        let tmp = tunnels.clone();
        let clear_idle_timeout = idle_timeout;
        let handle = Executor::spawn_with_handle(async move {
            loop {
                runtime::sleep(Duration::from_secs(120)).await;
                Self::clear_idle_tunnel(tmp.clone(), clear_idle_timeout).await;
            }
        })
        .unwrap();
        let tunnel_type = p2p_factory.tunnel_type();
        let manager = Arc::new(Self {
            tunnels,
            sn_service,
            local_identity,
            protocol_version,
            conn_timeout,
            idle_timeout,
            gen_id,
            listen_ports: ListenPorts::new(),
            clear_handle: handle,
            device_finder,
            cert_factory,
            sn_calling: Mutex::new(Default::default()),
            pn_client: pn_client.clone(),
            p2p_factory: Arc::new(p2p_factory),
            tunnel_listener: tunnel_listener.clone(),
            conn_info_cache,
            listener: Mutex::new(None),
        });

        tunnel_listener
            .register_new_tunnel_event(tunnel_type, NewTunnelListener::new(manager.clone()));

        manager
    }

    pub fn set_listener(&self, listener: impl TunnelManagerEvent) {
        let mut l = self.listener.lock().unwrap();
        *l = Some(Arc::new(listener));
    }
    pub fn add_listen_port(&self, port: u16) {
        self.listen_ports.add_listen_port(port)
    }

    pub fn is_exist_listen_port(&self, port: u16) -> bool {
        self.listen_ports.is_listen(port)
    }

    pub fn remove_listen_port(&self, port: u16) {
        self.listen_ports.remove_listen_port(port)
    }

    async fn clear_idle_tunnel(
        tunnels: Arc<RwLock<HashMap<P2pId, Arc<Tunnels<F>>>>>,
        idle_timeout: Duration,
    ) {
        // bucky_time_now() is in microseconds, keep unit consistent with idle_timeout.
        let idle_timeout_us = idle_timeout.as_micros() as u64;
        let tunnels_map = tunnels.read().unwrap();
        let mut removed_count = 0usize;
        for (_, tunnels) in tunnels_map.iter() {
            let mut remove_list = Vec::new();
            {
                let state = tunnels.state.lock().unwrap();
                for (_, tunnel) in state.tunnels.iter() {
                    let tunnel_stat = tunnel.tunnel_stat();
                    if tunnel.is_error()
                        || (tunnel.is_idle()
                            && tunnel_stat.get_work_instance_num() == 0
                            && bucky_time_now() - tunnel_stat.get_latest_active_time()
                                > idle_timeout_us)
                    {
                        remove_list.push(tunnel.clone());
                    }
                }
            }
            for tunnel in remove_list.into_iter() {
                let seq = tunnel.get_tunnel_id();
                let mut state = tunnels.state.lock().unwrap();
                state.tunnels.remove(&seq);
                removed_count += 1;
            }
        }
        if removed_count > 0 {
            log::info!(
                "tunnel manager idle cleanup removed {} tunnels timeout_ms {}",
                removed_count,
                idle_timeout.as_millis()
            );
        }
    }

    async fn close_all_tunnel(&self) {
        let tunnels = self.tunnels.read().unwrap();
        let mut tunnels_list = Vec::new();
        for (_, tunnels) in tunnels.iter() {
            tunnels_list.push(tunnels.clone());
        }

        for tunnels in tunnels_list.iter() {
            tunnels.close_all_tunnel().await;
        }
    }

    async fn on_new_tunnel(
        self: Arc<Self>,
        session: SynSession,
        conn: Option<TunnelConnectionRef>,
        read: TunnelConnectionRead,
        mut write: TunnelConnectionWrite,
    ) -> P2pResult<()> {
        let tunnels = self.get_tunnels(&read.remote_id());
        if let Some(conn) = conn {
            if tunnels.tunnel_exist(conn.get_tunnel_id()) {
                return Ok(());
            }

            let mut tunnel = Tunnel::new(
                self.sn_service.clone(),
                session.tunnel_id,
                self.protocol_version,
                read.remote_id(),
                vec![read.remote()],
                Some(read.remote_name()),
                self.local_identity.clone(),
                self.conn_timeout,
                self.idle_timeout,
                self.cert_factory.clone(),
                self.pn_client.clone(),
                self.p2p_factory.clone(),
                self.conn_info_cache.clone(),
            );
            tunnel.set_tunnel_conn(conn);
            tunnels.add_tunnel(tunnel);
        }

        let mut result = 0;
        if !self.listen_ports.is_listen(session.to_vport) {
            result = P2pErrorCode::PortNotListen as u8;
        }

        let pkg = Package::new(
            self.protocol_version,
            PackageCmdCode::AckSession,
            AckSession { result },
        );
        write.send_pkg(pkg).await?;

        if let Some(tunnel) = tunnels.get_tunnel(session.tunnel_id) {
            tunnel.set_tunnel_state(TunnelState::Worked);
        }
        if result == 0 {
            let listener = { self.listener.lock().unwrap().clone() };
            if let Some(listener) = listener {
                listener
                    .on_new_session(session.session_id, session.to_vport, read, write)
                    .await?;
            }
        } else {
        }
        Ok(())
    }

    async fn on_new_reverse_tunnel(
        self: &Arc<Self>,
        session: SynReverseSession,
        conn: TunnelConnectionRef,
        read: TunnelConnectionRead,
        mut write: TunnelConnectionWrite,
    ) -> P2pResult<()> {
        let remote_id = read.remote_id();
        let tunnels = self.get_tunnels(&remote_id);
        let _locker = Locker::get_locker(format!(
            "tunnel_{}_{:?}",
            remote_id.to_string(),
            conn.get_tunnel_id()
        ))
        .await;
        if tunnels.is_exist_pending_future(conn.get_tunnel_id()) {
            let ack = AckReverseSession { result: 0 };
            let pkg = Package::new(
                self.protocol_version,
                PackageCmdCode::AckReverseSession,
                ack,
            );
            match write.send_pkg(pkg).await {
                Ok(_) => {
                    log::info!(
                        "new tunnel {:?} remote_id {} remote_ep {} local_id {} local_ep {}",
                        conn.get_tunnel_id(),
                        remote_id.to_string(),
                        read.remote().to_string(),
                        read.local_id().to_string(),
                        read.local().to_string()
                    );
                    let future = tunnels
                        .try_pop_pending_future(conn.get_tunnel_id())
                        .unwrap();
                    future.notify(ReverseResult::Session(session.result, conn, read, write));
                    Ok(())
                }
                Err(e) => {
                    log::error!(
                        "write tunnel {:?} ack reverse stream error: {:?}",
                        conn.get_tunnel_id(),
                        e
                    );
                    Ok(())
                }
            }
        } else {
            //反连时如果找不到对应的tunnel，说明该连接是错误的，就应该直接关闭连接
            Ok(())
        }
    }

    fn get_tunnels(self: &Arc<Self>, remote_id: &P2pId) -> Arc<Tunnels<F>> {
        let mut tunnels = self.tunnels.write().unwrap();
        let device_tunnels = tunnels.get(remote_id);
        if device_tunnels.is_none() {
            let this = self.clone();
            let device_tunnels = Tunnels::new(
                move |session: SynSession,
                      read: TunnelConnectionRead,
                      write: TunnelConnectionWrite| {
                    let this = this.clone();
                    async move {
                        this.on_new_tunnel(session, None, read, write).await?;
                        Ok(())
                    }
                },
            );
            tunnels.insert(remote_id.clone(), device_tunnels.clone());
            device_tunnels
        } else {
            device_tunnels.unwrap().clone()
        }
    }

    pub async fn create_session(
        self: &Arc<Self>,
        remote: &P2pIdentityCertRef,
        session_id: SessionId,
        vport: u16,
    ) -> P2pResult<(TunnelConnectionRead, TunnelConnectionWrite)> {
        let remote_id = remote.get_id();
        // Serialize tunnel creation per remote to avoid connection storms.
        let _create_guard = Locker::get_locker(format!("create_tunnel_{}", remote_id)).await;
        let tunnels = self.get_tunnels(&remote_id);

        if let Some(tunnel) = tunnels.get_idle_tunnel() {
            log::debug!(
                "create session remote {} session {} vport {} reuse tunnel {:?}",
                remote_id,
                session_id,
                vport,
                tunnel.get_tunnel_id()
            );
            tunnel.open_session(vport, session_id).await
        } else {
            let seq = self.generate_tunnel_id(&remote_id);
            log::info!(
                "create session remote {} session {} vport {} new tunnel {:?}",
                remote_id,
                session_id,
                vport,
                seq
            );

            let remote = match self.device_finder.get_identity_cert(&remote.get_id()).await {
                Ok(remote) => remote,
                Err(_) => remote.clone(),
            };
            let mut tunnel = Tunnel::new(
                self.sn_service.clone(),
                seq,
                self.protocol_version,
                remote.get_id(),
                remote.endpoints(),
                Some(remote.get_name()),
                self.local_identity.clone(),
                self.conn_timeout,
                self.idle_timeout,
                self.cert_factory.clone(),
                self.pn_client.clone(),
                self.p2p_factory.clone(),
                self.conn_info_cache.clone(),
            );
            let ret = tunnel
                .connect_session(vport, session_id, tunnels.clone())
                .await;
            if tunnel.is_work() {
                tunnels.add_tunnel(tunnel);
                log::info!(
                    "create session remote {} session {} vport {} tunnel {:?} added to pool",
                    remote_id,
                    session_id,
                    vport,
                    seq
                );
            }
            ret
        }
    }

    fn generate_tunnel_id(self: &Arc<Self>, remote_id: &P2pId) -> TunnelId {
        if remote_id.is_default() {
            self.gen_id.generate()
        } else {
            let tunnels = self.get_tunnels(remote_id);
            loop {
                let seq = self.gen_id.generate();
                if !tunnels.tunnel_exist(seq) {
                    return seq;
                }
            }
        }
    }

    pub async fn create_session_from_id(
        self: &Arc<Self>,
        remote_id: &P2pId,
        session_id: SessionId,
        vport: u16,
    ) -> P2pResult<(TunnelConnectionRead, TunnelConnectionWrite)> {
        // Same single-flight guard for callers that only provide remote_id.
        let _create_guard = Locker::get_locker(format!("create_tunnel_{}", remote_id)).await;
        let tunnels = self.get_tunnels(remote_id);
        if let Some(tunnel) = tunnels.get_idle_tunnel() {
            log::debug!(
                "create session by id remote {} session {} vport {} reuse tunnel {:?}",
                remote_id,
                session_id,
                vport,
                tunnel.get_tunnel_id()
            );
            tunnel.open_session(vport, session_id).await
        } else {
            let remote = self.device_finder.get_identity_cert(remote_id).await?;
            let seq = self.generate_tunnel_id(remote_id);
            log::info!(
                "create session by id remote {} session {} vport {} new tunnel {:?}",
                remote_id,
                session_id,
                vport,
                seq
            );
            let mut tunnel = Tunnel::new(
                self.sn_service.clone(),
                seq,
                self.protocol_version,
                remote_id.clone(),
                remote.endpoints(),
                Some(remote.get_name()),
                self.local_identity.clone(),
                self.conn_timeout,
                self.idle_timeout,
                self.cert_factory.clone(),
                self.pn_client.clone(),
                self.p2p_factory.clone(),
                self.conn_info_cache.clone(),
            );
            let ret = tunnel
                .connect_session(vport, session_id, tunnels.clone())
                .await;
            if tunnel.is_work() {
                tunnels.add_tunnel(tunnel);
                log::info!(
                    "create session by id remote {} session {} vport {} tunnel {:?} added to pool",
                    remote_id,
                    session_id,
                    vport,
                    seq
                );
            }
            ret
        }
    }

    pub async fn create_session_direct(
        self: &Arc<Self>,
        remote_eps: Vec<Endpoint>,
        session_id: SessionId,
        vport: u16,
        remote_id: Option<P2pId>,
    ) -> P2pResult<(TunnelConnectionRead, TunnelConnectionWrite)> {
        let (remote_id, remote_name) = if let Some(remote_id) = remote_id {
            let remote_name = match self.device_finder.get_identity_cert(&remote_id).await {
                Ok(remote) => remote.get_name(),
                Err(_) => remote_id.to_string(),
            };
            (remote_id, remote_name)
        } else {
            let remote_id = P2pId::default();
            let remote_name = remote_id.to_string();
            (remote_id, remote_name)
        };

        let seq = self.generate_tunnel_id(&remote_id);
        let mut tunnel = Tunnel::new(
            self.sn_service.clone(),
            seq,
            self.protocol_version,
            remote_id.clone(),
            remote_eps,
            Some(remote_name),
            self.local_identity.clone(),
            self.conn_timeout,
            self.idle_timeout,
            self.cert_factory.clone(),
            self.pn_client.clone(),
            self.p2p_factory.clone(),
            self.conn_info_cache.clone(),
        );
        let ret = tunnel.connect_session_direct(vport, session_id).await;
        if tunnel.is_work() {
            let tunnels = self.get_tunnels(&remote_id);
            tunnels.add_tunnel(tunnel);
        }
        ret
    }

    pub async fn on_sn_called(self: &Arc<Self>, sn_called: SnCalled) -> P2pResult<()> {
        let tunnel_id = sn_called.tunnel_id.clone();
        let cert = self.cert_factory.create(&sn_called.peer_info)?;
        let from_id = cert.get_id();
        {
            let mut calling = self.sn_calling.lock().unwrap();
            if let Some(tunnels) = calling.get_mut(&from_id) {
                if tunnels.contains(&tunnel_id) {
                    return Ok(());
                } else {
                    tunnels.insert(tunnel_id);
                }
            } else {
                let mut tunnels = HashSet::new();
                tunnels.insert(tunnel_id);
                calling.insert(from_id.clone(), tunnels);
            }
        }
        let ret = self.on_sn_called_inner(sn_called, cert).await;

        {
            let mut calling = self.sn_calling.lock().unwrap();
            if let Some(tunnels) = calling.get_mut(&from_id) {
                tunnels.remove(&tunnel_id);
                if tunnels.len() == 0 {
                    calling.remove(&from_id);
                }
            }
        }

        ret
    }

    async fn on_sn_called_inner(
        self: &Arc<Self>,
        sn_called: SnCalled,
        cert: P2pIdentityCertRef,
    ) -> P2pResult<()> {
        log::info!("on_sn_called {:?}", sn_called);

        let eps = if self
            .sn_service
            .is_same_lan(&sn_called.reverse_endpoint_array)
        {
            let mut eps = cert.endpoints().clone();
            for ep in sn_called.reverse_endpoint_array.iter() {
                if !eps.contains(ep) {
                    eps.push(ep.clone());
                }
            }
            eps
        } else {
            let mut eps = sn_called.reverse_endpoint_array;
            for ep in cert.endpoints().iter() {
                if !eps.contains(ep) {
                    eps.push(ep.clone());
                }
            }
            eps
        };

        let from_device_id = cert.get_id();

        {
            let tunnels = self.get_tunnels(&from_device_id);
            if tunnels.tunnel_exist(sn_called.tunnel_id) {
                return Ok(());
            }
        }
        if sn_called.call_type != self.p2p_factory.tunnel_type() {
            return Err(p2p_err!(
                P2pErrorCode::InvalidParam,
                "invalid call type {:?}, expect {:?}",
                sn_called.call_type,
                self.p2p_factory.tunnel_type()
            ));
        }

        let session_info = SessionSnCall::clone_from_slice(sn_called.payload.as_slice())
            .map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
        let mut result = 0;
        if !self.listen_ports.is_listen(session_info.vport) {
            result = P2pErrorCode::PortNotListen as u8;
        }
        let mut tunnel = Tunnel::new(
            self.sn_service.clone(),
            sn_called.tunnel_id,
            self.protocol_version,
            cert.get_id(),
            eps.clone(),
            Some(cert.get_name()),
            self.local_identity.clone(),
            self.conn_timeout,
            self.idle_timeout,
            self.cert_factory.clone(),
            self.pn_client.clone(),
            self.p2p_factory.clone(),
            self.conn_info_cache.clone(),
        );

        let session_call = SessionSnCall::clone_from_slice(sn_called.payload.as_slice())
            .map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
        let ret = tunnel
            .connect_reverse_session(session_call.vport, session_call.session_id, result)
            .await;
        if tunnel.is_work() {
            let tunnels = self.get_tunnels(&from_device_id);
            tunnels.add_tunnel(tunnel);
        }
        match ret {
            Ok((read, write)) => {
                if result == 0 {
                    let listener = { self.listener.lock().unwrap().clone() };
                    if let Some(listener) = listener {
                        listener
                            .on_new_session(
                                session_call.session_id,
                                session_call.vport,
                                read,
                                write,
                            )
                            .await?;
                    }
                }
                Ok(())
            }
            Err(e) => Err(e),
        }
    }
}

impl<F: P2pConnectionFactory> Drop for TunnelManager<F> {
    fn drop(&mut self) {
        log::info!(
            "tunnel manager drop.device {}",
            self.local_identity.get_id().to_string()
        );
        self.tunnel_listener
            .remove_new_tunnel_event(self.p2p_factory.tunnel_type());
        self.clear_handle.abort();
        Executor::block_on(self.close_all_tunnel());
    }
}

#[cfg(all(test, feature = "x509"))]
mod tests {
    use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU16, AtomicUsize, Ordering};
    use std::time::Duration;

    use async_trait::async_trait;
    use bucky_raw_codec::RawConvertTo;
    use tokio::sync::mpsc;

    use super::*;
    use crate::endpoint::Protocol;
    use crate::finder::{DeviceCache, DeviceCacheConfig};
    use crate::p2p_connection::{DefaultP2pConnectionInfoCache, P2pConnection};
    use crate::p2p_identity::{
        P2pIdentityCertFactory, P2pIdentityCertFactoryRef, P2pIdentityRef, P2pSn,
    };
    use crate::p2p_network::P2pNetworkRef;
    use crate::protocol::v0::{SnCalled, TunnelType};
    use crate::sn::client::{SNClientService, SNClientServiceRef};
    use crate::sn::service::{SnServiceConfig, SnServiceRef, create_sn_service};
    use crate::sockets::tcp::TcpNetwork;
    use crate::sockets::{NetManager, NetManagerRef, QuicCongestionAlgorithm, QuicNetwork};
    use crate::tls::DefaultTlsServerCertResolver;
    use crate::tunnel::TunnelListener;
    use crate::types::{SequenceGenerator, TunnelIdGenerator};
    use crate::x509::{X509IdentityCertFactory, X509IdentityFactory, generate_x509_identity};

    const ONLINE_TIMEOUT: Duration = Duration::from_secs(10);
    const CALL_TIMEOUT: Duration = Duration::from_secs(8);
    const CONN_TIMEOUT: Duration = Duration::from_millis(500);
    const IDLE_TIMEOUT: Duration = Duration::from_secs(20);
    static NEXT_PORT: AtomicU16 = AtomicU16::new(43000);

    fn next_port() -> u16 {
        NEXT_PORT.fetch_add(1, Ordering::Relaxed)
    }

    fn localhost_quic_endpoint(port: u16) -> Endpoint {
        Endpoint::from((
            Protocol::Quic,
            SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)),
        ))
    }

    fn build_identity(name: &str, endpoint: Endpoint) -> P2pIdentityRef {
        let identity = generate_x509_identity(Some(name.to_owned())).unwrap();
        let identity: P2pIdentityRef = Arc::new(identity);
        identity.update_endpoints(vec![endpoint])
    }

    fn build_sn_entry(sn_identity: &P2pIdentityRef) -> P2pSn {
        let sn_cert = sn_identity.get_identity_cert().unwrap();
        P2pSn::new(sn_cert.get_id(), sn_cert.get_name(), sn_cert.endpoints())
    }

    async fn create_net_manager(
        endpoint: Endpoint,
        cert_factory: P2pIdentityCertFactoryRef,
    ) -> NetManagerRef {
        let cert_cache = Arc::new(DeviceCache::new(
            &DeviceCacheConfig {
                expire: Duration::from_secs(600),
                capacity: 1024,
            },
            None,
        ));
        let cert_resolver = DefaultTlsServerCertResolver::new();
        let tcp_network = Arc::new(TcpNetwork::new(
            cert_cache.clone(),
            cert_resolver.clone(),
            cert_factory.clone(),
            Duration::from_secs(3),
        )) as P2pNetworkRef;
        let quic_network = Arc::new(QuicNetwork::new(
            cert_cache,
            cert_resolver.clone(),
            cert_factory,
            QuicCongestionAlgorithm::Bbr,
            Duration::from_secs(3),
            Duration::from_secs(10),
        )) as P2pNetworkRef;
        let net_manager = Arc::new(
            NetManager::new(
                vec![tcp_network, quic_network],
                cert_resolver,
                DefaultP2pConnectionInfoCache::new(),
            )
            .unwrap(),
        );
        net_manager.listen(&[endpoint], None).await.unwrap();
        net_manager
    }

    async fn start_sn(
        sn_identity: P2pIdentityRef,
        identity_factory: Arc<X509IdentityFactory>,
        cert_factory: Arc<X509IdentityCertFactory>,
    ) -> SnServiceRef {
        let service = create_sn_service(SnServiceConfig::new(
            sn_identity,
            identity_factory,
            cert_factory,
        ))
        .await;
        service.start().await.unwrap();
        service
    }

    async fn start_sn_client(
        net_manager: NetManagerRef,
        local_identity: P2pIdentityRef,
        sn_list: Vec<P2pSn>,
        cert_factory: P2pIdentityCertFactoryRef,
    ) -> SNClientServiceRef {
        let sn_service = SNClientService::new(
            net_manager,
            sn_list,
            local_identity,
            Arc::new(SequenceGenerator::new()),
            Arc::new(TunnelIdGenerator::new()),
            cert_factory,
            2,
            Duration::from_millis(200),
            Duration::from_secs(3),
            Duration::from_secs(3),
        );
        sn_service.start().await.unwrap();
        sn_service.wait_online(Some(ONLINE_TIMEOUT)).await.unwrap();
        sn_service
    }

    struct QueryDeviceFinder {
        sn_service: SNClientServiceRef,
        cert_factory: P2pIdentityCertFactoryRef,
        override_target: Option<P2pId>,
        override_eps: Vec<Endpoint>,
    }

    impl QueryDeviceFinder {
        fn new(
            sn_service: SNClientServiceRef,
            cert_factory: P2pIdentityCertFactoryRef,
            override_target: Option<P2pId>,
            override_eps: Vec<Endpoint>,
        ) -> Arc<Self> {
            Arc::new(Self {
                sn_service,
                cert_factory,
                override_target,
                override_eps,
            })
        }
    }

    #[async_trait]
    impl DeviceFinder for QueryDeviceFinder {
        async fn get_identity_cert(&self, device_id: &P2pId) -> P2pResult<P2pIdentityCertRef> {
            let resp = self.sn_service.query(device_id).await?;
            let peer_info = resp
                .peer_info
                .ok_or_else(|| p2p_err!(P2pErrorCode::NotFound, "device not found"))?;
            let cert = self.cert_factory.create(&peer_info)?;
            if self
                .override_target
                .as_ref()
                .map(|id| id == device_id)
                .unwrap_or(false)
                && !self.override_eps.is_empty()
            {
                Ok(cert.update_endpoints(self.override_eps.clone()))
            } else {
                Ok(cert.update_endpoints(resp.end_point_array))
            }
        }
    }

    struct TestStreamFactory {
        net_manager: NetManagerRef,
    }

    impl TestStreamFactory {
        fn new(net_manager: NetManagerRef) -> Self {
            Self { net_manager }
        }
    }

    #[async_trait]
    impl P2pConnectionFactory for TestStreamFactory {
        fn tunnel_type(&self) -> TunnelType {
            TunnelType::Stream
        }

        async fn create_connect(
            &self,
            local_identity: &P2pIdentityRef,
            remote: &Endpoint,
            remote_id: &P2pId,
            remote_name: Option<String>,
        ) -> P2pResult<Vec<P2pConnection>> {
            self.net_manager
                .get_network(remote.protocol())?
                .create_stream_connect(local_identity, remote, remote_id, remote_name)
                .await
        }

        async fn create_connect_with_local_ep(
            &self,
            local_identity: &P2pIdentityRef,
            local_ep: &Endpoint,
            remote: &Endpoint,
            remote_id: &P2pId,
            remote_name: Option<String>,
        ) -> P2pResult<P2pConnection> {
            self.net_manager
                .get_network(remote.protocol())?
                .create_stream_connect_with_local_ep(
                    local_identity,
                    local_ep,
                    remote,
                    remote_id,
                    remote_name,
                )
                .await
        }
    }

    struct TestDatagramFactory {
        net_manager: NetManagerRef,
    }

    impl TestDatagramFactory {
        fn new(net_manager: NetManagerRef) -> Self {
            Self { net_manager }
        }
    }

    #[async_trait]
    impl P2pConnectionFactory for TestDatagramFactory {
        fn tunnel_type(&self) -> TunnelType {
            TunnelType::Datagram
        }

        async fn create_connect(
            &self,
            local_identity: &P2pIdentityRef,
            remote: &Endpoint,
            remote_id: &P2pId,
            remote_name: Option<String>,
        ) -> P2pResult<Vec<P2pConnection>> {
            self.net_manager
                .get_network(remote.protocol())?
                .create_datagram_connect(local_identity, remote, remote_id, remote_name)
                .await
        }

        async fn create_connect_with_local_ep(
            &self,
            local_identity: &P2pIdentityRef,
            local_ep: &Endpoint,
            remote: &Endpoint,
            remote_id: &P2pId,
            remote_name: Option<String>,
        ) -> P2pResult<P2pConnection> {
            self.net_manager
                .get_network(remote.protocol())?
                .create_datagram_connect_with_local_ep(
                    local_identity,
                    local_ep,
                    remote,
                    remote_id,
                    remote_name,
                )
                .await
        }
    }

    struct SessionEventCollector {
        called: Arc<AtomicUsize>,
        tx: mpsc::UnboundedSender<(SessionId, u16)>,
    }

    #[async_trait]
    impl TunnelManagerEvent for SessionEventCollector {
        async fn on_new_session(
            &self,
            session_id: SessionId,
            vport: u16,
            _read: TunnelConnectionRead,
            _write: TunnelConnectionWrite,
        ) -> P2pResult<()> {
            self.called.fetch_add(1, Ordering::SeqCst);
            let _ = self.tx.send((session_id, vport));
            Ok(())
        }
    }

    struct TestNode<F: P2pConnectionFactory> {
        identity: P2pIdentityRef,
        net_manager: NetManagerRef,
        sn_service: SNClientServiceRef,
        tunnel_listener: TunnelListenerRef,
        manager: Arc<TunnelManager<F>>,
    }

    impl<F: P2pConnectionFactory> TestNode<F> {
        async fn stop(&self) {
            self.sn_service.stop().await;
            self.tunnel_listener.stop();
            let _ = self
                .net_manager
                .remove_listen_device(self.identity.get_name().as_str())
                .await;
        }
    }

    async fn setup_real_sn_pair_with_override(
        caller_override_eps: Option<Vec<Endpoint>>,
    ) -> (
        SnServiceRef,
        TestNode<TestStreamFactory>,
        TestNode<TestStreamFactory>,
    ) {
        let identity_factory = Arc::new(X509IdentityFactory);
        let cert_factory = Arc::new(X509IdentityCertFactory);
        crate::tls::init_tls(identity_factory.clone());

        let sn_identity = build_identity("sn-server", localhost_quic_endpoint(next_port()));
        let sn_service = start_sn(
            sn_identity.clone(),
            identity_factory.clone(),
            cert_factory.clone(),
        )
        .await;
        let sn_list = vec![build_sn_entry(&sn_identity)];

        let caller_identity = build_identity("tm-caller", localhost_quic_endpoint(next_port()));
        let callee_identity = build_identity("tm-callee", localhost_quic_endpoint(next_port()));

        let caller_net = create_net_manager(
            *caller_identity.endpoints().first().unwrap(),
            cert_factory.clone(),
        )
        .await;
        let callee_net = create_net_manager(
            *callee_identity.endpoints().first().unwrap(),
            cert_factory.clone(),
        )
        .await;

        caller_net
            .add_listen_device(caller_identity.clone())
            .await
            .unwrap();
        callee_net
            .add_listen_device(callee_identity.clone())
            .await
            .unwrap();

        let caller_sn = start_sn_client(
            caller_net.clone(),
            caller_identity.clone(),
            sn_list.clone(),
            cert_factory.clone(),
        )
        .await;
        let callee_sn = start_sn_client(
            callee_net.clone(),
            callee_identity.clone(),
            sn_list,
            cert_factory.clone(),
        )
        .await;

        let caller_override_eps = caller_override_eps.unwrap_or_default();
        let caller_finder = QueryDeviceFinder::new(
            caller_sn.clone(),
            cert_factory.clone(),
            if caller_override_eps.is_empty() {
                None
            } else {
                Some(callee_identity.get_id())
            },
            caller_override_eps,
        );
        let callee_finder =
            QueryDeviceFinder::new(callee_sn.clone(), cert_factory.clone(), None, vec![]);

        let caller_listener = TunnelListener::new(
            caller_identity.clone(),
            caller_net.clone(),
            caller_sn.clone(),
            None,
            0,
            cert_factory.clone(),
            CONN_TIMEOUT,
        );
        caller_listener.listen();
        let callee_listener = TunnelListener::new(
            callee_identity.clone(),
            callee_net.clone(),
            callee_sn.clone(),
            None,
            0,
            cert_factory.clone(),
            CONN_TIMEOUT,
        );
        callee_listener.listen();

        let caller_manager = TunnelManager::new(
            caller_sn.clone(),
            caller_identity.clone(),
            caller_finder,
            cert_factory.clone(),
            None,
            Arc::new(TunnelIdGenerator::new()),
            TestStreamFactory::new(caller_net.clone()),
            caller_listener.clone(),
            caller_net.get_connection_info_cache().clone(),
            0,
            CONN_TIMEOUT,
            IDLE_TIMEOUT,
        );

        let callee_manager = TunnelManager::new(
            callee_sn.clone(),
            callee_identity.clone(),
            callee_finder,
            cert_factory,
            None,
            Arc::new(TunnelIdGenerator::new()),
            TestStreamFactory::new(callee_net.clone()),
            callee_listener.clone(),
            callee_net.get_connection_info_cache().clone(),
            0,
            CONN_TIMEOUT,
            IDLE_TIMEOUT,
        );

        (
            sn_service,
            TestNode {
                identity: caller_identity,
                net_manager: caller_net,
                sn_service: caller_sn,
                tunnel_listener: caller_listener,
                manager: caller_manager,
            },
            TestNode {
                identity: callee_identity,
                net_manager: callee_net,
                sn_service: callee_sn,
                tunnel_listener: callee_listener,
                manager: callee_manager,
            },
        )
    }

    async fn setup_real_sn_pair() -> (
        SnServiceRef,
        TestNode<TestStreamFactory>,
        TestNode<TestStreamFactory>,
    ) {
        setup_real_sn_pair_with_override(Some(vec![localhost_quic_endpoint(next_port())])).await
    }

    async fn setup_real_sn_pair_without_override() -> (
        SnServiceRef,
        TestNode<TestStreamFactory>,
        TestNode<TestStreamFactory>,
    ) {
        setup_real_sn_pair_with_override(None).await
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn sn_reverse_session_real_path_triggers_listener() {
        let (sn_service, caller, callee) = setup_real_sn_pair().await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        let called = Arc::new(AtomicUsize::new(0));
        callee.manager.set_listener(SessionEventCollector {
            called: called.clone(),
            tx,
        });

        let vport = 31001;
        callee.manager.add_listen_port(vport);
        let session_id = Default::default();

        let ret = caller
            .manager
            .create_session_from_id(&callee.identity.get_id(), session_id, vport)
            .await;
        assert!(ret.is_ok());

        let event = tokio::time::timeout(CALL_TIMEOUT, rx.recv()).await.unwrap();
        assert!(event.is_some());
        let (event_session, event_vport) = event.unwrap();
        assert_eq!(event_session, session_id);
        assert_eq!(event_vport, vport);
        assert_eq!(called.load(Ordering::SeqCst), 1);

        caller.stop().await;
        callee.stop().await;
        sn_service.stop();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn create_session_real_path_triggers_listener() {
        let (sn_service, caller, callee) = setup_real_sn_pair().await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        let called = Arc::new(AtomicUsize::new(0));
        callee.manager.set_listener(SessionEventCollector {
            called: called.clone(),
            tx,
        });

        let vport = 31101;
        callee.manager.add_listen_port(vport);
        let session_gen = crate::types::SessionIdGenerator::new();
        let session_id = session_gen.generate();
        let callee_cert = callee.identity.get_identity_cert().unwrap();

        let ret = caller
            .manager
            .create_session(&callee_cert, session_id, vport)
            .await;
        assert!(ret.is_ok());

        let event = tokio::time::timeout(CALL_TIMEOUT, rx.recv()).await.unwrap();
        assert!(event.is_some());
        let (event_session, event_vport) = event.unwrap();
        assert_eq!(event_session, session_id);
        assert_eq!(event_vport, vport);
        assert_eq!(called.load(Ordering::SeqCst), 1);

        caller.stop().await;
        callee.stop().await;
        sn_service.stop();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn create_session_reuses_existing_tunnel_for_second_session() {
        let (sn_service, caller, callee) = setup_real_sn_pair().await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        let called = Arc::new(AtomicUsize::new(0));
        callee.manager.set_listener(SessionEventCollector {
            called: called.clone(),
            tx,
        });

        let vport = 31102;
        callee.manager.add_listen_port(vport);
        let session_gen = crate::types::SessionIdGenerator::new();
        let first_session = session_gen.generate();
        let second_session = session_gen.generate();
        let callee_cert = callee.identity.get_identity_cert().unwrap();

        let first = caller
            .manager
            .create_session(&callee_cert, first_session, vport)
            .await
            .unwrap();
        let first_event = tokio::time::timeout(CALL_TIMEOUT, rx.recv()).await.unwrap();
        assert!(first_event.is_some());

        drop(first);
        tokio::time::sleep(Duration::from_millis(200)).await;

        let second = caller
            .manager
            .create_session(&callee_cert, second_session, vport)
            .await;
        assert!(second.is_ok());
        let second_event = tokio::time::timeout(CALL_TIMEOUT, rx.recv()).await.unwrap();
        assert!(second_event.is_some());

        assert_eq!(called.load(Ordering::SeqCst), 2);
        let caller_tunnels = caller.manager.get_tunnels(&callee.identity.get_id());
        let tunnel_count = {
            let state = caller_tunnels.state.lock().unwrap();
            state.tunnels.len()
        };
        assert_eq!(tunnel_count, 1);

        caller.stop().await;
        callee.stop().await;
        sn_service.stop();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn create_session_refreshes_stale_remote_cert_via_device_finder() {
        let (sn_service, caller, callee) = setup_real_sn_pair_without_override().await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        let called = Arc::new(AtomicUsize::new(0));
        callee.manager.set_listener(SessionEventCollector {
            called: called.clone(),
            tx,
        });

        let vport = 31103;
        callee.manager.add_listen_port(vport);
        let session_id = crate::types::SessionIdGenerator::new().generate();
        let stale_cert = callee
            .identity
            .get_identity_cert()
            .unwrap()
            .update_endpoints(vec![localhost_quic_endpoint(next_port())]);

        let ret = caller
            .manager
            .create_session(&stale_cert, session_id, vport)
            .await;
        assert!(ret.is_ok());

        let event = tokio::time::timeout(CALL_TIMEOUT, rx.recv()).await.unwrap();
        assert!(event.is_some());
        assert_eq!(called.load(Ordering::SeqCst), 1);

        caller.stop().await;
        callee.stop().await;
        sn_service.stop();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn sn_called_with_invalid_payload_real_path_does_not_fire_listener() {
        let (sn_service, caller, callee) = setup_real_sn_pair().await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        let called = Arc::new(AtomicUsize::new(0));
        callee.manager.set_listener(SessionEventCollector {
            called: called.clone(),
            tx,
        });

        callee.manager.add_listen_port(32001);

        let call_resp = caller
            .sn_service
            .call(
                0x4001u32.into(),
                Some(caller.identity.endpoints().as_slice()),
                &callee.identity.get_id(),
                TunnelType::Stream,
                b"invalid-payload".to_vec(),
            )
            .await
            .unwrap();
        assert_eq!(call_resp.result, P2pErrorCode::Ok.as_u8());

        let no_event = tokio::time::timeout(Duration::from_secs(2), rx.recv()).await;
        assert!(no_event.is_err());
        assert_eq!(called.load(Ordering::SeqCst), 0);

        caller.stop().await;
        callee.stop().await;
        sn_service.stop();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn sn_called_to_unlistened_port_real_path_does_not_fire_listener() {
        let (sn_service, caller, callee) = setup_real_sn_pair().await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        let called = Arc::new(AtomicUsize::new(0));
        callee.manager.set_listener(SessionEventCollector {
            called: called.clone(),
            tx,
        });

        let payload = SessionSnCall {
            vport: 32999,
            session_id: Default::default(),
        }
        .to_vec()
        .unwrap();

        let call_resp = caller
            .sn_service
            .call(
                0x4002u32.into(),
                Some(caller.identity.endpoints().as_slice()),
                &callee.identity.get_id(),
                TunnelType::Stream,
                payload,
            )
            .await
            .unwrap();
        assert_eq!(call_resp.result, P2pErrorCode::Ok.as_u8());

        let no_event = tokio::time::timeout(Duration::from_secs(2), rx.recv()).await;
        assert!(no_event.is_err());
        assert_eq!(called.load(Ordering::SeqCst), 0);

        caller.stop().await;
        callee.stop().await;
        sn_service.stop();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn on_sn_called_invalid_call_type_returns_invalid_param() {
        let (sn_service, caller, callee) = setup_real_sn_pair().await;
        let cert_factory: P2pIdentityCertFactoryRef = Arc::new(X509IdentityCertFactory);

        let datagram_finder = QueryDeviceFinder::new(
            callee.sn_service.clone(),
            cert_factory.clone(),
            None,
            vec![],
        );
        let datagram_manager = TunnelManager::new(
            callee.sn_service.clone(),
            callee.identity.clone(),
            datagram_finder,
            cert_factory,
            None,
            Arc::new(TunnelIdGenerator::new()),
            TestDatagramFactory::new(callee.net_manager.clone()),
            callee.tunnel_listener.clone(),
            callee.net_manager.get_connection_info_cache().clone(),
            0,
            CONN_TIMEOUT,
            IDLE_TIMEOUT,
        );

        let payload = SessionSnCall {
            vport: 33001,
            session_id: Default::default(),
        }
        .to_vec()
        .unwrap();
        let peer_info = caller
            .identity
            .get_identity_cert()
            .unwrap()
            .get_encoded_cert()
            .unwrap();
        let called = SnCalled {
            seq: 1u32.into(),
            sn_peer_id: P2pId::default(),
            to_peer_id: callee.identity.get_id(),
            reverse_endpoint_array: caller.identity.endpoints(),
            active_pn_list: vec![],
            peer_info,
            tunnel_id: 0x5001u32.into(),
            call_send_time: bucky_time_now(),
            call_type: TunnelType::Stream,
            payload,
        };

        let err = datagram_manager.on_sn_called(called).await.err().unwrap();
        assert_eq!(err.code(), P2pErrorCode::InvalidParam);

        caller.stop().await;
        callee.stop().await;
        sn_service.stop();
    }
}
