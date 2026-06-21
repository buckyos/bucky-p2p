use crate::p2p_identity::P2pId;
use crate::types::Timestamp;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::control_plane::OwnerControlPlane;
use super::membership::ServingLease;

pub type PeerRouteStoreRef = Arc<PeerRouteStore>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeerRoute {
    pub peer_id: P2pId,
    pub serving_sn_id: P2pId,
    pub sequence: u64,
}

impl PeerRoute {
    pub fn from_lease(lease: ServingLease) -> Self {
        Self {
            peer_id: lease.peer_id,
            serving_sn_id: lease.serving_sn_id,
            sequence: lease.sequence,
        }
    }

    pub fn to_lease(&self, expires_at: Timestamp) -> ServingLease {
        ServingLease {
            peer_id: self.peer_id.clone(),
            serving_sn_id: self.serving_sn_id.clone(),
            sequence: self.sequence,
            expires_at,
        }
    }
}

pub struct PeerRouteStore {
    routes: Mutex<HashMap<P2pId, HashMap<P2pId, PeerRoute>>>,
}

impl PeerRouteStore {
    pub fn new() -> PeerRouteStoreRef {
        Arc::new(Self {
            routes: Mutex::new(HashMap::new()),
        })
    }

    pub fn put_route(&self, route: PeerRoute) -> bool {
        let mut routes = self.routes.lock().unwrap();
        let peer_routes = routes.entry(route.peer_id.clone()).or_default();
        match peer_routes.get(&route.serving_sn_id) {
            Some(existing) if existing.sequence > route.sequence => false,
            _ => {
                peer_routes.insert(route.serving_sn_id.clone(), route);
                true
            }
        }
    }

    pub fn query(
        &self,
        peer_id: &P2pId,
        control_plane: &OwnerControlPlane,
        now: Timestamp,
    ) -> Vec<PeerRoute> {
        let mut routes = self.routes.lock().unwrap();
        let Some(peer_routes) = routes.get_mut(peer_id) else {
            return Vec::new();
        };
        peer_routes.retain(|_, route| control_plane.is_serving_online(&route.serving_sn_id, now));
        let mut result = peer_routes.values().cloned().collect::<Vec<_>>();
        result.sort_by(|left, right| {
            right
                .sequence
                .cmp(&left.sequence)
                .then_with(|| left.serving_sn_id.cmp(&right.serving_sn_id))
        });
        result
    }
}
