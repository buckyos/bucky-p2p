use crate::endpoint::Endpoint;
use crate::error::P2pResult;
use crate::nat_type::NatProfile;
use crate::p2p_identity::P2pIdentityCertRef;
use crate::types::Timestamp;
use std::time::Duration;

use super::{TraversalEndpointPrediction, TunnelConnectIntent, TunnelNetwork};

#[async_trait::async_trait]
pub trait UdpTunnelNetwork: TunnelNetwork {
    async fn punch_only(
        &self,
        remote: &Endpoint,
        intent: TunnelConnectIntent,
        max_duration: Duration,
    ) -> P2pResult<()>;

    /// Probe the same-SN-IP UDP mapping and classify it without requiring any
    /// prediction candidates. Implementations must keep a valid mapping
    /// observation even when ports are unpredictable.
    async fn probe_nat_profile(
        &self,
        probe_targets: &[Endpoint],
        expected_signer: &P2pIdentityCertRef,
        per_target_timeout: Duration,
        ttl: Duration,
    ) -> P2pResult<NatProfile>;

    async fn predict_traversal_endpoints(
        &self,
        probe_targets: &[Endpoint],
        expected_signer: &P2pIdentityCertRef,
        per_target_timeout: Duration,
        ttl: Duration,
    ) -> P2pResult<TraversalEndpointPrediction>;

    fn validate_traversal_prediction(
        &self,
        prediction: &TraversalEndpointPrediction,
        now: Timestamp,
    ) -> P2pResult<()>;
}
