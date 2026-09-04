use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpListener, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use bucky_raw_codec::RawConvertTo;
use p2p_frame::endpoint::{Endpoint, Protocol};
use p2p_frame::error::{P2pError, P2pErrorCode, P2pResult};
use p2p_frame::p2p_identity::{P2pId, P2pIdentityCertFactory, P2pIdentityRef, P2pSn};
use p2p_frame::sn::client::SnTunnelRendezvousActionAck;
use p2p_frame::sn::protocol::v0::{SnCalled, SnCalledResp, TunnelType};
use p2p_frame::sn::protocol::{
    PackageCmdCode, SN_PROTOCOL_VERSION, SN_TUNNEL_RENDEZVOUS_CMD_VERSION,
    SnTunnelRendezvousNotify, SnTunnelRendezvousOperation,
};
use p2p_frame::sn::service::{SnServerRef, SnServiceConfig, create_sn_service};
use p2p_frame::sn::types::SnTunnelClassification;
use p2p_frame::stack::{P2pConfig, P2pStackConfig, P2pStackRef, create_p2p_env, create_p2p_stack};
use p2p_frame::types::TunnelId;
use p2p_frame::x509::{X509IdentityCertFactory, X509IdentityFactory, generate_rsa_x509_identity};
use sfo_cmd_server::client::{ClassifiedCmdClient, CmdClient};
use sfo_reuseport::{ServerRuntime, ServerRuntimeConfig};
use tokio::sync::{Semaphore, mpsc};

const ONLINE_TIMEOUT: Duration = Duration::from_secs(30);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
const SETUP_MAX_RETRY: usize = 20;

fn protocol_label(protocol: Protocol) -> &'static str {
    match protocol {
        Protocol::Tcp => "tcp",
        Protocol::Quic => "quic",
        Protocol::Ext(_) => unreachable!("matrix only selects TCP or QUIC"),
    }
}

struct RealSnTopology {
    server: SnServerRef,
    caller: P2pStackRef,
    caller_id: P2pId,
    caller_endpoint: Endpoint,
    target: P2pStackRef,
    target_id: P2pId,
    sn_id: P2pId,
    sn_endpoint: Endpoint,
    caller_cmd_tunnel: sfo_cmd_server::TunnelId,
    target_cmd_tunnel: sfo_cmd_server::TunnelId,
    cert_factory: Arc<X509IdentityCertFactory>,
}

fn reserve_port(protocol: Protocol) -> u16 {
    let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0);
    match protocol {
        Protocol::Tcp => TcpListener::bind(address)
            .unwrap()
            .local_addr()
            .unwrap()
            .port(),
        Protocol::Quic => UdpSocket::bind(address)
            .unwrap()
            .local_addr()
            .unwrap()
            .port(),
        Protocol::Ext(_) => panic!("unsupported SN test transport: {protocol:?}"),
    }
}

fn loopback_endpoint(protocol: Protocol) -> Endpoint {
    Endpoint::from((
        protocol,
        SocketAddr::V4(SocketAddrV4::new(
            Ipv4Addr::LOCALHOST,
            reserve_port(protocol),
        )),
    ))
}

fn build_identity(name: &str, endpoint: Endpoint) -> P2pIdentityRef {
    let identity = generate_rsa_x509_identity(Some(name.to_owned())).unwrap();
    let identity: P2pIdentityRef = Arc::new(identity);
    identity.update_endpoints(vec![endpoint])
}

fn build_sn_entry(identity: &P2pIdentityRef) -> P2pSn {
    let cert = identity.get_identity_cert().unwrap();
    P2pSn::new(cert.get_id(), cert.get_name(), cert.endpoints())
}

fn test_runtime() -> ServerRuntime {
    ServerRuntime::start(ServerRuntimeConfig::new().with_workers(1)).unwrap()
}

fn is_retryable_bind_error(code: P2pErrorCode) -> bool {
    matches!(
        code,
        P2pErrorCode::AddrInUse | P2pErrorCode::AddrNotAvailable | P2pErrorCode::AlreadyExists
    )
}

async fn start_server(
    identity: P2pIdentityRef,
    identity_factory: Arc<X509IdentityFactory>,
    cert_factory: Arc<X509IdentityCertFactory>,
) -> P2pResult<SnServerRef> {
    let server = create_sn_service(SnServiceConfig::new(
        identity,
        identity_factory,
        cert_factory,
        test_runtime(),
    ))
    .await?;
    server.start().await?;
    Ok(server)
}

async fn start_client(
    identity: P2pIdentityRef,
    sn: P2pSn,
    identity_factory: Arc<X509IdentityFactory>,
    cert_factory: Arc<X509IdentityCertFactory>,
) -> P2pResult<P2pStackRef> {
    let endpoint = identity.endpoints()[0];
    let listen_endpoint = if endpoint.protocol() == Protocol::Tcp {
        Endpoint::from((
            Protocol::Tcp,
            SocketAddr::V4(SocketAddrV4::new(
                Ipv4Addr::UNSPECIFIED,
                endpoint.addr().port(),
            )),
        ))
    } else {
        endpoint
    };
    let env = create_p2p_env(
        P2pConfig::new(
            identity_factory,
            cert_factory,
            vec![listen_endpoint],
            test_runtime(),
        )
        .set_tcp_accept_timout(Duration::from_secs(3))
        .set_tcp_connect_timout(Duration::from_secs(3))
        .set_quic_connect_timeout(Duration::from_secs(3))
        .set_quic_idle_time(Duration::from_secs(10)),
    )
    .await?;

    create_p2p_stack(
        P2pStackConfig::new(env, identity)
            .add_sn_list(vec![sn])
            .set_conn_timeout(Duration::from_secs(3))
            .set_sn_ping_interval(Duration::from_millis(100))
            .set_sn_call_timeout(Duration::from_secs(3))
            .set_sn_query_interval(Duration::from_millis(200))
            .set_sn_tunnel_count(2),
    )
    .await
}

async fn setup_real_sn_topology(protocol: Protocol) -> RealSnTopology {
    for attempt in 0..SETUP_MAX_RETRY {
        let protocol_label = protocol_label(protocol);
        let identity_factory = Arc::new(X509IdentityFactory);
        let cert_factory = Arc::new(X509IdentityCertFactory);
        let sn_endpoint = loopback_endpoint(protocol);
        let sn_identity = build_identity(
            format!("real-sn-{protocol_label}-{attempt}").as_str(),
            sn_endpoint,
        );
        let sn_id = sn_identity.get_id();
        let server = match start_server(
            sn_identity.clone(),
            identity_factory.clone(),
            cert_factory.clone(),
        )
        .await
        {
            Ok(server) => server,
            Err(error) if is_retryable_bind_error(error.code()) => continue,
            Err(error) => panic!("start {protocol:?} SN failed: {error:?}"),
        };
        let sn_entry = build_sn_entry(&sn_identity);

        let caller_endpoint = loopback_endpoint(protocol);
        let caller_identity = build_identity(
            format!("real-sn-caller-{protocol_label}-{attempt}").as_str(),
            caller_endpoint,
        );
        let caller_id = caller_identity.get_id();
        let caller = match start_client(
            caller_identity,
            sn_entry.clone(),
            identity_factory.clone(),
            cert_factory.clone(),
        )
        .await
        {
            Ok(stack) => stack,
            Err(error) if is_retryable_bind_error(error.code()) => {
                server.stop();
                continue;
            }
            Err(error) => panic!("start {protocol:?} caller failed: {error:?}"),
        };

        let target_endpoint = loopback_endpoint(protocol);
        let target_identity = build_identity(
            format!("real-sn-target-{protocol_label}-{attempt}").as_str(),
            target_endpoint,
        );
        let target_id = target_identity.get_id();
        let target = match start_client(
            target_identity,
            sn_entry,
            identity_factory,
            cert_factory.clone(),
        )
        .await
        {
            Ok(stack) => stack,
            Err(error) if is_retryable_bind_error(error.code()) => {
                caller.sn_client().stop();
                server.stop();
                continue;
            }
            Err(error) => panic!("start {protocol:?} target failed: {error:?}"),
        };

        if let Err(error) = caller.wait_online(Some(ONLINE_TIMEOUT)).await {
            caller.sn_client().stop();
            target.sn_client().stop();
            server.stop();
            panic!("wait {protocol:?} caller online: {error:?}");
        }
        if let Err(error) = target.wait_online(Some(ONLINE_TIMEOUT)).await {
            caller.sn_client().stop();
            target.sn_client().stop();
            server.stop();
            panic!("wait {protocol:?} target online: {error:?}");
        }

        // Resolve the exact classified tunnels selected by the ReportSn loop.
        let command_local_endpoint = |endpoint: Endpoint| {
            if protocol == Protocol::Tcp {
                None
            } else {
                Some(endpoint)
            }
        };
        let caller_classification =
            SnTunnelClassification::new(command_local_endpoint(caller_endpoint), sn_endpoint);
        let target_classification =
            SnTunnelClassification::new(command_local_endpoint(target_endpoint), sn_endpoint);
        let caller_cmd_tunnel = caller
            .sn_client()
            .get_cmd_client()
            .find_tunnel_id_by_classified(caller_classification)
            .await
            .unwrap_or_else(|error| panic!("open {protocol:?} caller command tunnel: {error:?}"));
        let target_cmd_tunnel = target
            .sn_client()
            .get_cmd_client()
            .find_tunnel_id_by_classified(target_classification)
            .await
            .unwrap_or_else(|error| panic!("open {protocol:?} target command tunnel: {error:?}"));

        assert_ne!(caller_cmd_tunnel.value(), 0);
        assert_ne!(target_cmd_tunnel.value(), 0);

        return RealSnTopology {
            server,
            caller,
            caller_id,
            caller_endpoint,
            target,
            target_id,
            sn_id,
            sn_endpoint,
            caller_cmd_tunnel,
            target_cmd_tunnel,
            cert_factory,
        };
    }

    panic!("setup {protocol:?} SN topology failed after retries")
}

fn assert_registered_on_selected_transport(topology: &RealSnTopology, protocol: Protocol) {
    assert_eq!(topology.sn_endpoint.protocol(), protocol);
    assert!(topology.sn_endpoint.addr().ip().is_loopback());
    assert_ne!(topology.sn_endpoint.addr().port(), 0);

    for (stack, expected_tunnel) in [
        (&topology.caller, topology.caller_cmd_tunnel),
        (&topology.target, topology.target_cmd_tunnel),
    ] {
        let active = stack.sn_client().get_active_sn_list();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].sn_peer_id, topology.sn_id);
        assert_eq!(active[0].protocol, protocol);
        assert_eq!(active[0].conn_id, expected_tunnel);
        let listeners = stack.get_listen_eps(protocol).unwrap();
        assert!(listeners.iter().any(|(endpoint, _)| {
            endpoint.protocol() == protocol
                && (endpoint.addr().ip().is_loopback() || endpoint.addr().ip().is_unspecified())
                && endpoint.addr().port() != 0
        }));
    }
}

async fn exercise_query(topology: &RealSnTopology) {
    let target = topology
        .caller
        .sn_client()
        .query(&topology.target_id)
        .await
        .unwrap();
    let target_cert = topology
        .cert_factory
        .create(target.peer_info.as_ref().unwrap())
        .unwrap();
    assert_eq!(target_cert.get_id(), topology.target_id);
    match topology.sn_endpoint.protocol() {
        Protocol::Tcp => assert!(target.end_point_array.iter().any(|endpoint| {
            endpoint.protocol() == Protocol::Tcp && endpoint.addr().port() != 0
        })),
        Protocol::Quic => {
            #[cfg(not(feature = "test-real-socket-matrix"))]
            assert!(target.end_point_array.is_empty());
            #[cfg(feature = "test-real-socket-matrix")]
            assert!(target.end_point_array.iter().any(|endpoint| {
                endpoint.protocol() == Protocol::Quic
                    && endpoint.addr().ip().is_loopback()
                    && endpoint.addr().port() != 0
            }));
        }
        Protocol::Ext(_) => unreachable!("matrix only selects TCP or QUIC"),
    }
    assert_eq!(target.target_protocol_version, Some(SN_PROTOCOL_VERSION));

    let missing_id = P2pId::from(vec![0xE1; 32]);
    let missing = topology
        .caller
        .sn_client()
        .query(&missing_id)
        .await
        .unwrap();
    assert!(missing.peer_info.is_none());
    assert!(missing.end_point_array.is_empty());
    assert_eq!(missing.target_protocol_version, None);
}

async fn exercise_call_and_called(topology: &RealSnTopology, protocol: Protocol) {
    let (called_tx, mut called_rx) = mpsc::channel::<SnCalled>(1);
    topology.target.sn_client().set_listener(move |called| {
        let called_tx = called_tx.clone();
        async move {
            called_tx.send(called).await.unwrap();
            Ok(())
        }
    });

    let tunnel_id = TunnelId::from(match protocol {
        Protocol::Tcp => 0x2701,
        Protocol::Quic => 0x2702,
        Protocol::Ext(_) => unreachable!("matrix only selects TCP or QUIC"),
    });
    let payload = format!("real-sn-{protocol:?}-call").into_bytes();
    let response = topology
        .caller
        .sn_client()
        .call(
            tunnel_id,
            Some(&[topology.caller_endpoint]),
            &topology.target_id,
            TunnelType::Stream,
            payload.clone(),
        )
        .await
        .unwrap();
    assert_eq!(response.result, P2pErrorCode::Ok.as_u8());
    let target_cert = topology
        .cert_factory
        .create(response.to_peer_info.as_ref().unwrap())
        .unwrap();
    assert_eq!(target_cert.get_id(), topology.target_id);

    let called = tokio::time::timeout(COMMAND_TIMEOUT, called_rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(called.to_peer_id, topology.target_id);
    assert_eq!(called.sn_peer_id, topology.sn_id);
    assert_eq!(called.tunnel_id, tunnel_id);
    assert_eq!(called.call_type, TunnelType::Stream);
    assert_eq!(called.payload, payload);
    let caller_cert = topology.cert_factory.create(&called.peer_info).unwrap();
    assert_eq!(caller_cert.get_id(), topology.caller_id);

    // Exercise the standalone client -> SN acknowledgement command over the
    // target's selected real tunnel as well as the automatic acknowledgement
    // emitted by `on_called` after the listener returns.
    let acknowledgement = SnCalledResp {
        seq: called.seq,
        sn_peer_id: topology.sn_id.clone(),
        result: P2pErrorCode::Ok.into_u8(),
    };
    topology
        .target
        .sn_client()
        .get_cmd_client()
        .send_by_specify_tunnel(
            topology.target_cmd_tunnel,
            PackageCmdCode::SnCalledResp as u8,
            0,
            acknowledgement.to_vec().unwrap().as_slice(),
        )
        .await
        .unwrap();

    let missing_id = P2pId::from(vec![0xE2; 32]);
    let missing = topology
        .caller
        .sn_client()
        .call(
            TunnelId::from(tunnel_id.value() + 100),
            None,
            &missing_id,
            TunnelType::Stream,
            b"missing-target".to_vec(),
        )
        .await
        .unwrap();
    assert_eq!(missing.result, P2pErrorCode::NotFound.as_u8());
    assert!(missing.to_peer_info.is_none());
}

async fn exercise_rendezvous(topology: &RealSnTopology, protocol: Protocol) {
    let callback_count = Arc::new(AtomicUsize::new(0));
    let observed_count = callback_count.clone();
    let expected_caller = topology.caller_id.clone();
    let expected_sn = topology.sn_id.clone();
    let cert_factory = topology.cert_factory.clone();
    let release = Arc::new(Semaphore::new(0));
    let listener_release = release.clone();
    let (notify_tx, mut notify_rx) = mpsc::channel::<SnTunnelRendezvousNotify>(1);
    topology.target.sn_client().set_rendezvous_listener(
        move |notify: SnTunnelRendezvousNotify, serving_sn_id: P2pId| {
            let observed_count = observed_count.clone();
            let expected_caller = expected_caller.clone();
            let expected_sn = expected_sn.clone();
            let cert_factory = cert_factory.clone();
            let listener_release = listener_release.clone();
            let notify_tx = notify_tx.clone();
            async move {
                assert_eq!(serving_sn_id, expected_sn);
                assert_eq!(notify.operation, SnTunnelRendezvousOperation::WaitIncoming);
                assert!(notify.end_point_array.is_empty());
                assert!(!notify.need_predict_endpoint);
                assert_eq!(
                    cert_factory.create(&notify.peer_info)?.get_id(),
                    expected_caller
                );
                observed_count.fetch_add(1, Ordering::SeqCst);
                notify_tx.send(notify).await.unwrap();
                let _permit = listener_release.acquire().await.unwrap();
                Ok(SnTunnelRendezvousActionAck::without_prediction())
            }
        },
    );

    let request = topology
        .caller
        .sn_client()
        .new_rendezvous_request(
            TunnelId::from(match protocol {
                Protocol::Tcp => 0x2711,
                Protocol::Quic => 0x2712,
                Protocol::Ext(_) => unreachable!("matrix only selects TCP or QUIC"),
            }),
            &topology.target_id,
            SnTunnelRendezvousOperation::WaitIncoming,
            Vec::new(),
            false,
        )
        .unwrap();
    let caller = topology.caller.clone();
    let sn_id = topology.sn_id.clone();
    let request_for_task = request.clone();
    let response_task = tokio::spawn(async move {
        caller
            .sn_client()
            .rendezvous_via_sn(&sn_id, &request_for_task)
            .await
    });

    let notify = tokio::time::timeout(COMMAND_TIMEOUT, notify_rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(notify.seq, request.seq);
    assert_eq!(notify.tunnel_id, request.tunnel_id);
    assert!(!response_task.is_finished());
    release.add_permits(1);
    let response = response_task.await.unwrap().unwrap();
    assert!(response.is_success());
    assert_eq!(response.seq, request.seq);
    assert!(response.predicted_endpoint_array.is_empty());
    assert_eq!(callback_count.load(Ordering::SeqCst), 1);

    topology.target.sn_client().set_rendezvous_listener(
        |_notify: SnTunnelRendezvousNotify, _serving_sn_id: P2pId| async move {
            Err(P2pError::new(
                P2pErrorCode::InvalidData,
                "target rejects rendezvous".to_owned(),
            ))
        },
    );
    let rejected = topology
        .caller
        .sn_client()
        .new_rendezvous_request(
            TunnelId::from(request.tunnel_id.value() + 100),
            &topology.target_id,
            SnTunnelRendezvousOperation::WaitIncoming,
            Vec::new(),
            false,
        )
        .unwrap();
    let rejection = topology
        .caller
        .sn_client()
        .rendezvous_via_sn(&topology.sn_id, &rejected)
        .await
        .unwrap_err();
    assert_eq!(rejection.code(), P2pErrorCode::Failed);

    let wrong_version_callbacks = Arc::new(AtomicUsize::new(0));
    let observed_wrong_version = wrong_version_callbacks.clone();
    topology.target.sn_client().set_rendezvous_listener(
        move |_notify: SnTunnelRendezvousNotify, _serving_sn_id: P2pId| {
            let observed_wrong_version = observed_wrong_version.clone();
            async move {
                observed_wrong_version.fetch_add(1, Ordering::SeqCst);
                Ok(SnTunnelRendezvousActionAck::without_prediction())
            }
        },
    );
    let wrong_version = topology
        .caller
        .sn_client()
        .get_cmd_client()
        .send_by_specify_tunnel_with_resp(
            topology.caller_cmd_tunnel,
            PackageCmdCode::SnTunnelRendezvous as u8,
            SN_TUNNEL_RENDEZVOUS_CMD_VERSION - 1,
            rejected.to_vec().unwrap().as_slice(),
            COMMAND_TIMEOUT,
        )
        .await;
    assert!(wrong_version.is_err());
    assert_eq!(wrong_version_callbacks.load(Ordering::SeqCst), 0);
}

async fn exercise_malformed_report(topology: &RealSnTopology) {
    let malformed = topology
        .caller
        .sn_client()
        .get_cmd_client()
        .send_by_specify_tunnel_with_resp(
            topology.caller_cmd_tunnel,
            PackageCmdCode::ReportSn as u8,
            0,
            &[0xff],
            COMMAND_TIMEOUT,
        )
        .await;
    assert!(malformed.is_err());
}

async fn run_real_network_matrix(protocol: Protocol) {
    let topology = setup_real_sn_topology(protocol).await;
    assert_registered_on_selected_transport(&topology, protocol);
    exercise_query(&topology).await;
    exercise_call_and_called(&topology, protocol).await;
    exercise_rendezvous(&topology, protocol).await;
    exercise_malformed_report(&topology).await;

    topology.caller.sn_client().stop();
    topology.target.sn_client().stop();
    topology.server.stop();
}

#[test]
fn package_cmd_code_inventory_is_complete_and_roles_are_classified() {
    let actual = (u8::MIN..=u8::MAX)
        .filter_map(|value| PackageCmdCode::try_from(value).ok())
        .collect::<Vec<_>>();
    let classified = [
        (PackageCmdCode::SnCall, "client-request"),
        (PackageCmdCode::SnCallResp, "qa-response-payload-role"),
        (PackageCmdCode::SnCalled, "server-notify"),
        (PackageCmdCode::SnCalledResp, "client-acknowledgement"),
        (PackageCmdCode::ReportSn, "client-request"),
        (PackageCmdCode::ReportSnResp, "qa-response-payload-role"),
        (PackageCmdCode::SnQuery, "client-request"),
        (PackageCmdCode::SnQueryResp, "qa-response-payload-role"),
        (PackageCmdCode::SnTunnelRendezvous, "client-request"),
        (
            PackageCmdCode::SnTunnelRendezvousNotify,
            "server-notify-with-qa-response",
        ),
    ];
    assert_eq!(
        actual,
        classified
            .iter()
            .map(|(command, _)| *command)
            .collect::<Vec<_>>()
    );
    assert_eq!(actual.len(), 10);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sn_protocol_real_network_tcp_matrix() {
    run_real_network_matrix(Protocol::Tcp).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sn_protocol_real_network_quic_matrix() {
    run_real_network_matrix(Protocol::Quic).await;
}
