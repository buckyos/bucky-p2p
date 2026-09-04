use super::*;
use crate::error::{P2pErrorCode, p2p_err};

struct DefaultPunchNetwork;

#[async_trait::async_trait]
impl TunnelNetwork for DefaultPunchNetwork {
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

#[test]
fn generic_network_has_no_udp_traversal_capability_by_default() {
    let network: &dyn TunnelNetwork = &DefaultPunchNetwork;
    assert!(network.as_udp_tunnel_network().is_none());
}
