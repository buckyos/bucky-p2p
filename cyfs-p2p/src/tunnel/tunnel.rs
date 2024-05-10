use std::sync::{Arc, Mutex};
use std::time::Duration;
use callback_result::CallbackWaiter;
use cyfs_base::{bucky_time_now, BuckyError, BuckyErrorCode, BuckyResult, Device, DeviceDesc, NamedObject};
use crate::protocol::{AckTunnel, DynamicPackage, PackageCmdCode, SynTunnel};
use crate::sockets::{DataSender, NetManagerRef, SocketType};
use crate::{LocalDeviceRef, TempSeq};
use crate::protocol::v0::PingTunnel;
use crate::receive_processor::ReceiveDispatcherRef;
use crate::tunnel::tunnel_connection::{TunnelConnectionKey, TunnelWaiter, TunnelWaiterRef};
use crate::tunnel::{TunnelConnection};
use crate::tunnel::tcp_tunnel_connection::TcpTunnelConnection;
use crate::tunnel::udp_tunnel_connection::UdpTunnelConnection;


pub enum TunnelStatus {
    Connecting,
    Active,
    Dead,
}

struct TunnelState {
    status: TunnelStatus,
}

pub struct Tunnel {
    net_manager: NetManagerRef,
    receive_dispatcher: ReceiveDispatcherRef,
    sequence: TempSeq,
    tunnel_conn: Option<Arc<dyn TunnelConnection>>,
    resp_waiter: TunnelWaiterRef,
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
        resp_waiter: TunnelWaiterRef ) -> Self {
        Self {
            net_manager,
            receive_dispatcher,
            sequence,
            tunnel_conn: None,
            resp_waiter,
            state: Mutex::new(TunnelState { status: TunnelStatus::Connecting }),
            protocol_version,
            stack_version,
            duration: Default::default(),
            local_device,
            remote_device,
            conn_timeout,
        }
    }

    pub fn new_for_connected(
        net_manager: NetManagerRef,
        receive_dispatcher: ReceiveDispatcherRef,
        sequence: TempSeq,
        protocol_version: u8,
        stack_version: u32,
        remote_device: Device,
        local_device: LocalDeviceRef,
        conn_timeout: Duration,
        data_sender: Arc<dyn DataSender>,
        resp_waiter: TunnelWaiterRef) -> Self {
        let tunnel_conn = match data_sender.socket_type() {
            SocketType::TCP => {
                let processor = receive_dispatcher.get_processor(local_device.device_id()).unwrap();
                let tunnel_conn = TcpTunnelConnection::new(net_manager.clone(),
                                                           resp_waiter.clone(),
                                                           sequence,
                                                           local_device.clone(),
                                                           remote_device.desc().clone(),
                                                           data_sender.remote().clone(),
                                                           conn_timeout,
                                                           protocol_version,
                                                           stack_version,
                                                           Some(data_sender),
                                                           processor);
                Arc::new(tunnel_conn) as Arc<dyn TunnelConnection>
            }
            SocketType::UDP => {
                let tunnel_conn = UdpTunnelConnection::new(net_manager.clone(),
                                                               resp_waiter.clone(),
                                                               sequence,
                                                               local_device.clone(),
                                                               remote_device.desc().clone(),
                                                               data_sender.remote().clone(),
                                                               conn_timeout,
                                                               protocol_version,
                                                               stack_version,
                                                               data_sender.local().clone(),
                                                               Some(data_sender));
                Arc::new(tunnel_conn) as Arc<dyn TunnelConnection>
            }
        };

        Self {
            net_manager,
            receive_dispatcher,
            sequence,
            tunnel_conn: Some(tunnel_conn),
            resp_waiter,
            state: Mutex::new(TunnelState { status: TunnelStatus::Connecting }),
            protocol_version,
            stack_version,
            duration: Default::default(),
            local_device,
            remote_device,
            conn_timeout,
        }
    }
    pub fn get_sequence(&self) -> TempSeq {
        self.sequence
    }

    pub fn get_resp_waiter(&self) -> &TunnelWaiterRef {
        &self.resp_waiter
    }

    pub async fn connect_tunnel(&mut self) -> BuckyResult<()> {
        if self.tunnel_conn.is_some() {
            return Ok(());
        }

        let ep_list = self.remote_device.connect_info().endpoints();
        for ep in ep_list.iter() {
            if ep.is_tcp() {
                if ep.is_static_wan() {
                    let processor = self.receive_dispatcher.get_processor(self.local_device.device_id()).unwrap();
                    let mut tunnel_conn = TcpTunnelConnection::new(self.net_manager.clone(),
                                                                   self.resp_waiter.clone(),
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
                    self.tunnel_conn = Some(Arc::new(tunnel_conn));
                    return Ok(());
                }
            } else if ep.is_udp() {
                if ep.is_static_wan() {
                    for listener in self.net_manager.udp_listeners().iter() {
                        let local_ep = listener.local();
                        let mut tunnel_conn = UdpTunnelConnection::new(self.net_manager.clone(),
                                                                       self.resp_waiter.clone(),
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
                        self.tunnel_conn = Some(Arc::new(tunnel_conn));
                        return Ok(());
                    }
                }
            }
        }
        Err(BuckyError::new(BuckyErrorCode::ConnectFailed, "No available endpoint".to_string()))
    }

}
