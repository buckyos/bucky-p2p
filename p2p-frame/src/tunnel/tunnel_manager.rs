use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use bucky_raw_codec::{RawFrom};
use bucky_time::bucky_time_now;
use notify_future::NotifyFuture;
use crate::error::{p2p_err, P2pErrorCode, P2pResult, into_p2p_err};
use crate::executor::{Executor, SpawnHandle};
use crate::p2p_connection::P2pConnectionRef;
use crate::p2p_identity::{P2pId, P2pIdentityRef, P2pIdentityCertFactoryRef, P2pIdentityCertRef, P2pIdentityCertCacheRef};
use crate::protocol::v0::SnCalled;
use crate::runtime;
use crate::sn::client::SNClientServiceRef;
use crate::sockets::{NetManagerRef};
use crate::types::{IncreaseId, TempSeq, TempSeqGenerator};
use super::{ReverseFutureCache, ReverseResult, StreamSnCall, Tunnel, TunnelConnection, TunnelDatagramRecv, TunnelDatagramSend, TunnelInstance, TunnelListenPorts, TunnelStream};

struct TunnelsState {
    tunnels: HashMap<TempSeq, Arc<Tunnel>>,
    pending_tunnels: HashMap<TempSeq, NotifyFuture<ReverseResult>>,
}

impl TunnelsState {
    pub fn get_idle_tunnel(&mut self) -> Option<Arc<Tunnel>> {
        for (_, tunnel) in self.tunnels.iter() {
            if tunnel.is_idle() {
                return Some(tunnel.clone())
            }
        }
        None
    }

    pub fn tunnel_exist(&mut self, seq: TempSeq) -> bool {
        self.tunnels.contains_key(&seq)
    }

    pub fn remove_tunnel(&mut self, seq: TempSeq) {
        self.tunnels.remove(&seq);
    }

    pub fn add_pending_future(&mut self, seq: TempSeq, future: NotifyFuture<ReverseResult>) {
        self.pending_tunnels.insert(seq, future);
    }

    pub fn try_pop_pending_future(&mut self, seq: TempSeq) -> Option<NotifyFuture<ReverseResult>> {
        self.pending_tunnels.remove(&seq)
    }
}

struct Tunnels {
    state: Mutex<TunnelsState>
}

impl Drop for Tunnels {
    fn drop(&mut self) {
    }
}
impl Tunnels {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(TunnelsState {
                tunnels: Default::default(),
                pending_tunnels: Default::default(),
            })
        })
    }

    pub fn get_idle_tunnel(&self) -> Option<Arc<Tunnel>> {
        let mut state = self.state.lock().unwrap();
        state.get_idle_tunnel()
    }

    pub fn add_tunnel(self: &Arc<Self>, tunnel: Tunnel, listener: TunnelManagerEventRef) -> bool {
        let tunnel = Arc::new(tunnel);
        {
            let mut state = self.state.lock().unwrap();
            if state.tunnels.contains_key(&tunnel.get_sequence()) {
                return false;
            }
            state.tunnels.insert(tunnel.get_sequence(), tunnel.clone());
        }
        let this = self.clone();
        let _ = Executor::spawn(async move {
            loop {
                match tunnel.accept_instance().await {
                    Ok(instance) => {
                        match instance {
                            TunnelInstance::Stream(stream) => {
                                log::info!("accept stream tunnel {:?} stream_id {} port {} remote_id {} remote_ep {} local_id {} local_ep {}",
                                    stream.sequence(),
                                    stream.session_id(),
                                    stream.port(),
                                    stream.remote_identity_id().to_string(),
                                    stream.remote_endpoint().to_string(),
                                    stream.local_identity_id().to_string(),
                                    stream.local_endpoint().to_string());

                                    listener.on_new_tunnel_stream(stream).await;
                            }
                            TunnelInstance::Datagram(datagram) => {
                                log::info!("accept datagram tunnel {:?} remote_id {} remote_ep {} local_id {} local_ep {}",
                                    datagram.sequence(),
                                    datagram.remote_identity_id().to_string(),
                                    datagram.remote_endpoint().to_string(),
                                    datagram.local_identity_id().to_string(),
                                    datagram.local_endpoint().to_string());

                                    listener.on_new_tunnel_datagram(datagram).await;
                            }
                            TunnelInstance::ReverseStream(_stream) => {

                            }
                            TunnelInstance::ReverseDatagram(_datagram) => {

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
            state.tunnels.remove(&tunnel.get_sequence());
        });
        true
    }

    pub fn tunnel_exist(&self, seq: TempSeq) -> bool {
        let mut state = self.state.lock().unwrap();
        state.tunnel_exist(seq)
    }

    pub fn remove_tunnel(&self, seq: TempSeq) {
        let mut state = self.state.lock().unwrap();
        state.remove_tunnel(seq);
    }

    pub fn try_pop_pending_future(&self, seq: TempSeq) -> Option<NotifyFuture<ReverseResult>> {
        let mut state = self.state.lock().unwrap();
        state.try_pop_pending_future(seq)
    }

    pub fn add_pending_future(&self, seq: TempSeq, future: NotifyFuture<ReverseResult>) {
        let mut state = self.state.lock().unwrap();
        state.add_pending_future(seq, future);
    }

    pub async fn close_all_tunnel(&self) {
        let tunnels = {
            let state = self.state.lock().unwrap();
            state.tunnels.iter().map(|v| v.1.clone()).collect::<Vec<Arc<Tunnel>>>()
        };

        for tunnel in tunnels.into_iter() {
            let _ = tunnel.shutdown().await;
        }
    }
}

impl ReverseFutureCache for Tunnels {
    fn add_reverse_future(&self, sequence: TempSeq, future: NotifyFuture<ReverseResult>) {
        self.add_pending_future(sequence, future);
    }

    fn remove_reverse_future(&self, sequence: TempSeq) {
        self.try_pop_pending_future(sequence);
    }
}

pub struct TunnelManagerEvent {
    stream_event: Mutex<Option<TunnelManagerStreamEventRef>>,
    datagram_event: Mutex<Option<TunnelManagerDatagramEventRef>>,
}
pub type TunnelManagerEventRef = Arc<TunnelManagerEvent>;

impl TunnelManagerEvent {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            stream_event: Mutex::new(None),
            datagram_event: Mutex::new(None),
        })
    }

    fn set_stream_event_listener(&self, listener: impl TunnelManagerStreamEvent) {
        let mut stream_event = self.stream_event.lock().unwrap();
        *stream_event = Some(Arc::new(listener));
    }

    fn set_datagram_event_listener(&self, listener: impl TunnelManagerDatagramEvent) {
        let mut datagram_event = self.datagram_event.lock().unwrap();
        *datagram_event = Some(Arc::new(listener));
    }

    async fn on_new_tunnel_stream(&self, tunnel: TunnelStream) {
        let stream_event = {
            self.stream_event.lock().unwrap().clone()
        };
        if stream_event.is_some() {
            stream_event.as_ref().unwrap().on_new_tunnel_stream(tunnel).await;
        }
    }

    async fn on_new_tunnel_datagram(&self, tunnel: TunnelDatagramRecv) {
        let datagram_event = {
            self.datagram_event.lock().unwrap().clone()
        };
        if datagram_event.is_some() {
            datagram_event.as_ref().unwrap().on_new_tunnel_datagram(tunnel).await;
        }
    }
}

#[callback_trait::callback_trait]
pub trait TunnelManagerStreamEvent: 'static + Send + Sync {
    async fn on_new_tunnel_stream(&self, tunnel: TunnelStream) -> ();
}
pub type TunnelManagerStreamEventRef = Arc<dyn TunnelManagerStreamEvent>;

#[callback_trait::callback_trait]
pub trait TunnelManagerDatagramEvent: 'static + Send + Sync {
    async fn on_new_tunnel_datagram(&self, tunnel: TunnelDatagramRecv) -> ();
}
pub type TunnelManagerDatagramEventRef = Arc<dyn TunnelManagerDatagramEvent>;

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
}

impl DefaultDeviceFinder {
    pub fn new(sn_service: SNClientServiceRef, cert_factory: P2pIdentityCertFactoryRef, cert_cache: P2pIdentityCertCacheRef) -> Arc<Self> {
        Arc::new(Self {
            cert_cache,
            sn_service,
            cert_factory,
        })
    }
}

#[async_trait::async_trait]
impl DeviceFinder for DefaultDeviceFinder {
    async fn get_identity_cert(&self, device_id: &P2pId) -> P2pResult<P2pIdentityCertRef> {
        if let Some(device) = self.cert_cache.get(device_id).await {
            return Ok(device);
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

pub struct TunnelManager {
    tunnels: Arc<RwLock<HashMap<P2pId, Arc<Tunnels>>>>,
    net_manager: NetManagerRef,
    sn_service: SNClientServiceRef,
    local_identity: P2pIdentityRef,
    protocol_version: u8,
    stack_version: u32,
    listener: TunnelManagerEventRef,
    conn_timeout: Duration,
    idle_timeout: Duration,
    gen_seq: Arc<TempSeqGenerator>,
    listen_ports: Arc<ListenPorts>,
    clear_handle: SpawnHandle<()>,
    device_finder: DeviceFinderRef,
    cert_factory: P2pIdentityCertFactoryRef,
}
pub type TunnelManagerRef = Arc<TunnelManager>;

impl TunnelManager {
    pub fn new(
        net_manager: NetManagerRef,
        sn_service: SNClientServiceRef,
        local_identity: P2pIdentityRef,
        device_finder: DeviceFinderRef,
        cert_factory: P2pIdentityCertFactoryRef,
        protocol_version: u8,
        stack_version: u32,
        conn_timeout: Duration,
        idle_timeout: Duration,) -> Arc<Self> {
        let tunnels = Arc::new(RwLock::new(HashMap::<P2pId, Arc<Tunnels>>::new()));
        let tmp = tunnels.clone();
        let handle = Executor::spawn_with_handle(async move {
            loop {
                runtime::sleep(Duration::from_secs(120)).await;
                Self::clear_idle_tunnel(tmp.clone()).await;
            }
        }).unwrap();


        let manager = Arc::new(Self {
            tunnels,
            net_manager: net_manager.clone(),
            sn_service,
            local_identity,
            protocol_version,
            stack_version,
            listener: TunnelManagerEvent::new(),
            conn_timeout,
            idle_timeout,
            gen_seq: Arc::new(TempSeqGenerator::new()),
            listen_ports: ListenPorts::new(),
            clear_handle: handle,
            device_finder,
            cert_factory,
        });

        let tmp_manager = manager.clone();
        net_manager.set_connection_event_listener(manager.local_identity.get_id(), move |conn| {
            let tmp_manager = tmp_manager.clone();
            async move {
                tmp_manager.on_new_tunnel(conn).await
            }
        });

        manager
    }

    async fn clear_idle_tunnel(tunnels: Arc<RwLock<HashMap<P2pId, Arc<Tunnels>>>>) {
        let tunnels_map = tunnels.read().unwrap();
        for (_, tunnels) in tunnels_map.iter() {
            let mut remove_list = Vec::new();
            {
                let state = tunnels.state.lock().unwrap();
                for (_, tunnel) in state.tunnels.iter() {
                    let tunnel_stat = tunnel.tunnel_stat();
                    if tunnel_stat.get_work_instance_num() == 0 && bucky_time_now() - tunnel_stat.get_latest_active_time() > 250 * 1000 * 1000 {
                        remove_list.push(tunnel.clone());
                    }
                }
            }
            for tunnel in remove_list.into_iter() {
                let seq = tunnel.get_sequence();
                Executor::spawn_ok(async move {
                    if let Err(e) = tunnel.shutdown().await {
                        log::error!("shutdown tunnel error: {:?}", e);
                    }
                });
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

    async fn on_new_tunnel(&self, socket: P2pConnectionRef) -> P2pResult<()> {
        let remote_id = socket.remote_id().clone();
        let remote_ep = socket.remote().clone();
        let local_id = socket.local_id().clone();
        let local_ep = socket.local().clone();
        let net_manager = self.net_manager.clone();
        let sn_service = self.sn_service.clone();
        let protocol_version = self.protocol_version;
        let stack_version = self.stack_version;
        let local_identity = self.local_identity.clone();
        let conn_timeout = self.conn_timeout;
        let idle_timeout = self.idle_timeout;

        let listen_ports = self.listen_ports.clone();

        let tunnel_conn = TunnelConnection::new(
            TempSeq::from(0),
            self.local_identity.clone(),
            remote_id.clone(),
            socket.remote().clone(),
            self.conn_timeout,
            self.protocol_version,
            self.stack_version,
            Some(socket),
            self.listen_ports.clone(),
            self.cert_factory.clone())?;

        let tunnels = self.get_tunnels(&remote_id);
        let listener = self.listener.clone();
        let cert_factory = self.cert_factory.clone();
        let _ = Executor::spawn(async move {
            match tunnel_conn.accept_instance().await {
                Ok(instance) => {
                    log::info!("new tunnel {} remote_id {} remote_ep {} local_id {} local_ep {}",
                        instance,
                        remote_id.to_string(),
                        remote_ep.to_string(),
                        local_id.to_string(),
                        local_ep.to_string());
                    match instance {
                        TunnelInstance::Stream(stream) => {
                            let mut tunnel = Tunnel::new(
                                net_manager.clone(),
                                sn_service.clone(),
                                tunnel_conn.get_sequence(),
                                protocol_version,
                                stack_version,
                                remote_id.clone(),
                                vec![],
                                local_identity.clone(),
                                conn_timeout,
                                idle_timeout,
                                listen_ports.clone(),
                                cert_factory.clone());
                            tunnel.set_tunnel_conn(tunnel_conn);
                            if tunnels.add_tunnel(tunnel, listener.clone()) {
                                listener.on_new_tunnel_stream(stream).await;
                            }
                        }
                        TunnelInstance::Datagram(datagram) => {
                            let mut tunnel = Tunnel::new(
                                net_manager.clone(),
                                sn_service.clone(),
                                tunnel_conn.get_sequence(),
                                protocol_version,
                                stack_version,
                                remote_id.clone(),
                                vec![],
                                local_identity.clone(),
                                conn_timeout,
                                idle_timeout,
                                listen_ports.clone(),
                                cert_factory.clone());
                            tunnel.set_tunnel_conn(tunnel_conn);
                            if tunnels.add_tunnel(tunnel, listener.clone()) {
                                listener.on_new_tunnel_datagram(datagram).await;
                            }
                        }
                        TunnelInstance::ReverseStream(stream) => {
                            if let Some(future) = tunnels.try_pop_pending_future(tunnel_conn.get_sequence()) {
                                future.set_complete(ReverseResult::Stream(tunnel_conn, stream));
                            }
                        }
                        TunnelInstance::ReverseDatagram(datagram) => {
                            if let Some(future) = tunnels.try_pop_pending_future(tunnel_conn.get_sequence()) {
                                future.set_complete(ReverseResult::Datagram(tunnel_conn, datagram));
                            }
                        }
                    }
                }
                Err(e) => {
                    log::error!("accept tunnel error: {:?}", e);
                }
            }
        });

        Ok(())
    }

    pub fn set_stream_listener(&self, listener: impl TunnelManagerStreamEvent) {
        self.listener.set_stream_event_listener(listener);
    }

    pub fn set_datagram_listener(&self, listener: impl TunnelManagerDatagramEvent) {
        self.listener.set_datagram_event_listener(listener);
    }

    pub fn add_listen_port(&self, port: u16) {
        self.listen_ports.add_listen_port(port)
    }

    pub fn remove_listen_port(&self, port: u16) {
        self.listen_ports.remove_listen_port(port)
    }

    fn get_tunnels(&self, remote_id: &P2pId) -> Arc<Tunnels> {
        let mut tunnels = self.tunnels.write().unwrap();
        let device_tunnels = tunnels.get(remote_id);
        if device_tunnels.is_none() {
            let device_tunnels = Tunnels::new();
            tunnels.insert(remote_id.clone(), device_tunnels.clone());
            device_tunnels
        } else {
            device_tunnels.unwrap().clone()
        }
    }

    pub async fn create_datagram_tunnel(&self, remote: &P2pIdentityCertRef) -> P2pResult<TunnelDatagramSend> {
        let remote_id = remote.get_id();
        let tunnels = self.get_tunnels(&remote_id);

        if let Some(tunnel) = tunnels.get_idle_tunnel() {
            return tunnel.open_datagram().await;
        } else {
            let seq = self.gen_seq.generate();
            let mut tunnel = Tunnel::new(
                self.net_manager.clone(),
                self.sn_service.clone(),
                seq,
                self.protocol_version,
                self.stack_version,
                remote.get_id(),
                remote.endpoints(),
                self.local_identity.clone(),
                self.conn_timeout,
                self.idle_timeout,
                self.listen_ports.clone(),
                self.cert_factory.clone(),
            );
            let datagram = tunnel.connect_datagram(tunnels.clone()).await?;
            let listener = self.listener.clone();
            tunnels.add_tunnel(tunnel, listener);
            Ok(datagram)
        }
    }

    pub async fn create_datagram_tunnel_from_id(&self, remote_id: &P2pId) -> P2pResult<TunnelDatagramSend> {
        let tunnels = self.get_tunnels(remote_id);
        if let Some(tunnel) = tunnels.get_idle_tunnel() {
            return tunnel.open_datagram().await;
        } else {
            let remote = self.device_finder.get_identity_cert(remote_id).await?;
            let seq = self.gen_seq.generate();
            let mut tunnel = Tunnel::new(
                self.net_manager.clone(),
                self.sn_service.clone(),
                seq,
                self.protocol_version,
                self.stack_version,
                remote_id.clone(),
                remote.endpoints(),
                self.local_identity.clone(),
                self.conn_timeout,
                self.idle_timeout,
                self.listen_ports.clone(),
                self.cert_factory.clone()
            );
            let datagram = tunnel.connect_datagram(tunnels.clone()).await?;
            let listener = self.listener.clone();
            tunnels.add_tunnel(tunnel, listener);
            Ok(datagram)
        }
    }

    pub async fn create_stream_tunnel(&self, remote: &P2pIdentityCertRef, session_id: IncreaseId, vport: u16) -> P2pResult<TunnelStream> {
        let remote_id = remote.get_id();
        let tunnels = self.get_tunnels(&remote_id);

        if let Some(tunnel) = tunnels.get_idle_tunnel() {
            return tunnel.open_stream(vport, session_id).await;
        } else {
            let seq = self.gen_seq.generate();
            let mut tunnel = Tunnel::new(
                self.net_manager.clone(),
                self.sn_service.clone(),
                seq,
                self.protocol_version,
                self.stack_version,
                remote.get_id(),
                remote.endpoints(),
                self.local_identity.clone(),
                self.conn_timeout,
                self.idle_timeout,
                self.listen_ports.clone(),
                self.cert_factory.clone(),
            );
            let stream = tunnel.connect_stream(vport, session_id, tunnels.clone()).await?;
            let listener = self.listener.clone();
            tunnels.add_tunnel(tunnel, listener);
            Ok(stream)
        }
    }

    pub async fn create_stream_tunnel_from_id(&self, remote_id: &P2pId, session_id: IncreaseId, vport: u16) -> P2pResult<TunnelStream> {
        let tunnels = self.get_tunnels(remote_id);
        if let Some(tunnel) = tunnels.get_idle_tunnel() {
            return tunnel.open_stream(vport, session_id).await;
        } else {
            let remote = self.device_finder.get_identity_cert(remote_id).await?;
            let seq = self.gen_seq.generate();
            let mut tunnel = Tunnel::new(
                self.net_manager.clone(),
                self.sn_service.clone(),
                seq,
                self.protocol_version,
                self.stack_version,
                remote_id.clone(),
                remote.endpoints(),
                self.local_identity.clone(),
                self.conn_timeout,
                self.idle_timeout,
                self.listen_ports.clone(),
                self.cert_factory.clone(),
            );
            let stream = tunnel.connect_stream(vport, session_id, tunnels.clone()).await?;
            let listener = self.listener.clone();
            tunnels.add_tunnel(tunnel, listener);
            Ok(stream)
        }
    }

    pub async fn on_sn_called(&self, sn_called: SnCalled) -> P2pResult<()> {
        log::info!("on_sn_called {:?}", sn_called);

        let cert = self.cert_factory.create(&sn_called.peer_info)?;
        let eps = if self.sn_service.is_same_lan(&sn_called.reverse_endpoint_array) {
            let mut eps = cert.endpoints().clone();
            eps.extend_from_slice(sn_called.reverse_endpoint_array.as_slice());
            eps
        } else {
            let mut eps = sn_called.reverse_endpoint_array;
            eps.extend_from_slice(cert.endpoints().as_slice());
            eps
        };

        let from_device_id = cert.get_id();
        if sn_called.payload.is_empty() {
            let mut tunnel = Tunnel::new(
                self.net_manager.clone(),
                self.sn_service.clone(),
                sn_called.tunnel_id,
                self.protocol_version,
                self.stack_version,
                from_device_id.clone(),
                eps.clone(),
                self.local_identity.clone(),
                self.conn_timeout,
                self.idle_timeout,
                self.listen_ports.clone(),
                self.cert_factory.clone());

            let recv = tunnel.connect_reverse_datagram().await?;

            let listener = self.listener.clone();
            {
                let tunnels = self.get_tunnels(&from_device_id);
                tunnels.add_tunnel(tunnel, listener.clone());
            }
            let listener = self.listener.clone();
            listener.on_new_tunnel_datagram(recv).await;
        } else {
            let mut tunnel = Tunnel::new(
                self.net_manager.clone(),
                self.sn_service.clone(),
                sn_called.tunnel_id,
                self.protocol_version,
                self.stack_version,
                self.cert_factory.create(&sn_called.peer_info)?.get_id(),
                eps.clone(),
                self.local_identity.clone(),
                self.conn_timeout,
                self.idle_timeout,
                self.listen_ports.clone(),
                self.cert_factory.clone());

            let stream_call = StreamSnCall::clone_from_slice(sn_called.payload.as_slice()).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
            let stream = tunnel.connect_reverse_stream(stream_call.vport, stream_call.session_id).await?;
            let listener = self.listener.clone();
            {
                let tunnels = self.get_tunnels(&from_device_id);
                tunnels.add_tunnel(tunnel, listener.clone());
            }
            let listener = self.listener.clone();
            listener.on_new_tunnel_stream(stream).await;
        }
        Ok(())
    }
}

impl Drop for TunnelManager {
    fn drop(&mut self) {
        log::info!("tunnel manager drop.device {}", self.local_identity.get_id().to_string());
        self.net_manager.remove_connection_event_listener(&self.local_identity.get_id());
        self.clear_handle.abort();
        Executor::block_on(self.close_all_tunnel());
    }
}
