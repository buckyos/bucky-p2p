use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use cyfs_base::{BuckyResult, Device, DeviceDesc, DeviceId, Endpoint, NamedObject, Protocol, RawDecodeWithContext};
use crate::{LocalDeviceRef, TempSeq, TempSeqGenerator};
use crate::finder::DeviceCache;
use crate::protocol::{AckTunnel, DynamicPackage, MTU, PackageBox, PackageBoxDecodeContext, PackageCmdCode, SynTunnel};
use crate::protocol::v0::SnCalled;
use crate::receive_processor::{ReceiveDispatcherRef, ReceiveProcessor, RespSender, TCPReceiver};
use crate::sn::client::{SNClientServiceRef, SNEvent};
use crate::sockets::{DataSender, DataSenderFactory, NetManagerRef, TcpExtraParams, UdpExtraParams};
use crate::tunnel::tunnel_connection::TunnelConnectionKey;
use super::{Tunnel, TunnelWaiter, TunnelWaiterRef};

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
        self.tunnels.add_tunnel(self.tunnel.take().unwrap());
    }
}

struct TunnelsState {
    tunnels: Vec<Tunnel>,
    result_waiters: HashMap<TempSeq, TunnelWaiterRef>,
}
struct Tunnels {
    state: Mutex<TunnelsState>
}

impl Tunnels {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(TunnelsState {
                tunnels: vec![],
                result_waiters: Default::default(),
            })
        })
    }

    pub fn add_tunnel(&self, tunnel: Tunnel) {
        let mut state = self.state.lock().unwrap();
        state.tunnels.push(tunnel);
    }

    pub fn get_tunnel(&self) -> Option<Tunnel> {
        self.state.lock().unwrap().tunnels.pop()
    }

    pub fn add_tunnel_waiter(&self, tunnel: &Tunnel) {
        let mut state = self.state.lock().unwrap();
        state.result_waiters.insert(tunnel.get_sequence(), tunnel.get_resp_waiter().clone());
    }

    pub fn get_tunnel_waiter(&self, seq: TempSeq) -> Option<TunnelWaiterRef> {
        let mut state = self.state.lock().unwrap();
        state.result_waiters.get(&seq).map(|v| v.clone())
    }
}

#[async_trait::async_trait]
pub trait TunnelManagerEvent: 'static + Send + Sync {
    async fn on_new_tunnel(&self, tunnel: TunnelGuard);
}
pub type TunnelManagerEventRef = Arc<dyn TunnelManagerEvent>;

pub struct TunnelManager {
    tunnels: RwLock<HashMap<DeviceId, Arc<Tunnels>>>,
    net_manager: NetManagerRef,
    receive_dispatcher: ReceiveDispatcherRef,
    sn_service: SNClientServiceRef,
    local_device: LocalDeviceRef,
    protocol_version: u8,
    stack_version: u32,
    listener: TunnelManagerEventRef,
    device_cache: Arc<DeviceCache>,
    conn_timeout: Duration,
    gen_seq: Arc<TempSeqGenerator>,
    resp_waiter: TunnelWaiterRef,
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
        listener: TunnelManagerEventRef,
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
            listener,
            device_cache,
            conn_timeout,
            gen_seq: Arc::new(TempSeqGenerator::new()),
            resp_waiter: Arc::new(TunnelWaiter::new()),
        })
    }

    pub fn register_pkg_processor(self: &Arc<Self>, processor: &mut ReceiveProcessor) {
        let this = self.clone();
        processor.add_package_box_processor(PackageCmdCode::SynTunnel, move |resp_sender: &'static mut RespSender,
                                                                             pkg: DynamicPackage| {
            let this = this.clone();
            async move {
                this.on_syn_tunnel(resp_sender, pkg.as_ref()).await
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
        processor.add_package_box_processor(PackageCmdCode::PingTunnel, move |resp_sender: &'static mut RespSender,
                                                                               pkg: DynamicPackage| {
            let this = this.clone();
            async move {
                this.on_syn_tunnel(resp_sender, pkg.as_ref()).await
            }
        });

        let this = self.clone();
        processor.add_package_box_processor(PackageCmdCode::PingTunnelResp, move |resp_sender: &'static mut RespSender,
                                                                               pkg: DynamicPackage| {
            let this = this.clone();
            async move {
                this.on_syn_tunnel(resp_sender, pkg.as_ref()).await
            }
        });

        let this = self.clone();
        processor.add_package_box_processor(PackageCmdCode::SessionData, move |resp_sender: &'static mut RespSender,
                                                                             pkg: DynamicPackage| {
            let this = this.clone();
            async move {
                this.on_syn_tunnel(resp_sender, pkg.as_ref()).await
            }
        });

        let this = self.clone();
        processor.add_package_box_processor(PackageCmdCode::TcpSynConnection, move |resp_sender: &'static mut RespSender,
                                                                             pkg: DynamicPackage| {
            let this = this.clone();
            async move {
                this.on_syn_tunnel(resp_sender, pkg.as_ref()).await
            }
        });
    }

    async fn on_syn_tunnel(&self, resp_sender: &'static mut RespSender, req: &SynTunnel) -> BuckyResult<()> {
        self.device_cache.add(&req.from_device_desc.desc().device_id(), &req.from_device_desc);

        let tunnels = self.get_tunnels(&req.from_device_desc.desc().device_id());
        if tunnels.get_tunnel_waiter(req.sequence).is_some() {
            return Ok(());
        }

        let mut tunnel = Tunnel::new_for_connected(
            self.net_manager.clone(),
            self.receive_dispatcher.clone(),
            req.sequence,
            self.protocol_version,
            self.stack_version,
            req.from_device_desc.clone(),
            self.local_device.clone(),
            self.conn_timeout,
            resp_sender.clone_data_sender(),
            self.resp_waiter.clone(),
        );

        let ack = AckTunnel {
            protocol_version: self.protocol_version,
            stack_version: self.stack_version,
            sequence: req.sequence,
            result: 0,
            send_time: 0,
            mtu: MTU as u16,
            to_device_desc: self.local_device.device().clone(),
        };
        resp_sender.send_dynamic_pkg(DynamicPackage::from(ack)).await?;

        tunnels.add_tunnel_waiter(&tunnel);
        self.listener.on_new_tunnel(TunnelGuard::new(tunnel, tunnels.clone())).await;
        Ok(())
    }

    async fn on_ack_tunnel(&self, resp_sender: &'static mut RespSender, req: DynamicPackage) -> BuckyResult<()> {
        let (to_device_desc, seq) = {
            let ack: &AckTunnel = req.as_ref();
            (ack.to_device_desc.clone(), ack.sequence)
        };
        self.resp_waiter.set_result_with_cache(TunnelConnectionKey::new(seq, resp_sender.local().clone(), resp_sender.remote().clone()), req);
        self.device_cache.add(&to_device_desc.desc().device_id(), &to_device_desc);
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

    pub async fn create_tunnel(&self, remote: &Device) -> BuckyResult<TunnelGuard> {
        let remote_id = remote.desc().device_id();
        let tunnels = self.get_tunnels(&remote_id);

        if let Some(tunnel) = tunnels.get_tunnel() {
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
                self.resp_waiter.clone(),
            );
            tunnel.connect_tunnel().await?;
            tunnels.add_tunnel_waiter(&tunnel);
            Ok(TunnelGuard::new(tunnel, tunnels.clone()))
        }
    }

    async fn create_data_sender(&self, remote: &Device, remote_ep: &[Endpoint]) -> BuckyResult<Vec<Arc<dyn DataSender>>> {
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
    async fn on_called(&self, called: &SnCalled) -> BuckyResult<()> {
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
            .map_err(|err| {
                error!("{} ignore decode payload failed, err={}.", self.local_device.device_id(), err);
                err
            })?;
        if caller_box.has_exchange() {
            // let exchange: &Exchange = caller_box.packages()[0].as_ref();
            self.net_manager.key_store().add_key(caller_box.key(), caller_box.local(), caller_box.remote());
        }
        Ok(())
    }
}
