use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Arc;
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::Duration;

use tokio::sync::mpsc;

use crate::endpoint::{Endpoint, Protocol};
use crate::error::{P2pErrorCode, P2pResult};
use crate::p2p_identity::{P2pId, P2pIdentityCertFactory, P2pIdentityRef, P2pSn};
use crate::sn::protocol::v0::{SnCalled, TunnelType};
use crate::sn::service::{SnServerRef, SnServiceConfig, create_sn_service};
use crate::stack::{P2pConfig, P2pStackConfig, P2pStackRef, create_p2p_env, create_p2p_stack};
use crate::types::TunnelId;
use crate::x509::{X509IdentityCertFactory, X509IdentityFactory, generate_rsa_x509_identity};

const ONLINE_TIMEOUT: Duration = Duration::from_secs(10);
const CALL_TIMEOUT: Duration = Duration::from_secs(5);
const SETUP_MAX_RETRY: usize = 20;
static NEXT_PORT: AtomicU16 = AtomicU16::new(42000);

fn next_port() -> u16 {
    NEXT_PORT.fetch_add(1, Ordering::Relaxed)
}

fn localhost_quic_endpoint(port: u16) -> Endpoint {
    Endpoint::from((
        Protocol::Quic,
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)),
    ))
}

fn build_identity(name: &str, endpoint: Endpoint) -> P2pIdentityRef {
    let identity = generate_rsa_x509_identity(Some(name.to_owned())).unwrap();
    let identity: P2pIdentityRef = Arc::new(identity);
    identity.update_endpoints(vec![endpoint])
}

fn build_sn_entry(sn_identity: &P2pIdentityRef) -> P2pSn {
    let sn_cert = sn_identity.get_identity_cert().unwrap();
    P2pSn::new(sn_cert.get_id(), sn_cert.get_name(), sn_cert.endpoints())
}

fn is_addr_bind_conflict(code: P2pErrorCode) -> bool {
    code == P2pErrorCode::AddrInUse || code == P2pErrorCode::AddrNotAvailable
}

async fn start_sn_service(
    sn_identity: P2pIdentityRef,
    identity_factory: Arc<X509IdentityFactory>,
    cert_factory: Arc<X509IdentityCertFactory>,
) -> P2pResult<SnServerRef> {
    let service = create_sn_service(SnServiceConfig::new(
        sn_identity,
        identity_factory,
        cert_factory,
    ))
    .await;
    service.start().await?;
    Ok(service)
}

async fn start_client_stack(
    client_identity: P2pIdentityRef,
    sn_list: Vec<P2pSn>,
    identity_factory: Arc<X509IdentityFactory>,
    cert_factory: Arc<X509IdentityCertFactory>,
) -> P2pResult<P2pStackRef> {
    let endpoint = *client_identity.endpoints().first().unwrap();
    let env = create_p2p_env(
        P2pConfig::new(identity_factory, cert_factory, vec![endpoint])
            .set_tcp_accept_timout(Duration::from_secs(3))
            .set_tcp_connect_timout(Duration::from_secs(3))
            .set_quic_connect_timeout(Duration::from_secs(3))
            .set_quic_idle_time(Duration::from_secs(10)),
    )
    .await?;

    create_p2p_stack(
        P2pStackConfig::new(env, client_identity)
            .add_sn_list(sn_list)
            .set_conn_timeout(Duration::from_secs(3))
            .set_sn_ping_interval(Duration::from_millis(200))
            .set_sn_call_timeout(Duration::from_secs(3))
            .set_sn_query_interval(Duration::from_secs(1))
            .set_sn_tunnel_count(2),
    )
    .await
}

async fn setup_sn_and_one_client(
    name: &str,
) -> (
    SnServerRef,
    P2pStackRef,
    P2pId,
    P2pId,
    Arc<X509IdentityCertFactory>,
) {
    for _ in 0..SETUP_MAX_RETRY {
        let identity_factory = Arc::new(X509IdentityFactory);
        let cert_factory = Arc::new(X509IdentityCertFactory);

        let sn_identity = build_identity("sn-server", localhost_quic_endpoint(next_port()));
        let sn_id = sn_identity.get_id();
        let sn_service = match start_sn_service(
            sn_identity.clone(),
            identity_factory.clone(),
            cert_factory.clone(),
        )
        .await
        {
            Ok(service) => service,
            Err(e) => {
                if is_addr_bind_conflict(e.code()) {
                    continue;
                }
                panic!("start sn service failed: {:?}", e);
            }
        };

        let client_identity = build_identity(name, localhost_quic_endpoint(next_port()));
        let client_id = client_identity.get_id();
        let stack = match start_client_stack(
            client_identity,
            vec![build_sn_entry(&sn_identity)],
            identity_factory,
            cert_factory.clone(),
        )
        .await
        {
            Ok(stack) => stack,
            Err(e) => {
                if is_addr_bind_conflict(e.code()) {
                    continue;
                }
                panic!("start client stack failed: {:?}", e);
            }
        };
        if let Err(e) = stack.wait_online(Some(ONLINE_TIMEOUT)).await {
            panic!("wait client online failed: {:?}", e);
        }

        return (sn_service, stack, sn_id, client_id, cert_factory);
    }

    panic!("setup sn and one client failed after retries");
}

async fn setup_sn_and_two_clients() -> (
    SnServerRef,
    P2pStackRef,
    P2pId,
    P2pStackRef,
    P2pId,
    P2pId,
    Arc<X509IdentityCertFactory>,
) {
    for _ in 0..SETUP_MAX_RETRY {
        let identity_factory = Arc::new(X509IdentityFactory);
        let cert_factory = Arc::new(X509IdentityCertFactory);

        let sn_identity = build_identity("sn-server", localhost_quic_endpoint(next_port()));
        let sn_id = sn_identity.get_id();
        let sn_service = match start_sn_service(
            sn_identity.clone(),
            identity_factory.clone(),
            cert_factory.clone(),
        )
        .await
        {
            Ok(service) => service,
            Err(e) => {
                if is_addr_bind_conflict(e.code()) {
                    continue;
                }
                panic!("start sn service failed: {:?}", e);
            }
        };
        let sn_list = vec![build_sn_entry(&sn_identity)];

        let client_a_identity = build_identity("sn-client-a", localhost_quic_endpoint(next_port()));
        let client_a_id = client_a_identity.get_id();
        let client_a = match start_client_stack(
            client_a_identity,
            sn_list.clone(),
            identity_factory.clone(),
            cert_factory.clone(),
        )
        .await
        {
            Ok(stack) => stack,
            Err(e) => {
                if is_addr_bind_conflict(e.code()) {
                    continue;
                }
                panic!("start client-a stack failed: {:?}", e);
            }
        };

        let client_b_identity = build_identity("sn-client-b", localhost_quic_endpoint(next_port()));
        let client_b_id = client_b_identity.get_id();
        let client_b = match start_client_stack(
            client_b_identity,
            sn_list,
            identity_factory,
            cert_factory.clone(),
        )
        .await
        {
            Ok(stack) => stack,
            Err(e) => {
                if is_addr_bind_conflict(e.code()) {
                    continue;
                }
                panic!("start client-b stack failed: {:?}", e);
            }
        };

        if let Err(e) = client_a.wait_online(Some(ONLINE_TIMEOUT)).await {
            panic!("wait client-a online failed: {:?}", e);
        }
        if let Err(e) = client_b.wait_online(Some(ONLINE_TIMEOUT)).await {
            panic!("wait client-b online failed: {:?}", e);
        }

        return (
            sn_service,
            client_a,
            client_a_id,
            client_b,
            client_b_id,
            sn_id,
            cert_factory,
        );
    }

    panic!("setup sn and two clients failed after retries");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sn_client_wait_online_report_resp_ok() {
    let (_sn_service, stack, sn_id, _client_id, _cert_factory) =
        setup_sn_and_one_client("sn-client").await;

    let active_sn_list = stack.sn_client().get_active_sn_list();
    assert!(!active_sn_list.is_empty());
    assert!(active_sn_list.iter().any(|item| item.sn_peer_id == sn_id));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sn_client_query_registered_peer_returns_full_info() {
    let (_sn_service, stack, _sn_id, client_id, cert_factory) =
        setup_sn_and_one_client("sn-query-client").await;

    let query_resp = stack.sn_client().query(&client_id).await.unwrap();
    assert!(query_resp.peer_info.is_some());
    assert!(!query_resp.end_point_array.is_empty());

    let peer_cert = cert_factory
        .create(query_resp.peer_info.as_ref().unwrap())
        .unwrap();
    assert_eq!(peer_cert.get_id(), client_id);
    assert!(
        query_resp
            .end_point_array
            .iter()
            .any(|ep| ep.protocol() == Protocol::Quic && ep.addr().port() > 0)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sn_client_query_unknown_peer_returns_empty_info() {
    let (_sn_service, stack, _sn_id, _client_id, _cert_factory) =
        setup_sn_and_one_client("sn-query-miss").await;

    let unknown_id = P2pId::from(vec![0x5A; 32]);
    let query_resp = stack.sn_client().query(&unknown_id).await.unwrap();
    assert!(query_resp.peer_info.is_none());
    assert!(query_resp.end_point_array.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sn_call_stream_path_field_level_assertions() {
    let (_sn_service, caller_stack, caller_id, callee_stack, callee_id, sn_id, cert_factory) =
        setup_sn_and_two_clients().await;

    let (tx, mut rx) = mpsc::channel::<SnCalled>(1);
    callee_stack
        .sn_client()
        .set_listener(move |called: SnCalled| {
            let tx = tx.clone();
            async move {
                let _ = tx.send(called).await;
                Ok(())
            }
        });

    let reverse_eps = vec![
        localhost_quic_endpoint(next_port()),
        localhost_quic_endpoint(next_port()),
    ];
    let tunnel_id: TunnelId = 0x1001u32.into();
    let payload = b"sn-stream-call-payload".to_vec();

    let call_resp = caller_stack
        .sn_client()
        .call(
            tunnel_id,
            Some(reverse_eps.as_slice()),
            &callee_id,
            TunnelType::Stream,
            payload.clone(),
        )
        .await
        .unwrap();

    assert_eq!(call_resp.result, P2pErrorCode::Ok.as_u8());
    assert!(call_resp.to_peer_info.is_some());
    let to_peer_cert = cert_factory
        .create(call_resp.to_peer_info.as_ref().unwrap())
        .unwrap();
    assert_eq!(to_peer_cert.get_id(), callee_id);

    let called = tokio::time::timeout(CALL_TIMEOUT, rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(called.to_peer_id, callee_id);
    assert_eq!(called.sn_peer_id, sn_id);
    assert_eq!(called.call_type, TunnelType::Stream);
    assert_eq!(called.tunnel_id, tunnel_id);
    assert_eq!(called.payload, payload);
    assert!(called.reverse_endpoint_array.len() >= reverse_eps.len());
    assert_eq!(
        &called.reverse_endpoint_array[..reverse_eps.len()],
        reverse_eps.as_slice()
    );
    assert!(called.active_pn_list.is_empty());

    let from_peer_cert = cert_factory.create(&called.peer_info).unwrap();
    assert_eq!(from_peer_cert.get_id(), caller_id);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sn_call_datagram_path_field_level_assertions() {
    let (_sn_service, caller_stack, caller_id, callee_stack, callee_id, sn_id, cert_factory) =
        setup_sn_and_two_clients().await;

    let (tx, mut rx) = mpsc::channel::<SnCalled>(1);
    callee_stack
        .sn_client()
        .set_listener(move |called: SnCalled| {
            let tx = tx.clone();
            async move {
                let _ = tx.send(called).await;
                Ok(())
            }
        });

    let reverse_eps = vec![localhost_quic_endpoint(next_port())];
    let tunnel_id: TunnelId = 0x1002u32.into();
    let payload = b"sn-datagram-call-payload".to_vec();

    let call_resp = caller_stack
        .sn_client()
        .call(
            tunnel_id,
            Some(reverse_eps.as_slice()),
            &callee_id,
            TunnelType::Datagram,
            payload.clone(),
        )
        .await
        .unwrap();

    assert_eq!(call_resp.result, P2pErrorCode::Ok.as_u8());
    assert!(call_resp.to_peer_info.is_some());
    let to_peer_cert = cert_factory
        .create(call_resp.to_peer_info.as_ref().unwrap())
        .unwrap();
    assert_eq!(to_peer_cert.get_id(), callee_id);

    let called = tokio::time::timeout(CALL_TIMEOUT, rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(called.to_peer_id, callee_id);
    assert_eq!(called.sn_peer_id, sn_id);
    assert_eq!(called.call_type, TunnelType::Datagram);
    assert_eq!(called.tunnel_id, tunnel_id);
    assert_eq!(called.payload, payload);
    assert!(called.reverse_endpoint_array.len() >= reverse_eps.len());
    assert_eq!(
        &called.reverse_endpoint_array[..reverse_eps.len()],
        reverse_eps.as_slice()
    );
    assert!(called.active_pn_list.is_empty());

    let from_peer_cert = cert_factory.create(&called.peer_info).unwrap();
    assert_eq!(from_peer_cert.get_id(), caller_id);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sn_call_unknown_peer_returns_not_found() {
    let (_sn_service, stack, _sn_id, _client_id, _cert_factory) =
        setup_sn_and_one_client("sn-call-miss").await;

    let unknown_id = P2pId::from(vec![0x88; 32]);
    let call_resp = stack
        .sn_client()
        .call(
            0x1003u32.into(),
            None,
            &unknown_id,
            TunnelType::Stream,
            b"missing-target".to_vec(),
        )
        .await
        .unwrap();

    assert_eq!(call_resp.result, P2pErrorCode::NotFound.as_u8());
    assert!(call_resp.to_peer_info.is_none());
}
