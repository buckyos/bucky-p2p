use std::{
    collections::{HashMap, hash_map},
    time::Duration,
    sync::{Arc, atomic::{AtomicU64, Ordering}}
};
use std::net::SocketAddr;
use std::sync::Mutex;
use bucky_objects::{Device, DeviceId, Endpoint, NamedObject};
use bucky_time::bucky_time_now;
use crate::{runtime, types::*};
use crate::error::{bdt_err, BdtError, BdtErrorCode, BdtResult};
use crate::executor::Executor;
use crate::sn::service::peer_connection::PeerConnection;
use crate::sn::service::statistic::{PeerStatus, StatisticManager};
use crate::sockets::QuicSocket;

struct Config {
    pub client_ping_interval: Duration,
    pub client_ping_timeout: Duration
}

impl Default for Config {
    fn default() -> Self {
        Self {
            client_ping_interval: Duration::from_millis(25000),
            client_ping_timeout: Duration::from_secs(300)
        }
    }
}


pub struct FoundPeer {
    pub desc: Device,
    pub conn_id: TempSeq,
    pub is_wan: bool,
    pub peer_status: PeerStatus,
}


#[derive(Clone)]
pub(crate) struct CachedPeerInfo {
    pub conn_list: Vec<TempSeq>,
    pub desc: Device,
    pub map_port: Option<u16>,
    pub last_send_time: Timestamp,
    pub last_call_time: Timestamp,
    pub local_eps: Arc<mini_moka::sync::Cache<String, Endpoint>>,
    pub is_wan: bool,
    // pub call_peers: HashMap<DeviceId, TempSeq>, // <peerid, last_call_seq>
    // pub receipt: SnServiceReceipt,
    // pub last_receipt_request_time: ReceiptRequestTime,
}

fn has_wan_endpoint(desc: &Device) -> bool {
    for ep in desc.connect_info().endpoints() {
        if ep.is_static_wan() {
            return true;
        }
    }
    false
}

fn contain_addr(dev: &Device, addr: &SocketAddr) -> bool {
    let endpoints: &Vec<Endpoint> = dev.connect_info().endpoints();
    for endpoint in endpoints {
        if endpoint.addr() == addr {
            return true;
        }
    }
    false
}

impl CachedPeerInfo {
    fn new(
        desc: Device,
        conn_id: TempSeq,
        send_time: Timestamp) -> CachedPeerInfo {
        CachedPeerInfo {
            conn_list: vec![conn_id],
            is_wan: has_wan_endpoint(&desc),
            desc,
            map_port: None,
            last_send_time: send_time,
            last_call_time: 0,
            // call_peers: Default::default(),
            // receipt: Default::default(),
            // last_receipt_request_time: ReceiptRequestTime::None,
            local_eps: Arc::new(mini_moka::sync::CacheBuilder::new(100).time_to_idle(Duration::from_secs(1800)).build()),
        }
    }

    fn add_conn(&mut self, conn_id: TempSeq) {
        self.conn_list.push(conn_id);
    }

    fn remove_conn(&mut self, conn_id: TempSeq) {
        self.conn_list.retain(|c| c != &conn_id);
    }

    fn conn_count(&self) -> usize {
        self.conn_list.len()
    }

    fn update_desc(&mut self, desc: &Device) -> BdtResult<bool> {
        match desc.signs().body_signs() {
            Some(sigs) if !sigs.is_empty() => {
                if let Some(old_sigs) = self.desc.signs().body_signs() {
                    if let Some(old_sig) = old_sigs.get(0) {
                        let new_sig = sigs.get(0).unwrap();
                        match new_sig.sign_time().cmp(&old_sig.sign_time()) {
                            std::cmp::Ordering::Equal => return Ok(false), // 签名时间不变，不更新
                            std::cmp::Ordering::Less => return Err(bdt_err!(BdtErrorCode::Expired, "sign expired")), // 签名时间更早，忽略
                            std::cmp::Ordering::Greater => {}
                        }
                    }
                }
            },
            _ => match self.desc.signs().body_signs() {
                Some(sigs) if !sigs.is_empty() => return Err(bdt_err!(BdtErrorCode::NotMatch, "attempt update signed-object with no-signed")), // 未签名不能取代签名device信息，要等被淘汰后才生效
                _ => {},
            }
        };

        self.desc = desc.clone();
        self.is_wan = has_wan_endpoint(desc);
        Ok(true)
    }

    fn update_local_eps(&mut self, local_ips: &Vec<Endpoint>) {
        for ip in local_ips {
            self.local_eps.insert(ip.to_string(), ip.clone());
        }
    }
}

struct Peers {
    active_peers: HashMap<DeviceId, CachedPeerInfo>,
    knock_peers: HashMap<DeviceId, CachedPeerInfo>,
}

impl Peers {
    fn find_peer(&mut self, peerid: &DeviceId, reason: FindPeerReason) -> Option<&mut CachedPeerInfo> {
        let found_cache = match self.active_peers.get_mut(peerid) {
            Some(p) => {
                Some(p)
            },
            None => match self.knock_peers.get_mut(peerid) {
                Some(p) => Some(p),
                None => None
            }
        };

        if let Some(p) = found_cache {
            match reason {
                FindPeerReason::CallFrom(t) => {
                    if t > p.last_call_time {
                        p.last_call_time = t;
                    }
                    Some(p)
                },
                FindPeerReason::Other => {
                    Some(p)
                }
            }
        } else {
            None
        }
    }
}


pub struct PeerManager {
    config: Config,
    statistic_manager: &'static StatisticManager,
    conn_cache: Mutex<HashMap<TempSeq, (DeviceId, Arc<runtime::Mutex<PeerConnection>>)>>,
    device_conn_map: Mutex<HashMap<DeviceId, CachedPeerInfo>>,
    conn_id_generator: TempSeqGenerator,
}
pub type PeerManagerRef = Arc<PeerManager>;

enum FindPeerReason {
    CallFrom(Timestamp),
    Other,
}


impl PeerManager {
    pub fn new() -> PeerManagerRef {
        Arc::new(PeerManager {
            config: Default::default(),
            statistic_manager: StatisticManager::get_instance(),
            conn_cache: Mutex::new(Default::default()),
            device_conn_map: Mutex::new(Default::default()),
            conn_id_generator: TempSeqGenerator::new(),
        })
    }

    pub fn generate_conn_id(&self) -> TempSeq {
        self.conn_id_generator.generate()
    }

    pub fn add_peer_connection(self: &Arc<Self>, peer_desc: Device, mut conn: PeerConnection) {
        let conn_id = conn.conn_id();
        let remote_id = conn.remote_device_id().clone();
        let recv_handle = conn.take_recv_handle().unwrap();
        let conn = Arc::new(runtime::Mutex::new(conn));
        let mut conn_cache = self.conn_cache.lock().unwrap();
        conn_cache.insert(conn_id, (remote_id.clone(), conn.clone()));
        let mut device_conn_map = self.device_conn_map.lock().unwrap();
        if !device_conn_map.contains_key(&remote_id) {
            device_conn_map.insert(remote_id.clone(), CachedPeerInfo::new(peer_desc, conn_id, bucky_time_now()));
        } else {
            let mut peer_info = device_conn_map.get_mut(&remote_id).unwrap();
            let _ = peer_info.update_desc(&peer_desc);
            peer_info.add_conn(conn_id);
        }

        let this = self.clone();
        Executor::spawn_ok(async move {
            recv_handle.await;
            this.remove_peer_connection(conn_id);
        });
    }

    pub fn remove_peer_connection(&self, conn_id: TempSeq) {
        let mut conn_cache = self.conn_cache.lock().unwrap();
        if let Some(conn) = conn_cache.remove(&conn_id) {
            let remote_id = conn.0.clone();
            let mut device_conn_map = self.device_conn_map.lock().unwrap();
            if let Some(conns) = device_conn_map.get_mut(&remote_id) {;
                conns.remove_conn(conn_id);
                if conns.conn_list.len() == 0 {
                    device_conn_map.remove(&remote_id);
                }
            }
        }
    }

    pub fn update_peer(&self, device_id: &DeviceId, device: &Option<Device>, map_port: Option<u16>, local_eps: &Vec<Endpoint>) {
        let mut device_conn_map = self.device_conn_map.lock().unwrap();
        if let Some(peer) = device_conn_map.get_mut(device_id) {
            if let Some(device) = device {
                peer.update_desc(device);
            }
            peer.map_port = map_port;
            peer.update_local_eps(local_eps);
        }
    }

    pub fn find_connection(&self, conn_id: TempSeq) -> Option<Arc<runtime::Mutex<PeerConnection>>> {
        let conn_cache = self.conn_cache.lock().unwrap();
        conn_cache.get(&conn_id).map(|c| c.1.clone())
    }

    pub fn find_connections(&self, device_id: &DeviceId) -> Vec<Arc<runtime::Mutex<PeerConnection>>> {
        let conn_cache = self.conn_cache.lock().unwrap();
        let device_conn_map = self.device_conn_map.lock().unwrap();
        device_conn_map.get(device_id).map(|conns| {
            conns.conn_list.iter().filter_map(|c| conn_cache.get(c).map(|c| c.1.clone())).collect()
        }).unwrap_or_default()
    }

    pub fn find_peer(&self, id: &DeviceId) -> Option<CachedPeerInfo> {
        self.device_conn_map.lock().unwrap().get(id).map(|v| {
            v.clone()
        })
    }


}
