use crate::p2p_identity::{EncodedP2pIdentityCert, P2pId, P2pIdentityCertFactoryRef};
use crate::protocol::{v0::*, *};
use crate::types::TunnelId;
use std::{
    collections::{HashMap, hash_map::Entry},
    sync::{
        Arc, Mutex,
        atomic::{self, AtomicU32},
    },
    time::SystemTime,
};

pub trait ServiceAppraiser: Send + Sync {
    // 对SN服务进行评分，可以依据本地记录的服务清单和SN提供的服务清单作对比进行评价；
    // 因为客户端向SN提供的服务清单可能丢失，所以还要参照上次提供给SN的服务清单：
    // local_receipt：从上次向SN提供服务清单后产生的服务清单
    // last_receipt: 上次向SN提供的可能丢失的服务清单
    fn appraise(
        &self,
        sn: &EncodedP2pIdentityCert,
        local_receipt: &Option<SnServiceReceipt>,
        last_receipt: &Option<SnServiceReceipt>,
        receipt_from_sn: &Option<SnServiceReceipt>,
    ) -> SnServiceGrade;
}

struct Contract {
    sn_peerid: P2pId,
    sn: EncodedP2pIdentityCert,
    stat: Mutex<ContractStat>,
    wait_seq: AtomicU32,
    appraiser: Arc<Box<dyn ServiceAppraiser>>,
    cert_factory: P2pIdentityCertFactoryRef,
}

#[derive(Clone)]
struct CallPeerStat {
    peerid: P2pId,
    last_seq: TunnelId,
    is_connect_success: bool,
}

struct ContractStat {
    commit_receipt_start_time: SystemTime,
    last_receipt: SnServiceReceipt,
    receipt: SnServiceReceipt,
    last_call_peers: HashMap<P2pId, CallPeerStat>,
    call_peers: HashMap<P2pId, CallPeerStat>,
}

impl Contract {
    fn on_ping_resp(&self, resp: &SnPingResp, rto: u16) {
        if let Ok(wait_seq) = self.wait_seq.compare_exchange(
            resp.seq.value(),
            0,
            atomic::Ordering::SeqCst,
            atomic::Ordering::SeqCst,
        ) {
            if wait_seq != 0 {
                // 统计并获取当前服务清单
                let (receipt, last_receipt) = {
                    let mut stat = self.stat.lock().unwrap();
                    let receipt = &mut stat.receipt;
                    receipt.ping_resp_count += 1;
                    if rto > 0 {
                        receipt.rto = ((receipt.rto as u32 * 7 + rto as u32) / 8) as u16;
                    }

                    match resp.receipt.as_ref() {
                        Some(_) => {
                            let last_receipt = &mut stat.last_receipt;
                            let cloned_last_receipt = match last_receipt.version {
                                SnServiceReceiptVersion::Invalid => None,
                                SnServiceReceiptVersion::Current => Some((*last_receipt).clone()),
                            };
                            (Some(stat.receipt.clone()), cloned_last_receipt)
                        }
                        None => (None, None),
                    }
                };

                if let Some(sn_receipt) = resp.receipt.as_ref() {
                    let grade = self.appraiser.appraise(
                        &self.sn,
                        &receipt,
                        &last_receipt,
                        &Some(sn_receipt.clone()),
                    );
                    let mut stat = self.stat.lock().unwrap();
                    stat.receipt.grade = grade;
                    stat.commit_receipt_start_time = sn_receipt.start_time;
                }
            }
        }
    }

    fn on_called(&self, called: &SnCalled, seq: TunnelId, call_time: SystemTime) {
        let now = SystemTime::now();
        let mut stat = self.stat.lock().unwrap();
        let receipt = &mut stat.receipt;
        if now > call_time {
            let delay = now.duration_since(call_time).unwrap().as_millis() as u16;
            receipt.call_delay = ((receipt.call_delay as u32 * 7 + delay as u32) / 8) as u16
        }

        let peer = match self.cert_factory.create(&called.peer_info) {
            Ok(resp) => resp,
            Err(e) => {
                log::error!("parse peer failed {}", e);
                return;
            }
        };
        let (called_inc, call_peer_inc) = match stat.call_peers.entry(peer.get_id()) {
            Entry::Occupied(exist) => {
                let exist = exist.into_mut();
                if exist.last_seq != seq {
                    exist.last_seq = seq;
                    (1, 0)
                } else {
                    (0, 0)
                }
            }
            Entry::Vacant(entry) => {
                let init_stat = CallPeerStat {
                    peerid: self
                        .cert_factory
                        .create(&called.peer_info)
                        .unwrap()
                        .get_id(),
                    last_seq: seq,
                    is_connect_success: false,
                };
                entry.insert(init_stat);
                (1, 1)
            }
        };

        stat.receipt.called_count += called_inc;
        stat.receipt.call_peer_count += call_peer_inc;
    }
    //
    // fn prepare_receipt(&self, ping_pkg: &mut ReportSn, now_abs: SystemTime, secret: &PrivateKey) {
    //     let mut stat = self.stat.lock().unwrap();
    //     if stat.commit_receipt_start_time > UNIX_EPOCH && stat.receipt.grade.is_accept() {
    //         let commit_receipt_start_time = stat.commit_receipt_start_time;
    //         if stat.last_receipt.version != SnServiceReceiptVersion::Invalid &&
    //             commit_receipt_start_time <= stat.last_receipt.start_time {
    //             stat.last_receipt.grade = stat.receipt.grade;
    //             stat.last_receipt.rto = stat.receipt.rto;
    //             stat.last_receipt.ping_count += stat.receipt.ping_count;
    //             stat.last_receipt.ping_resp_count += stat.receipt.ping_resp_count;
    //             stat.last_receipt.called_count += stat.receipt.called_count;
    //             stat.last_receipt.call_delay = stat.receipt.call_delay;
    //
    //             // 合并call peer
    //             let mut add_succ_peer_count = 0u32;
    //             let mut add_peer_ount = 0u32;
    //             let mut cur_call_peers = Default::default();
    //             std::mem::swap(&mut stat.call_peers, &mut cur_call_peers);
    //             for cur in cur_call_peers.values() {
    //                 match stat.last_call_peers.entry(cur.peerid.clone()) {
    //                     Entry::Occupied(entry) => {
    //                         let mut last = entry.into_mut();
    //                         last.last_seq = cur.last_seq;
    //                         if cur.is_connect_success && !last.is_connect_success {
    //                             last.is_connect_success = true;
    //                             add_succ_peer_count += 1;
    //                         }
    //                     }
    //                     Entry::Vacant(entry) => {
    //                         entry.insert((*cur).clone());
    //                         add_peer_ount += 1;
    //                         if cur.is_connect_success {
    //                             add_succ_peer_count += 1;
    //                         }
    //                     }
    //                 }
    //             }
    //
    //             let last_receipt = &mut stat.last_receipt;
    //             last_receipt.call_peer_count += add_peer_ount;
    //             last_receipt.connect_peer_count += add_succ_peer_count;
    //         } else {
    //             let mut cur_call_peers = Default::default();
    //             std::mem::swap(&mut stat.call_peers, &mut cur_call_peers);
    //             std::mem::swap(&mut stat.last_call_peers, &mut cur_call_peers);
    //             stat.last_receipt = stat.receipt.clone();
    //         }
    //
    //         if let Ok(d) = now_abs.duration_since(stat.last_receipt.start_time) {
    //             stat.last_receipt.duration = d;
    //         }
    //
    //         // 重置正在进行的统计
    //         stat.receipt.ping_count = 0;
    //         stat.receipt.ping_resp_count = 0;
    //         stat.receipt.called_count = 0;
    //         stat.receipt.call_peer_count = 0;
    //         stat.receipt.connect_peer_count = 0;
    //         stat.receipt.start_time = now_abs;
    //
    //         // 签名
    //         let sign = match stat.last_receipt.sign(&ping_pkg.sn_peer_id, secret) {
    //             Ok(s) => s,
    //             Err(e) => {
    //                 log::warn!("sign for receipt failed, err: {:?}", e);
    //                 return;
    //             }
    //         };
    //         ping_pkg.receipt = Some(ReceiptWithSignature::from((stat.last_receipt.clone(), sign)));
    //
    //         stat.commit_receipt_start_time = UNIX_EPOCH;
    //     }
    // }

    fn will_ping(&self, seq: u32) {
        self.wait_seq.store(seq, atomic::Ordering::SeqCst);
        self.stat.lock().unwrap().receipt.ping_count += 1;
    }
}
