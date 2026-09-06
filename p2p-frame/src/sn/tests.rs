use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::atomic::{AtomicU16, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex, Once};
use std::time::Duration;

use tokio::sync::mpsc;

use crate::endpoint::{Endpoint, Protocol};
use crate::error::{P2pErrorCode, P2pResult};
use crate::p2p_identity::{P2pId, P2pIdentityCertFactory, P2pIdentityRef, P2pSn};
use crate::sn::protocol::v0::{SnCallResp, SnCalled, TunnelType};
use crate::sn::protocol::{PackageCmdCode, ReportSn, ReportSnResp, SnCall, SnQuery, SnQueryResp};
use crate::sn::service::{SnServerRef, SnServiceConfig, create_sn_service};
use crate::sn::types::{SnTunnelClassification, SnTunnelRead};
use crate::stack::{P2pConfig, P2pStackConfig, P2pStackRef, create_p2p_env, create_p2p_stack};
use crate::types::{Sequence, TunnelId};
use crate::x509::{X509IdentityCertFactory, X509IdentityFactory, generate_rsa_x509_identity};
use bucky_raw_codec::{RawConvertTo, RawFrom};
use sfo_cmd_server::CmdBody;
use sfo_cmd_server::client::{ClassifiedCmdClient, ClassifiedCmdTunnelRead, CmdClient};
use sfo_cmd_server::server::CmdServer;
use sfo_reuseport::{ServerRuntime, ServerRuntimeConfig};

#[path = "../../tests/sn_command_matrix/five_by_five_command_matrix_tests.rs"]
mod five_by_five_command_matrix_tests;
#[path = "../../tests/nat_type_aware/sn_profile_flow_tests.rs"]
mod sn_profile_flow_tests;
#[path = "../../tests/tunnel_rendezvous/sn_same_sn_tests.rs"]
mod sn_same_sn_tests;

const ONLINE_TIMEOUT: Duration = Duration::from_secs(10);
const CALL_TIMEOUT: Duration = Duration::from_secs(5);
const SETUP_MAX_RETRY: usize = 20;
const TEST_PORT_LOW: u16 = 25025;
const TEST_PORT_HIGH_CAP: u16 = 43100;
static NEXT_PORT: AtomicU16 = AtomicU16::new(0);
static TEST_PORT_HIGH: LazyLock<u16> = LazyLock::new(|| {
    #[cfg(target_os = "linux")]
    if let Some(start) = read_ephemeral_port_start() {
        return start
            .saturating_sub(1)
            .clamp(TEST_PORT_LOW + 1, TEST_PORT_HIGH_CAP);
    }
    TEST_PORT_HIGH_CAP
});

struct NatProbeTestLogger {
    records: Mutex<Vec<(log::Level, String)>>,
}

static NAT_PROBE_TEST_LOGGER: NatProbeTestLogger = NatProbeTestLogger {
    records: Mutex::new(Vec::new()),
};
static NAT_PROBE_TEST_LOGGER_INIT: Once = Once::new();

impl log::Log for NatProbeTestLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Debug
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let message = record.args().to_string();
        if message.contains("event=nat_probe_") {
            self.records.lock().unwrap().push((record.level(), message));
        }
    }

    fn flush(&self) {}
}

pub(crate) fn enable_nat_probe_test_logging() {
    NAT_PROBE_TEST_LOGGER_INIT.call_once(|| {
        log::set_logger(&NAT_PROBE_TEST_LOGGER).unwrap();
        log::set_max_level(log::LevelFilter::Debug);
    });
}

pub(crate) fn nat_probe_test_logs() -> Vec<(log::Level, String)> {
    NAT_PROBE_TEST_LOGGER.records.lock().unwrap().clone()
}

#[cfg(target_os = "linux")]
fn read_ephemeral_port_start() -> Option<u16> {
    let contents = std::fs::read_to_string("/proc/sys/net/ipv4/ip_local_port_range").ok()?;
    let mut parts = contents.split_whitespace();
    let start = parts.next()?.parse::<u16>().ok()?;
    let _end = parts.next()?.parse::<u16>().ok()?;
    Some(start)
}

fn next_port() -> u16 {
    let high = *TEST_PORT_HIGH;
    let mut current = NEXT_PORT.load(Ordering::Relaxed);
    loop {
        let next = if current == 0 || current <= TEST_PORT_LOW {
            high
        } else {
            current - 1
        };
        match NEXT_PORT.compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => {
                assert!(
                    next >= TEST_PORT_LOW && next <= high,
                    "test port {} outside reserved test range {}..={}",
                    next,
                    TEST_PORT_LOW,
                    high
                );
                return next;
            }
            Err(observed) => current = observed,
        }
    }
}

#[test]
fn next_port_stays_outside_os_ephemeral_range() {
    let mut seen = std::collections::HashSet::new();
    #[cfg(target_os = "linux")]
    let ephemeral_start = read_ephemeral_port_start();
    for _ in 0..64 {
        let port = next_port();
        assert!(
            port >= TEST_PORT_LOW && port <= *TEST_PORT_HIGH,
            "port {port} outside reserved test range {}..={}",
            TEST_PORT_LOW,
            *TEST_PORT_HIGH
        );
        assert!(
            seen.insert(port),
            "next_port reused a port within the sampled window: {port}"
        );
        #[cfg(target_os = "linux")]
        if let Some(start) = ephemeral_start {
            assert!(
                port < start,
                "test port {port} must stay below OS ephemeral start {start}"
            );
        }
    }
}

fn localhost_quic_endpoint(port: u16) -> Endpoint {
    Endpoint::from((
        Protocol::Quic,
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)),
    ))
}

fn localhost_tcp_endpoint(port: u16) -> Endpoint {
    Endpoint::from((
        Protocol::Tcp,
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

fn test_server_runtime() -> ServerRuntime {
    ServerRuntime::start(ServerRuntimeConfig::new().with_workers(1)).unwrap()
}

#[test]
fn sn_tunnel_read_keeps_actual_metadata_separate_from_requested_classification() {
    let actual_local = localhost_tcp_endpoint(43210);
    let actual_remote = localhost_tcp_endpoint(43211);
    let requested_local = localhost_tcp_endpoint(0);
    let requested = SnTunnelClassification::new(Some(requested_local), actual_remote);
    let local_id = P2pId::default();
    let remote_id = P2pId::default();

    let (_peer, stream) = tokio::io::duplex(64);
    let (read, _write) = tokio::io::split(stream);
    let classified = SnTunnelRead::new_with_classification(
        Box::pin(read),
        actual_local,
        actual_remote,
        local_id.clone(),
        remote_id.clone(),
        requested.clone(),
    );
    assert_eq!(classified.local(), actual_local);
    assert_eq!(classified.remote(), actual_remote);
    assert_eq!(classified.get_classification(), requested);

    let (_peer, stream) = tokio::io::duplex(64);
    let (read, _write) = tokio::io::split(stream);
    let existing = SnTunnelRead::new(
        Box::pin(read),
        actual_local,
        actual_remote,
        local_id,
        remote_id,
    );
    assert_eq!(existing.local(), actual_local);
    assert_eq!(existing.remote(), actual_remote);
    assert_eq!(
        existing.get_classification(),
        SnTunnelClassification::new(Some(actual_local), actual_remote)
    );
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
        test_server_runtime(),
    ))
    .await?;
    service.start().await?;
    Ok(service)
}

#[tokio::test]
async fn create_sn_service_accepts_external_server_runtime() {
    let identity_factory = Arc::new(X509IdentityFactory);
    let cert_factory = Arc::new(X509IdentityCertFactory);
    let sn_identity = build_identity("sn-server", localhost_quic_endpoint(next_port()));

    let result = create_sn_service(SnServiceConfig::new(
        sn_identity,
        identity_factory,
        cert_factory,
        test_server_runtime(),
    ))
    .await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn external_server_runtime_can_be_shared_by_sn_and_stack() {
    let identity_factory = Arc::new(X509IdentityFactory);
    let cert_factory = Arc::new(X509IdentityCertFactory);
    let server_runtime = test_server_runtime();
    let sn_identity = build_identity("shared-runtime-sn", localhost_quic_endpoint(next_port()));

    let sn_service = create_sn_service(SnServiceConfig::new(
        sn_identity,
        identity_factory.clone(),
        cert_factory.clone(),
        server_runtime.clone(),
    ))
    .await;
    assert!(sn_service.is_ok());

    let stack_env = create_p2p_env(P2pConfig::new(
        identity_factory,
        cert_factory,
        vec![localhost_quic_endpoint(next_port())],
        server_runtime,
    ))
    .await;
    assert!(stack_env.is_ok());
}

async fn start_client_stack(
    client_identity: P2pIdentityRef,
    sn_list: Vec<P2pSn>,
    identity_factory: Arc<X509IdentityFactory>,
    cert_factory: Arc<X509IdentityCertFactory>,
) -> P2pResult<P2pStackRef> {
    let endpoint = *client_identity.endpoints().first().unwrap();
    let env = create_p2p_env(
        P2pConfig::new(
            identity_factory,
            cert_factory,
            vec![endpoint],
            test_server_runtime(),
        )
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

async fn start_client_stack_with_endpoints(
    client_identity: P2pIdentityRef,
    sn_list: Vec<P2pSn>,
    identity_factory: Arc<X509IdentityFactory>,
    cert_factory: Arc<X509IdentityCertFactory>,
    endpoints: Vec<Endpoint>,
    call_timeout: Duration,
) -> P2pResult<P2pStackRef> {
    let env = create_p2p_env(
        P2pConfig::new(
            identity_factory,
            cert_factory,
            endpoints,
            test_server_runtime(),
        )
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
            .set_sn_ping_interval(Duration::from_millis(50))
            .set_sn_call_timeout(call_timeout)
            .set_sn_query_interval(Duration::from_secs(1))
            .set_sn_tunnel_count(2),
    )
    .await
}

pub(super) async fn setup_sn_and_one_client(
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

async fn setup_tcp_sn_and_one_client(
    name: &str,
) -> (
    SnServerRef,
    P2pStackRef,
    P2pId,
    P2pId,
    Endpoint,
    Arc<X509IdentityCertFactory>,
) {
    for _ in 0..SETUP_MAX_RETRY {
        let identity_factory = Arc::new(X509IdentityFactory);
        let cert_factory = Arc::new(X509IdentityCertFactory);

        let sn_identity = build_identity("sn-tcp-server", localhost_tcp_endpoint(next_port()));
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
                panic!("start tcp sn service failed: {:?}", e);
            }
        };

        let client_identity = build_identity(name, localhost_tcp_endpoint(next_port()));
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
                panic!("start tcp client stack failed: {:?}", e);
            }
        };
        let sn_endpoint = *sn_identity.endpoints().first().unwrap();
        return (
            sn_service,
            stack,
            sn_id,
            client_id,
            sn_endpoint,
            cert_factory,
        );
    }

    panic!("setup tcp sn and one client failed after retries");
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
async fn sn_client_connects_to_sn_over_tcp_tunnel() {
    let (_sn_service, stack, _sn_id, client_id, sn_endpoint, _cert_factory) =
        setup_tcp_sn_and_one_client("sn-tcp-client").await;

    assert_eq!(sn_endpoint.protocol(), Protocol::Tcp);
    assert!(sn_endpoint.addr().ip().is_loopback());
    assert!(sn_endpoint.addr().port() > 0);

    let tunnel_id = stack
        .sn_client()
        .get_cmd_client()
        .find_tunnel_id_by_classified(SnTunnelClassification::new(None, sn_endpoint))
        .await
        .unwrap();
    assert_ne!(tunnel_id.value(), 0);
    assert_eq!(stack.local_identity().get_id(), client_id);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sn_client_query_registered_peer_returns_full_info() {
    let (_sn_service, stack, _sn_id, client_id, cert_factory) =
        setup_sn_and_one_client("sn-query-client").await;

    let query_resp = stack.sn_client().query(&client_id).await.unwrap();
    assert!(query_resp.peer_info.is_some());

    let peer_cert = cert_factory
        .create(query_resp.peer_info.as_ref().unwrap())
        .unwrap();
    assert_eq!(peer_cert.get_id(), client_id);
    #[cfg(not(feature = "test-real-socket-matrix"))]
    assert!(query_resp.end_point_array.is_empty());
    #[cfg(feature = "test-real-socket-matrix")]
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sn_client_server_connection_covers_report_query_call_and_called_response() {
    let (_sn_service, caller_stack, caller_id, callee_stack, callee_id, sn_id, cert_factory) =
        setup_sn_and_two_clients().await;

    let caller_active = caller_stack.sn_client().get_active_sn_list();
    assert_eq!(caller_active.len(), 1);
    assert_eq!(caller_active[0].sn_peer_id, sn_id);

    let callee_active = callee_stack.sn_client().get_active_sn_list();
    assert_eq!(callee_active.len(), 1);
    assert_eq!(callee_active[0].sn_peer_id, sn_id);

    let caller_query = caller_stack.sn_client().query(&caller_id).await.unwrap();
    assert!(caller_query.peer_info.is_some());
    let caller_cert = cert_factory
        .create(caller_query.peer_info.as_ref().unwrap())
        .unwrap();
    assert_eq!(caller_cert.get_id(), caller_id);

    let callee_query = caller_stack.sn_client().query(&callee_id).await.unwrap();
    assert!(callee_query.peer_info.is_some());
    let callee_cert = cert_factory
        .create(callee_query.peer_info.as_ref().unwrap())
        .unwrap();
    assert_eq!(callee_cert.get_id(), callee_id);
    #[cfg(not(feature = "test-real-socket-matrix"))]
    assert!(callee_query.end_point_array.is_empty());
    #[cfg(feature = "test-real-socket-matrix")]
    assert!(
        callee_query
            .end_point_array
            .iter()
            .any(|ep| ep.protocol() == Protocol::Quic && ep.addr().port() > 0)
    );

    let missing_peer = P2pId::from(vec![0x44; 32]);
    let missing_query = caller_stack.sn_client().query(&missing_peer).await.unwrap();
    assert!(missing_query.peer_info.is_none());
    assert!(missing_query.end_point_array.is_empty());

    let (called_tx, mut called_rx) = mpsc::channel::<SnCalled>(1);
    callee_stack
        .sn_client()
        .set_listener(move |called: SnCalled| {
            let called_tx = called_tx.clone();
            async move {
                let _ = called_tx.send(called).await;
                Ok(())
            }
        });

    let reverse_eps = vec![
        localhost_quic_endpoint(next_port()),
        localhost_quic_endpoint(next_port()),
    ];
    let tunnel_id: TunnelId = 0x2001u32.into();
    let payload = b"sn-client-server-control-stream-e2e".to_vec();

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

    let called = tokio::time::timeout(CALL_TIMEOUT, called_rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(called.sn_peer_id, sn_id);
    assert_eq!(called.to_peer_id, callee_id);
    assert_eq!(called.tunnel_id, tunnel_id);
    assert_eq!(called.call_type, TunnelType::Stream);
    assert_eq!(called.payload, payload);
    assert_eq!(
        &called.reverse_endpoint_array[..reverse_eps.len()],
        reverse_eps.as_slice()
    );
    let from_peer_cert = cert_factory.create(&called.peer_info).unwrap();
    assert_eq!(from_peer_cert.get_id(), caller_id);

    let unknown_call_resp = caller_stack
        .sn_client()
        .call(
            0x2002u32.into(),
            None,
            &missing_peer,
            TunnelType::Datagram,
            b"missing-peer".to_vec(),
        )
        .await
        .unwrap();
    assert_eq!(unknown_call_resp.result, P2pErrorCode::NotFound.as_u8());
    assert!(unknown_call_resp.to_peer_info.is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sn_report_late_response_does_not_complete_next_tunnel_report() {
    let identity_factory = Arc::new(X509IdentityFactory);
    let cert_factory = Arc::new(X509IdentityCertFactory);
    let sn_endpoints = vec![
        localhost_quic_endpoint(next_port()),
        localhost_tcp_endpoint(next_port()),
    ];
    let sn_identity =
        build_identity("sn-late-report", sn_endpoints[0]).update_endpoints(sn_endpoints.clone());
    let sn_id = sn_identity.get_id();
    let sn_service = create_sn_service(SnServiceConfig::new(
        sn_identity.clone(),
        identity_factory.clone(),
        cert_factory.clone(),
        test_server_runtime(),
    ))
    .await
    .unwrap();

    let attempts = Arc::new(AtomicUsize::new(0));
    let late_endpoint = localhost_quic_endpoint(next_port());
    let current_endpoint = localhost_quic_endpoint(next_port());
    let handler_attempts = attempts.clone();
    let handler_sn_id = sn_id.clone();
    sn_service.get_cmd_server().register_cmd_handler(
        PackageCmdCode::ReportSn as u8,
        move |_local_id, _peer_id, _tunnel_id, _header, mut body: CmdBody| {
            let attempts = handler_attempts.clone();
            let sn_id = handler_sn_id.clone();
            async move {
                let report = ReportSn::clone_from_slice(body.read_all().await?.as_slice()).unwrap();
                let attempt = attempts.fetch_add(1, Ordering::SeqCst);
                let (delay, endpoint) = if attempt == 0 {
                    (Duration::from_millis(2200), late_endpoint)
                } else {
                    (Duration::from_millis(600), current_endpoint)
                };
                tokio::time::sleep(delay).await;
                let resp = ReportSnResp {
                    seq: report.seq,
                    sn_peer_id: sn_id,
                    result: P2pErrorCode::Ok.as_u8(),
                    peer_info: None,
                    end_point_array: vec![endpoint],
                    receipt: None,
                    nat_probe_ports: vec![],
                    nat_probe_directive: None,
                };
                Ok(Some(CmdBody::from(resp.to_vec().unwrap())))
            }
        },
    );
    sn_service.start().await.unwrap();

    let client_endpoints = vec![
        localhost_quic_endpoint(next_port()),
        localhost_tcp_endpoint(next_port()),
    ];
    let client_identity = build_identity("client-late-report", client_endpoints[0])
        .update_endpoints(client_endpoints.clone());
    let stack = start_client_stack_with_endpoints(
        client_identity,
        vec![build_sn_entry(&sn_identity)],
        identity_factory,
        cert_factory,
        client_endpoints,
        Duration::from_secs(1),
    )
    .await
    .unwrap();

    stack
        .wait_online(Some(Duration::from_secs(10)))
        .await
        .unwrap();
    assert!(attempts.load(Ordering::SeqCst) >= 2);
    let active = stack.sn_client().get_active_sn_list();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].sn_peer_id, sn_id);
    assert_eq!(active[0].wan_ep_list, vec![current_endpoint]);
    assert!(!active[0].wan_ep_list.contains(&late_endpoint));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sn_report_rejects_wrong_business_seq_and_sn_identity() {
    let identity_factory = Arc::new(X509IdentityFactory);
    let cert_factory = Arc::new(X509IdentityCertFactory);
    let sn_endpoints = vec![
        localhost_quic_endpoint(next_port()),
        localhost_tcp_endpoint(next_port()),
    ];
    let sn_identity = build_identity("sn-wrong-report-body", sn_endpoints[0])
        .update_endpoints(sn_endpoints.clone());
    let sn_id = sn_identity.get_id();
    let sn_service = create_sn_service(SnServiceConfig::new(
        sn_identity.clone(),
        identity_factory.clone(),
        cert_factory.clone(),
        test_server_runtime(),
    ))
    .await
    .unwrap();

    let attempts = Arc::new(AtomicUsize::new(0));
    let handler_attempts = attempts.clone();
    let handler_sn_id = sn_id.clone();
    sn_service.get_cmd_server().register_cmd_handler(
        PackageCmdCode::ReportSn as u8,
        move |_local_id, _peer_id, _tunnel_id, _header, mut body: CmdBody| {
            let attempts = handler_attempts.clone();
            let sn_id = handler_sn_id.clone();
            async move {
                let report = ReportSn::clone_from_slice(body.read_all().await?.as_slice()).unwrap();
                let attempt = attempts.fetch_add(1, Ordering::SeqCst);
                if attempt == 2 {
                    return Ok(Some(CmdBody::from(vec![0xFF])));
                }
                let (seq, response_sn_id) = if attempt == 0 {
                    (Sequence::from(report.seq.value().wrapping_add(1)), sn_id)
                } else {
                    (report.seq, P2pId::from(vec![0xA5; 32]))
                };
                let resp = ReportSnResp {
                    seq,
                    sn_peer_id: response_sn_id,
                    result: P2pErrorCode::Ok.as_u8(),
                    peer_info: None,
                    end_point_array: vec![localhost_quic_endpoint(next_port())],
                    receipt: None,
                    nat_probe_ports: vec![],
                    nat_probe_directive: None,
                };
                Ok(Some(CmdBody::from(resp.to_vec().unwrap())))
            }
        },
    );
    sn_service.start().await.unwrap();

    let client_endpoints = vec![
        localhost_quic_endpoint(next_port()),
        localhost_tcp_endpoint(next_port()),
    ];
    let client_identity = build_identity("client-wrong-report-body", client_endpoints[0])
        .update_endpoints(client_endpoints.clone());
    let stack = start_client_stack_with_endpoints(
        client_identity,
        vec![build_sn_entry(&sn_identity)],
        identity_factory,
        cert_factory,
        client_endpoints,
        Duration::from_millis(300),
    )
    .await
    .unwrap();

    tokio::time::timeout(Duration::from_secs(10), async {
        while attempts.load(Ordering::SeqCst) < 3 {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    assert!(attempts.load(Ordering::SeqCst) >= 3);
    assert!(stack.sn_client().get_active_sn_list().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sn_call_and_query_reject_mismatched_qa_response_bodies() {
    let (sn_service, stack, sn_id, client_id, _cert_factory) =
        setup_sn_and_one_client("sn-call-query-mismatch").await;
    let wrong_sn_id = P2pId::from(vec![0x5A; 32]);
    sn_service.get_cmd_server().register_cmd_handler(
        PackageCmdCode::SnCall as u8,
        move |_local_id, _peer_id, _tunnel_id, _header, mut body: CmdBody| {
            let wrong_sn_id = wrong_sn_id.clone();
            async move {
                let call = SnCall::clone_from_slice(body.read_all().await?.as_slice()).unwrap();
                let resp = SnCallResp {
                    seq: call.seq,
                    sn_peer_id: wrong_sn_id,
                    result: P2pErrorCode::Ok.as_u8(),
                    to_peer_info: None,
                };
                Ok(Some(CmdBody::from(resp.to_vec().unwrap())))
            }
        },
    );
    sn_service.get_cmd_server().register_cmd_handler(
        PackageCmdCode::SnQuery as u8,
        move |_local_id, _peer_id, _tunnel_id, _header, mut body: CmdBody| async move {
            let query = SnQuery::clone_from_slice(body.read_all().await?.as_slice()).unwrap();
            let resp = SnQueryResp {
                seq: Sequence::from(query.seq.value().wrapping_add(1)),
                peer_info: None,
                end_point_array: vec![],
                net_profile: None,
                target_protocol_version: None,
            };
            Ok(Some(CmdBody::from(resp.to_vec().unwrap())))
        },
    );

    let call_err = stack
        .sn_client()
        .call(
            0x3001u32.into(),
            None,
            &client_id,
            TunnelType::Stream,
            b"mismatched-call-response".to_vec(),
        )
        .await
        .unwrap_err();
    assert_eq!(call_err.code(), P2pErrorCode::ConnectFailed);
    assert_eq!(stack.sn_client().get_active_sn_list()[0].sn_peer_id, sn_id);

    let query_err = stack.sn_client().query(&client_id).await.unwrap_err();
    assert_eq!(query_err.code(), P2pErrorCode::ConnectFailed);
    assert_eq!(stack.sn_client().get_active_sn_list()[0].sn_peer_id, sn_id);

    sn_service.get_cmd_server().register_cmd_handler(
        PackageCmdCode::SnCall as u8,
        move |_local_id, _peer_id, _tunnel_id, _header, _body: CmdBody| async move {
            Ok(Some(CmdBody::from(vec![0xFF])))
        },
    );
    sn_service.get_cmd_server().register_cmd_handler(
        PackageCmdCode::SnQuery as u8,
        move |_local_id, _peer_id, _tunnel_id, _header, _body: CmdBody| async move {
            Ok(Some(CmdBody::from(vec![0xFF])))
        },
    );

    let malformed_call_err = stack
        .sn_client()
        .call(
            0x3002u32.into(),
            None,
            &client_id,
            TunnelType::Datagram,
            b"malformed-call-response".to_vec(),
        )
        .await
        .unwrap_err();
    assert_eq!(malformed_call_err.code(), P2pErrorCode::ConnectFailed);
    let malformed_query_err = stack.sn_client().query(&client_id).await.unwrap_err();
    assert_eq!(malformed_query_err.code(), P2pErrorCode::ConnectFailed);
    assert_eq!(stack.sn_client().get_active_sn_list()[0].sn_peer_id, sn_id);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sn_call_and_query_timeouts_preserve_healthy_active_sn() {
    let (sn_service, caller, caller_id, query_client, query_id, sn_id, _cert_factory) =
        setup_sn_and_two_clients().await;
    let call_sn_id = sn_id.clone();
    sn_service.get_cmd_server().register_cmd_handler(
        PackageCmdCode::SnCall as u8,
        move |_local_id, _peer_id, _tunnel_id, _header, mut body: CmdBody| {
            let sn_id = call_sn_id.clone();
            async move {
                let call = SnCall::clone_from_slice(body.read_all().await?.as_slice()).unwrap();
                tokio::time::sleep(Duration::from_secs(4)).await;
                let resp = SnCallResp {
                    seq: call.seq,
                    sn_peer_id: sn_id,
                    result: P2pErrorCode::Ok.as_u8(),
                    to_peer_info: None,
                };
                Ok(Some(CmdBody::from(resp.to_vec().unwrap())))
            }
        },
    );
    sn_service.get_cmd_server().register_cmd_handler(
        PackageCmdCode::SnQuery as u8,
        move |_local_id, _peer_id, _tunnel_id, _header, mut body: CmdBody| async move {
            let query = SnQuery::clone_from_slice(body.read_all().await?.as_slice()).unwrap();
            tokio::time::sleep(Duration::from_secs(4)).await;
            let resp = SnQueryResp {
                seq: query.seq,
                peer_info: None,
                end_point_array: vec![],
                net_profile: None,
                target_protocol_version: None,
            };
            Ok(Some(CmdBody::from(resp.to_vec().unwrap())))
        },
    );

    let call = caller.sn_client().call(
        0x3003u32.into(),
        None,
        &query_id,
        TunnelType::Stream,
        b"timeout-call".to_vec(),
    );
    let query = query_client.sn_client().query(&caller_id);
    let (call_result, query_result) = tokio::join!(call, query);

    assert_eq!(call_result.unwrap_err().code(), P2pErrorCode::ConnectFailed);
    assert_eq!(
        query_result.unwrap_err().code(),
        P2pErrorCode::ConnectFailed
    );
    assert_eq!(caller.sn_client().get_active_sn_list()[0].sn_peer_id, sn_id);
    assert_eq!(
        query_client.sn_client().get_active_sn_list()[0].sn_peer_id,
        sn_id
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sn_call_and_query_closed_tunnels_remove_stale_active_sn() {
    let (_sn_service, caller, _caller_id, query_client, query_id, _sn_id, _cert_factory) =
        setup_sn_and_two_clients().await;

    caller.sn_client().get_cmd_client().clear_all_tunnel().await;
    query_client
        .sn_client()
        .get_cmd_client()
        .clear_all_tunnel()
        .await;

    let call_err = caller
        .sn_client()
        .call(
            0x3004u32.into(),
            None,
            &query_id,
            TunnelType::Stream,
            b"closed-call-tunnel".to_vec(),
        )
        .await
        .unwrap_err();
    let query_err = query_client.sn_client().query(&query_id).await.unwrap_err();

    assert_eq!(call_err.code(), P2pErrorCode::ConnectFailed);
    assert_eq!(query_err.code(), P2pErrorCode::ConnectFailed);
    assert!(caller.sn_client().get_active_sn_list().is_empty());
    assert!(query_client.sn_client().get_active_sn_list().is_empty());
}
