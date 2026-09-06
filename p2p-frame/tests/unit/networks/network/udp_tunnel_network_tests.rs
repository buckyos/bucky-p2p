use super::*;
use crate::error::{P2pErrorCode, p2p_err};

struct GenericTunnelNetwork;

struct UdpCapableTunnelNetwork {
    base: GenericTunnelNetwork,
}

#[async_trait::async_trait]
impl TunnelNetwork for GenericTunnelNetwork {
    fn protocol(&self) -> Protocol {
        Protocol::Tcp
    }

    fn is_udp(&self) -> bool {
        false
    }

    async fn listen(
        &self,
        _local: &Endpoint,
        _out: Option<Endpoint>,
        _mapping_port: Option<u16>,
        _on_incoming_tunnel: IncomingTunnelCallback,
    ) -> P2pResult<()> {
        Ok(())
    }

    async fn close_all_listener(&self) -> P2pResult<()> {
        Ok(())
    }

    fn listener_infos(&self) -> Vec<TunnelListenerInfo> {
        Vec::new()
    }

    async fn create_tunnel_with_intent(
        &self,
        _local_identity: &P2pIdentityRef,
        _remote: &Endpoint,
        _remote_id: &P2pId,
        _remote_name: Option<String>,
        _intent: TunnelConnectIntent,
    ) -> P2pResult<TunnelRef> {
        Err(p2p_err!(P2pErrorCode::NotSupport, "test network"))
    }

    async fn create_tunnel_with_local_ep_and_intent(
        &self,
        _local_identity: &P2pIdentityRef,
        _local_ep: &Endpoint,
        _remote: &Endpoint,
        _remote_id: &P2pId,
        _remote_name: Option<String>,
        _intent: TunnelConnectIntent,
    ) -> P2pResult<TunnelRef> {
        Err(p2p_err!(P2pErrorCode::NotSupport, "test network"))
    }
}

#[async_trait::async_trait]
impl TunnelNetwork for UdpCapableTunnelNetwork {
    fn protocol(&self) -> Protocol {
        Protocol::Quic
    }

    fn is_udp(&self) -> bool {
        true
    }

    fn as_udp_tunnel_network(&self) -> Option<&dyn UdpTunnelNetwork> {
        Some(self)
    }

    async fn listen(
        &self,
        local: &Endpoint,
        out: Option<Endpoint>,
        mapping_port: Option<u16>,
        on_incoming_tunnel: IncomingTunnelCallback,
    ) -> P2pResult<()> {
        self.base
            .listen(local, out, mapping_port, on_incoming_tunnel)
            .await
    }

    async fn close_all_listener(&self) -> P2pResult<()> {
        self.base.close_all_listener().await
    }

    fn listener_infos(&self) -> Vec<TunnelListenerInfo> {
        self.base.listener_infos()
    }

    async fn create_tunnel_with_intent(
        &self,
        local_identity: &P2pIdentityRef,
        remote: &Endpoint,
        remote_id: &P2pId,
        remote_name: Option<String>,
        intent: TunnelConnectIntent,
    ) -> P2pResult<TunnelRef> {
        self.base
            .create_tunnel_with_intent(local_identity, remote, remote_id, remote_name, intent)
            .await
    }

    async fn create_tunnel_with_local_ep_and_intent(
        &self,
        local_identity: &P2pIdentityRef,
        local_ep: &Endpoint,
        remote: &Endpoint,
        remote_id: &P2pId,
        remote_name: Option<String>,
        intent: TunnelConnectIntent,
    ) -> P2pResult<TunnelRef> {
        self.base
            .create_tunnel_with_local_ep_and_intent(
                local_identity,
                local_ep,
                remote,
                remote_id,
                remote_name,
                intent,
            )
            .await
    }
}

#[async_trait::async_trait]
impl UdpTunnelNetwork for UdpCapableTunnelNetwork {
    async fn punch_only(
        &self,
        _remote: &Endpoint,
        _intent: TunnelConnectIntent,
        _max_duration: std::time::Duration,
    ) -> P2pResult<()> {
        Err(p2p_err!(P2pErrorCode::NotSupport, "test network"))
    }

    async fn probe_nat_profile(
        &self,
        _probe_targets: &[Endpoint],
        _expected_signer: &crate::p2p_identity::P2pIdentityCertRef,
        _per_target_timeout: std::time::Duration,
        _ttl: std::time::Duration,
    ) -> P2pResult<crate::nat_type::NatProfile> {
        Err(p2p_err!(P2pErrorCode::NotSupport, "test network"))
    }

    async fn predict_traversal_endpoints(
        &self,
        _probe_targets: &[Endpoint],
        _expected_signer: &crate::p2p_identity::P2pIdentityCertRef,
        _per_target_timeout: std::time::Duration,
        _ttl: std::time::Duration,
    ) -> P2pResult<TraversalEndpointPrediction> {
        Err(p2p_err!(P2pErrorCode::NotSupport, "test network"))
    }

    fn validate_traversal_prediction(
        &self,
        _prediction: &TraversalEndpointPrediction,
        _now: crate::types::Timestamp,
    ) -> P2pResult<()> {
        Err(p2p_err!(P2pErrorCode::NotSupport, "test network"))
    }
}

#[test]
fn generic_tunnel_network_has_no_udp_capability_by_default() {
    let network: &dyn TunnelNetwork = &GenericTunnelNetwork;
    assert!(network.as_udp_tunnel_network().is_none());
}

#[test]
fn udp_capability_is_the_same_object_and_satisfies_tunnel_network() {
    fn as_tunnel_network<T: UdpTunnelNetwork>(network: &T) -> &dyn TunnelNetwork {
        network
    }

    let capable = UdpCapableTunnelNetwork {
        base: GenericTunnelNetwork,
    };
    let network = as_tunnel_network(&capable);
    let udp_network = network.as_udp_tunnel_network().unwrap();
    assert_eq!(network.protocol(), udp_network.protocol());
    assert_eq!(
        &capable as *const UdpCapableTunnelNetwork as *const (),
        udp_network as *const dyn UdpTunnelNetwork as *const ()
    );
}
