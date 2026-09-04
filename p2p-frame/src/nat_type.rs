use crate::endpoint::Endpoint;
use crate::p2p_identity::P2pId;
use crate::types::Timestamp;
use bucky_raw_codec::{RawDecode, RawEncode};
use std::time::Duration;

/// Wire version of [`NatProfile`]. Unknown versions must be handled by the
/// enclosing optional protocol extension rather than interpreted as v1.
pub const NAT_PROFILE_VERSION: u8 = 1;

/// Wire version of [`NatTraversalContext`].
pub const NAT_TRAVERSAL_CONTEXT_VERSION: u8 = 1;

/// A hard local ceiling for best-effort symmetric-port prediction.
pub const MAX_NAT_PREDICTION_PORTS: usize = 8;

/// What was actually observed while one local UDP socket contacted different
/// ports on one SN IP. This deliberately does not model the traditional
/// four-class NAT taxonomy.
#[derive(Clone, Copy, Debug, Eq, PartialEq, RawEncode, RawDecode)]
pub enum NatMappingObservation {
    Unknown,
    NonSymmetricLike,
    SymmetricLike,
}

/// Parity relationship between consecutive observed external ports.
#[derive(Clone, Copy, Debug, Eq, PartialEq, RawEncode, RawDecode)]
pub enum NatPortParityRelation {
    Same,
    Alternating,
}

/// A bounded, best-effort hint derived from complete observed endpoints.
///
/// It is useful only while its owning profile is fresh and only when a caller
/// supplies a base endpoint with the same IPv4 address. It is not a promise
/// that a mapping created toward a peer IP will follow the same sequence.
#[derive(Clone, Debug, Eq, PartialEq, RawEncode, RawDecode)]
pub struct NatPredictionHint {
    pub first_observed: Endpoint,
    pub last_observed: Endpoint,
    pub sample_count: u16,
    pub port_delta: i32,
    pub parity: NatPortParityRelation,
}

impl NatPredictionHint {
    fn from_observations(observations: &[Endpoint]) -> Option<Self> {
        if observations.len() < 2 || observations.iter().any(|ep| !ep.addr().is_ipv4()) {
            return None;
        }

        let first = observations[0];
        if observations
            .iter()
            .any(|ep| ep.addr().ip() != first.addr().ip())
        {
            return None;
        }

        let delta =
            i32::from(observations[1].addr().port()) - i32::from(observations[0].addr().port());
        if delta == 0
            || observations.windows(2).any(|pair| {
                i32::from(pair[1].addr().port()) - i32::from(pair[0].addr().port()) != delta
            })
        {
            return None;
        }

        let parity = if observations[0].addr().port() % 2 == observations[1].addr().port() % 2 {
            NatPortParityRelation::Same
        } else {
            NatPortParityRelation::Alternating
        };

        Some(Self {
            first_observed: first,
            last_observed: *observations.last().expect("length checked above"),
            sample_count: observations.len().min(u16::MAX as usize) as u16,
            port_delta: delta,
            parity,
        })
    }

    pub fn is_usable_with(&self, base: &Endpoint) -> bool {
        if self.sample_count < 2
            || self.port_delta == 0
            || !self.first_observed.addr().is_ipv4()
            || !self.last_observed.addr().is_ipv4()
            || !base.addr().is_ipv4()
            || self.first_observed.addr().ip() != self.last_observed.addr().ip()
            || base.addr().ip() != self.last_observed.addr().ip()
        {
            return false;
        }

        let Some(expected_total_delta) = self
            .port_delta
            .checked_mul(i32::from(self.sample_count - 1))
        else {
            return false;
        };
        if i32::from(self.last_observed.addr().port())
            - i32::from(self.first_observed.addr().port())
            != expected_total_delta
        {
            return false;
        }

        let delta_parity = if self.port_delta.unsigned_abs() % 2 == 0 {
            NatPortParityRelation::Same
        } else {
            NatPortParityRelation::Alternating
        };

        self.parity == delta_parity
    }

    /// Generate at most `limit` next ports from `base`, additionally capped by
    /// [`MAX_NAT_PREDICTION_PORTS`]. Overflow terminates the window and the
    /// base port is never repeated.
    pub fn predicted_ports(&self, base: &Endpoint, limit: usize) -> Vec<u16> {
        if !self.is_usable_with(base) {
            return Vec::new();
        }

        let limit = limit.min(MAX_NAT_PREDICTION_PORTS);
        let mut ports = Vec::with_capacity(limit);
        let base_port = i64::from(base.addr().port());
        let delta = i64::from(self.port_delta);

        for step in 1..=limit {
            let Some(offset) = delta.checked_mul(step as i64) else {
                break;
            };
            let Some(candidate) = base_port.checked_add(offset) else {
                break;
            };
            if !(1..=u16::MAX as i64).contains(&candidate) {
                break;
            }

            let candidate = candidate as u16;
            if candidate != base.addr().port() && !ports.contains(&candidate) {
                ports.push(candidate);
            }
        }

        ports
    }
}

/// Versioned result of the latest same-SN-IP mapping observation.
#[derive(Clone, Debug, Eq, PartialEq, RawEncode, RawDecode)]
pub struct NatProfile {
    pub version: u8,
    pub observation: NatMappingObservation,
    pub observed_endpoint: Option<Endpoint>,
    pub observed_at: Timestamp,
    pub valid_until: Timestamp,
    pub prediction_hint: Option<NatPredictionHint>,
}

impl Default for NatProfile {
    fn default() -> Self {
        Self::unknown()
    }
}

impl NatProfile {
    pub fn unknown() -> Self {
        Self {
            version: NAT_PROFILE_VERSION,
            observation: NatMappingObservation::Unknown,
            observed_endpoint: None,
            observed_at: 0,
            valid_until: 0,
            prediction_hint: None,
        }
    }

    /// Classify complete observed endpoints from one local socket contacting
    /// different ports on one SN IP.
    pub fn from_observations(
        observations: &[Endpoint],
        observed_at: Timestamp,
        ttl: Duration,
    ) -> Self {
        if observations.len() < 2 || observations.len() > u16::MAX as usize || ttl.is_zero() {
            return Self::unknown();
        }

        let valid_until = observed_at.saturating_add(duration_to_bucky_time(ttl));
        let observed_endpoint = observations.last().copied();
        if observations
            .iter()
            .all(|ep| ep.addr() == observations[0].addr())
        {
            return Self {
                version: NAT_PROFILE_VERSION,
                observation: NatMappingObservation::NonSymmetricLike,
                observed_endpoint,
                observed_at,
                valid_until,
                prediction_hint: None,
            };
        }

        Self {
            version: NAT_PROFILE_VERSION,
            observation: NatMappingObservation::SymmetricLike,
            observed_endpoint,
            observed_at,
            valid_until,
            prediction_hint: NatPredictionHint::from_observations(observations),
        }
    }

    pub fn is_supported(&self) -> bool {
        self.version == NAT_PROFILE_VERSION
    }

    pub fn is_fresh(&self, now: Timestamp) -> bool {
        self.is_supported()
            && self.observation != NatMappingObservation::Unknown
            && self.observed_endpoint.is_some()
            && self.valid_until >= self.observed_at
            && now >= self.observed_at
            && now <= self.valid_until
    }

    /// Return the effective mapping observation, failing closed to Unknown for
    /// unsupported, incomplete, or stale profiles.
    pub fn mapping_at(&self, now: Timestamp) -> NatMappingObservation {
        if self.is_fresh(now) {
            self.observation
        } else {
            NatMappingObservation::Unknown
        }
    }

    pub fn usable_prediction_hint(&self, now: Timestamp) -> Option<&NatPredictionHint> {
        if self.mapping_at(now) != NatMappingObservation::SymmetricLike {
            return None;
        }

        let base = self.observed_endpoint.as_ref()?;
        self.prediction_hint
            .as_ref()
            .filter(|hint| hint.is_usable_with(base))
    }

    pub fn predicted_ports(&self, now: Timestamp, limit: usize) -> Vec<u16> {
        let Some(base) = self.observed_endpoint.as_ref() else {
            return Vec::new();
        };
        self.usable_prediction_hint(now)
            .map(|hint| hint.predicted_ports(base, limit))
            .unwrap_or_default()
    }
}

/// Immutable caller/callee ordered profile snapshots used by both sides of one
/// logical tunnel attempt.
#[derive(Clone, Debug, Eq, PartialEq, RawEncode, RawDecode)]
pub struct NatTraversalContext {
    pub version: u8,
    pub caller_peer_id: P2pId,
    pub callee_peer_id: P2pId,
    pub caller_profile: NatProfile,
    pub callee_profile: NatProfile,
}

impl NatTraversalContext {
    pub fn new(
        caller_peer_id: P2pId,
        callee_peer_id: P2pId,
        caller_profile: NatProfile,
        callee_profile: NatProfile,
    ) -> Self {
        Self {
            version: NAT_TRAVERSAL_CONTEXT_VERSION,
            caller_peer_id,
            callee_peer_id,
            caller_profile,
            callee_profile,
        }
    }

    pub fn is_valid_for(
        &self,
        caller_peer_id: &P2pId,
        callee_peer_id: &P2pId,
        now: Timestamp,
    ) -> bool {
        self.is_supported()
            && &self.caller_peer_id == caller_peer_id
            && &self.callee_peer_id == callee_peer_id
            && self.caller_peer_id != self.callee_peer_id
            && self.caller_profile.is_fresh(now)
            && self.callee_profile.is_fresh(now)
    }

    pub fn is_supported(&self) -> bool {
        self.version == NAT_TRAVERSAL_CONTEXT_VERSION
    }
}

fn duration_to_bucky_time(duration: Duration) -> Timestamp {
    duration.as_micros().min(Timestamp::MAX as u128) as Timestamp
}

#[cfg(test)]
mod tests {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/unit/nat_type/tests.rs"
    ));
}
