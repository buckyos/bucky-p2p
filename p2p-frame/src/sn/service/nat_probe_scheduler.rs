use crate::endpoint::{Endpoint, Protocol};
use crate::nat_type::{NatMappingObservation, NatProfile};
use crate::p2p_identity::P2pId;
use crate::sn::nat_probe::MAX_NAT_PROBE_ENDPOINTS;
use crate::sn::protocol::{NAT_PROBE_CONTROL_VERSION, NatProbeDirective, NatProbeResult};
use crate::sn::types::CmdTunnelId;
use crate::types::Timestamp;
use std::collections::HashMap;
use std::time::Duration;

pub(super) const NAT_PROBE_PERIOD: Duration = Duration::from_secs(2 * 60 * 60);
pub(super) const NAT_PROBE_DIRECTIVE_TIMEOUT: Duration = Duration::from_secs(30);
pub(super) const NAT_PROBE_FAILURE_BACKOFF: Duration = Duration::from_secs(60);
pub(super) const MAX_CONCURRENT_NAT_PROBES: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum NatProbeTriggerReason {
    Online,
    ExternalAddress,
    Config,
    Demand,
    Periodic,
}

impl NatProbeTriggerReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::Online => "online",
            Self::ExternalAddress => "external_address",
            Self::Config => "config",
            Self::Demand => "demand",
            Self::Periodic => "periodic",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum NatProbeAuthorityRemovalReason {
    TunnelMissing,
    PeerDisconnected,
}

impl NatProbeAuthorityRemovalReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::TunnelMissing => "tunnel_missing",
            Self::PeerDisconnected => "peer_disconnected",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NatProbeResultRejectReason {
    MissingAuthority,
    CapabilityUnsupported,
    VersionUnsupported,
    ProfileStale,
    SnMismatch,
    PeerMismatch,
    RegistrationGenerationMismatch,
    ConfigGenerationMismatch,
    MissingInFlight,
    RequestMismatch,
    DeadlineExpired,
}

impl NatProbeResultRejectReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::MissingAuthority => "missing_authority",
            Self::CapabilityUnsupported => "capability_unsupported",
            Self::VersionUnsupported => "version_unsupported",
            Self::ProfileStale => "profile_stale",
            Self::SnMismatch => "sn_mismatch",
            Self::PeerMismatch => "peer_mismatch",
            Self::RegistrationGenerationMismatch => "registration_generation_mismatch",
            Self::ConfigGenerationMismatch => "config_generation_mismatch",
            Self::MissingInFlight => "missing_in_flight",
            Self::RequestMismatch => "request_mismatch",
            Self::DeadlineExpired => "deadline_expired",
        }
    }
}

fn duration_to_bucky_time(duration: Duration) -> Timestamp {
    duration.as_micros().min(Timestamp::MAX as u128) as Timestamp
}

#[derive(Clone, Debug)]
struct InFlightProbe {
    request_id: u64,
    expires_at: Timestamp,
}

#[derive(Clone, Debug)]
struct PeerProbeState {
    authority_tunnel_id: CmdTunnelId,
    remote_endpoint: Endpoint,
    registration_generation: u64,
    config_generation: u64,
    in_flight: Option<InFlightProbe>,
    retry_after: Timestamp,
    next_periodic_at: Timestamp,
    pending_trigger: Option<NatProbeTriggerReason>,
    pending_demand: bool,
    profile: Option<NatProfile>,
    control_supported: bool,
}

#[derive(Clone, Debug, Default)]
pub(super) struct ProbeTransition {
    pub directive: Option<NatProbeDirective>,
    /// `Some(None)` invalidates publication, `Some(Some(_))` replaces it.
    pub profile_update: Option<Option<NatProfile>>,
}

pub(super) struct NatProbeScheduler {
    sn_peer_id: P2pId,
    ports: Vec<u16>,
    config_generation: u64,
    next_registration_generation: u64,
    next_request_id: u64,
    peers: HashMap<P2pId, PeerProbeState>,
}

impl NatProbeScheduler {
    pub fn new(sn_peer_id: P2pId) -> Self {
        Self {
            sn_peer_id,
            ports: Vec::new(),
            config_generation: 1,
            next_registration_generation: 1,
            next_request_id: 1,
            peers: HashMap::new(),
        }
    }

    pub fn set_ports(&mut self, mut ports: Vec<u16>) -> Vec<P2pId> {
        let configured_count = ports.len();
        ports.sort_unstable();
        let valid = (2..=MAX_NAT_PROBE_ENDPOINTS).contains(&ports.len())
            && ports.iter().all(|port| *port != 0)
            && ports.windows(2).all(|ports| ports[0] != ports[1]);
        if !valid {
            ports.clear();
        }
        if ports == self.ports {
            if configured_count > 0 && !valid {
                log::warn!(
                    "event=nat_probe_config_invalid sn_id={} config_generation={} configured_port_count={} effective_changed=false",
                    self.sn_peer_id,
                    self.config_generation,
                    configured_count
                );
            }
            return Vec::new();
        }

        self.ports = ports;
        self.config_generation = self.config_generation.wrapping_add(1).max(1);
        if valid || configured_count == 0 {
            log::info!(
                "event=nat_probe_config_changed sn_id={} config_generation={} port_count={}",
                self.sn_peer_id,
                self.config_generation,
                self.ports.len()
            );
        } else {
            log::warn!(
                "event=nat_probe_config_invalid sn_id={} config_generation={} configured_port_count={}",
                self.sn_peer_id,
                self.config_generation,
                configured_count
            );
        }
        let affected = self.peers.keys().cloned().collect();
        for (peer_id, state) in self.peers.iter_mut() {
            let invalidated = state.profile.is_some() || state.in_flight.is_some();
            state.config_generation = self.config_generation;
            state.in_flight = None;
            state.pending_trigger = Some(NatProbeTriggerReason::Config);
            state.profile = None;
            if invalidated {
                log::info!(
                    "event=nat_probe_profile_invalidated sn_id={} peer_id={} tunnel_id={:?} registration_generation={} config_generation={} reason=config_changed",
                    self.sn_peer_id,
                    peer_id,
                    state.authority_tunnel_id,
                    state.registration_generation,
                    state.config_generation
                );
            }
            log::debug!(
                "event=nat_probe_trigger_queued sn_id={} peer_id={} tunnel_id={:?} registration_generation={} config_generation={} trigger={}",
                self.sn_peer_id,
                peer_id,
                state.authority_tunnel_id,
                state.registration_generation,
                state.config_generation,
                NatProbeTriggerReason::Config.as_str()
            );
        }
        affected
    }

    pub fn set_sn_peer_id(&mut self, sn_peer_id: &P2pId) {
        if &self.sn_peer_id == sn_peer_id {
            return;
        }
        for (peer_id, state) in self.peers.iter() {
            log::info!(
                "event=nat_probe_authority_removed sn_id={} peer_id={} tunnel_id={:?} registration_generation={} config_generation={} reason=sn_identity_changed",
                self.sn_peer_id,
                peer_id,
                state.authority_tunnel_id,
                state.registration_generation,
                state.config_generation
            );
        }
        self.sn_peer_id = sn_peer_id.clone();
        self.peers.clear();
    }

    pub fn ports(&self) -> &[u16] {
        &self.ports
    }

    pub fn observe_capable_report(
        &mut self,
        peer_id: &P2pId,
        tunnel_id: CmdTunnelId,
        remote_endpoint: Endpoint,
        control_version: Option<u8>,
        result: Option<NatProbeResult>,
        now: Timestamp,
    ) -> ProbeTransition {
        if remote_endpoint.protocol() != Protocol::Quic {
            return self.observe_ineligible_report(peer_id, tunnel_id);
        }

        let mut transition = ProbeTransition::default();
        if self
            .peers
            .get(peer_id)
            .map(|state| state.authority_tunnel_id != tunnel_id)
            .unwrap_or(false)
        {
            // The service reconciles a missing authority before this call. If
            // it is still present, a concurrently reporting QUIC tunnel must
            // not flap the authoritative registration generation.
            log::debug!(
                "event=nat_probe_authority_observation_ignored sn_id={} peer_id={} tunnel_id={:?} reason=non_authority_quic_tunnel",
                self.sn_peer_id,
                peer_id,
                tunnel_id
            );
            return transition;
        }
        let had_registration = self.peers.contains_key(peer_id);
        let needs_registration = self
            .peers
            .get(peer_id)
            .map(|state| state.remote_endpoint.addr() != remote_endpoint.addr())
            .unwrap_or(true);
        if needs_registration {
            let trigger = if had_registration {
                NatProbeTriggerReason::ExternalAddress
            } else {
                NatProbeTriggerReason::Online
            };
            let generation = self.next_registration_generation;
            self.next_registration_generation =
                self.next_registration_generation.wrapping_add(1).max(1);
            self.peers.insert(
                peer_id.clone(),
                PeerProbeState {
                    authority_tunnel_id: tunnel_id,
                    remote_endpoint,
                    registration_generation: generation,
                    config_generation: self.config_generation,
                    in_flight: None,
                    retry_after: 0,
                    next_periodic_at: now,
                    pending_trigger: Some(trigger),
                    pending_demand: false,
                    profile: None,
                    control_supported: control_version == Some(NAT_PROBE_CONTROL_VERSION),
                },
            );
            transition.profile_update = Some(None);
            log::info!(
                "event=nat_probe_authority_established sn_id={} peer_id={} tunnel_id={:?} transport=quic registration_generation={} config_generation={} trigger={}",
                self.sn_peer_id,
                peer_id,
                tunnel_id,
                generation,
                self.config_generation,
                trigger.as_str()
            );
            log::debug!(
                "event=nat_probe_authority_endpoint sn_id={} peer_id={} tunnel_id={:?} registration_generation={} remote_endpoint={}",
                self.sn_peer_id,
                peer_id,
                tunnel_id,
                generation,
                remote_endpoint
            );
            log::debug!(
                "event=nat_probe_trigger_queued sn_id={} peer_id={} tunnel_id={:?} registration_generation={} config_generation={} trigger={}",
                self.sn_peer_id,
                peer_id,
                tunnel_id,
                generation,
                self.config_generation,
                trigger.as_str()
            );
            if had_registration {
                log::info!(
                    "event=nat_probe_profile_invalidated sn_id={} peer_id={} tunnel_id={:?} registration_generation={} config_generation={} reason=external_address_changed",
                    self.sn_peer_id,
                    peer_id,
                    tunnel_id,
                    generation,
                    self.config_generation
                );
            }
        } else if let Some(state) = self.peers.get_mut(peer_id) {
            let control_supported = control_version == Some(NAT_PROBE_CONTROL_VERSION);
            if state.control_supported && !control_supported {
                let invalidated = state.profile.is_some() || state.in_flight.is_some();
                state.in_flight = None;
                state.profile = None;
                transition.profile_update = Some(None);
                if invalidated {
                    log::info!(
                        "event=nat_probe_profile_invalidated sn_id={} peer_id={} tunnel_id={:?} registration_generation={} config_generation={} reason=capability_lost",
                        self.sn_peer_id,
                        peer_id,
                        state.authority_tunnel_id,
                        state.registration_generation,
                        state.config_generation
                    );
                }
                log::info!(
                    "event=nat_probe_capability_changed sn_id={} peer_id={} tunnel_id={:?} registration_generation={} supported=false",
                    self.sn_peer_id,
                    peer_id,
                    state.authority_tunnel_id,
                    state.registration_generation
                );
            } else if !state.control_supported && control_supported {
                log::info!(
                    "event=nat_probe_capability_changed sn_id={} peer_id={} tunnel_id={:?} registration_generation={} supported=true",
                    self.sn_peer_id,
                    peer_id,
                    state.authority_tunnel_id,
                    state.registration_generation
                );
            }
            state.control_supported = control_supported;
        }

        self.finish_expired(peer_id, now, &mut transition);
        if let Some(result) = result {
            self.accept_result(peer_id, result, now, &mut transition);
        }
        transition.directive = self.issue_if_due(peer_id, now);
        transition
    }

    #[cfg(test)]
    pub fn observe_report(
        &mut self,
        peer_id: &P2pId,
        tunnel_id: CmdTunnelId,
        remote_endpoint: Endpoint,
        result: Option<NatProbeResult>,
        now: Timestamp,
    ) -> ProbeTransition {
        self.observe_capable_report(
            peer_id,
            tunnel_id,
            remote_endpoint,
            Some(NAT_PROBE_CONTROL_VERSION),
            result,
            now,
        )
    }

    pub fn observe_control(
        &mut self,
        peer_id: &P2pId,
        tunnel_id: CmdTunnelId,
        remote_endpoint: Endpoint,
        now: Timestamp,
    ) -> ProbeTransition {
        if remote_endpoint.protocol() != Protocol::Quic {
            return self.observe_ineligible_report(peer_id, tunnel_id);
        }

        let mut transition = ProbeTransition::default();
        if self
            .peers
            .get(peer_id)
            .map(|state| state.authority_tunnel_id != tunnel_id)
            .unwrap_or(false)
        {
            log::debug!(
                "event=nat_probe_authority_observation_ignored sn_id={} peer_id={} tunnel_id={:?} reason=non_authority_control_tunnel",
                self.sn_peer_id,
                peer_id,
                tunnel_id
            );
            return transition;
        }
        let had_registration = self.peers.contains_key(peer_id);
        let needs_registration = self
            .peers
            .get(peer_id)
            .map(|state| state.remote_endpoint.addr() != remote_endpoint.addr())
            .unwrap_or(true);
        if needs_registration {
            let trigger = if had_registration {
                NatProbeTriggerReason::ExternalAddress
            } else {
                NatProbeTriggerReason::Online
            };
            let control_supported = self
                .peers
                .get(peer_id)
                .map(|state| state.control_supported)
                .unwrap_or(false);
            let generation = self.next_registration_generation;
            self.next_registration_generation =
                self.next_registration_generation.wrapping_add(1).max(1);
            self.peers.insert(
                peer_id.clone(),
                PeerProbeState {
                    authority_tunnel_id: tunnel_id,
                    remote_endpoint,
                    registration_generation: generation,
                    config_generation: self.config_generation,
                    in_flight: None,
                    retry_after: 0,
                    next_periodic_at: now,
                    pending_trigger: Some(trigger),
                    pending_demand: false,
                    profile: None,
                    control_supported,
                },
            );
            transition.profile_update = Some(None);
            log::info!(
                "event=nat_probe_authority_established sn_id={} peer_id={} tunnel_id={:?} transport=quic registration_generation={} config_generation={} trigger={}",
                self.sn_peer_id,
                peer_id,
                tunnel_id,
                generation,
                self.config_generation,
                trigger.as_str()
            );
            log::debug!(
                "event=nat_probe_authority_endpoint sn_id={} peer_id={} tunnel_id={:?} registration_generation={} remote_endpoint={}",
                self.sn_peer_id,
                peer_id,
                tunnel_id,
                generation,
                remote_endpoint
            );
            log::debug!(
                "event=nat_probe_trigger_queued sn_id={} peer_id={} tunnel_id={:?} registration_generation={} config_generation={} trigger={}",
                self.sn_peer_id,
                peer_id,
                tunnel_id,
                generation,
                self.config_generation,
                trigger.as_str()
            );
            if had_registration {
                log::info!(
                    "event=nat_probe_profile_invalidated sn_id={} peer_id={} tunnel_id={:?} registration_generation={} config_generation={} reason=external_address_changed",
                    self.sn_peer_id,
                    peer_id,
                    tunnel_id,
                    generation,
                    self.config_generation
                );
            }
        }
        self.finish_expired(peer_id, now, &mut transition);
        transition
    }

    fn observe_ineligible_report(
        &mut self,
        peer_id: &P2pId,
        tunnel_id: CmdTunnelId,
    ) -> ProbeTransition {
        let invalidate = self
            .peers
            .get(peer_id)
            .map(|state| state.authority_tunnel_id == tunnel_id)
            .unwrap_or(false);
        if invalidate {
            if let Some(state) = self.peers.remove(peer_id) {
                log::info!(
                    "event=nat_probe_authority_removed sn_id={} peer_id={} tunnel_id={:?} registration_generation={} config_generation={} reason=transport_ineligible",
                    self.sn_peer_id,
                    peer_id,
                    state.authority_tunnel_id,
                    state.registration_generation,
                    state.config_generation
                );
                if state.profile.is_some() || state.in_flight.is_some() {
                    log::info!(
                        "event=nat_probe_profile_invalidated sn_id={} peer_id={} tunnel_id={:?} registration_generation={} config_generation={} reason=transport_ineligible",
                        self.sn_peer_id,
                        peer_id,
                        state.authority_tunnel_id,
                        state.registration_generation,
                        state.config_generation
                    );
                }
            }
            ProbeTransition {
                directive: None,
                profile_update: Some(None),
            }
        } else {
            ProbeTransition::default()
        }
    }

    fn finish_expired(
        &mut self,
        peer_id: &P2pId,
        now: Timestamp,
        transition: &mut ProbeTransition,
    ) {
        let Some(state) = self.peers.get_mut(peer_id) else {
            return;
        };
        let expired = state
            .in_flight
            .as_ref()
            .map(|probe| now > probe.expires_at)
            .unwrap_or(false);
        if !expired {
            return;
        }
        let request_id = state
            .in_flight
            .as_ref()
            .map(|probe| probe.request_id)
            .unwrap_or(0);
        state.in_flight = None;
        state.profile = None;
        state.pending_trigger = None;
        state.retry_after = now.saturating_add(duration_to_bucky_time(NAT_PROBE_FAILURE_BACKOFF));
        state.next_periodic_at = now.saturating_add(duration_to_bucky_time(NAT_PROBE_PERIOD));
        transition.profile_update = Some(None);
        log::warn!(
            "event=nat_probe_directive_timeout sn_id={} peer_id={} tunnel_id={:?} registration_generation={} config_generation={} request_id={}",
            self.sn_peer_id,
            peer_id,
            state.authority_tunnel_id,
            state.registration_generation,
            state.config_generation,
            request_id
        );
        log::info!(
            "event=nat_probe_profile_invalidated sn_id={} peer_id={} tunnel_id={:?} registration_generation={} config_generation={} reason=directive_timeout",
            self.sn_peer_id,
            peer_id,
            state.authority_tunnel_id,
            state.registration_generation,
            state.config_generation
        );
    }

    fn accept_result(
        &mut self,
        peer_id: &P2pId,
        result: NatProbeResult,
        now: Timestamp,
        transition: &mut ProbeTransition,
    ) {
        let Some(state) = self.peers.get_mut(peer_id) else {
            log::debug!(
                "event=nat_probe_result_rejected sn_id={} peer_id={} registration_generation={} config_generation={} request_id={} reason={}",
                self.sn_peer_id,
                peer_id,
                result.registration_generation,
                result.probe_config_generation,
                result.request_id,
                NatProbeResultRejectReason::MissingAuthority.as_str()
            );
            return;
        };
        let profile_valid = result.profile.observation == NatMappingObservation::Unknown
            || result.profile.is_fresh(now);
        let rejection = if !state.control_supported {
            Some(NatProbeResultRejectReason::CapabilityUnsupported)
        } else if !result.is_supported() {
            Some(NatProbeResultRejectReason::VersionUnsupported)
        } else if !profile_valid {
            Some(NatProbeResultRejectReason::ProfileStale)
        } else if result.sn_peer_id != self.sn_peer_id {
            Some(NatProbeResultRejectReason::SnMismatch)
        } else if &result.peer_id != peer_id {
            Some(NatProbeResultRejectReason::PeerMismatch)
        } else if result.registration_generation != state.registration_generation {
            Some(NatProbeResultRejectReason::RegistrationGenerationMismatch)
        } else if result.probe_config_generation != state.config_generation {
            Some(NatProbeResultRejectReason::ConfigGenerationMismatch)
        } else if state.in_flight.is_none() {
            Some(NatProbeResultRejectReason::MissingInFlight)
        } else if state
            .in_flight
            .as_ref()
            .map(|probe| probe.request_id != result.request_id)
            .unwrap_or(false)
        {
            Some(NatProbeResultRejectReason::RequestMismatch)
        } else if state
            .in_flight
            .as_ref()
            .map(|probe| now > probe.expires_at)
            .unwrap_or(false)
        {
            Some(NatProbeResultRejectReason::DeadlineExpired)
        } else {
            None
        };
        if let Some(reason) = rejection {
            log::debug!(
                "event=nat_probe_result_rejected sn_id={} peer_id={} tunnel_id={:?} registration_generation={} config_generation={} request_id={} reason={}",
                self.sn_peer_id,
                peer_id,
                state.authority_tunnel_id,
                result.registration_generation,
                result.probe_config_generation,
                result.request_id,
                reason.as_str()
            );
            return;
        }

        state.in_flight = None;
        state.pending_trigger = None;
        state.pending_demand = false;
        state.next_periodic_at = now.saturating_add(duration_to_bucky_time(NAT_PROBE_PERIOD));
        let mut profile = result.profile;
        if profile.observation == NatMappingObservation::Unknown {
            state.profile = None;
            state.retry_after =
                now.saturating_add(duration_to_bucky_time(NAT_PROBE_FAILURE_BACKOFF));
            transition.profile_update = Some(None);
            log::info!(
                "event=nat_probe_result_unknown sn_id={} peer_id={} tunnel_id={:?} registration_generation={} config_generation={} request_id={} observation=unknown",
                self.sn_peer_id,
                peer_id,
                state.authority_tunnel_id,
                state.registration_generation,
                state.config_generation,
                result.request_id
            );
            log::info!(
                "event=nat_probe_profile_invalidated sn_id={} peer_id={} tunnel_id={:?} registration_generation={} config_generation={} reason=unknown_result",
                self.sn_peer_id,
                peer_id,
                state.authority_tunnel_id,
                state.registration_generation,
                state.config_generation
            );
        } else {
            profile.valid_until = state.next_periodic_at;
            state.retry_after = 0;
            state.profile = Some(profile.clone());
            transition.profile_update = Some(Some(profile));
            log::info!(
                "event=nat_probe_result_accepted sn_id={} peer_id={} tunnel_id={:?} registration_generation={} config_generation={} request_id={} observation={:?}",
                self.sn_peer_id,
                peer_id,
                state.authority_tunnel_id,
                state.registration_generation,
                state.config_generation,
                result.request_id,
                state
                    .profile
                    .as_ref()
                    .map(|profile| profile.observation)
                    .unwrap_or(NatMappingObservation::Unknown)
            );
        }
    }

    fn issue_if_due(&mut self, peer_id: &P2pId, now: Timestamp) -> Option<NatProbeDirective> {
        let in_flight_count = self
            .peers
            .values()
            .filter(|state| state.in_flight.is_some())
            .count();
        let state = self.peers.get_mut(peer_id)?;
        let event_due = state.pending_trigger;
        let demand_due = state.pending_demand && now >= state.retry_after;
        let periodic_due = now >= state.next_periodic_at;
        let trigger = if let Some(trigger) = event_due {
            Some(trigger)
        } else if demand_due {
            Some(NatProbeTriggerReason::Demand)
        } else if periodic_due {
            Some(NatProbeTriggerReason::Periodic)
        } else {
            None
        };
        let Some(trigger) = trigger else {
            if state.pending_demand && now < state.retry_after {
                log::debug!(
                    "event=nat_probe_directive_suppressed sn_id={} peer_id={} tunnel_id={:?} registration_generation={} config_generation={} trigger=demand reason=failure_backoff retry_after={}",
                    self.sn_peer_id,
                    peer_id,
                    state.authority_tunnel_id,
                    state.registration_generation,
                    state.config_generation,
                    state.retry_after
                );
            }
            return None;
        };
        let suppression = if self.ports.is_empty() {
            Some("no_probe_ports")
        } else if !state.control_supported {
            Some("capability_unsupported")
        } else if state.in_flight.is_some() {
            Some("in_flight")
        } else if in_flight_count >= MAX_CONCURRENT_NAT_PROBES {
            Some("global_capacity")
        } else {
            None
        };
        if let Some(reason) = suppression {
            log::debug!(
                "event=nat_probe_directive_suppressed sn_id={} peer_id={} tunnel_id={:?} registration_generation={} config_generation={} trigger={} reason={} in_flight_count={}",
                self.sn_peer_id,
                peer_id,
                state.authority_tunnel_id,
                state.registration_generation,
                state.config_generation,
                trigger.as_str(),
                reason,
                in_flight_count
            );
            return None;
        }

        let request_id = self.next_request_id;
        self.next_request_id = self.next_request_id.wrapping_add(1).max(1);
        let expires_at = now.saturating_add(duration_to_bucky_time(NAT_PROBE_DIRECTIVE_TIMEOUT));
        state.in_flight = Some(InFlightProbe {
            request_id,
            expires_at,
        });
        state.pending_trigger = None;
        state.pending_demand = false;
        log::info!(
            "event=nat_probe_directive_issued sn_id={} peer_id={} tunnel_id={:?} transport=quic registration_generation={} config_generation={} request_id={} trigger={} expires_at={} port_count={}",
            self.sn_peer_id,
            peer_id,
            state.authority_tunnel_id,
            state.registration_generation,
            state.config_generation,
            request_id,
            trigger.as_str(),
            expires_at,
            self.ports.len()
        );
        Some(NatProbeDirective {
            version: NAT_PROBE_CONTROL_VERSION,
            sn_peer_id: self.sn_peer_id.clone(),
            peer_id: peer_id.clone(),
            registration_generation: state.registration_generation,
            request_id,
            probe_config_generation: state.config_generation,
            expires_at,
            ports: self.ports.clone(),
        })
    }

    pub fn mark_demand(&mut self, peer_id: &P2pId, now: Timestamp) {
        let profile_missing = self.current_profile(peer_id, now).is_none();
        let Some(state) = self.peers.get_mut(peer_id) else {
            return;
        };
        if profile_missing && !state.pending_demand {
            state.pending_demand = true;
            log::debug!(
                "event=nat_probe_trigger_queued sn_id={} peer_id={} tunnel_id={:?} registration_generation={} config_generation={} trigger={}",
                self.sn_peer_id,
                peer_id,
                state.authority_tunnel_id,
                state.registration_generation,
                state.config_generation,
                NatProbeTriggerReason::Demand.as_str()
            );
        }
    }

    pub fn authority_tunnel(&self, peer_id: &P2pId) -> Option<CmdTunnelId> {
        self.peers
            .get(peer_id)
            .map(|state| state.authority_tunnel_id)
    }

    /// Snapshot of the current registration identity for a peer. Callers that
    /// observe the connection list across an await point must re-validate this
    /// snapshot with [`Self::remove_peer_if_authority`] before removing state,
    /// because a concurrent task may replace the registration in between.
    pub fn authority_registration(&self, peer_id: &P2pId) -> Option<(CmdTunnelId, u64)> {
        self.peers
            .get(peer_id)
            .map(|state| (state.authority_tunnel_id, state.registration_generation))
    }

    pub fn authorities(&self) -> Vec<(P2pId, CmdTunnelId)> {
        self.peers
            .iter()
            .map(|(peer_id, state)| (peer_id.clone(), state.authority_tunnel_id))
            .collect()
    }

    pub fn expire_due(&mut self, now: Timestamp) -> Vec<P2pId> {
        let peer_ids = self.peers.keys().cloned().collect::<Vec<_>>();
        let mut invalidated = Vec::new();
        for peer_id in peer_ids {
            let mut transition = ProbeTransition::default();
            self.finish_expired(&peer_id, now, &mut transition);
            if transition.profile_update.is_some() {
                invalidated.push(peer_id);
            }
        }
        invalidated
    }

    pub fn current_profile(&self, peer_id: &P2pId, now: Timestamp) -> Option<NatProfile> {
        self.peers
            .get(peer_id)
            .and_then(|state| state.profile.as_ref())
            .filter(|profile| profile.is_fresh(now))
            .cloned()
    }

    pub fn remove_peer(&mut self, peer_id: &P2pId, reason: NatProbeAuthorityRemovalReason) -> bool {
        let Some(state) = self.peers.remove(peer_id) else {
            return false;
        };
        log::info!(
            "event=nat_probe_authority_removed sn_id={} peer_id={} tunnel_id={:?} registration_generation={} config_generation={} reason={}",
            self.sn_peer_id,
            peer_id,
            state.authority_tunnel_id,
            state.registration_generation,
            state.config_generation,
            reason.as_str()
        );
        if state.profile.is_some() || state.in_flight.is_some() {
            log::info!(
                "event=nat_probe_profile_invalidated sn_id={} peer_id={} tunnel_id={:?} registration_generation={} config_generation={} reason={}",
                self.sn_peer_id,
                peer_id,
                state.authority_tunnel_id,
                state.registration_generation,
                state.config_generation,
                reason.as_str()
            );
        }
        true
    }

    /// Removes the peer's registration only when it still matches the
    /// `(authority_tunnel_id, registration_generation)` snapshot the caller
    /// captured before an await point. A mismatch means another task already
    /// replaced the registration (for example after a reconnect), so the stale
    /// reconciliation must give up instead of deleting the newer registration.
    pub fn remove_peer_if_authority(
        &mut self,
        peer_id: &P2pId,
        expected_tunnel_id: CmdTunnelId,
        expected_registration_generation: u64,
        reason: NatProbeAuthorityRemovalReason,
    ) -> bool {
        let matches_snapshot = self
            .peers
            .get(peer_id)
            .map(|state| {
                state.authority_tunnel_id == expected_tunnel_id
                    && state.registration_generation == expected_registration_generation
            })
            .unwrap_or(false);
        if !matches_snapshot {
            log::debug!(
                "event=nat_probe_authority_reconcile_skipped sn_id={} peer_id={} tunnel_id={:?} registration_generation={} reason=stale_snapshot",
                self.sn_peer_id,
                peer_id,
                expected_tunnel_id,
                expected_registration_generation
            );
            return false;
        }
        self.remove_peer(peer_id, reason)
    }

    #[cfg(test)]
    pub fn force_periodic_due(&mut self, peer_id: &P2pId, now: Timestamp) -> bool {
        let Some(state) = self.peers.get_mut(peer_id) else {
            return false;
        };
        if state.in_flight.is_some() {
            return false;
        }
        state.next_periodic_at = now;
        true
    }
}
