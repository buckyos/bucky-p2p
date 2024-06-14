use std::collections::{HashMap, HashSet};
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use bucky_objects::{Device, DeviceId, Endpoint, NamedObject};
use bucky_raw_codec::RawDecodeWithContext;
use crate::{IncreaseId, LocalDeviceRef, TempSeq, TempSeqGenerator};
use crate::error::{bdt_err, BdtError, BdtErrorCode, BdtResult, into_bdt_err};
use crate::executor::Executor;
use crate::finder::DeviceCache;
use crate::history::keystore::EncryptedKey;
use crate::protocol::{AckTunnel, DynamicPackage, Exchange, MTU, PackageBox, PackageBoxDecodeContext, PackageCmdCode, SynTunnel};
use crate::protocol::v0::{AckAckTunnel, SessionData, SESSIONDATA_FLAG_ACK, SESSIONDATA_FLAG_SYN, SnCalled, TcpAckAckConnection, TcpAckConnection, TcpSynConnection};
use crate::receive_processor::{ReceiveDispatcherRef, ReceiveProcessor, RespSender, TCPReceiver};
use crate::sn::client::{SNClientServiceRef, SNEvent};
use crate::sockets::{DataSender, DataSenderFactory, NetManagerRef, TcpExtraParams, UdpExtraParams};
use crate::sockets::tcp::TCPSocket;
use crate::tunnel::tunnel_connection::TunnelConnectionKey;
use crate::tunnel::udp_tunnel_connection::{UdpTunnelDataReceiver, UdpTunnelDataReceiverRef};
use super::{Tunnel, TunnelType};

pub struct TunnelGuard {
    tunnel: Option<Tunnel>,
    tunnels: Arc<Tunnels>,
}

impl TunnelGuard {
    pub fn new(tunnel: Tunnel, tunnels: Arc<Tunnels>) -> Self {
        Self {
            tunnel: Some(tunnel),
            tunnels
        }
    }
}

impl Deref for TunnelGuard {
    type Target = Tunnel;

    fn deref(&self) -> &Self::Target {
        self.tunnel.as_ref().unwrap()
    }
}

impl DerefMut for TunnelGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.tunnel.as_mut().unwrap()
    }
}

impl Drop for TunnelGuard {
    fn drop(&mut self) {
        // self.tunnels.add_idle_tunnel(self.tunnel.take().unwrap());
        self.tunnels.remove_tunnel(self.tunnel.take().unwrap().get_sequence());
    }
}

struct TunnelsState {
    idle_tunnels: Vec<Tunnel>,
    tunnels: HashSet<TempSeq>,
}
struct Tunnels {
    state: Mutex<TunnelsState>
}

impl Tunnels {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(TunnelsState {
                idle_tunnels: vec![],
                tunnels: Default::default(),
            })
        })
    }

    pub fn add_idle_tunnel(&self, tunnel: Tunnel) {
        let mut state = self.state.lock().unwrap();
        state.idle_tunnels.push(tunnel);
    }

    pub fn get_idle_tunnel(&self) -> Option<Tunnel> {
        self.state.lock().unwrap().idle_tunnels.pop()
    }

    pub fn add_tunnel(&self, tunnel: &Tunnel) {
        let mut state = self.state.lock().unwrap();
        state.tunnels.insert(tunnel.get_sequence());
    }

    pub fn tunnel_exist(&self, seq: TempSeq) -> bool {
        let mut state = self.state.lock().unwrap();
        state.tunnels.contains(&seq)
    }

    pub fn remove_tunnel(&self, seq: TempSeq) {
        let mut state = self.state.lock().unwrap();
        state.tunnels.remove(&seq);
    }
}

#[callback_trait::callback_trait]
pub trait TunnelManagerEvent: 'static + Send + Sync {
    async fn on_new_tunnel(&self, tunnel: TunnelGuard) -> ();
}
pub type TunnelManagerEventRef = Arc<dyn TunnelManagerEvent>;

#[derive(Clone)]
struct TunnelManagerEventListener {
    tunnel_listener: Option<TunnelManagerEventRef>,
    stream_listener: Option<TunnelManagerEventRef>,
}

pub struct TunnelManager {
    tunnels: RwLock<HashMap<DeviceId, Arc<Tunnels>>>,
    net_manager: NetManagerRef,
    receive_dispatcher: ReceiveDispatcherRef,
    sn_service: SNClientServiceRef,
    local_device: LocalDeviceRef,
    protocol_version: u8,
    stack_version: u32,
    listener: Mutex<TunnelManagerEventListener>,
    device_cache: Arc<DeviceCache>,
    conn_timeout: Duration,
    gen_seq: Arc<TempSeqGenerator>,
    data_receiver: UdpTunnelDataReceiverRef,
    listen_ports: Mutex<HashSet<u16>>,
}
pub type TunnelManagerRef = Arc<TunnelManager>;

impl TunnelManager {
    pub fn new(
        net_manager: NetManagerRef,
        receive_dispatcher: ReceiveDispatcherRef,
        sn_service: SNClientServiceRef,
        local_device: LocalDeviceRef,
        protocol_version: u8,
        stack_version: u32,
        device_cache: Arc<DeviceCache>,
        conn_timeout: Duration,) -> Arc<Self> {
        Arc::new(Self {
            tunnels: Default::default(),
            net_manager,
            receive_dispatcher,
            sn_service,
            local_device,
            protocol_version,
            stack_version,
            listener: Mutex::new(TunnelManagerEventListener {
                tunnel_listener: None,
                stream_listener: None,
            }),
            device_cache,
            conn_timeout,
            gen_seq: Arc::new(TempSeqGenerator::new()),
            data_receiver: Arc::new(UdpTunnelDataReceiver::new()),
            listen_ports: Mutex::new(Default::default()),
        })
    }

    pub(crate) fn set_tunnel_listener(&self, listener: impl TunnelManagerEvent) {
        let mut listeners = self.listener.lock().unwrap();
        listeners.tunnel_listener = Some(Arc::new(listener));
    }

    pub(crate) fn set_stream_listener(&self, listener: impl TunnelManagerEvent) {
        let mut listeners = self.listener.lock().unwrap();
        listeners.stream_listener = Some(Arc::new(listener));
    }

    pub(crate) fn add_listen_port(&self, port: u16) {
        let mut listen_ports = self.listen_ports.lock().unwrap();
        listen_ports.insert(port);
    }

    pub(crate) fn remove_listen_port(&self, port: u16) {
        let mut listen_ports = self.listen_ports.lock().unwrap();
        listen_ports.remove(&port);
    }

    pub(crate) fn register_pkg_processor(self: &Arc<Self>, processor: &mut ReceiveProcessor) {
        let this = self.clone();
        processor.add_package_box_processor(PackageCmdCode::SynTunnel, move |resp_sender: &'static mut RespSender,
                                                                             pkg: DynamicPackage| {
            let this = this.clone();
            async move {
                this.on_syn_tunnel(resp_sender, pkg).await
            }
        });

        let this = self.clone();
        processor.add_package_box_processor(PackageCmdCode::AckTunnel, move |resp_sender: &'static mut RespSender,
                                                                             pkg: DynamicPackage| {
            let this = this.clone();
            async move {
                this.on_ack_tunnel(resp_sender, pkg).await
            }
        });

        let this = self.clone();
        processor.add_package_box_processor(PackageCmdCode::AckAckTunnel, move |resp_sender: &'static mut RespSender,
                                                                             pkg: DynamicPackage| {
            let this = this.clone();
            async move {
                this.on_ack_ack_sync_tunnel(resp_sender, pkg).await
            }
        });

        let this = self.clone();
        processor.add_package_box_processor(PackageCmdCode::TcpSynConnection, move |resp_sender: &'static mut RespSender,
                                                                             pkg: DynamicPackage| {
            let this = this.clone();
            async move {
                this.on_tcp_sync_tunnel(resp_sender, pkg).await
            }
        });

        let this = self.clone();
        processor.add_package_box_processor(PackageCmdCode::TcpAckConnection, move |resp_sender: &'static mut RespSender,
                                                                                    pkg: DynamicPackage| {
            let this = this.clone();
            async move {
                this.on_tcp_ack_connection(resp_sender, pkg).await
            }
        });

        let this = self.clone();
        processor.add_package_box_processor(PackageCmdCode::TcpAckAckConnection, move |resp_sender: &'static mut RespSender,
                                                                                    pkg: DynamicPackage| {
            let this = this.clone();
            async move {
                this.on_tcp_ack_ack_connection(resp_sender, pkg).await
            }
        });

        let this = self.clone();
        processor.add_package_box_processor(PackageCmdCode::PingTunnel, move |resp_sender: &'static mut RespSender,
                                                                               pkg: DynamicPackage| {
            let this = this.clone();
            async move {
                this.on_syn_tunnel(resp_sender, pkg).await
            }
        });

        let this = self.clone();
        processor.add_package_box_processor(PackageCmdCode::PingTunnelResp, move |resp_sender: &'static mut RespSender,
                                                                               pkg: DynamicPackage| {
            let this = this.clone();
            async move {
                this.on_syn_tunnel(resp_sender, pkg).await
            }
        });

        let this = self.clone();
        processor.add_package_box_processor(PackageCmdCode::SessionData, move |resp_sender: &'static mut RespSender,
                                                                             pkg: DynamicPackage| {
            let this = this.clone();
            async move {
                this.on_session_data(resp_sender, pkg).await
            }
        });

        let this = self.clone();
        processor.add_package_box_processor(PackageCmdCode::TcpSynConnection, move |resp_sender: &'static mut RespSender,
                                                                             pkg: DynamicPackage| {
            let this = this.clone();
            async move {
                this.on_tcp_sync_tunnel(resp_sender, pkg).await
            }
        });

        let this = self.clone();
        processor.add_tcp_processor(move | socket: Arc<TCPSocket>, first_box: PackageBox,| {
            let this = this.clone();
            async move {
                this.on_new_tcp_tunnel(socket, first_box).await
            }
        });

        let this = self.clone();
        processor.add_tcp_processor(move | socket: Arc<TCPSocket>, first_box: PackageBox,| {
            let this = this.clone();
            async move {
                this.on_new_tcp_tunnel(socket, first_box).await
            }
        });
    }

    async fn on_new_tcp_tunnel(&self, socket: Arc<TCPSocket>, first_box: PackageBox) -> BdtResult<()> {
        let pkgs = first_box.packages_no_exchange();
        if pkgs.len() == 0 {
            return Err(bdt_err!(BdtErrorCode::InvalidInput, "no package in first box"));
        }

        let syn: &SynTunnel = pkgs[0].as_ref();

        let (from_device_desc, seq) = {
            (syn.from_device_desc.clone(), syn.sequence)
        };

        self.device_cache.add(&from_device_desc.desc().device_id(), &from_device_desc);

        let tunnels = self.get_tunnels(&from_device_desc.desc().device_id());
        if tunnels.tunnel_exist(seq) {
            return Ok(());
        }

        let mut tunnel = Tunnel::new(
            self.net_manager.clone(),
            self.receive_dispatcher.clone(),
            seq,
            self.protocol_version,
            self.stack_version,
            from_device_desc.clone(),
            self.local_device.clone(),
            self.conn_timeout,
            self.data_receiver.clone()
        );

        tunnels.add_tunnel(&tunnel);
        let listener = {
            self.listener.lock().unwrap().clone()
        };
        let listen_ports = {
            self.listen_ports.lock().unwrap().clone()
        };
        let future = tunnel.accept_tcp_tunnel(listen_ports, socket, first_box);
        Executor::spawn(async move {
            match future.await {
                Ok(tunnel) => {
                    match tunnel.tunnel_type() {
                        TunnelType::TUNNEL => {
                            if listener.tunnel_listener.is_some() {
                                listener.tunnel_listener.as_ref().unwrap().on_new_tunnel(TunnelGuard::new(tunnel, tunnels.clone())).await;
                            }
                        }
                        TunnelType::STREAM(_) => {
                            if listener.stream_listener.is_some() {
                                listener.stream_listener.as_ref().unwrap().on_new_tunnel(TunnelGuard::new(tunnel, tunnels.clone())).await;
                            }
                        }
                    }
                }
                Err(_) => {
                    tunnels.remove_tunnel(seq);
                }
            }
        });

        Ok(())
    }

    async fn on_syn_tunnel(&self, resp_sender: &'static mut RespSender, req: DynamicPackage) -> BdtResult<()> {
        let (from_device_desc, seq) = {
            let ack: &SynTunnel = req.as_ref();
            (ack.from_device_desc.clone(), ack.sequence)
        };

        self.device_cache.add(&from_device_desc.desc().device_id(), &from_device_desc);

        let tunnels = self.get_tunnels(&from_device_desc.desc().device_id());
        if tunnels.tunnel_exist(seq) {
            return Ok(());
        }

        let mut tunnel = Tunnel::new(
            self.net_manager.clone(),
            self.receive_dispatcher.clone(),
            seq,
            self.protocol_version,
            self.stack_version,
            from_device_desc.clone(),
            self.local_device.clone(),
            self.conn_timeout,
            self.data_receiver.clone()
        );

        tunnels.add_tunnel(&tunnel);
        let data_sender = resp_sender.clone_data_sender();
        let listener = {
            self.listener.lock().unwrap().clone()
        };
        let listen_ports = {
            self.listen_ports.lock().unwrap().clone()
        };
        let future = tunnel.accept_udp_tunnel(listen_ports, data_sender);
        Executor::spawn(async move {
            match future.await {
                Ok(tunnel) => {
                    match tunnel.tunnel_type() {
                        TunnelType::TUNNEL => {
                            if listener.tunnel_listener.is_some() {
                                listener.tunnel_listener.as_ref().unwrap().on_new_tunnel(TunnelGuard::new(tunnel, tunnels.clone())).await;
                            }
                        }
                        TunnelType::STREAM(_) => {
                            if listener.stream_listener.is_some() {
                                listener.stream_listener.as_ref().unwrap().on_new_tunnel(TunnelGuard::new(tunnel, tunnels.clone())).await;
                            }
                        }
                    }
                }
                Err(_) => {
                    tunnels.remove_tunnel(seq);
                }
            }
        });

        let result =  self.data_receiver.on_recv_pkg_with_resp(&TunnelConnectionKey::new(seq, resp_sender.local_device_id().clone(), resp_sender.remote_device_id().clone()),
                                                               req, self.conn_timeout).await?;
        resp_sender.send_dynamic_pkg(result).await?;

        Ok(())
    }

    async fn on_ack_tunnel(&self, resp_sender: &'static mut RespSender, req: DynamicPackage) -> BdtResult<()> {
        let (to_device_desc, seq) = {
            let ack: &AckTunnel = req.as_ref();
            (ack.to_device_desc.clone(), ack.sequence)
        };
        let result =  self.data_receiver.on_recv_pkg_with_resp(&TunnelConnectionKey::new(seq, resp_sender.local_device_id().clone(), resp_sender.remote_device_id().clone()),
                                                       req, self.conn_timeout).await?;
        resp_sender.send_dynamic_pkg(result).await?;
        self.device_cache.add(&to_device_desc.desc().device_id(), &to_device_desc);
        Ok(())
    }

    async fn on_ack_ack_sync_tunnel(&self, resp_sender: &'static mut RespSender, req: DynamicPackage) -> BdtResult<()> {
        let seq = {
            let ack: &AckAckTunnel = req.as_ref();
            ack.seq
        };
        self.data_receiver.on_recv_pkg(&TunnelConnectionKey::new(seq, resp_sender.local_device_id().clone(), resp_sender.remote_device_id().clone()),
                                                               req).await?;
        Ok(())
    }

    async fn on_tcp_sync_tunnel(&self, resp_sender: &'static mut RespSender, req: DynamicPackage) -> BdtResult<()> {
        let seq = {
            let ack: &TcpSynConnection = req.as_ref();
            ack.sequence
        };
        let result =  self.data_receiver.on_recv_pkg_with_resp(&TunnelConnectionKey::new(seq, resp_sender.local_device_id().clone(), resp_sender.remote_device_id().clone()),
                                                       req, self.conn_timeout).await?;
        resp_sender.send_dynamic_pkg(result).await?;
        Ok(())
    }

    async fn on_tcp_ack_connection(&self, resp_sender: &'static mut RespSender, req: DynamicPackage) -> BdtResult<()> {
        let seq = {
            let ack: &TcpAckConnection = req.as_ref();
            ack.sequence
        };
        let result =  self.data_receiver.on_recv_pkg_with_resp(&TunnelConnectionKey::new(seq, resp_sender.local_device_id().clone(), resp_sender.remote_device_id().clone()),
                                                       req, self.conn_timeout).await?;
        resp_sender.send_dynamic_pkg(result).await?;
        Ok(())
    }

    async fn on_tcp_ack_ack_connection(&self, resp_sender: &'static mut RespSender, req: DynamicPackage) -> BdtResult<()> {
        let seq = {
            let ack: &TcpAckAckConnection = req.as_ref();
            ack.sequence
        };
        self.data_receiver.on_recv_pkg(&TunnelConnectionKey::new(seq, resp_sender.local_device_id().clone(), resp_sender.remote_device_id().clone()),
                                       req).await?;
        Ok(())
    }

    async fn on_session_data(&self, resp_sender: &'static mut RespSender, req: DynamicPackage) -> BdtResult<()> {
        let (seq, need_resp) = {
            let data: &SessionData = req.as_ref();
            if data.syn_info.is_none() {
                return Err(BdtError::new(BdtErrorCode::InvalidInput, "no syn info in session data".to_string()));
            }
            let need_resp = data.is_flags_contain(SESSIONDATA_FLAG_ACK) || data.is_flags_contain(SESSIONDATA_FLAG_SYN);
            (data.syn_info.as_ref().unwrap().sequence, need_resp)
        };

        if need_resp {
            let result =  self.data_receiver.on_recv_pkg_with_resp(&TunnelConnectionKey::new(seq, resp_sender.local_device_id().clone(), resp_sender.remote_device_id().clone()),
                                                                   req, self.conn_timeout).await?;
            resp_sender.send_dynamic_pkg(result).await?;
        } else {
            self.data_receiver.on_recv_pkg(&TunnelConnectionKey::new(seq, resp_sender.local_device_id().clone(), resp_sender.remote_device_id().clone()),
                                           req).await?;
        }
        Ok(())
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

    pub async fn create_tunnel(&self, remote: &Device) -> BdtResult<TunnelGuard> {
        let remote_id = remote.desc().device_id();
        let tunnels = self.get_tunnels(&remote_id);

        if let Some(tunnel) = tunnels.get_idle_tunnel() {
            Ok(TunnelGuard::new(tunnel, tunnels.clone()))
        } else {
            let seq = self.gen_seq.generate();
            let mut tunnel = Tunnel::new(
                self.net_manager.clone(),
                self.receive_dispatcher.clone(),
                seq,
                self.protocol_version,
                self.stack_version,
                remote.clone(),
                self.local_device.clone(),
                self.conn_timeout,
                self.data_receiver.clone(),
            );
            tunnel.connect_tunnel().await?;
            tunnels.add_tunnel(&tunnel);
            Ok(TunnelGuard::new(tunnel, tunnels.clone()))
        }
    }

    pub async fn create_stream_tunnel(&self, remote: &Device, vport: u16) -> BdtResult<TunnelGuard> {
        let remote_id = remote.desc().device_id();
        let tunnels = self.get_tunnels(&remote_id);

        if let Some(tunnel) = tunnels.get_idle_tunnel() {
            Ok(TunnelGuard::new(tunnel, tunnels.clone()))
        } else {
            let seq = self.gen_seq.generate();
            let session_id = IncreaseId::default();
            let mut tunnel = Tunnel::new(
                self.net_manager.clone(),
                self.receive_dispatcher.clone(),
                seq,
                self.protocol_version,
                self.stack_version,
                remote.clone(),
                self.local_device.clone(),
                self.conn_timeout,
                self.data_receiver.clone(),
            );
            tunnel.connect_stream_tunnel(vport, session_id).await?;
            tunnels.add_tunnel(&tunnel);
            Ok(TunnelGuard::new(tunnel, tunnels.clone()))
        }
    }

    async fn create_data_sender(&self, remote: &Device, remote_ep: &[Endpoint]) -> BdtResult<Vec<Arc<dyn DataSender>>> {
        let mut data_senders: Vec<Arc<dyn DataSender>> = vec![];
        for ep in remote_ep.iter().filter(|ep| ep.is_tcp() && ep.is_static_wan()) {
            if let Ok(data_sender) = self.net_manager.create_sender(self.local_device.device_id().clone(), remote.desc().clone(), ep.clone(), TcpExtraParams {
                timeout: self.conn_timeout,
            }).await {
                let processor = self.receive_dispatcher.get_processor(self.local_device.device_id());
                if processor.is_some() {
                    data_senders.push(TCPReceiver::new(Arc::new(data_sender), processor.unwrap(), self.net_manager.key_store().clone()));
                    return Ok(data_senders);
                }
            }
        }

        for ep in remote_ep.iter().filter(|ep| ep.is_udp() && ep.is_static_wan()) {
            for udp_listener in self.net_manager.udp_listeners().iter().filter(|udp| udp.local().is_same_ip_version(ep)) {
                let data_sender = self.net_manager.create_sender(self.local_device.device_id().clone(), remote.desc().clone(), ep.clone(), UdpExtraParams {
                    local_ep: udp_listener.local()
                }).await?;
                data_senders.push(Arc::new(data_sender));
            }
        }
        Ok(data_senders)
    }
}

#[async_trait::async_trait]
impl SNEvent for TunnelManager {
    async fn on_called(&self, called: &SnCalled) -> BdtResult<()> {
        if called.payload.len() == 0 {
            warn!("{} ignore called for no payload.", self.local_device.device_id());
            return Ok(());
        }

        let mut crypto_buf = vec![0u8; called.payload.as_ref().len()];
        let ctx = PackageBoxDecodeContext::new_copy(crypto_buf.as_mut(), self.net_manager.key_store());
        let caller_box = PackageBox::raw_decode_with_context(
            called.payload.as_ref(),
            (ctx, Some(called.into())),
        ).map(|(package_box, _)| package_box)
            .map_err(into_bdt_err!(BdtErrorCode::IoError, "{} ignore decode payload failed.", self.local_device.device_id()))?;
        if caller_box.has_exchange() {
            let exchange: &Exchange = caller_box.packages()[0].as_ref();
            self.net_manager.key_store().add_key(caller_box.key(), caller_box.local(), caller_box.remote(), EncryptedKey::Unconfirmed(exchange.key_encrypted.clone()));
        }
        Ok(())
    }
}
