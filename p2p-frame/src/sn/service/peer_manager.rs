use std::{
    collections::{HashMap},
    sync::{Arc},
    time::Duration
};
use std::net::SocketAddr;
use std::sync::Mutex;
use bucky_time::bucky_time_now;
use sfo_cmd_server::PeerId;
use crate::endpoint::{Endpoint, Protocol};
use crate::error::{P2pResult};
use crate::executor::Executor;
use crate::p2p_identity::{P2pId, P2pIdentityCertRef};
use crate::runtime;
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
}


#[derive(Clone)]
pub struct CachedPeerInfo {
    pub desc: P2pIdentityCertRef,
    pub map_ports: Vec<(Protocol, u16)>,
    pub last_send_time: Timestamp,
    pub last_call_time: Timestamp,
    pub local_eps: Vec<Endpoint>,
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
        send_time: Timestamp) -> CachedPeerInfo {
        CachedPeerInfo {
            is_wan: has_wan_endpoint(&desc),
            desc,
            map_ports: vec![],
            last_send_time: send_time,
            last_call_time: 0,
            // call_peers: Default::default(),
            // receipt: Default::default(),
            // last_receipt_request_time: ReceiptRequestTime::None,
            local_eps: Vec::new(),
        }
    }

    fn update_desc(&mut self, desc: &P2pIdentityCertRef) -> P2pResult<bool> {
        self.desc = desc.clone();
        self.is_wan = has_wan_endpoint(desc);
        Ok(true)
    }

    fn update_local_eps(&mut self, local_ips: &Vec<Endpoint>) {
        self.local_eps = local_ips.clone();
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
    device_conn_map: Mutex<HashMap<P2pId, CachedPeerInfo>>,
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
            device_conn_map: Mutex::new(Default::default()),
        })
    }

    pub fn add_peer_info(self: &Arc<Self>, remote_id: P2pId, peer_desc: P2pIdentityCertRef) {
        let mut device_conn_map = self.device_conn_map.lock().unwrap();
        if !device_conn_map.contains_key(&remote_id) {
            device_conn_map.insert(remote_id.clone(), CachedPeerInfo::new(peer_desc, bucky_time_now()));
        } else {
            let peer_info = device_conn_map.get_mut(&remote_id).unwrap();
            let _ = peer_info.update_desc(&peer_desc);
        }
    }

    pub fn remove_peer(self: &Arc<Self>, remote_id: P2pId) {
        let mut device_conn_map = self.device_conn_map.lock().unwrap();
        device_conn_map.remove(&remote_id);
    }

    pub fn add_or_update_peer(&self, device_id: &P2pId, device: &Option<P2pIdentityCertRef>, map_ports: Vec<(Protocol, u16)>, local_eps: &Vec<Endpoint>) {
        let mut device_conn_map = self.device_conn_map.lock().unwrap();
        if let Some(peer) = device_conn_map.get_mut(device_id) {
            if let Some(device) = device {
                let _ = peer.update_desc(device);
            }
            peer.map_ports = map_ports;
            peer.update_local_eps(local_eps);
        } else {
            if device.is_none() {
                return;
            }

            let mut peer = CachedPeerInfo::new(device.clone().unwrap(), bucky_time_now());
            peer.map_ports = map_ports;
            peer.update_local_eps(local_eps);
            device_conn_map.insert(device_id.clone(), peer);
        }
    }

    pub fn find_peer(&self, id: &P2pId) -> Option<CachedPeerInfo> {
        self.device_conn_map.lock().unwrap().get(id).map(|v| {
            v.clone()
        })
    }


}
