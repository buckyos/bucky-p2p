use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, RwLock, Weak};
use std::time::Duration;
use async_named_locker::Locker;
use bucky_raw_codec::{RawConvertTo, RawFrom};
use bucky_time::bucky_time_now;
use notify_future::Notify;
use tokio::io::AsyncWriteExt;
use crate::endpoint::Endpoint;
use crate::error::{p2p_err, P2pErrorCode, P2pResult, into_p2p_err};
use crate::executor::{Executor, SpawnHandle};
use crate::p2p_connection::{P2pConnection, P2pConnectionInfoCacheRef};
use crate::p2p_identity::{P2pId, P2pIdentityRef, P2pIdentityCertFactoryRef, P2pIdentityCertRef, P2pIdentityCertCacheRef};
use crate::pn::PnClientRef;
use crate::protocol::{Package, PackageCmdCode};
use crate::protocol::v0::{AckDatagram, AckReverseDatagram, AckReverseSession, AckSession, SnCalled, SynReverseSession, SynSession, TunnelType};
use crate::runtime;
use crate::sn::client::SNClientServiceRef;
use crate::sockets::{NetManagerRef};
use crate::tunnel::proxy_connection::{ProxyConnectionRead, ProxyConnectionWrite};
use crate::tunnel::tunnel_listener::{NewTunnelEvent, TunnelListenerRef};
use crate::types::{SessionId, TunnelId, TunnelIdGenerator};
use super::{ReverseWaiterCache, ReverseResult, SessionSnCall, Tunnel, TunnelConnection, TunnelSession, TunnelListenPorts, P2pConnectionFactory, TunnelConnectionRef, TunnelConnectionRead, TunnelConnectionWrite, TunnelState};

struct TunnelsState<F: P2pConnectionFactory> {
    tunnels: HashMap<TunnelId, Arc<Tunnel<F>>>,
    pending_tunnels: HashMap<TunnelId, Notify<ReverseResult>>,
}

impl<F: P2pConnectionFactory> TunnelsState<F> {
    pub fn get_idle_tunnel(&mut self) -> Option<Arc<Tunnel<F>>> {
        for (_, tunnel) in self.tunnels.iter() {
            if tunnel.is_idle() {
                return Some(tunnel.clone())
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
    async fn on_new_session(&self, session: SynSession, read: TunnelConnectionRead, write: TunnelConnectionWrite) -> P2pResult<()>;
}

struct Tunnels<F: P2pConnectionFactory> {
    listener: Arc<dyn NewSessionEvent>,
    state: Mutex<TunnelsState<F>>
}

impl<F: P2pConnectionFactory> Drop for Tunnels<F> {
    fn drop(&mut self) {
    }
}
impl<F: P2pConnectionFactory> Tunnels<F> {
    pub fn new(listener: impl NewSessionEvent) -> Arc<Self> {
        Arc::new(Self {
            listener: Arc::new(listener),
            state: Mutex::new(TunnelsState {
                tunnels: Default::default(),
                pending_tunnels: Default::default(),
            })
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

                                    if let Err(e) = this.listener.on_new_session(session, read, write).await {
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

    pub async fn close_all_tunnel(&self) {
        let tunnels = {
            let state = self.state.lock().unwrap();
            state.tunnels.iter().map(|v| v.1.clone()).collect::<Vec<Arc<Tunnel<F>>>>()
        };
    }
}

impl<F: P2pConnectionFactory> ReverseWaiterCache for Tunnels<F> {
    fn add_reverse_waiter(&self, sequence: TunnelId, future: Notify<ReverseResult>) {
        self.add_pending_future(sequence, future);
    }

    fn remove_reverse_waiter(&self, sequence: TunnelId) {
        self.try_pop_pending_future(sequence);
    }
}

struct ListenPorts {
    listen_ports: Mutex<HashSet<u16>>,
}

impl ListenPorts {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            listen_ports: Mutex::new(Default::default())
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
    pub fn new(sn_service: SNClientServiceRef, cert_factory: P2pIdentityCertFactoryRef, cert_cache: P2pIdentityCertCacheRef, interval: Duration) -> Arc<Self> {
        Arc::new(Self {
            cert_cache,
            sn_service,
            cert_factory,
            query_cache: mini_moka::sync::Cache::builder().time_to_live(interval).build(),
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
    async fn on_new_tunnel(&self, session: SynSession, conn: TunnelConnectionRef, read: TunnelConnectionRead, write: TunnelConnectionWrite) -> P2pResult<()> {
        if let Some(manager) = self.manager.upgrade() {
            manager.on_new_tunnel(session, Some(conn), read, write).await?;
        }
        Ok(())
    }

    async fn on_new_reverse_tunnel(&self, session: SynReverseSession, conn: TunnelConnectionRef, read: TunnelConnectionRead, write: TunnelConnectionWrite) -> P2pResult<()> {
        if let Some(manager) = self.manager.upgrade() {
            manager.on_new_reverse_tunnel(session, conn, read, write).await?;
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
    async fn on_new_session(&self, session_id: SessionId, vport: u16, read: TunnelConnectionRead, write: TunnelConnectionWrite) -> P2pResult<()>;
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
        let local_id = local_identity.get_id();
        let handle = Executor::spawn_with_handle(async move {
            loop {
                runtime::sleep(Duration::from_secs(120)).await;
                // log::info!("{} start clear idle tunnel", local_id);
                Self::clear_idle_tunnel(tmp.clone()).await;
            }
        }).unwrap();
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

        tunnel_listener.register_new_tunnel_event(tunnel_type, NewTunnelListener::new(manager.clone()));

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

    async fn clear_idle_tunnel(tunnels: Arc<RwLock<HashMap<P2pId, Arc<Tunnels<F>>>>>) {
        let tunnels_map = tunnels.read().unwrap();
        for (_, tunnels) in tunnels_map.iter() {
            let mut remove_list = Vec::new();
            {
                let state = tunnels.state.lock().unwrap();
                for (_, tunnel) in state.tunnels.iter() {
                    let tunnel_stat = tunnel.tunnel_stat();
                    if tunnel.is_error() || (tunnel.is_idle() && tunnel_stat.get_work_instance_num() == 0 && bucky_time_now() - tunnel_stat.get_latest_active_time() > 250 * 1000 * 1000) {
                        remove_list.push(tunnel.clone());
                    }
                }
            }
            for tunnel in remove_list.into_iter() {
                let seq = tunnel.get_tunnel_id();
                let mut state = tunnels.state.lock().unwrap();
                state.tunnels.remove(&seq);
            }
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

    async fn on_new_tunnel(self: Arc<Self>,
                           session: SynSession,
                           conn: Option<TunnelConnectionRef>,
                           read: TunnelConnectionRead,
                           mut write: TunnelConnectionWrite) -> P2pResult<()> {
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
                self.conn_info_cache.clone()
            );
            tunnel.set_tunnel_conn(conn);
            tunnels.add_tunnel(tunnel);
        }

        let mut result = 0;
        if !self.listen_ports.is_listen(session.to_vport) {
            result = P2pErrorCode::PortNotListen as u8;
        }

        let pkg = Package::new(self.protocol_version, PackageCmdCode::AckSession, AckSession {
            result,
        });
        write.send_pkg(pkg).await?;

        if let Some(tunnel) = tunnels.get_tunnel(session.tunnel_id) {
            tunnel.set_tunnel_state(TunnelState::Worked);
        }
        if result == 0 {
            let listener = {
                self.listener.lock().unwrap().clone()
            };
            if let Some(listener) = listener {
                listener.on_new_session(session.session_id, session.to_vport, read, write).await?;
            }
        } else {

        }
        Ok(())
    }

    async fn on_new_reverse_tunnel(self: &Arc<Self>,
                                   session: SynReverseSession,
                                   conn: TunnelConnectionRef,
                                   read: TunnelConnectionRead,
                                   mut write: TunnelConnectionWrite) -> P2pResult<()> {
        let remote_id = read.remote_id();
        let tunnels = self.get_tunnels(&remote_id);
        let _locker = Locker::get_locker(format!("tunnel_{}_{:?}", remote_id.to_string(), conn.get_tunnel_id())).await;
        if tunnels.is_exist_pending_future(conn.get_tunnel_id()) {
            let ack = AckReverseSession {
                result: 0
            };
            let pkg = Package::new(self.protocol_version, PackageCmdCode::AckReverseSession, ack);
            match write.send_pkg(pkg).await {
                Ok(_) => {
                    log::info!("new tunnel {:?} remote_id {} remote_ep {} local_id {} local_ep {}",
                            conn.get_tunnel_id(),
                            remote_id.to_string(),
                            read.remote().to_string(),
                            read.local_id().to_string(),
                            read.local().to_string());
                    let future = tunnels.try_pop_pending_future(conn.get_tunnel_id()).unwrap();
                    future.notify(ReverseResult::Session(session.result, conn, read, write));
                    Ok(())
                }
                Err(e) => {
                    log::error!("write tunnel {:?} ack reverse stream error: {:?}", conn.get_tunnel_id(), e);
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
            let device_tunnels = Tunnels::new(move |session: SynSession, read: TunnelConnectionRead, write: TunnelConnectionWrite| {
                let this = this.clone();
                async move {
                    this.on_new_tunnel(session, None, read, write).await?;
                    Ok(())
                }
            });
            tunnels.insert(remote_id.clone(), device_tunnels.clone());
            device_tunnels
        } else {
            device_tunnels.unwrap().clone()
        }
    }

    pub async fn create_session(self: &Arc<Self>, remote: &P2pIdentityCertRef, session_id: SessionId, vport: u16) -> P2pResult<(TunnelConnectionRead, TunnelConnectionWrite)> {
        let remote_id = remote.get_id();
        let tunnels = self.get_tunnels(&remote_id);

        if let Some(tunnel) = tunnels.get_idle_tunnel() {
            tunnel.open_session(vport, session_id).await
        } else {
            let seq = self.gen_id.generate();

            let remote = match self.device_finder.get_identity_cert(&remote.get_id()).await {
                Ok(remote) => remote,
                Err(_) => {
                    remote.clone()
                }
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
                self.conn_info_cache.clone()
            );
            let ret = tunnel.connect_session(vport, session_id, tunnels.clone()).await;
            if tunnel.is_work() {
                tunnels.add_tunnel(tunnel);
            }
            ret
        }
    }

    pub async fn create_session_from_id(self: &Arc<Self>, remote_id: &P2pId, session_id: SessionId, vport: u16) -> P2pResult<(TunnelConnectionRead, TunnelConnectionWrite)> {
        let tunnels = self.get_tunnels(remote_id);
        if let Some(tunnel) = tunnels.get_idle_tunnel() {
            tunnel.open_session(vport, session_id).await
        } else {
            let remote = self.device_finder.get_identity_cert(remote_id).await?;
            let seq = self.gen_id.generate();
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
                self.conn_info_cache.clone()
            );
            let ret = tunnel.connect_session(vport, session_id, tunnels.clone()).await;
            if tunnel.is_work() {
                tunnels.add_tunnel(tunnel);
            }
            ret
        }
    }

    pub async fn create_session_direct(self: &Arc<Self>, remote_id: &P2pId, remote_eps: Vec<Endpoint>, session_id: SessionId, vport: u16) -> P2pResult<(TunnelConnectionRead, TunnelConnectionWrite)> {
        let remote_name = match self.device_finder.get_identity_cert(remote_id).await {
            Ok(remote) => {
                remote.get_name()
            },
            Err(_) => {
                remote_id.to_string()
            }
        };

        let seq = self.gen_id.generate();
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
            self.conn_info_cache.clone()
        );
        let ret = tunnel.connect_session_direct(vport,  session_id).await;
        if tunnel.is_work() {
            let tunnels = self.get_tunnels(remote_id);
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

    async fn on_sn_called_inner(self: &Arc<Self>, sn_called: SnCalled, cert: P2pIdentityCertRef) -> P2pResult<()> {
        log::info!("on_sn_called {:?}", sn_called);

        let eps = if self.sn_service.is_same_lan(&sn_called.reverse_endpoint_array) {
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
        assert_eq!(sn_called.call_type, self.p2p_factory.tunnel_type());

        let session_info = SessionSnCall::clone_from_slice(sn_called.payload.as_slice()).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
        let mut result = 0;
        if !self.listen_ports.is_listen(session_info.vport) {
            result = P2pErrorCode::PortNotListen as u8;
        }
        let remote_cert = self.cert_factory.create(&sn_called.peer_info)?;
        let mut tunnel = Tunnel::new(
            self.sn_service.clone(),
            sn_called.tunnel_id,
            self.protocol_version,
            remote_cert.get_id(),
            eps.clone(),
            Some(remote_cert.get_name()),
            self.local_identity.clone(),
            self.conn_timeout,
            self.idle_timeout,
            self.cert_factory.clone(),
            self.pn_client.clone(),
            self.p2p_factory.clone(),
            self.conn_info_cache.clone());

        let session_call = SessionSnCall::clone_from_slice(sn_called.payload.as_slice()).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
        let ret = tunnel.connect_reverse_session(session_call.vport, session_call.session_id, result).await;
        if tunnel.is_work() {
            let tunnels = self.get_tunnels(&from_device_id);
            tunnels.add_tunnel(tunnel);
        }
        match ret {
            Ok((read, write)) => {
                if result == 0 {
                    let listener = {
                        self.listener.lock().unwrap().clone()
                    };
                    if let Some(listener) = listener {
                        listener.on_new_session(session_call.session_id, session_call.vport, read, write).await?;
                    }
                }
                Ok(())
            }
            Err(e) => {
                Err(e)
            }
        }
    }
}

impl<F: P2pConnectionFactory> Drop for TunnelManager<F> {
    fn drop(&mut self) {
        log::info!("tunnel manager drop.device {}", self.local_identity.get_id().to_string());
        self.tunnel_listener.remove_new_tunnel_event(self.p2p_factory.tunnel_type());
        self.clear_handle.abort();
        Executor::block_on(self.close_all_tunnel());
    }
}
