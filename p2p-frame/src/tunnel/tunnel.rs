use std::future::Future;
use std::net::{SocketAddr};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use bucky_raw_codec::{RawConvertTo, RawDecode, RawEncode};
use notify_future::NotifyFuture;
use crate::endpoint::{Endpoint, EndpointArea, Protocol};
use crate::error::{bdt_err, BdtErrorCode, BdtResult, into_bdt_err};
use crate::p2p_identity::{P2pId, LocalDeviceRef, P2pIdentityCertFactoryRef};
use crate::runtime;
use crate::sn::client::SNClientServiceRef;
use crate::sockets::NetManagerRef;
use crate::tunnel::{select_successful, QuicTunnelConnection, SocketType, TcpTunnelConnection, TunnelConnection, TunnelDatagramSend, TunnelInstance, TunnelListenPortsRef, TunnelStatRef, TunnelStream};
use crate::types::{IncreaseId, TempSeq};

pub enum ReverseResult {
    Stream(Box<dyn TunnelConnection>, Box<dyn TunnelStream>),
    Datagram(Box<dyn TunnelConnection>, Box<dyn TunnelDatagramSend>),
}

pub trait ReverseFutureCache: 'static + Send + Sync {
    fn add_reverse_future(&self, sequence: TempSeq, future: NotifyFuture<ReverseResult>);
    fn remove_reverse_future(&self, sequence: TempSeq);
}

#[derive(RawEncode, RawDecode)]
pub struct StreamSnCall {
    pub vport: u16,
    pub session_id: IncreaseId,
}

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
    sn_service: SNClientServiceRef,
    sequence: TempSeq,
    tunnel_conn: Option<Box<dyn TunnelConnection>>,
    state: Mutex<TunnelState>,
    protocol_version: u8,
    stack_version: u32,
    local_device: LocalDeviceRef,
    remote_id: P2pId,
    remote_eps: Vec<Endpoint>,
    conn_timeout: Duration,
    idle_timeout: Duration,
    listen_ports: TunnelListenPortsRef,
    cert_factory: P2pIdentityCertFactoryRef,
}

impl Tunnel {
    pub fn new(
        net_manager: NetManagerRef,
        sn_service: SNClientServiceRef,
        sequence: TempSeq,
        protocol_version: u8,
        stack_version: u32,
        remote_id: P2pId,
        remote_eps: Vec<Endpoint>,
        local_device: LocalDeviceRef,
        conn_timeout: Duration,
        idle_timeout: Duration,
        listen_ports: TunnelListenPortsRef,
        cert_factory: P2pIdentityCertFactoryRef,) -> Self {
        Self {
            net_manager,
            sn_service,
            sequence,
            tunnel_conn: None,
            state: Mutex::new(TunnelState { status: TunnelStatus::Connecting }),
            protocol_version,
            stack_version,
            local_device,
            remote_id,
            remote_eps,
            conn_timeout,
            idle_timeout,
            listen_ports,
            cert_factory,
        }
    }

    pub fn set_tunnel_conn(&mut self, tunnel_conn: Box<dyn TunnelConnection>) {
        self.tunnel_conn = Some(tunnel_conn);
    }

    pub async fn accept_instance(&self) -> BdtResult<TunnelInstance> {
        self.tunnel_conn.as_ref().unwrap().accept_instance().await
    }

    pub fn get_sequence(&self) -> TempSeq {
        self.sequence
    }

    pub fn socket_type(&self) -> SocketType {
        self.tunnel_conn.as_ref().unwrap().socket_type()
    }

    pub fn tunnel_stat(&self) -> TunnelStatRef {
        self.tunnel_conn.as_ref().unwrap().tunnel_stat()
    }

    pub fn is_idle(&self) -> bool {
        self.tunnel_conn.as_ref().unwrap().is_idle()
    }

    fn get_reverse_ep_list(&self) -> Vec<Endpoint> {
        let mut wan_udp_eps = Vec::new();
        self.sn_service.get_active_sn_list().iter().map(|v| v.wan_ep_list.iter()).flatten().for_each(|ep| {
            let mut ep = ep.clone();
            ep.set_area(EndpointArea::Wan);
            wan_udp_eps.push(ep);
        });
        let mut reverse_eps = Vec::new();
        for listener in self.net_manager.tcp_listeners() {
            listener.mapping_port().map(|port| {
                for ep in wan_udp_eps.iter() {
                    let mut tcp_ep = Endpoint::from((Protocol::Tcp, SocketAddr::new(ep.addr().ip(), port)));
                    tcp_ep.set_area(EndpointArea::Wan);
                    reverse_eps.push(tcp_ep);
                }
            });
        }
        reverse_eps.extend_from_slice(wan_udp_eps.as_slice());
        reverse_eps
    }

    pub async fn connect_stream(&mut self, vport: u16, session_id: IncreaseId, future_cache: Arc<dyn ReverseFutureCache>) -> BdtResult<Box<dyn TunnelStream>> {
        if self.tunnel_conn.is_some() {
            let stream = self.tunnel_conn.as_ref().unwrap().connect_stream(vport, session_id).await.map_err(into_bdt_err!(BdtErrorCode::ConnectFailed))?;
            return Ok(stream);
        }

        let mut futures: Vec<Pin<Box<dyn Future<Output=BdtResult<(Box<dyn TunnelConnection>, Box<dyn TunnelStream>)>> + Send>>> = Vec::new();
        let ep_list = &self.remote_eps;
        let is_lan = self.sn_service.is_same_lan(ep_list);
        for ep in ep_list.iter() {
            if ep.is_tcp() && (is_lan || ep.is_static_wan()) && ep.addr().is_ipv4() {
                let tunnel_conn: Box<dyn TunnelConnection> = Box::new(TcpTunnelConnection::new(
                    self.sequence,
                    self.local_device.clone(),
                    self.remote_id.clone(),
                    ep.clone(),
                    self.conn_timeout,
                    self.protocol_version,
                    self.stack_version,
                    None, self.listen_ports.clone(),
                    self.cert_factory.clone())?);
                let future = Box::pin(async move {
                    let stream = tunnel_conn.connect_stream(vport, session_id).await?;
                    Ok((tunnel_conn, stream))
                });
                futures.push(future);
            } else if ep.is_udp() && (is_lan || ep.is_static_wan()) && ep.addr().is_ipv4() {
                for listener in self.net_manager.quic_listeners().iter() {
                    let local_ep = listener.local();
                    let tunnel_conn: Box<dyn TunnelConnection> = Box::new(QuicTunnelConnection::new(
                        self.net_manager.clone(),
                        self.sequence,
                        self.local_device.clone(),
                        self.remote_id.clone(),
                        ep.clone(),
                        self.conn_timeout,
                        self.idle_timeout,
                        self.protocol_version,
                        self.stack_version,
                        local_ep.clone(),
                        None,
                        self.listen_ports.clone(),
                        self.cert_factory.clone()));
                    let future = Box::pin(async move {
                        let stream = tunnel_conn.connect_stream(vport, session_id).await?;
                        Ok((tunnel_conn, stream))
                    });
                    futures.push(future);
                }
            }
        }

        if futures.len() > 0 {
            match select_successful(futures).await {
                Ok((conn, stream)) => {
                    self.tunnel_conn = Some(conn);
                    return Ok(stream);
                }
                Err(e) => {
                    log::error!("connect stream error: {:?} msg: {}", e.code(), e.msg());
                }
            }
        }
        let reverse_eps = self.get_reverse_ep_list();

        let future = NotifyFuture::new();
        future_cache.add_reverse_future(self.sequence, future.clone());
        let call_data = StreamSnCall {
            vport,
            session_id,
        };
        self.sn_service.call(self.get_sequence(),
                             Some(reverse_eps.as_slice()),
                             &self.remote_id,
                             call_data.to_vec().map_err(into_bdt_err!(BdtErrorCode::RawCodecError))?).await?;
        let result = runtime::timeout(self.conn_timeout, future).await.map_err(into_bdt_err!(BdtErrorCode::Timeout))?;
        if let ReverseResult::Stream(tunnel_conn, stream) = result {
            self.tunnel_conn = Some(tunnel_conn);
            Ok(stream)
        } else {
            Err(bdt_err!(BdtErrorCode::ConnectFailed, "No available endpoint"))
        }
    }

    pub async fn connect_datagram(&mut self, future_cache: Arc<dyn ReverseFutureCache>) -> BdtResult<Box<dyn TunnelDatagramSend>> {
        if self.tunnel_conn.is_some() {
            let datagram = self.tunnel_conn.as_ref().unwrap().connect_datagram().await.map_err(into_bdt_err!(BdtErrorCode::ConnectFailed))?;
            return Ok(datagram);
        }

        let mut futures: Vec<Pin<Box<dyn Future<Output=BdtResult<(Box<dyn TunnelConnection>, Box<dyn TunnelDatagramSend>)>> + Send>>> = Vec::new();
        let ep_list = &self.remote_eps;
        let is_lan = self.sn_service.is_same_lan(ep_list);
        for ep in ep_list.iter() {
            if ep.is_tcp() && (is_lan || ep.is_static_wan()) && ep.addr().is_ipv4() {
                let tunnel_conn: Box<dyn TunnelConnection> = Box::new(TcpTunnelConnection::new(
                    self.sequence,
                    self.local_device.clone(),
                    self.remote_id.clone(),
                    ep.clone(),
                    self.conn_timeout,
                    self.protocol_version,
                    self.stack_version,
                    None,
                    self.listen_ports.clone(),
                    self.cert_factory.clone())?);

                let future = Box::pin(async move {
                    let stream = tunnel_conn.connect_datagram().await?;
                    Ok((tunnel_conn, stream))
                });
                futures.push(future);
            } else if ep.is_udp() && (is_lan || ep.is_static_wan()) && ep.addr().is_ipv4() {
                for listener in self.net_manager.quic_listeners().iter() {
                    let local_ep = listener.local();
                    let tunnel_conn: Box<dyn TunnelConnection> = Box::new(QuicTunnelConnection::new(
                        self.net_manager.clone(),
                        self.sequence,
                        self.local_device.clone(),
                        self.remote_id.clone(),
                        ep.clone(),
                        self.conn_timeout,
                        self.idle_timeout,
                        self.protocol_version,
                        self.stack_version,
                        local_ep.clone(),
                        None,
                        self.listen_ports.clone(),
                        self.cert_factory.clone()));

                    let future = Box::pin(async move {
                        let stream = tunnel_conn.connect_datagram().await?;
                        Ok((tunnel_conn, stream))
                    });
                    futures.push(future);
                }
            }
        }

        if futures.len() > 0 {
            match select_successful(futures).await {
                Ok((conn, datagram_send)) => {
                    self.tunnel_conn = Some(conn);
                    return Ok(datagram_send);
                }
                Err(e) => {
                    log::error!("connect stream error: {:?} msg: {}", e.code(), e.msg());
                }
            }
        }

        let reverse_eps = self.get_reverse_ep_list();

        let future = NotifyFuture::new();
        future_cache.add_reverse_future(self.sequence, future.clone());
        self.sn_service.call(self.get_sequence(),
                             Some(reverse_eps.as_slice()),
                             &self.remote_id,
                             Vec::new()).await?;
        let result = runtime::timeout(self.conn_timeout, future).await.map_err(into_bdt_err!(BdtErrorCode::Timeout))?;
        if let ReverseResult::Datagram(tunnel_conn, stream) = result {
            self.tunnel_conn = Some(tunnel_conn);
            Ok(stream)
        } else {
            Err(bdt_err!(BdtErrorCode::ConnectFailed, "No available endpoint"))
        }
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
