use std::future::Future;
use std::net::{SocketAddr};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use bucky_raw_codec::{RawConvertTo, RawDecode, RawEncode};
use notify_future::NotifyFuture;
use crate::endpoint::{Endpoint, EndpointArea};
use crate::error::{p2p_err, P2pErrorCode, P2pResult, into_p2p_err};
use crate::p2p_identity::{P2pId, P2pIdentityRef, P2pIdentityCertFactoryRef};
use crate::runtime;
use crate::sn::client::SNClientServiceRef;
use crate::sockets::NetManagerRef;
use crate::tunnel::{select_successful, TunnelConnection, TunnelDatagramRecv, TunnelDatagramSend, TunnelInstance, TunnelListenPortsRef, TunnelStatRef, TunnelStream};
use crate::types::{IncreaseId, TempSeq};

pub enum ReverseResult {
    Stream(TunnelConnection, TunnelStream),
    Datagram(TunnelConnection, TunnelDatagramSend),
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
    tunnel_conn: Option<TunnelConnection>,
    state: Mutex<TunnelState>,
    protocol_version: u8,
    stack_version: u32,
    local_identity: P2pIdentityRef,
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
        local_identity: P2pIdentityRef,
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
            local_identity,
            remote_id,
            remote_eps,
            conn_timeout,
            idle_timeout,
            listen_ports,
            cert_factory,
        }
    }

    pub fn set_tunnel_conn(&mut self, tunnel_conn: TunnelConnection) {
        self.tunnel_conn = Some(tunnel_conn);
    }

    pub async fn accept_instance(&self) -> P2pResult<TunnelInstance> {
        self.tunnel_conn.as_ref().unwrap().accept_instance().await
    }

    pub fn get_sequence(&self) -> TempSeq {
        self.sequence
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
        for (ep, port) in self.net_manager.port_mapping() {
            for udp_ep in wan_udp_eps.iter() {
                let mut ep = Endpoint::from((ep.protocol(), SocketAddr::new(udp_ep.addr().ip(), port)));
                ep.set_area(EndpointArea::Wan);
                reverse_eps.push(ep);
            }
        }
        reverse_eps.extend_from_slice(wan_udp_eps.as_slice());
        reverse_eps
    }

    pub async fn connect_stream(&mut self, vport: u16, session_id: IncreaseId, future_cache: Arc<dyn ReverseFutureCache>) -> P2pResult<TunnelStream> {
        if self.tunnel_conn.is_some() {
            let stream = self.tunnel_conn.as_ref().unwrap().connect_stream(vport, session_id).await.map_err(into_p2p_err!(P2pErrorCode::ConnectFailed))?;
            return Ok(stream);
        }

        let mut futures: Vec<Pin<Box<dyn Future<Output=P2pResult<(TunnelConnection, TunnelStream)>> + Send>>> = Vec::new();
        let ep_list = &self.remote_eps;
        let is_lan = self.sn_service.is_same_lan(ep_list);
        for ep in ep_list.iter() {
            if is_lan || ep.is_static_wan() {
                let sequence = self.sequence;
                let local_identity = self.local_identity.clone();
                let remote_id = self.remote_id.clone();
                let conn_timeout = self.conn_timeout;
                let protocol_version = self.protocol_version;
                let stack_version = self.stack_version;
                let listen_ports = self.listen_ports.clone();
                let cert_factory = self.cert_factory.clone();
                let net_manager = self.net_manager.clone();
                let future = Box::pin(async move {
                    let conns = net_manager.get_network(ep.protocol())?.create_stream_connect(&local_identity, ep, &remote_id).await?;
                    let mut futures: Vec<Pin<Box<dyn Future<Output=P2pResult<(TunnelConnection, TunnelStream)>> + Send>>> = Vec::new();
                    for conn in conns {
                        let tunnel_conn = TunnelConnection::new(
                            sequence,
                            local_identity.clone(),
                            remote_id.clone(),
                            ep.clone(),
                            conn_timeout,
                            protocol_version,
                            stack_version,
                            Some(conn),
                            listen_ports.clone(),
                            cert_factory.clone())?;
                        let future = Box::pin(async move {
                            let stream = tunnel_conn.connect_stream(vport, session_id).await?;
                            Ok((tunnel_conn, stream))
                        });
                        futures.push(future);
                    }
                    if futures.len() > 0 {
                        select_successful(futures).await
                    } else {
                        Err(p2p_err!(P2pErrorCode::ConnectFailed, "No available endpoint"))
                    }
                });
                futures.push(future)
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
                             call_data.to_vec().map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?).await?;
        let result = runtime::timeout(self.conn_timeout, future).await.map_err(into_p2p_err!(P2pErrorCode::Timeout))?;
        if let ReverseResult::Stream(tunnel_conn, stream) = result {
            self.tunnel_conn = Some(tunnel_conn);
            Ok(stream)
        } else {
            Err(p2p_err!(P2pErrorCode::ConnectFailed, "No available endpoint"))
        }
    }

    pub async fn connect_datagram(&mut self, future_cache: Arc<dyn ReverseFutureCache>) -> P2pResult<TunnelDatagramSend> {
        if self.tunnel_conn.is_some() {
            let datagram = self.tunnel_conn.as_ref().unwrap().connect_datagram().await.map_err(into_p2p_err!(P2pErrorCode::ConnectFailed))?;
            return Ok(datagram);
        }

        let mut futures: Vec<Pin<Box<dyn Future<Output=P2pResult<(TunnelConnection, TunnelDatagramSend)>> + Send>>> = Vec::new();
        let ep_list = &self.remote_eps;
        let is_lan = self.sn_service.is_same_lan(ep_list);
        for ep in ep_list.iter() {
            if is_lan || ep.is_static_wan() {
                let sequence = self.sequence;
                let local_identity = self.local_identity.clone();
                let remote_id = self.remote_id.clone();
                let conn_timeout = self.conn_timeout;
                let protocol_version = self.protocol_version;
                let stack_version = self.stack_version;
                let listen_ports = self.listen_ports.clone();
                let cert_factory = self.cert_factory.clone();
                let net_manager = self.net_manager.clone();
                let future = Box::pin(async move {
                    let conns = net_manager.get_network(ep.protocol())?.create_stream_connect(&local_identity, ep, &remote_id).await?;
                    let mut futures: Vec<Pin<Box<dyn Future<Output=P2pResult<(TunnelConnection, TunnelDatagramSend)>> + Send>>> = Vec::new();
                    for conn in conns {
                        let tunnel_conn = TunnelConnection::new(
                            sequence,
                            local_identity.clone(),
                            remote_id.clone(),
                            ep.clone(),
                            conn_timeout,
                            protocol_version,
                            stack_version,
                            Some(conn),
                            listen_ports.clone(),
                            cert_factory.clone())?;
                        let future = Box::pin(async move {
                            let stream = tunnel_conn.connect_datagram().await?;
                            Ok((tunnel_conn, stream))
                        });
                        futures.push(future);
                    }
                    if futures.len() > 0 {
                        select_successful(futures).await
                    } else {
                        Err(p2p_err!(P2pErrorCode::ConnectFailed, "No available endpoint"))
                    }
                });
                futures.push(future)
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
        let result = runtime::timeout(self.conn_timeout, future).await.map_err(into_p2p_err!(P2pErrorCode::Timeout))?;
        if let ReverseResult::Datagram(tunnel_conn, stream) = result {
            self.tunnel_conn = Some(tunnel_conn);
            Ok(stream)
        } else {
            Err(p2p_err!(P2pErrorCode::ConnectFailed, "No available endpoint"))
        }
    }

    pub async fn connect_reverse_stream(&mut self, vport: u16, session_id: IncreaseId) -> P2pResult<TunnelStream> {
        if self.tunnel_conn.is_some() {
            let datagram = self.tunnel_conn.as_ref().unwrap().connect_reverse_stream(vport, session_id).await.map_err(into_p2p_err!(P2pErrorCode::ConnectFailed))?;
            return Ok(datagram);
        }

        let mut futures: Vec<Pin<Box<dyn Future<Output=P2pResult<(TunnelConnection, TunnelStream)>> + Send>>> = Vec::new();
        for ep in self.remote_eps.iter() {
            let sequence = self.sequence;
            let local_identity = self.local_identity.clone();
            let remote_id = self.remote_id.clone();
            let conn_timeout = self.conn_timeout;
            let protocol_version = self.protocol_version;
            let stack_version = self.stack_version;
            let listen_ports = self.listen_ports.clone();
            let cert_factory = self.cert_factory.clone();
            let net_manager = self.net_manager.clone();
            let future = Box::pin(async move {
                let conns = net_manager.get_network(ep.protocol())?.create_stream_connect(&local_identity, ep, &remote_id).await?;
                let mut futures: Vec<Pin<Box<dyn Future<Output=P2pResult<(TunnelConnection, TunnelStream)>> + Send>>> = Vec::new();
                for conn in conns {
                    let tunnel_conn = TunnelConnection::new(
                        sequence,
                        local_identity.clone(),
                        remote_id.clone(),
                        ep.clone(),
                        conn_timeout,
                        protocol_version,
                        stack_version,
                        Some(conn),
                        listen_ports.clone(),
                        cert_factory.clone())?;
                    let future = Box::pin(async move {
                        let stream = tunnel_conn.connect_reverse_stream(vport, session_id).await?;
                        Ok((tunnel_conn, stream))
                    });
                    futures.push(future);
                }
                if futures.len() > 0 {
                    select_successful(futures).await
                } else {
                    Err(p2p_err!(P2pErrorCode::ConnectFailed, "No available endpoint"))
                }
            });
            futures.push(future);
        }

        if futures.len() > 0 {
            match select_successful(futures).await {
                Ok((conn, stream)) => {
                    self.tunnel_conn = Some(conn);
                    return Ok(stream);
                }
                Err(e) => {
                    log::error!("connect stream error: {:?}", e);
                }
            }
        }
        Err(p2p_err!(P2pErrorCode::ConnectFailed, "No available endpoint"))
    }

    pub async fn connect_reverse_datagram(&mut self) -> P2pResult<TunnelDatagramRecv> {
        if self.tunnel_conn.is_some() {
            let datagram = self.tunnel_conn.as_ref().unwrap().connect_reverse_datagram().await.map_err(into_p2p_err!(P2pErrorCode::ConnectFailed))?;
            return Ok(datagram);
        }

        let mut futures: Vec<Pin<Box<dyn Future<Output=P2pResult<(TunnelConnection, TunnelDatagramRecv)>> + Send>>> = Vec::new();
        for ep in self.remote_eps.iter() {
            let sequence = self.sequence;
            let local_identity = self.local_identity.clone();
            let remote_id = self.remote_id.clone();
            let conn_timeout = self.conn_timeout;
            let protocol_version = self.protocol_version;
            let stack_version = self.stack_version;
            let listen_ports = self.listen_ports.clone();
            let cert_factory = self.cert_factory.clone();
            let net_manager = self.net_manager.clone();
            let future = Box::pin(async move {
                let conns = net_manager.get_network(ep.protocol())?.create_stream_connect(&local_identity, ep, &remote_id).await?;
                let mut futures: Vec<Pin<Box<dyn Future<Output=P2pResult<(TunnelConnection, TunnelDatagramRecv)>> + Send>>> = Vec::new();
                for conn in conns {
                    let tunnel_conn = TunnelConnection::new(
                        sequence,
                        local_identity.clone(),
                        remote_id.clone(),
                        ep.clone(),
                        conn_timeout,
                        protocol_version,
                        stack_version,
                        Some(conn),
                        listen_ports.clone(),
                        cert_factory.clone())?;
                    let future = Box::pin(async move {
                        let stream = tunnel_conn.connect_reverse_datagram().await?;
                        Ok((tunnel_conn, stream))
                    });
                    futures.push(future);
                }
                if futures.len() > 0 {
                    select_successful(futures).await
                } else {
                    Err(p2p_err!(P2pErrorCode::ConnectFailed, "No available endpoint"))
                }
            });
            futures.push(future);
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
        Err(p2p_err!(P2pErrorCode::ConnectFailed, "No available endpoint"))
    }

    pub async fn open_stream(&self, vport: u16, session_id: IncreaseId) -> P2pResult<TunnelStream> {
        if self.tunnel_conn.is_none() {
            return Err(p2p_err!(P2pErrorCode::TunnelNotConnected, "Tunnel not connected"));
        }

        let stream = self.tunnel_conn.as_ref().unwrap().open_stream(vport, session_id).await?;
        log::info!("Open stream tunnel {:?} session_id {:?} vport {} remote_id {} remote_ep {} local_id {} local_ep {}",
            stream.sequence(),
            stream.session_id(),
            vport,
            stream.remote_identity_id().to_string(),
            stream.remote_endpoint().to_string(),
            stream.local_identity_id().to_string(),
            stream.local_endpoint().to_string());

        Ok(stream)
    }

    pub async fn open_datagram(&self) -> P2pResult<TunnelDatagramSend> {
        if self.tunnel_conn.is_none() {
            return Err(p2p_err!(P2pErrorCode::TunnelNotConnected, "Tunnel not connected"));
        }

        let datagram = self.tunnel_conn.as_ref().unwrap().open_datagram().await?;

        log::info!("Open stream tunnel {:?} remote_id {} remote_ep {} local_id {} local_ep {}",
            datagram.sequence(),
            datagram.remote_identity_id().to_string(),
            datagram.remote_endpoint().to_string(),
            datagram.local_identity_id().to_string(),
            datagram.local_endpoint().to_string());

        Ok(datagram)
    }

    pub async fn shutdown(&self) -> P2pResult<()> {
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
