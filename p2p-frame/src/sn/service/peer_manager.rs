use std::{
    collections::{HashMap},
    sync::{Arc},
    time::Duration
};
use std::net::SocketAddr;
use std::sync::Mutex;
use bucky_time::bucky_time_now;
use crate::endpoint::{Endpoint, Protocol};
use crate::error::{P2pResult};
use crate::executor::Executor;
use crate::p2p_identity::{P2pId, P2pIdentityCertRef};
use crate::runtime;
use crate::sn::service::PeerConnection;
use crate::sn::service::statistic::{PeerStatus, StatisticManager};
use crate::types::{TunnelId, TunnelIdGenerator, Timestamp};

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
    pub desc: P2pIdentityCertRef,
    pub conn_id: TunnelId,
    pub is_wan: bool,
    pub peer_status: PeerStatus,
}


#[derive(Clone)]
pub struct CachedPeerInfo {
    pub conn_list: Vec<TunnelId>,
    pub desc: P2pIdentityCertRef,
    pub map_ports: Vec<(Protocol, u16)>,
    pub last_send_time: Timestamp,
    pub last_call_time: Timestamp,
    pub local_eps: Arc<mini_moka::sync::Cache<String, Endpoint>>,
    pub is_wan: bool,
    // pub call_peers: HashMap<DeviceId, TempSeq>, // <peerid, last_call_seq>
    // pub receipt: SnServiceReceipt,
    // pub last_receipt_request_time: ReceiptRequestTime,
}

fn has_wan_endpoint(desc: &P2pIdentityCertRef) -> bool {
    for ep in desc.endpoints() {
        if ep.is_static_wan() {
            return true;
        }
    }
    false
}

fn contain_addr(dev: &P2pIdentityCertRef, addr: &SocketAddr) -> bool {
    let endpoints = dev.endpoints();
    for endpoint in endpoints {
        if endpoint.addr() == addr {
            return true;
        }
    }
    false
}

impl CachedPeerInfo {
    fn new(
        desc: P2pIdentityCertRef,
        conn_id: TunnelId,
        send_time: Timestamp) -> CachedPeerInfo {
        CachedPeerInfo {
            conn_list: vec![conn_id],
            is_wan: has_wan_endpoint(&desc),
            desc,
            map_ports: vec![],
            last_send_time: send_time,
            last_call_time: 0,
            // call_peers: Default::default(),
            // receipt: Default::default(),
            // last_receipt_request_time: ReceiptRequestTime::None,
            local_eps: Arc::new(mini_moka::sync::CacheBuilder::new(100).time_to_idle(Duration::from_secs(1800)).build()),
        }
    }

    fn add_conn(&mut self, conn_id: TunnelId) {
        self.conn_list.push(conn_id);
    }

    fn remove_conn(&mut self, conn_id: TunnelId) {
        self.conn_list.retain(|c| c != &conn_id);
    }

    fn conn_count(&self) -> usize {
        self.conn_list.len()
    }

    fn update_desc(&mut self, desc: &P2pIdentityCertRef) -> P2pResult<bool> {
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
    active_peers: HashMap<P2pId, CachedPeerInfo>,
    knock_peers: HashMap<P2pId, CachedPeerInfo>,
}

impl Peers {
    fn find_peer(&mut self, peerid: &P2pId, reason: FindPeerReason) -> Option<&mut CachedPeerInfo> {
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
    conn_cache: Mutex<HashMap<TunnelId, (P2pId, Arc<runtime::Mutex<PeerConnection>>)>>,
    device_conn_map: Mutex<HashMap<P2pId, CachedPeerInfo>>,
    conn_id_generator: TunnelIdGenerator,
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
            conn_id_generator: TunnelIdGenerator::new(),
        })
    }

    pub fn generate_conn_id(&self) -> TunnelId {
        self.conn_id_generator.generate()
    }

    pub fn add_peer_connection(self: &Arc<Self>, peer_desc: P2pIdentityCertRef, mut conn: PeerConnection) {
        let conn_id = conn.conn_id();
        let remote_id = conn.remote_identity_id().clone();
        let recv_handle = conn.take_recv_handle().unwrap();
        let conn = Arc::new(runtime::Mutex::new(conn));
        let mut conn_cache = self.conn_cache.lock().unwrap();
        conn_cache.insert(conn_id, (remote_id.clone(), conn.clone()));
        let mut device_conn_map = self.device_conn_map.lock().unwrap();
        if !device_conn_map.contains_key(&remote_id) {
            device_conn_map.insert(remote_id.clone(), CachedPeerInfo::new(peer_desc, conn_id, bucky_time_now()));
        } else {
            let peer_info = device_conn_map.get_mut(&remote_id).unwrap();
            let _ = peer_info.update_desc(&peer_desc);
            peer_info.add_conn(conn_id);
        }

        let this = self.clone();
        Executor::spawn_ok(async move {
            let _ = recv_handle.await;
            this.remove_peer_connection(conn_id);
        });
    }

    pub fn remove_peer_connection(&self, conn_id: TunnelId) {
        let mut conn_cache = self.conn_cache.lock().unwrap();
        if let Some(conn) = conn_cache.remove(&conn_id) {
            let remote_id = conn.0.clone();
            let mut device_conn_map = self.device_conn_map.lock().unwrap();
            if let Some(conns) = device_conn_map.get_mut(&remote_id) {
                conns.remove_conn(conn_id);
                if conns.conn_list.len() == 0 {
                    device_conn_map.remove(&remote_id);
                }
            }
        }
    }

    pub fn update_peer(&self, device_id: &P2pId, device: &Option<P2pIdentityCertRef>, map_ports: Vec<(Protocol, u16)>, local_eps: &Vec<Endpoint>) {
        let mut device_conn_map = self.device_conn_map.lock().unwrap();
        if let Some(peer) = device_conn_map.get_mut(device_id) {
            if let Some(device) = device {
                let _ = peer.update_desc(device);
            }
            peer.map_ports = map_ports;
            peer.update_local_eps(local_eps);
        }
    }

    pub fn find_connection(&self, conn_id: TunnelId) -> Option<Arc<runtime::Mutex<PeerConnection>>> {
        let conn_cache = self.conn_cache.lock().unwrap();
        conn_cache.get(&conn_id).map(|c| c.1.clone())
    }

    pub fn find_connections(&self, device_id: &P2pId) -> Vec<Arc<runtime::Mutex<PeerConnection>>> {
        let conn_cache = self.conn_cache.lock().unwrap();
        let device_conn_map = self.device_conn_map.lock().unwrap();
        device_conn_map.get(device_id).map(|conns| {
            conns.conn_list.iter().filter_map(|c| conn_cache.get(c).map(|c| c.1.clone())).collect()
        }).unwrap_or_default()
    }

    pub fn find_peer(&self, id: &P2pId) -> Option<CachedPeerInfo> {
        self.device_conn_map.lock().unwrap().get(id).map(|v| {
            v.clone()
        })
    }


}
