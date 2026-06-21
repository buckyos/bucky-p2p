use crate::p2p_identity::P2pId;
use bucky_time::bucky_time_now;
use std::sync::Arc;

use super::DEFAULT_OWNER_MEMBER_HEALTH_TTL;
use super::control_plane::OwnerControlPlaneRef;
use super::membership::{
    OwnerMemberHealth, OwnerMemberHealthRef, OwnerMembership, OwnerResolver, ServingLease,
};
use super::route::PeerRoute;
use super::store::{OwnerDirectoryStore, OwnerDirectoryStoreRef};

pub type LocalSnDirectoryRef = Arc<LocalSnDirectory>;
pub type LocalOwnerDirectoryRef = Arc<LocalOwnerDirectory>;

#[derive(Clone)]
pub struct LocalOwnerDirectory {
    directory: LocalSnDirectoryRef,
}

impl LocalOwnerDirectory {
    pub fn new(directory: LocalSnDirectoryRef) -> LocalOwnerDirectoryRef {
        Arc::new(Self { directory })
    }

    pub fn publish_serving_lease(&self, lease: ServingLease) -> bool {
        self.directory.put_lease_route(lease)
    }

    pub fn query_serving_leases(&self, peer_id: &P2pId) -> Vec<ServingLease> {
        self.directory.query(peer_id)
    }

    pub fn refresh_owner_member(&self, member_sn_id: &P2pId) -> bool {
        self.directory.refresh_owner_member(member_sn_id)
    }

    pub fn control_plane(&self) -> &OwnerControlPlaneRef {
        self.directory.control_plane()
    }
}

#[derive(Clone)]
pub struct LocalSnDirectory {
    resolver: OwnerResolver,
    member_health: OwnerMemberHealthRef,
    store: OwnerDirectoryStoreRef,
}

impl LocalSnDirectory {
    pub fn new(membership: Option<OwnerMembership>) -> LocalSnDirectoryRef {
        let now = bucky_time_now();
        let member_health = membership
            .as_ref()
            .map(|membership| {
                OwnerMemberHealth::with_members(
                    membership.members(),
                    membership.member_health_ttl(),
                    now,
                )
            })
            .unwrap_or_else(|| OwnerMemberHealth::new(DEFAULT_OWNER_MEMBER_HEALTH_TTL));
        Arc::new(Self {
            resolver: OwnerResolver::new(membership.clone()),
            member_health,
            store: membership
                .map(OwnerDirectoryStore::with_membership)
                .unwrap_or_else(|| OwnerDirectoryStore::local_only(P2pId::from(vec![0; 32]))),
        })
    }

    pub fn local_only() -> LocalSnDirectoryRef {
        Arc::new(Self {
            resolver: OwnerResolver::local_only(),
            member_health: OwnerMemberHealth::new(DEFAULT_OWNER_MEMBER_HEALTH_TTL),
            store: OwnerDirectoryStore::local_only(P2pId::from(vec![0; 32])),
        })
    }

    pub fn owner_set(&self, peer_id: &P2pId, local_sn_id: &P2pId) -> Vec<P2pId> {
        self.resolver.owner_set_at(
            peer_id,
            local_sn_id,
            Some(&self.member_health),
            bucky_time_now(),
        )
    }

    pub fn refresh_owner_member(&self, member_sn_id: &P2pId) -> bool {
        if !self.resolver.has_member(member_sn_id) {
            return false;
        }
        self.member_health.refresh(member_sn_id);
        true
    }

    pub fn new_lease(&self, peer_id: P2pId, serving_sn_id: P2pId, sequence: u64) -> ServingLease {
        ServingLease::new(
            peer_id,
            serving_sn_id,
            sequence,
            self.resolver.lease_ttl(),
            bucky_time_now(),
        )
    }

    pub fn put_lease(&self, lease: ServingLease) -> bool {
        self.store.put_lease(lease, bucky_time_now())
    }

    pub fn put_lease_route(&self, lease: ServingLease) -> bool {
        self.store
            .put_peer_route(PeerRoute::from_lease(lease), bucky_time_now())
    }

    pub fn query(&self, peer_id: &P2pId) -> Vec<ServingLease> {
        self.store.query(peer_id, bucky_time_now())
    }

    pub fn control_plane(&self) -> &OwnerControlPlaneRef {
        self.store.control_plane()
    }
}
