use crate::error::{P2pErrorCode, P2pResult, p2p_err};
use crate::p2p_identity::P2pId;
use crate::types::Timestamp;
use async_trait::async_trait;
use bucky_raw_codec::{RawDecode, RawEncode};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::control_plane::{
    OwnerControlPlane, OwnerControlPlaneRef, OwnerSessionEntry, ServingSnSession,
};
use super::membership::OwnerMembership;

pub type OwnerElectionNodeRef = Arc<OwnerElectionNode>;
pub type OwnerPeerControlClientRef = Arc<dyn OwnerPeerControlClient>;

const DEFAULT_HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OwnerElectionRole {
    Follower,
    Candidate,
    Leader,
}

#[derive(Clone, Debug, Eq, PartialEq, RawEncode, RawDecode)]
pub struct OwnerVoteRequest {
    pub term: u64,
    pub candidate_id: P2pId,
}

#[derive(Clone, Debug, Eq, PartialEq, RawEncode, RawDecode)]
pub struct OwnerVoteResponse {
    pub term: u64,
    pub vote_granted: bool,
    pub leader_id: Option<P2pId>,
}

#[derive(Clone, Debug, Eq, PartialEq, RawEncode, RawDecode)]
pub struct OwnerHeartbeatRequest {
    pub term: u64,
    pub leader_id: P2pId,
    pub committed_index: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, RawEncode, RawDecode)]
pub struct OwnerHeartbeatResponse {
    pub term: u64,
    pub accepted: bool,
    pub leader_id: Option<P2pId>,
}

#[derive(Clone, Debug, Eq, PartialEq, RawEncode, RawDecode)]
pub struct OwnerSessionReplication {
    pub term: u64,
    pub leader_id: P2pId,
    pub entry: OwnerSessionEntry,
    pub now: Timestamp,
    pub committed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, RawEncode, RawDecode)]
pub struct OwnerSessionReplicationResponse {
    pub term: u64,
    pub accepted: bool,
    pub committed_index: u64,
    pub leader_id: Option<P2pId>,
}

#[derive(Clone, Debug, Eq, PartialEq, RawEncode, RawDecode)]
pub struct OwnerSessionForward {
    pub entry: OwnerSessionEntry,
    pub now: Timestamp,
}

#[derive(Clone, Debug, Eq, PartialEq, RawEncode, RawDecode)]
pub struct OwnerSessionForwardResponse {
    pub term: u64,
    pub accepted: bool,
    pub committed_index: u64,
    pub leader_id: Option<P2pId>,
}

#[async_trait]
pub trait OwnerPeerControlClient: Send + Sync + 'static {
    async fn request_vote(
        &self,
        remote_owner_id: &P2pId,
        request: OwnerVoteRequest,
    ) -> P2pResult<OwnerVoteResponse>;

    async fn send_heartbeat(
        &self,
        remote_owner_id: &P2pId,
        request: OwnerHeartbeatRequest,
    ) -> P2pResult<OwnerHeartbeatResponse>;

    async fn replicate_session(
        &self,
        remote_owner_id: &P2pId,
        request: OwnerSessionReplication,
    ) -> P2pResult<OwnerSessionReplicationResponse>;

    async fn forward_session(
        &self,
        remote_owner_id: &P2pId,
        request: OwnerSessionForward,
    ) -> P2pResult<OwnerSessionForwardResponse>;
}

#[derive(Clone, Debug)]
struct OwnerElectionState {
    role: OwnerElectionRole,
    current_term: u64,
    voted_for: Option<P2pId>,
    leader_id: Option<P2pId>,
    last_heartbeat_at: Timestamp,
}

pub struct OwnerElectionNode {
    local_owner_id: P2pId,
    membership: OwnerMembership,
    control_plane: OwnerControlPlaneRef,
    heartbeat_timeout: Duration,
    state: Mutex<OwnerElectionState>,
    peer_client: Mutex<Option<OwnerPeerControlClientRef>>,
}

impl OwnerElectionNode {
    pub fn new(
        local_owner_id: P2pId,
        membership: OwnerMembership,
        control_plane: OwnerControlPlaneRef,
        now: Timestamp,
    ) -> OwnerElectionNodeRef {
        let single_member =
            membership.members().len() == 1 && membership.member(&local_owner_id).is_some();
        let initial_leader_id = local_owner_id.clone();
        Arc::new(Self {
            local_owner_id,
            membership,
            control_plane,
            heartbeat_timeout: DEFAULT_HEARTBEAT_TIMEOUT,
            state: Mutex::new(OwnerElectionState {
                role: if single_member {
                    OwnerElectionRole::Leader
                } else {
                    OwnerElectionRole::Follower
                },
                current_term: if single_member { 1 } else { 0 },
                voted_for: if single_member {
                    Some(initial_leader_id.clone())
                } else {
                    None
                },
                leader_id: if single_member {
                    Some(initial_leader_id)
                } else {
                    None
                },
                last_heartbeat_at: now,
            }),
            peer_client: Mutex::new(None),
        })
    }

    pub fn with_control_plane(
        local_owner_id: P2pId,
        membership: OwnerMembership,
        now: Timestamp,
    ) -> OwnerElectionNodeRef {
        let control_plane = OwnerControlPlane::new(membership.clone());
        Self::new(local_owner_id, membership, control_plane, now)
    }

    pub fn set_peer_client(&self, peer_client: OwnerPeerControlClientRef) {
        *self.peer_client.lock().unwrap() = Some(peer_client);
    }

    pub fn local_owner_id(&self) -> &P2pId {
        &self.local_owner_id
    }

    pub fn role(&self) -> OwnerElectionRole {
        self.state.lock().unwrap().role
    }

    pub fn current_term(&self) -> u64 {
        self.state.lock().unwrap().current_term
    }

    pub fn current_leader(&self) -> Option<P2pId> {
        self.state.lock().unwrap().leader_id.clone()
    }

    pub fn control_plane(&self) -> &OwnerControlPlaneRef {
        &self.control_plane
    }

    pub async fn tick(&self, now: Timestamp) -> P2pResult<Option<P2pId>> {
        let should_elect = {
            let state = self.state.lock().unwrap();
            state.role != OwnerElectionRole::Leader
                && now.saturating_sub(state.last_heartbeat_at)
                    > duration_to_bucky_time(self.heartbeat_timeout)
        };
        if should_elect {
            self.start_election(now).await.map(Some)
        } else {
            Ok(self.current_leader())
        }
    }

    pub async fn start_election(&self, now: Timestamp) -> P2pResult<P2pId> {
        let term = {
            let mut state = self.state.lock().unwrap();
            state.role = OwnerElectionRole::Candidate;
            state.current_term = state.current_term.saturating_add(1).max(1);
            state.voted_for = Some(self.local_owner_id.clone());
            state.leader_id = None;
            state.last_heartbeat_at = now;
            state.current_term
        };

        let mut votes = 1usize;
        let mut highest_term = term;
        let remote_members = self.remote_members();
        let peer_client = if remote_members.is_empty() {
            None
        } else {
            Some(self.peer_client())
        };
        for member in remote_members {
            let response = peer_client
                .as_ref()
                .expect("peer client exists when remote members are present")
                .request_vote(
                    &member,
                    OwnerVoteRequest {
                        term,
                        candidate_id: self.local_owner_id.clone(),
                    },
                )
                .await;
            match response {
                Ok(response) => {
                    highest_term = highest_term.max(response.term);
                    if response.term > term {
                        self.step_down(response.term, response.leader_id, now)?;
                        return Err(p2p_err!(
                            P2pErrorCode::PermissionDenied,
                            "owner election observed higher term"
                        ));
                    }
                    if response.vote_granted {
                        votes += 1;
                    }
                }
                Err(err) => {
                    log::debug!("owner vote request to {} failed: {:?}", member, err);
                }
            }
        }

        if votes >= self.quorum_size() {
            self.become_leader(term, now)?;
            let _ = self.send_heartbeat(now).await;
            Ok(self.local_owner_id.clone())
        } else {
            self.step_down(highest_term, None, now)?;
            Err(p2p_err!(
                P2pErrorCode::NotFound,
                "owner election cannot elect leader without quorum"
            ))
        }
    }

    pub fn receive_vote_request(
        &self,
        remote_owner_id: P2pId,
        request: OwnerVoteRequest,
        now: Timestamp,
    ) -> OwnerVoteResponse {
        if !self.is_member(&remote_owner_id) || remote_owner_id != request.candidate_id {
            let state = self.state.lock().unwrap();
            return OwnerVoteResponse {
                term: state.current_term,
                vote_granted: false,
                leader_id: state.leader_id.clone(),
            };
        }

        let mut state = self.state.lock().unwrap();
        if request.term < state.current_term {
            return OwnerVoteResponse {
                term: state.current_term,
                vote_granted: false,
                leader_id: state.leader_id.clone(),
            };
        }
        if request.term > state.current_term {
            state.current_term = request.term;
            state.role = OwnerElectionRole::Follower;
            state.voted_for = None;
            state.leader_id = None;
        }

        let vote_granted = state
            .voted_for
            .as_ref()
            .map(|voted_for| voted_for == &request.candidate_id)
            .unwrap_or(true);
        if vote_granted {
            state.voted_for = Some(request.candidate_id);
            state.last_heartbeat_at = now;
        }
        OwnerVoteResponse {
            term: state.current_term,
            vote_granted,
            leader_id: state.leader_id.clone(),
        }
    }

    pub fn receive_heartbeat(
        &self,
        remote_owner_id: P2pId,
        request: OwnerHeartbeatRequest,
        now: Timestamp,
    ) -> OwnerHeartbeatResponse {
        if !self.is_member(&remote_owner_id) || remote_owner_id != request.leader_id {
            let state = self.state.lock().unwrap();
            return OwnerHeartbeatResponse {
                term: state.current_term,
                accepted: false,
                leader_id: state.leader_id.clone(),
            };
        }

        let mut state = self.state.lock().unwrap();
        if request.term < state.current_term {
            return OwnerHeartbeatResponse {
                term: state.current_term,
                accepted: false,
                leader_id: state.leader_id.clone(),
            };
        }
        state.current_term = request.term;
        state.role = OwnerElectionRole::Follower;
        state.voted_for = None;
        state.leader_id = Some(request.leader_id.clone());
        state.last_heartbeat_at = now;
        let _ = self
            .control_plane
            .set_known_leader(request.leader_id.clone(), request.term);
        OwnerHeartbeatResponse {
            term: state.current_term,
            accepted: true,
            leader_id: state.leader_id.clone(),
        }
    }

    pub async fn send_heartbeat(&self, now: Timestamp) -> P2pResult<usize> {
        let (term, leader_id) = self.require_leader()?;
        let request = OwnerHeartbeatRequest {
            term,
            leader_id,
            committed_index: self.control_plane.committed_index(),
        };
        let remote_members = self.remote_members();
        let peer_client = if remote_members.is_empty() {
            None
        } else {
            Some(self.peer_client())
        };
        let mut accepted = 1usize;
        for member in remote_members {
            match peer_client
                .as_ref()
                .expect("peer client exists when remote members are present")
                .send_heartbeat(&member, request.clone())
                .await
            {
                Ok(response) if response.term > term => {
                    self.step_down(response.term, response.leader_id, now)?;
                    return Err(p2p_err!(
                        P2pErrorCode::PermissionDenied,
                        "owner heartbeat observed higher term"
                    ));
                }
                Ok(response) if response.accepted => accepted += 1,
                Ok(_) => {}
                Err(err) => log::debug!("owner heartbeat to {} failed: {:?}", member, err),
            }
        }
        Ok(accepted)
    }

    pub async fn renew_serving_session(
        &self,
        serving_sn_id: P2pId,
        _epoch: u64,
        ttl: Duration,
        now: Timestamp,
    ) -> P2pResult<u64> {
        let entry = OwnerSessionEntry::RenewServingSession(ServingSnSession::online(
            serving_sn_id,
            ttl,
            now,
        ));
        self.commit_or_forward_session(entry, now).await
    }

    pub async fn revoke_serving_session(
        &self,
        serving_sn_id: P2pId,
        _epoch: u64,
        now: Timestamp,
    ) -> P2pResult<u64> {
        self.commit_or_forward_session(
            OwnerSessionEntry::RevokeServingSession { serving_sn_id },
            now,
        )
        .await
    }

    pub async fn commit_or_forward_session(
        &self,
        entry: OwnerSessionEntry,
        now: Timestamp,
    ) -> P2pResult<u64> {
        if self.role() == OwnerElectionRole::Leader {
            return self.replicate_and_commit(entry, now).await;
        }
        let leader_id = self.current_leader().ok_or_else(|| {
            p2p_err!(
                P2pErrorCode::NotFound,
                "owner election has no current leader for session forward"
            )
        })?;
        let response = self
            .peer_client()
            .forward_session(&leader_id, OwnerSessionForward { entry, now })
            .await?;
        if response.accepted {
            Ok(response.committed_index)
        } else {
            Err(p2p_err!(
                P2pErrorCode::PermissionDenied,
                "owner leader rejected forwarded session mutation"
            ))
        }
    }

    pub async fn receive_session_forward(
        &self,
        remote_owner_id: P2pId,
        request: OwnerSessionForward,
    ) -> OwnerSessionForwardResponse {
        if !self.is_member(&remote_owner_id) {
            return OwnerSessionForwardResponse {
                term: self.current_term(),
                accepted: false,
                committed_index: self.control_plane.committed_index(),
                leader_id: self.current_leader(),
            };
        }
        match self
            .commit_or_forward_session(request.entry, request.now)
            .await
        {
            Ok(committed_index) => OwnerSessionForwardResponse {
                term: self.current_term(),
                accepted: true,
                committed_index,
                leader_id: self.current_leader(),
            },
            Err(_) => OwnerSessionForwardResponse {
                term: self.current_term(),
                accepted: false,
                committed_index: self.control_plane.committed_index(),
                leader_id: self.current_leader(),
            },
        }
    }

    pub fn receive_session_replication(
        &self,
        remote_owner_id: P2pId,
        request: OwnerSessionReplication,
        now: Timestamp,
    ) -> OwnerSessionReplicationResponse {
        if !self.is_member(&remote_owner_id) || remote_owner_id != request.leader_id {
            return OwnerSessionReplicationResponse {
                term: self.current_term(),
                accepted: false,
                committed_index: self.control_plane.committed_index(),
                leader_id: self.current_leader(),
            };
        }
        let heartbeat = self.receive_heartbeat(
            remote_owner_id,
            OwnerHeartbeatRequest {
                term: request.term,
                leader_id: request.leader_id.clone(),
                committed_index: self.control_plane.committed_index(),
            },
            now,
        );
        if !heartbeat.accepted {
            return OwnerSessionReplicationResponse {
                term: heartbeat.term,
                accepted: false,
                committed_index: self.control_plane.committed_index(),
                leader_id: heartbeat.leader_id,
            };
        }
        if !request.committed {
            return OwnerSessionReplicationResponse {
                term: self.current_term(),
                accepted: true,
                committed_index: self.control_plane.committed_index(),
                leader_id: self.current_leader(),
            };
        }
        match self.control_plane.apply_committed_session_entry(
            &request.leader_id,
            request.term,
            request.entry,
            request.now,
        ) {
            Ok(committed_index) => OwnerSessionReplicationResponse {
                term: self.current_term(),
                accepted: true,
                committed_index,
                leader_id: self.current_leader(),
            },
            Err(_) => OwnerSessionReplicationResponse {
                term: self.current_term(),
                accepted: false,
                committed_index: self.control_plane.committed_index(),
                leader_id: self.current_leader(),
            },
        }
    }

    async fn replicate_and_commit(
        &self,
        entry: OwnerSessionEntry,
        now: Timestamp,
    ) -> P2pResult<u64> {
        let (term, leader_id) = self.require_leader()?;
        let remote_members = self.remote_members();
        let peer_client = if remote_members.is_empty() {
            None
        } else {
            Some(self.peer_client())
        };
        let mut accepted = 1usize;
        for member in remote_members.iter() {
            let response = peer_client
                .as_ref()
                .expect("peer client exists when remote members are present")
                .replicate_session(
                    &member,
                    OwnerSessionReplication {
                        term,
                        leader_id: leader_id.clone(),
                        entry: entry.clone(),
                        now,
                        committed: false,
                    },
                )
                .await;
            match response {
                Ok(response) if response.term > term => {
                    self.step_down(response.term, response.leader_id, now)?;
                    return Err(p2p_err!(
                        P2pErrorCode::PermissionDenied,
                        "owner session replication observed higher term"
                    ));
                }
                Ok(response) if response.accepted => accepted += 1,
                Ok(_) => {}
                Err(err) => {
                    log::debug!("owner session replication to {} failed: {:?}", member, err)
                }
            }
        }
        if accepted < self.quorum_size() {
            return Err(p2p_err!(
                P2pErrorCode::NotFound,
                "owner session replication cannot commit without quorum"
            ));
        }
        let committed_index = self.control_plane.apply_committed_session_entry(
            &leader_id,
            term,
            entry.clone(),
            now,
        )?;
        for member in remote_members {
            let response = peer_client
                .as_ref()
                .expect("peer client exists when remote members are present")
                .replicate_session(
                    &member,
                    OwnerSessionReplication {
                        term,
                        leader_id: leader_id.clone(),
                        entry: entry.clone(),
                        now,
                        committed: true,
                    },
                )
                .await;
            if let Err(err) = response {
                log::debug!(
                    "owner committed session replication to {} failed: {:?}",
                    member,
                    err
                );
            }
        }
        Ok(committed_index)
    }

    fn become_leader(&self, term: u64, now: Timestamp) -> P2pResult<()> {
        {
            let mut state = self.state.lock().unwrap();
            state.role = OwnerElectionRole::Leader;
            state.current_term = term;
            state.voted_for = Some(self.local_owner_id.clone());
            state.leader_id = Some(self.local_owner_id.clone());
            state.last_heartbeat_at = now;
        }
        self.control_plane
            .set_known_leader(self.local_owner_id.clone(), term)
    }

    fn step_down(&self, term: u64, leader_id: Option<P2pId>, now: Timestamp) -> P2pResult<()> {
        {
            let mut state = self.state.lock().unwrap();
            state.role = OwnerElectionRole::Follower;
            state.current_term = state.current_term.max(term);
            state.voted_for = None;
            state.leader_id = leader_id.clone();
            state.last_heartbeat_at = now;
        }
        if let Some(leader_id) = leader_id {
            self.control_plane.set_known_leader(leader_id, term)?;
        }
        Ok(())
    }

    fn require_leader(&self) -> P2pResult<(u64, P2pId)> {
        let state = self.state.lock().unwrap();
        if state.role != OwnerElectionRole::Leader {
            return Err(p2p_err!(
                P2pErrorCode::PermissionDenied,
                "local ownerSN is not leader"
            ));
        }
        Ok((state.current_term, self.local_owner_id.clone()))
    }

    fn peer_client(&self) -> OwnerPeerControlClientRef {
        self.peer_client
            .lock()
            .unwrap()
            .clone()
            .expect("owner peer control client must be attached")
    }

    fn remote_members(&self) -> Vec<P2pId> {
        self.membership
            .members()
            .iter()
            .filter(|member| member.sn_id != self.local_owner_id)
            .map(|member| member.sn_id.clone())
            .collect()
    }

    fn is_member(&self, owner_id: &P2pId) -> bool {
        self.membership.member(owner_id).is_some()
    }

    fn quorum_size(&self) -> usize {
        self.membership.members().len() / 2 + 1
    }
}

fn duration_to_bucky_time(duration: Duration) -> Timestamp {
    duration.as_micros().min(u128::from(u64::MAX)) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    struct MemoryOwnerPeerClient {
        local_id: P2pId,
        nodes: Arc<Mutex<HashMap<P2pId, OwnerElectionNodeRef>>>,
        offline: Arc<Mutex<HashSet<P2pId>>>,
    }

    #[async_trait]
    impl OwnerPeerControlClient for MemoryOwnerPeerClient {
        async fn request_vote(
            &self,
            remote_owner_id: &P2pId,
            request: OwnerVoteRequest,
        ) -> P2pResult<OwnerVoteResponse> {
            let node = self.remote(remote_owner_id)?;
            Ok(node.receive_vote_request(self.local_id.clone(), request, 1_000_000))
        }

        async fn send_heartbeat(
            &self,
            remote_owner_id: &P2pId,
            request: OwnerHeartbeatRequest,
        ) -> P2pResult<OwnerHeartbeatResponse> {
            let node = self.remote(remote_owner_id)?;
            Ok(node.receive_heartbeat(self.local_id.clone(), request, 1_000_000))
        }

        async fn replicate_session(
            &self,
            remote_owner_id: &P2pId,
            request: OwnerSessionReplication,
        ) -> P2pResult<OwnerSessionReplicationResponse> {
            let node = self.remote(remote_owner_id)?;
            Ok(node.receive_session_replication(self.local_id.clone(), request, 1_000_000))
        }

        async fn forward_session(
            &self,
            remote_owner_id: &P2pId,
            request: OwnerSessionForward,
        ) -> P2pResult<OwnerSessionForwardResponse> {
            let node = self.remote(remote_owner_id)?;
            Ok(node
                .receive_session_forward(self.local_id.clone(), request)
                .await)
        }
    }

    impl MemoryOwnerPeerClient {
        fn remote(&self, remote_owner_id: &P2pId) -> P2pResult<OwnerElectionNodeRef> {
            if self.offline.lock().unwrap().contains(remote_owner_id) {
                return Err(p2p_err!(P2pErrorCode::Timeout, "remote owner offline"));
            }
            self.nodes
                .lock()
                .unwrap()
                .get(remote_owner_id)
                .cloned()
                .ok_or_else(|| p2p_err!(P2pErrorCode::NotFound, "remote owner missing"))
        }
    }

    fn test_id(seed: u8) -> P2pId {
        P2pId::from(vec![seed; 32])
    }

    fn owner_cluster() -> (Vec<OwnerElectionNodeRef>, Arc<Mutex<HashSet<P2pId>>>) {
        let ids = vec![test_id(1), test_id(2), test_id(3)];
        let membership =
            OwnerMembership::with_options(ids.clone(), 2, Duration::from_secs(60)).unwrap();
        let nodes = ids
            .iter()
            .map(|id| OwnerElectionNode::with_control_plane(id.clone(), membership.clone(), 0))
            .collect::<Vec<_>>();
        let registry = Arc::new(Mutex::new(HashMap::new()));
        for node in &nodes {
            registry
                .lock()
                .unwrap()
                .insert(node.local_owner_id().clone(), node.clone());
        }
        let offline = Arc::new(Mutex::new(HashSet::new()));
        for node in &nodes {
            node.set_peer_client(Arc::new(MemoryOwnerPeerClient {
                local_id: node.local_owner_id().clone(),
                nodes: registry.clone(),
                offline: offline.clone(),
            }));
        }
        (nodes, offline)
    }

    #[tokio::test]
    async fn sn_owner_network_leader_election_votes_over_owner_peers() {
        let (nodes, _) = owner_cluster();

        let leader = nodes[0].start_election(1_000_000).await.unwrap();

        assert_eq!(leader, nodes[0].local_owner_id().clone());
        assert_eq!(nodes[0].role(), OwnerElectionRole::Leader);
        assert_eq!(nodes[1].current_leader(), Some(leader.clone()));
        assert_eq!(nodes[2].current_leader(), Some(leader));
    }

    #[tokio::test]
    async fn sn_owner_network_leader_failover_elects_new_leader_after_timeout() {
        let (nodes, offline) = owner_cluster();
        let old_leader = nodes[0].start_election(1_000_000).await.unwrap();
        nodes[0].send_heartbeat(1_000_001).await.unwrap();
        offline.lock().unwrap().insert(old_leader);

        let new_leader = nodes[1].tick(20_000_002).await.unwrap().unwrap();

        assert_eq!(new_leader, nodes[1].local_owner_id().clone());
        assert_eq!(nodes[1].role(), OwnerElectionRole::Leader);
        assert_eq!(nodes[2].current_leader(), Some(new_leader));
    }

    #[tokio::test]
    async fn sn_owner_leader_online_replication_forwards_and_replicates() {
        let (nodes, _) = owner_cluster();
        let leader = nodes[0].start_election(1_000_000).await.unwrap();
        nodes[0].send_heartbeat(1_000_001).await.unwrap();
        let serving = test_id(9);

        let committed = nodes[1]
            .renew_serving_session(serving.clone(), 7, Duration::from_secs(30), 1_000_002)
            .await
            .unwrap();

        assert!(committed > 0);
        for node in &nodes {
            assert_eq!(node.current_leader(), Some(leader.clone()));
            assert!(
                node.control_plane()
                    .is_serving_session_alive(&serving, 7, 1_000_003)
            );
        }

        nodes[2]
            .revoke_serving_session(serving.clone(), 7, 1_000_004)
            .await
            .unwrap();
        for node in &nodes {
            assert!(
                !node
                    .control_plane()
                    .is_serving_session_alive(&serving, 7, 1_000_005)
            );
        }
    }

    #[tokio::test]
    async fn sn_owner_replication_does_not_apply_before_commit() {
        let (nodes, _) = owner_cluster();
        let leader = nodes[0].start_election(1_000_000).await.unwrap();
        let serving = test_id(10);
        let entry = OwnerSessionEntry::RenewServingSession(ServingSnSession::online(
            serving.clone(),
            Duration::from_secs(30),
            1_000_001,
        ));

        let pending = nodes[1].receive_session_replication(
            leader.clone(),
            OwnerSessionReplication {
                term: nodes[0].current_term(),
                leader_id: leader.clone(),
                entry: entry.clone(),
                now: 1_000_001,
                committed: false,
            },
            1_000_001,
        );

        assert!(pending.accepted);
        assert!(
            !nodes[1]
                .control_plane()
                .is_serving_session_alive(&serving, 0, 1_000_002)
        );

        let committed = nodes[1].receive_session_replication(
            leader.clone(),
            OwnerSessionReplication {
                term: nodes[0].current_term(),
                leader_id: leader,
                entry,
                now: 1_000_003,
                committed: true,
            },
            1_000_003,
        );

        assert!(committed.accepted);
        assert!(
            nodes[1]
                .control_plane()
                .is_serving_session_alive(&serving, 0, 1_000_004)
        );
    }

    #[tokio::test]
    async fn sn_owner_forward_rejects_non_member_remote_owner() {
        let (nodes, _) = owner_cluster();
        nodes[0].start_election(1_000_000).await.unwrap();
        let serving = test_id(11);
        let entry = OwnerSessionEntry::RenewServingSession(ServingSnSession::online(
            serving,
            Duration::from_secs(30),
            1_000_001,
        ));

        let response = nodes[0]
            .receive_session_forward(
                test_id(99),
                OwnerSessionForward {
                    entry,
                    now: 1_000_001,
                },
            )
            .await;

        assert!(!response.accepted);
    }
}
