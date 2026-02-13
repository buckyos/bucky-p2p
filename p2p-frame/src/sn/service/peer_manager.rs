use crate::endpoint::{Endpoint, Protocol};
use crate::error::P2pResult;
use crate::executor::Executor;
use crate::p2p_identity::{P2pId, P2pIdentityCertRef};
use crate::runtime;
use crate::types::{Timestamp, TunnelId, TunnelIdGenerator};
use bucky_time::bucky_time_now;
use sfo_cmd_server::PeerId;
use std::net::SocketAddr;
use std::sync::Mutex;
use std::{collections::HashMap, sync::Arc, time::Duration};

struct Config {
    pub client_ping_interval: Duration,
    pub client_ping_timeout: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            client_ping_interval: Duration::from_millis(25000),
            client_ping_timeout: Duration::from_secs(300),
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
    fn new(desc: P2pIdentityCertRef, send_time: Timestamp) -> CachedPeerInfo {
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
            Some(p) => Some(p),
            None => match self.knock_peers.get_mut(peerid) {
                Some(p) => Some(p),
                None => None,
            },
        };

        if let Some(p) = found_cache {
            match reason {
                FindPeerReason::CallFrom(t) => {
                    if t > p.last_call_time {
                        p.last_call_time = t;
                    }
                    Some(p)
                }
                FindPeerReason::Other => Some(p),
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
            device_conn_map.insert(
                remote_id.clone(),
                CachedPeerInfo::new(peer_desc, bucky_time_now()),
            );
        } else {
            let peer_info = device_conn_map.get_mut(&remote_id).unwrap();
            let _ = peer_info.update_desc(&peer_desc);
        }
    }

    pub fn remove_peer(self: &Arc<Self>, remote_id: P2pId) {
        let mut device_conn_map = self.device_conn_map.lock().unwrap();
        device_conn_map.remove(&remote_id);
    }

    pub fn add_or_update_peer(
        &self,
        device_id: &P2pId,
        device: &Option<P2pIdentityCertRef>,
        map_ports: Vec<(Protocol, u16)>,
        local_eps: &Vec<Endpoint>,
    ) {
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
        self.device_conn_map
            .lock()
            .unwrap()
            .get(id)
            .map(|v| v.clone())
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::sync::Arc;

    use super::*;
    use crate::endpoint::EndpointArea;
    use crate::p2p_identity::{
        EncodedP2pIdentityCert, P2pIdentityCert, P2pIdentityCertRef, P2pSignature, P2pSn,
    };

    struct DummyIdentityCert {
        id: P2pId,
        name: String,
        endpoints: Vec<Endpoint>,
    }

    impl P2pIdentityCert for DummyIdentityCert {
        fn get_id(&self) -> P2pId {
            self.id.clone()
        }

        fn get_name(&self) -> String {
            self.name.clone()
        }

        fn verify(&self, _message: &[u8], _sign: &P2pSignature) -> bool {
            true
        }

        fn verify_cert(&self, _name: &str) -> bool {
            true
        }

        fn get_encoded_cert(&self) -> P2pResult<EncodedP2pIdentityCert> {
            Ok(vec![])
        }

        fn endpoints(&self) -> Vec<Endpoint> {
            self.endpoints.clone()
        }

        fn sn_list(&self) -> Vec<P2pSn> {
            vec![]
        }

        fn update_endpoints(&self, eps: Vec<Endpoint>) -> P2pIdentityCertRef {
            Arc::new(Self {
                id: self.id.clone(),
                name: self.name.clone(),
                endpoints: eps,
            })
        }
    }

    fn test_id(seed: u8) -> P2pId {
        P2pId::from(vec![seed; 32])
    }

    fn lan_ep(port: u16) -> Endpoint {
        Endpoint::from((
            Protocol::Tcp,
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2)),
            port,
        ))
    }

    fn wan_ep(port: u16) -> Endpoint {
        let mut ep = Endpoint::from((Protocol::Quic, IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)), port));
        ep.set_area(EndpointArea::Wan);
        ep
    }

    fn mapped_ep(port: u16) -> Endpoint {
        let mut ep = Endpoint::from((Protocol::Tcp, IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)), port));
        ep.set_area(EndpointArea::Mapped);
        ep
    }

    fn test_cert(id: P2pId, endpoints: Vec<Endpoint>) -> P2pIdentityCertRef {
        Arc::new(DummyIdentityCert {
            id,
            name: "dummy".to_owned(),
            endpoints,
        })
    }

    #[test]
    fn test_has_wan_endpoint_detects_wan_or_mapped() {
        let lan = test_cert(test_id(1), vec![lan_ep(1001)]);
        let wan = test_cert(test_id(2), vec![wan_ep(1002)]);
        let mapped = test_cert(test_id(3), vec![mapped_ep(1003)]);

        assert!(!has_wan_endpoint(&lan));
        assert!(has_wan_endpoint(&wan));
        assert!(has_wan_endpoint(&mapped));
    }

    #[test]
    fn test_contain_addr_matches_socket_addr() {
        let cert = test_cert(test_id(4), vec![lan_ep(2001), wan_ep(2002)]);
        let exists = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)), 2002);
        let missing = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(9, 9, 9, 9)), 2003);

        assert!(contain_addr(&cert, &exists));
        assert!(!contain_addr(&cert, &missing));
    }

    #[test]
    fn test_add_peer_info_insert_and_update() {
        let mgr = PeerManager::new();
        let remote_id = test_id(10);
        let wan_cert = test_cert(remote_id.clone(), vec![wan_ep(3001)]);
        let lan_cert = test_cert(remote_id.clone(), vec![lan_ep(3002)]);

        mgr.add_peer_info(remote_id.clone(), wan_cert.clone());

        let inserted = mgr.find_peer(&remote_id).unwrap();
        assert!(inserted.is_wan);
        assert_eq!(inserted.desc.get_id(), remote_id);

        mgr.add_peer_info(remote_id.clone(), lan_cert);

        let updated = mgr.find_peer(&remote_id).unwrap();
        assert!(!updated.is_wan);
        assert_eq!(updated.desc.get_id(), remote_id);
    }

    #[test]
    fn test_add_or_update_peer_skip_insert_when_device_none() {
        let mgr = PeerManager::new();
        let remote_id = test_id(11);
        let none_device: Option<P2pIdentityCertRef> = None;
        let local_eps = vec![lan_ep(4001)];

        mgr.add_or_update_peer(
            &remote_id,
            &none_device,
            vec![(Protocol::Tcp, 1234)],
            &local_eps,
        );

        assert!(mgr.find_peer(&remote_id).is_none());
    }

    #[test]
    fn test_add_or_update_peer_insert_and_update() {
        let mgr = PeerManager::new();
        let remote_id = test_id(12);

        let cert_wan = test_cert(remote_id.clone(), vec![wan_ep(5001)]);
        let some_device = Some(cert_wan);
        let local_eps1 = vec![lan_ep(5002)];
        mgr.add_or_update_peer(
            &remote_id,
            &some_device,
            vec![(Protocol::Quic, 6001)],
            &local_eps1,
        );

        let inserted = mgr.find_peer(&remote_id).unwrap();
        assert!(inserted.is_wan);
        assert_eq!(inserted.map_ports, vec![(Protocol::Quic, 6001)]);
        assert_eq!(inserted.local_eps, local_eps1);

        let none_device: Option<P2pIdentityCertRef> = None;
        let local_eps2 = vec![lan_ep(5003), lan_ep(5004)];
        mgr.add_or_update_peer(
            &remote_id,
            &none_device,
            vec![(Protocol::Tcp, 6002)],
            &local_eps2,
        );

        let updated_without_desc = mgr.find_peer(&remote_id).unwrap();
        assert!(updated_without_desc.is_wan);
        assert_eq!(updated_without_desc.map_ports, vec![(Protocol::Tcp, 6002)]);
        assert_eq!(updated_without_desc.local_eps, local_eps2);

        let cert_lan = test_cert(remote_id.clone(), vec![lan_ep(5005)]);
        let some_device2 = Some(cert_lan);
        let local_eps3 = vec![lan_ep(5006)];
        mgr.add_or_update_peer(
            &remote_id,
            &some_device2,
            vec![(Protocol::Tcp, 6003)],
            &local_eps3,
        );

        let updated_with_desc = mgr.find_peer(&remote_id).unwrap();
        assert!(!updated_with_desc.is_wan);
        assert_eq!(updated_with_desc.map_ports, vec![(Protocol::Tcp, 6003)]);
        assert_eq!(updated_with_desc.local_eps, local_eps3);
    }

    #[test]
    fn test_remove_peer_removes_cached_info() {
        let mgr = PeerManager::new();
        let remote_id = test_id(13);
        let cert = test_cert(remote_id.clone(), vec![wan_ep(7001)]);

        mgr.add_peer_info(remote_id.clone(), cert);
        assert!(mgr.find_peer(&remote_id).is_some());

        mgr.remove_peer(remote_id.clone());
        assert!(mgr.find_peer(&remote_id).is_none());
    }
}
