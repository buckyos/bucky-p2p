use super::*;
use crate::networks::{
    DeviceFinder, IncomingStreamCallback, IncomingTunnelCallback, TcpTunnelNetwork, Tunnel,
    TunnelConnectIntent, TunnelManager, TunnelNetwork, TunnelPurpose, allow_all_listen_vports,
};
use crate::p2p_identity::{P2pIdentityCertRef, P2pIdentityRef};
use crate::tls::TlsServerCertResolver;
use crate::tunnel::DefaultP2pConnectionInfoCache;
use crate::types::{TunnelCandidateId, TunnelId, TunnelIdGenerator};
use crate::x509::{X509IdentityCertFactory, X509IdentityFactory, generate_rsa_x509_identity};
use async_named_locker::Locker;
use sfo_reuseport::{ServerRuntime, ServerRuntimeConfig};
use std::collections::HashMap;
use std::sync::{Arc, Once};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::timeout;

static TLS_INIT: Once = Once::new();
const RUNTIME_TEST_CHANNEL_CAPACITY: usize = 8;

struct StaticDeviceFinder {
    devices: HashMap<P2pId, P2pIdentityCertRef>,
}

#[async_trait::async_trait]
impl DeviceFinder for StaticDeviceFinder {
    async fn get_identity_cert(&self, device_id: &P2pId) -> P2pResult<P2pIdentityCertRef> {
        self.devices
            .get(device_id)
            .cloned()
            .ok_or_else(|| p2p_err!(P2pErrorCode::NotFound, "device not found"))
    }
}

fn init_tls_once() {
    TLS_INIT.call_once(|| crate::tls::init_tls(Arc::new(X509IdentityFactory)));
}

fn new_identity(name: &str) -> P2pIdentityRef {
    Arc::new(generate_rsa_x509_identity(Some(name.to_owned())).unwrap())
}

fn loopback_tcp_ep() -> Endpoint {
    Endpoint::from((Protocol::Tcp, "127.0.0.1:0".parse().unwrap()))
}

fn ignore_incoming() -> IncomingTunnelCallback {
    Arc::new(|_| Box::pin(async {}))
}

#[tokio::test]
async fn duplicate_control_rejection_preserves_original_data_connection_route() {
    init_tls_once();

    let caller_identity = new_identity("tcp-post-accept-caller");
    let callee_identity = new_identity("tcp-post-accept-callee");

    let callee_resolver = DefaultTlsServerCertResolver::new();
    callee_resolver
        .add_server_identity(callee_identity.clone())
        .await
        .unwrap();
    let callee_network = Arc::new(TcpTunnelNetwork::new(
        callee_resolver.clone(),
        Arc::new(X509IdentityCertFactory),
        Duration::from_secs(3),
        Duration::from_millis(200),
        Duration::from_secs(5),
        ServerRuntime::start(ServerRuntimeConfig::default()).unwrap(),
    ));
    let callee_net_manager =
        NetManager::new(vec![callee_network.clone()], callee_resolver.clone()).unwrap();
    let callee_manager = TunnelManager::new(
        callee_identity.clone(),
        Some(Arc::new(StaticDeviceFinder {
            devices: HashMap::from([(
                caller_identity.get_id(),
                caller_identity.get_identity_cert().unwrap(),
            )]),
        })),
        callee_net_manager.clone(),
        None,
        Arc::new(X509IdentityCertFactory),
        None,
        DefaultP2pConnectionInfoCache::new(),
        Arc::new(TunnelIdGenerator::new()),
        Duration::from_secs(3),
        Duration::from_secs(30),
        Duration::from_secs(300),
    )
    .unwrap();
    let (published_tx, mut published_rx) = mpsc::channel(RUNTIME_TEST_CHANNEL_CAPACITY);
    callee_manager.subscribe(Arc::new(move |result| {
        let published_tx = published_tx.clone();
        Box::pin(async move {
            let _ = published_tx.send(result).await;
        })
    }));
    callee_net_manager
        .listen(&[loopback_tcp_ep()], None)
        .await
        .unwrap();
    let callee_endpoint = callee_net_manager.get_listener_info(Protocol::Tcp)[0].local;

    let caller_resolver = DefaultTlsServerCertResolver::new();
    caller_resolver
        .add_server_identity(caller_identity.clone())
        .await
        .unwrap();
    let caller_network = Arc::new(TcpTunnelNetwork::new(
        caller_resolver,
        Arc::new(X509IdentityCertFactory),
        Duration::from_secs(3),
        Duration::from_millis(200),
        Duration::from_secs(5),
        ServerRuntime::start(ServerRuntimeConfig::default()).unwrap(),
    ));
    caller_network
        .listen(&loopback_tcp_ep(), None, None, ignore_incoming())
        .await
        .unwrap();

    let tunnel_id = TunnelId::from(0x7001);
    let candidate_id = TunnelCandidateId::from(0x7101);
    let intent = TunnelConnectIntent::active(tunnel_id, candidate_id);
    let first = caller_network
        .create_tunnel_with_intent(
            &caller_identity,
            &callee_endpoint,
            &callee_identity.get_id(),
            Some(callee_identity.get_name()),
            intent,
        )
        .await
        .unwrap();
    let accepted = timeout(Duration::from_secs(3), published_rx.recv())
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    let (stream_tx, mut stream_rx) = mpsc::channel(RUNTIME_TEST_CHANNEL_CAPACITY);
    let stream_callback: IncomingStreamCallback = Arc::new(move |result| {
        let stream_tx = stream_tx.clone();
        Box::pin(async move {
            let _ = stream_tx.send(result).await;
        })
    });
    accepted
        .listen_stream(allow_all_listen_vports(), stream_callback)
        .await
        .unwrap();

    let registration_guard =
        Locker::get_locker(format!("network-register-{}", caller_identity.get_id())).await;
    let duplicate = caller_network
        .create_tunnel_with_intent(
            &caller_identity,
            &callee_endpoint,
            &callee_identity.get_id(),
            Some(callee_identity.get_name()),
            intent,
        )
        .await
        .unwrap();

    let pending_purpose = TunnelPurpose::from_value(&7200u16).unwrap();
    let (pending_opened, pending_incoming) = tokio::join!(
        first.open_stream(pending_purpose.clone()),
        timeout(Duration::from_secs(3), stream_rx.recv())
    );
    let (_pending_read, _pending_write) = pending_opened
        .expect("old tunnel must remain routable while upper duplicate decision is pending");
    let (received_pending_purpose, _pending_peer_read, _pending_peer_write) = pending_incoming
        .unwrap()
        .unwrap()
        .expect("old accepted tunnel must receive data while duplicate is pending");
    assert_eq!(received_pending_purpose, pending_purpose);

    drop(registration_guard);
    assert!(
        timeout(Duration::from_millis(200), published_rx.recv())
            .await
            .is_err(),
        "duplicate candidate must not be published"
    );

    let purpose = TunnelPurpose::from_value(&7201u16).unwrap();
    let (opened, incoming) = tokio::join!(
        first.open_stream(purpose.clone()),
        timeout(Duration::from_secs(3), stream_rx.recv())
    );
    let (_read, _write) = opened.expect("original tunnel data connection must remain routable");
    let (incoming_purpose, _peer_read, _peer_write) = incoming
        .unwrap()
        .unwrap()
        .expect("accepted tunnel must receive the data connection");
    assert_eq!(incoming_purpose, purpose);

    let _ = duplicate.close();
    let _ = first.close();
    let _ = accepted.close();
    callee_network.close_all_listener().await.unwrap();
    caller_network.close_all_listener().await.unwrap();
}
