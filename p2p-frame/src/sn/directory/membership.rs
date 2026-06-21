use crate::endpoint::Endpoint;
use crate::error::{P2pErrorCode, P2pResult, p2p_err};
use crate::p2p_identity::P2pId;
use crate::types::Timestamp;
use bucky_time::bucky_time_now;
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::{DEFAULT_LEASE_TTL, DEFAULT_OWNER_COUNT, DEFAULT_OWNER_MEMBER_HEALTH_TTL};

pub type OwnerMemberHealthRef = Arc<OwnerMemberHealth>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServingLease {
    pub peer_id: P2pId,
    pub serving_sn_id: P2pId,
    pub sequence: u64,
    pub expires_at: Timestamp,
}

impl ServingLease {
    pub fn new(
        peer_id: P2pId,
        serving_sn_id: P2pId,
        sequence: u64,
        ttl: Duration,
        now: Timestamp,
    ) -> Self {
        Self {
            peer_id,
            serving_sn_id,
            sequence,
            expires_at: now.saturating_add(duration_to_bucky_time(ttl)),
        }
    }

    pub fn is_fresh(&self, now: Timestamp) -> bool {
        self.expires_at > now
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerMember {
    pub sn_id: P2pId,
    pub endpoints: Vec<Endpoint>,
    pub name: Option<String>,
}

impl OwnerMember {
    pub fn new(sn_id: P2pId) -> Self {
        Self {
            sn_id,
            endpoints: Vec::new(),
            name: None,
        }
    }

    pub fn with_endpoint(sn_id: P2pId, endpoint: Endpoint) -> Self {
        Self {
            sn_id,
            endpoints: vec![endpoint],
            name: None,
        }
    }

    pub fn with_endpoints(sn_id: P2pId, endpoints: Vec<Endpoint>, name: Option<String>) -> Self {
        Self {
            sn_id,
            endpoints,
            name,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerMembership {
    members: Vec<OwnerMember>,
    owner_count: usize,
    lease_ttl: Duration,
    member_health_ttl: Duration,
}

impl OwnerMembership {
    pub fn new(members: Vec<P2pId>) -> P2pResult<Self> {
        Self::with_members(
            members.into_iter().map(OwnerMember::new).collect(),
            DEFAULT_OWNER_COUNT,
            DEFAULT_LEASE_TTL,
        )
    }

    pub fn with_options(
        members: Vec<P2pId>,
        owner_count: usize,
        lease_ttl: Duration,
    ) -> P2pResult<Self> {
        Self::with_members(
            members.into_iter().map(OwnerMember::new).collect(),
            owner_count,
            lease_ttl,
        )
    }

    pub fn with_members(
        mut members: Vec<OwnerMember>,
        owner_count: usize,
        lease_ttl: Duration,
    ) -> P2pResult<Self> {
        members.sort_by(|left, right| left.sn_id.cmp(&right.sn_id));
        members.dedup_by(|left, right| left.sn_id == right.sn_id);
        if members.is_empty() {
            return Err(p2p_err!(
                P2pErrorCode::InvalidParam,
                "owner membership must contain at least one SN"
            ));
        }
        Ok(Self {
            members,
            owner_count: owner_count.max(1),
            lease_ttl,
            member_health_ttl: lease_ttl.min(DEFAULT_OWNER_MEMBER_HEALTH_TTL),
        })
    }

    pub fn members(&self) -> &[OwnerMember] {
        &self.members
    }

    pub fn member(&self, sn_id: &P2pId) -> Option<&OwnerMember> {
        self.members.iter().find(|member| &member.sn_id == sn_id)
    }

    pub fn lease_ttl(&self) -> Duration {
        self.lease_ttl
    }

    pub fn member_health_ttl(&self) -> Duration {
        self.member_health_ttl
    }
}

#[derive(Clone, Debug)]
pub struct OwnerResolver {
    membership: Option<OwnerMembership>,
}

impl OwnerResolver {
    pub fn local_only() -> Self {
        Self { membership: None }
    }

    pub fn new(membership: Option<OwnerMembership>) -> Self {
        Self { membership }
    }

    pub fn lease_ttl(&self) -> Duration {
        self.membership
            .as_ref()
            .map(|membership| membership.lease_ttl())
            .unwrap_or(DEFAULT_LEASE_TTL)
    }

    pub fn owner_set(&self, peer_id: &P2pId, local_sn_id: &P2pId) -> Vec<P2pId> {
        self.owner_set_at(peer_id, local_sn_id, None, bucky_time_now())
    }

    pub(super) fn owner_set_at(
        &self,
        peer_id: &P2pId,
        local_sn_id: &P2pId,
        member_health: Option<&OwnerMemberHealth>,
        now: Timestamp,
    ) -> Vec<P2pId> {
        let Some(membership) = &self.membership else {
            return vec![local_sn_id.clone()];
        };

        let mut scored = membership
            .members()
            .iter()
            .map(|member| (owner_score(peer_id, &member.sn_id), member.sn_id.clone()))
            .collect::<Vec<_>>();
        scored.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
        let static_owner_set = scored
            .into_iter()
            .take(membership.owner_count.min(membership.members().len()))
            .map(|(_, member)| member)
            .collect::<Vec<_>>();

        if let Some(member_health) = member_health {
            static_owner_set
                .into_iter()
                .filter(|member| member == local_sn_id || member_health.is_fresh(member, now))
                .collect()
        } else {
            static_owner_set
        }
    }

    pub(super) fn has_member(&self, sn_id: &P2pId) -> bool {
        self.membership
            .as_ref()
            .map(|membership| membership.member(sn_id).is_some())
            .unwrap_or(false)
    }
}

pub struct OwnerMemberHealth {
    ttl: Duration,
    fresh_until: Mutex<HashMap<P2pId, Timestamp>>,
}

impl OwnerMemberHealth {
    pub fn new(ttl: Duration) -> OwnerMemberHealthRef {
        Arc::new(Self {
            ttl,
            fresh_until: Mutex::new(HashMap::new()),
        })
    }

    pub fn with_members(
        members: &[OwnerMember],
        ttl: Duration,
        now: Timestamp,
    ) -> OwnerMemberHealthRef {
        let health = Self::new(ttl);
        for member in members {
            health.refresh_at(&member.sn_id, now);
        }
        health
    }

    pub fn refresh(&self, sn_id: &P2pId) {
        self.refresh_at(sn_id, bucky_time_now());
    }

    pub(super) fn refresh_at(&self, sn_id: &P2pId, now: Timestamp) {
        self.fresh_until.lock().unwrap().insert(
            sn_id.clone(),
            now.saturating_add(duration_to_bucky_time(self.ttl)),
        );
    }

    pub fn is_fresh(&self, sn_id: &P2pId, now: Timestamp) -> bool {
        self.fresh_until
            .lock()
            .unwrap()
            .get(sn_id)
            .copied()
            .map(|fresh_until| fresh_until > now)
            .unwrap_or(false)
    }
}

fn owner_score(peer_id: &P2pId, member: &P2pId) -> u64 {
    let mut hasher = DefaultHasher::new();
    peer_id.hash(&mut hasher);
    member.hash(&mut hasher);
    hasher.finish()
}

pub(super) fn bucky_time_to_duration(delta: Timestamp) -> Duration {
    Duration::from_micros(delta.max(1))
}

pub(super) fn duration_to_bucky_time(duration: Duration) -> Timestamp {
    duration.as_micros().min(u128::from(u64::MAX)) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_id(seed: u8) -> P2pId {
        P2pId::from(vec![seed; 32])
    }

    #[test]
    fn sn_distributed_directory_owner_resolver_uses_local_only_without_membership() {
        let resolver = OwnerResolver::local_only();
        let local = test_id(3);

        assert_eq!(resolver.owner_set(&test_id(30), &local), vec![local]);
    }

    #[test]
    fn sn_distributed_directory_owner_resolver_returns_deterministic_hrw_owner_set() {
        let members = vec![test_id(40), test_id(41), test_id(42)];
        let membership =
            OwnerMembership::with_options(members.clone(), 2, Duration::from_secs(60)).unwrap();
        let resolver = OwnerResolver::new(Some(membership));
        let peer = test_id(4);

        let first = resolver.owner_set(&peer, &test_id(99));
        let second = resolver.owner_set(&peer, &test_id(98));

        assert_eq!(first, second);
        assert_eq!(first.len(), 2);
        assert!(first.iter().all(|owner| members.contains(owner)));
    }

    #[test]
    fn sn_owner_member_heartbeat_validity_filters_expired_owner_and_recovers() {
        let members = vec![test_id(50), test_id(51), test_id(52)];
        let membership =
            OwnerMembership::with_options(members.clone(), 2, Duration::from_secs(10)).unwrap();
        let resolver = OwnerResolver::new(Some(membership));
        let local = test_id(99);
        let peer = test_id(5);
        let now = 1_000_000;
        let health = OwnerMemberHealth::new(Duration::from_secs(10));
        let static_owners = resolver.owner_set(&peer, &local);

        health.refresh_at(&static_owners[0], now);

        let only_fresh = resolver.owner_set_at(&peer, &local, Some(&health), now + 1);
        assert_eq!(only_fresh, vec![static_owners[0].clone()]);

        let expired = resolver.owner_set_at(&peer, &local, Some(&health), now + 10_000_001);
        assert!(expired.is_empty());

        health.refresh_at(&static_owners[1], now + 10_000_001);
        let recovered = resolver.owner_set_at(&peer, &local, Some(&health), now + 10_000_002);
        assert_eq!(recovered, vec![static_owners[1].clone()]);
    }
}
