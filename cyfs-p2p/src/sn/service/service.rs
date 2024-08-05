use log::*;
use std::{
    any::Any,
    sync::{
        atomic::{self, AtomicBool},
        Arc,
    },
    time::Duration,
};
use std::net::ToSocketAddrs;
use bucky_crypto::PrivateKey;
use bucky_error::{BuckyError, BuckyErrorCode};
use bucky_objects::{DeviceId, Endpoint, EndpointArea, endpoints_to_string, NamedObject, Protocol};
use bucky_raw_codec::{RawFrom, SizedOwnedData};
use bucky_time::bucky_time_now;
use bucky_rustls::ServerCertResolver;

use crate::{
    history::keystore::{self, Keystore},
    protocol::{*, v0::*},
    types::*,
};
use crate::error::{BdtErrorCode, BdtResult, into_bdt_err};
use crate::executor::Executor;
use crate::finder::{DeviceCache, DeviceCacheConfig};
use crate::history::keystore::EncryptedKey;
use crate::sn::service::peer_connection::PeerConnection;
use crate::sn::service::peer_manager::PeerManagerRef;
use crate::sockets::{NetListener, NetListenerRef, QuicListenerEventListener, QuicSocket};
use crate::sockets::tcp::{TcpListenerEventListener, TCPSocket};

use super::{
    call_stub::CallStub,
    peer_manager::PeerManager,
    receipt::*,
};

// const TRACKER_INTERVAL: Duration = Duration::from_secs(60);
// struct CallTracker {
//     calls: HashMap<TempSeq, (u64, Instant, DeviceId)>, // <called_seq, (call_send_time, called_send_time)>
//     begin_time: Instant,
// }

pub struct SnService {
    seq_generator: TempSeqGenerator,
    device_cache: Arc<DeviceCache>,
    local_device: LocalDeviceRef,
    stopped: AtomicBool,
    contract: Box<dyn SnServiceContractServer + Send + Sync>,

    // call_tracker: CallTracker,
    peer_mgr: PeerManagerRef,
    call_stub: CallStub,
    net_listener: NetListenerRef,
}

pub type SnServiceRef = Arc<SnService>;

impl SnService {
    pub async fn new(
        local_device: LocalDeviceRef,
        contract: Box<dyn SnServiceContractServer + Send + Sync>,
    ) -> SnServiceRef {
        Executor::init(None);
        let device_cache = Arc::new(DeviceCache::new(&DeviceCacheConfig {
            expire: Duration::from_secs(240),
            capacity: 10240,
        }, None));
        let cert_resolver = ServerCertResolver::new();
        cert_resolver.add_device(local_device.device().clone(), local_device.key().clone());
        let net_listener = NetListener::open(
            device_cache.clone(),
            cert_resolver,
            local_device.device().connect_info().endpoints().as_slice(),
            None,
            Duration::from_secs(30),
        ).await.unwrap();
        let mut service = SnService {
            seq_generator: TempSeqGenerator::new(),
            local_device: local_device.clone(),
            stopped: AtomicBool::new(false),
            peer_mgr: PeerManager::new(),
            call_stub: CallStub::new(),
            contract,
            // call_tracker: CallTracker {
            //     calls: Default::default(),
            //     begin_time: Instant::now()
            // }
            net_listener,
            device_cache,
        };

        let peer_manager = service.peer_manager().clone();
        let service_ref = Arc::new(service);

        service_ref
    }

    pub async fn start(self: &Arc<Self>) -> BdtResult<()> {
        let this = self.clone();
        self.net_listener.set_udp_listener_event_listener(Arc::new(move |socket: QuicSocket| {
            let this = this.clone();
            async move {
                let id = this.peer_mgr.generate_conn_id();
                let tmp = this.clone();
                match PeerConnection::accept(id, socket, move |conn_id: TempSeq, cmd_code: PackageCmdCode, cmd_body: Vec<u8>| {
                    let this = tmp.clone();
                    async move {
                        this.handle(cmd_code, cmd_body.as_slice(), conn_id).await
                    }
                }).await {
                    Ok(conn) => {
                        let peer_desc = this.device_cache.get(conn.remote_device_id()).await;
                        if peer_desc.is_some() {
                            this.peer_mgr.add_peer_connection(peer_desc.unwrap(), conn);
                        }
                        Ok(())
                    }
                    Err(e) => {
                        log::error!("accept error: {:?}", e);
                        Err(e)
                    }
                }
            }
        }));
        // self.net_listener.set_tcp_listener_event_listener(Arc::new(move |socket: TCPSocket| {
        //
        // }));
        self.net_listener.start();

        // 清理过期数据
        let service = self.clone();
        Executor::spawn(async move {
            loop {
                {
                    if service.is_stopped() {
                        return;
                    }
                    service.clean_timeout_resource().await;
                }
                crate::runtime::sleep(Duration::from_secs(100)).await;
            }
        });

        Ok(())
    }

    pub fn stop(&self) {
        self.stopped.store(true, atomic::Ordering::Relaxed);
    }

    pub fn is_stopped(&self) -> bool {
        self.stopped.load(atomic::Ordering::Relaxed)
    }

    pub fn local_device_id(&self) -> &DeviceId {
        &self.local_device.device_id()
    }

    fn peer_manager(&self) -> &PeerManagerRef {
        &self.peer_mgr
    }

    async fn clean_timeout_resource(&self) {
        let now = bucky_time_now();

        self.call_stub.recycle(now);
        // {
        //     let tracker = &mut self.call_tracker;
        //     if let Ordering::Greater = now.duration_since(tracker.begin_time).cmp(&TRACKER_INTERVAL) {
        //         tracker.calls.clear();
        //         tracker.begin_time = now;
        //     }
        // }
    }

    pub(super) async fn handle(&self, cmd_code: PackageCmdCode, cmd_body: &[u8], conn_id: TempSeq) -> BdtResult<()> {
        match cmd_code {
            PackageCmdCode::SnCall => {
                let call_req = SnCall::clone_from_slice(cmd_body).map_err(into_bdt_err!(BdtErrorCode::RawCodecError))?;
                self.handle_call(
                    call_req,
                    conn_id,
                    bucky_time_now(),
                ).await;
            }
            PackageCmdCode::SnCalledResp => {
                let called_resp = SnCalledResp::clone_from_slice(cmd_body).map_err(into_bdt_err!(BdtErrorCode::RawCodecError))?;
                self.handle_called_resp(called_resp).await;
            }
            PackageCmdCode::ReportSn => {
                let report_sn = ReportSn::clone_from_slice(cmd_body).map_err(into_bdt_err!(BdtErrorCode::RawCodecError))?;
                self.handle_report_sn(&conn_id, report_sn).await;
                // self.peer_mgr.report_sn(report_sn).await;
            }
            PackageCmdCode::SnQuery => {
                let query = SnQuery::clone_from_slice(cmd_body).map_err(into_bdt_err!(BdtErrorCode::RawCodecError))?;
                self.handle_query_sn(&conn_id, query).await;
            }
            _ => warn!("invalid cmd-package, conn: {:?} cmd_code {:?}.", conn_id, cmd_code),
        }
        Ok(())
    }


    // fn verify_receipt_sign(
    //     &self,
    //     client_desc: &DeviceDesc,
    //     signed_receipt: &Option<ReceiptWithSignature>) -> bool {
    //     match signed_receipt {
    //         None => false,
    //         Some(receipt) => {
    //             receipt.receipt().verify(sn_peerid, receipt.signature(), client_desc)
    //         }
    //     }
    // }

    // // 处理ping服务证明
    // fn ping_receipt(&self, ping_req: &SnPing, from_id: &DeviceId) -> Option<(IsAcceptClient, SnServiceReceipt)> {
    //     let mut cache_peer = self.peer_mgr.find_peer(from_id, FindPeerReason::Other);

    //     let (device, local_receipt, last_receipt_request_time) = match &cache_peer {
    //         Some(cache) => (&cache.desc, cache.receipt.clone(), cache.last_receipt_request_time),
    //         None => {
    //             let dev = match ping_req.peer_info.as_ref() {
    //                 Some(dev) => dev,
    //                 None => return None,
    //             };
    //             (
    //                 dev,
    //                 SnServiceReceipt::default(),
    //                 ReceiptRequestTime::None
    //             )
    //         }
    //     };

    //     let is_verify_ok = self.verify_receipt_sign(ping_req.peer_info.desc(), &ping_req.receipt);
    //     let client_receipt = if is_verify_ok { &ping_req.receipt } else { &None };
    //     let check_receipt = self.contract.check_receipt(device, &local_receipt, client_receipt, &last_receipt_request_time);

    //     let is_reset_receipt = if is_verify_ok {
    //         match cache_peer.as_mut() {
    //             Some(cache_peer) => match last_receipt_request_time {
    //                 ReceiptRequestTime::Wait(t) => {
    //                     cache_peer.last_receipt_request_time = ReceiptRequestTime::Last(t);
    //                     // 重置统计计数
    //                     true
    //                 }
    //                 _ => false
    //             }
    //             None => false
    //         }
    //     } else {
    //         false
    //     };

    //     let is_request_receipt = match check_receipt {
    //         IsAcceptClient::Refuse => {
    //             warn!("[ping from {} seq({})] refused by contract.", from_id, ping_req.seq.value());
    //             return Some((IsAcceptClient::Refuse, local_receipt))
    //         },
    //         IsAcceptClient::Accept(r) => r,
    //     };

    //     if let Some(cache_peer) = cache_peer {
    //         if is_reset_receipt {
    //             cache_peer.receipt.start_time = SystemTime::now();
    //             cache_peer.receipt.ping_count = 0;
    //             cache_peer.receipt.ping_resp_count = 0;
    //             cache_peer.receipt.called_count = 0;
    //             cache_peer.receipt.call_peer_count = 0;
    //             cache_peer.call_peers.clear();
    //         }

    //         if cache_peer.last_ping_seq != ping_req.seq {
    //             cache_peer.receipt.ping_count += 1;
    //             cache_peer.receipt.ping_resp_count += 1;
    //             cache_peer.last_ping_seq = ping_req.seq;
    //         }

    //         if is_request_receipt {
    //             if let ReceiptRequestTime::Last(_) = cache_peer.last_receipt_request_time { // 一次新的请求
    //                 cache_peer.last_receipt_request_time = ReceiptRequestTime::Wait(SystemTime::now());
    //             }
    //         }
    //     }

    //     Some((IsAcceptClient::Accept(is_request_receipt), local_receipt))
    // }

    async fn handle_call(
        &self,
        mut call_req: SnCall,
        conn_id: TempSeq,
        _send_time: Timestamp,
    ) {
        let from_peer_id = &call_req.from_peer_id;
        let log_key = format!(
            "[call {}->{} seq({})]",
            from_peer_id.to_string(),
            call_req.to_peer_id.to_string(),
            call_req.seq.value()
        );
        info!("{}.", log_key);

        let call_requestor = self.peer_manager().find_peer(&call_req.from_peer_id);

        let call_resp =
            if let Some(to_peer_cache) = self.peer_manager().find_peer(&call_req.to_peer_id) {
                // Self::call_stat_contract(to_peer_cache, &call_req);
                let from_peer_desc = if call_req.peer_info.is_none() {
                    self.peer_manager().find_peer(from_peer_id).map(|c| c.desc)
                } else {
                    call_req.peer_info
                };

                if let Some(from_peer_desc) = from_peer_desc {
                    info!(
                        "{} to-peer found, endpoints: {}, always_call: {}, to-peer.is_wan: {}.",
                        log_key,
                        endpoints_to_string(to_peer_cache.desc.connect_info().endpoints()),
                        call_req.is_always_call,
                        to_peer_cache.is_wan
                    );

                    if self.call_stub.insert(from_peer_id, &call_req.tunnel_id) {
                        if call_req.is_always_call || !to_peer_cache.is_wan {
                            let called_seq = self.seq_generator.generate();
                            let mut called_req = SnCalled {
                                seq: called_seq,
                                to_peer_id: call_req.to_peer_id.clone(),
                                sn_peer_id: self.local_device_id().clone(),
                                peer_info: from_peer_desc,
                                tunnel_id: call_req.tunnel_id,
                                call_send_time: call_req.send_time,
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

                            let called_log =
                                format!("{} called-req seq({})", log_key, called_seq.value());
                            log::info!(
                                "{} will send with payload(len={}) pn_list({:?}).",
                                called_log,
                                called_req.payload.len(),
                                called_req.active_pn_list
                            );
                            for conn_id in to_peer_cache.conn_list.iter() {
                                let conn = self.peer_mgr.find_connection(*conn_id);
                                if conn.is_some() {
                                    let mut peer_conn = conn.as_ref().unwrap().lock().await;
                                    if let Err(e) = peer_conn.send(Package::new(PackageCmdCode::SnCalled, called_req.clone())).await {
                                        log::info!("send called-req failed, conn_id: {:?}, error: {:?}", conn_id, e);
                                        continue;
                                    }
                                }
                            }
                            // self.call_tracker.calls.insert(called_seq, (call_req.send_time, Instant::now(), call_req.to_peer_id.clone()));
                        }
                    } else {
                        info!("{} ignore send called req for already exists.", log_key);
                    }

                    SnCallResp {
                        seq: call_req.seq,
                        sn_peer_id: self.local_device_id().clone(),
                        result: BuckyErrorCode::Ok.into_u8(),
                        to_peer_info: Some(to_peer_cache.desc),
                    }
                } else {
                    warn!("{} without from-desc.", log_key);

                    SnCallResp {
                        seq: call_req.seq,
                        sn_peer_id: self.local_device_id().clone(),
                        result: BuckyErrorCode::NotFound.into_u8(),
                        to_peer_info: None,
                    }
                }
            } else {
                warn!("{} to-peer not found.", log_key);
                SnCallResp {
                    seq: call_req.seq,
                    sn_peer_id: self.local_device_id().clone(),
                    result: BuckyErrorCode::NotFound.into_u8(),
                    to_peer_info: None,
                }
            };

        let conn = self.peer_mgr.find_connection(conn_id);
        if conn.is_some() {
            let mut conn = conn.as_ref().unwrap().lock().await;
            if let Err(e) = conn.send(Package::new(PackageCmdCode::SnCallResp, call_resp)).await {
                log::info!("send call-resp failed, conn_id: {:?}, error: {:?}", conn_id, e);
            }
        }
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

    async fn handle_report_sn(&self, conn_id: &TempSeq, report_sn: ReportSn) {
        let conn = self.peer_mgr.find_connection(*conn_id);
        assert!(conn.is_some());
        let mut peer_conn = conn.as_ref().unwrap().lock().await;
        log::info!("report sn from {}.", peer_conn.remote_device_id().to_string());
        if report_sn.peer_info.is_some() {

        }
        if report_sn.from_peer_id.is_some() {
            self.peer_mgr.update_peer(report_sn.from_peer_id.as_ref().unwrap(), &report_sn.peer_info, report_sn.tcp_map_port, report_sn.udp_map_port, &report_sn.local_eps);
        }
        let mut remote_ep = peer_conn.remote().clone();
        remote_ep.set_area(EndpointArea::Wan);
        if let Err(e) = peer_conn.send(Package::new(PackageCmdCode::ReportSnResp, ReportSnResp {
            seq: report_sn.seq,
            sn_peer_id: self.local_device_id().clone(),
            result: BuckyErrorCode::Ok.into_u8(),
            peer_info: None,
            end_point_array: vec![remote_ep],
            receipt: None,
        })).await {
            log::error!("send report-sn-resp failed, conn_id: {:?}, error: {:?}", conn_id, e);
        }
    }

    async fn handle_query_sn(&self, conn_id: &TempSeq, query: SnQuery) {
        let device_info = self.peer_mgr.find_peer(&query.query_id);
        let resp = if device_info.is_some() {
            let device_info = device_info.unwrap();
            let mut end_point_array = Vec::new();
            for conn_id in device_info.conn_list.iter().rev() {
                let conn = self.peer_mgr.find_connection(*conn_id);
                if conn.is_some() {
                    let mut peer_conn = conn.as_ref().unwrap().lock().await;
                    let remote_ep = peer_conn.remote().clone();
                    if device_info.tcp_map_port.is_some() {
                        let mut map_ep = Endpoint::from((Protocol::Tcp, remote_ep.addr().ip(), device_info.tcp_map_port.unwrap()));
                        map_ep.set_area(EndpointArea::Wan);
                    }
                    if device_info.udp_map_port.is_some() {
                        let mut map_ep = Endpoint::from((Protocol::Udp, remote_ep.addr().ip(), device_info.udp_map_port.unwrap()));
                        map_ep.set_area(EndpointArea::Wan);
                        end_point_array.push(map_ep);
                    }
                    let mut remote_ep = peer_conn.remote().clone();
                    remote_ep.set_area(EndpointArea::Wan);
                    end_point_array.push(remote_ep);
                    end_point_array.extend_from_slice(device_info.local_eps.iter().map(|v| v.value().clone()).collect::<Vec<_>>().as_slice());
                }
            }
            SnQueryResp {
                seq: query.seq,
                peer_info: Some(device_info.desc),
                end_point_array,
            }
        } else {
            SnQueryResp {
                seq: query.seq,
                peer_info: None,
                end_point_array: vec![],
            }
        };

        let conn = self.peer_mgr.find_connection(*conn_id);
        assert!(conn.is_some());
        let mut peer_conn = conn.as_ref().unwrap().lock().await;
        log::info!("query sn from {}.", peer_conn.remote_device_id().to_string());
        if let Err(e) = peer_conn.send(Package::new(PackageCmdCode::SnQueryResp, resp)).await {
            log::error!("send query-sn-resp failed, conn_id: {:?}, error: {:?}", conn_id, e);
        }
    }
}

// #[async_trait::async_trait]
// impl TcpListenerEventListener for SnService {
//     async fn on_new_connection(&self, socket: TCPSocket) -> BdtResult<()> {
//         self.handle(socket).await
//     }
// }
