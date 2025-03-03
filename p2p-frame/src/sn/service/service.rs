use log::*;
use std::{
    sync::{
        atomic::{self, AtomicBool},
        Arc,
    },
    time::Duration,
};
use bucky_raw_codec::{RawConvertTo, RawFrom};
use bucky_time::bucky_time_now;
use callback_result::SingleCallbackWaiter;
use sfo_cmd_server::{CmdBodyRead, CmdHeader, CmdTunnel, CmdTunnelRead, CmdTunnelWrite, PeerId};
use sfo_cmd_server::errors::{cmd_err, into_cmd_err, CmdErrorCode, CmdResult};
use sfo_cmd_server::server::{CmdServer, CmdTunnelListener, DefaultCmdServer};
use crate::endpoint::{endpoints_to_string, Endpoint, EndpointArea, Protocol};
use crate::error::{into_p2p_err, P2pErrorCode, P2pResult};
use crate::executor::Executor;
use crate::finder::{DeviceCache, DeviceCacheConfig};
use crate::p2p_connection::{DefaultP2pConnectionInfoCache, P2pConnection};
use crate::p2p_identity::{P2pId, P2pIdentityRef, P2pIdentityCertFactoryRef, P2pIdentityFactoryRef, P2pIdentityCertCacheRef, EncodedP2pIdentityCert};
use crate::p2p_network::P2pNetworkRef;
use crate::pn::PnServer;
use crate::protocol::{v0::*, *};
use crate::runtime;
use crate::sn::service::peer_manager::PeerManagerRef;
use crate::sn::types::{CmdTunnelId, SnCmdHeader, SnTunnelRead, SnTunnelWrite};
use crate::sockets::{NetListener, NetListenerRef, NetManager, NetManagerRef, QuicNetwork};
use crate::sockets::tcp::TcpNetwork;
use crate::tls::{init_tls, DefaultTlsServerCertResolver, TlsServerCertResolver};
use crate::types::{TunnelId, TunnelIdGenerator, Timestamp, SessionIdGenerator, SequenceGenerator};
use super::{call_stub::CallStub, peer_manager::PeerManager, receipt::*};

// const TRACKER_INTERVAL: Duration = Duration::from_secs(60);
// struct CallTracker {
//     calls: HashMap<TempSeq, (u64, Instant, DeviceId)>, // <called_seq, (call_send_time, called_send_time)>
//     begin_time: Instant,
// }

pub struct SnTunnelListener {
    net_manager: NetManagerRef,
    waiter: Arc<SingleCallbackWaiter<P2pConnection>>,
}

impl SnTunnelListener {
    pub fn new(local_identity: P2pIdentityRef,
               net_manager: NetManagerRef) -> Self {
        let waiter = Arc::new(SingleCallbackWaiter::new());
        let socket_waiter = waiter.clone();
        net_manager.set_connection_event_listener(local_identity.get_id(), move |socket: P2pConnection| {
            let socket_waiter = socket_waiter.clone();
            async move {
                socket_waiter.set_result_with_cache(socket);
                Ok(())
            }
        });
        Self {
            net_manager,
            waiter,
        }
    }
}

#[async_trait::async_trait]
impl CmdTunnelListener<SnTunnelRead, SnTunnelWrite> for SnTunnelListener {
    async fn accept(&self) -> CmdResult<CmdTunnel<SnTunnelRead, SnTunnelWrite>> {
        let socket = self.waiter.create_result_future().map_err(into_cmd_err!(CmdErrorCode::Failed))?.await.map_err(|e| cmd_err!(CmdErrorCode::Failed, "{:?}", e))?;
        let (read, write) = socket.split();
        Ok(CmdTunnel::new(SnTunnelRead::new(read), SnTunnelWrite::new(write)))
    }
}

type SnCmdServer = DefaultCmdServer<SnTunnelRead, SnTunnelWrite, u16, u8, SnTunnelListener>;
pub type SnCmdServerRef = Arc<DefaultCmdServer<SnTunnelRead, SnTunnelWrite, u16, u8, SnTunnelListener>>;


struct DefaultSnServiceContractServer {}

impl DefaultSnServiceContractServer {
    fn new() -> DefaultSnServiceContractServer {
        DefaultSnServiceContractServer {}
    }
}

impl SnServiceContractServer for DefaultSnServiceContractServer {

    fn check_receipt(&self,
                     _client_peer_desc: &EncodedP2pIdentityCert, // 客户端desc
                     _local_receipt: &SnServiceReceipt, // 本地(服务端)统计的服务清单
                     _client_receipt: &Option<ReceiptWithSignature>, // 客户端提供的服务清单
                     _last_request_time: &ReceiptRequestTime, // 上次要求服务清单的时间
    ) -> IsAcceptClient
    {
        IsAcceptClient::Accept(false)
    }

    fn verify_auth(&self, _client_device_id: &P2pId) -> IsAcceptClient {
        IsAcceptClient::Accept(false)
    }
}


pub struct SnService {
    seq_generator: Arc<SequenceGenerator>,
    device_cache: P2pIdentityCertCacheRef,
    local_identity: P2pIdentityRef,
    stopped: AtomicBool,
    contract: Box<dyn SnServiceContractServer + Send + Sync>,

    // call_tracker: CallTracker,
    peer_mgr: PeerManagerRef,
    call_stub: CallStub,
    cert_factory: P2pIdentityCertFactoryRef,
    net_manager: NetManagerRef,
    cmd_server: SnCmdServerRef,
    proxy_server: Option<Arc<PnServer<SnCmdServer>>>,
    cmd_version: u8,
}

pub type SnServiceRef = Arc<SnService>;

impl SnService {
    pub(crate) async fn new(
        local_identity: P2pIdentityRef,
        identity_factory: P2pIdentityFactoryRef,
        cert_factory: P2pIdentityCertFactoryRef,
        contract: Box<dyn SnServiceContractServer + Send + Sync>,
        support_proxy: bool,
    ) -> SnServiceRef {
        Executor::init_new_multi_thread(None);
        init_tls(identity_factory);
        let device_cache = Arc::new(DeviceCache::new(&DeviceCacheConfig {
            expire: Duration::from_secs(240),
            capacity: 10240,
        }, None));
        let cert_resolver = DefaultTlsServerCertResolver::new();
        let _ = cert_resolver.add_server_identity(local_identity.clone()).await;

        let tcp_network = Arc::new(TcpNetwork::new(device_cache.clone(), cert_resolver.clone(), cert_factory.clone(), Duration::from_secs(30)));
        let quic_network = Arc::new(QuicNetwork::new(device_cache.clone(), cert_resolver.clone(), cert_factory.clone(), Duration::from_secs(30), Duration::from_secs(30)));

        let mut networks = Vec::new();
        networks.push(tcp_network as P2pNetworkRef);
        networks.push(quic_network as P2pNetworkRef);

        let connection_info_cache = DefaultP2pConnectionInfoCache::new();
        let net_manager = Arc::new(NetManager::new(networks, cert_resolver, connection_info_cache).unwrap());
        let sn_cmd_server  = DefaultCmdServer::new(SnTunnelListener::new(local_identity.clone(), net_manager.clone()));

        let proxy_server = if support_proxy {
            let proxy_server = PnServer::new(sn_cmd_server.clone());
            proxy_server.start();
            Some(proxy_server)
        } else {
            None
        };

        let service = SnService {
            seq_generator: Arc::new(SequenceGenerator::new()),
            local_identity: local_identity.clone(),
            stopped: AtomicBool::new(false),
            peer_mgr: PeerManager::new(),
            call_stub: CallStub::new(),
            contract,
            device_cache,
            cert_factory,
            net_manager,
            cmd_server: sn_cmd_server,
            proxy_server,
            cmd_version: 0,
        };

        let service_ref = Arc::new(service);

        service_ref
    }

    pub fn get_cmd_server(&self) -> &SnCmdServerRef {
        &self.cmd_server
    }

    fn register_sn_cmd_handler(self: &Arc<Self>) {
        let service = self.clone();
        self.cmd_server.register_cmd_handler(PackageCmdCode::SnCall as u8, move |peer_id: PeerId, tunnel_id: CmdTunnelId, header: SnCmdHeader, mut cmd_body: CmdBodyRead| {
            let service = service.clone();
            async move {
                let call_req = SnCall::clone_from_slice(cmd_body.read_all().await?.as_slice()).map_err(into_cmd_err!(CmdErrorCode::RawCodecError))?;
                service.handle_call(call_req, &peer_id, tunnel_id, bucky_time_now()).await;
                Ok(())
            }
        });

        let service = self.clone();
        self.cmd_server.register_cmd_handler(PackageCmdCode::SnCalledResp as u8, move |_peer_id: PeerId, _tunnel_id: CmdTunnelId, header: SnCmdHeader, mut cmd_body: CmdBodyRead| {
            let service = service.clone();
            async move {
                let called_resp = SnCalledResp::clone_from_slice(cmd_body.read_all().await?.as_slice()).map_err(into_cmd_err!(CmdErrorCode::RawCodecError))?;
                service.handle_called_resp(called_resp).await;
                Ok(())
            }
        });

        let service = self.clone();
        self.cmd_server.register_cmd_handler(PackageCmdCode::ReportSn as u8, move |peer_id: PeerId, tunnel_id: CmdTunnelId, header: SnCmdHeader, mut cmd_body: CmdBodyRead| {
            let service = service.clone();
            async move {
                let report_sn = ReportSn::clone_from_slice(cmd_body.read_all().await?.as_slice()).map_err(into_cmd_err!(CmdErrorCode::RawCodecError))?;
                service.handle_report_sn(&peer_id, tunnel_id, report_sn).await;
                Ok(())
            }
        });

        let service = self.clone();
        self.cmd_server.register_cmd_handler(PackageCmdCode::SnQuery as u8, move |peer_id: PeerId, tunnel_id: CmdTunnelId, header: SnCmdHeader, mut cmd_body: CmdBodyRead| {
            let service = service.clone();
            async move {
                let query = SnQuery::clone_from_slice(cmd_body.read_all().await?.as_slice()).map_err(into_cmd_err!(CmdErrorCode::RawCodecError))?;
                service.handle_query_sn(&peer_id, tunnel_id, query).await;
                Ok(())
            }
        });
    }

    pub async fn start(self: &Arc<Self>) -> P2pResult<()> {
        self.net_manager.listen(self.local_identity.endpoints().as_slice(), None).await?;
        self.register_sn_cmd_handler();
        self.cmd_server.start();

        Ok(())
    }

    pub fn stop(&self) {
        self.stopped.store(true, atomic::Ordering::Relaxed);
    }

    pub fn is_stopped(&self) -> bool {
        self.stopped.load(atomic::Ordering::Relaxed)
    }

    pub fn local_identity_id(&self) -> P2pId {
        self.local_identity.get_id()
    }

    fn peer_manager(&self) -> &PeerManagerRef {
        &self.peer_mgr
    }

    async fn handle_call(
        &self,
        mut call_req: SnCall,
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

        let call_resp =
        if let Some(from_peer_cache) = self.peer_manager().find_peer(&call_req.from_peer_id) {
            if let Some(to_peer_cache) = self.peer_manager().find_peer(&call_req.to_peer_id) {
                // Self::call_stat_contract(to_peer_cache, &call_req);
                let from_peer_desc = if call_req.peer_info.is_none() {
                    self.peer_manager().find_peer(from_peer_id).map(|c| c.desc)
                } else {
                    call_req.peer_info.map(|info| self.cert_factory.create(&info).unwrap())
                };

                let mut reverse_eps = self.get_peer_wan_ep_with_map_port(&peer_id, from_peer_cache.map_ports.as_slice()).await;
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
                                sn_peer_id: self.local_identity_id().clone(),
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
                            called_req.reverse_endpoint_array.extend_from_slice(reverse_eps.as_slice());

                            let called_log =
                                format!("{} called-req seq({})", log_key, called_seq.value());
                            log::info!(
                                "{} will send with payload(len={}) pn_list({:?}).",
                                called_log,
                                called_req.payload.len(),
                                called_req.active_pn_list
                            );

                            self.cmd_server.send_by_all_tunnels(&PeerId::from(call_req.to_peer_id.as_slice()),
                                                                PackageCmdCode::SnCalled as u8,
                                                                self.cmd_version,
                                                                called_req.to_vec().map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?.as_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
                        }
                    } else {
                        info!("{} ignore send called req for already exists.", log_key);
                    }

                    SnCallResp {
                        seq: call_req.seq,
                        sn_peer_id: self.local_identity_id().clone(),
                        result: P2pErrorCode::Ok.into_u8(),
                        to_peer_info: Some(to_peer_cache.desc.get_encoded_cert().unwrap()),
                    }
                } else {
                    warn!("{} without from-desc.", log_key);

                    SnCallResp {
                        seq: call_req.seq,
                        sn_peer_id: self.local_identity_id().clone(),
                        result: P2pErrorCode::NotFound.into_u8(),
                        to_peer_info: None,
                    }
                }
            } else {
                warn!("{} to-peer not found.", log_key);
                SnCallResp {
                    seq: call_req.seq,
                    sn_peer_id: self.local_identity_id().clone(),
                    result: P2pErrorCode::NotFound.into_u8(),
                    to_peer_info: None,
                }
            }
        } else {
            warn!("{} from-peer not found.", log_key);
            SnCallResp {
                seq: call_req.seq,
                sn_peer_id: self.local_identity_id().clone(),
                result: P2pErrorCode::NotFound.into_u8(),
                to_peer_info: None,
            }
        };


        self.cmd_server.send_by_specify_tunnel(peer_id,
                                               tunnel_id,
                                               PackageCmdCode::SnCallResp as u8,
                                               self.cmd_version,
                                               call_resp.to_vec().map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?.as_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
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

    async fn get_peer_wan_ep(&self, peer_id: &PeerId) -> Vec<Endpoint> {
        let tunnels = self.cmd_server.get_peer_tunnels(peer_id).await;
        let mut remotes = Vec::new();
        for tunnel in tunnels.iter() {
            let tunnel = tunnel.lock().await;
            let remote = tunnel.send.remote();
            if !remotes.contains(&remote) {
                remotes.push(remote);
            }
        }
        let mut remote_ep = remotes.iter_mut().map(|remote| {
            remote.set_area(EndpointArea::Wan);
            remote.clone()
        }).collect();
        remote_ep
    }

    async fn get_peer_wan_ep_with_map_port(&self, peer_id: &PeerId, map_ports: &[(Protocol, u16)]) -> Vec<Endpoint> {
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

    async fn handle_report_sn(&self, peer_id: &PeerId, tunnel_id: CmdTunnelId, report_sn: ReportSn) -> P2pResult<()> {
        log::info!("report sn from {}.eps: {:?} map_port: {:?}", peer_id.to_base58(), report_sn.local_eps, report_sn.map_ports);
        let mut remote_ep = self.get_peer_wan_ep(peer_id).await;

        if report_sn.from_peer_id.is_some() {
            self.peer_mgr.add_or_update_peer(report_sn.from_peer_id.as_ref().unwrap(),
                                             &report_sn.peer_info.map(|info| self.cert_factory.create(&info).unwrap()),
                                             report_sn.map_ports,
                                             &report_sn.local_eps);
        }
        if let Err(e) = self.cmd_server.send_by_specify_tunnel(peer_id,
                                                               tunnel_id,
                                                               PackageCmdCode::ReportSnResp as u8,
                                                               self.cmd_version,
                                                               ReportSnResp {
                                                                   seq: report_sn.seq,
                                                                   sn_peer_id: self.local_identity_id().clone(),
                                                                   result: P2pErrorCode::Ok.into_u8(),
                                                                   peer_info: None,
                                                                   end_point_array: remote_ep,
                                                                   receipt: None,
                                                               }.to_vec().map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?.as_slice()).await {
            log::error!("send report-sn-resp failed, peer_id: {:?}, error: {:?}", peer_id, e);
        }
        Ok(())
    }

    async fn handle_query_sn(&self, peer_id: &PeerId, tunnel_id: CmdTunnelId, query: SnQuery) -> P2pResult<()> {
        let device_info = self.peer_mgr.find_peer(&query.query_id);
        let resp = if device_info.is_some() {
            let device_info = device_info.unwrap();
            let mut end_point_array = self.get_peer_wan_ep_with_map_port(&peer_id, device_info.map_ports.as_slice()).await;
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

        self.cmd_server.send_by_specify_tunnel(peer_id,
                                               tunnel_id,
                                               PackageCmdCode::SnQueryResp as u8,
                                               self.cmd_version,
                                               resp.to_vec().map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?.as_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
        Ok(())
    }
}

// #[async_trait::async_trait]
// impl TcpListenerEventListener for SnService {
//     async fn on_new_connection(&self, socket: TCPSocket) -> BdtResult<()> {
//         self.handle(socket).await
//     }
// }

pub struct SnServiceConfig {
    local_identity: P2pIdentityRef,
    identity_factory: P2pIdentityFactoryRef,
    cert_factory: P2pIdentityCertFactoryRef,
    contract: Box<dyn SnServiceContractServer + Send + Sync>,
    support_proxy: bool,
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
            support_proxy: false,
        }
    }

    pub fn set_contract(mut self, contract: Box<dyn SnServiceContractServer + Send + Sync>) -> Self {
        self.contract = contract;
        self
    }

    pub fn set_support_proxy(mut self, support_proxy: bool) -> Self {
        self.support_proxy = support_proxy;
        self
    }
}

pub async fn create_sn_service(config: SnServiceConfig) -> SnServiceRef {
    let service = SnService::new(
        config.local_identity,
        config.identity_factory,
        config.cert_factory,
        config.contract,
        config.support_proxy,
    ).await;
    service
}
