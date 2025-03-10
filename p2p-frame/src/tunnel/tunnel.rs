use std::future::Future;
use std::net::{SocketAddr};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use bucky_raw_codec::{RawConvertTo, RawDecode, RawEncode};
use notify_future::{Notify};
use num_traits::FromPrimitive;
use crate::endpoint::{Endpoint, EndpointArea};
use crate::error::{p2p_err, P2pErrorCode, P2pResult, into_p2p_err};
use crate::p2p_connection::{ConnectDirection, P2pConnection, P2pConnectionInfo, P2pConnectionInfoCacheRef};
use crate::p2p_identity::{P2pId, P2pIdentityRef, P2pIdentityCertFactoryRef};
use crate::pn::{PnClient, PnClientRef};
use crate::protocol::v0::{AckReverseSession, AckSession, TunnelType};
use crate::runtime;
use crate::sn::client::SNClientServiceRef;
use crate::sockets::NetManagerRef;
use crate::tunnel::{select_successful, TunnelConnection, TunnelSession, TunnelListenPortsRef, TunnelStatRef, TunnelStat, TunnelConnectionRef, TunnelConnectionRead, TunnelConnectionWrite, TunnelState};
use crate::tunnel::proxy_connection::{ProxyConnectionRead, ProxyConnectionWrite};
use crate::types::{SessionId, TunnelId};

pub enum ReverseResult {
    Session(u8, TunnelConnectionRef, TunnelConnectionRead, TunnelConnectionWrite),
}

#[derive(RawEncode, RawDecode)]
pub struct SessionSnCall {
    pub vport: u16,
    pub session_id: SessionId,
}

pub trait ReverseWaiterCache: 'static + Send + Sync {
    fn add_reverse_waiter(&self, sequence: TunnelId, notify: Notify<ReverseResult>);
    fn remove_reverse_waiter(&self, sequence: TunnelId);
}

#[async_trait::async_trait]
pub trait P2pConnectionFactory: Send + Sync + 'static {
    fn tunnel_type(&self) -> TunnelType;
    async fn create_connect(&self, local_identity: &P2pIdentityRef, remote: &Endpoint, remote_id: &P2pId, remote_name: Option<String>) -> P2pResult<Vec<P2pConnection>>;
    async fn create_connect_with_local_ep(&self, local_identity: &P2pIdentityRef, local_ep: &Endpoint, remote: &Endpoint, remote_id: &P2pId, remote_name: Option<String>) -> P2pResult<P2pConnection>;
}

pub struct Tunnel<F: P2pConnectionFactory> {
    sn_service: SNClientServiceRef,
    tunnel_id: TunnelId,
    tunnel_conn: Option<TunnelConnectionRef>,
    protocol_version: u8,
    local_identity: P2pIdentityRef,
    remote_id: P2pId,
    remote_eps: Vec<Endpoint>,
    remote_name: Option<String>,
    conn_timeout: Duration,
    idle_timeout: Duration,
    cert_factory: P2pIdentityCertFactoryRef,
    pn_client: Option<PnClientRef>,
    tunnel_stat: TunnelStatRef,
    p2p_factory: Arc<F>,
    conn_info_cache: P2pConnectionInfoCacheRef,
}

impl<F: P2pConnectionFactory> Tunnel<F> {
    pub fn new(
        sn_service: SNClientServiceRef,
        sequence: TunnelId,
        protocol_version: u8,
        remote_id: P2pId,
        remote_eps: Vec<Endpoint>,
        remote_name: Option<String>,
        local_identity: P2pIdentityRef,
        conn_timeout: Duration,
        idle_timeout: Duration,
        cert_factory: P2pIdentityCertFactoryRef,
        pn_client: Option<PnClientRef>,
        p2p_factory: Arc<F>,
        conn_info_cache: P2pConnectionInfoCacheRef,
    ) -> Self {
        Self {
            sn_service,
            tunnel_id: sequence,
            tunnel_conn: None,
            protocol_version,
            local_identity,
            remote_id,
            remote_eps,
            remote_name,
            conn_timeout,
            idle_timeout,
            cert_factory,
            pn_client,
            tunnel_stat: TunnelStat::new(),
            p2p_factory,
            conn_info_cache,
        }
    }

    pub fn set_tunnel_conn(&mut self, tunnel_conn: TunnelConnectionRef) {
        self.tunnel_conn = Some(tunnel_conn);
    }

    pub async fn accept_session(&self) -> P2pResult<TunnelSession> {
        self.tunnel_conn.as_ref().unwrap().accept_session().await
    }

    pub fn get_tunnel_id(&self) -> TunnelId {
        self.tunnel_id
    }

    pub fn tunnel_stat(&self) -> TunnelStatRef {
        self.tunnel_conn.as_ref().unwrap().tunnel_stat()
    }

    pub(crate) fn set_tunnel_state(&self, new_state: TunnelState) {
        if (self.tunnel_conn.is_some()) {
            self.tunnel_conn.as_ref().unwrap().set_tunnel_state(new_state);
        }
    }

    pub fn is_idle(&self) -> bool {
        self.tunnel_conn.as_ref().unwrap().is_idle()
    }

    pub fn is_work(&self) -> bool {
        self.tunnel_conn.as_ref().unwrap().is_work()
    }

    pub fn is_error(&self) -> bool {
        self.tunnel_conn.as_ref().unwrap().is_error()
    }

    // fn get_reverse_ep_list(&self) -> Vec<Endpoint> {
    //     let mut wan_udp_eps = Vec::new();
    //     self.sn_service.get_active_sn_list().iter().map(|v| v.wan_ep_list.iter()).flatten().for_each(|ep| {
    //         let mut ep = ep.clone();
    //         ep.set_area(EndpointArea::Wan);
    //         wan_udp_eps.push(ep);
    //     });
    //     let mut reverse_eps = Vec::new();
    //     for (ep, port) in self.net_manager.port_mapping() {
    //         for udp_ep in wan_udp_eps.iter() {
    //             let mut ep = Endpoint::from((ep.protocol(), SocketAddr::new(udp_ep.addr().ip(), port)));
    //             ep.set_area(EndpointArea::Wan);
    //             reverse_eps.push(ep);
    //         }
    //     }
    //     reverse_eps.extend_from_slice(wan_udp_eps.as_slice());
    //     reverse_eps
    // }

    fn create_connection(&self, ep: &Endpoint, vport: u16, session_id: SessionId) -> Pin<Box<dyn Future<Output=P2pResult<(AckSession, TunnelConnectionRef, TunnelConnectionRead, TunnelConnectionWrite)>> + Send>> {
        let sequence = self.tunnel_id;
        let local_identity = self.local_identity.clone();
        let remote_id = self.remote_id.clone();
        let conn_timeout = self.conn_timeout;
        let protocol_version = self.protocol_version;
        let cert_factory = self.cert_factory.clone();
        let p2p_factory = self.p2p_factory.clone();
        let ep = ep.clone();
        let remote_name = self.remote_name.clone();
        let future = Box::pin(async move {
            let conns = p2p_factory.create_connect(&local_identity, &ep, &remote_id, remote_name).await?;
            let mut futures: Vec<Pin<Box<dyn Future<Output=P2pResult<(AckSession, TunnelConnectionRef, TunnelConnectionRead, TunnelConnectionWrite)>> + Send>>> = Vec::new();
            for conn in conns {
                let tunnel_conn = TunnelConnection::new(
                    sequence,
                    local_identity.clone(),
                    remote_id.clone(),
                    ep.clone(),
                    conn_timeout,
                    protocol_version,
                    conn,
                    cert_factory.clone());
                let p2p_factory = p2p_factory.clone();
                let future = Box::pin(async move {
                    let (ack, read, write) = tunnel_conn.open_session(p2p_factory.tunnel_type(), vport, session_id).await?;
                    Ok((ack, tunnel_conn, read, write))
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

    async fn create_proxy_connection(&self) -> P2pResult<Option<TunnelConnectionRef>> {
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
            proxy_conn,
            self.cert_factory.clone());
        Ok(Some(conn))
    }

    pub async fn connect_session(&mut self, vport: u16, session_id: SessionId, future_cache: Arc<dyn ReverseWaiterCache>) -> P2pResult<(TunnelConnectionRead, TunnelConnectionWrite)> {
        if self.tunnel_conn.is_some() {
            let (ack, read, write) = self.tunnel_conn.as_ref().unwrap().open_session(self.p2p_factory.tunnel_type(), vport, session_id).await.map_err(into_p2p_err!(P2pErrorCode::ConnectFailed))?;
            return if ack.result == 0 {
                Ok((read, write))
            } else {
                Err(p2p_err!(P2pErrorCode::from_u8(ack.result).unwrap_or(P2pErrorCode::ConnectFailed), "ack err session {} port {}", session_id, vport))
            };
        }

        let mut has_reversed = false;
        if let Some(latest_connection_info) = self.conn_info_cache.get(&self.remote_id).await {
            if latest_connection_info.direct == ConnectDirection::Direct {
                for ep in self.remote_eps.iter() {
                    if ep == &latest_connection_info.remote_ep {
                        let mut futures: Vec<Pin<Box<dyn Future<Output=P2pResult<(AckSession, TunnelConnectionRef, TunnelConnectionRead, TunnelConnectionWrite)>> + Send>>> = Vec::new();
                        let future = self.create_connection(ep, vport, session_id);
                        futures.push(future);
                        if futures.len() > 0 {
                            match select_successful(futures).await {
                                Ok((ack, conn, read, write)) => {
                                    self.tunnel_conn = Some(conn);
                                    if ack.result == 0 {
                                        return Ok((read, write));
                                    } else {
                                        return Err(p2p_err!(P2pErrorCode::from_u8(ack.result).unwrap_or(P2pErrorCode::ConnectFailed), "ack err session {} port {}", session_id, vport));
                                    }
                                }
                                Err(e) => {
                                }
                            }
                        }
                    }
                }
            } else if latest_connection_info.direct == ConnectDirection::Reverse {
                let (notify, waiter) = Notify::new();
                future_cache.add_reverse_waiter(self.tunnel_id, notify);
                let call_data = SessionSnCall {
                    vport,
                    session_id,
                };
                self.sn_service.call(self.get_tunnel_id(),
                                     None,
                                     &self.remote_id,
                                     self.p2p_factory.tunnel_type(),
                                     call_data.to_vec().map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?).await?;
                match runtime::timeout(self.conn_timeout, waiter).await.map_err(into_p2p_err!(P2pErrorCode::Timeout)) {
                    Ok(result) => {
                        let ReverseResult::Session(result, tunnel_conn, read, write) = result;
                        return if result == 0 {
                            self.tunnel_conn = Some(tunnel_conn);
                            self.conn_info_cache.add(self.remote_id.clone(), P2pConnectionInfo {
                                direct: ConnectDirection::Reverse,
                                local_ep: read.local(),
                                remote_ep: read.remote(),
                            }).await;
                            Ok((read, write))
                        } else {
                            Err(p2p_err!(P2pErrorCode::from_u8(result).unwrap_or(P2pErrorCode::ConnectFailed)))
                        }
                    },
                    Err(_) => {
                        has_reversed = true;
                    }
                }
            } else if latest_connection_info.direct == ConnectDirection::Proxy {
                let proxy_conn = self.create_proxy_connection().await?;
                if proxy_conn.is_some() {
                    let conn = proxy_conn.unwrap();
                    let (ack, read, write) = conn.open_session(self.p2p_factory.tunnel_type(), vport, session_id).await?;
                    self.tunnel_conn = Some(conn);
                    self.conn_info_cache.add(self.remote_id.clone(), P2pConnectionInfo {
                        direct: ConnectDirection::Proxy,
                        local_ep: read.local(),
                        remote_ep: read.remote(),
                    }).await;
                    if ack.result == 0 {
                        return Ok((read, write));
                    } else {
                        return Err(p2p_err!(P2pErrorCode::from_u8(ack.result).unwrap_or(P2pErrorCode::ConnectFailed), "ack err session {} port {}", session_id, vport));
                    }
                }
            }
        }

        let mut futures: Vec<Pin<Box<dyn Future<Output=P2pResult<(AckSession, TunnelConnectionRef, TunnelConnectionRead, TunnelConnectionWrite)>> + Send>>> = Vec::new();
        let ep_list = &self.remote_eps;
        let is_lan = self.sn_service.is_same_lan(ep_list);
        for ep in ep_list.iter() {
            if is_lan || ep.is_static_wan() {
                let future = self.create_connection(ep, vport, session_id);
                futures.push(future)
            }
        }

        if futures.len() > 0 {
            match select_successful(futures).await {
                Ok((ack, conn, read, write)) => {
                    self.tunnel_conn = Some(conn);
                    self.conn_info_cache.add(self.remote_id.clone(), P2pConnectionInfo {
                        direct: ConnectDirection::Direct,
                        local_ep: read.local(),
                        remote_ep: read.remote(),
                    }).await;
                    return if ack.result == 0 {
                        Ok((read, write))
                    } else {
                        Err(p2p_err!(P2pErrorCode::from_u8(ack.result).unwrap_or(P2pErrorCode::ConnectFailed), "ack err session {} port {}", session_id, vport))
                    }
                }
                Err(e) => {
                    log::error!("connect stream error: {:?} msg: {}", e.code(), e.msg());
                }
            }
        }

        if !has_reversed {
            let (notify, waiter) = Notify::new();
            future_cache.add_reverse_waiter(self.tunnel_id, notify);
            let call_data = SessionSnCall {
                vport,
                session_id,
            };
            self.sn_service.call(self.get_tunnel_id(),
                                 None,
                                 &self.remote_id,
                                 self.p2p_factory.tunnel_type(),
                                 call_data.to_vec().map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?).await?;
            let result = runtime::timeout(self.conn_timeout, waiter).await.map_err(into_p2p_err!(P2pErrorCode::Timeout))?;
            let ReverseResult::Session(result, tunnel_conn, read, write) = result;
            tunnel_conn.set_tunnel_state(TunnelState::Worked);
            self.tunnel_conn = Some(tunnel_conn);
            self.conn_info_cache.add(self.remote_id.clone(), P2pConnectionInfo {
                direct: ConnectDirection::Reverse,
                local_ep: read.local(),
                remote_ep: read.remote(),
            }).await;
            return if result == 0 {
                Ok((read, write))
            } else {
                Err(p2p_err!(P2pErrorCode::from_u8(result).unwrap_or(P2pErrorCode::ConnectFailed)))
            }
        }

        let proxy_conn = self.create_proxy_connection().await?;
        if proxy_conn.is_some() {
            let conn = proxy_conn.unwrap();
            let (ack, read, write) = conn.open_session(self.p2p_factory.tunnel_type(), vport, session_id).await?;
            self.tunnel_conn = Some(conn);
            self.conn_info_cache.add(self.remote_id.clone(), P2pConnectionInfo {
                direct: ConnectDirection::Proxy,
                local_ep: read.local(),
                remote_ep: read.remote(),
            }).await;
            return if ack.result == 0 {
                Ok((read, write))
            } else {
                Err(p2p_err!(P2pErrorCode::from_u8(ack.result).unwrap_or(P2pErrorCode::ConnectFailed), "ack err session {} port {}", session_id, vport))
            }
        }
        Err(p2p_err!(P2pErrorCode::ConnectFailed, "No available endpoint"))
    }

    pub async fn open_session(&self, vport: u16, session_id: SessionId) -> P2pResult<(TunnelConnectionRead, TunnelConnectionWrite)> {
        if self.tunnel_conn.is_some() {
            let (ack, read, write) = self.tunnel_conn.as_ref().unwrap().open_session(self.p2p_factory.tunnel_type(), vport, session_id).await.map_err(into_p2p_err!(P2pErrorCode::ConnectFailed))?;
            if ack.result == 0 {
                Ok((read, write))
            } else {
                Err(p2p_err!(P2pErrorCode::from_u8(ack.result).unwrap_or(P2pErrorCode::ConnectFailed), "ack err session {} port {}", session_id, vport))
            }
        } else {
            Err(p2p_err!(P2pErrorCode::ConnectFailed, "connect has not been established"))
        }
    }

    pub async fn connect_reverse_session(&mut self, vport: u16, session_id: SessionId, result: u8) -> P2pResult<(TunnelConnectionRead, TunnelConnectionWrite)> {
        if self.tunnel_conn.is_some() {
            let (ack, read, write) = self.tunnel_conn.as_ref().unwrap().open_reverse_session(self.p2p_factory.tunnel_type(),
                                                                                   vport,
                                                                                   session_id,
                                                                                   result).await.map_err(into_p2p_err!(P2pErrorCode::ConnectFailed))?;
            return if ack.result == 0 {
                Ok((read, write))
            } else {
                Err(p2p_err!(P2pErrorCode::from_u8(ack.result).unwrap_or(P2pErrorCode::ConnectFailed), "ack err session {} port {}", session_id, vport))
            }
        }

        let mut futures: Vec<Pin<Box<dyn Future<Output=P2pResult<(AckReverseSession, TunnelConnectionRef, TunnelConnectionRead, TunnelConnectionWrite)>> + Send>>> = Vec::new();
        for ep in self.remote_eps.iter() {
            let sequence = self.tunnel_id;
            let local_identity = self.local_identity.clone();
            let remote_id = self.remote_id.clone();
            let conn_timeout = self.conn_timeout;
            let protocol_version = self.protocol_version;
            let cert_factory = self.cert_factory.clone();
            let p2p_factory = self.p2p_factory.clone();
            let remote_name = self.remote_name.clone();
            let future = Box::pin(async move {
                let conns = p2p_factory.create_connect(&local_identity, ep, &remote_id, remote_name).await?;
                let mut futures: Vec<Pin<Box<dyn Future<Output=P2pResult<(AckReverseSession, TunnelConnectionRef, TunnelConnectionRead, TunnelConnectionWrite)>> + Send>>> = Vec::new();
                for conn in conns {
                    let tunnel_conn = TunnelConnection::new(
                        sequence,
                        local_identity.clone(),
                        remote_id.clone(),
                        ep.clone(),
                        conn_timeout,
                        protocol_version,
                        conn,
                        cert_factory.clone());
                    let p2p_factory = p2p_factory.clone();
                    let future = Box::pin(async move {
                        let (ack, read, write) = tunnel_conn.open_reverse_session(p2p_factory.tunnel_type(), vport, session_id, result).await?;
                        Ok((ack, tunnel_conn, read, write))
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
                Ok((ack, conn, read, write)) => {
                    self.tunnel_conn = Some(conn);
                    self.conn_info_cache.add(self.remote_id.clone(), P2pConnectionInfo {
                        direct: ConnectDirection::Direct,
                        local_ep: read.local(),
                        remote_ep: read.remote(),
                    }).await;
                    return if ack.result == 0 {
                        Ok((read, write))
                    } else {
                        Err(p2p_err!(P2pErrorCode::from_u8(ack.result).unwrap_or(P2pErrorCode::ConnectFailed), "ack err session {} port {}", session_id, vport))
                    }
                }
                Err(e) => {
                    log::error!("connect stream error: {:?}", e);
                }
            }
        }
        Err(p2p_err!(P2pErrorCode::ConnectFailed, "No available endpoint session {} port {}", session_id, vport))
    }

    pub async fn open_reverse_session(&self, vport: u16, session_id: SessionId, result: u8) -> P2pResult<(TunnelConnectionRead, TunnelConnectionWrite)> {
        if self.tunnel_conn.is_some() {
            let (ack, read, write) = self.tunnel_conn.as_ref().unwrap().open_reverse_session(self.p2p_factory.tunnel_type(),
                                                                                        vport,
                                                                                        session_id,
                                                                                        result).await.map_err(into_p2p_err!(P2pErrorCode::ConnectFailed))?;
            if ack.result == 0 {
                Ok((read, write))
            } else {
                Err(p2p_err!(P2pErrorCode::from_u8(ack.result).unwrap_or(P2pErrorCode::ConnectFailed), "ack err session {} port {}", session_id, vport))
            }
        } else {
            Err(p2p_err!(P2pErrorCode::ConnectFailed, "connect has not been established session {} port {}", session_id, vport))
        }
    }
}

impl<F: P2pConnectionFactory> Drop for Tunnel<F> {
    fn drop(&mut self) {
        log::info!("drop tunnel {:?} local {}", self.tunnel_id, self.local_identity.get_id());
    }
}
