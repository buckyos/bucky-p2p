use super::{call_stub::CallStub, peer_manager::PeerManager, receipt::*};
use crate::endpoint::{Endpoint, EndpointArea, Protocol, endpoints_to_string};
use crate::error::{P2pErrorCode, P2pResult, into_p2p_err};
use crate::executor::Executor;
use crate::finder::{DeviceCache, DeviceCacheConfig};
use crate::networks::{
    NetManager, NetManagerRef, QuicCongestionAlgorithm, QuicTunnelNetwork, TcpTunnelNetwork,
    TunnelNetwork, TunnelNetworkRef, TunnelStreamRead, TunnelStreamWrite,
};
use crate::p2p_identity::{
    EncodedP2pIdentityCert, P2pId, P2pIdentityCertFactoryRef, P2pIdentityFactoryRef, P2pIdentityRef,
};
use crate::sn::protocol::{v0::*, *};
use crate::sn::service::peer_manager::PeerManagerRef;
use crate::sn::types::{CmdTunnelId, SN_CMD_SERVICE, SnCmdHeader, SnTunnelRead, SnTunnelWrite};
use crate::tls::{DefaultTlsServerCertResolver, TlsServerCertResolver, init_tls};
use crate::ttp::{TtpListenerRef, TtpPortListener, TtpServer, TtpServerRef};
use crate::types::{SequenceGenerator, Timestamp, TunnelId};
use bucky_raw_codec::{RawConvertTo, RawFrom};
use bucky_time::bucky_time_now;
use log::*;
use sfo_cmd_server::errors::{CmdErrorCode, CmdResult, into_cmd_err};
use sfo_cmd_server::server::{CmdServer, CmdTunnelService, DefaultCmdServerService};
use sfo_cmd_server::{CmdBody, CmdTunnel, PeerId};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{self, AtomicBool},
    },
    time::Duration,
};

// const TRACKER_INTERVAL: Duration = Duration::from_secs(60);
// struct CallTracker {
//     calls: HashMap<TempSeq, (u64, Instant, DeviceId)>, // <called_seq, (call_send_time, called_send_time)>
//     begin_time: Instant,
// }

type SnCmdService = DefaultCmdServerService<(), SnTunnelRead, SnTunnelWrite, u16, u8>;
pub type SnCmdServiceRef = Arc<SnCmdService>;
pub type SnServiceRef = Arc<SnService>;
pub type SnServerRef = Arc<SnServer>;

struct DefaultSnServiceContractServer {}

impl DefaultSnServiceContractServer {
    fn new() -> DefaultSnServiceContractServer {
        DefaultSnServiceContractServer {}
    }
}

impl SnServiceContractServer for DefaultSnServiceContractServer {
    fn check_receipt(
        &self,
        _client_peer_desc: &EncodedP2pIdentityCert, // 客户端desc
        _local_receipt: &SnServiceReceipt,          // 本地(服务端)统计的服务清单
        _client_receipt: &Option<ReceiptWithSignature>, // 客户端提供的服务清单
        _last_request_time: &ReceiptRequestTime,    // 上次要求服务清单的时间
    ) -> IsAcceptClient {
        IsAcceptClient::Accept(false)
    }

    fn verify_auth(&self, _client_device_id: &P2pId) -> IsAcceptClient {
        IsAcceptClient::Accept(false)
    }
}

pub struct SnService {
    seq_generator: Arc<SequenceGenerator>,
    _contract: Box<dyn SnServiceContractServer + Send + Sync>,
    peer_mgr: PeerManagerRef,
    call_stub: CallStub,
    cert_factory: P2pIdentityCertFactoryRef,
    cmd_server: SnCmdServiceRef,
    cmd_version: u8,
}

impl SnService {
    pub fn new(
        cert_factory: P2pIdentityCertFactoryRef,
        contract: Box<dyn SnServiceContractServer + Send + Sync>,
    ) -> SnServiceRef {
        let service = Arc::new(SnService {
            seq_generator: Arc::new(SequenceGenerator::new()),
            _contract: contract,
            peer_mgr: PeerManager::new(),
            call_stub: CallStub::new(),
            cert_factory,
            cmd_server: DefaultCmdServerService::new(),
            cmd_version: 0,
        });
        service.register_sn_cmd_handler();
        service
    }

    pub fn get_cmd_server(&self) -> &SnCmdServiceRef {
        &self.cmd_server
    }

    fn register_sn_cmd_handler(self: &Arc<Self>) {
        let service = self.clone();
        self.cmd_server.register_cmd_handler(
            PackageCmdCode::SnCall as u8,
            move |local_id: PeerId,
                  peer_id: PeerId,
                  tunnel_id: CmdTunnelId,
                  _header: SnCmdHeader,
                  mut cmd_body: CmdBody| {
                let service = service.clone();
                async move {
                    let local_id = P2pId::from(local_id.as_slice());
                    let call_req = SnCall::clone_from_slice(cmd_body.read_all().await?.as_slice())
                        .map_err(into_cmd_err!(CmdErrorCode::RawCodecError))?;
                    service
                        .handle_call(call_req, &local_id, &peer_id, tunnel_id, bucky_time_now())
                        .await;
                    Ok(None)
                }
            },
        );

        let service = self.clone();
        self.cmd_server.register_cmd_handler(
            PackageCmdCode::SnCalledResp as u8,
            move |_local_id: PeerId,
                  _peer_id: PeerId,
                  _tunnel_id: CmdTunnelId,
                  _header: SnCmdHeader,
                  mut cmd_body: CmdBody| {
                let service = service.clone();
                async move {
                    let called_resp =
                        SnCalledResp::clone_from_slice(cmd_body.read_all().await?.as_slice())
                            .map_err(into_cmd_err!(CmdErrorCode::RawCodecError))?;
                    service.handle_called_resp(called_resp).await;
                    Ok(None)
                }
            },
        );

        let service = self.clone();
        self.cmd_server.register_cmd_handler(
            PackageCmdCode::ReportSn as u8,
            move |local_id: PeerId,
                  peer_id: PeerId,
                  tunnel_id: CmdTunnelId,
                  _header: SnCmdHeader,
                  mut cmd_body: CmdBody| {
                let service = service.clone();
                async move {
                    let local_id = P2pId::from(local_id.as_slice());
                    let report_sn =
                        ReportSn::clone_from_slice(cmd_body.read_all().await?.as_slice())
                            .map_err(into_cmd_err!(CmdErrorCode::RawCodecError))?;
                    service
                        .handle_report_sn(&local_id, &peer_id, tunnel_id, report_sn)
                        .await;
                    Ok(None)
                }
            },
        );

        let service = self.clone();
        self.cmd_server.register_cmd_handler(
            PackageCmdCode::SnQuery as u8,
            move |_local_id: PeerId,
                  peer_id: PeerId,
                  tunnel_id: CmdTunnelId,
                  _header: SnCmdHeader,
                  mut cmd_body: CmdBody| {
                let service = service.clone();
                async move {
                    let query = SnQuery::clone_from_slice(cmd_body.read_all().await?.as_slice())
                        .map_err(into_cmd_err!(CmdErrorCode::RawCodecError))?;
                    service.handle_query_sn(&peer_id, tunnel_id, query).await;
                    Ok(None)
                }
            },
        );
    }

    fn peer_manager(&self) -> &PeerManagerRef {
        &self.peer_mgr
    }

    async fn handle_call(
        &self,
        mut call_req: SnCall,
        local_id: &P2pId,
        peer_id: &PeerId,
        tunnel_id: CmdTunnelId,
        _send_time: Timestamp,
    ) -> P2pResult<()> {
        let from_peer_id = &call_req.from_peer_id;
        let log_key = format!(
            "[call {}->{} seq({})]",
            from_peer_id.to_string(),
            call_req.to_peer_id.to_string(),
            call_req.seq.value()
        );
        info!("{}.", log_key);

        let call_resp = if let Some(from_peer_cache) =
            self.peer_manager().find_peer(&call_req.from_peer_id)
        {
            if let Some(to_peer_cache) = self.peer_manager().find_peer(&call_req.to_peer_id) {
                // Self::call_stat_contract(to_peer_cache, &call_req);
                let from_peer_desc = if call_req.peer_info.is_none() {
                    self.peer_manager().find_peer(from_peer_id).map(|c| c.desc)
                } else {
                    call_req
                        .peer_info
                        .map(|info| self.cert_factory.create(&info).unwrap())
                };

                let mut reverse_eps = self
                    .get_peer_wan_ep_with_map_port(&peer_id, from_peer_cache.map_ports.as_slice())
                    .await;
                reverse_eps.extend_from_slice(from_peer_cache.local_eps.as_slice());

                if let Some(from_peer_desc) = from_peer_desc {
                    info!(
                        "{} to-peer found, endpoints: {}, always_call: {}, to-peer.is_wan: {}.",
                        log_key,
                        endpoints_to_string(to_peer_cache.desc.endpoints().as_slice()),
                        call_req.is_always_call,
                        to_peer_cache.is_wan
                    );

                    if self.call_stub.insert(from_peer_id, &call_req.tunnel_id) {
                        if call_req.is_always_call || !to_peer_cache.is_wan {
                            let called_seq = self.seq_generator.generate();
                            let mut called_req = SnCalled {
                                seq: called_seq,
                                to_peer_id: call_req.to_peer_id.clone(),
                                sn_peer_id: local_id.clone(),
                                peer_info: from_peer_desc.get_encoded_cert().unwrap(),
                                tunnel_id: call_req.tunnel_id,
                                call_send_time: call_req.send_time,
                                call_type: call_req.call_type,
                                payload: vec![],
                                reverse_endpoint_array: vec![],
                                active_pn_list: vec![],
                            };

                            std::mem::swap(&mut call_req.payload, &mut called_req.payload);
                            if let Some(eps) = call_req.reverse_endpoint_array.as_mut() {
                                std::mem::swap(eps, &mut called_req.reverse_endpoint_array);
                            }
                            if let Some(pn_list) = call_req.active_pn_list.as_mut() {
                                std::mem::swap(pn_list, &mut called_req.active_pn_list);
                            }
                            called_req
                                .reverse_endpoint_array
                                .extend_from_slice(reverse_eps.as_slice());

                            let called_log =
                                format!("{} called-req seq({})", log_key, called_seq.value());
                            log::info!(
                                "{} will send with payload(len={}) pn_list({:?}).",
                                called_log,
                                called_req.payload.len(),
                                called_req.active_pn_list
                            );

                            self.cmd_server
                                .send_by_all_tunnels(
                                    &PeerId::from(call_req.to_peer_id.as_slice()),
                                    PackageCmdCode::SnCalled as u8,
                                    self.cmd_version,
                                    called_req
                                        .to_vec()
                                        .map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?
                                        .as_slice(),
                                )
                                .await
                                .map_err(into_p2p_err!(P2pErrorCode::IoError))?;
                        }
                    } else {
                        info!("{} ignore send called req for already exists.", log_key);
                    }

                    SnCallResp {
                        seq: call_req.seq,
                        sn_peer_id: local_id.clone(),
                        result: P2pErrorCode::Ok.into_u8(),
                        to_peer_info: Some(to_peer_cache.desc.get_encoded_cert().unwrap()),
                    }
                } else {
                    warn!("{} without from-desc.", log_key);

                    SnCallResp {
                        seq: call_req.seq,
                        sn_peer_id: local_id.clone(),
                        result: P2pErrorCode::NotFound.into_u8(),
                        to_peer_info: None,
                    }
                }
            } else {
                warn!("{} to-peer not found.", log_key);
                SnCallResp {
                    seq: call_req.seq,
                    sn_peer_id: local_id.clone(),
                    result: P2pErrorCode::NotFound.into_u8(),
                    to_peer_info: None,
                }
            }
        } else {
            warn!("{} from-peer not found.", log_key);
            SnCallResp {
                seq: call_req.seq,
                sn_peer_id: local_id.clone(),
                result: P2pErrorCode::NotFound.into_u8(),
                to_peer_info: None,
            }
        };

        self.cmd_server
            .send_by_specify_tunnel(
                peer_id,
                tunnel_id,
                PackageCmdCode::SnCallResp as u8,
                self.cmd_version,
                call_resp
                    .to_vec()
                    .map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?
                    .as_slice(),
            )
            .await
            .map_err(into_p2p_err!(P2pErrorCode::IoError))?;
        Ok(())
    }

    async fn handle_called_resp(&self, called_resp: SnCalledResp) {
        info!("called-resp seq {}.", called_resp.seq.value());

        // 统计性能
        // if let Some((call_send_time, called_send_time, peerid)) = self.call_tracker.calls.remove(&called_resp.seq) {
        //     if let Some(cached_peer) = self.peer_mgr.find_peer(&peerid, FindPeerReason::Other) {
        //         let now_time_stamp = bucky_time_now();
        //         if now_time_stamp > call_send_time {
        //             let call_delay = (now_time_stamp - call_send_time) / 1000;
        //             cached_peer.receipt.call_delay = ((cached_peer.receipt.call_delay as u64 * 7 + call_delay) / 8) as u16;
        //         }

        //         let rto = Instant::now().duration_since(called_send_time).as_millis() as u32;
        //         cached_peer.receipt.rto = ((cached_peer.receipt.rto as u32 * 7 + rto) / 8) as u16;
        //     }
        // }
    }

    pub async fn get_peer_wan_ep(&self, peer_id: &PeerId) -> Vec<Endpoint> {
        let tunnels = self.cmd_server.get_peer_tunnels(peer_id).await;
        let mut remotes = Vec::new();
        for tunnel in tunnels.iter() {
            let remote = tunnel.send.get().await.remote();
            if !remotes.contains(&remote) {
                remotes.push(remote);
            }
        }
        remotes
            .iter_mut()
            .filter(|remote| !remote.is_loopback())
            .map(|remote| {
                remote.set_area(EndpointArea::Wan);
                remote.clone()
            })
            .collect()
    }

    async fn get_peer_wan_ep_with_map_port(
        &self,
        peer_id: &PeerId,
        map_ports: &[(Protocol, u16)],
    ) -> Vec<Endpoint> {
        let mut remote_ep = self.get_peer_wan_ep(peer_id).await;
        let mut map_eps = Vec::new();
        for ep in remote_ep.iter() {
            for (protocol, port) in map_ports.iter() {
                let mut map_ep = Endpoint::from((*protocol, ep.addr().ip(), *port));
                if map_eps.contains(&map_ep) {
                    continue;
                }
                map_ep.set_area(EndpointArea::Wan);
                map_eps.push(map_ep);
            }
        }
        remote_ep.extend_from_slice(map_eps.as_slice());
        remote_ep
    }

    async fn handle_report_sn(
        &self,
        local_id: &P2pId,
        peer_id: &PeerId,
        tunnel_id: CmdTunnelId,
        report_sn: ReportSn,
    ) -> P2pResult<()> {
        log::info!(
            "report sn from {}.eps: {:?} map_port: {:?}",
            peer_id.to_base36(),
            report_sn.local_eps,
            report_sn.map_ports
        );
        let mut remote_ep = self.get_peer_wan_ep(peer_id).await;

        if report_sn.from_peer_id.is_some() {
            self.peer_mgr.add_or_update_peer(
                report_sn.from_peer_id.as_ref().unwrap(),
                &report_sn
                    .peer_info
                    .map(|info| self.cert_factory.create(&info).unwrap()),
                report_sn.map_ports,
                &report_sn.local_eps,
            );
        }
        if let Err(e) = self
            .cmd_server
            .send_by_specify_tunnel(
                peer_id,
                tunnel_id,
                PackageCmdCode::ReportSnResp as u8,
                self.cmd_version,
                ReportSnResp {
                    seq: report_sn.seq,
                    sn_peer_id: local_id.clone(),
                    result: P2pErrorCode::Ok.into_u8(),
                    peer_info: None,
                    end_point_array: remote_ep,
                    receipt: None,
                }
                .to_vec()
                .map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?
                .as_slice(),
            )
            .await
        {
            log::error!(
                "send report-sn-resp failed, peer_id: {:?}, error: {:?}",
                peer_id,
                e
            );
        }
        Ok(())
    }

    async fn handle_query_sn(
        &self,
        peer_id: &PeerId,
        tunnel_id: CmdTunnelId,
        query: SnQuery,
    ) -> P2pResult<()> {
        let device_info = self.peer_mgr.find_peer(&query.query_id);
        let resp = if device_info.is_some() {
            let device_info = device_info.unwrap();
            let mut end_point_array = self
                .get_peer_wan_ep_with_map_port(
                    &PeerId::from(query.query_id.as_slice()),
                    device_info.map_ports.as_slice(),
                )
                .await;
            end_point_array.extend_from_slice(device_info.local_eps.as_slice());
            SnQueryResp {
                seq: query.seq,
                peer_info: Some(device_info.desc.get_encoded_cert().unwrap()),
                end_point_array,
            }
        } else {
            SnQueryResp {
                seq: query.seq,
                peer_info: None,
                end_point_array: vec![],
            }
        };

        self.cmd_server
            .send_by_specify_tunnel(
                peer_id,
                tunnel_id,
                PackageCmdCode::SnQueryResp as u8,
                self.cmd_version,
                resp.to_vec()
                    .map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?
                    .as_slice(),
            )
            .await
            .map_err(into_p2p_err!(P2pErrorCode::IoError))?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl CmdTunnelService<(), SnTunnelRead, SnTunnelWrite> for SnService {
    async fn handle_tunnel(&self, tunnel: CmdTunnel<SnTunnelRead, SnTunnelWrite>) -> CmdResult<()> {
        self.cmd_server.serve_tunnel(tunnel).await
    }
}

pub struct SnServer {
    local_identity: P2pIdentityRef,
    net_manager: NetManagerRef,
    ttp_server: TtpServerRef,
    service: SnServiceRef,
    started: AtomicBool,
    stopped: AtomicBool,
    cmd_accept_task: Mutex<Option<crate::executor::SpawnHandle<()>>>,
}

impl SnServer {
    pub(crate) async fn new(
        local_identity: P2pIdentityRef,
        identity_factory: P2pIdentityFactoryRef,
        cert_factory: P2pIdentityCertFactoryRef,
        contract: Box<dyn SnServiceContractServer + Send + Sync>,
        congestion_algorithm: QuicCongestionAlgorithm,
        reuse_address: bool,
    ) -> SnServerRef {
        Executor::init_new_multi_thread(None);
        init_tls(identity_factory);

        let device_cache = Arc::new(DeviceCache::new(
            &DeviceCacheConfig {
                expire: Duration::from_secs(240),
                capacity: 10240,
            },
            None,
        ));
        let cert_resolver = DefaultTlsServerCertResolver::new();
        let _ = cert_resolver
            .add_server_identity(local_identity.clone())
            .await;
        let tcp_network = Arc::new(TcpTunnelNetwork::new(
            cert_resolver.clone(),
            cert_factory.clone(),
            Duration::from_secs(30),
            Duration::from_secs(5),
            Duration::from_secs(15),
        ));
        TunnelNetwork::set_reuse_address(tcp_network.as_ref(), reuse_address);
        let quic_network = Arc::new(QuicTunnelNetwork::new(
            device_cache,
            cert_resolver.clone(),
            cert_factory.clone(),
            congestion_algorithm,
            Duration::from_secs(30),
            Duration::from_secs(30),
        ));
        TunnelNetwork::set_reuse_address(quic_network.as_ref(), reuse_address);
        let tunnel_networks = vec![
            tcp_network as TunnelNetworkRef,
            quic_network as TunnelNetworkRef,
        ];

        let net_manager = NetManager::new(tunnel_networks, cert_resolver).unwrap();
        let ttp_server = TtpServer::new(local_identity.clone(), net_manager.clone()).unwrap();
        let service = SnService::new(cert_factory, contract);

        Arc::new(Self {
            local_identity,
            net_manager,
            ttp_server,
            service,
            started: AtomicBool::new(false),
            stopped: AtomicBool::new(false),
            cmd_accept_task: Mutex::new(None),
        })
    }

    pub fn get_cmd_server(&self) -> &SnCmdServiceRef {
        self.service.get_cmd_server()
    }

    pub fn service(&self) -> &SnServiceRef {
        &self.service
    }

    pub fn ttp_server(&self) -> TtpServerRef {
        self.ttp_server.clone()
    }

    pub async fn start(self: &Arc<Self>) -> P2pResult<()> {
        if self
            .started
            .compare_exchange(
                false,
                true,
                atomic::Ordering::SeqCst,
                atomic::Ordering::SeqCst,
            )
            .is_err()
        {
            log::debug!("sn server start skipped because already started");
            return Ok(());
        }

        log::info!(
            "sn server start begin local_id={}",
            self.local_identity.get_id()
        );

        if let Err(err) = self.start_inner().await {
            self.abort_accept_tasks();
            self.started.store(false, atomic::Ordering::SeqCst);
            log::error!(
                "sn server start failed local_id={} err={:?}",
                self.local_identity.get_id(),
                err
            );
            return Err(err);
        }

        log::info!("sn server start success local_id={}", self.local_identity.get_id());
        Ok(())
    }

    async fn start_inner(self: &Arc<Self>) -> P2pResult<()> {
        log::debug!(
            "sn server listen endpoints local_id={} endpoints={:?}",
            self.local_identity.get_id(),
            self.local_identity.endpoints()
        );
        self.net_manager
            .listen(self.local_identity.endpoints().as_slice(), None)
            .await?;
        log::debug!(
            "sn server net_manager listen ready local_id={}",
            self.local_identity.get_id()
        );
        self.start_cmd_accept_loop().await?;
        Ok(())
    }

    async fn start_cmd_accept_loop(self: &Arc<Self>) -> P2pResult<()> {
        let purpose =
            crate::networks::TunnelPurpose::from_value(&SN_CMD_SERVICE.to_string()).unwrap();
        log::debug!(
            "sn server start cmd accept loop local_id={} purpose={}",
            self.local_identity.get_id(),
            purpose
        );
        let listener = self
            .ttp_server
            .listen_stream(purpose)
            .await?;
        let server = self.clone();
        let task = Executor::spawn_with_handle(async move {
            server.run_cmd_accept_loop(listener).await;
        })
        .unwrap();
        *self.cmd_accept_task.lock().unwrap() = Some(task);
        log::debug!(
            "sn server cmd accept loop started local_id={}",
            self.local_identity.get_id()
        );
        Ok(())
    }

    async fn run_cmd_accept_loop(self: Arc<Self>, listener: TtpListenerRef) {
        loop {
            let accepted = match listener.accept().await {
                Ok(accepted) => accepted,
                Err(err) => {
                    warn!("sn server cmd accept stopped: {:?}", err);
                    break;
                }
            };

            let tunnel = Self::into_cmd_tunnel(accepted);
            let service = self.service.clone();
            Executor::spawn(async move {
                if let Err(err) = service.handle_tunnel(tunnel).await {
                    error!("sn server handle cmd tunnel failed: {:?}", err);
                }
            });
        }
    }

    fn into_cmd_tunnel(
        accepted: (
            crate::ttp::TtpStreamMeta,
            TunnelStreamRead,
            TunnelStreamWrite,
        ),
    ) -> CmdTunnel<SnTunnelRead, SnTunnelWrite> {
        let (meta, read, write) = accepted;
        let local = meta.local_ep.unwrap_or_default();
        let remote = meta.remote_ep.unwrap_or_default();
        let local_id = meta.local_id;
        let remote_id = meta.remote_id;
        CmdTunnel::new(
            SnTunnelRead::new(read, local, remote, local_id.clone(), remote_id.clone()),
            SnTunnelWrite::new(write, local, remote, local_id, remote_id),
        )
    }

    pub fn stop(&self) {
        self.stopped.store(true, atomic::Ordering::Relaxed);
        self.abort_accept_tasks();
    }

    pub fn is_stopped(&self) -> bool {
        self.stopped.load(atomic::Ordering::Relaxed)
    }

    fn abort_accept_tasks(&self) {
        if let Some(task) = self.cmd_accept_task.lock().unwrap().take() {
            task.abort();
        }
    }
}

impl Drop for SnServer {
    fn drop(&mut self) {
        self.abort_accept_tasks();
    }
}

// #[async_trait::async_trait]
// impl TcpListenerEventListener for SnServer {
//     async fn on_new_connection(&self, socket: TCPSocket) -> BdtResult<()> {
//         self.handle(socket).await
//     }
// }

pub struct SnServiceConfig {
    local_identity: P2pIdentityRef,
    identity_factory: P2pIdentityFactoryRef,
    cert_factory: P2pIdentityCertFactoryRef,
    contract: Box<dyn SnServiceContractServer + Send + Sync>,
    quic_congestion_algorithm: QuicCongestionAlgorithm,
    reuse_address: bool,
}

impl SnServiceConfig {
    pub fn new(
        local_identity: P2pIdentityRef,
        identity_factory: P2pIdentityFactoryRef,
        cert_factory: P2pIdentityCertFactoryRef,
    ) -> Self {
        Self {
            local_identity,
            identity_factory,
            cert_factory,
            contract: Box::new(DefaultSnServiceContractServer::new()),
            quic_congestion_algorithm: QuicCongestionAlgorithm::Bbr,
            reuse_address: false,
        }
    }

    pub fn set_contract(
        mut self,
        contract: Box<dyn SnServiceContractServer + Send + Sync>,
    ) -> Self {
        self.contract = contract;
        self
    }

    pub fn set_quic_congestion_algorithm(
        mut self,
        quic_algorithm: QuicCongestionAlgorithm,
    ) -> Self {
        self.quic_congestion_algorithm = quic_algorithm;
        self
    }

    pub fn set_reuse_address(mut self, reuse_address: bool) -> Self {
        self.reuse_address = reuse_address;
        self
    }
}

pub async fn create_sn_service(config: SnServiceConfig) -> SnServerRef {
    let service = SnServer::new(
        config.local_identity,
        config.identity_factory,
        config.cert_factory,
        config.contract,
        config.quic_congestion_algorithm,
        config.reuse_address,
    )
    .await;
    service
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{P2pErrorCode, p2p_err};
    use crate::networks::{
        Tunnel, TunnelListener, TunnelListenerInfo, TunnelListenerRef, TunnelNetwork,
        TunnelNetworkRef, TunnelState, TunnelStreamRead, TunnelStreamWrite,
    };
    use crate::p2p_identity::{
        EncodedP2pIdentity, P2pIdentity, P2pIdentityCertRef, P2pIdentityRef, P2pSignature,
    };
    use crate::tls::DefaultTlsServerCertResolver;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream, WriteHalf, split};
    use tokio::sync::{Mutex as AsyncMutex, mpsc};
    use tokio::time::{Duration, timeout};

    struct DummyIdentity {
        id: P2pId,
        name: String,
        endpoints: Vec<Endpoint>,
    }

    impl P2pIdentity for DummyIdentity {
        fn get_identity_cert(&self) -> P2pResult<P2pIdentityCertRef> {
            Err(p2p_err!(P2pErrorCode::NotSupport, "unused in test"))
        }

        fn get_id(&self) -> P2pId {
            self.id.clone()
        }

        fn get_name(&self) -> String {
            self.name.clone()
        }

        fn sign_type(&self) -> crate::p2p_identity::P2pIdentitySignType {
            crate::p2p_identity::P2pIdentitySignType::Rsa
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

    struct FakeTunnel {
        tunnel_id: crate::types::TunnelId,
        candidate_id: crate::types::TunnelCandidateId,
        local_id: P2pId,
        remote_id: P2pId,
        local_ep: Endpoint,
        remote_ep: Endpoint,
        incoming_rx: AsyncMutex<
            mpsc::UnboundedReceiver<(
                crate::networks::TunnelPurpose,
                TunnelStreamRead,
                TunnelStreamWrite,
            )>,
        >,
    }

    impl FakeTunnel {
        fn new(
            local_id: P2pId,
            remote_id: P2pId,
            local_ep: Endpoint,
            remote_ep: Endpoint,
        ) -> (
            Arc<Self>,
            mpsc::UnboundedSender<(
                crate::networks::TunnelPurpose,
                TunnelStreamRead,
                TunnelStreamWrite,
            )>,
        ) {
            let (tx, rx) = mpsc::unbounded_channel();
            (
                Arc::new(Self {
                    tunnel_id: crate::types::TunnelId::from(1),
                    candidate_id: crate::types::TunnelCandidateId::from(1),
                    local_id,
                    remote_id,
                    local_ep,
                    remote_ep,
                    incoming_rx: AsyncMutex::new(rx),
                }),
                tx,
            )
        }
    }

    #[async_trait::async_trait]
    impl Tunnel for FakeTunnel {
        fn tunnel_id(&self) -> crate::types::TunnelId {
            self.tunnel_id
        }

        fn candidate_id(&self) -> crate::types::TunnelCandidateId {
            self.candidate_id
        }

        fn form(&self) -> crate::networks::TunnelForm {
            crate::networks::TunnelForm::Active
        }

        fn is_reverse(&self) -> bool {
            false
        }

        fn protocol(&self) -> Protocol {
            self.local_ep.protocol()
        }

        fn local_id(&self) -> P2pId {
            self.local_id.clone()
        }

        fn remote_id(&self) -> P2pId {
            self.remote_id.clone()
        }

        fn local_ep(&self) -> Option<Endpoint> {
            Some(self.local_ep)
        }

        fn remote_ep(&self) -> Option<Endpoint> {
            Some(self.remote_ep)
        }

        fn state(&self) -> TunnelState {
            TunnelState::Connected
        }

        fn is_closed(&self) -> bool {
            false
        }

        async fn close(&self) -> P2pResult<()> {
            Ok(())
        }

        async fn listen_stream(&self, _vports: crate::networks::ListenVPortsRef) -> P2pResult<()> {
            Ok(())
        }

        async fn listen_datagram(
            &self,
            _vports: crate::networks::ListenVPortsRef,
        ) -> P2pResult<()> {
            Ok(())
        }

        async fn open_stream(
            &self,
            _purpose: crate::networks::TunnelPurpose,
        ) -> P2pResult<(TunnelStreamRead, TunnelStreamWrite)> {
            Err(p2p_err!(P2pErrorCode::NotSupport, "unused in test"))
        }

        async fn accept_stream(
            &self,
        ) -> P2pResult<(
            crate::networks::TunnelPurpose,
            TunnelStreamRead,
            TunnelStreamWrite,
        )> {
            let mut rx = self.incoming_rx.lock().await;
            rx.recv()
                .await
                .ok_or_else(|| p2p_err!(P2pErrorCode::Interrupted, "stream closed"))
        }

        async fn open_datagram(
            &self,
            _purpose: crate::networks::TunnelPurpose,
        ) -> P2pResult<crate::networks::TunnelDatagramWrite> {
            Err(p2p_err!(P2pErrorCode::NotSupport, "unused in test"))
        }

        async fn accept_datagram(
            &self,
        ) -> P2pResult<(
            crate::networks::TunnelPurpose,
            crate::networks::TunnelDatagramRead,
        )> {
            Err(p2p_err!(P2pErrorCode::NotSupport, "unused in test"))
        }
    }

    struct FakeTunnelListener {
        rx: AsyncMutex<mpsc::UnboundedReceiver<P2pResult<crate::networks::TunnelRef>>>,
    }

    #[async_trait::async_trait]
    impl TunnelListener for FakeTunnelListener {
        async fn accept_tunnel(&self) -> P2pResult<crate::networks::TunnelRef> {
            let mut rx = self.rx.lock().await;
            rx.recv()
                .await
                .ok_or_else(|| p2p_err!(P2pErrorCode::Interrupted, "tunnel listener closed"))?
        }
    }

    struct FakeTunnelNetwork {
        protocol: Protocol,
        listener: TunnelListenerRef,
        tx: mpsc::UnboundedSender<P2pResult<crate::networks::TunnelRef>>,
        infos: Mutex<Vec<TunnelListenerInfo>>,
    }

    impl FakeTunnelNetwork {
        fn new(protocol: Protocol) -> Arc<Self> {
            let (tx, rx) = mpsc::unbounded_channel();
            Arc::new(Self {
                protocol,
                listener: Arc::new(FakeTunnelListener {
                    rx: AsyncMutex::new(rx),
                }),
                tx,
                infos: Mutex::new(Vec::new()),
            })
        }

        fn push_tunnel(&self, tunnel: crate::networks::TunnelRef) {
            let _ = self.tx.send(Ok(tunnel));
        }
    }

    #[async_trait::async_trait]
    impl TunnelNetwork for FakeTunnelNetwork {
        fn protocol(&self) -> Protocol {
            self.protocol
        }

        fn is_udp(&self) -> bool {
            self.protocol == Protocol::Quic
        }

        async fn listen(
            &self,
            local: &Endpoint,
            _out: Option<Endpoint>,
            mapping_port: Option<u16>,
        ) -> P2pResult<TunnelListenerRef> {
            *self.infos.lock().unwrap() = vec![TunnelListenerInfo {
                local: *local,
                mapping_port,
            }];
            Ok(self.listener.clone())
        }

        async fn close_all_listener(&self) -> P2pResult<()> {
            Ok(())
        }

        fn listeners(&self) -> Vec<TunnelListenerRef> {
            vec![self.listener.clone()]
        }

        fn listener_infos(&self) -> Vec<TunnelListenerInfo> {
            self.infos.lock().unwrap().clone()
        }

        async fn create_tunnel_with_intent(
            &self,
            _local_identity: &P2pIdentityRef,
            _remote: &Endpoint,
            _remote_id: &P2pId,
            _remote_name: Option<String>,
            _intent: crate::networks::TunnelConnectIntent,
        ) -> P2pResult<crate::networks::TunnelRef> {
            Err(p2p_err!(P2pErrorCode::NotSupport, "unused in test"))
        }

        async fn create_tunnel_with_local_ep_and_intent(
            &self,
            _local_identity: &P2pIdentityRef,
            _local_ep: &Endpoint,
            _remote: &Endpoint,
            _remote_id: &P2pId,
            _remote_name: Option<String>,
            _intent: crate::networks::TunnelConnectIntent,
        ) -> P2pResult<crate::networks::TunnelRef> {
            Err(p2p_err!(P2pErrorCode::NotSupport, "unused in test"))
        }
    }

    fn test_identity(local_ep: Endpoint) -> P2pIdentityRef {
        Arc::new(DummyIdentity {
            id: P2pId::from(vec![1u8; 32]),
            name: "local-test".to_owned(),
            endpoints: vec![local_ep],
        })
    }

    fn remote_id() -> P2pId {
        P2pId::from(vec![2u8; 32])
    }

    fn make_stream_pair() -> (
        (TunnelStreamRead, TunnelStreamWrite),
        WriteHalf<DuplexStream>,
    ) {
        let (test_end, tunnel_end) = tokio::io::duplex(64);
        let (_test_read, test_write) = split(test_end);
        let (tunnel_read, tunnel_write) = split(tunnel_end);
        ((Box::pin(tunnel_read), Box::pin(tunnel_write)), test_write)
    }

    fn init_executor() {
        Executor::init_new_multi_thread(None);
    }

    #[tokio::test]
    async fn sn_server_wraps_sn_cmd_vport_into_cmd_tunnel() {
        init_executor();
        let local_ep = Endpoint::from((Protocol::Quic, "127.0.0.1:23001".parse().unwrap()));
        let remote_ep = Endpoint::from((Protocol::Quic, "127.0.0.1:23002".parse().unwrap()));
        let identity = test_identity(local_ep);
        let fake_network = FakeTunnelNetwork::new(Protocol::Quic);
        let net_manager = NetManager::new(
            vec![fake_network.clone() as TunnelNetworkRef],
            DefaultTlsServerCertResolver::new(),
        )
        .unwrap();
        net_manager.listen(&[local_ep], None).await.unwrap();
        let ttp_server = TtpServer::new(identity.clone(), net_manager.clone()).unwrap();
        let listener = ttp_server
            .listen_stream(
                crate::networks::TunnelPurpose::from_value(&SN_CMD_SERVICE.to_string()).unwrap(),
            )
            .await
            .unwrap();

        let (tunnel, stream_tx) =
            FakeTunnel::new(identity.get_id(), remote_id(), local_ep, remote_ep);
        let ((read, write), mut remote_write) = make_stream_pair();
        stream_tx
            .send((
                crate::networks::TunnelPurpose::from_value(&SN_CMD_SERVICE.to_string()).unwrap(),
                read,
                write,
            ))
            .unwrap();
        fake_network.push_tunnel(tunnel);

        let accepted = timeout(Duration::from_secs(1), listener.accept())
            .await
            .unwrap()
            .unwrap();
        let cmd_tunnel = SnServer::into_cmd_tunnel(accepted);
        let (mut cmd_read, _cmd_write) = cmd_tunnel.split();

        remote_write.write_all(b"ping").await.unwrap();
        let mut buf = [0u8; 4];
        cmd_read.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"ping");
    }
}
