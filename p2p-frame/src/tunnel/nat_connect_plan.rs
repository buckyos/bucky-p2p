use crate::nat_type::{NatMappingObservation, NatTraversalContext};
use crate::sn::protocol::SnTunnelRendezvousOperation;
use crate::types::Timestamp;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PlanParty {
    Caller,
    Callee,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CandidateMode {
    Base,
    Predicted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConnectStrategy {
    Legacy,
    Public,
    Matrix,
    BoundedBestEffort,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PlanFallback {
    Legacy,
    Proxy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PlanAction {
    Legacy,
    Connect {
        candidates: CandidateMode,
        reverse: bool,
    },
    PunchThenWait {
        candidates: CandidateMode,
    },
    WaitIncoming,
}

/// A deterministic, ordered plan. Public reachability is supplied separately
/// from the profile because it may only come from independent static-WAN
/// evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ConnectPlan {
    pub strategy: ConnectStrategy,
    pub connector: Option<PlanParty>,
    pub connector_candidates: CandidateMode,
    pub peer_candidates: Option<CandidateMode>,
    pub reverse: bool,
    pub fallback: PlanFallback,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RendezvousCallerAction {
    Connect { use_predicted_response: bool },
    PunchThenWait { use_predicted_response: bool },
    WaitIncoming,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RendezvousPlan {
    pub operation: SnTunnelRendezvousOperation,
    pub request_candidates: Option<CandidateMode>,
    pub need_predict_endpoint: bool,
    pub caller_action: RendezvousCallerAction,
}

impl ConnectPlan {
    pub fn legacy() -> Self {
        Self {
            strategy: ConnectStrategy::Legacy,
            connector: None,
            connector_candidates: CandidateMode::Base,
            peer_candidates: None,
            reverse: false,
            fallback: PlanFallback::Legacy,
        }
    }

    pub fn action_for(&self, party: PlanParty) -> PlanAction {
        let Some(connector) = self.connector else {
            return PlanAction::Legacy;
        };

        if connector == party {
            return PlanAction::Connect {
                candidates: self.connector_candidates,
                reverse: self.reverse,
            };
        }

        match self.peer_candidates {
            Some(candidates) => PlanAction::PunchThenWait { candidates },
            None => PlanAction::WaitIncoming,
        }
    }

    pub fn rendezvous_plan(&self) -> Option<RendezvousPlan> {
        let connector = self.connector?;
        match connector {
            PlanParty::Caller => {
                let request_candidates = self.peer_candidates;
                Some(RendezvousPlan {
                    operation: if request_candidates.is_some() {
                        SnTunnelRendezvousOperation::PunchOnly
                    } else {
                        SnTunnelRendezvousOperation::WaitIncoming
                    },
                    request_candidates,
                    need_predict_endpoint: self.connector_candidates == CandidateMode::Predicted,
                    caller_action: RendezvousCallerAction::Connect {
                        use_predicted_response: self.connector_candidates
                            == CandidateMode::Predicted,
                    },
                })
            }
            PlanParty::Callee => {
                let caller_punch_candidates = self.peer_candidates;
                Some(RendezvousPlan {
                    operation: if caller_punch_candidates.is_some() {
                        SnTunnelRendezvousOperation::PunchAndReverseConnect
                    } else {
                        SnTunnelRendezvousOperation::ReverseConnectOnly
                    },
                    request_candidates: Some(self.connector_candidates),
                    need_predict_endpoint: caller_punch_candidates
                        == Some(CandidateMode::Predicted),
                    caller_action: match caller_punch_candidates {
                        Some(mode) => RendezvousCallerAction::PunchThenWait {
                            use_predicted_response: mode == CandidateMode::Predicted,
                        },
                        None => RendezvousCallerAction::WaitIncoming,
                    },
                })
            }
        }
    }
}

/// Select the caller/callee actions from one immutable ordered snapshot.
///
/// `caller_public` and `callee_public` are the only Public inputs and must be
/// derived by the caller from independent identity-cert static-WAN evidence.
pub(crate) fn select_connect_plan(
    context: &NatTraversalContext,
    now: Timestamp,
    caller_public: bool,
    callee_public: bool,
) -> ConnectPlan {
    // Public is independent of mapping observation. When both peers are
    // public, caller is the deterministic tie-break connector.
    if callee_public {
        return public_plan(PlanParty::Caller);
    }
    if caller_public {
        return public_plan(PlanParty::Callee);
    }

    if !context.is_supported() {
        return ConnectPlan::legacy();
    }

    let caller = context.caller_profile.mapping_at(now);
    let callee = context.callee_profile.mapping_at(now);
    if caller == NatMappingObservation::Unknown || callee == NatMappingObservation::Unknown {
        return ConnectPlan::legacy();
    }

    let caller_predictable = caller != NatMappingObservation::SymmetricLike
        || context.caller_profile.usable_prediction_hint(now).is_some();
    let callee_predictable = callee != NatMappingObservation::SymmetricLike
        || context.callee_profile.usable_prediction_hint(now).is_some();

    let strategy = if caller_predictable && callee_predictable {
        ConnectStrategy::Matrix
    } else {
        ConnectStrategy::BoundedBestEffort
    };

    match (caller, callee) {
        (NatMappingObservation::NonSymmetricLike, NatMappingObservation::NonSymmetricLike) => {
            matrix_plan(
                strategy,
                PlanParty::Caller,
                CandidateMode::Base,
                CandidateMode::Base,
            )
        }
        (NatMappingObservation::NonSymmetricLike, NatMappingObservation::SymmetricLike) => {
            matrix_plan(
                strategy,
                PlanParty::Callee,
                CandidateMode::Base,
                prediction_or_base(callee_predictable),
            )
        }
        (NatMappingObservation::SymmetricLike, NatMappingObservation::NonSymmetricLike) => {
            matrix_plan(
                strategy,
                PlanParty::Caller,
                CandidateMode::Base,
                prediction_or_base(caller_predictable),
            )
        }
        (NatMappingObservation::SymmetricLike, NatMappingObservation::SymmetricLike) => {
            matrix_plan(
                strategy,
                PlanParty::Caller,
                prediction_or_base(callee_predictable),
                prediction_or_base(caller_predictable),
            )
        }
        _ => ConnectPlan::legacy(),
    }
}

fn public_plan(connector: PlanParty) -> ConnectPlan {
    ConnectPlan {
        strategy: ConnectStrategy::Public,
        connector: Some(connector),
        connector_candidates: CandidateMode::Base,
        peer_candidates: None,
        reverse: connector == PlanParty::Callee,
        fallback: PlanFallback::Proxy,
    }
}

fn matrix_plan(
    strategy: ConnectStrategy,
    connector: PlanParty,
    connector_candidates: CandidateMode,
    peer_candidates: CandidateMode,
) -> ConnectPlan {
    ConnectPlan {
        strategy,
        connector: Some(connector),
        connector_candidates,
        peer_candidates: Some(peer_candidates),
        reverse: connector == PlanParty::Callee,
        fallback: PlanFallback::Proxy,
    }
}

fn prediction_or_base(prediction_usable: bool) -> CandidateMode {
    if prediction_usable {
        CandidateMode::Predicted
    } else {
        CandidateMode::Base
    }
}

#[cfg(test)]
mod tests {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/unit/tunnel/nat_connect_plan/tests.rs"
    ));
}
