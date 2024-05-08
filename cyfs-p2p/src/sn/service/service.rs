use async_std::task;
use log::*;
use std::{
    any::Any,
    sync::{
        atomic::{self, AtomicBool},
        Arc,
    },
    time::Duration,
};

use cyfs_base::*;

use crate::{
    history::keystore::{self, Keystore},
    protocol::{*, v0::*},
    types::*,
};
use crate::executor::Executor;
use crate::sn::service::peer_manager::PeerManagerRef;
use crate::sockets::{NetListener, DataSender, SocketType, UdpDataSender, NetListenerRef};
use crate::sockets::tcp::{TcpListenerEventListener, TCPSocket};
use crate::sockets::udp::{UDPListenerEventListener, UdpPackageBox, UDPSocket};

use super::{
    call_stub::CallStub,
    peer_manager::PeerManager,
    receipt::*,
    resend_queue::{ResendQueue, ResendCallbackTrait},
};

// const TRACKER_INTERVAL: Duration = Duration::from_secs(60);
// struct CallTracker {
//     calls: HashMap<TempSeq, (u64, Instant, DeviceId)>, // <called_seq, (call_send_time, called_send_time)>
//     begin_time: Instant,
// }

pub struct SnService {
    seq_generator: TempSeqGenerator,
    key_store: Arc<Keystore>,
    local_device: LocalDeviceRef,
    stopped: AtomicBool,
    contract: Box<dyn SnServiceContractServer + Send + Sync>,

    // call_tracker: CallTracker,
    peer_mgr: PeerManagerRef,
    resend_queue: Option<ResendQueue>,
    call_stub: CallStub,
    udp_recv_buffer: usize,
    net_listener: NetListenerRef,
}

pub type SnServiceRef = Arc<SnService>;

impl SnService {
    pub async fn new(
        local_device: LocalDeviceRef,
        local_secret: PrivateKey,
        udp_recv_buffer: usize,
        contract: Box<dyn SnServiceContractServer + Send + Sync>,
    ) -> SnServiceRef {
        let key_store = Arc::new(Keystore::new(
            keystore::Config {
                // <TODO>提供配置
                active_time: Duration::from_secs(600),
                capacity: 100000,
            },
        ));
        let net_listener = NetListener::open(
            key_store.clone(),
            local_device.device().connect_info().endpoints().as_slice(),
            None,
            Duration::from_secs(30),
            udp_recv_buffer,
            false,
        ).await.unwrap();
        let mut service = SnService {
            seq_generator: TempSeqGenerator::new(),
            key_store,
            resend_queue: None,/* ResendQueue::new(thread_pool.clone(), Duration::from_millis(200), 5), */
            local_device: local_device.clone(),
            stopped: AtomicBool::new(false),
            peer_mgr: PeerManager::new(),
            call_stub: CallStub::new(),
            contract,
            // call_tracker: CallTracker {
            //     calls: Default::default(),
            //     begin_time: Instant::now()
            // }
            udp_recv_buffer,
            net_listener,
        };

        service.key_store.add_local_key(local_device.device_id().clone(),
                                        local_secret.clone(),
                                        local_device.device().desc().clone());

        let peer_manager = service.peer_manager().clone();
        let mut resend_queue = ResendQueue::new(Duration::from_millis(200), 5);
        resend_queue.set_callback(move |pkg: Arc<PackageBox>, errno: BuckyErrorCode| {
            let peer_manager = peer_manager.clone();
            async move {
                if let Some(p) = pkg.packages_no_exchange()
                    .get(0)
                    .map(| p | {
                        let p: &SnCalled = p.as_ref();
                        p
                    }) {
                    peer_manager.find_peer(&p.peer_info.desc().device_id())
                        .map(| requestor | {
                            requestor.peer_status.record(p.to_peer_id.clone(), p.call_seq, errno);
                        });
                }
            }
        });
        service.resend_queue = Some(resend_queue);
        let service_ref = Arc::new(service);

        service_ref
    }

    pub async fn start(self: &Arc<Self>) -> BuckyResult<()> {
        Executor::init(None);
        self.net_listener.set_udp_listener_event_listener(self.clone());
        self.net_listener.set_tcp_listener_event_listener(self.clone());
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
                async_std::task::sleep(Duration::from_secs(100)).await;
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

    pub(super) fn key_store(&self) -> &Keystore {
        &self.key_store
    }

    fn resend_queue(&self) -> &ResendQueue {
        self.resend_queue.as_ref().unwrap()
    }

    fn peer_manager(&self) -> &PeerManagerRef {
        &self.peer_mgr
    }

    async fn send_resp(&self, mut sender: Arc<dyn DataSender>, pkg: DynamicPackage, send_log: String) -> BuckyResult<()> {
        if let Err(e) = sender.send_dynamic_pkg(pkg).await {
            warn!("{} send failed. error: {}.", send_log, e.to_string());
            Err(e)
        } else {
            debug!("{} send ok.", send_log);
            Ok(())
        }
    }

    async fn clean_timeout_resource(&self) {
        let now = bucky_time_now();

        if let Some(drops) = self.peer_manager().try_knock_timeout(now) {
            for device in &drops {
                self.key_store().reset_peer(self.local_device.device_id(), device)
            }
        }

        self.resend_queue().try_resend(now).await;
        self.call_stub.recycle(now);
        // {
        //     let tracker = &mut self.call_tracker;
        //     if let Ordering::Greater = now.duration_since(tracker.begin_time).cmp(&TRACKER_INTERVAL) {
        //         tracker.calls.clear();
        //         tracker.begin_time = now;
        //     }
        // }
    }

    pub(super) async fn handle(&self, mut pkg_box: PackageBox, resp_sender: Arc<dyn DataSender>) -> BuckyResult<()> {
        let first_pkg = pkg_box.pop();
        if first_pkg.is_none() {
            warn!("fetch none pkg");
            return Ok(());
        }

        let send_time = bucky_time_now();
        let first_pkg = first_pkg.unwrap();
        let cmd_pkg = match first_pkg.cmd_code() {
            PackageCmdCode::Exchange => {
                let exchg = <Box<dyn Any + Send>>::downcast::<Exchange>(first_pkg.into_any()); // pkg.into_any().downcast::<Exchange>();
                if let Ok(_) = exchg {
                    self.key_store().add_key(pkg_box.key(), pkg_box.local(), pkg_box.remote());
                } else {
                    warn!("fetch exchange failed, from: {:?}.", resp_sender.remote().addr());
                    return Ok(());
                }

                match pkg_box.pop() {
                    Some(pkg) => pkg,
                    None => {
                        warn!("fetch none cmd-pkg, from: {:?}.", resp_sender.remote().addr());
                        return Ok(());
                    }
                }
            }
            _ => first_pkg,
        };

        match cmd_pkg.cmd_code() {
            PackageCmdCode::SnPing => {
                let ping_req = <Box<dyn Any + Send>>::downcast::<SnPing>(cmd_pkg.into_any());
                return if let Ok(ping_req) = ping_req {
                    self.handle_ping(
                        ping_req,
                        resp_sender.clone(),
                        send_time,
                    ).await?;
                    Ok(())
                } else {
                    warn!("fetch ping-req failed, from: {:?}.", resp_sender.remote());
                    Ok(())
                }
            }
            PackageCmdCode::SnCall => {
                let call_req = <Box<dyn Any + Send>>::downcast::<SnCall>(cmd_pkg.into_any());
                return if let Ok(call_req) = call_req {
                    self.handle_call(
                        call_req,
                        resp_sender,
                        send_time,
                    ).await;
                    Ok(())
                } else {
                    warn!("fetch sn-call failed, from: {:?}.", resp_sender.remote());
                    Ok(())
                }
            }
            PackageCmdCode::SnCalledResp => {
                let called_resp =
                    <Box<dyn Any + Send>>::downcast::<SnCalledResp>(cmd_pkg.into_any());
                return if let Ok(called_resp) = called_resp {
                    self.handle_called_resp(called_resp, Some(pkg_box.key())).await;
                    Ok(())
                } else {
                    warn!(
                        "fetch sn-called-resp failed, from: {:?}.",
                        resp_sender.remote()
                    );
                    Ok(())
                }
            }
            _ => warn!("invalid cmd-package, from: {:?}.", resp_sender.remote()),
        }
        Ok(())
    }


    async fn handle_ping(
        &self,
        ping_req: Box<SnPing>,
        resp_sender: Arc<dyn DataSender>,
        send_time: Timestamp,
    ) -> BuckyResult<()> {
        if resp_sender.local().addr().is_ipv4() {
            self.handle_ipv4_ping(ping_req, resp_sender, send_time).await
        } else {
            self.handle_ipv6_ping(ping_req, resp_sender, send_time).await
        }
    }

    async fn handle_ipv6_ping(
        &self,
        ping_req: Box<SnPing>,
        resp_sender: Arc<dyn DataSender>,
        _send_time: Timestamp,
    ) -> BuckyResult<()> {
        let from_peer_id = match ping_req.from_peer_id.as_ref() {
            Some(id) => id,
            None => resp_sender.remote_device_id()
        };

        let aes_key = resp_sender.key();

        let log_key = format!(
            "[ping from {} seq({})]",
            from_peer_id.to_string(),
            ping_req.seq.value()
        );

        info!("{}", log_key);

        let ping_resp = SnPingResp {
            seq: ping_req.seq,
            sn_peer_id: self.local_device_id().clone(),
            result: BuckyErrorCode::Ok.into_u8(),
            peer_info: None,
            end_point_array: vec![Endpoint::from((
                Protocol::Udp,
                resp_sender.remote().addr().clone(),
            ))],
            receipt: None,
        };

        self.send_resp(
            resp_sender,
            DynamicPackage::from(ping_resp),
            format!("{}", log_key),
        ).await?;

        Ok(())
    }

    async fn handle_ipv4_ping(
        &self,
        ping_req: Box<SnPing>,
        resp_sender: Arc<dyn DataSender>,
        send_time: Timestamp,
    ) -> BuckyResult<()> {
        let from_peer_id = match ping_req.from_peer_id.as_ref() {
            Some(id) => id,
            None => resp_sender.remote_device_id()
        };

        let aes_key = resp_sender.key();

        let log_key = format!(
            "[ping from {} seq({})]",
            from_peer_id.to_string(),
            ping_req.seq.value()
        );

        info!("{}", log_key);

        // let (result, endpoints, receipt) = if let Some((accept, local_receipt)) = self.ping_receipt(&ping_req, from_peer_id) {
        //     let receipt = match accept {
        //         IsAcceptClient::Refuse => {
        //             return;
        //         }
        //         IsAcceptClient::Accept(is_request_receipt) => if is_request_receipt {
        //             Some(local_receipt)
        //         } else {
        //             None
        //         }
        //     };

        //     info!("{} from-endpoint: {}", log_key, resp_sender.remote());
        //     (BuckyErrorCode::Ok as u8, vec![Endpoint::from((Protocol::Udp, resp_sender.remote().clone()))], receipt)
        // } else {
        //     (BuckyErrorCode::NotFound as u8, vec![], None)
        // };

        if !self.peer_manager().peer_heartbeat(
            from_peer_id.clone(),
            &ping_req.peer_info,
            resp_sender.clone(),
            Some(aes_key),
            send_time,
            ping_req.seq,
        ) {
            warn!("{} cache peer failed. the ping maybe is timeout.", log_key);
            return Ok(());
        };

        let ping_resp = SnPingResp {
            seq: ping_req.seq,
            sn_peer_id: self.local_device_id().clone(),
            result: BuckyErrorCode::Ok.into_u8(),
            peer_info: Some(self.local_device.device().clone()),
            end_point_array: vec![Endpoint::from((
                Protocol::Udp,
                resp_sender.remote().addr().clone(),
            ))],
            receipt: None,
        };

        self.send_resp(
            resp_sender,
            DynamicPackage::from(ping_resp),
            format!("{}", log_key),
        ).await?;
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
        mut call_req: Box<SnCall>,
        mut resp_sender: Arc<dyn DataSender>,
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
        // if let IsAcceptClient::Refuse = self.contract.verify_auth(&call_req.to_peer_id) {
        //     warn!("{} refused by contract.", log_key);
        //     send_responce(self,
        //                   resp_sender,
        //                   call_req.seq,
        //                   BuckyErrorCode::PermissionDenied,
        //                   None,
        //                   log_key.as_str()
        //     );
        //     return;
        // }

        // if let Some(cached_from) = self.peer_mgr.find_peer(from_peer_id, FindPeerReason::CallFrom(*send_time)) {
        //     if &cached_from.last_call_time > send_time {
        //         warn!("{} ignore for timeout.", log_key);
        //         return;
        //     } else {
        //         if from_peer_desc.is_none() {
        //             from_peer_desc = Some(cached_from.desc.clone());
        //         }
        //     }
        // } else {
        //     warn!("{} without from-desc.", log_key);
        //     call_result = BuckyErrorCode::NotFound;
        // };


        let call_requestor = self.peer_manager().find_peer(&call_req.from_peer_id);

        if let Some(call_requestor) = call_requestor.as_ref() {
            call_requestor.peer_status.add_record(call_req.to_peer_id.clone(), call_req.seq);
        }

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

                    if self.call_stub.insert(from_peer_id, &call_req.seq) {
                        if call_req.is_always_call || !to_peer_cache.is_wan {
                            let called_seq = self.seq_generator.generate();
                            let mut called_req = SnCalled {
                                seq: called_seq,
                                to_peer_id: call_req.to_peer_id.clone(),
                                sn_peer_id: self.local_device_id().clone(),
                                peer_info: from_peer_desc,
                                call_seq: call_req.seq,
                                call_send_time: call_req.send_time,
                                payload: SizedOwnedData::from(vec![]),
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
                            log::debug!(
                                "{} will send with payload(len={}) pn_list({:?}).",
                                called_log,
                                called_req.payload.len(),
                                called_req.active_pn_list
                            );
                            self.resend_queue().send(
                                to_peer_cache.sender.clone(),
                                DynamicPackage::from(called_req),
                                called_seq.value(),
                                called_log,
                            ).await;
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

        match &call_resp.result {
            0 => { /* wait confirm */ },

            _ => {
                if let Some(call_requestor) = call_requestor.as_ref() {
                    call_requestor.peer_status.record(call_req.to_peer_id.clone(), call_req.seq, BuckyErrorCode::from(call_resp.result as u16));
                }
            }

        }

        self.send_resp(
            resp_sender,
            DynamicPackage::from(call_resp),
            format!("{} call-resp", log_key),
        );
    }

    async fn handle_called_resp(&self, called_resp: Box<SnCalledResp>, _aes_key: Option<&MixAesKey>) {
        info!("called-resp seq {}.", called_resp.seq.value());
        self.resend_queue().confirm_pkg(called_resp.seq.value()).await;

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
}

#[async_trait::async_trait]
impl UDPListenerEventListener for SnService {
    async fn on_udp_package_box(&self, socket: Arc<UDPSocket>, package_box: UdpPackageBox) {
        let remote_ep = package_box.remote().clone();
        let pkg: PackageBox = package_box.into();
        let resp_sender = Arc::new(UdpDataSender::new(socket,
                                             remote_ep,
                                             pkg.remote().clone(),
                                             pkg.local().clone(),
                                             pkg.key().clone()));
        if let Err(e) = self.handle(pkg, resp_sender).await {
            error!("handle udp package failed, e:{}", e);
        }
    }

    async fn on_udp_raw_data(&self, data: &[u8], context: (Arc<UDPSocket>, DeviceId, MixAesKey, Endpoint)) -> Result<(), BuckyError> {
        Ok(())
    }
}

#[async_trait::async_trait]
impl TcpListenerEventListener for SnService {
    async fn on_new_connection(&self, socket: Arc<TCPSocket>, first_box: PackageBox) -> BuckyResult<()> {
        self.handle(first_box, socket).await
    }
}
