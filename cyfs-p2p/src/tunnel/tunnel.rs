use std::any::Any;
use std::collections::HashSet;
use std::future::Future;
use std::net::Shutdown;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use as_any::Downcast;
use bucky_objects::{Device, DeviceId, NamedObject};
use callback_result::CallbackWaiter;
use crate::protocol::{AckTunnel, Package, PackageCmdCode, SynTunnel};
use crate::sockets::{NetManagerRef, QuicSocket};
use crate::{IncreaseId, LocalDeviceRef, MixAesKey, TempSeq};
use crate::error::{bdt_err, BdtErrorCode, BdtResult, into_bdt_err};
use crate::receive_processor::ReceiveDispatcherRef;
use crate::sockets::tcp::TCPSocket;
use crate::tunnel::{SocketType, TunnelConnection, TunnelDatagramSend, TunnelInstance, TunnelListenPorts, TunnelListenPortsRef, TunnelStatRef, TunnelStream, TunnelType};
use crate::tunnel::tcp_tunnel_connection::TcpTunnelConnection;
use crate::tunnel::quic_tunnel_connection::{QuicTunnelConnection};


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
    sequence: TempSeq,
    tunnel_conn: Option<Box<dyn TunnelConnection>>,
    state: Mutex<TunnelState>,
    protocol_version: u8,
    stack_version: u32,
    local_device: LocalDeviceRef,
    remote: Device,
    conn_timeout: Duration,
    listen_ports: TunnelListenPortsRef,
}

impl Tunnel {
    pub fn new(
        net_manager: NetManagerRef,
        sequence: TempSeq,
        protocol_version: u8,
        stack_version: u32,
        remote: Device,
        local_device: LocalDeviceRef,
        conn_timeout: Duration,
        listen_ports: TunnelListenPortsRef,) -> Self {
        Self {
            net_manager,
            sequence,
            tunnel_conn: None,
            state: Mutex::new(TunnelState { status: TunnelStatus::Connecting }),
            protocol_version,
            stack_version,
            local_device,
            remote,
            conn_timeout,
            listen_ports,
        }
    }

    pub(crate) fn accept_tcp_tunnel(&mut self, socket: TCPSocket) -> BdtResult<()> {
        let mut tunnel_conn = TcpTunnelConnection::new(
            self.sequence,
            self.local_device.clone(),
            self.remote.desc().device_id(),
            socket.remote().clone(),
            self.conn_timeout,
            self.protocol_version,
            self.stack_version,
            Some(socket),
            self.listen_ports.clone())?;
        // tunnel_conn.accept_tunnel(listen_ports).await?;
        self.tunnel_conn = Some(Box::new(tunnel_conn));
        Ok(())
    }

    pub(crate) fn accept_quic_tunnel(&mut self, data_sender: QuicSocket) -> BdtResult<()> {
        let tunnel_conn = QuicTunnelConnection::new(
            self.net_manager.clone(),
            self.sequence,
            self.local_device.clone(),
            self.remote.desc().device_id(),
            data_sender.remote().clone(),
            self.conn_timeout,
            self.protocol_version,
            self.stack_version,
            data_sender.local().clone(),
            Some(data_sender),
            self.listen_ports.clone());

        self.tunnel_conn = Some(Box::new(tunnel_conn));
        Ok(())
    }

    pub(crate) async fn accept_instance(&self) -> BdtResult<TunnelInstance> {
        self.tunnel_conn.as_ref().unwrap().accept_instance().await
    }

    pub fn get_sequence(&self) -> TempSeq {
        self.sequence
    }

    pub fn socket_type(&self) -> SocketType {
        self.tunnel_conn.as_ref().unwrap().socket_type()
    }

    pub(crate) fn tunnel_stat(&self) -> TunnelStatRef {
        self.tunnel_conn.as_ref().unwrap().tunnel_stat()
    }

    pub fn is_idle(&self) -> bool {
        self.tunnel_conn.as_ref().unwrap().is_idle()
    }

    pub async fn connect(&mut self) -> BdtResult<()> {
        if self.tunnel_conn.is_some() {
            return Ok(());
        }

        let ep_list = self.remote.connect_info().endpoints();
        for ep in ep_list.iter() {
            if ep.is_tcp() {
                if ep.is_static_wan() {
                    let mut tunnel_conn = TcpTunnelConnection::new(
                        self.sequence,
                        self.local_device.clone(),
                        self.remote.desc().device_id(),
                        ep.clone(),
                        self.conn_timeout,
                        self.protocol_version,
                        self.stack_version,
                        None, self.listen_ports.clone())?;
                    tunnel_conn.connect().await.map_err(into_bdt_err!(BdtErrorCode::ConnectFailed))?;
                    self.tunnel_conn = Some(Box::new(tunnel_conn));
                    return Ok(());
                }
            } else if ep.is_udp() {
                if ep.is_static_wan() {
                    for listener in self.net_manager.udp_listeners().iter() {
                        let local_ep = listener.local();
                        let mut tunnel_conn = QuicTunnelConnection::new(self.net_manager.clone(),
                                                                        self.sequence,
                                                                        self.local_device.clone(),
                                                                        self.remote.desc().device_id(),
                                                                        ep.clone(),
                                                                        self.conn_timeout,
                                                                        self.protocol_version,
                                                                        self.stack_version,
                                                                        local_ep.clone(),
                                                                        None,
                                                                        self.listen_ports.clone());
                        tunnel_conn.connect().await.map_err(into_bdt_err!(BdtErrorCode::ConnectFailed))?;
                        self.tunnel_conn = Some(Box::new(tunnel_conn));
                        return Ok(());
                    }
                }
            }
        }
        Err(bdt_err!(BdtErrorCode::ConnectFailed, "No available endpoint"))
    }

    pub async fn open_stream(&self, vport: u16, session_id: IncreaseId) -> BdtResult<Box<dyn TunnelStream>> {
        if self.tunnel_conn.is_none() {
            return Err(bdt_err!(BdtErrorCode::TunnelNotConnected, "Tunnel not connected"));
        }

        let stream = self.tunnel_conn.as_ref().unwrap().open_stream(vport, session_id).await?;
        log::info!("Open stream tunnel {:?} session_id {:?} vport {} remote_id {} remote_ep {} local_id {} local_ep {}",
            stream.sequence(),
            stream.session_id(),
            vport,
            stream.remote_device_id().to_string(),
            stream.remote_endpoint().to_string(),
            stream.local_device_id().to_string(),
            stream.local_endpoint().to_string());

        Ok(stream)
    }

    pub async fn open_datagram(&self) -> BdtResult<Box<dyn TunnelDatagramSend>> {
        if self.tunnel_conn.is_none() {
            return Err(bdt_err!(BdtErrorCode::TunnelNotConnected, "Tunnel not connected"));
        }

        let datagram = self.tunnel_conn.as_ref().unwrap().open_datagram().await?;

        log::info!("Open stream tunnel {:?} remote_id {} remote_ep {} local_id {} local_ep {}",
            datagram.sequence(),
            datagram.remote_device_id().to_string(),
            datagram.remote_endpoint().to_string(),
            datagram.local_device_id().to_string(),
            datagram.local_endpoint().to_string());

        Ok(datagram)
    }

    pub async fn shutdown(&self) -> BdtResult<()> {
        log::info!("shutdown tunnel {:?}", self.sequence);
        if self.tunnel_conn.is_some() {
            self.tunnel_conn.as_ref().unwrap().shutdown().await
        } else {
            Ok(())
        }
    }
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        log::info!("drop tunnel {:?}", self.sequence);
    }
}
