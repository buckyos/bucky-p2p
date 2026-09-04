use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Arc;
use std::time::Duration;

use p2p_frame::ConnectDirection;
use p2p_frame::endpoint::Endpoint;
use p2p_frame::endpoint::{EndpointArea, Protocol};
use p2p_frame::error::{P2pError, P2pErrorCode, P2pResult};
use p2p_frame::nat_type::NatMappingObservation;
use p2p_frame::p2p_identity::P2pIdentityRef;
use p2p_frame::pn::{PnServer, PnServerRef};
use p2p_frame::stack::{
    P2pConfig, P2pPn, P2pStackConfig, PnServerAddress, create_p2p_env, create_p2p_stack,
};
use p2p_frame::ttp::TtpServer;
use p2p_frame::x509::{X509IdentityCertFactory, X509IdentityFactory};

use super::fixture::{
    AbsoluteDeadline, ConnectionInfoRecorder, DEFAULT_FLOW_TIMEOUT, DEFAULT_SETUP_TIMEOUT,
    RealNode, assert_bidirectional_unique_payload, connect_and_exchange_from_id,
    connect_stream_pair_from_id, single_worker_runtime, start_two_node_sn, unique_purpose,
    x509_identity,
};

struct RealPnTopology {
    server: PnServerRef,
    caller: RealNode,
    target: RealNode,
}

impl Drop for RealPnTopology {
    fn drop(&mut self) {
        self.caller.stack.sn_client().stop();
        self.target.stack.sn_client().stop();
        self.server.stop();
    }
}

fn loopback_tcp_zero() -> Endpoint {
    let mut endpoint = Endpoint::from((
        Protocol::Tcp,
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)),
    ));
    endpoint.set_area(EndpointArea::Lan);
    endpoint
}

async fn start_proxy_node(
    label: &str,
    pn: P2pPn,
    identity_factory: Arc<X509IdentityFactory>,
    cert_factory: Arc<X509IdentityCertFactory>,
) -> P2pResult<RealNode> {
    let requested_endpoint = loopback_tcp_zero();
    let identity = x509_identity(label, requested_endpoint)?;
    let id = identity.get_id();
    let connection_info = ConnectionInfoRecorder::new();
    let env = create_p2p_env(
        P2pConfig::new(
            identity_factory,
            cert_factory,
            vec![requested_endpoint],
            single_worker_runtime(),
        )
        .set_connection_info_cache(connection_info.clone())
        .set_tcp_accept_timout(Duration::from_secs(2))
        .set_tcp_connect_timout(Duration::from_secs(2)),
    )
    .await?;
    let stack = create_p2p_stack(
        P2pStackConfig::new(env, identity.clone())
            .set_conn_timeout(Duration::from_secs(2))
            .set_support_proxy(true)
            .set_proxy_server(PnServerAddress::Server(pn)),
    )
    .await?;
    let endpoint = stack
        .get_listen_eps(Protocol::Tcp)
        .and_then(|entries| entries.first().map(|entry| entry.0))
        .ok_or_else(|| {
            P2pError::new(
                P2pErrorCode::NotFound,
                "proxy node has no TCP listener".to_owned(),
            )
        })?;
    Ok(RealNode {
        stack,
        identity,
        id,
        endpoint,
        connection_info,
    })
}

async fn start_real_pn_topology() -> P2pResult<RealPnTopology> {
    let identity_factory = Arc::new(X509IdentityFactory);
    let cert_factory = Arc::new(X509IdentityCertFactory);
    let requested_endpoint = loopback_tcp_zero();
    let pn_identity: P2pIdentityRef = x509_identity("real-flow-pn", requested_endpoint)?;
    let env = create_p2p_env(P2pConfig::new(
        identity_factory.clone(),
        cert_factory.clone(),
        vec![requested_endpoint],
        single_worker_runtime(),
    ))
    .await?;
    let manager = env.net_manager().clone();
    manager.add_listen_device(pn_identity.clone()).await?;
    manager.listen(&[requested_endpoint], None).await?;
    let pn_endpoint = manager
        .get_listener_info(Protocol::Tcp)
        .first()
        .map(|info| info.local)
        .ok_or_else(|| {
            P2pError::new(P2pErrorCode::NotFound, "PN has no TCP listener".to_owned())
        })?;
    let descriptor = P2pPn::new(
        pn_identity.get_id(),
        pn_identity.get_name(),
        vec![pn_endpoint],
    );
    let server = PnServer::new(TtpServer::new(pn_identity, manager)?);
    server.start().await?;
    let target = start_proxy_node(
        "real-flow-pn-target",
        descriptor.clone(),
        identity_factory.clone(),
        cert_factory.clone(),
    )
    .await?;
    let caller = start_proxy_node(
        "real-flow-pn-caller",
        descriptor,
        identity_factory,
        cert_factory,
    )
    .await?;
    Ok(RealPnTopology {
        server,
        caller,
        target,
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn missing_profile_uses_real_legacy_socket_and_payload() -> P2pResult<()> {
    let deadline = AbsoluteDeadline::after(DEFAULT_SETUP_TIMEOUT + DEFAULT_FLOW_TIMEOUT);
    let topology = start_two_node_sn(
        Protocol::Tcp,
        EndpointArea::Lan,
        EndpointArea::Lan,
        deadline,
    )
    .await?;

    let query = deadline
        .p2p(
            "querying the real SN for a missing NAT profile",
            topology
                .caller
                .stack
                .sn_client()
                .query_with_context(&topology.target.id),
        )
        .await?;
    assert_eq!(query.sn_peer_id, topology.sn_id);
    assert_eq!(
        query.local_net_profile.observation,
        NatMappingObservation::Unknown
    );
    assert!(query.response.net_profile.is_none());
    assert!(query.response.peer_info.is_some());

    let _streams = connect_and_exchange_from_id(
        &topology.caller,
        &topology.target,
        "missing-profile-legacy",
        deadline,
    )
    .await?;

    let info = topology
        .caller
        .connection_info
        .latest(&topology.target.id)
        .expect("real legacy tunnel must publish connection info");
    assert!(matches!(
        info.direct,
        ConnectDirection::Direct | ConnectDirection::Reverse
    ));

    // Unknown planning and expired-to-missing freshness are deterministic
    // production-contract branches covered by the task plan's bounded unit
    // cases; this socket test intentionally does not inject stale SN state or
    // wait for the production two-hour profile TTL.
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn no_sn_lookup_uses_real_pn_proxy_and_payload() -> P2pResult<()> {
    let deadline = AbsoluteDeadline::after(DEFAULT_SETUP_TIMEOUT + DEFAULT_FLOW_TIMEOUT);
    let topology = deadline
        .p2p("starting real PN topology", start_real_pn_topology())
        .await?;
    let purpose = unique_purpose("pn-proxy-fallback");
    let mut streams =
        connect_stream_pair_from_id(&topology.caller, &topology.target, purpose, deadline).await?;
    let info = topology
        .caller
        .connection_info
        .wait_for_direction(&topology.target.id, ConnectDirection::Proxy, deadline)
        .await?;
    assert_eq!(info.direct, ConnectDirection::Proxy);
    assert_bidirectional_unique_payload(&mut streams, "pn-proxy-fallback", deadline).await
}
