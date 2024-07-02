use std::collections::{HashMap, HashSet};
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use bucky_objects::{Device, DeviceId, Endpoint, NamedObject};
use bucky_raw_codec::RawDecodeWithContext;
use crate::{IncreaseId, LocalDeviceRef, runtime, TempSeq, TempSeqGenerator};
use crate::error::{bdt_err, BdtError, BdtErrorCode, BdtResult, into_bdt_err};
use crate::executor::Executor;
use crate::finder::DeviceCache;
use crate::history::keystore::EncryptedKey;
use crate::protocol::{AckTunnel, Package, MTU, PackageCmdCode, SynTunnel};
use crate::protocol::v0::{SnCalled, AckStream, SynStream};
use crate::receive_processor::{ReceiveDispatcherRef, ReceiveProcessor,};
use crate::sockets::{NetManagerRef, QuicSocket};
use crate::sockets::tcp::TCPSocket;
use super::{Tunnel, TunnelDatagramRecv, TunnelDatagramSend, TunnelInstance, TunnelListenPorts, TunnelListenPortsRef, TunnelStream, TunnelType};

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
        let mut tunnel = self.tunnel.take().unwrap();
        self.tunnels.remove_tunnel(tunnel.get_sequence());
        // let ret = Executor::block_on(
        //     // tunnel.close()
        // );
        // match ret {
        //     Ok(_) => {
        //         self.tunnels.add_idle_tunnel(tunnel);
        //     }
        //     Err(err) => {
        //         log::error!("close tunnel error: {:?}", err);
        //     }
        // }
    }
}

struct TunnelsState {
    tunnels: HashMap<TempSeq, Arc<Tunnel>>,
}
struct Tunnels {
    state: Mutex<TunnelsState>
}

impl Tunnels {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(TunnelsState {
                tunnels: Default::default(),
            })
        })
    }

    pub fn get_idle_tunnel(&self) -> Option<Arc<Tunnel>> {
        let mut state = self.state.lock().unwrap();
        for (_, tunnel) in state.tunnels.iter() {
            if tunnel.is_idle() {
                return Some(tunnel.clone());
            }
        }
        None
    }

    pub fn add_tunnel(self: &Arc<Self>, tunnel: Tunnel, listener: Option<TunnelManagerEventRef>) {
        let mut state = self.state.lock().unwrap();
        let tunnel = Arc::new(tunnel);
        state.tunnels.insert(tunnel.get_sequence(), tunnel.clone());
        let this = self.clone();
        Executor::spawn(async move {
            loop {
                match tunnel.accept_instance().await {
                    Ok(instance) => {
                        match instance {
                            TunnelInstance::Stream(stream) => {
                                if listener.is_some() {
                                    listener.as_ref().unwrap().on_new_tunnel_stream(stream).await;
                                }
                            }
                            TunnelInstance::Datagram(datagram) => {
                                if listener.is_some() {
                                    listener.as_ref().unwrap().on_new_tunnel_datagram(datagram).await;
                                }
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
    }

    pub fn tunnel_exist(&self, seq: TempSeq) -> bool {
        let mut state = self.state.lock().unwrap();
        state.tunnels.contains_key(&seq)
    }

    pub fn remove_tunnel(&self, seq: TempSeq) {
        let mut state = self.state.lock().unwrap();
        state.tunnels.remove(&seq);
    }
}

#[async_trait::async_trait]
pub trait TunnelManagerEvent: 'static + Send + Sync {
    async fn on_new_tunnel_stream(&self, tunnel: Box<dyn TunnelStream>) -> ();
    async fn on_new_tunnel_datagram(&self, tunnel: Box<dyn TunnelDatagramRecv>) -> ();
}

pub type TunnelManagerEventRef = Arc<dyn TunnelManagerEvent>;

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
    tunnels: RwLock<HashMap<DeviceId, Arc<Tunnels>>>,
    net_manager: NetManagerRef,
    receive_dispatcher: ReceiveDispatcherRef,
    // sn_service: SNClientServiceRef,
    local_device: LocalDeviceRef,
    protocol_version: u8,
    stack_version: u32,
    listener: Mutex<Option<TunnelManagerEventRef>>,
    conn_timeout: Duration,
    gen_seq: Arc<TempSeqGenerator>,
    listen_ports: Arc<ListenPorts>,
}
pub type TunnelManagerRef = Arc<TunnelManager>;

impl TunnelManager {
    pub fn new(
        net_manager: NetManagerRef,
        receive_dispatcher: ReceiveDispatcherRef,
        // sn_service: SNClientServiceRef,
        local_device: LocalDeviceRef,
        protocol_version: u8,
        stack_version: u32,
        conn_timeout: Duration,) -> Arc<Self> {
        Arc::new(Self {
            tunnels: Default::default(),
            net_manager,
            receive_dispatcher,
            // sn_service,
            local_device,
            protocol_version,
            stack_version,
            listener: Mutex::new(None),
            conn_timeout,
            gen_seq: Arc::new(TempSeqGenerator::new()),
            listen_ports: ListenPorts::new(),
        })
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
        let remote = self.net_manager.get_device_cache().get(&remote_id).await.unwrap();
        let mut tunnel = Tunnel::new(
            self.net_manager.clone(),
            self.gen_seq.generate(),
            self.protocol_version,
            self.stack_version,
            remote,
            self.local_device.clone(),
            self.conn_timeout.clone(),
            self.listen_ports.clone());
        tunnel.accept_tcp_tunnel(socket)?;

        let listener = self.listener.lock().unwrap().clone();
        self.get_tunnels(&remote_id).add_tunnel(tunnel, listener);
        Ok(())
    }

    async fn on_new_quic_tunnel(&self, socket: QuicSocket) -> BdtResult<()> {
        let remote_id = socket.remote_device_id().clone();
        let remote = self.net_manager.get_device_cache().get(&remote_id).await.unwrap();
        let mut tunnel = Tunnel::new(
            self.net_manager.clone(),
            self.gen_seq.generate(),
            self.protocol_version,
            self.stack_version,
            remote,
            self.local_device.clone(),
            self.conn_timeout.clone(),
            self.listen_ports.clone());
        tunnel.accept_quic_tunnel(socket)?;

        let listener = self.listener.lock().unwrap().clone();
        self.get_tunnels(&remote_id).add_tunnel(tunnel, listener);
        Ok(())
    }

    pub(crate) fn set_listener(&self, listener: impl TunnelManagerEvent) {
        let mut listeners = self.listener.lock().unwrap();
        *listeners = Some(Arc::new(listener));
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
                seq,
                self.protocol_version,
                self.stack_version,
                remote.clone(),
                self.local_device.clone(),
                self.conn_timeout,
                self.listen_ports.clone(),
            );
            tunnel.connect().await?;
            let datagram = tunnel.open_datagram().await?;
            let listener = self.listener.lock().unwrap().clone();
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
                seq,
                self.protocol_version,
                self.stack_version,
                remote.clone(),
                self.local_device.clone(),
                self.conn_timeout,
                self.listen_ports.clone(),
            );
            tunnel.connect().await?;
            let stream = tunnel.open_stream(vport, session_id).await?;
            let listener = self.listener.lock().unwrap().clone();
            tunnels.add_tunnel(tunnel, listener);
            Ok(stream)
        }
    }

}

// #[async_trait::async_trait]
// impl SNEvent for TunnelManager {
//     async fn on_called(&self, called: &SnCalled) -> BdtResult<()> {
//         Ok(())
//     }
// }
