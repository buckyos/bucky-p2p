use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration};
use bucky_raw_codec::{RawFrom};
use bucky_time::bucky_time_now;
use notify_future::NotifyFuture;
use quinn::crypto::rustls::QuicClientConfig;
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
use rustls::version::TLS13;
use crate::endpoint::{Endpoint, Protocol};
use crate::error::{bdt_err, into_bdt_err, P2pErrorCode, P2pResult};
use crate::runtime;
use crate::executor::{Executor, SpawnHandle};
use crate::p2p_identity::{P2pId, P2pIdentityRef, EncodedP2pIdentityCert, P2pIdentityCertFactoryRef, P2pIdentityCertRef};
use crate::protocol::{Package, PackageCmdCode, ReportSn, ReportSnResp, SnCall, SnQuery, SnQueryResp};
use crate::protocol::v0::{SnCallResp, SnCalled, SnCalledResp};
use crate::sn::service::PeerConnection;
use crate::sockets::{NetManagerRef, QuicSocket};
use crate::types::{TempSeq, TempSeqGenerator};

#[callback_trait::callback_trait]
pub trait SNEvent: 'static + Send + Sync {
    async fn on_called(&self, called: SnCalled) -> P2pResult<()>;
}
pub type SNEventRef = Arc<dyn SNEvent>;

pub enum SnResp {
    SnCallResp(SnCallResp),
    SnQueryResp(SnQueryResp),
}

#[derive(Clone)]
pub struct ActiveSN {
    pub sn: EncodedP2pIdentityCert,
    pub latest_time: u64,
    pub conn_id: TempSeq,
    pub recv_future: Arc<Mutex<HashMap<TempSeq, NotifyFuture<SnResp>>>>,
    pub peer_connection: Arc<runtime::Mutex<PeerConnection>>,
    pub wan_ep_list: Vec<Endpoint>,
}

pub struct SNServiceState {
    pub pinging_handle: Option<SpawnHandle<()>>,
    pub active_sn_list: Vec<ActiveSN>,
    pub latest_sn_interval: u64,
    pub cur_report_future: Option<NotifyFuture<ReportSnResp>>,
}

pub struct SNClientService {
    net_manager: NetManagerRef,
    sn_list: Vec<P2pIdentityCertRef>,
    local_identity: P2pIdentityRef,
    gen_seq: Arc<TempSeqGenerator>,
    ping_timeout: Duration,
    call_timeout: Duration,
    conn_timeout: Duration,
    state: RwLock<SNServiceState>,
    listener: Mutex<Option<SNEventRef>>,
    cert_factory: P2pIdentityCertFactoryRef,

}
pub type SNClientServiceRef = Arc<SNClientService>;

impl Drop for SNClientService {
    fn drop(&mut self) {
        log::info!("SNClientService drop.device = {}", self.local_identity.get_id());
    }

}

impl SNClientService {
    pub fn new(net_manager: NetManagerRef,
               sn_list: Vec<P2pIdentityCertRef>,
               local_identity: P2pIdentityRef,
               gen_seq: Arc<TempSeqGenerator>,
               cert_factory: P2pIdentityCertFactoryRef,
               ping_timeout: Duration,
               call_timeout: Duration,
               conn_timeout: Duration,) -> Arc<Self> {
        Arc::new(Self {
            net_manager,
            sn_list,
            local_identity,
            gen_seq,
            ping_timeout,
            call_timeout,
            conn_timeout,
            state: RwLock::new(SNServiceState {
                pinging_handle: None,
                active_sn_list: vec![],
                latest_sn_interval: 0,
                cur_report_future: None,
            }),
            listener: Mutex::new(None),
            cert_factory,
        })
    }

    pub fn set_listener(&self, listener: impl SNEvent) {
        let mut _listener = self.listener.lock().unwrap();
        *_listener = Some(Arc::new(listener));
    }

    pub fn get_wan_ip_list(&self) -> Vec<Endpoint> {
        let mut wan_list = Vec::new();
        self.get_active_sn_list().iter().map(|v| v.wan_ep_list.as_slice()).flatten().for_each(|ep| {
            wan_list.push(ep.clone());
        });
        wan_list
    }

    pub fn is_same_lan(&self, reverse_list: &Vec<Endpoint>) -> bool {
        let local_wan_list = self.get_wan_ip_list();
        for ep in reverse_list.iter() {
            for wan_ip in local_wan_list.iter() {
                if ep.is_same_ip_addr(wan_ip) {
                    return true;
                }
            }
        }
        return false;
    }

    async fn handle(&self, conn_id: TempSeq, cmd_code: PackageCmdCode, cmd_body: Vec<u8>) -> P2pResult<()> {
        match cmd_code {
            PackageCmdCode::SnCallResp => {
                let resp = SnCallResp::clone_from_slice(cmd_body.as_slice()).map_err(into_bdt_err!(P2pErrorCode::RawCodecError))?;
                let mut state = self.state.write().unwrap();
                for active_sn in state.active_sn_list.iter_mut() {
                    if active_sn.conn_id == conn_id {
                        let mut recv_futures = active_sn.recv_future.lock().unwrap();
                        if let Some(recv_future) = recv_futures.remove(&resp.seq) {
                            recv_future.set_complete(SnResp::SnCallResp(resp));
                        }
                        break;
                    }
                }
            },
            PackageCmdCode::SnQueryResp => {
                let resp = SnQueryResp::clone_from_slice(cmd_body.as_slice()).map_err(into_bdt_err!(P2pErrorCode::RawCodecError))?;
                let mut state = self.state.write().unwrap();
                for active_sn in state.active_sn_list.iter_mut() {
                    if active_sn.conn_id == conn_id {
                        let mut recv_futures = active_sn.recv_future.lock().unwrap();
                        if let Some(recv_future) = recv_futures.remove(&resp.seq) {
                            recv_future.set_complete(SnResp::SnQueryResp(resp));
                        }
                        break;
                    }
                }
            },
            PackageCmdCode::SnCalled => {
                let sn_called = SnCalled::clone_from_slice(cmd_body.as_slice()).map_err(into_bdt_err!(P2pErrorCode::RawCodecError))?;
                self.on_called(conn_id, sn_called).await?;
            },
            PackageCmdCode::ReportSnResp => {
                let resp = ReportSnResp::clone_from_slice(cmd_body.as_slice()).map_err(into_bdt_err!(P2pErrorCode::RawCodecError))?;
                log::info!("report sn resp: {:?}", resp);
                if let Some(cur_report_future) = {
                    let mut state = self.state.write().unwrap();
                    state.cur_report_future.take()
                } {
                    cur_report_future.set_complete(resp);
                }
            },
            _ => warn!("invalid cmd-package, conn: {:?} cmd_code {:?}.", conn_id, cmd_code),
        }
        Ok(())
    }

    async fn on_called(&self, conn_id: TempSeq, sn_called: SnCalled) -> P2pResult<()> {
        let listener = {
            let listener = self.listener.lock().unwrap();
            listener.clone()
        };
        let seq = sn_called.tunnel_id.clone();
        let sn_peer_id = sn_called.sn_peer_id.clone();
        let to_peer_id = sn_called.to_peer_id.clone();

        let resp = if to_peer_id == self.local_identity.get_id() {
            if listener.is_some() {
                match listener.as_ref().unwrap().on_called(sn_called).await {
                    Ok(_) => {
                        SnCalledResp {
                            seq,
                            sn_peer_id,
                            result: 0,
                        }
                    }
                    Err(e) => {
                        log::info!("on called to {} failed: {:?}", to_peer_id, e);
                        SnCalledResp {
                            seq,
                            sn_peer_id,
                            result: e.code().into_u8(),
                        }
                    }
                }
            } else {
                SnCalledResp {
                    seq,
                    sn_peer_id,
                    result: 0,
                }
            }
        } else {
            SnCalledResp {
                seq,
                sn_peer_id,
                result: P2pErrorCode::TargetNotFound.into_u8(),
            }
        };

        let peer_conn = self.get_peer_connection(conn_id);
        if let Some(peer_conn) = peer_conn {
            peer_conn.lock().await.send(Package::new(PackageCmdCode::SnCalledResp, resp)).await?;
        }
        Ok(())
    }

    fn get_peer_connection(&self, conn_id: TempSeq) -> Option<Arc<runtime::Mutex<PeerConnection>>> {
        let state = self.state.read().unwrap();
        for active_sn in state.active_sn_list.iter() {
            if active_sn.conn_id == conn_id {
                return Some(active_sn.peer_connection.clone());
            }
        }
        None
    }

    pub async fn start(self: &Arc<Self>) -> P2pResult<()> {
        let this = self.clone();
        let handle = Executor::spawn_with_handle(async move {
            this.ping_proc().await;
        }).map_err(into_bdt_err!(P2pErrorCode::Failed, "start sn ping proc failed"))?;
        {
            let mut state = self.state.write().unwrap();
            state.pinging_handle = Some(handle);
        }
        Ok(())
    }

    pub async fn stop(&self) {
        {
            let state = self.state.read().unwrap();
            if state.pinging_handle.is_some() {
                state.pinging_handle.as_ref().unwrap().abort();
            }
            for active_sn in state.active_sn_list.iter() {
                let mut peer_conn = active_sn.peer_connection.lock().await;
                peer_conn.shutdown().await;
            }
        }
    }

    async fn ping_proc(self: &Arc<Self>) {
        loop {
            {
                let (active_sn_count, latest_sn_interval, cur_sn_interval) = {
                    let mut state = self.state.write().unwrap();
                    if state.active_sn_list.len() > 0 {
                        state.latest_sn_interval = 10;
                        (state.active_sn_list.len(), state.latest_sn_interval, state.latest_sn_interval)
                    } else {
                        let cur_sn_interval = state.latest_sn_interval;
                        if state.latest_sn_interval == 0 {
                            state.latest_sn_interval = 1;
                        } else if state.latest_sn_interval == 10 {
                            state.latest_sn_interval = 1;
                        } else {
                            state.latest_sn_interval = state.latest_sn_interval * 2;
                        }
                        if state.latest_sn_interval > 600 {
                            state.latest_sn_interval = 600;
                        }
                        (state.active_sn_list.len(), cur_sn_interval, state.latest_sn_interval)
                    }
                };
                if latest_sn_interval != 0 {
                    runtime::sleep(Duration::from_secs(cur_sn_interval)).await;
                }
                if active_sn_count > 0 {
                    let mut ping_sn_list = Vec::new();
                    {
                        let mut state = self.state.write().unwrap();
                        for active_sn in state.active_sn_list.iter_mut() {
                            if bucky_time_now() - active_sn.latest_time > 600 * 1000 * 1000 {
                                active_sn.latest_time = bucky_time_now();
                                ping_sn_list.push(active_sn.clone());
                            }
                        }
                    }

                    for active_sn in ping_sn_list.iter() {
                        let mut peer_conn = active_sn.peer_connection.lock().await;
                        match self.report(&mut peer_conn).await {
                            Ok(_) => {}
                            Err(e) => {
                                if let Ok(sn) = self.cert_factory.create(&active_sn.sn) {
                                    log::error!("ping to {} failed: {:?}", sn.get_id(), e);
                                } else {
                                    log::error!("ping failed: {:?}", e);
                                }
                                continue;
                            }
                        }
                    }
                    continue;
                }
            }
            for listener in self.net_manager.quic_listeners().iter() {
                let quic_ep = listener.quic_ep();
                for sn_cert in self.sn_list.iter() {
                    for sn_ep in sn_cert.endpoints().iter() {
                        let mut peer_conn = match self.create_connection(listener.local(), quic_ep.clone(), &sn_cert, sn_ep).await {
                            Ok(peer_conn) => peer_conn,
                            Err(e) => {
                                log::error!("connect to sn {} failed: {:?}", sn_cert.get_id(), e);
                                continue;
                            }
                        };
                        let report_resp = match self.report(&mut peer_conn).await {
                            Ok(resp) => {
                                resp
                            }
                            Err(e) => {
                                log::error!("ping to {} failed: {:?}", sn_cert.get_id(), e);
                                continue;
                            }
                        };

                        let sn = match sn_cert.get_encoded_cert() {
                            Ok(resp) => resp,
                            Err(e) => {
                                log::error!("ping to {} failed: {:?}", sn_cert.get_id(), e);
                                continue;
                            }
                        };

                        let recv_handle = peer_conn.take_recv_handle().unwrap();
                        let conn_id = peer_conn.conn_id();
                        let active_sn = ActiveSN {
                            sn,
                            latest_time: bucky_time_now(),
                            conn_id: peer_conn.conn_id(),
                            recv_future: Arc::new(Mutex::new(HashMap::new())),
                            peer_connection: Arc::new(runtime::Mutex::new(peer_conn)),
                            wan_ep_list: report_resp.end_point_array,
                        };
                        let mut state = self.state.write().unwrap();
                        state.active_sn_list.push(active_sn);

                        let this = self.clone();
                        Executor::spawn_ok(async move {
                            recv_handle.await;
                            this.remote_sn_conn(conn_id);
                        });
                    }
                }
            }
        }
    }

    fn remote_sn_conn(&self, conn_id: TempSeq) {
        let mut state = self.state.write().unwrap();
        state.active_sn_list.retain(|sn| sn.conn_id != conn_id);
    }

    async fn create_connection(self: &Arc<Self>, local_ep: Endpoint, quic_ep: quinn::Endpoint, sn: &P2pIdentityCertRef, sn_ep: &Endpoint) -> P2pResult<PeerConnection> {
        log::info!("connect to sn: {} sn_ep: {}", sn.get_id(), sn_ep);
        let client_key = self.local_identity.get_encoded_identity()?;
        let client_cert = self.local_identity.get_identity_cert()?.get_encoded_cert()?;
        let mut config =
            rustls::ClientConfig::builder_with_provider(crate::tls::provider().into())
                .with_protocol_versions(&[&TLS13])
                .unwrap()
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(crate::tls::TlsServerCertVerifier::new(self.cert_factory.clone())))
                .with_client_auth_cert(vec![CertificateDer::from(client_cert)], PrivatePkcs8KeyDer::from(client_key).into())
                .map_err(into_bdt_err!(P2pErrorCode::TlsError))?;
        config.enable_early_data = true;

        let mut client_config =
            quinn::ClientConfig::new(Arc::new(QuicClientConfig::try_from(config).unwrap()));
        let mut transport_config = quinn::TransportConfig::default();
        transport_config.max_idle_timeout(Some(std::time::Duration::from_secs(120).try_into().unwrap()));
        transport_config.keep_alive_interval(Some(Duration::from_secs(30)));
        client_config.transport_config(Arc::new(transport_config));

        let conning = quic_ep.connect_with(client_config, sn_ep.addr().clone(), sn.get_id().to_string().as_str())
            .map_err(into_bdt_err!(P2pErrorCode::ConnectFailed, "connect to sn failed"))?;

        let conn = conning.await.map_err(into_bdt_err!(P2pErrorCode::ConnectFailed, "connect to sn failed"))?;
        let quic_socket = QuicSocket::new(conn, self.local_identity.get_id(), sn.get_id(), local_ep, sn_ep.clone());
        let conn_id = self.gen_seq.generate();
        let this = self.clone();
        let peer_conn = PeerConnection::connect(conn_id, quic_socket, move |conn_id: TempSeq, cmd_code: PackageCmdCode, cmd_body: Vec<u8>| {
            let this = this.clone();
            async move {
                if let Err(e) = this.handle(conn_id, cmd_code, cmd_body).await {
                    log::error!("handle cmd {:?} error: {:?}", cmd_code, e);
                    Err(e)
                } else {
                    Ok(())
                }
            }
        }).await.map_err(into_bdt_err!(P2pErrorCode::ConnectFailed, "connect to sn failed"))?;
        Ok(peer_conn)
    }

    pub async fn wait_online(&self, _timeout: Option<Duration>) -> P2pResult<()> {
        loop {
            {
                let state = self.state.read().unwrap();
                if state.active_sn_list.len() > 0 {
                    break;
                }
            }
            runtime::sleep(Duration::from_secs(1)).await;
        }
        Ok(())
    }

    pub fn get_active_sn_list(&self) -> Vec<ActiveSN> {
        let state = self.state.read().unwrap();
        state.active_sn_list.clone()
    }

    async fn report(&self, conn: &mut PeerConnection) -> P2pResult<ReportSnResp> {
        let seq = self.gen_seq.generate();
        let local_ips = if_addrs::get_if_addrs().map(|addrs| {
            addrs.iter().filter(|addr| !addr.name.contains("VMware")
                && !addr.name.contains("VirtualBox")
                && !addr.name.contains("ZeroTier")
                && !addr.name.contains("Tun")
                && !addr.name.contains("docker")
                && !addr.name.contains("lo")
                && !addr.name.contains("veth")
                && !addr.name.contains("V-M")
                && !addr.name.contains("br-")
                && !addr.name.contains("vEthernet")
                && addr.ip().to_string() != "127.0.0.1")
                .map(|addr| addr.addr.ip().to_string()).collect::<Vec<String>>()
        }).unwrap_or(Vec::new());

        let mut local_eps = Vec::new();
        let mut tcp_map_port = None;
        for listener in self.net_manager.tcp_listeners().iter() {
            tcp_map_port = listener.mapping_port();
            if listener.local().addr().ip().is_unspecified() {
                for ip in local_ips.iter() {
                    match ip.parse::<IpAddr>() {
                        Ok(ip) => {
                            local_eps.push(Endpoint::from((Protocol::Tcp, ip, listener.local().addr().port())));
                        }
                        Err(_) => {
                            continue;
                        }
                    }
                }
            } else {
                local_eps.push(listener.local());
            }
        }

        let mut udp_map_port = None;
        for listener in self.net_manager.quic_listeners().iter() {
            udp_map_port = listener.mapping_port();
            if listener.local().addr().ip().is_unspecified() {
                for ip in local_ips.iter() {
                    match ip.parse::<IpAddr>() {
                        Ok(ip) => {
                            local_eps.push(Endpoint::from((Protocol::Udp, ip, listener.local().addr().port())));
                        }
                        Err(_) => {
                            continue;
                        }
                    }
                }
            } else {
                local_eps.push(listener.local());
            }
        }

        let report = ReportSn {
            protocol_version: 0,
            stack_version: 0,
            seq,
            sn_peer_id: conn.remote_identity_id().clone(),
            from_peer_id: Some(self.local_identity.get_id()),
            peer_info: Some(self.local_identity.get_identity_cert()?.get_encoded_cert()?),
            send_time: bucky_time_now(),
            contract_id: None,
            receipt: None,
            tcp_map_port,
            udp_map_port,
            local_eps,
        };
        let future = NotifyFuture::<ReportSnResp>::new();
        {
            let mut state = self.state.write().unwrap();
            state.cur_report_future = Some(future.clone());
        }
        conn.send(Package::new(PackageCmdCode::ReportSn, report)).await?;
        let resp = runtime::timeout(self.call_timeout, future).await.map_err(into_bdt_err!(P2pErrorCode::ConnectFailed, "report timeout"))?;
        {
            let mut state = self.state.write().unwrap();
            state.cur_report_future = None;
        }
        Ok(resp)
    }

    pub async fn call(&self,
                      tunnel_id: TempSeq,
                      reverse_endpoints: Option<&[Endpoint]>,
                      remote: &P2pId,
                      payload_pkg: Vec<u8>) -> P2pResult<SnCallResp> {
        let active_list = self.get_active_sn_list();
        for active in active_list.iter() {
            let sn = self.cert_factory.create(&active.sn)?;
            let seq = self.gen_seq.generate();
            let call = SnCall {
                protocol_version: 0,
                stack_version: 0,
                seq,
                tunnel_id,
                sn_peer_id: sn.get_id(),
                to_peer_id: remote.clone(),
                from_peer_id: self.local_identity.get_id().clone(),
                reverse_endpoint_array: reverse_endpoints.map(|ep_list| Vec::from(ep_list)),
                active_pn_list: None,
                peer_info: Some(self.local_identity.get_identity_cert()?.get_encoded_cert()?),
                send_time: bucky_time_now(),
                payload: payload_pkg.clone(),
                is_always_call: false,
            };
            let mut peer_conn = active.peer_connection.lock().await;
            let future = NotifyFuture::<SnResp>::new();
            {
                let mut recv_future = active.recv_future.lock().unwrap();
                recv_future.insert(seq, future.clone());
            }
            if let Err(e) = peer_conn.send(Package::new(PackageCmdCode::SnCall, call)).await {
                log::error!("send call to {} failed: {:?}", sn.get_id(), e);
                continue;
            }

            let resp = match runtime::timeout(self.call_timeout, future).await {
                Ok(resp) => {
                    match resp {
                        SnResp::SnCallResp(resp) => resp,
                        SnResp::SnQueryResp(_) => {
                            log::error!("unexpect resp");
                            continue;
                        }
                    }
                },
                Err(_) => {
                    let mut recv_future = active.recv_future.lock().unwrap();
                    recv_future.remove(&seq);
                    log::error!("call to {} timeout", sn.get_id());
                    continue;
                }
            };

            return Ok(resp);
        }
        Err(bdt_err!(P2pErrorCode::ConnectFailed, "call timeout"))
    }

    pub async fn query(&self, device_id: &P2pId) -> P2pResult<SnQueryResp> {
        let active_list = self.get_active_sn_list();
        for active in active_list.iter() {
            let sn = self.cert_factory.create(&active.sn)?;
            let seq = self.gen_seq.generate();
            let query = SnQuery {
                protocol_version: 0,
                stack_version: 0,
                seq,
                query_id: device_id.clone(),
            };
            let mut peer_conn = active.peer_connection.lock().await;
            let future = NotifyFuture::<SnResp>::new();
            {
                let mut recv_future = active.recv_future.lock().unwrap();
                recv_future.insert(seq, future.clone());
            }
            if let Err(e) = peer_conn.send(Package::new(PackageCmdCode::SnQuery, query)).await {
                log::error!("send call to {} failed: {:?}", sn.get_id(), e);
                continue;
            }

            let resp = match runtime::timeout(self.call_timeout, future).await {
                Ok(resp) => {
                    match resp {
                        SnResp::SnQueryResp(resp) => resp,
                        SnResp::SnCallResp(_) => {
                            log::error!("unexpect resp");
                            continue;
                        }
                    }
                },
                Err(_) => {
                    let mut recv_future = active.recv_future.lock().unwrap();
                    recv_future.remove(&seq);
                    log::error!("call to {} timeout", sn.get_id());
                    continue;
                }
            };

            return Ok(resp);
        }
        Err(bdt_err!(P2pErrorCode::ConnectFailed, "call timeout"))
    }
}
