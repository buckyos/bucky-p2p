use std::any::Any;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use as_any::Downcast;
use callback_result::CallbackWaiter;
use cyfs_base::{bucky_time_now, BuckyError, BuckyErrorCode, Device, DeviceDesc, NamedObject};
use crate::protocol::{AckTunnel, DynamicPackage, PackageBox, PackageCmdCode, SynTunnel};
use crate::sockets::{DataSender, NetManagerRef, SocketType};
use crate::{IncreaseId, LocalDeviceRef, TempSeq};
use crate::error::{bdt_err, BdtErrorCode, BdtResult};
use crate::protocol::v0::PingTunnel;
use crate::receive_processor::ReceiveDispatcherRef;
use crate::sockets::tcp::TCPSocket;
use crate::tunnel::tunnel_connection::{TunnelConnectionKey};
use crate::tunnel::{TunnelConnection};
use crate::tunnel::tcp_tunnel_connection::TcpTunnelConnection;
use crate::tunnel::udp_tunnel_connection::{UdpTunnelConnection, UdpTunnelDataReceiverRef};


pub enum TunnelStatus {
    Connecting,
    Active,
    Dead,
}

struct TunnelState {
    status: TunnelStatus,
}

pub struct TunnelReceiver {

}

pub struct Tunnel {
    net_manager: NetManagerRef,
    receive_dispatcher: ReceiveDispatcherRef,
    sequence: TempSeq,
    tunnel_conn: Option<Box<dyn TunnelConnection>>,
    data_receiver: UdpTunnelDataReceiverRef,
    state: Mutex<TunnelState>,
    protocol_version: u8,
    stack_version: u32,
    duration: Duration,
    local_device: LocalDeviceRef,
    remote_device: Device,
    conn_timeout: Duration,
}

impl Tunnel {
    pub fn new(
        net_manager: NetManagerRef,
        receive_dispatcher: ReceiveDispatcherRef,
        sequence: TempSeq,
        protocol_version: u8,
        stack_version: u32,
        remote_device: Device,
        local_device: LocalDeviceRef,
        conn_timeout: Duration,
        data_receiver: UdpTunnelDataReceiverRef, ) -> Self {
        Self {
            net_manager,
            receive_dispatcher,
            sequence,
            tunnel_conn: None,
            data_receiver,
            state: Mutex::new(TunnelState { status: TunnelStatus::Connecting }),
            protocol_version,
            stack_version,
            duration: Default::default(),
            local_device,
            remote_device,
            conn_timeout,
        }
    }

    pub fn accept_tcp_tunnel(mut self, socket: Arc<TCPSocket>, first_box: PackageBox) -> impl Future<Output=BdtResult<Self>> + Send {
        let mut tunnel_conn = TcpTunnelConnection::new(self.net_manager.clone(),
                                                       self.sequence,
                                                       self.local_device.clone(),
                                                       self.remote_device.desc().clone(),
                                                       socket.remote().clone(),
                                                       self.conn_timeout,
                                                       self.protocol_version,
                                                       self.stack_version,
                                                       Some(socket),
                                                       self.receive_dispatcher.get_processor(self.local_device.device_id()).unwrap());
        async move {
            tunnel_conn.accept_tunnel(first_box.packages_no_exchange()).await?;
            self.tunnel_conn = Some(Box::new(tunnel_conn));
            Ok(self)
        }
    }

    pub fn accept_udp_tunnel(mut self, data_sender: Arc<dyn DataSender>) -> impl Future<Output=BdtResult<Self>> + Send {
        let mut tunnel_conn = match data_sender.socket_type() {
            SocketType::TCP => {
                unreachable!("TCP socket is not supported")
            }
            SocketType::UDP => {
                let tunnel_conn = UdpTunnelConnection::new(self.net_manager.clone(),
                                                           self.data_receiver.clone(),
                                                           self.sequence,
                                                           self.local_device.clone(),
                                                           self.remote_device.desc().clone(),
                                                           data_sender.remote().clone(),
                                                           self.conn_timeout,
                                                           self.protocol_version,
                                                           self.stack_version,
                                                           data_sender.local().clone(),
                                                           Some(data_sender));
                Box::new(tunnel_conn)
            }
        };

        async move {
            tunnel_conn.accept_tunnel().await?;
            self.tunnel_conn = Some(tunnel_conn);
            Ok(self)
        }
    }

    pub fn get_sequence(&self) -> TempSeq {
        self.sequence
    }
    pub fn get_tunnel_connection<T: TunnelConnection>(&self) -> Option<&T> {
        self.tunnel_conn.as_ref().and_then(|conn| {
            conn.downcast_ref::<T>()
        })
    }

    pub async fn connect_tunnel(&mut self) -> BdtResult<()> {
        if self.tunnel_conn.is_some() {
            return Ok(());
        }

        let ep_list = self.remote_device.connect_info().endpoints();
        for ep in ep_list.iter() {
            if ep.is_tcp() {
                if ep.is_static_wan() {
                    let processor = self.receive_dispatcher.get_processor(self.local_device.device_id()).unwrap();
                    let mut tunnel_conn = TcpTunnelConnection::new(self.net_manager.clone(),
                                                                   self.sequence,
                                                                   self.local_device.clone(),
                                                                   self.remote_device.desc().clone(),
                                                                   ep.clone(),
                                                                   self.conn_timeout,
                                                                   self.protocol_version,
                                                                   self.stack_version,
                                                                   None,
                                                                   processor);
                    tunnel_conn.connect_tunnel().await?;
                    self.tunnel_conn = Some(Box::new(tunnel_conn));
                    return Ok(());
                }
            } else if ep.is_udp() {
                if ep.is_static_wan() {
                    for listener in self.net_manager.udp_listeners().iter() {
                        let local_ep = listener.local();
                        let mut tunnel_conn = UdpTunnelConnection::new(self.net_manager.clone(),
                                                                       self.data_receiver.clone(),
                                                                       self.sequence,
                                                                       self.local_device.clone(),
                                                                       self.remote_device.desc().clone(),
                                                                       ep.clone(),
                                                                       self.conn_timeout,
                                                                       self.protocol_version,
                                                                       self.stack_version,
                                                                       local_ep.clone(),
                                                                       None);
                        tunnel_conn.connect_tunnel().await?;
                        self.tunnel_conn = Some(Box::new(tunnel_conn));
                        return Ok(());
                    }
                }
            }
        }
        Err(bdt_err!(BdtErrorCode::ConnectFailed, "No available endpoint"))
    }

    pub async fn connect_stream_tunnel(&mut self, vport: u16, session_id: IncreaseId) -> BdtResult<()> {
        if self.tunnel_conn.is_some() {
            return Ok(());
        }

        let ep_list = self.remote_device.connect_info().endpoints();
        for ep in ep_list.iter() {
            if ep.is_tcp() {
                if ep.is_static_wan() {
                    let processor = self.receive_dispatcher.get_processor(self.local_device.device_id()).unwrap();
                    let mut tunnel_conn = TcpTunnelConnection::new(self.net_manager.clone(),
                                                                   self.sequence,
                                                                   self.local_device.clone(),
                                                                   self.remote_device.desc().clone(),
                                                                   ep.clone(),
                                                                   self.conn_timeout,
                                                                   self.protocol_version,
                                                                   self.stack_version,
                                                                   None,
                                                                   processor);
                    tunnel_conn.connect_stream(vport, session_id).await?;
                    self.tunnel_conn = Some(Box::new(tunnel_conn));
                    return Ok(());
                }
            } else if ep.is_udp() {
                if ep.is_static_wan() {
                    for listener in self.net_manager.udp_listeners().iter() {
                        let local_ep = listener.local();
                        let mut tunnel_conn = UdpTunnelConnection::new(self.net_manager.clone(),
                                                                       self.data_receiver.clone(),
                                                                       self.sequence,
                                                                       self.local_device.clone(),
                                                                       self.remote_device.desc().clone(),
                                                                       ep.clone(),
                                                                       self.conn_timeout,
                                                                       self.protocol_version,
                                                                       self.stack_version,
                                                                       local_ep.clone(),
                                                                       None);
                        tunnel_conn.connect_stream(vport, session_id).await?;
                        self.tunnel_conn = Some(Box::new(tunnel_conn));
                        return Ok(());
                    }
                }
            }
        }
        Err(bdt_err!(BdtErrorCode::ConnectFailed, "No available endpoint"))
    }
}
