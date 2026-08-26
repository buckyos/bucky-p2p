use super::super::TcpTunnelNetwork;
use crate::endpoint::{Endpoint, Protocol};
use crate::error::{P2pError, P2pErrorCode, P2pResult};
use crate::networks::tcp::protocol::{
    ClaimConnAckResult, OpenDataConnResp, OpenDataConnRespResult, PingCmd, TcpControlCmd,
    TcpLeaseSeq, TcpTunnelWireDecode, TcpTunnelWireEncode,
};
use crate::networks::tcp::tunnel::{
    ReverseOpenCorrelation, ReverseOpenCorrelationError, ReverseOpenCorrelationEvent,
    ReverseOpenCorrelationOutcome, TcpTunnel, local_simultaneous_claim_wins,
    validate_remote_first_claim,
};
use crate::networks::{
    IncomingStream, IncomingTunnelCallback, ListenVPortRegistry, Tunnel, TunnelNetwork,
    TunnelPurpose, TunnelRef, TunnelStreamRead, TunnelStreamWrite, allow_all_listen_vports,
};
use crate::p2p_identity::{P2pId, P2pIdentityCertFactoryRef, P2pIdentityRef};
use crate::runtime::{AsyncReadExt, AsyncWriteExt};
use crate::tls::{DefaultTlsServerCertResolver, TlsServerCertResolver};
use crate::x509::{X509IdentityCertFactory, X509IdentityFactory, generate_rsa_x509_identity};
use sfo_reuseport::{ServerRuntime, ServerRuntimeConfig};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex as StdMutex, Once};
use std::time::Duration;
use tokio::sync::{Mutex as AsyncMutex, mpsc};
use tokio::time::timeout;

static TLS_INIT: Once = Once::new();
type TestStreamRx = mpsc::Receiver<P2pResult<IncomingStream>>;
static TEST_STREAM_RX: LazyLock<StdMutex<HashMap<String, TestStreamRx>>> =
    LazyLock::new(|| StdMutex::new(HashMap::new()));
static REAL_NETWORK_TEST_LOCK: LazyLock<Arc<AsyncMutex<()>>> =
    LazyLock::new(|| Arc::new(AsyncMutex::new(())));
const TEST_CHANNEL_CAPACITY: usize = 8;

struct TestNetworkPair {
    _serial_guard: tokio::sync::OwnedMutexGuard<()>,
    client_network: TcpTunnelNetwork,
    client_identity: P2pIdentityRef,
    client_local_ep: Endpoint,
    server_network: TcpTunnelNetwork,
    server_identity: P2pIdentityRef,
    server_local_ep: Endpoint,
    server_incoming: AsyncMutex<mpsc::Receiver<P2pResult<TunnelRef>>>,
}

fn init_tls_once() {
    TLS_INIT.call_once(|| {
        crate::tls::init_tls(Arc::new(X509IdentityFactory));
    });
}

fn incoming_channel() -> (IncomingTunnelCallback, mpsc::Receiver<P2pResult<TunnelRef>>) {
    let (tx, rx) = mpsc::channel(TEST_CHANNEL_CAPACITY);
    let callback = Arc::new(move |result| {
        let tx = tx.clone();
        Box::pin(async move {
            let _ = tx.send(result).await;
        }) as std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>
    });
    (callback, rx)
}

fn ignore_incoming() -> IncomingTunnelCallback {
    incoming_channel().0
}

fn test_tunnel_key(tunnel: &dyn Tunnel) -> String {
    format!(
        "{}:{}:{:?}:{:?}",
        tunnel.local_id(),
        tunnel.remote_id(),
        tunnel.tunnel_id(),
        tunnel.candidate_id()
    )
}

async fn listen_stream_collect(tunnel: &dyn Tunnel, vports: crate::networks::ListenVPortsRef) {
    let (tx, rx) = mpsc::channel(TEST_CHANNEL_CAPACITY);
    let callback: crate::networks::IncomingStreamCallback = Arc::new(move |accepted| {
        let tx = tx.clone();
        Box::pin(async move {
            let _ = tx.send(accepted).await;
        })
    });
    TEST_STREAM_RX
        .lock()
        .unwrap()
        .insert(test_tunnel_key(tunnel), rx);
    tunnel.listen_stream(vports, callback).await.unwrap();
}

async fn recv_stream(tunnel: &dyn Tunnel) -> P2pResult<IncomingStream> {
    let key = test_tunnel_key(tunnel);
    let mut rx = TEST_STREAM_RX
        .lock()
        .unwrap()
        .remove(&key)
        .expect("test stream receiver should be registered");
    let accepted = rx.recv().await.expect("test stream sender should stay open")?;
    TEST_STREAM_RX.lock().unwrap().insert(key, rx);
    Ok(accepted)
}

async fn assert_no_incoming_stream(tunnel: &dyn Tunnel) {
    let key = test_tunnel_key(tunnel);
    let mut rx = TEST_STREAM_RX
        .lock()
        .unwrap()
        .remove(&key)
        .expect("test stream receiver should be registered");
    match timeout(Duration::from_millis(200), rx.recv()).await {
        Err(_) | Ok(None) => {}
        Ok(Some(result)) => panic!("unexpected business stream result: {:?}", result.err()),
    }
}

async fn accept_incoming(rx: &AsyncMutex<mpsc::Receiver<P2pResult<TunnelRef>>>) -> TunnelRef {
    rx.lock().await.recv().await.unwrap().unwrap()
}

fn new_identity(name: &str) -> P2pIdentityRef {
    Arc::new(generate_rsa_x509_identity(Some(name.to_owned())).unwrap())
}

fn loopback_tcp_ep() -> Endpoint {
    Endpoint::from((Protocol::Tcp, "127.0.0.1:0".parse().unwrap()))
}

fn purpose_of(vport: u16) -> TunnelPurpose {
    TunnelPurpose::from_value(&vport).unwrap()
}

async fn register_listener_identity(
    resolver: &Arc<DefaultTlsServerCertResolver>,
    identity: P2pIdentityRef,
) {
    resolver.add_server_identity(identity).await.unwrap();
}

fn new_network() -> (TcpTunnelNetwork, Arc<DefaultTlsServerCertResolver>) {
    let resolver = DefaultTlsServerCertResolver::new();
    let cert_factory: P2pIdentityCertFactoryRef = Arc::new(X509IdentityCertFactory);
    (
        TcpTunnelNetwork::new(
            resolver.clone(),
            cert_factory,
            Duration::from_secs(3),
            Duration::from_secs(5),
            Duration::from_secs(30),
            ServerRuntime::start(ServerRuntimeConfig::default())
                .expect("sfo reuseport server runtime should start"),
        ),
        resolver,
    )
}

async fn decode_control(bytes: &[u8]) -> P2pResult<TcpControlCmd> {
    let capacity = bytes.len().max(64);
    let (mut write, mut read) = tokio::io::duplex(capacity);
    write.write_all(bytes).await.unwrap();
    write.shutdown().await.unwrap();
    TcpControlCmd::read_from_wire(&mut read).await
}

fn reverse_response(result: OpenDataConnRespResult) -> OpenDataConnResp {
    OpenDataConnResp {
        request_id: crate::types::TunnelId::from(0x1020_3040),
        conn_id: (result == OpenDataConnRespResult::Success)
            .then(|| crate::types::TunnelId::from(0x5060_7080)),
        result,
    }
}

#[test]
fn tcp_reverse_data_first_claim_correlation_completes_once_in_both_orders() {
    let conn_id = crate::types::TunnelId::from(41);

    let mut arrival_first = ReverseOpenCorrelation::new();
    assert_eq!(
        arrival_first.apply(ReverseOpenCorrelationEvent::Arrival(conn_id)),
        ReverseOpenCorrelationOutcome::Wait
    );
    assert_eq!(
        arrival_first.apply(ReverseOpenCorrelationEvent::ArrivalRegistered(conn_id)),
        ReverseOpenCorrelationOutcome::Wait
    );
    assert_eq!(
        arrival_first.apply(ReverseOpenCorrelationEvent::SuccessResponse(conn_id)),
        ReverseOpenCorrelationOutcome::Complete(conn_id)
    );
    assert_eq!(
        arrival_first.apply(ReverseOpenCorrelationEvent::SuccessResponse(conn_id)),
        ReverseOpenCorrelationOutcome::Idempotent
    );
    assert_eq!(arrival_first.completed_conn_id(), Some(conn_id));

    let mut response_first = ReverseOpenCorrelation::new();
    assert_eq!(
        response_first.apply(ReverseOpenCorrelationEvent::SuccessResponse(conn_id)),
        ReverseOpenCorrelationOutcome::Wait
    );
    assert_eq!(
        response_first.apply(ReverseOpenCorrelationEvent::SuccessResponse(conn_id)),
        ReverseOpenCorrelationOutcome::Idempotent
    );
    assert_eq!(
        response_first.apply(ReverseOpenCorrelationEvent::Arrival(conn_id)),
        ReverseOpenCorrelationOutcome::Wait
    );
    assert_eq!(
        response_first.apply(ReverseOpenCorrelationEvent::ArrivalRegistered(conn_id)),
        ReverseOpenCorrelationOutcome::Complete(conn_id)
    );
    assert_eq!(
        response_first.apply(ReverseOpenCorrelationEvent::SuccessResponse(conn_id)),
        ReverseOpenCorrelationOutcome::Idempotent
    );
    assert_eq!(response_first.completed_conn_id(), Some(conn_id));
}

#[test]
fn tcp_reverse_data_first_claim_correlation_rejects_duplicates_mismatches_and_late_input() {
    let first = crate::types::TunnelId::from(51);
    let other = crate::types::TunnelId::from(52);

    let mut duplicate = ReverseOpenCorrelation::new();
    assert_eq!(
        duplicate.apply(ReverseOpenCorrelationEvent::Arrival(first)),
        ReverseOpenCorrelationOutcome::Wait
    );
    assert_eq!(
        duplicate.apply(ReverseOpenCorrelationEvent::Arrival(first)),
        ReverseOpenCorrelationOutcome::Reject(ReverseOpenCorrelationError::DuplicateArrival)
    );
    assert!(duplicate.is_terminal());
    assert_eq!(
        duplicate.apply(ReverseOpenCorrelationEvent::ArrivalRegistered(first)),
        ReverseOpenCorrelationOutcome::Reject(ReverseOpenCorrelationError::Terminal)
    );

    let mut mismatch = ReverseOpenCorrelation::new();
    assert_eq!(
        mismatch.apply(ReverseOpenCorrelationEvent::SuccessResponse(first)),
        ReverseOpenCorrelationOutcome::Wait
    );
    assert_eq!(
        mismatch.apply(ReverseOpenCorrelationEvent::Arrival(other)),
        ReverseOpenCorrelationOutcome::Reject(ReverseOpenCorrelationError::ConnIdMismatch)
    );
    assert!(mismatch.is_terminal());

    for terminal_event in [
        ReverseOpenCorrelationEvent::Failure,
        ReverseOpenCorrelationEvent::TerminalCleanup,
    ] {
        let mut terminal = ReverseOpenCorrelation::new();
        assert_eq!(
            terminal.apply(terminal_event),
            ReverseOpenCorrelationOutcome::Terminal
        );
        assert!(terminal.is_terminal());
        assert_eq!(
            terminal.apply(ReverseOpenCorrelationEvent::Arrival(first)),
            ReverseOpenCorrelationOutcome::Reject(ReverseOpenCorrelationError::Terminal)
        );
        assert_eq!(
            terminal.apply(ReverseOpenCorrelationEvent::SuccessResponse(first)),
            ReverseOpenCorrelationOutcome::Reject(ReverseOpenCorrelationError::Terminal)
        );
    }
}

#[test]
fn tcp_reverse_data_first_claim_validates_owner_lease_and_simultaneous_tie_break() {
    assert_eq!(
        validate_remote_first_claim(false, TcpLeaseSeq::from(1)),
        Ok(())
    );
    assert_eq!(
        validate_remote_first_claim(true, TcpLeaseSeq::from(1)),
        Err(ClaimConnAckResult::ProtocolError)
    );
    assert_eq!(
        validate_remote_first_claim(false, TcpLeaseSeq::from(2)),
        Err(ClaimConnAckResult::LeaseMismatch)
    );

    let low_id = P2pId::from(vec![1; 32]);
    let high_id = P2pId::from(vec![2; 32]);
    assert!(local_simultaneous_claim_wins(&low_id, &high_id, 20, 10));
    assert!(!local_simultaneous_claim_wins(&high_id, &low_id, 10, 20));
    assert!(!local_simultaneous_claim_wins(&low_id, &high_id, 10, 10));
    assert!(local_simultaneous_claim_wins(&high_id, &low_id, 10, 10));
}

#[tokio::test]
async fn tcp_reverse_data_first_claim_failure_wire_maps_to_exact_error_categories() {
    for (result, expected) in [
        (OpenDataConnRespResult::ConnectFailed, P2pErrorCode::ConnectFailed),
        (OpenDataConnRespResult::ProtocolError, P2pErrorCode::InvalidData),
        (OpenDataConnRespResult::InternalError, P2pErrorCode::InternalError),
    ] {
        let bytes = TcpControlCmd::OpenDataConnResp(reverse_response(result))
            .encode_wire()
            .unwrap();
        let decoded = match decode_control(&bytes).await.unwrap() {
            TcpControlCmd::OpenDataConnResp(decoded) => decoded,
            other => panic!("expected open data response, got {other:?}"),
        };
        assert_eq!(decoded.result, result);
        assert_eq!(TcpTunnel::open_data_resp_error(decoded.result).code(), expected);
        assert_eq!(
            TcpTunnel::open_data_resp_result_from_error(&P2pError::new(
                expected,
                "test".to_owned(),
            )),
            result
        );
    }
}

#[tokio::test]
async fn tcp_reverse_data_first_claim_response_wire_round_trips_success_and_failures() {
    let success = reverse_response(OpenDataConnRespResult::Success);
    let bytes = TcpControlCmd::OpenDataConnResp(success.clone())
        .encode_wire()
        .unwrap();
    match decode_control(&bytes).await.unwrap() {
        TcpControlCmd::OpenDataConnResp(decoded) => {
            assert_eq!(decoded.request_id, success.request_id);
            assert_eq!(decoded.conn_id, success.conn_id);
            assert_eq!(decoded.result, OpenDataConnRespResult::Success);
        }
        other => panic!("expected open data response, got {other:?}"),
    }

    for result in [
        OpenDataConnRespResult::ConnectFailed,
        OpenDataConnRespResult::ProtocolError,
        OpenDataConnRespResult::InternalError,
    ] {
        let response = reverse_response(result);
        let bytes = TcpControlCmd::OpenDataConnResp(response.clone())
            .encode_wire()
            .unwrap();
        match decode_control(&bytes).await.unwrap() {
            TcpControlCmd::OpenDataConnResp(decoded) => {
                assert_eq!(decoded.request_id, response.request_id);
                assert_eq!(decoded.conn_id, None);
                assert_eq!(decoded.result, result);
            }
            other => panic!("expected open data response, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn tcp_reverse_data_first_claim_response_wire_fails_closed_on_malformed_frames() {
    let mut truncated = TcpControlCmd::OpenDataConnResp(reverse_response(
        OpenDataConnRespResult::Success,
    ))
    .encode_wire()
    .unwrap();
    truncated.pop();
    assert!(decode_control(&truncated).await.is_err());

    let mut invalid_result = TcpControlCmd::OpenDataConnResp(reverse_response(
        OpenDataConnRespResult::ConnectFailed,
    ))
    .encode_wire()
    .unwrap();
    *invalid_result.last_mut().unwrap() = u8::MAX;
    assert!(decode_control(&invalid_result).await.is_err());

    let mut unknown_command = TcpControlCmd::Ping(PingCmd {
        seq: 1,
        send_time: Default::default(),
    })
    .encode_wire()
    .unwrap();
    unknown_command[1] = u8::MAX;
    let err = decode_control(&unknown_command).await.err().unwrap();
    assert_eq!(err.code(), P2pErrorCode::InvalidData);
}

async fn open_reverse_stream(
    opened: &dyn Tunnel,
    accepted: &dyn Tunnel,
    purpose: TunnelPurpose,
) -> ((TunnelStreamRead, TunnelStreamWrite), IncomingStream) {
    timeout(Duration::from_secs(10), async {
        let (opened_stream, accepted_stream) =
            tokio::join!(opened.open_stream(purpose.clone()), recv_stream(accepted));
        (opened_stream.unwrap(), accepted_stream.unwrap())
    })
    .await
    .expect("reverse stream open should complete within the test bound")
}

async fn setup_reverse_network_pair() -> (TestNetworkPair, TunnelRef, TunnelRef) {
    init_tls_once();
    let serial_guard = REAL_NETWORK_TEST_LOCK.clone().lock_owned().await;

    let (client_network, client_resolver) = new_network();
    let (server_network, server_resolver) = new_network();
    client_network.set_reuse_address(true);
    server_network.set_reuse_address(true);

    let client_identity = new_identity("tcp-reverse-client");
    let server_identity = new_identity("tcp-reverse-server");
    register_listener_identity(&client_resolver, client_identity.clone()).await;
    register_listener_identity(&server_resolver, server_identity.clone()).await;

    let client_port_reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let client_local_ep = Endpoint::from((
        Protocol::Tcp,
        client_port_reservation.local_addr().unwrap(),
    ));
    drop(client_port_reservation);
    let (server_callback, server_incoming) = incoming_channel();
    server_network
        .listen(&loopback_tcp_ep(), None, None, server_callback)
        .await
        .unwrap();

    let server_local_ep = server_network.listener_infos()[0].local;
    let pair = TestNetworkPair {
        _serial_guard: serial_guard,
        client_network,
        client_identity,
        client_local_ep,
        server_network,
        server_identity,
        server_local_ep,
        server_incoming: AsyncMutex::new(server_incoming),
    };
    let opened = pair
        .client_network
        .create_tunnel_with_local_ep(
            &pair.client_identity,
            &pair.client_local_ep,
            &pair.server_local_ep,
            &pair.server_identity.get_id(),
            Some(pair.server_identity.get_name()),
        )
        .await
        .unwrap();
    let accepted = accept_incoming(&pair.server_incoming).await;
    pair.client_network
        .listen(&pair.client_local_ep, None, None, ignore_incoming())
        .await
        .unwrap();
    (pair, opened, accepted)
}

fn concrete_tunnel(network: &TcpTunnelNetwork, tunnel: &dyn Tunnel) -> Arc<TcpTunnel> {
    network
        .registry
        .find_tunnel(
            &tunnel.local_id(),
            &tunnel.remote_id(),
            tunnel.tunnel_id(),
            tunnel.candidate_id(),
        )
        .expect("test tunnel should remain registered")
}

async fn wait_reverse_open_snapshot(
    tunnel: &TcpTunnel,
    expected_pending: usize,
    expected_staged: usize,
    expected_data_connections: usize,
) {
    timeout(Duration::from_secs(2), async {
        loop {
            let snapshot = tunnel.test_reverse_open_snapshot();
            if snapshot.pending_requests == expected_pending
                && snapshot.staged_entries == expected_staged
                && snapshot.data_connections == expected_data_connections
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "reverse-open state did not converge: {:?}",
            tunnel.test_reverse_open_snapshot()
        )
    });
}

#[tokio::test]
async fn tcp_reverse_data_first_claim_caller_abort_drops_guard_and_retires_staged_entry() {
    let (pair, opened, accepted) = setup_reverse_network_pair().await;
    listen_stream_collect(&*accepted, allow_all_listen_vports()).await;
    let requester = concrete_tunnel(&pair.client_network, &*opened);
    let creator = concrete_tunnel(&pair.server_network, &*accepted);
    creator.test_pause_reverse_open_response();
    pair.server_network.close_all_listener().await.unwrap();

    let open_tunnel = opened.clone();
    let open = tokio::spawn(async move { open_tunnel.open_stream(purpose_of(3510)).await });
    timeout(
        Duration::from_secs(10),
        creator.test_wait_reverse_open_response_ready(),
    )
    .await
    .expect("reverse connection should register before the response gate");
    wait_reverse_open_snapshot(&requester, 1, 1, 1).await;

    open.abort();
    let _ = open.await;
    wait_reverse_open_snapshot(&requester, 0, 0, 0).await;

    creator.test_release_reverse_open_response();
    tokio::time::sleep(Duration::from_millis(50)).await;
    wait_reverse_open_snapshot(&requester, 0, 0, 0).await;
    opened.close().unwrap();
    accepted.close().unwrap();
}

#[tokio::test]
async fn tcp_reverse_data_first_claim_tunnel_close_removes_waiter_and_staged_entry() {
    let (pair, opened, accepted) = setup_reverse_network_pair().await;
    listen_stream_collect(&*accepted, allow_all_listen_vports()).await;
    let requester = concrete_tunnel(&pair.client_network, &*opened);
    let creator = concrete_tunnel(&pair.server_network, &*accepted);
    creator.test_pause_reverse_open_response();
    pair.server_network.close_all_listener().await.unwrap();

    let open_tunnel = opened.clone();
    let open = tokio::spawn(async move { open_tunnel.open_stream(purpose_of(3511)).await });
    timeout(
        Duration::from_secs(10),
        creator.test_wait_reverse_open_response_ready(),
    )
    .await
    .expect("reverse connection should register before tunnel close");
    wait_reverse_open_snapshot(&requester, 1, 1, 1).await;

    opened.close().unwrap();
    let close_result = timeout(Duration::from_secs(2), open)
        .await
        .expect("open future should finish after tunnel close")
        .expect("open task should not panic");
    assert!(close_result.is_err(), "closed tunnel must fail the pending open");
    wait_reverse_open_snapshot(&requester, 0, 0, 0).await;

    creator.test_release_reverse_open_response();
    accepted.close().unwrap();
}

#[tokio::test]
async fn tcp_reverse_data_first_claim_new_requester_old_creator_timeout_cleans_owned_state() {
    let (pair, opened, accepted) = setup_reverse_network_pair().await;
    listen_stream_collect(&*accepted, allow_all_listen_vports()).await;
    let requester = concrete_tunnel(&pair.client_network, &*opened);
    let creator = concrete_tunnel(&pair.server_network, &*accepted);
    creator.test_suppress_reverse_open_response();
    pair.server_network.close_all_listener().await.unwrap();

    let open_tunnel = opened.clone();
    let open = tokio::spawn(async move { open_tunnel.open_stream(purpose_of(3512)).await });
    timeout(
        Duration::from_secs(10),
        creator.test_wait_reverse_open_response_ready(),
    )
    .await
    .expect("legacy creator fixture should register data and omit the response");
    wait_reverse_open_snapshot(&requester, 1, 1, 1).await;

    let timeout_result = timeout(Duration::from_secs(6), open)
        .await
        .expect("new requester should reach its production timeout")
        .expect("open task should not panic");
    assert!(
        timeout_result.is_err(),
        "old creator response omission must fail closed"
    );
    let err = timeout_result.err().unwrap();
    assert!(!format!("{err:?}").contains("claim retries exhausted"));
    wait_reverse_open_snapshot(&requester, 0, 0, 0).await;
    opened.close().unwrap();
    accepted.close().unwrap();
}

#[tokio::test]
async fn tcp_reverse_data_first_claim_old_requester_new_creator_rejects_command_12_on_wire() {
    let (pair, opened, accepted) = setup_reverse_network_pair().await;
    listen_stream_collect(&*accepted, allow_all_listen_vports()).await;
    let requester = concrete_tunnel(&pair.client_network, &*opened);
    requester.test_use_legacy_control_decoder();
    pair.server_network.close_all_listener().await.unwrap();

    let legacy_result = timeout(
        Duration::from_secs(10),
        opened.open_stream(purpose_of(3513)),
    )
    .await
    .expect("legacy requester fixture must reject command 12 within the bound");
    assert!(
        legacy_result.is_err(),
        "old requester must close instead of consuming new response semantics"
    );
    let err = legacy_result.err().unwrap();
    assert!(matches!(
        err.code(),
        P2pErrorCode::Interrupted | P2pErrorCode::ErrorState
    ));
    assert!(opened.is_closed());
    wait_reverse_open_snapshot(&requester, 0, 0, 0).await;
    assert_no_incoming_stream(&*accepted).await;
    accepted.close().unwrap();
}

async fn assert_bidirectional_stream(
    opened_stream: (TunnelStreamRead, TunnelStreamWrite),
    accepted_stream: IncomingStream,
    expected_purpose: TunnelPurpose,
    outbound: &[u8],
    inbound: &[u8],
) {
    let (mut opened_read, mut opened_write) = opened_stream;
    let (purpose, mut accepted_read, mut accepted_write) = accepted_stream;
    assert_eq!(purpose, expected_purpose);

    opened_write.write_all(outbound).await.unwrap();
    let mut received = vec![0; outbound.len()];
    accepted_read.read_exact(&mut received).await.unwrap();
    assert_eq!(received, outbound);

    accepted_write.write_all(inbound).await.unwrap();
    let mut reply = vec![0; inbound.len()];
    opened_read.read_exact(&mut reply).await.unwrap();
    assert_eq!(reply, inbound);

    opened_write.shutdown().await.unwrap();
    accepted_write.shutdown().await.unwrap();
    assert!(timeout(Duration::from_secs(5), async {
        let mut opened_tail = Vec::new();
        let mut accepted_tail = Vec::new();
        tokio::join!(
            opened_read.read_to_end(&mut opened_tail),
            accepted_read.read_to_end(&mut accepted_tail)
        )
    })
    .await
    .expect("stream drain should complete within the test bound")
    .0
    .is_ok());
}

#[tokio::test]
async fn tcp_reverse_data_first_claim_opens_and_reopens_after_direct_listener_closes() {
    let (pair, opened, accepted) = setup_reverse_network_pair().await;
    listen_stream_collect(&*accepted, allow_all_listen_vports()).await;

    pair.server_network.close_all_listener().await.unwrap();

    let first_purpose = purpose_of(3501);
    let (first_opened, first_accepted) =
        open_reverse_stream(&*opened, &*accepted, first_purpose.clone()).await;
    assert_bidirectional_stream(
        first_opened,
        first_accepted,
        first_purpose,
        b"reverse-one",
        b"return-one",
    )
    .await;

    let second_purpose = purpose_of(3502);
    let (second_opened, second_accepted) =
        open_reverse_stream(&*opened, &*accepted, second_purpose.clone()).await;
    assert_bidirectional_stream(
        second_opened,
        second_accepted,
        second_purpose,
        b"reverse-two",
        b"return-two",
    )
    .await;

    opened.close().unwrap();
    accepted.close().unwrap();
}

#[tokio::test]
async fn tcp_reverse_data_first_claim_preserves_unlistened_purpose_error() {
    let (pair, opened, accepted) = setup_reverse_network_pair().await;
    let empty_vports = ListenVPortRegistry::<()>::new();
    listen_stream_collect(&*accepted, empty_vports.as_listen_vports_ref()).await;

    pair.server_network.close_all_listener().await.unwrap();

    let err = timeout(
        Duration::from_secs(10),
        opened.open_stream(purpose_of(3503)),
    )
    .await
    .expect("reverse unlistened-purpose failure should be bounded")
    .err()
    .expect("unlistened purpose should fail");
    assert_eq!(err.code(), P2pErrorCode::PortNotListen);
    assert!(!format!("{err:?}").contains("claim retries exhausted"));

    opened.close().unwrap();
    accepted.close().unwrap();
}

#[tokio::test]
async fn tcp_reverse_data_first_claim_returns_connect_failure_when_both_listeners_close() {
    let (pair, opened, accepted) = setup_reverse_network_pair().await;
    listen_stream_collect(&*accepted, allow_all_listen_vports()).await;

    pair.server_network.close_all_listener().await.unwrap();
    pair.client_network.close_all_listener().await.unwrap();

    let err = timeout(
        Duration::from_secs(10),
        opened.open_stream(purpose_of(3504)),
    )
    .await
    .expect("reverse connect failure should be bounded")
    .err()
    .expect("opening without either listener should fail");
    assert_eq!(err.code(), P2pErrorCode::ConnectFailed);
    assert!(!format!("{err:?}").contains("claim retries exhausted"));

    opened.close().unwrap();
    accepted.close().unwrap();
}
