use std::collections::HashMap;
use std::net::IpAddr;
use std::ops::Add;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration};
use bucky_raw_codec::{RawConvertTo, RawFrom};
use bucky_time::bucky_time_now;
use chrono::Utc;
use notify_future::Notify;
use quinn::crypto::rustls::QuicClientConfig;
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
use rustls::version::TLS13;
use sfo_cmd_server::client::{ClassifiedCmdClient, ClassifiedCmdSend, ClassifiedCmdTunnel, ClassifiedCmdTunnelFactory, CmdClient, DefaultClassifiedCmdClient};
use sfo_cmd_server::errors::{cmd_err, into_cmd_err, CmdErrorCode, CmdResult};
use sfo_cmd_server::{CmdBodyRead, CmdTunnel, PeerId};
use crate::endpoint::{Endpoint, Protocol};
use crate::error::{p2p_err, into_p2p_err, P2pErrorCode, P2pResult};
use crate::runtime;
use crate::executor::{Executor, SpawnHandle};
use crate::p2p_connection::P2pConnection;
use crate::p2p_identity::{P2pId, P2pIdentityRef, EncodedP2pIdentityCert, P2pIdentityCertFactoryRef, P2pIdentityCertRef, P2pSn};
use crate::protocol::{Package, PackageCmdCode, ReportSn, ReportSnResp, SnCall, SnQuery, SnQueryResp};
use crate::protocol::v0::{SnCallResp, SnCalled, SnCalledResp, TunnelType};
use crate::sn::types::{CmdTunnelId, SnCmdHeader, SnTunnelClassification, SnTunnelRead, SnTunnelWrite};
use crate::sockets::{NetManager, NetManagerRef, QuicConnection};
use crate::types::{Sequence, SequenceGenerator, TunnelId, TunnelIdGenerator};

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
    pub sn_peer_id: P2pId,
    pub latest_time: u64,
    pub conn_id: CmdTunnelId,
    pub recv_future: Arc<Mutex<HashMap<Sequence, Notify<SnResp>>>>,
    pub wan_ep_list: Vec<Endpoint>,
}

pub struct SNServiceState {
    pub pinging_handle: Option<SpawnHandle<()>>,
    pub active_sn_list: Vec<ActiveSN>,
    pub latest_sn_interval: u64,
    pub cur_report_future: Option<Notify<ReportSnResp>>,
}

pub struct SnClientTunnelFactory {
    net_manager: NetManagerRef,
    local_identity: P2pIdentityRef,
    sn_list: Vec<P2pSn>,
}

impl SnClientTunnelFactory {
    pub fn new(net_manager: NetManagerRef,
               local_identity: P2pIdentityRef,
               sn_list: Vec<P2pSn>,) -> Self {
        Self {
            net_manager,
            local_identity,
            sn_list,
        }
    }
}

#[async_trait::async_trait]
impl ClassifiedCmdTunnelFactory<SnTunnelClassification, SnTunnelRead, SnTunnelWrite, > for SnClientTunnelFactory {
    async fn create_tunnel(&self, classification: Option<SnTunnelClassification>) -> CmdResult<ClassifiedCmdTunnel<SnTunnelRead, SnTunnelWrite>> {
        if classification.is_some() {
            let classification = classification.unwrap();
            if classification.local_ep.is_some() {
                let local_ep = classification.local_ep.unwrap();
                let p2p_network = self.net_manager.get_network(local_ep.protocol()).map_err(into_cmd_err!(CmdErrorCode::Failed, "get network failed"))?;
                for sn_cert in self.sn_list.iter() {
                    for sn_ep in sn_cert.endpoints().iter() {
                        if sn_ep.protocol() != local_ep.protocol() || sn_ep != &classification.remote_ep {
                            continue;
                        }

                        let conn =  p2p_network.create_stream_connect_with_local_ep(&self.local_identity, &local_ep, sn_ep, &sn_cert.get_id(), Some(sn_cert.get_name())).await
                            .map_err(into_cmd_err!(CmdErrorCode::Failed, "create tunnel failed"))?;
                        let (read, write) = conn.split();
                        return Ok(ClassifiedCmdTunnel::new(SnTunnelRead::new(read), SnTunnelWrite::new(write)));
                    }
                }
            } else {
                for sn_cert in self.sn_list.iter() {
                    for sn_ep in sn_cert.endpoints().iter() {
                        if sn_ep.protocol() != classification.remote_ep.protocol() || sn_ep != &classification.remote_ep {
                            continue;
                        }

                        let p2p_network = self.net_manager.get_network(classification.remote_ep.protocol()).map_err(into_cmd_err!(CmdErrorCode::Failed, "get network failed"))?;
                        let quic_listener = p2p_network.listeners();
                        for listener in quic_listener.iter() {
                            let local_ep = listener.local();
                            let conn =  p2p_network.create_stream_connect_with_local_ep(&self.local_identity, &local_ep, sn_ep, &sn_cert.get_id(), Some(sn_cert.get_name())).await
                                .map_err(into_cmd_err!(CmdErrorCode::Failed, "create tunnel failed"))?;
                            let (read, write) = conn.split();
                            return Ok(ClassifiedCmdTunnel::new(SnTunnelRead::new(read), SnTunnelWrite::new(write)));
                        }
                    }
                }
            }
        } else {
            if let Ok(quic_network) = self.net_manager.get_network(Protocol::Quic) {
                let quic_listener = quic_network.listeners();
                for listener in quic_listener.iter() {
                    let local_ep = listener.local();
                    for sn_cert in self.sn_list.iter() {
                        for sn_ep in sn_cert.endpoints().iter() {
                            if sn_ep.protocol() != Protocol::Quic {
                                continue;
                            }

                            match quic_network.create_stream_connect_with_local_ep(&self.local_identity, &local_ep, sn_ep, &sn_cert.get_id(), Some(sn_cert.get_name())).await {
                                Ok(conn) => {
                                    let (read, write) = conn.split();
                                    return Ok(ClassifiedCmdTunnel::new(SnTunnelRead::new(read), SnTunnelWrite::new(write)));
                                }
                                Err(_) => {
                                    continue;
                                }
                            }
                        }
                    }
                }
            }

            for p2p_network in self.net_manager.get_networks().iter() {
                if p2p_network.protocol() == Protocol::Quic {
                    continue;
                }

                let quic_listener = p2p_network.listeners();
                for listener in quic_listener.iter() {
                    let local_ep = listener.local();
                    for sn_cert in self.sn_list.iter() {
                        for sn_ep in sn_cert.endpoints().iter() {
                            if sn_ep.protocol() == Protocol::Quic || sn_ep.protocol() == Protocol::Tcp {
                                continue;
                            }

                            match p2p_network.create_stream_connect_with_local_ep(&self.local_identity, &local_ep, sn_ep, &sn_cert.get_id(), Some(sn_cert.get_name())).await {
                                Ok(conn) => {
                                    let (read, write) = conn.split();
                                    return Ok(ClassifiedCmdTunnel::new(SnTunnelRead::new(read), SnTunnelWrite::new(write)));
                                }
                                Err(_) => {
                                    continue;
                                }
                            }
                        }
                    }
                }
            }
        }
        Err(cmd_err!(CmdErrorCode::Failed, "create tunnel failed"))
    }
}

pub type SnCmdClient = DefaultClassifiedCmdClient<SnTunnelClassification, SnTunnelRead, SnTunnelWrite, SnClientTunnelFactory, u16, u8>;

pub type SnCmdClientRef = Arc<SnCmdClient>;
pub struct SNClientService {
    net_manager: NetManagerRef,
    sn_list: Vec<P2pSn>,
    local_identity: P2pIdentityRef,
    gen_seq: Arc<SequenceGenerator>,
    gen_id: Arc<TunnelIdGenerator>,
    ping_timeout: Duration,
    call_timeout: Duration,
    conn_timeout: Duration,
    state: RwLock<SNServiceState>,
    listener: Mutex<Option<SNEventRef>>,
    cert_factory: P2pIdentityCertFactoryRef,
    cmd_client: SnCmdClientRef,
    cmd_version: u8,
}
pub type SNClientServiceRef = Arc<SNClientService>;

impl Drop for SNClientService {
    fn drop(&mut self) {
        log::info!("SNClientService drop.device = {}", self.local_identity.get_id());
    }

}

impl SNClientService {
    pub fn new(net_manager: NetManagerRef,
               sn_list: Vec<P2pSn>,
               local_identity: P2pIdentityRef,
               gen_seq: Arc<SequenceGenerator>,
               gen_id: Arc<TunnelIdGenerator>,
               cert_factory: P2pIdentityCertFactoryRef,
               tunnel_count: u16,
               ping_timeout: Duration,
               call_timeout: Duration,
               conn_timeout: Duration,) -> Arc<Self> {
        let cmd_client = DefaultClassifiedCmdClient::new(SnClientTunnelFactory::new(net_manager.clone(), local_identity.clone(), sn_list.clone()), tunnel_count);
        Arc::new(Self {
            net_manager,
            sn_list,
            local_identity,
            gen_seq,
            gen_id,
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
            cmd_client,
            cmd_version: 0,
        })
    }

    pub fn set_listener(&self, listener: impl SNEvent) {
        let mut _listener = self.listener.lock().unwrap();
        *_listener = Some(Arc::new(listener));
    }

    pub fn get_cmd_client(&self) -> &SnCmdClientRef {
        &self.cmd_client
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

    fn register_cmd_handler(self: &Arc<Self>) {
        let this = self.clone();
        self.cmd_client.register_cmd_handler(PackageCmdCode::SnCallResp as u8, move |peer_id: PeerId, tunnel_id: CmdTunnelId, header: SnCmdHeader, mut body: CmdBodyRead| {
            let this = this.clone();
            async move {
                let resp = SnCallResp::clone_from_slice(body.read_all().await?.as_slice()).map_err(into_cmd_err!(CmdErrorCode::RawCodecError))?;
                this.sn_call_resp_handle(tunnel_id, resp).await.map_err(into_cmd_err!(CmdErrorCode::Failed, "sn call resp handle failed"))?;
                Ok(())
            }
        });

        let this = self.clone();
        self.cmd_client.register_cmd_handler(PackageCmdCode::SnQueryResp as u8, move |peer_id: PeerId, tunnel_id: CmdTunnelId, header: SnCmdHeader, mut body: CmdBodyRead| {
            let this = this.clone();
            async move {
                let resp = SnQueryResp::clone_from_slice(body.read_all().await?.as_slice()).map_err(into_cmd_err!(CmdErrorCode::RawCodecError))?;
                this.sn_query_resp_handle(tunnel_id, resp).await.map_err(into_cmd_err!(CmdErrorCode::Failed, "sn query resp handle failed"))?;
                Ok(())
            }
        });

        let this = self.clone();
        self.cmd_client.register_cmd_handler(PackageCmdCode::SnCalled as u8, move |peer_id: PeerId, tunnel_id: CmdTunnelId, header: SnCmdHeader, mut body: CmdBodyRead| {
            let this = this.clone();
            async move {
                let sn_called = SnCalled::clone_from_slice(body.read_all().await?.as_slice()).map_err(into_cmd_err!(CmdErrorCode::RawCodecError))?;
                this.on_called(tunnel_id, sn_called).await.map_err(into_cmd_err!(CmdErrorCode::Failed, "sn called handle failed"))?;
                Ok(())
            }
        });

        let this = self.clone();
        self.cmd_client.register_cmd_handler(PackageCmdCode::ReportSnResp as u8, move |peer_id: PeerId, tunnel_id: CmdTunnelId, header: SnCmdHeader, mut body: CmdBodyRead| {
            let this = this.clone();
            async move {
                let resp = ReportSnResp::clone_from_slice(body.read_all().await?.as_slice()).map_err(into_cmd_err!(CmdErrorCode::RawCodecError))?;
                this.report_sn_resp_handle(tunnel_id, resp).await.map_err(into_cmd_err!(CmdErrorCode::Failed, "report sn resp handle failed"))?;
                Ok(())
            }
        });
    }

    async fn sn_call_resp_handle(&self, conn_id: CmdTunnelId, resp: SnCallResp) -> P2pResult<()> {
        let mut state = self.state.write().unwrap();
        for active_sn in state.active_sn_list.iter_mut() {
            if active_sn.conn_id == conn_id {
                let mut recv_futures = active_sn.recv_future.lock().unwrap();
                if let Some(recv_future) = recv_futures.remove(&resp.seq) {
                    recv_future.notify(SnResp::SnCallResp(resp));
                }
                break;
            }
        }
        Ok(())
    }

    async fn sn_query_resp_handle(&self, conn_id: CmdTunnelId, resp: SnQueryResp) -> P2pResult<()> {
        let mut state = self.state.write().unwrap();
        for active_sn in state.active_sn_list.iter_mut() {
            if active_sn.conn_id == conn_id {
                let mut recv_futures = active_sn.recv_future.lock().unwrap();
                if let Some(recv_future) = recv_futures.remove(&resp.seq) {
                    recv_future.notify(SnResp::SnQueryResp(resp));
                }
                break;
            }
        }
        Ok(())
    }

    async fn report_sn_resp_handle(&self, tunnel_id: CmdTunnelId, resp: ReportSnResp) -> P2pResult<()> {
        log::info!("report sn resp: {:?}", resp);
        if let Some(cur_report_future) = {
            let mut state = self.state.write().unwrap();
            state.cur_report_future.take()
        } {
            cur_report_future.notify(resp);
        }
        Ok(())
    }

    async fn on_called(&self, conn_id: CmdTunnelId, sn_called: SnCalled) -> P2pResult<()> {
        let listener = {
            let listener = self.listener.lock().unwrap();
            listener.clone()
        };
        let seq = sn_called.seq.clone();
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

        self.cmd_client.send(PackageCmdCode::SnCalledResp as u8, self.cmd_version, resp.to_vec().map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?.as_slice()).await
            .map_err(into_p2p_err!(P2pErrorCode::IoError, "send SnCalledResp failed"))?;
        Ok(())
    }

    pub async fn start(self: &Arc<Self>) -> P2pResult<()> {
        self.register_cmd_handler();
        let this = self.clone();
        let handle = Executor::spawn_with_handle(async move {
            this.ping_proc().await;
        }).map_err(into_p2p_err!(P2pErrorCode::Failed, "start sn ping proc failed"))?;
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
                        match self.report(active_sn.conn_id, active_sn.sn_peer_id.clone()).await {
                            Ok(_) => {}
                            Err(e) => {
                                log::error!("ping to {} failed: {:?}", active_sn.sn_peer_id, e);
                                continue;
                            }
                        }
                    }
                    continue;
                }
            }
            for p2p_network in self.net_manager.get_networks().iter() {
                for sn_cert in self.sn_list.iter() {
                    for sn_ep in sn_cert.endpoints().iter() {
                        if sn_ep.protocol() != p2p_network.protocol() {
                            continue;
                        }
                        for listener in p2p_network.listeners().iter() {
                            let local_ep = listener.local();
                            let ret = if local_ep.addr().ip().is_unspecified() {
                                self.cmd_client.find_tunnel_id_by_classified(SnTunnelClassification::new(None, sn_ep.clone())).await
                            } else {
                                if local_ep.protocol() == Protocol::Tcp {
                                    let mut ep = local_ep.clone();
                                    ep.mut_addr().set_port(0);
                                    self.cmd_client.find_tunnel_id_by_classified(SnTunnelClassification::new(Some(ep), sn_ep.clone())).await
                                } else {
                                    self.cmd_client.find_tunnel_id_by_classified(SnTunnelClassification::new(Some(local_ep.clone()), sn_ep.clone())).await
                                }
                            };

                            if ret.is_err() {
                                continue;
                            }

                            let tunnel_id = ret.unwrap();

                            let report_resp = match self.report(tunnel_id, sn_cert.get_id()).await {
                                Ok(resp) => {
                                    resp
                                }
                                Err(e) => {
                                    log::error!("ping to {} failed: {:?}", sn_cert.get_id(), e);
                                    continue;
                                }
                            };

                            let active_sn = ActiveSN {
                                sn_peer_id: sn_cert.get_id(),
                                latest_time: bucky_time_now(),
                                conn_id: tunnel_id,
                                recv_future: Arc::new(Mutex::new(HashMap::new())),
                                wan_ep_list: report_resp.end_point_array,
                            };
                            let mut state = self.state.write().unwrap();
                            state.active_sn_list.push(active_sn);
                        }
                    }
                }
            }
        }
    }

    fn remove_sn_conn(&self, conn_id: CmdTunnelId) {
        let mut state = self.state.write().unwrap();
        state.active_sn_list.retain(|sn| sn.conn_id != conn_id);
    }

    pub async fn wait_online(&self, timeout: Option<Duration>) -> P2pResult<()> {
        let expire = if timeout.is_some() {
            Some(Utc::now().add(timeout.unwrap()))
        } else {
            None
        };
        loop {
            {
                if expire.is_some() {
                    if Utc::now() > expire.unwrap() {
                        return Err(p2p_err!(P2pErrorCode::Timeout, "wait online timeout"));
                    }
                }
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


    async fn report(&self, tunnel_id: CmdTunnelId, sn_peer_id: P2pId) -> P2pResult<ReportSnResp> {
        let seq = self.gen_seq.generate();
        let local_ips = if_addrs::get_if_addrs().map(|addrs| {
            addrs.iter().filter(|addr| !addr.name.contains("VMware")
                && !addr.name.contains("VirtualBox")
                && !addr.name.contains("ZeroTier")
                && !addr.name.starts_with("zt")
                && !addr.name.contains("Tun")
                && !addr.name.contains("tun")
                && !addr.name.contains("utun")
                && !addr.name.contains("docker")
                && !addr.name.contains("lo")
                && !addr.name.contains("veth")
                && !addr.name.contains("feth")
                && !addr.name.contains("V-M")
                && !addr.name.contains("br-")
                && !addr.name.contains("vEthernet")
                && addr.ip().to_string() != "127.0.0.1")
                .map(|addr| addr.addr.ip().to_string()).collect::<Vec<String>>()
        }).unwrap_or(Vec::new());

        let mut local_eps = Vec::new();
        let mut map_ports = Vec::new();
        for p2p_network in self.net_manager.get_networks().iter() {
            for listener in p2p_network.listeners().iter() {
                if let Some(tcp_map_port) = listener.mapping_port() {
                    map_ports.push((p2p_network.protocol(), tcp_map_port));
                }
                if listener.local().addr().ip().is_unspecified() {
                    for ip in local_ips.iter() {
                        match ip.parse::<IpAddr>() {
                            Ok(ip) => {
                                local_eps.push(Endpoint::from((p2p_network.protocol(), ip, listener.local().addr().port())));
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
        }

        let report = ReportSn {
            protocol_version: 0,
            stack_version: 0,
            seq,
            sn_peer_id,
            from_peer_id: Some(self.local_identity.get_id()),
            peer_info: Some(self.local_identity.get_identity_cert()?.get_encoded_cert()?),
            send_time: bucky_time_now(),
            contract_id: None,
            receipt: None,
            map_ports,
            local_eps,
        };
        let (notify, waiter) = Notify::<ReportSnResp>::new();
        {
            let mut state = self.state.write().unwrap();
            state.cur_report_future = Some(notify);
        }
        match self.cmd_client.send_by_specify_tunnel(tunnel_id, PackageCmdCode::ReportSn as u8, self.cmd_version, report.to_vec().map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?.as_slice()).await {
            Ok(_) => {}
            Err(e) => {
                self.remove_sn_conn(tunnel_id);
                return Err(p2p_err!(P2pErrorCode::IoError));
            }
        }
        let resp = runtime::timeout(self.call_timeout, waiter).await.map_err(into_p2p_err!(P2pErrorCode::ConnectFailed, "report timeout"))?;
        {
            let mut state = self.state.write().unwrap();
            state.cur_report_future = None;
        }
        Ok(resp)
    }

    pub async fn call(&self,
                      tunnel_id: TunnelId,
                      reverse_endpoints: Option<&[Endpoint]>,
                      remote: &P2pId,
                      call_type: TunnelType,
                      payload_pkg: Vec<u8>) -> P2pResult<SnCallResp> {
        let active_list = self.get_active_sn_list();
        for active in active_list.iter() {
            let seq = self.gen_seq.generate();
            let call = SnCall {
                protocol_version: 0,
                stack_version: 0,
                seq,
                tunnel_id,
                sn_peer_id: active.sn_peer_id.clone(),
                to_peer_id: remote.clone(),
                from_peer_id: self.local_identity.get_id().clone(),
                reverse_endpoint_array: reverse_endpoints.map(|ep_list| Vec::from(ep_list)),
                active_pn_list: None,
                peer_info: Some(self.local_identity.get_identity_cert()?.get_encoded_cert()?),
                send_time: bucky_time_now(),
                call_type,
                payload: payload_pkg.clone(),
                is_always_call: false,
            };

            let (notify, waiter) = Notify::<SnResp>::new();
            {
                let mut recv_future = active.recv_future.lock().unwrap();
                recv_future.insert(seq, notify);
            }
            if let Err(e) = self.cmd_client.send_by_specify_tunnel(active.conn_id, PackageCmdCode::SnCall as u8, self.cmd_version, call.to_vec().map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?.as_slice()).await {
                log::error!("send call to {} failed: {:?}", active.sn_peer_id, e);
                self.remove_sn_conn(active.conn_id);
                continue;
            }

            let resp = match runtime::timeout(self.call_timeout, waiter).await {
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
                    log::error!("call to {} timeout", active.sn_peer_id);
                    continue;
                }
            };

            return Ok(resp);
        }
        Err(p2p_err!(P2pErrorCode::ConnectFailed, "call timeout"))
    }

    pub async fn query(&self, device_id: &P2pId) -> P2pResult<SnQueryResp> {
        let active_list = self.get_active_sn_list();
        for active in active_list.iter() {
            let seq = self.gen_seq.generate();
            let query = SnQuery {
                protocol_version: 0,
                stack_version: 0,
                seq,
                query_id: device_id.clone(),
            };
            let (notify, waiter) = Notify::<SnResp>::new();
            {
                let mut recv_future = active.recv_future.lock().unwrap();
                recv_future.insert(seq, notify);
            }
            if let Err(e) = self.cmd_client.send_by_specify_tunnel(active.conn_id, PackageCmdCode::SnQuery as u8, self.cmd_version, query.to_vec().map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?.as_slice()).await {
                log::error!("send call to {} failed: {:?}", active.sn_peer_id, e);
                self.remove_sn_conn(active.conn_id);
                continue;
            }

            let resp = match runtime::timeout(self.call_timeout, waiter).await {
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
                    log::error!("call to {} timeout", active.sn_peer_id);
                    continue;
                }
            };

            return Ok(resp);
        }
        Err(p2p_err!(P2pErrorCode::ConnectFailed, "call timeout"))
    }
}
