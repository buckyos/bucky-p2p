use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use bucky_objects::{Device, DeviceId, Endpoint, NamedObject};
use bucky_raw_codec::{RawDecodeWithContext, RawFrom};
use bucky_time::bucky_time_now;
use callback_trait::callback_trait;
use notify_future::NotifyFuture;
use crate::{IncreaseId, LocalDeviceRef, runtime, TempSeq, TempSeqGenerator};
use crate::error::{bdt_err, BdtError, BdtErrorCode, BdtResult, into_bdt_err};
use crate::executor::{Executor, SpawnHandle};
use crate::finder::DeviceCache;
use crate::history::keystore::EncryptedKey;
use crate::protocol::{AckTunnel, Package, MTU, PackageCmdCode, SynTunnel};
use crate::protocol::v0::{SnCalled, AckStream, SynStream};
use crate::receive_processor::{ReceiveDispatcherRef, ReceiveProcessor,};
use crate::sn::client::{SNClientServiceRef, SNEvent};
use crate::sockets::{NetManagerRef, QuicSocket};
use crate::sockets::tcp::TCPSocket;
use super::{QuicTunnelConnection, ReverseFutureCache, ReverseResult, SocketType, StreamSnCall, TcpTunnelConnection, Tunnel, TunnelConnection, TunnelDatagramRecv, TunnelDatagramSend, TunnelInstance, TunnelListenPorts, TunnelListenPortsRef, TunnelStream, TunnelType};

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

impl Tunnels {
    pub(crate) fn new() -> Arc<Self> {
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
        Executor::spawn(async move {
            loop {
                match tunnel.accept_instance().await {
                    Ok(instance) => {
                        match instance {
                            TunnelInstance::Stream(stream) => {
                                log::info!("accept stream tunnel {:?} stream_id {} port {} remote_id {} remote_ep {} local_id {} local_ep {}",
                                    stream.sequence(),
                                    stream.session_id(),
                                    stream.port(),
                                    stream.remote_device_id().to_string(),
                                    stream.remote_endpoint().to_string(),
                                    stream.local_device_id().to_string(),
                                    stream.local_endpoint().to_string());

                                    listener.on_new_tunnel_stream(stream).await;
                            }
                            TunnelInstance::Datagram(datagram) => {
                                log::info!("accept datagram tunnel {:?} remote_id {} remote_ep {} local_id {} local_ep {}",
                                    datagram.sequence(),
                                    datagram.remote_device_id().to_string(),
                                    datagram.remote_endpoint().to_string(),
                                    datagram.local_device_id().to_string(),
                                    datagram.local_endpoint().to_string());

                                    listener.on_new_tunnel_datagram(datagram).await;
                            }
                            TunnelInstance::ReverseStream(stream) => {

                            }
                            TunnelInstance::ReverseDatagram(datagram) => {

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

    async fn on_new_tunnel_stream(&self, tunnel: Box<dyn TunnelStream>) {
        let stream_event = {
            self.stream_event.lock().unwrap().clone()
        };
        if stream_event.is_some() {
            stream_event.as_ref().unwrap().on_new_tunnel_stream(tunnel).await;
        }
    }

    async fn on_new_tunnel_datagram(&self, tunnel: Box<dyn TunnelDatagramRecv>) {
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
    async fn on_new_tunnel_stream(&self, tunnel: Box<dyn TunnelStream>) -> ();
}
pub type TunnelManagerStreamEventRef = Arc<dyn TunnelManagerStreamEvent>;

#[callback_trait::callback_trait]
pub trait TunnelManagerDatagramEvent: 'static + Send + Sync {
    async fn on_new_tunnel_datagram(&self, tunnel: Box<dyn TunnelDatagramRecv>) -> ();
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

pub struct TunnelManager {
    tunnels: Arc<RwLock<HashMap<DeviceId, Arc<Tunnels>>>>,
    net_manager: NetManagerRef,
    receive_dispatcher: ReceiveDispatcherRef,
    sn_service: SNClientServiceRef,
    local_device: LocalDeviceRef,
    protocol_version: u8,
    stack_version: u32,
    listener: TunnelManagerEventRef,
    conn_timeout: Duration,
    gen_seq: Arc<TempSeqGenerator>,
    listen_ports: Arc<ListenPorts>,
    clear_handle: SpawnHandle<()>,
}
pub type TunnelManagerRef = Arc<TunnelManager>;

impl TunnelManager {
    pub(crate) fn new(
        net_manager: NetManagerRef,
        receive_dispatcher: ReceiveDispatcherRef,
        sn_service: SNClientServiceRef,
        local_device: LocalDeviceRef,
        protocol_version: u8,
        stack_version: u32,
        conn_timeout: Duration,) -> Arc<Self> {
        let tunnels = Arc::new(RwLock::new(HashMap::<DeviceId, Arc<Tunnels>>::new()));
        let tmp = tunnels.clone();
        let handle = Executor::spawn_with_handle(async move {
            loop {
                runtime::sleep(Duration::from_secs(120)).await;
                Self::clear_idle_tunnel(tmp.clone()).await;
            }
            ()
        }).unwrap();


        let manager = Arc::new(Self {
            tunnels,
            net_manager,
            receive_dispatcher,
            sn_service,
            local_device,
            protocol_version,
            stack_version,
            listener: TunnelManagerEvent::new(),
            conn_timeout,
            gen_seq: Arc::new(TempSeqGenerator::new()),
            listen_ports: ListenPorts::new(),
            clear_handle: handle,
        });

        manager
    }

    async fn clear_idle_tunnel(tunnels: Arc<RwLock<HashMap<DeviceId, Arc<Tunnels>>>>) {
        let tunnels_map = tunnels.read().unwrap();
        for (_, tunnels) in tunnels_map.iter() {
            let mut remove_list = Vec::new();
            {
                let mut state = tunnels.state.lock().unwrap();
                for (seq, tunnel) in state.tunnels.iter() {
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

    pub(crate) fn register_pkg_processor(self: &Arc<Self>, processor: &mut ReceiveProcessor) {
        let this = self.clone();
        processor.add_tcp_processor(move |socket: TCPSocket| {
            let this = this.clone();
            async move {
                this.on_new_tcp_tunnel(socket).await
            }
        });

        let this = self.clone();
        processor.add_quic_processor(move |socket: QuicSocket| {
            let this = this.clone();
            async move {
                this.on_new_quic_tunnel(socket).await
            }
        });
    }

    async fn on_new_tcp_tunnel(&self, socket: TCPSocket) -> BdtResult<()> {
        let remote_id = socket.remote_device_id().clone();
        let remote_ep = socket.remote().clone();
        let local_id = socket.local_device_id().clone();
        let local_ep = socket.local().clone();
        let remote = self.net_manager.get_device_cache().get(&remote_id).await.unwrap();
        let net_manager = self.net_manager.clone();
        let sn_service = self.sn_service.clone();
        let protocol_version = self.protocol_version;
        let stack_version = self.stack_version;
        let local_device = self.local_device.clone();
        let conn_timeout = self.conn_timeout;
        let listen_ports = self.listen_ports.clone();

        let mut tunnel_conn = Box::new(TcpTunnelConnection::new(
            TempSeq::from(0),
            self.local_device.clone(),
            remote_id.clone(),
            socket.remote().clone(),
            self.conn_timeout,
            self.protocol_version,
            self.stack_version,
            Some(socket),
            self.listen_ports.clone())?);

        let tunnels = self.get_tunnels(&remote_id);
        let listener = self.listener.clone();
        Executor::spawn(async move {
            match tunnel_conn.accept_instance().await {
                Ok(instance) => {
                    log::info!("new tcp tunnel {:?} remote_id {} remote_ep {} local_id {} local_ep {}",
                        tunnel_conn.get_sequence(),
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
                                remote.clone(),
                                local_device.clone(),
                                conn_timeout,
                                listen_ports.clone());
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
                                remote.clone(),
                                local_device.clone(),
                                conn_timeout,
                                listen_ports.clone());
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

    async fn on_new_quic_tunnel(&self, socket: QuicSocket) -> BdtResult<()> {
        let remote_id = socket.remote_device_id().clone();
        let remote_ep = socket.remote().clone();
        let local_id = socket.local_device_id().clone();
        let local_ep = socket.local().clone();
        let remote = self.net_manager.get_device_cache().get(&remote_id).await.unwrap();
        let net_manager = self.net_manager.clone();
        let sn_service = self.sn_service.clone();
        let protocol_version = self.protocol_version;
        let stack_version = self.stack_version;
        let local_device = self.local_device.clone();
        let conn_timeout = self.conn_timeout;
        let listen_ports = self.listen_ports.clone();

        let mut tunnel = Tunnel::new(
            self.net_manager.clone(),
            self.sn_service.clone(),
            self.gen_seq.generate(),
            self.protocol_version,
            self.stack_version,
            remote.clone(),
            self.local_device.clone(),
            self.conn_timeout.clone(),
            self.listen_ports.clone());
        // tunnel.accept_quic_tunnel(socket)?;

        let tunnel_conn = Box::new(QuicTunnelConnection::new(
            self.net_manager.clone(),
            TempSeq::from(0),
            self.local_device.clone(),
            remote_id.clone(),
            remote_ep,
            self.conn_timeout,
            self.protocol_version,
            self.stack_version,
            local_ep,
            Some(socket),
            self.listen_ports.clone()));

        let tunnels = self.get_tunnels(&remote_id);
        let listener = self.listener.clone();
        Executor::spawn(async move {
            match tunnel_conn.accept_instance().await {
                Ok(instance) => {
                    log::info!("new quic tunnel {:?} remote_id {} remote_ep {} local_id {} local_ep {}",
                        tunnel_conn.get_sequence(),
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
                                remote.clone(),
                                local_device.clone(),
                                conn_timeout,
                                listen_ports.clone());
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
                                remote.clone(),
                                local_device.clone(),
                                conn_timeout,
                                listen_ports.clone());
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

    pub(crate) fn set_stream_listener(&self, listener: impl TunnelManagerStreamEvent) {
        self.listener.set_stream_event_listener(listener);
    }

    pub(crate) fn set_datagram_listener(&self, listener: impl TunnelManagerDatagramEvent) {
        self.listener.set_datagram_event_listener(listener);
    }

    pub(crate) fn add_listen_port(&self, port: u16) {
        self.listen_ports.add_listen_port(port)
    }

    pub(crate) fn remove_listen_port(&self, port: u16) {
        self.listen_ports.remove_listen_port(port)
    }

    fn get_tunnels(&self, remote_id: &DeviceId) -> Arc<Tunnels> {
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

    pub async fn create_datagram_tunnel(&self, remote: &Device) -> BdtResult<Box<dyn TunnelDatagramSend>> {
        let remote_id = remote.desc().device_id();
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
                remote.clone(),
                self.local_device.clone(),
                self.conn_timeout,
                self.listen_ports.clone(),
            );
            let datagram = tunnel.connect_datagram(tunnels.clone()).await?;
            let listener = self.listener.clone();
            tunnels.add_tunnel(tunnel, listener);
            Ok(datagram)
        }
    }

    pub async fn create_stream_tunnel(&self, remote: &Device, session_id: IncreaseId, vport: u16) -> BdtResult<Box<dyn TunnelStream>> {
        let remote_id = remote.desc().device_id();
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
                remote.clone(),
                self.local_device.clone(),
                self.conn_timeout,
                self.listen_ports.clone(),
            );
            let stream = tunnel.connect_stream(vport, session_id, tunnels.clone()).await?;
            let listener = self.listener.clone();
            tunnels.add_tunnel(tunnel, listener);
            Ok(stream)
        }
    }

    fn get_wan_ip_list(&self) -> Vec<Endpoint> {
        let mut wan_list = Vec::new();
        self.sn_service.get_active_sn_list().iter().map(|v| v.wan_ep_list.as_slice()).flatten().for_each(|ep| {
            wan_list.push(ep.clone());
        });
        wan_list
    }

    fn is_same_lan(&self, reverse_list: &Vec<Endpoint>) -> bool {
        let local_wan_list = self.get_wan_ip_list();
        for ep in reverse_list.iter() {
            for wan_ip in local_wan_list.iter() {
                if ep.is_same_ip_addr(wan_ip) {
                    return true;
                }
            }
        }
        return false;
    }

    pub async fn on_sn_called(&self, sn_called: SnCalled) -> BdtResult<()> {
        log::info!("on_sn_called {:?}", sn_called);

        let eps = if self.is_same_lan(&sn_called.reverse_endpoint_array) {
            let mut eps = sn_called.peer_info.connect_info().endpoints().clone();
            eps.extend_from_slice(sn_called.reverse_endpoint_array.as_slice());
            eps
        } else {
            let mut eps = sn_called.reverse_endpoint_array;
            eps.extend_from_slice(sn_called.peer_info.connect_info().endpoints());
            eps
        };

        let from_device_id = sn_called.peer_info.desc().device_id();
        for ep in eps.iter() {
            if ep.is_tcp() {
                let mut tunnel = Tunnel::new(
                    self.net_manager.clone(),
                    self.sn_service.clone(),
                    sn_called.tunnel_id,
                    self.protocol_version,
                    self.stack_version,
                    sn_called.peer_info.clone(),
                    self.local_device.clone(),
                    self.conn_timeout,
                    self.listen_ports.clone());

                let mut tunnel_conn = TcpTunnelConnection::new(
                    sn_called.tunnel_id,
                    self.local_device.clone(),
                    from_device_id.clone(),
                    ep.clone(),
                    self.conn_timeout,
                    self.protocol_version,
                    self.stack_version,
                    None, self.listen_ports.clone())?;
                if sn_called.payload.is_empty() {
                    let datagram = match tunnel_conn.connect_reverse_datagram().await {
                        Ok(datagram) => datagram,
                        Err(e) => {
                            log::error!("connect reverse stream error: {:?} msg: {}", e.code(), e.msg());
                            continue;
                        }
                    };
                    tunnel.set_tunnel_conn(Box::new(tunnel_conn));
                    let listener = self.listener.clone();
                    {
                        let mut tunnels = self.get_tunnels(&from_device_id);
                        tunnels.add_tunnel(tunnel, listener.clone());
                    }
                    let listener = self.listener.clone();
                    listener.on_new_tunnel_datagram(datagram).await;
                } else {
                    let stream_call = StreamSnCall::clone_from_slice(sn_called.payload.as_slice()).map_err(into_bdt_err!(BdtErrorCode::RawCodecError))?;
                    let stream = match tunnel_conn.connect_reverse_stream(stream_call.vport, stream_call.session_id).await {
                        Ok(stream) => stream,
                        Err(e) => {
                            log::error!("connect reverse datagram error: {:?} msg: {}", e.code(), e.msg());
                            continue;
                        }
                    };
                    tunnel.set_tunnel_conn(Box::new(tunnel_conn));
                    let listener = self.listener.clone();
                    {
                        let mut tunnels = self.get_tunnels(&from_device_id);
                        tunnels.add_tunnel(tunnel, listener.clone());
                    }
                    listener.on_new_tunnel_stream(stream).await;
                }
            } else if ep.is_udp() {
                for listener in self.net_manager.udp_listeners().iter() {
                    let local_ep = listener.local();
                    let mut tunnel = Tunnel::new(
                        self.net_manager.clone(),
                        self.sn_service.clone(),
                        sn_called.tunnel_id,
                        self.protocol_version,
                        self.stack_version,
                        sn_called.peer_info.clone(),
                        self.local_device.clone(),
                        self.conn_timeout,
                        self.listen_ports.clone());

                    let mut tunnel_conn = QuicTunnelConnection::new(self.net_manager.clone(),
                                                                    sn_called.tunnel_id,
                                                                    self.local_device.clone(),
                                                                    from_device_id.clone(),
                                                                    ep.clone(),
                                                                    self.conn_timeout,
                                                                    self.protocol_version,
                                                                    self.stack_version,
                                                                    local_ep.clone(),
                                                                    None,
                                                                    self.listen_ports.clone());
                    if sn_called.payload.is_empty() {
                        let datagram = match tunnel_conn.connect_reverse_datagram().await {
                            Ok(datagram) => datagram,
                            Err(e) => {
                                log::error!("connect reverse datagram error: {:?} msg: {}", e.code(), e.msg());
                                continue;
                            }
                        };
                        tunnel.set_tunnel_conn(Box::new(tunnel_conn));
                        let listener = self.listener.clone();
                        {
                            let mut tunnels = self.get_tunnels(&from_device_id);
                            tunnels.add_tunnel(tunnel, listener.clone());
                        }
                        let listener = self.listener.clone();
                        listener.on_new_tunnel_datagram(datagram).await;
                    } else {
                        let stream_call = StreamSnCall::clone_from_slice(sn_called.payload.as_slice()).map_err(into_bdt_err!(BdtErrorCode::RawCodecError))?;
                        let stream = match tunnel_conn.connect_reverse_stream(stream_call.vport, stream_call.session_id).await {
                            Ok(stream) => stream,
                            Err(e) => {
                                log::error!("connect reverse stream error: {:?} msg: {}", e.code(), e.msg());
                                continue;
                            }
                        };
                        tunnel.set_tunnel_conn(Box::new(tunnel_conn));
                        let listener = self.listener.clone();
                        {
                            let mut tunnels = self.get_tunnels(&from_device_id);
                            tunnels.add_tunnel(tunnel, listener.clone());
                        }
                        listener.on_new_tunnel_stream(stream).await;
                    }
                }
            }
        }
        Ok(())
    }
}

impl Drop for TunnelManager {
    fn drop(&mut self) {
        log::info!("tunnel manager drop.device {}", self.local_device.device_id().to_string());
        self.clear_handle.abort();
    }
}
