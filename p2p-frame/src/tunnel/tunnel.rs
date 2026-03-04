use crate::endpoint::{Endpoint, EndpointArea};
use crate::error::{P2pErrorCode, P2pResult, into_p2p_err, p2p_err};
use crate::p2p_connection::{
    ConnectDirection, P2pConnection, P2pConnectionInfo, P2pConnectionInfoCacheRef,
};
use crate::p2p_identity::{P2pId, P2pIdentityCertFactoryRef, P2pIdentityRef};
use crate::pn::{PnClient, PnClientRef};
use crate::protocol::v0::{AckReverseSession, AckSession, TunnelType};
use crate::runtime;
use crate::sn::client::SNClientServiceRef;
use crate::sockets::NetManagerRef;
use crate::tunnel::proxy_connection::{ProxyConnectionRead, ProxyConnectionWrite};
use crate::tunnel::{
    TunnelConnection, TunnelConnectionRead, TunnelConnectionRef, TunnelConnectionWrite,
    TunnelListenPortsRef, TunnelSession, TunnelStat, TunnelStatRef, TunnelState, select_successful,
};
use crate::types::{SessionId, TunnelId};
use bucky_raw_codec::{RawConvertTo, RawDecode, RawEncode};
use notify_future::Notify;
use num_traits::FromPrimitive;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

// Direct connect starts first, then reverse connect is fired after a short delay
// to reduce tail latency when NAT conditions make direct path slow.
const HEDGED_REVERSE_DELAY: Duration = Duration::from_millis(2000);

macro_rules! p2p_err_from_result {
    ($result:expr) => {{
        let code = P2pErrorCode::from_u8($result).unwrap_or(P2pErrorCode::ConnectFailed);
        sfo_result::Error::new2(code, "".to_string(), file!(), line!())
    }};
    ($result:expr, $($arg:tt)*) => {{
        let code = P2pErrorCode::from_u8($result).unwrap_or(P2pErrorCode::ConnectFailed);
        sfo_result::Error::new2(code, format!($($arg)*), file!(), line!())
    }};
}

pub enum ReverseResult {
    Session(
        u8,
        TunnelConnectionRef,
        TunnelConnectionRead,
        TunnelConnectionWrite,
    ),
}

#[derive(RawEncode, RawDecode)]
pub struct SessionSnCall {
    pub vport: u16,
    pub session_id: SessionId,
}

pub trait ReverseWaiterCache: 'static + Send + Sync {
    fn add_reverse_waiter(&self, sequence: TunnelId, notify: Notify<ReverseResult>);
    fn remove_reverse_waiter(&self, sequence: TunnelId);
    // Reorder direct endpoints by recent outcome and cache preference.
    fn preferred_direct_endpoints(
        &self,
        endpoints: &[Endpoint],
        preferred_ep: Option<&Endpoint>,
    ) -> Vec<Endpoint>;
    // Feed direct connect result back to cache for later ordering.
    fn on_direct_connect_result(&self, endpoint: &Endpoint, success: bool);
}

#[async_trait::async_trait]
pub trait P2pConnectionFactory: Send + Sync + 'static {
    fn tunnel_type(&self) -> TunnelType;
    async fn create_connect(
        &self,
        local_identity: &P2pIdentityRef,
        remote: &Endpoint,
        remote_id: &P2pId,
        remote_name: Option<String>,
    ) -> P2pResult<Vec<P2pConnection>>;
    async fn create_connect_with_local_ep(
        &self,
        local_identity: &P2pIdentityRef,
        local_ep: &Endpoint,
        remote: &Endpoint,
        remote_id: &P2pId,
        remote_name: Option<String>,
    ) -> P2pResult<P2pConnection>;
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
            self.tunnel_conn
                .as_ref()
                .unwrap()
                .set_tunnel_state(new_state);
        }
    }

    pub fn is_idle(&self) -> bool {
        self.tunnel_conn.as_ref().unwrap().is_idle()
    }

    pub fn is_work(&self) -> bool {
        self.tunnel_conn.is_some() && self.tunnel_conn.as_ref().unwrap().is_work()
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

    fn create_connection(
        &self,
        ep: &Endpoint,
        vport: u16,
        session_id: SessionId,
    ) -> Pin<
        Box<
            dyn Future<
                    Output = P2pResult<(
                        AckSession,
                        TunnelConnectionRef,
                        TunnelConnectionRead,
                        TunnelConnectionWrite,
                    )>,
                > + Send,
        >,
    > {
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
            let conns = p2p_factory
                .create_connect(&local_identity, &ep, &remote_id, remote_name)
                .await?;
            let mut futures: Vec<
                Pin<
                    Box<
                        dyn Future<
                                Output = P2pResult<(
                                    AckSession,
                                    TunnelConnectionRef,
                                    TunnelConnectionRead,
                                    TunnelConnectionWrite,
                                )>,
                            > + Send,
                    >,
                >,
            > = Vec::new();
            for conn in conns {
                let tunnel_conn = TunnelConnection::new(
                    sequence,
                    local_identity.clone(),
                    remote_id.clone(),
                    ep.clone(),
                    conn_timeout,
                    protocol_version,
                    conn,
                    cert_factory.clone(),
                );
                let p2p_factory = p2p_factory.clone();
                let future = Box::pin(async move {
                    let (ack, read, write) = tunnel_conn
                        .open_session(p2p_factory.tunnel_type(), vport, session_id)
                        .await?;
                    Ok((ack, tunnel_conn, read, write))
                });
                futures.push(future);
            }
            if futures.len() > 0 {
                select_successful(futures).await
            } else {
                Err(p2p_err!(
                    P2pErrorCode::ConnectFailed,
                    "No available endpoint"
                ))
            }
        });
        future
    }

    async fn create_proxy_connection(&self) -> P2pResult<Option<TunnelConnectionRef>> {
        if self.pn_client.is_none() {
            return Ok(None);
        }

        let pn_client = self.pn_client.as_ref().unwrap();

        let (read, write) = runtime::timeout(
            self.conn_timeout,
            pn_client.connect(
                self.tunnel_id,
                self.remote_id.clone(),
                self.remote_name.clone(),
            ),
        )
        .await
        .map_err(into_p2p_err!(P2pErrorCode::Timeout))??;
        let read = Box::new(ProxyConnectionRead::new(read, self.local_identity.get_id()));
        let write = Box::new(ProxyConnectionWrite::new(
            write,
            self.local_identity.get_id(),
        ));
        let proxy_conn = P2pConnection::new(read, write);
        let conn = TunnelConnection::new(
            self.tunnel_id,
            self.local_identity.clone(),
            self.remote_id.clone(),
            Endpoint::default(),
            self.conn_timeout,
            self.protocol_version,
            proxy_conn,
            self.cert_factory.clone(),
        );
        Ok(Some(conn))
    }

    async fn connect_reverse_by_sn(
        &self,
        vport: u16,
        session_id: SessionId,
        future_cache: Arc<dyn ReverseWaiterCache>,
    ) -> P2pResult<(
        u8,
        TunnelConnectionRef,
        TunnelConnectionRead,
        TunnelConnectionWrite,
    )> {
        Self::connect_reverse_by_sn_inner(
            self.sn_service.clone(),
            self.tunnel_id,
            self.remote_id.clone(),
            self.p2p_factory.tunnel_type(),
            self.conn_timeout,
            vport,
            session_id,
            future_cache,
        )
        .await
    }

    async fn connect_reverse_by_sn_inner(
        sn_service: SNClientServiceRef,
        tunnel_id: TunnelId,
        remote_id: P2pId,
        tunnel_type: TunnelType,
        conn_timeout: Duration,
        vport: u16,
        session_id: SessionId,
        future_cache: Arc<dyn ReverseWaiterCache>,
    ) -> P2pResult<(
        u8,
        TunnelConnectionRef,
        TunnelConnectionRead,
        TunnelConnectionWrite,
    )> {
        // Always pair add/remove for reverse waiter to avoid pending waiter leaks
        // on call error, timeout, or normal completion.
        let (notify, waiter) = Notify::new();
        future_cache.add_reverse_waiter(tunnel_id, notify);
        let call_data = SessionSnCall { vport, session_id };
        log::info!(
            "tunnel {:?} remote {} path reverse sn-call start session {} vport {}",
            tunnel_id,
            remote_id,
            session_id,
            vport
        );
        let call_result = sn_service
            .call(
                tunnel_id,
                None,
                &remote_id,
                tunnel_type,
                call_data
                    .to_vec()
                    .map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?,
            )
            .await;
        if let Err(err) = call_result {
            future_cache.remove_reverse_waiter(tunnel_id);
            log::warn!(
                "tunnel {:?} remote {} path reverse sn-call failed session {} vport {} err {:?}",
                tunnel_id,
                remote_id,
                session_id,
                vport,
                err
            );
            return Err(err);
        }

        let result = runtime::timeout(conn_timeout, waiter)
            .await
            .map_err(into_p2p_err!(P2pErrorCode::Timeout));
        future_cache.remove_reverse_waiter(tunnel_id);
        let result = result?;
        let ReverseResult::Session(result, tunnel_conn, read, write) = result;
        log::info!(
            "tunnel {:?} remote {} path reverse sn-call done session {} vport {} result {}",
            tunnel_id,
            remote_id,
            session_id,
            vport,
            result
        );
        Ok((result, tunnel_conn, read, write))
    }

    pub async fn connect_session(
        &mut self,
        vport: u16,
        session_id: SessionId,
        future_cache: Arc<dyn ReverseWaiterCache>,
    ) -> P2pResult<(TunnelConnectionRead, TunnelConnectionWrite)> {
        if self.tunnel_conn.is_some() {
            log::info!(
                "tunnel {:?} remote {} path reuse session {} vport {}",
                self.tunnel_id,
                self.remote_id,
                session_id,
                vport
            );
            let (ack, read, write) = self
                .tunnel_conn
                .as_ref()
                .unwrap()
                .open_session(self.p2p_factory.tunnel_type(), vport, session_id)
                .await
                .map_err(into_p2p_err!(P2pErrorCode::ConnectFailed))?;
            return if ack.result == 0 {
                Ok((read, write))
            } else {
                Err(p2p_err_from_result!(
                    ack.result,
                    "ack err session {} port {}",
                    session_id,
                    vport
                ))
            };
        }

        let mut has_reversed = false;
        if let Some(latest_connection_info) = self.conn_info_cache.get(&self.remote_id).await {
            if latest_connection_info.direct == ConnectDirection::Direct {
                for ep in self.remote_eps.iter() {
                    if ep == &latest_connection_info.remote_ep {
                        log::info!(
                            "tunnel {:?} remote {} path direct(cache) start session {} vport {} ep {}",
                            self.tunnel_id,
                            self.remote_id,
                            session_id,
                            vport,
                            ep
                        );
                        match self.create_connection(ep, vport, session_id).await {
                            Ok((ack, conn, read, write)) => {
                                future_cache.on_direct_connect_result(ep, true);
                                conn.set_tunnel_state(TunnelState::Worked);
                                self.tunnel_conn = Some(conn);
                                return if ack.result == 0 {
                                    Ok((read, write))
                                } else {
                                    Err(p2p_err_from_result!(
                                        ack.result,
                                        "ack err session {} port {}",
                                        session_id,
                                        vport
                                    ))
                                };
                            }
                            Err(e) => {
                                future_cache.on_direct_connect_result(ep, false);
                                log::warn!(
                                    "connect stream with cached endpoint failed: {:?} msg: {}",
                                    e.code(),
                                    e.msg()
                                );
                            }
                        }
                    }
                }
            } else if latest_connection_info.direct == ConnectDirection::Reverse {
                log::info!(
                    "tunnel {:?} remote {} path reverse(cache) start session {} vport {}",
                    self.tunnel_id,
                    self.remote_id,
                    session_id,
                    vport
                );
                match self
                    .connect_reverse_by_sn(vport, session_id, future_cache.clone())
                    .await
                {
                    Ok((result, tunnel_conn, read, write)) => {
                        tunnel_conn.set_tunnel_state(TunnelState::Worked);
                        self.tunnel_conn = Some(tunnel_conn);
                        self.conn_info_cache
                            .add(
                                self.remote_id.clone(),
                                P2pConnectionInfo {
                                    direct: ConnectDirection::Reverse,
                                    local_ep: read.local(),
                                    remote_ep: read.remote(),
                                },
                            )
                            .await;
                        return if result == 0 {
                            Ok((read, write))
                        } else {
                            Err(p2p_err_from_result!(result))
                        };
                    }
                    Err(e) => {
                        has_reversed = true;
                        log::warn!(
                            "connect reverse with cache direction failed: {:?} msg: {}",
                            e.code(),
                            e.msg()
                        );
                    }
                }
            } else if latest_connection_info.direct == ConnectDirection::Proxy {
                log::info!(
                    "tunnel {:?} remote {} path proxy(cache) start session {} vport {}",
                    self.tunnel_id,
                    self.remote_id,
                    session_id,
                    vport
                );
                let proxy_conn = self.create_proxy_connection().await?;
                if proxy_conn.is_some() {
                    let conn = proxy_conn.unwrap();
                    let (ack, read, write) = conn
                        .open_session(self.p2p_factory.tunnel_type(), vport, session_id)
                        .await?;
                    conn.set_tunnel_state(TunnelState::Worked);
                    self.tunnel_conn = Some(conn);
                    self.conn_info_cache
                        .add(
                            self.remote_id.clone(),
                            P2pConnectionInfo {
                                direct: ConnectDirection::Proxy,
                                local_ep: read.local(),
                                remote_ep: read.remote(),
                            },
                        )
                        .await;
                    if ack.result == 0 {
                        return Ok((read, write));
                    } else {
                        return Err(p2p_err_from_result!(
                            ack.result,
                            "ack err session {} port {}",
                            session_id,
                            vport
                        ));
                    }
                }
            }
        }

        let mut futures: Vec<
            Pin<
                Box<
                    dyn Future<
                            Output = P2pResult<(
                                AckSession,
                                TunnelConnectionRef,
                                TunnelConnectionRead,
                                TunnelConnectionWrite,
                            )>,
                        > + Send,
                >,
            >,
        > = Vec::new();
        let ep_list = &self.remote_eps;
        let is_lan = self.sn_service.is_same_lan(ep_list);
        let mut direct_eps = Vec::new();
        for ep in ep_list.iter() {
            if is_lan || ep.is_static_wan() {
                direct_eps.push(ep.clone());
            }
        }
        // First follow last successful endpoint from connection cache, then apply
        // per-endpoint success/failure scores.
        let preferred_ep = self
            .conn_info_cache
            .get(&self.remote_id)
            .await
            .map(|v| v.remote_ep);
        let direct_eps =
            future_cache.preferred_direct_endpoints(direct_eps.as_slice(), preferred_ep.as_ref());
        for ep in direct_eps.iter() {
            let future = self.create_connection(ep, vport, session_id);
            futures.push(future);
        }

        if futures.len() > 0 {
            if !has_reversed {
                log::info!(
                    "tunnel {:?} remote {} path hedged start session {} vport {} direct_eps {:?} delay_ms {}",
                    self.tunnel_id,
                    self.remote_id,
                    session_id,
                    vport,
                    direct_eps,
                    HEDGED_REVERSE_DELAY.as_millis()
                );
                let reverse_future_cache = future_cache.clone();
                let reverse_sn_service = self.sn_service.clone();
                let reverse_tunnel_id = self.tunnel_id;
                let reverse_remote_id = self.remote_id.clone();
                let reverse_tunnel_type = self.p2p_factory.tunnel_type();
                let reverse_conn_timeout = self.conn_timeout;
                let reverse_future = async {
                    runtime::sleep(HEDGED_REVERSE_DELAY).await;
                    Self::connect_reverse_by_sn_inner(
                        reverse_sn_service,
                        reverse_tunnel_id,
                        reverse_remote_id,
                        reverse_tunnel_type,
                        reverse_conn_timeout,
                        vport,
                        session_id,
                        reverse_future_cache,
                    )
                    .await
                };
                let direct_future = select_successful(futures);
                tokio::pin!(reverse_future);
                tokio::pin!(direct_future);

                // Hedged race: direct and reverse compete, winner is used.
                tokio::select! {
                    direct_result = &mut direct_future => {
                        match direct_result {
                            Ok((ack, conn, read, write)) => {
                                log::info!(
                                    "tunnel {:?} remote {} path direct won race session {} vport {} ep {} ack {}",
                                    self.tunnel_id,
                                    self.remote_id,
                                    session_id,
                                    vport,
                                    read.remote(),
                                    ack.result
                                );
                                future_cache.on_direct_connect_result(&read.remote(), true);
                                conn.set_tunnel_state(TunnelState::Worked);
                                self.tunnel_conn = Some(conn);
                                self.conn_info_cache.add(self.remote_id.clone(), P2pConnectionInfo {
                                    direct: ConnectDirection::Direct,
                                    local_ep: read.local(),
                                    remote_ep: read.remote(),
                                }).await;
                                return if ack.result == 0 {
                                    Ok((read, write))
                                } else {
                                    Err(p2p_err_from_result!(
                                        ack.result,
                                        "ack err session {} port {}",
                                        session_id,
                                        vport
                                    ))
                                };
                            }
                            Err(e) => {
                                has_reversed = true;
                                for ep in direct_eps.iter() {
                                    future_cache.on_direct_connect_result(ep, false);
                                }
                                log::warn!("connect direct failed, fallback to reverse: {:?} msg: {}", e.code(), e.msg());
                                match reverse_future.await {
                                    Ok((result, tunnel_conn, read, write)) => {
                                        log::info!(
                                            "tunnel {:?} remote {} path reverse after direct fail session {} vport {} result {}",
                                            self.tunnel_id,
                                            self.remote_id,
                                            session_id,
                                            vport,
                                            result
                                        );
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
                                            Err(p2p_err_from_result!(result))
                                        };
                                    }
                                    Err(reverse_err) => {
                                        log::warn!("connect reverse after direct failed: {:?} msg: {}", reverse_err.code(), reverse_err.msg());
                                    }
                                }
                            }
                        }
                    }
                    reverse_result = &mut reverse_future => {
                        has_reversed = true;
                        match reverse_result {
                            Ok((result, tunnel_conn, read, write)) => {
                                log::info!(
                                    "tunnel {:?} remote {} path reverse won race session {} vport {} result {}",
                                    self.tunnel_id,
                                    self.remote_id,
                                    session_id,
                                    vport,
                                    result
                                );
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
                                    Err(p2p_err_from_result!(result))
                                };
                            }
                            Err(e) => {
                                log::warn!("connect reverse failed, fallback to direct: {:?} msg: {}", e.code(), e.msg());
                                match direct_future.await {
                                    Ok((ack, conn, read, write)) => {
                                        log::info!(
                                            "tunnel {:?} remote {} path direct after reverse fail session {} vport {} ep {} ack {}",
                                            self.tunnel_id,
                                            self.remote_id,
                                            session_id,
                                            vport,
                                            read.remote(),
                                            ack.result
                                        );
                                        future_cache.on_direct_connect_result(&read.remote(), true);
                                        conn.set_tunnel_state(TunnelState::Worked);
                                        self.tunnel_conn = Some(conn);
                                        self.conn_info_cache.add(self.remote_id.clone(), P2pConnectionInfo {
                                            direct: ConnectDirection::Direct,
                                            local_ep: read.local(),
                                            remote_ep: read.remote(),
                                        }).await;
                                        return if ack.result == 0 {
                                            Ok((read, write))
                                        } else {
                                            Err(p2p_err_from_result!(
                                                ack.result,
                                                "ack err session {} port {}",
                                                session_id,
                                                vport
                                            ))
                                        };
                                    }
                                    Err(direct_err) => {
                                        log::warn!("connect direct after reverse failed: {:?} msg: {}", direct_err.code(), direct_err.msg());
                                    }
                                }
                            }
                        }
                    }
                }
            } else {
                match select_successful(futures).await {
                    Ok((ack, conn, read, write)) => {
                        log::info!(
                            "tunnel {:?} remote {} path direct-only session {} vport {} ep {} ack {}",
                            self.tunnel_id,
                            self.remote_id,
                            session_id,
                            vport,
                            read.remote(),
                            ack.result
                        );
                        future_cache.on_direct_connect_result(&read.remote(), true);
                        conn.set_tunnel_state(TunnelState::Worked);
                        self.tunnel_conn = Some(conn);
                        self.conn_info_cache
                            .add(
                                self.remote_id.clone(),
                                P2pConnectionInfo {
                                    direct: ConnectDirection::Direct,
                                    local_ep: read.local(),
                                    remote_ep: read.remote(),
                                },
                            )
                            .await;
                        return if ack.result == 0 {
                            Ok((read, write))
                        } else {
                            Err(p2p_err_from_result!(
                                ack.result,
                                "ack err session {} port {}",
                                session_id,
                                vport
                            ))
                        };
                    }
                    Err(e) => {
                        for ep in direct_eps.iter() {
                            future_cache.on_direct_connect_result(ep, false);
                        }
                        log::error!("connect stream error: {:?} msg: {}", e.code(), e.msg());
                    }
                }
            }
        }

        if !has_reversed {
            log::debug!(
                "tunnel {:?} remote {} path reverse fallback start session {} vport {}",
                self.tunnel_id,
                self.remote_id,
                session_id,
                vport
            );
            match self
                .connect_reverse_by_sn(vport, session_id, future_cache.clone())
                .await
            {
                Ok((result, tunnel_conn, read, write)) => {
                    log::info!(
                        "tunnel {:?} remote {} path reverse fallback done session {} vport {} result {}",
                        self.tunnel_id,
                        self.remote_id,
                        session_id,
                        vport,
                        result
                    );
                    tunnel_conn.set_tunnel_state(TunnelState::Worked);
                    self.tunnel_conn = Some(tunnel_conn);
                    self.conn_info_cache
                        .add(
                            self.remote_id.clone(),
                            P2pConnectionInfo {
                                direct: ConnectDirection::Reverse,
                                local_ep: read.local(),
                                remote_ep: read.remote(),
                            },
                        )
                        .await;
                    return if result == 0 {
                        Ok((read, write))
                    } else {
                        Err(p2p_err_from_result!(result))
                    };
                }
                Err(e) => {
                    log::warn!("connect reverse failed: {:?} msg: {}", e.code(), e.msg());
                }
            }
        }

        log::debug!(
            "tunnel {:?} remote {} path proxy fallback start session {} vport {}",
            self.tunnel_id,
            self.remote_id,
            session_id,
            vport
        );
        let proxy_conn = self.create_proxy_connection().await?;
        if proxy_conn.is_some() {
            let conn = proxy_conn.unwrap();
            let (ack, read, write) = conn
                .open_session(self.p2p_factory.tunnel_type(), vport, session_id)
                .await?;
            log::info!(
                "tunnel {:?} remote {} path proxy fallback done session {} vport {} ack {}",
                self.tunnel_id,
                self.remote_id,
                session_id,
                vport,
                ack.result
            );
            conn.set_tunnel_state(TunnelState::Worked);
            self.tunnel_conn = Some(conn);
            self.conn_info_cache
                .add(
                    self.remote_id.clone(),
                    P2pConnectionInfo {
                        direct: ConnectDirection::Proxy,
                        local_ep: read.local(),
                        remote_ep: read.remote(),
                    },
                )
                .await;
            return if ack.result == 0 {
                Ok((read, write))
            } else {
                Err(p2p_err_from_result!(
                    ack.result,
                    "ack err session {} port {}",
                    session_id,
                    vport
                ))
            };
        }
        log::warn!(
            "tunnel {:?} remote {} create session failed all paths session {} vport {}",
            self.tunnel_id,
            self.remote_id,
            session_id,
            vport
        );
        Err(p2p_err!(
            P2pErrorCode::ConnectFailed,
            "No available endpoint"
        ))
    }

    pub async fn open_session(
        &self,
        vport: u16,
        session_id: SessionId,
    ) -> P2pResult<(TunnelConnectionRead, TunnelConnectionWrite)> {
        if self.tunnel_conn.is_some() {
            let (ack, read, write) = self
                .tunnel_conn
                .as_ref()
                .unwrap()
                .open_session(self.p2p_factory.tunnel_type(), vport, session_id)
                .await
                .map_err(into_p2p_err!(P2pErrorCode::ConnectFailed))?;
            if ack.result == 0 {
                Ok((read, write))
            } else {
                Err(p2p_err_from_result!(
                    ack.result,
                    "ack err session {} port {}",
                    session_id,
                    vport
                ))
            }
        } else {
            Err(p2p_err!(
                P2pErrorCode::ConnectFailed,
                "connect has not been established"
            ))
        }
    }

    pub async fn connect_reverse_session(
        &mut self,
        vport: u16,
        session_id: SessionId,
        result: u8,
    ) -> P2pResult<(TunnelConnectionRead, TunnelConnectionWrite)> {
        if self.tunnel_conn.is_some() {
            let (ack, read, write) = self
                .tunnel_conn
                .as_ref()
                .unwrap()
                .open_reverse_session(self.p2p_factory.tunnel_type(), vport, session_id, result)
                .await
                .map_err(into_p2p_err!(P2pErrorCode::ConnectFailed))?;
            return if ack.result == 0 {
                Ok((read, write))
            } else {
                Err(p2p_err_from_result!(
                    ack.result,
                    "ack err session {} port {}",
                    session_id,
                    vport
                ))
            };
        }

        let mut futures: Vec<
            Pin<
                Box<
                    dyn Future<
                            Output = P2pResult<(
                                AckReverseSession,
                                TunnelConnectionRef,
                                TunnelConnectionRead,
                                TunnelConnectionWrite,
                            )>,
                        > + Send,
                >,
            >,
        > = Vec::new();
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
                let conns = p2p_factory
                    .create_connect(&local_identity, ep, &remote_id, remote_name)
                    .await?;
                let mut futures: Vec<
                    Pin<
                        Box<
                            dyn Future<
                                    Output = P2pResult<(
                                        AckReverseSession,
                                        TunnelConnectionRef,
                                        TunnelConnectionRead,
                                        TunnelConnectionWrite,
                                    )>,
                                > + Send,
                        >,
                    >,
                > = Vec::new();
                for conn in conns {
                    let tunnel_conn = TunnelConnection::new(
                        sequence,
                        local_identity.clone(),
                        remote_id.clone(),
                        ep.clone(),
                        conn_timeout,
                        protocol_version,
                        conn,
                        cert_factory.clone(),
                    );
                    let p2p_factory = p2p_factory.clone();
                    let future = Box::pin(async move {
                        let (ack, read, write) = tunnel_conn
                            .open_reverse_session(
                                p2p_factory.tunnel_type(),
                                vport,
                                session_id,
                                result,
                            )
                            .await?;
                        Ok((ack, tunnel_conn, read, write))
                    });
                    futures.push(future);
                }
                if futures.len() > 0 {
                    select_successful(futures).await
                } else {
                    Err(p2p_err!(
                        P2pErrorCode::ConnectFailed,
                        "No available endpoint"
                    ))
                }
            });
            futures.push(future);
        }

        if futures.len() > 0 {
            match select_successful(futures).await {
                Ok((ack, conn, read, write)) => {
                    self.tunnel_conn = Some(conn);
                    self.conn_info_cache
                        .add(
                            self.remote_id.clone(),
                            P2pConnectionInfo {
                                direct: ConnectDirection::Direct,
                                local_ep: read.local(),
                                remote_ep: read.remote(),
                            },
                        )
                        .await;
                    return if ack.result == 0 {
                        Ok((read, write))
                    } else {
                        Err(p2p_err_from_result!(
                            ack.result,
                            "ack err session {} port {}",
                            session_id,
                            vport
                        ))
                    };
                }
                Err(e) => {
                    log::error!("connect stream error: {:?}", e);
                }
            }
        }
        Err(p2p_err!(
            P2pErrorCode::ConnectFailed,
            "No available endpoint session {} port {}",
            session_id,
            vport
        ))
    }

    pub async fn open_reverse_session(
        &self,
        vport: u16,
        session_id: SessionId,
        result: u8,
    ) -> P2pResult<(TunnelConnectionRead, TunnelConnectionWrite)> {
        if self.tunnel_conn.is_some() {
            let (ack, read, write) = self
                .tunnel_conn
                .as_ref()
                .unwrap()
                .open_reverse_session(self.p2p_factory.tunnel_type(), vport, session_id, result)
                .await
                .map_err(into_p2p_err!(P2pErrorCode::ConnectFailed))?;
            if ack.result == 0 {
                Ok((read, write))
            } else {
                Err(p2p_err_from_result!(
                    ack.result,
                    "ack err session {} port {}",
                    session_id,
                    vport
                ))
            }
        } else {
            Err(p2p_err!(
                P2pErrorCode::ConnectFailed,
                "connect has not been established session {} port {}",
                session_id,
                vport
            ))
        }
    }

    pub async fn connect_session_direct(
        &mut self,
        vport: u16,
        session_id: SessionId,
    ) -> P2pResult<(TunnelConnectionRead, TunnelConnectionWrite)> {
        let mut futures: Vec<
            Pin<
                Box<
                    dyn Future<
                            Output = P2pResult<(
                                AckSession,
                                TunnelConnectionRef,
                                TunnelConnectionRead,
                                TunnelConnectionWrite,
                            )>,
                        > + Send,
                >,
            >,
        > = Vec::new();
        for ep in self.remote_eps.iter() {
            let future = self.create_connection(ep, vport, session_id);
            futures.push(future);
        }
        if futures.len() > 0 {
            match select_successful(futures).await {
                Ok((ack, conn, read, write)) => {
                    self.tunnel_conn = Some(conn);
                    if ack.result == 0 {
                        Ok((read, write))
                    } else {
                        Err(p2p_err_from_result!(
                            ack.result,
                            "ack err session {} port {}",
                            session_id,
                            vport
                        ))
                    }
                }
                Err(e) => Err(p2p_err!(
                    P2pErrorCode::ConnectFailed,
                    "connect session {} {} error: {:?} msg: {}",
                    session_id,
                    vport,
                    e.code(),
                    e.msg()
                )),
            }
        } else {
            Err(p2p_err!(
                P2pErrorCode::ConnectFailed,
                "No available endpoint session {} port {}",
                session_id,
                vport
            ))
        }
    }
}

impl<F: P2pConnectionFactory> Drop for Tunnel<F> {
    fn drop(&mut self) {
        log::info!(
            "drop tunnel {:?} local {}",
            self.tunnel_id,
            self.local_identity.get_id()
        );
    }
}

#[cfg(test)]
mod tests {
    use std::io::Error;
    use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
    use std::pin::Pin;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::{Context, Poll};

    use super::*;
    use crate::endpoint::Protocol;
    use crate::executor::Executor;
    use crate::p2p_connection::{
        ConnectDirection, DefaultP2pConnectionInfoCache, P2pConnectionInfo,
        P2pConnectionInfoCacheRef,
    };
    use crate::p2p_identity::{
        EncodedP2pIdentity, EncodedP2pIdentityCert, P2pIdentity, P2pIdentityCert,
        P2pIdentityCertRef, P2pSignature,
    };
    use crate::protocol::v0::{AckSession, SynSession};
    use crate::protocol::{Package, PackageCmdCode, PackageHeader};
    use crate::sn::client::SNClientService;
    use crate::sockets::NetManager;
    use crate::tls::DefaultTlsServerCertResolver;
    use crate::types::{SequenceGenerator, TunnelIdGenerator};
    use bucky_raw_codec::{RawFixedBytes, RawFrom};

    struct DummyIdentityCert {
        id: P2pId,
        name: String,
        endpoints: Vec<Endpoint>,
    }

    impl P2pIdentityCert for DummyIdentityCert {
        fn get_id(&self) -> P2pId {
            self.id.clone()
        }

        fn get_name(&self) -> String {
            self.name.clone()
        }

        fn verify(&self, _message: &[u8], _sign: &P2pSignature) -> bool {
            true
        }

        fn verify_cert(&self, _name: &str) -> bool {
            true
        }

        fn get_encoded_cert(&self) -> P2pResult<EncodedP2pIdentityCert> {
            Ok(vec![])
        }

        fn endpoints(&self) -> Vec<Endpoint> {
            self.endpoints.clone()
        }

        fn sn_list(&self) -> Vec<crate::p2p_identity::P2pSn> {
            vec![]
        }

        fn update_endpoints(&self, eps: Vec<Endpoint>) -> P2pIdentityCertRef {
            Arc::new(Self {
                id: self.id.clone(),
                name: self.name.clone(),
                endpoints: eps,
            })
        }
    }

    struct DummyIdentity {
        id: P2pId,
        name: String,
        endpoints: Vec<Endpoint>,
    }

    impl P2pIdentity for DummyIdentity {
        fn get_identity_cert(&self) -> P2pResult<P2pIdentityCertRef> {
            Ok(Arc::new(DummyIdentityCert {
                id: self.id.clone(),
                name: self.name.clone(),
                endpoints: self.endpoints.clone(),
            }))
        }

        fn get_id(&self) -> P2pId {
            self.id.clone()
        }

        fn get_name(&self) -> String {
            self.name.clone()
        }

        fn sign(&self, _message: &[u8]) -> P2pResult<P2pSignature> {
            Ok(vec![])
        }

        fn get_encoded_identity(&self) -> P2pResult<EncodedP2pIdentity> {
            Ok(vec![])
        }

        fn endpoints(&self) -> Vec<Endpoint> {
            self.endpoints.clone()
        }

        fn update_endpoints(&self, eps: Vec<Endpoint>) -> P2pIdentityRef {
            Arc::new(Self {
                id: self.id.clone(),
                name: self.name.clone(),
                endpoints: eps,
            })
        }
    }

    struct DummyIdentityCertFactory;

    impl crate::p2p_identity::P2pIdentityCertFactory for DummyIdentityCertFactory {
        fn create(&self, _cert: &EncodedP2pIdentityCert) -> P2pResult<P2pIdentityCertRef> {
            Ok(Arc::new(DummyIdentityCert {
                id: P2pId::from(vec![9u8; 32]),
                name: "dummy-cert".to_owned(),
                endpoints: vec![],
            }))
        }
    }

    struct EmptyConnectionFactory {
        calls: Arc<AtomicUsize>,
    }

    impl EmptyConnectionFactory {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                calls: Arc::new(AtomicUsize::new(0)),
            })
        }

        fn call_count(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    struct ScriptedConnectionFactory {
        calls: Arc<AtomicUsize>,
        ack_result: u8,
    }

    impl ScriptedConnectionFactory {
        fn new(ack_result: u8) -> Arc<Self> {
            Arc::new(Self {
                calls: Arc::new(AtomicUsize::new(0)),
                ack_result,
            })
        }

        fn call_count(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait::async_trait]
    impl P2pConnectionFactory for ScriptedConnectionFactory {
        fn tunnel_type(&self) -> TunnelType {
            TunnelType::Stream
        }

        async fn create_connect(
            &self,
            local_identity: &P2pIdentityRef,
            remote: &Endpoint,
            remote_id: &P2pId,
            _remote_name: Option<String>,
        ) -> P2pResult<Vec<P2pConnection>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(vec![mock_p2p_connection(
                local_identity.get_id(),
                remote_id.clone(),
                *remote,
                self.ack_result,
            )])
        }

        async fn create_connect_with_local_ep(
            &self,
            _local_identity: &P2pIdentityRef,
            _local_ep: &Endpoint,
            _remote: &Endpoint,
            _remote_id: &P2pId,
            _remote_name: Option<String>,
        ) -> P2pResult<P2pConnection> {
            Err(p2p_err!(P2pErrorCode::NotSupport, "not used in test"))
        }
    }

    struct MockP2pRead {
        inner: tokio::io::ReadHalf<tokio::io::DuplexStream>,
        local_ep: Endpoint,
        remote_ep: Endpoint,
        local_id: P2pId,
        remote_id: P2pId,
    }

    impl runtime::AsyncRead for MockP2pRead {
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut runtime::ReadBuf<'_>,
        ) -> Poll<Result<(), Error>> {
            Pin::new(&mut self.inner).poll_read(cx, buf)
        }
    }

    impl crate::p2p_connection::P2pRead for MockP2pRead {
        fn remote(&self) -> Endpoint {
            self.remote_ep
        }

        fn local(&self) -> Endpoint {
            self.local_ep
        }

        fn remote_id(&self) -> P2pId {
            self.remote_id.clone()
        }

        fn local_id(&self) -> P2pId {
            self.local_id.clone()
        }

        fn remote_name(&self) -> String {
            self.remote_id.to_string()
        }
    }

    struct MockP2pWrite {
        inner: tokio::io::WriteHalf<tokio::io::DuplexStream>,
        local_ep: Endpoint,
        remote_ep: Endpoint,
        local_id: P2pId,
        remote_id: P2pId,
    }

    impl runtime::AsyncWrite for MockP2pWrite {
        fn poll_write(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<Result<usize, Error>> {
            Pin::new(&mut self.inner).poll_write(cx, buf)
        }

        fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
            Pin::new(&mut self.inner).poll_flush(cx)
        }

        fn poll_shutdown(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
        ) -> Poll<Result<(), Error>> {
            Pin::new(&mut self.inner).poll_shutdown(cx)
        }
    }

    impl crate::p2p_connection::P2pWrite for MockP2pWrite {
        fn remote(&self) -> Endpoint {
            self.remote_ep
        }

        fn local(&self) -> Endpoint {
            self.local_ep
        }

        fn remote_id(&self) -> P2pId {
            self.remote_id.clone()
        }

        fn local_id(&self) -> P2pId {
            self.local_id.clone()
        }

        fn remote_name(&self) -> String {
            self.remote_id.to_string()
        }
    }

    fn mock_p2p_connection(
        local_id: P2pId,
        remote_id: P2pId,
        remote_ep: Endpoint,
        ack_result: u8,
    ) -> P2pConnection {
        let local_ep = test_wan_endpoint(39999);
        let (local_stream, mut peer_stream) = tokio::io::duplex(4096);
        let (read_half, write_half) = tokio::io::split(local_stream);

        tokio::spawn(async move {
            let mut header_buf = [0u8; 16];
            let header_len = PackageHeader::raw_bytes().unwrap();
            if runtime::AsyncReadExt::read_exact(&mut peer_stream, &mut header_buf[..header_len])
                .await
                .is_err()
            {
                return;
            }
            let header = match PackageHeader::clone_from_slice(header_buf.as_slice()) {
                Ok(h) => h,
                Err(_) => return,
            };
            let mut body = vec![0u8; header.pkg_len() as usize];
            if runtime::AsyncReadExt::read_exact(&mut peer_stream, body.as_mut_slice())
                .await
                .is_err()
            {
                return;
            }
            if header.cmd_code().ok() != Some(PackageCmdCode::SynSession) {
                return;
            }
            let _ = SynSession::clone_from_slice(body.as_slice());

            let ack = Package::new(
                0,
                PackageCmdCode::AckSession,
                AckSession { result: ack_result },
            );
            let ack_buf = match ack.to_vec() {
                Ok(v) => v,
                Err(_) => return,
            };
            if runtime::AsyncWriteExt::write_all(&mut peer_stream, ack_buf.as_slice())
                .await
                .is_err()
            {
                return;
            }
            let _ = runtime::AsyncWriteExt::flush(&mut peer_stream).await;
        });

        P2pConnection::new(
            Box::new(MockP2pRead {
                inner: read_half,
                local_ep,
                remote_ep,
                local_id: local_id.clone(),
                remote_id: remote_id.clone(),
            }),
            Box::new(MockP2pWrite {
                inner: write_half,
                local_ep,
                remote_ep,
                local_id,
                remote_id,
            }),
        )
    }

    #[async_trait::async_trait]
    impl P2pConnectionFactory for EmptyConnectionFactory {
        fn tunnel_type(&self) -> TunnelType {
            TunnelType::Stream
        }

        async fn create_connect(
            &self,
            _local_identity: &P2pIdentityRef,
            _remote: &Endpoint,
            _remote_id: &P2pId,
            _remote_name: Option<String>,
        ) -> P2pResult<Vec<P2pConnection>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(vec![])
        }

        async fn create_connect_with_local_ep(
            &self,
            _local_identity: &P2pIdentityRef,
            _local_ep: &Endpoint,
            _remote: &Endpoint,
            _remote_id: &P2pId,
            _remote_name: Option<String>,
        ) -> P2pResult<P2pConnection> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Err(p2p_err!(P2pErrorCode::NotSupport, "not used in test"))
        }
    }

    fn test_endpoint(port: u16) -> Endpoint {
        Endpoint::from((
            Protocol::Quic,
            SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), port)),
        ))
    }

    fn test_wan_endpoint(port: u16) -> Endpoint {
        let mut ep = test_endpoint(port);
        ep.set_area(EndpointArea::Wan);
        ep
    }

    struct MockReverseWaiterCache {
        preferred_calls: AtomicUsize,
        direct_result_calls: AtomicUsize,
        reverse_add_calls: AtomicUsize,
        reverse_remove_calls: AtomicUsize,
    }

    impl MockReverseWaiterCache {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                preferred_calls: AtomicUsize::new(0),
                direct_result_calls: AtomicUsize::new(0),
                reverse_add_calls: AtomicUsize::new(0),
                reverse_remove_calls: AtomicUsize::new(0),
            })
        }

        fn preferred_calls(&self) -> usize {
            self.preferred_calls.load(Ordering::SeqCst)
        }

        fn direct_result_calls(&self) -> usize {
            self.direct_result_calls.load(Ordering::SeqCst)
        }

        fn reverse_add_calls(&self) -> usize {
            self.reverse_add_calls.load(Ordering::SeqCst)
        }

        fn reverse_remove_calls(&self) -> usize {
            self.reverse_remove_calls.load(Ordering::SeqCst)
        }
    }

    impl ReverseWaiterCache for MockReverseWaiterCache {
        fn add_reverse_waiter(&self, _sequence: TunnelId, _notify: Notify<ReverseResult>) {
            self.reverse_add_calls.fetch_add(1, Ordering::SeqCst);
        }

        fn remove_reverse_waiter(&self, _sequence: TunnelId) {
            self.reverse_remove_calls.fetch_add(1, Ordering::SeqCst);
        }

        fn preferred_direct_endpoints(
            &self,
            endpoints: &[Endpoint],
            _preferred_ep: Option<&Endpoint>,
        ) -> Vec<Endpoint> {
            self.preferred_calls.fetch_add(1, Ordering::SeqCst);
            let mut v = endpoints.to_vec();
            v.reverse();
            v
        }

        fn on_direct_connect_result(&self, _endpoint: &Endpoint, _success: bool) {
            self.direct_result_calls.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn make_tunnel(
        factory: Arc<EmptyConnectionFactory>,
        remote_eps: Vec<Endpoint>,
    ) -> (Tunnel<EmptyConnectionFactory>, P2pConnectionInfoCacheRef) {
        Executor::init();
        let local_identity: P2pIdentityRef = Arc::new(DummyIdentity {
            id: P2pId::from(vec![1u8; 32]),
            name: "dummy-local".to_owned(),
            endpoints: vec![],
        });
        let cert_factory: P2pIdentityCertFactoryRef = Arc::new(DummyIdentityCertFactory);
        let conn_info_cache = DefaultP2pConnectionInfoCache::new();
        let cert_resolver = DefaultTlsServerCertResolver::new();
        let net_manager =
            Arc::new(NetManager::new(vec![], cert_resolver, conn_info_cache.clone()).unwrap());
        let sn_service = SNClientService::new(
            net_manager,
            vec![],
            local_identity.clone(),
            Arc::new(SequenceGenerator::new()),
            Arc::new(TunnelIdGenerator::new()),
            cert_factory.clone(),
            1,
            Duration::from_millis(50),
            Duration::from_millis(50),
            Duration::from_millis(50),
        );

        (
            Tunnel::new(
                sn_service,
                100u32.into(),
                0,
                P2pId::from(vec![2u8; 32]),
                remote_eps,
                Some("remote".to_owned()),
                local_identity,
                Duration::from_millis(50),
                Duration::from_secs(1),
                cert_factory,
                None,
                factory,
                conn_info_cache.clone(),
            ),
            conn_info_cache,
        )
    }

    #[test]
    fn session_sn_call_raw_codec_roundtrip() {
        let call = SessionSnCall {
            vport: 10001,
            session_id: Default::default(),
        };

        let encoded = call.to_vec().unwrap();
        let decoded = SessionSnCall::clone_from_slice(encoded.as_slice()).unwrap();
        assert_eq!(decoded.vport, call.vport);
        assert_eq!(decoded.session_id, call.session_id);
    }

    #[tokio::test]
    async fn open_session_without_connection_returns_connect_failed() {
        let factory = EmptyConnectionFactory::new();
        let (tunnel, _) = make_tunnel(factory, vec![]);

        let err = tunnel
            .open_session(10001, Default::default())
            .await
            .err()
            .unwrap();
        assert_eq!(err.code(), P2pErrorCode::ConnectFailed);
        assert!(err.msg().contains("connect has not been established"));
    }

    #[tokio::test]
    async fn open_reverse_session_without_connection_returns_connect_failed() {
        let factory = EmptyConnectionFactory::new();
        let (tunnel, _) = make_tunnel(factory, vec![]);

        let err = tunnel
            .open_reverse_session(10001, Default::default(), 0)
            .await
            .err()
            .unwrap();
        assert_eq!(err.code(), P2pErrorCode::ConnectFailed);
        assert!(err.msg().contains("connect has not been established"));
    }

    #[tokio::test]
    async fn connect_session_direct_without_endpoints_returns_connect_failed() {
        let factory = EmptyConnectionFactory::new();
        let (mut tunnel, _) = make_tunnel(factory.clone(), vec![]);

        let err = tunnel
            .connect_session_direct(10001, Default::default())
            .await
            .err()
            .unwrap();
        assert_eq!(err.code(), P2pErrorCode::ConnectFailed);
        assert!(err.msg().contains("No available endpoint"));
        assert_eq!(factory.call_count(), 0);
    }

    #[tokio::test]
    async fn connect_session_direct_with_empty_factory_result_returns_connect_failed() {
        let factory = EmptyConnectionFactory::new();
        let (mut tunnel, _) = make_tunnel(factory.clone(), vec![test_endpoint(18080)]);

        let err = tunnel
            .connect_session_direct(10001, Default::default())
            .await
            .err()
            .unwrap();
        assert_eq!(err.code(), P2pErrorCode::ConnectFailed);
        assert!(err.msg().contains("connect session"));
        assert_eq!(factory.call_count(), 1);
    }

    #[tokio::test]
    async fn connect_reverse_session_without_endpoints_returns_connect_failed() {
        let factory = EmptyConnectionFactory::new();
        let (mut tunnel, _) = make_tunnel(factory, vec![]);

        let err = tunnel
            .connect_reverse_session(10001, Default::default(), 0)
            .await
            .err()
            .unwrap();
        assert_eq!(err.code(), P2pErrorCode::ConnectFailed);
        assert!(err.msg().contains("No available endpoint"));
    }

    #[tokio::test]
    async fn connect_reverse_session_with_empty_factory_result_returns_connect_failed() {
        let factory = EmptyConnectionFactory::new();
        let (mut tunnel, _) = make_tunnel(factory, vec![test_endpoint(19090)]);

        let err = tunnel
            .connect_reverse_session(10001, Default::default(), 0)
            .await
            .err()
            .unwrap();
        assert_eq!(err.code(), P2pErrorCode::ConnectFailed);
        assert!(err.msg().contains("No available endpoint"));
    }

    #[tokio::test]
    async fn connect_session_uses_preferred_direct_endpoints_and_records_failures() {
        let factory = EmptyConnectionFactory::new();
        let (mut tunnel, _) = make_tunnel(
            factory.clone(),
            vec![test_wan_endpoint(20001), test_wan_endpoint(20002)],
        );
        let cache = MockReverseWaiterCache::new();

        let err = tunnel
            .connect_session(10001, Default::default(), cache.clone())
            .await
            .err()
            .unwrap();

        assert_eq!(err.code(), P2pErrorCode::ConnectFailed);
        assert_eq!(cache.preferred_calls(), 1);
        assert!(cache.direct_result_calls() >= 2);
        assert_eq!(factory.call_count(), 2);
    }

    #[tokio::test]
    async fn connect_session_cache_reverse_failure_does_not_retry_reverse_fallback() {
        let factory = EmptyConnectionFactory::new();
        let (mut tunnel, conn_info_cache) = make_tunnel(factory, vec![]);
        let remote_id = P2pId::from(vec![2u8; 32]);
        conn_info_cache
            .add(
                remote_id,
                P2pConnectionInfo {
                    direct: ConnectDirection::Reverse,
                    local_ep: Endpoint::default(),
                    remote_ep: Endpoint::default(),
                },
            )
            .await;

        let cache = MockReverseWaiterCache::new();
        let err = tunnel
            .connect_session(10001, Default::default(), cache.clone())
            .await
            .err()
            .unwrap();

        assert_eq!(err.code(), P2pErrorCode::ConnectFailed);
        assert_eq!(cache.reverse_add_calls(), 1);
        assert_eq!(cache.reverse_remove_calls(), 1);
    }

    fn make_scripted_tunnel(
        factory: Arc<ScriptedConnectionFactory>,
        remote_eps: Vec<Endpoint>,
    ) -> (Tunnel<ScriptedConnectionFactory>, P2pConnectionInfoCacheRef) {
        Executor::init();
        let local_identity: P2pIdentityRef = Arc::new(DummyIdentity {
            id: P2pId::from(vec![1u8; 32]),
            name: "dummy-local".to_owned(),
            endpoints: vec![],
        });
        let cert_factory: P2pIdentityCertFactoryRef = Arc::new(DummyIdentityCertFactory);
        let conn_info_cache = DefaultP2pConnectionInfoCache::new();
        let cert_resolver = DefaultTlsServerCertResolver::new();
        let net_manager =
            Arc::new(NetManager::new(vec![], cert_resolver, conn_info_cache.clone()).unwrap());
        let sn_service = SNClientService::new(
            net_manager,
            vec![],
            local_identity.clone(),
            Arc::new(SequenceGenerator::new()),
            Arc::new(TunnelIdGenerator::new()),
            cert_factory.clone(),
            1,
            Duration::from_millis(50),
            Duration::from_millis(50),
            Duration::from_millis(50),
        );

        (
            Tunnel::new(
                sn_service,
                101u32.into(),
                0,
                P2pId::from(vec![3u8; 32]),
                remote_eps,
                Some("remote-scripted".to_owned()),
                local_identity,
                Duration::from_millis(80),
                Duration::from_secs(1),
                cert_factory,
                None,
                factory,
                conn_info_cache.clone(),
            ),
            conn_info_cache,
        )
    }

    #[tokio::test]
    async fn connect_session_direct_success_updates_cache_to_direct() {
        let factory = ScriptedConnectionFactory::new(0);
        let remote_ep = test_wan_endpoint(21001);
        let (mut tunnel, conn_info_cache) = make_scripted_tunnel(factory.clone(), vec![remote_ep]);
        let cache = MockReverseWaiterCache::new();

        let ret = tunnel
            .connect_session(10011, Default::default(), cache.clone())
            .await;
        assert!(ret.is_ok());

        let info = conn_info_cache.get(&P2pId::from(vec![3u8; 32])).await;
        assert!(info.is_some());
        let info = info.unwrap();
        assert_eq!(info.direct, ConnectDirection::Direct);
        assert_eq!(info.remote_ep, remote_ep);
        assert_eq!(factory.call_count(), 1);
        assert_eq!(cache.preferred_calls(), 1);
        assert!(cache.direct_result_calls() >= 1);
    }

    #[tokio::test]
    async fn connect_session_cached_direct_success_skips_preferred_sort() {
        let factory = ScriptedConnectionFactory::new(0);
        let remote_ep = test_wan_endpoint(22001);
        let (mut tunnel, conn_info_cache) = make_scripted_tunnel(factory.clone(), vec![remote_ep]);
        conn_info_cache
            .add(
                P2pId::from(vec![3u8; 32]),
                P2pConnectionInfo {
                    direct: ConnectDirection::Direct,
                    local_ep: Endpoint::default(),
                    remote_ep,
                },
            )
            .await;
        let cache = MockReverseWaiterCache::new();

        let ret = tunnel
            .connect_session(10012, Default::default(), cache.clone())
            .await;
        assert!(ret.is_ok());
        assert_eq!(factory.call_count(), 1);
        assert_eq!(cache.preferred_calls(), 0);
        assert_eq!(cache.direct_result_calls(), 1);
    }

    #[tokio::test]
    async fn connect_session_reuse_existing_tunnel_conn_success() {
        let factory = ScriptedConnectionFactory::new(0);
        let (mut tunnel, _) = make_scripted_tunnel(factory.clone(), vec![]);
        let tunnel_conn = TunnelConnection::new(
            tunnel.tunnel_id,
            tunnel.local_identity.clone(),
            tunnel.remote_id.clone(),
            test_wan_endpoint(23001),
            Duration::from_millis(80),
            tunnel.protocol_version,
            mock_p2p_connection(
                tunnel.local_identity.get_id(),
                tunnel.remote_id.clone(),
                test_wan_endpoint(23001),
                0,
            ),
            tunnel.cert_factory.clone(),
        );
        tunnel.set_tunnel_conn(tunnel_conn);

        let cache = MockReverseWaiterCache::new();
        let ret = tunnel
            .connect_session(10013, Default::default(), cache.clone())
            .await;
        assert!(ret.is_ok());
        assert_eq!(factory.call_count(), 0);
        assert_eq!(cache.preferred_calls(), 0);
        assert_eq!(cache.direct_result_calls(), 0);
    }

    #[tokio::test]
    async fn connect_session_reuse_existing_tunnel_conn_ack_error() {
        let factory = ScriptedConnectionFactory::new(0);
        let (mut tunnel, _) = make_scripted_tunnel(factory.clone(), vec![]);
        let tunnel_conn = TunnelConnection::new(
            tunnel.tunnel_id,
            tunnel.local_identity.clone(),
            tunnel.remote_id.clone(),
            test_wan_endpoint(24001),
            Duration::from_millis(80),
            tunnel.protocol_version,
            mock_p2p_connection(
                tunnel.local_identity.get_id(),
                tunnel.remote_id.clone(),
                test_wan_endpoint(24001),
                P2pErrorCode::Timeout.as_u8(),
            ),
            tunnel.cert_factory.clone(),
        );
        tunnel.set_tunnel_conn(tunnel_conn);

        let cache = MockReverseWaiterCache::new();
        let err = tunnel
            .connect_session(10014, Default::default(), cache)
            .await
            .err()
            .unwrap();
        assert_eq!(err.code(), P2pErrorCode::Timeout);
        assert_eq!(factory.call_count(), 0);
    }
}
