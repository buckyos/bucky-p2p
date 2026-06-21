use crate::error::{P2pErrorCode, P2pResult, p2p_err};
use crate::p2p_identity::P2pId;
use crate::types::Timestamp;
use std::sync::Arc;
use std::time::Duration;

use super::control_plane::{OwnerControlPlane, OwnerControlPlaneRef};
use super::membership::{OwnerMembership, ServingLease, bucky_time_to_duration};
use super::route::{PeerRoute, PeerRouteStore, PeerRouteStoreRef};

pub type OwnerDirectoryStoreRef = Arc<OwnerDirectoryStore>;

pub struct OwnerDirectoryStore {
    control_plane: OwnerControlPlaneRef,
    routes: PeerRouteStoreRef,
}

impl OwnerDirectoryStore {
    pub fn new() -> OwnerDirectoryStoreRef {
        Self::local_only(P2pId::from(vec![0; 32]))
    }

    pub fn with_membership(membership: OwnerMembership) -> OwnerDirectoryStoreRef {
        Arc::new(Self {
            control_plane: OwnerControlPlane::new(membership),
            routes: PeerRouteStore::new(),
        })
    }

    pub fn local_only(local_sn_id: P2pId) -> OwnerDirectoryStoreRef {
        Arc::new(Self {
            control_plane: OwnerControlPlane::local_only(local_sn_id),
            routes: PeerRouteStore::new(),
        })
    }

    pub fn control_plane(&self) -> &OwnerControlPlaneRef {
        &self.control_plane
    }

    pub fn put_route(&self, route: PeerRoute, now: Timestamp) -> bool {
        if !self.control_plane.is_serving_session_alive(
            &route.serving_sn_id,
            route.serving_epoch,
            now,
        ) {
            return false;
        }
        self.routes.put_route(route)
    }

    pub fn query_routes(&self, peer_id: &P2pId, now: Timestamp) -> Vec<PeerRoute> {
        self.routes.query(peer_id, &self.control_plane, now)
    }

    pub fn put_lease(&self, lease: ServingLease, now: Timestamp) -> bool {
        if !lease.is_fresh(now) {
            return false;
        }
        let Some(leader_id) = self.control_plane.current_leader() else {
            return false;
        };
        let ttl = bucky_time_to_duration(lease.expires_at.saturating_sub(now));
        if self
            .control_plane
            .renew_serving_session(&leader_id, lease.serving_sn_id.clone(), 0, ttl, now)
            .is_err()
        {
            return false;
        }
        self.put_route(PeerRoute::from_lease(lease, 0), now)
    }

    pub fn revoke_serving_session(
        &self,
        serving_sn_id: P2pId,
        epoch: u64,
        now: Timestamp,
    ) -> P2pResult<u64> {
        let leader_id = self.control_plane.current_leader().ok_or_else(|| {
            p2p_err!(
                P2pErrorCode::NotFound,
                "owner control plane has no current leader"
            )
        })?;
        self.control_plane
            .revoke_serving_session(&leader_id, serving_sn_id, epoch, now)
    }

    pub fn renew_serving_session(
        &self,
        serving_sn_id: P2pId,
        epoch: u64,
        ttl: Duration,
        now: Timestamp,
    ) -> P2pResult<u64> {
        let leader_id = self.control_plane.current_leader().ok_or_else(|| {
            p2p_err!(
                P2pErrorCode::NotFound,
                "owner control plane has no current leader"
            )
        })?;
        self.control_plane
            .renew_serving_session(&leader_id, serving_sn_id, epoch, ttl, now)
    }

    pub fn put_peer_route(&self, route: PeerRoute, now: Timestamp) -> bool {
        self.put_route(route, now)
    }

    pub fn query_peer_routes(&self, peer_id: &P2pId, now: Timestamp) -> Vec<PeerRoute> {
        self.query_routes(peer_id, now)
    }

    pub fn query(&self, peer_id: &P2pId, now: Timestamp) -> Vec<ServingLease> {
        self.query_routes(peer_id, now)
            .into_iter()
            .filter_map(|route| {
                self.control_plane
                    .serving_session_expires_at(&route.serving_sn_id, route.serving_epoch, now)
                    .map(|expires_at| route.to_lease(expires_at))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_id(seed: u8) -> P2pId {
        P2pId::from(vec![seed; 32])
    }

    #[test]
    fn sn_distributed_directory_store_keeps_multi_serving_leases_and_rejects_stale() {
        let store = OwnerDirectoryStore::new();
        let peer = test_id(1);
        let sn_a = test_id(10);
        let sn_b = test_id(11);
        let now = 1_000_000;

        assert!(store.put_lease(
            ServingLease::new(peer.clone(), sn_a.clone(), 2, Duration::from_secs(10), now),
            now
        ));
        assert!(store.put_lease(
            ServingLease::new(peer.clone(), sn_b.clone(), 1, Duration::from_secs(10), now),
            now
        ));
        assert!(!store.put_lease(
            ServingLease::new(peer.clone(), sn_a.clone(), 1, Duration::from_secs(10), now),
            now
        ));

        let leases = store.query(&peer, now);
        assert_eq!(leases.len(), 2);
        assert_eq!(leases[0].serving_sn_id, sn_a);
        assert_eq!(leases[0].sequence, 2);
        assert_eq!(leases[1].serving_sn_id, sn_b);
    }

    #[test]
    fn sn_distributed_directory_store_drops_expired_leases() {
        let store = OwnerDirectoryStore::new();
        let peer = test_id(2);
        let sn = test_id(20);
        let now = 1_000_000;

        assert!(store.put_lease(
            ServingLease::new(peer.clone(), sn, 1, Duration::from_micros(1), now),
            now
        ));

        assert!(store.query(&peer, now + 2).is_empty());
    }

    #[test]
    fn sn_partitioned_peer_route_filters_revoked_serving_session() {
        let owner = test_id(80);
        let serving = test_id(81);
        let peer = test_id(82);
        let membership =
            OwnerMembership::with_options(vec![owner], 1, Duration::from_secs(60)).unwrap();
        let store = OwnerDirectoryStore::with_membership(membership);
        let now = 1_000_000;

        store
            .renew_serving_session(serving.clone(), 3, Duration::from_secs(10), now)
            .unwrap();
        assert!(store.put_peer_route(
            PeerRoute {
                peer_id: peer.clone(),
                serving_sn_id: serving.clone(),
                serving_epoch: 3,
                sequence: 1,
            },
            now + 1
        ));
        assert_eq!(store.query_peer_routes(&peer, now + 2).len(), 1);

        store
            .revoke_serving_session(serving.clone(), 3, now + 3)
            .unwrap();
        assert!(store.query_peer_routes(&peer, now + 4).is_empty());
    }
}
