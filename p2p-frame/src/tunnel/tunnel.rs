use std::future::Future;
use std::net::{SocketAddr};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use bucky_raw_codec::{RawConvertTo, RawDecode, RawEncode};
use notify_future::NotifyFuture;
use crate::endpoint::{Endpoint, EndpointArea};
use crate::error::{p2p_err, P2pErrorCode, P2pResult, into_p2p_err};
use crate::p2p_connection::{ConnectDirection, P2pConnection, P2pConnectionInfo};
use crate::p2p_identity::{P2pId, P2pIdentityRef, P2pIdentityCertFactoryRef};
use crate::pn::{PnClient, PnClientRef};
use crate::protocol::v0::SnCallType;
use crate::runtime;
use crate::sn::client::SNClientServiceRef;
use crate::sockets::NetManagerRef;
use crate::tunnel::{select_successful, TunnelConnection, TunnelDatagramRecv, TunnelDatagramSend, TunnelSession, TunnelListenPortsRef, TunnelStatRef, TunnelStream};
use crate::tunnel::proxy_connection::{ProxyConnectionRead, ProxyConnectionWrite};
use crate::types::{SessionId, TunnelId};

pub enum ReverseResult {
    Stream(TunnelConnection, TunnelStream),
    Datagram(TunnelConnection, TunnelDatagramSend),
}

pub trait ReverseFutureCache: 'static + Send + Sync {
    fn add_reverse_future(&self, sequence: TunnelId, future: NotifyFuture<ReverseResult>);
    fn remove_reverse_future(&self, sequence: TunnelId);
}

#[derive(RawEncode, RawDecode)]
pub struct StreamSnCall {
    pub vport: u16,
    pub session_id: SessionId,
}

#[derive(RawEncode, RawDecode)]
pub struct DatagramSnCall {
    pub vport: u16,
    pub session_id: SessionId,
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
    tunnel_id: TunnelId,
    tunnel_conn: Option<TunnelConnection>,
    state: Mutex<TunnelState>,
    protocol_version: u8,
    stack_version: u32,
    local_identity: P2pIdentityRef,
    remote_id: P2pId,
    remote_eps: Vec<Endpoint>,
    conn_timeout: Duration,
    idle_timeout: Duration,
    cert_factory: P2pIdentityCertFactoryRef,
    pn_client: Option<PnClientRef>,
}

impl Tunnel {
    pub fn new(
        net_manager: NetManagerRef,
        sn_service: SNClientServiceRef,
        sequence: TunnelId,
        protocol_version: u8,
        stack_version: u32,
        remote_id: P2pId,
        remote_eps: Vec<Endpoint>,
        local_identity: P2pIdentityRef,
        conn_timeout: Duration,
        idle_timeout: Duration,
        cert_factory: P2pIdentityCertFactoryRef,
        pn_client: Option<PnClientRef>) -> Self {
        Self {
            net_manager,
            sn_service,
            tunnel_id: sequence,
            tunnel_conn: None,
            state: Mutex::new(TunnelState { status: TunnelStatus::Connecting }),
            protocol_version,
            stack_version,
            local_identity,
            remote_id,
            remote_eps,
            conn_timeout,
            idle_timeout,
            cert_factory,
            pn_client,
        }
    }

    pub fn set_tunnel_conn(&mut self, tunnel_conn: TunnelConnection) {
        self.tunnel_conn = Some(tunnel_conn);
    }

    pub async fn accept_instance(&self) -> P2pResult<TunnelSession> {
        self.tunnel_conn.as_ref().unwrap().accept_next_session().await
    }

    pub fn get_tunnel_id(&self) -> TunnelId {
        self.tunnel_id
    }

    pub fn tunnel_stat(&self) -> TunnelStatRef {
        self.tunnel_conn.as_ref().unwrap().tunnel_stat()
    }

    pub fn is_idle(&self) -> bool {
        self.tunnel_conn.as_ref().unwrap().is_idle()
    }

    pub fn is_error(&self) -> bool {
        self.tunnel_conn.as_ref().unwrap().is_error()
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

    fn create_stream_connection(&self, ep: &Endpoint, vport: u16, session_id: SessionId) -> Pin<Box<dyn Future<Output=P2pResult<(TunnelConnection, TunnelStream)>> + Send>> {
        let sequence = self.tunnel_id;
        let local_identity = self.local_identity.clone();
        let remote_id = self.remote_id.clone();
        let conn_timeout = self.conn_timeout;
        let protocol_version = self.protocol_version;
        let stack_version = self.stack_version;
        let cert_factory = self.cert_factory.clone();
        let net_manager = self.net_manager.clone();
        let ep = ep.clone();
        let future = Box::pin(async move {
            let conns = net_manager.get_network(ep.protocol())?.create_stream_connect(&local_identity, &ep, &remote_id).await?;
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
                    conn,
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
        future
    }

    async fn create_proxy_connection(&self) -> P2pResult<Option<TunnelConnection>> {
        if self.pn_client.is_none() {
            return Ok(None);
        }

        let pn_client = self.pn_client.as_ref().unwrap();

        let (read, write) = runtime::timeout(self.conn_timeout,
                                             pn_client.connect(self.tunnel_id, self.remote_id.clone())).await.map_err(into_p2p_err!(P2pErrorCode::Timeout))??;
        let read = Box::new(ProxyConnectionRead::new(read, self.local_identity.get_id()));
        let write = Box::new(ProxyConnectionWrite::new(write, self.local_identity.get_id()));
        let proxy_conn = P2pConnection::new(read, write);
        let conn = TunnelConnection::new(
            self.tunnel_id,
            self.local_identity.clone(),
            self.remote_id.clone(),
            Endpoint::default(),
            self.conn_timeout,
            self.protocol_version,
            self.stack_version,
            proxy_conn,
            self.cert_factory.clone())?;
        Ok(Some(conn))
    }

    pub async fn connect_stream(&mut self, vport: u16, session_id: SessionId, future_cache: Arc<dyn ReverseFutureCache>) -> P2pResult<TunnelStream> {
        if self.tunnel_conn.is_some() {
            let stream = self.tunnel_conn.as_ref().unwrap().connect_stream(vport, session_id).await.map_err(into_p2p_err!(P2pErrorCode::ConnectFailed))?;
            return Ok(stream);
        }

        let mut has_reversed = false;
        if let Some(latest_connection_info) = self.net_manager.get_connection_info_cache().get(&self.remote_id).await {
            if latest_connection_info.direct == ConnectDirection::Direct {
                for ep in self.remote_eps.iter() {
                    if ep == &latest_connection_info.remote_ep {
                        let mut futures: Vec<Pin<Box<dyn Future<Output=P2pResult<(TunnelConnection, TunnelStream)>> + Send>>> = Vec::new();
                        let future = self.create_stream_connection(ep, vport, session_id);
                        futures.push(future);
                        if futures.len() > 0 {
                            match select_successful(futures).await {
                                Ok((conn, stream)) => {
                                    self.tunnel_conn = Some(conn);
                                    return Ok(stream);
                                }
                                Err(e) => {
                                }
                            }
                        }
                    }
                }
            } else if latest_connection_info.direct == ConnectDirection::Reverse {
                let future = NotifyFuture::new();
                future_cache.add_reverse_future(self.tunnel_id, future.clone());
                let call_data = StreamSnCall {
                    vport,
                    session_id,
                };
                self.sn_service.call(self.get_tunnel_id(),
                                     None,
                                     &self.remote_id,
                                     SnCallType::Stream,
                                     call_data.to_vec().map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?).await?;
                let result = runtime::timeout(self.conn_timeout, future).await.map_err(into_p2p_err!(P2pErrorCode::Timeout))?;
                if let ReverseResult::Stream(tunnel_conn, stream) = result {
                    self.tunnel_conn = Some(tunnel_conn);
                    self.net_manager.get_connection_info_cache().add(self.remote_id.clone(), P2pConnectionInfo {
                        direct: ConnectDirection::Reverse,
                        local_ep: stream.local_endpoint(),
                        remote_ep: stream.remote_endpoint(),
                    }).await;
                    return Ok(stream)
                }
                has_reversed = true;
            } else if latest_connection_info.direct == ConnectDirection::Proxy {
                let proxy_conn = self.create_proxy_connection().await?;
                if proxy_conn.is_some() {
                    let conn = proxy_conn.unwrap();
                    let stream = conn.connect_stream(vport, session_id).await?;
                    self.tunnel_conn = Some(conn);
                    self.net_manager.get_connection_info_cache().add(self.remote_id.clone(), P2pConnectionInfo {
                        direct: ConnectDirection::Proxy,
                        local_ep: stream.local_endpoint(),
                        remote_ep: stream.remote_endpoint(),
                    }).await;
                    return Ok(stream);
                }
            }
        }

        let mut futures: Vec<Pin<Box<dyn Future<Output=P2pResult<(TunnelConnection, TunnelStream)>> + Send>>> = Vec::new();
        let ep_list = &self.remote_eps;
        let is_lan = self.sn_service.is_same_lan(ep_list);
        for ep in ep_list.iter() {
            if is_lan || ep.is_static_wan() {
                let future = self.create_stream_connection(ep, vport, session_id);
                futures.push(future)
            }
        }

        if futures.len() > 0 {
            match select_successful(futures).await {
                Ok((conn, stream)) => {
                    self.tunnel_conn = Some(conn);
                    self.net_manager.get_connection_info_cache().add(self.remote_id.clone(), P2pConnectionInfo {
                        direct: ConnectDirection::Direct,
                        local_ep: stream.local_endpoint(),
                        remote_ep: stream.remote_endpoint(),
                    }).await;
                    return Ok(stream);
                }
                Err(e) => {
                    log::error!("connect stream error: {:?} msg: {}", e.code(), e.msg());
                }
            }
        }

        if !has_reversed {
            let future = NotifyFuture::new();
            future_cache.add_reverse_future(self.tunnel_id, future.clone());
            let call_data = StreamSnCall {
                vport,
                session_id,
            };
            self.sn_service.call(self.get_tunnel_id(),
                                 None,
                                 &self.remote_id,
                                 SnCallType::Stream,
                                 call_data.to_vec().map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?).await?;
            let result = runtime::timeout(self.conn_timeout, future).await.map_err(into_p2p_err!(P2pErrorCode::Timeout))?;
            if let ReverseResult::Stream(tunnel_conn, stream) = result {
                self.tunnel_conn = Some(tunnel_conn);
                self.net_manager.get_connection_info_cache().add(self.remote_id.clone(), P2pConnectionInfo {
                    direct: ConnectDirection::Reverse,
                    local_ep: stream.local_endpoint(),
                    remote_ep: stream.remote_endpoint(),
                }).await;
                return Ok(stream);
            }
        }

        let proxy_conn = self.create_proxy_connection().await?;
        if proxy_conn.is_some() {
            let conn = proxy_conn.unwrap();
            let stream = conn.connect_stream(vport, session_id).await?;
            self.tunnel_conn = Some(conn);
            self.net_manager.get_connection_info_cache().add(self.remote_id.clone(), P2pConnectionInfo {
                direct: ConnectDirection::Proxy,
                local_ep: stream.local_endpoint(),
                remote_ep: stream.remote_endpoint(),
            }).await;
            return Ok(stream);
        }
        Err(p2p_err!(P2pErrorCode::ConnectFailed, "No available endpoint"))
    }

    fn create_datagram_connection(&self, ep: &Endpoint, vport: u16, session_id: SessionId) -> Pin<Box<dyn Future<Output=P2pResult<(TunnelConnection, TunnelDatagramSend)>> + Send>> {
        let sequence = self.tunnel_id;
        let local_identity = self.local_identity.clone();
        let remote_id = self.remote_id.clone();
        let conn_timeout = self.conn_timeout;
        let protocol_version = self.protocol_version;
        let stack_version = self.stack_version;
        let cert_factory = self.cert_factory.clone();
        let net_manager = self.net_manager.clone();
        let ep = ep.clone();
        let future = Box::pin(async move {
            let conns = net_manager.get_network(ep.protocol())?.create_stream_connect(&local_identity, &ep, &remote_id).await?;
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
                    conn,
                    cert_factory.clone())?;
                let future = Box::pin(async move {
                    let stream = tunnel_conn.connect_datagram(vport, session_id).await?;
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
        future
    }

    pub async fn connect_datagram(&mut self, vport: u16, session_id: SessionId, future_cache: Arc<dyn ReverseFutureCache>) -> P2pResult<TunnelDatagramSend> {
        if self.tunnel_conn.is_some() {
            let datagram = self.tunnel_conn.as_ref().unwrap().connect_datagram(vport, session_id).await.map_err(into_p2p_err!(P2pErrorCode::ConnectFailed))?;
            return Ok(datagram);
        }

        let mut is_reversed = false;
        if let Some(connection_info) = self.net_manager.get_connection_info_cache().get(&self.remote_id).await {
            if connection_info.direct == ConnectDirection::Direct {
                for ep in self.remote_eps.iter() {
                    if ep == &connection_info.remote_ep {
                        let mut futures: Vec<Pin<Box<dyn Future<Output=P2pResult<(TunnelConnection, TunnelDatagramSend)>> + Send>>> = Vec::new();
                        let future = self.create_datagram_connection(ep, vport, session_id);
                        futures.push(future);
                        if futures.len() > 0 {
                            match select_successful(futures).await {
                                Ok((conn, datagram_send)) => {
                                    self.tunnel_conn = Some(conn);
                                    return Ok(datagram_send);
                                }
                                Err(e) => {
                                }
                            }
                        }
                    }
                }
            } else if connection_info.direct == ConnectDirection::Reverse {
                let future = NotifyFuture::new();
                future_cache.add_reverse_future(self.tunnel_id, future.clone());
                self.sn_service.call(self.get_tunnel_id(),
                                     None,
                                     &self.remote_id,
                                     SnCallType::Datagram,
                                     Vec::new()).await?;
                let result = runtime::timeout(self.conn_timeout, future).await.map_err(into_p2p_err!(P2pErrorCode::Timeout))?;
                if let ReverseResult::Datagram(tunnel_conn, datagram_send) = result {
                    self.tunnel_conn = Some(tunnel_conn);
                    self.net_manager.get_connection_info_cache().add(self.remote_id.clone(), P2pConnectionInfo {
                        direct: ConnectDirection::Reverse,
                        local_ep: datagram_send.local_endpoint(),
                        remote_ep: datagram_send.remote_endpoint(),
                    }).await;
                    return Ok(datagram_send);
                }
                is_reversed = true;
            } else if connection_info.direct == ConnectDirection::Proxy {
                let proxy_conn = self.create_proxy_connection().await?;
                if proxy_conn.is_some() {
                    let conn = proxy_conn.unwrap();
                    let send = conn.connect_datagram(vport, session_id).await?;
                    self.tunnel_conn = Some(conn);
                    self.net_manager.get_connection_info_cache().add(self.remote_id.clone(), P2pConnectionInfo {
                        direct: ConnectDirection::Proxy,
                        local_ep: send.local_endpoint(),
                        remote_ep: send.remote_endpoint(),
                    }).await;
                    return Ok(send);
                }
            }
        }

        let mut futures: Vec<Pin<Box<dyn Future<Output=P2pResult<(TunnelConnection, TunnelDatagramSend)>> + Send>>> = Vec::new();
        let ep_list = &self.remote_eps;
        let is_lan = self.sn_service.is_same_lan(ep_list);
        for ep in ep_list.iter() {
            if is_lan || ep.is_static_wan() {
                let future = self.create_datagram_connection(ep, vport, session_id);
                futures.push(future)
            }
        }

        if futures.len() > 0 {
            match select_successful(futures).await {
                Ok((conn, datagram_send)) => {
                    self.tunnel_conn = Some(conn);
                    self.net_manager.get_connection_info_cache().add(self.remote_id.clone(), P2pConnectionInfo {
                        direct: ConnectDirection::Direct,
                        local_ep: datagram_send.local_endpoint(),
                        remote_ep: datagram_send.remote_endpoint(),
                    }).await;
                    return Ok(datagram_send);
                }
                Err(e) => {
                    log::error!("connect stream error: {:?} msg: {}", e.code(), e.msg());
                }
            }
        }

        if !is_reversed {
            let future = NotifyFuture::new();
            future_cache.add_reverse_future(self.tunnel_id, future.clone());
            self.sn_service.call(self.get_tunnel_id(),
                                 None,
                                 &self.remote_id,
                                 SnCallType::Datagram,
                                 Vec::new()).await?;
            let result = runtime::timeout(self.conn_timeout, future).await.map_err(into_p2p_err!(P2pErrorCode::Timeout))?;
            if let ReverseResult::Datagram(tunnel_conn, datagram_send) = result {
                self.tunnel_conn = Some(tunnel_conn);
                self.net_manager.get_connection_info_cache().add(self.remote_id.clone(), P2pConnectionInfo {
                    direct: ConnectDirection::Reverse,
                    local_ep: datagram_send.local_endpoint(),
                    remote_ep: datagram_send.remote_endpoint(),
                }).await;
                return Ok(datagram_send);
            }
        }

        let proxy_conn = self.create_proxy_connection().await?;
        if proxy_conn.is_some() {
            let conn = proxy_conn.unwrap();
            let send = conn.connect_datagram(vport, session_id).await?;
            self.tunnel_conn = Some(conn);
            self.net_manager.get_connection_info_cache().add(self.remote_id.clone(), P2pConnectionInfo {
                direct: ConnectDirection::Proxy,
                local_ep: send.local_endpoint(),
                remote_ep: send.remote_endpoint(),
            }).await;
            return Ok(send);
        }

        Err(p2p_err!(P2pErrorCode::ConnectFailed, "No available endpoint"))
    }

    pub async fn connect_reverse_stream(&mut self, vport: u16, session_id: SessionId) -> P2pResult<TunnelStream> {
        if self.tunnel_conn.is_some() {
            let datagram = self.tunnel_conn.as_ref().unwrap().connect_reverse_stream(vport, session_id).await.map_err(into_p2p_err!(P2pErrorCode::ConnectFailed))?;
            return Ok(datagram);
        }

        let mut futures: Vec<Pin<Box<dyn Future<Output=P2pResult<(TunnelConnection, TunnelStream)>> + Send>>> = Vec::new();
        for ep in self.remote_eps.iter() {
            let sequence = self.tunnel_id;
            let local_identity = self.local_identity.clone();
            let remote_id = self.remote_id.clone();
            let conn_timeout = self.conn_timeout;
            let protocol_version = self.protocol_version;
            let stack_version = self.stack_version;
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
                        conn,
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
                    self.net_manager.get_connection_info_cache().add(self.remote_id.clone(), P2pConnectionInfo {
                        direct: ConnectDirection::Direct,
                        local_ep: stream.local_endpoint(),
                        remote_ep: stream.remote_endpoint(),
                    }).await;
                    return Ok(stream);
                }
                Err(e) => {
                    log::error!("connect stream error: {:?}", e);
                }
            }
        }
        Err(p2p_err!(P2pErrorCode::ConnectFailed, "No available endpoint"))
    }

    pub async fn connect_reverse_datagram(&mut self, vport: u16, session_id: SessionId) -> P2pResult<TunnelDatagramRecv> {
        if self.tunnel_conn.is_some() {
            let datagram = self.tunnel_conn.as_ref().unwrap().connect_reverse_datagram(vport, session_id).await.map_err(into_p2p_err!(P2pErrorCode::ConnectFailed))?;
            return Ok(datagram);
        }

        let mut futures: Vec<Pin<Box<dyn Future<Output=P2pResult<(TunnelConnection, TunnelDatagramRecv)>> + Send>>> = Vec::new();
        for ep in self.remote_eps.iter() {
            let sequence = self.tunnel_id;
            let local_identity = self.local_identity.clone();
            let remote_id = self.remote_id.clone();
            let conn_timeout = self.conn_timeout;
            let protocol_version = self.protocol_version;
            let stack_version = self.stack_version;
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
                        conn,
                        cert_factory.clone())?;
                    let future = Box::pin(async move {
                        let stream = tunnel_conn.connect_reverse_datagram(vport, session_id).await?;
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
                    self.net_manager.get_connection_info_cache().add(self.remote_id.clone(), P2pConnectionInfo {
                        direct: ConnectDirection::Direct,
                        local_ep: datagram_send.local_endpoint(),
                        remote_ep: datagram_send.remote_endpoint(),
                    }).await;
                    return Ok(datagram_send);
                }
                Err(e) => {
                    log::error!("connect stream error: {:?} msg: {}", e.code(), e.msg());
                }
            }
        }
        Err(p2p_err!(P2pErrorCode::ConnectFailed, "No available endpoint"))
    }

    pub async fn open_stream(&self, vport: u16, session_id: SessionId) -> P2pResult<TunnelStream> {
        if self.tunnel_conn.is_none() {
            return Err(p2p_err!(P2pErrorCode::TunnelNotConnected, "Tunnel not connected"));
        }

        log::info!("Opening stream tunnel session_id {:?} vport {} remote_id {} local_id {}",
            session_id,
            vport,
            self.remote_id.to_string(),
            self.local_identity.get_id().to_string());
        let stream = self.tunnel_conn.as_ref().unwrap().open_stream(vport, session_id).await?;
        log::info!("Open stream tunnel {:?} session_id {:?} vport {} remote_id {} remote_ep {} local_id {} local_ep {}",
            stream.tunnel_id(),
            stream.session_id(),
            vport,
            stream.remote_identity_id().to_string(),
            stream.remote_endpoint().to_string(),
            stream.local_identity_id().to_string(),
            stream.local_endpoint().to_string());

        Ok(stream)
    }

    pub async fn open_datagram(&self, vport: u16, session_id: SessionId) -> P2pResult<TunnelDatagramSend> {
        if self.tunnel_conn.is_none() {
            return Err(p2p_err!(P2pErrorCode::TunnelNotConnected, "Tunnel not connected"));
        }

        let datagram = self.tunnel_conn.as_ref().unwrap().open_datagram(vport, session_id).await?;

        log::info!("Open stream tunnel {:?} remote_id {} remote_ep {} local_id {} local_ep {}",
            datagram.tunnel_id(),
            datagram.remote_identity_id().to_string(),
            datagram.remote_endpoint().to_string(),
            datagram.local_identity_id().to_string(),
            datagram.local_endpoint().to_string());

        Ok(datagram)
    }

    pub async fn shutdown(&self) -> P2pResult<()> {
        log::info!("shutdown tunnel {:?}", self.tunnel_id);
        if self.tunnel_conn.is_some() {
            self.tunnel_conn.as_ref().unwrap().shutdown().await
        } else {
            Ok(())
        }
    }
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        log::info!("drop tunnel {:?} local {}", self.tunnel_id, self.local_identity.get_id());
    }
}
