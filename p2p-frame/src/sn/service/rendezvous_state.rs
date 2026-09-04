use crate::error::{P2pErrorCode, P2pResult, p2p_err};
use crate::p2p_identity::P2pId;
use crate::sn::protocol::{SnTunnelRendezvous, SnTunnelRendezvousResp};
use crate::types::{Sequence, Timestamp, TunnelId};
use std::collections::{HashMap, VecDeque};
use tokio::sync::oneshot;

const MAX_LIVE_ATTEMPTS: usize = 256;
const MAX_LIVE_ATTEMPTS_PER_PAIR: usize = 8;
const MAX_REQUESTS_PER_INITIATOR_WINDOW: usize = 32;
pub(super) const MAX_INFLIGHT_WAITERS_PER_ATTEMPT: usize = 8;
const RATE_WINDOW: Timestamp = 60 * 1_000_000;
const MAX_REQUEST_LIFETIME: Timestamp = 30 * 1_000_000;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct RendezvousKey {
    initiator: P2pId,
    seq: Sequence,
    tunnel_id: TunnelId,
}

impl RendezvousKey {
    fn new(initiator: &P2pId, request: &SnTunnelRendezvous) -> Self {
        Self {
            initiator: initiator.clone(),
            seq: request.seq,
            tunnel_id: request.tunnel_id,
        }
    }
}

struct RendezvousEntry {
    request: SnTunnelRendezvous,
    expires_at: Timestamp,
    response: Option<SnTunnelRendezvousResp>,
    waiters: Vec<oneshot::Sender<P2pResult<SnTunnelRendezvousResp>>>,
}

pub(super) enum RendezvousBegin {
    New,
    Cached(SnTunnelRendezvousResp),
    InFlight(oneshot::Receiver<P2pResult<SnTunnelRendezvousResp>>),
}

pub(super) struct RendezvousState {
    entries: HashMap<RendezvousKey, RendezvousEntry>,
    rate_windows: HashMap<P2pId, VecDeque<Timestamp>>,
}

impl RendezvousState {
    pub(super) fn new() -> Self {
        Self {
            entries: HashMap::new(),
            rate_windows: HashMap::new(),
        }
    }

    fn cleanup(&mut self, now: Timestamp) {
        self.entries.retain(|_, entry| {
            let live = now < entry.expires_at;
            if !live {
                for waiter in entry.waiters.drain(..) {
                    let _ = waiter.send(Err(p2p_err!(
                        P2pErrorCode::Expired,
                        "rendezvous request expired"
                    )));
                }
            }
            live
        });
        self.rate_windows.retain(|_, timestamps| {
            while timestamps
                .front()
                .is_some_and(|timestamp| timestamp.saturating_add(RATE_WINDOW) < now)
            {
                timestamps.pop_front();
            }
            !timestamps.is_empty()
        });
    }

    pub(super) fn begin(
        &mut self,
        authenticated_initiator: &P2pId,
        request: &SnTunnelRendezvous,
        now: Timestamp,
    ) -> P2pResult<RendezvousBegin> {
        self.cleanup(now);
        let key = RendezvousKey::new(authenticated_initiator, request);
        if let Some(entry) = self.entries.get_mut(&key) {
            if entry.request != *request {
                return Err(p2p_err!(
                    P2pErrorCode::Conflict,
                    "rendezvous sequence and tunnel id reused with different request"
                ));
            }
            if let Some(response) = entry.response.clone() {
                return Ok(RendezvousBegin::Cached(response));
            }
            if entry.waiters.len() >= MAX_INFLIGHT_WAITERS_PER_ATTEMPT {
                return Err(p2p_err!(
                    P2pErrorCode::OutOfLimit,
                    "rendezvous duplicate waiter limit exceeded"
                ));
            }
            let (sender, receiver) = oneshot::channel();
            entry.waiters.push(sender);
            return Ok(RendezvousBegin::InFlight(receiver));
        }
        if self.entries.len() >= MAX_LIVE_ATTEMPTS
            || self
                .entries
                .iter()
                .filter(|(entry_key, entry)| {
                    entry_key.initiator == *authenticated_initiator
                        && entry.request.to_peer_id == request.to_peer_id
                })
                .count()
                >= MAX_LIVE_ATTEMPTS_PER_PAIR
        {
            return Err(p2p_err!(
                P2pErrorCode::OutOfLimit,
                "rendezvous live-attempt limit exceeded"
            ));
        }
        let timestamps = self
            .rate_windows
            .entry(authenticated_initiator.clone())
            .or_default();
        while timestamps
            .front()
            .is_some_and(|timestamp| timestamp.saturating_add(RATE_WINDOW) < now)
        {
            timestamps.pop_front();
        }
        if timestamps.len() >= MAX_REQUESTS_PER_INITIATOR_WINDOW {
            return Err(p2p_err!(
                P2pErrorCode::OutOfLimit,
                "rendezvous request rate limit exceeded"
            ));
        }
        timestamps.push_back(now);
        self.entries.insert(
            key,
            RendezvousEntry {
                request: request.clone(),
                expires_at: now.saturating_add(MAX_REQUEST_LIFETIME),
                response: None,
                waiters: Vec::new(),
            },
        );
        Ok(RendezvousBegin::New)
    }

    pub(super) fn cache_response(
        &mut self,
        authenticated_initiator: &P2pId,
        request: &SnTunnelRendezvous,
        response: SnTunnelRendezvousResp,
        now: Timestamp,
    ) -> P2pResult<()> {
        let key = RendezvousKey::new(authenticated_initiator, request);
        if self
            .entries
            .get(&key)
            .is_some_and(|entry| now >= entry.expires_at)
        {
            let Some(mut entry) = self.entries.remove(&key) else {
                return Err(p2p_err!(
                    P2pErrorCode::NotFound,
                    "expired rendezvous request state not found"
                ));
            };
            for waiter in entry.waiters.drain(..) {
                let _ = waiter.send(Err(p2p_err!(
                    P2pErrorCode::Expired,
                    "rendezvous response arrived after request expiry"
                )));
            }
            return Err(p2p_err!(
                P2pErrorCode::Expired,
                "rendezvous response arrived after request expiry"
            ));
        }

        let entry = self.entries.get_mut(&key).ok_or_else(|| {
            p2p_err!(P2pErrorCode::NotFound, "rendezvous request state not found")
        })?;
        if entry.request != *request {
            return Err(p2p_err!(
                P2pErrorCode::Conflict,
                "rendezvous response request snapshot mismatch"
            ));
        }
        response.validate(request.seq, request.need_predict_endpoint)?;
        let cached_response = if request.need_predict_endpoint {
            SnTunnelRendezvousResp::failure(request.seq)
        } else {
            response.clone()
        };
        for waiter in entry.waiters.drain(..) {
            let _ = waiter.send(Ok(response.clone()));
        }
        entry.response = Some(cached_response);
        Ok(())
    }

    pub(super) fn fail_unanswered(
        &mut self,
        authenticated_initiator: &P2pId,
        request: &SnTunnelRendezvous,
    ) {
        let key = RendezvousKey::new(authenticated_initiator, request);
        if let Some(mut entry) = self.entries.remove(&key) {
            if entry.request != *request || entry.response.is_some() {
                self.entries.insert(key, entry);
                return;
            }
            for waiter in entry.waiters.drain(..) {
                let _ = waiter.send(Err(p2p_err!(
                    P2pErrorCode::NetworkError,
                    "rendezvous request failed before a response was available"
                )));
            }
        }
    }

    pub(super) fn remove_peer(&mut self, peer_id: &P2pId) {
        self.entries.retain(|key, entry| {
            let keep = &key.initiator != peer_id && &entry.request.to_peer_id != peer_id;
            if !keep {
                for waiter in entry.waiters.drain(..) {
                    let _ = waiter.send(Err(p2p_err!(
                        P2pErrorCode::UserCanceled,
                        "rendezvous peer disconnected"
                    )));
                }
            }
            keep
        });
        self.rate_windows.remove(peer_id);
    }
}
