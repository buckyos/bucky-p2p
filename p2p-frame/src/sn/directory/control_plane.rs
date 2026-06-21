use crate::error::{P2pErrorCode, P2pResult, p2p_err};
use crate::p2p_identity::P2pId;
use crate::types::Timestamp;
use bucky_raw_codec::{RawDecode, RawEncode};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::membership::{OwnerMembership, duration_to_bucky_time};

pub type OwnerControlPlaneRef = Arc<OwnerControlPlane>;

#[derive(Clone, Copy, Debug, Eq, PartialEq, RawEncode, RawDecode)]
pub enum ServingSnSessionState {
    Alive,
    Revoked,
}

#[derive(Clone, Debug, Eq, PartialEq, RawEncode, RawDecode)]
pub struct ServingSnSession {
    pub serving_sn_id: P2pId,
    pub epoch: u64,
    pub state: ServingSnSessionState,
    pub expires_at: Timestamp,
}

impl ServingSnSession {
    pub fn alive(serving_sn_id: P2pId, epoch: u64, ttl: Duration, now: Timestamp) -> Self {
        Self {
            serving_sn_id,
            epoch,
            state: ServingSnSessionState::Alive,
            expires_at: now.saturating_add(duration_to_bucky_time(ttl)),
        }
    }

    pub fn is_alive(&self, epoch: u64, now: Timestamp) -> bool {
        self.epoch == epoch && self.state == ServingSnSessionState::Alive && self.expires_at > now
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_id(seed: u8) -> P2pId {
        P2pId::from(vec![seed; 32])
    }

    #[test]
    fn sn_owner_control_plane_sessions_require_leader_and_quorum() {
        let members = vec![test_id(60), test_id(61), test_id(62)];
        let membership =
            OwnerMembership::with_options(members.clone(), 2, Duration::from_secs(60)).unwrap();
        let control = OwnerControlPlane::new(membership);
        let now = 1_000_000;
        let serving = test_id(70);

        let term = control.elect_leader(members[1].clone()).unwrap();
        assert_eq!(term, 2);
        assert_eq!(control.current_leader(), Some(members[1].clone()));

        assert!(
            control
                .renew_serving_session(
                    &members[0],
                    serving.clone(),
                    7,
                    Duration::from_secs(10),
                    now
                )
                .is_err()
        );
        assert!(
            control
                .renew_serving_session(
                    &members[1],
                    serving.clone(),
                    7,
                    Duration::from_secs(10),
                    now
                )
                .is_ok()
        );
        assert!(control.is_serving_session_alive(&serving, 7, now + 1));

        assert!(control.set_voter_active(&members[2], false));
        assert!(control.set_voter_active(&members[1], false));
        assert!(
            control
                .renew_serving_session(
                    &members[1],
                    serving.clone(),
                    7,
                    Duration::from_secs(10),
                    now + 2
                )
                .is_err()
        );
    }
}

#[derive(Clone, Debug, Eq, PartialEq, RawEncode, RawDecode)]
pub enum OwnerSessionEntry {
    RenewServingSession(ServingSnSession),
    RevokeServingSession { serving_sn_id: P2pId, epoch: u64 },
}

#[derive(Clone, Debug)]
struct OwnerControlPlaneState {
    current_term: u64,
    leader_id: Option<P2pId>,
    committed_index: u64,
    log: Vec<OwnerSessionEntry>,
    sessions: HashMap<P2pId, ServingSnSession>,
    active_voters: HashSet<P2pId>,
}

pub struct OwnerControlPlane {
    membership: OwnerMembership,
    state: Mutex<OwnerControlPlaneState>,
}

impl OwnerControlPlane {
    pub fn new(membership: OwnerMembership) -> OwnerControlPlaneRef {
        let leader_id = membership
            .members()
            .first()
            .map(|member| member.sn_id.clone());
        let active_voters = membership
            .members()
            .iter()
            .map(|member| member.sn_id.clone())
            .collect::<HashSet<_>>();
        Arc::new(Self {
            membership,
            state: Mutex::new(OwnerControlPlaneState {
                current_term: leader_id.as_ref().map(|_| 1).unwrap_or(0),
                leader_id,
                committed_index: 0,
                log: Vec::new(),
                sessions: HashMap::new(),
                active_voters,
            }),
        })
    }

    pub fn local_only(local_sn_id: P2pId) -> OwnerControlPlaneRef {
        let membership = OwnerMembership::new(vec![local_sn_id])
            .expect("local owner membership must contain one member");
        Self::new(membership)
    }

    pub fn current_leader(&self) -> Option<P2pId> {
        self.state.lock().unwrap().leader_id.clone()
    }

    pub fn current_term(&self) -> u64 {
        self.state.lock().unwrap().current_term
    }

    pub fn committed_index(&self) -> u64 {
        self.state.lock().unwrap().committed_index
    }

    pub fn set_voter_active(&self, member: &P2pId, active: bool) -> bool {
        if self.membership.member(member).is_none() {
            return false;
        }
        let mut state = self.state.lock().unwrap();
        if active {
            state.active_voters.insert(member.clone());
        } else {
            state.active_voters.remove(member);
        }
        true
    }

    pub fn elect_leader(&self, candidate: P2pId) -> P2pResult<u64> {
        if self.membership.member(&candidate).is_none() {
            return Err(p2p_err!(
                P2pErrorCode::PermissionDenied,
                "owner control candidate is not a member"
            ));
        }
        let mut state = self.state.lock().unwrap();
        if !state.active_voters.contains(&candidate) || !self.has_quorum_locked(&state) {
            return Err(p2p_err!(
                P2pErrorCode::NotFound,
                "owner control plane cannot elect leader without quorum"
            ));
        }
        state.current_term = state.current_term.saturating_add(1).max(1);
        state.leader_id = Some(candidate);
        Ok(state.current_term)
    }

    pub fn renew_serving_session(
        &self,
        leader_id: &P2pId,
        serving_sn_id: P2pId,
        epoch: u64,
        ttl: Duration,
        now: Timestamp,
    ) -> P2pResult<u64> {
        let session = ServingSnSession::alive(serving_sn_id, epoch, ttl, now);
        self.append_as_leader(
            leader_id,
            OwnerSessionEntry::RenewServingSession(session),
            now,
        )
    }

    pub fn revoke_serving_session(
        &self,
        leader_id: &P2pId,
        serving_sn_id: P2pId,
        epoch: u64,
        now: Timestamp,
    ) -> P2pResult<u64> {
        self.append_as_leader(
            leader_id,
            OwnerSessionEntry::RevokeServingSession {
                serving_sn_id,
                epoch,
            },
            now,
        )
    }

    pub fn is_serving_session_alive(
        &self,
        serving_sn_id: &P2pId,
        epoch: u64,
        now: Timestamp,
    ) -> bool {
        self.state
            .lock()
            .unwrap()
            .sessions
            .get(serving_sn_id)
            .map(|session| session.is_alive(epoch, now))
            .unwrap_or(false)
    }

    pub fn serving_session_expires_at(
        &self,
        serving_sn_id: &P2pId,
        epoch: u64,
        now: Timestamp,
    ) -> Option<Timestamp> {
        self.state
            .lock()
            .unwrap()
            .sessions
            .get(serving_sn_id)
            .filter(|session| session.is_alive(epoch, now))
            .map(|session| session.expires_at)
    }

    pub fn set_known_leader(&self, leader_id: P2pId, term: u64) -> P2pResult<()> {
        if self.membership.member(&leader_id).is_none() {
            return Err(p2p_err!(
                P2pErrorCode::PermissionDenied,
                "owner control leader is not a member"
            ));
        }
        let mut state = self.state.lock().unwrap();
        state.current_term = state.current_term.max(term);
        state.leader_id = Some(leader_id);
        Ok(())
    }

    pub fn apply_committed_session_entry(
        &self,
        leader_id: &P2pId,
        term: u64,
        entry: OwnerSessionEntry,
        now: Timestamp,
    ) -> P2pResult<u64> {
        if self.membership.member(leader_id).is_none() {
            return Err(p2p_err!(
                P2pErrorCode::PermissionDenied,
                "owner control leader is not a member"
            ));
        }
        let mut state = self.state.lock().unwrap();
        state.current_term = state.current_term.max(term);
        state.leader_id = Some(leader_id.clone());
        state.log.push(entry.clone());
        state.committed_index = state.log.len() as u64;
        Self::apply_entry(&mut state, entry, now);
        Ok(state.committed_index)
    }

    fn append_as_leader(
        &self,
        leader_id: &P2pId,
        entry: OwnerSessionEntry,
        now: Timestamp,
    ) -> P2pResult<u64> {
        let mut state = self.state.lock().unwrap();
        if state.leader_id.as_ref() != Some(leader_id) {
            return Err(p2p_err!(
                P2pErrorCode::PermissionDenied,
                "owner control mutation must be submitted to current leader"
            ));
        }
        if !self.has_quorum_locked(&state) {
            return Err(p2p_err!(
                P2pErrorCode::NotFound,
                "owner control plane cannot commit without quorum"
            ));
        }
        state.log.push(entry.clone());
        state.committed_index = state.log.len() as u64;
        Self::apply_entry(&mut state, entry, now);
        Ok(state.committed_index)
    }

    fn has_quorum_locked(&self, state: &OwnerControlPlaneState) -> bool {
        state.active_voters.len() >= self.quorum_size()
    }

    fn quorum_size(&self) -> usize {
        self.membership.members().len() / 2 + 1
    }

    fn apply_entry(state: &mut OwnerControlPlaneState, entry: OwnerSessionEntry, now: Timestamp) {
        match entry {
            OwnerSessionEntry::RenewServingSession(session) => {
                let should_update = state
                    .sessions
                    .get(&session.serving_sn_id)
                    .map(|existing| existing.epoch <= session.epoch)
                    .unwrap_or(true);
                if should_update {
                    state
                        .sessions
                        .insert(session.serving_sn_id.clone(), session);
                }
            }
            OwnerSessionEntry::RevokeServingSession {
                serving_sn_id,
                epoch,
            } => {
                if let Some(existing) = state.sessions.get_mut(&serving_sn_id) {
                    if existing.epoch == epoch && existing.expires_at > now {
                        existing.state = ServingSnSessionState::Revoked;
                        existing.expires_at = now;
                    }
                }
            }
        }
    }
}
