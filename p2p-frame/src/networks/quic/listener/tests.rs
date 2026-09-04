use super::*;
use crate::finder::{DeviceCache, DeviceCacheConfig};
use crate::networks::quic::QuicTunnelNetwork;
use crate::networks::TunnelNetwork;
use crate::tls::DefaultTlsServerCertResolver;
use crate::x509::{
    X509IdentityCertFactory, X509IdentityFactory, generate_rsa_x509_identity,
};
use sfo_reuseport::ServerRuntimeConfig;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::{Arc, Barrier, Once};

static TLS_INIT: Once = Once::new();

fn init_tls_once() {
    TLS_INIT.call_once(|| crate::tls::init_tls(Arc::new(X509IdentityFactory)));
}

fn new_listener() -> Arc<QuicTunnelListener> {
    init_tls_once();
    let cert_cache = Arc::new(DeviceCache::new(
        &DeviceCacheConfig {
            expire: Duration::from_secs(60),
            capacity: 8,
        },
        None,
    ));
    let callback: IncomingTunnelCallback = Arc::new(|_| Box::pin(async {}));
    QuicTunnelListener::new(
        cert_cache,
        DefaultTlsServerCertResolver::new(),
        Arc::new(X509IdentityCertFactory),
        QuicCongestionAlgorithm::Bbr,
        ServerRuntime::start(ServerRuntimeConfig::new().with_workers(1)).unwrap(),
        callback,
    )
}

fn new_network() -> QuicTunnelNetwork {
    init_tls_once();
    QuicTunnelNetwork::new(
        Arc::new(DeviceCache::new(
            &DeviceCacheConfig {
                expire: Duration::from_secs(60),
                capacity: 8,
            },
            None,
        )),
        DefaultTlsServerCertResolver::new(),
        Arc::new(X509IdentityCertFactory),
        QuicCongestionAlgorithm::Bbr,
        Duration::from_secs(3),
        Duration::from_secs(10),
        ServerRuntime::start(ServerRuntimeConfig::new().with_workers(1)).unwrap(),
    )
}

fn punch_remote() -> Endpoint {
    let mut remote = Endpoint::from((
        Protocol::Quic,
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(192, 0, 2, 1), 49152)),
    ));
    remote.set_area(EndpointArea::ServerReflexive);
    remote
}

struct DropFlag(Arc<AtomicBool>);

impl Drop for DropFlag {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[test]
fn udp_punch_missed_tick_keeps_normal_grid_and_intent_offsets() {
    let active = TunnelConnectIntent::active_logical(crate::types::TunnelId::from(703));
    let reverse = TunnelConnectIntent::reverse_logical(crate::types::TunnelId::from(704));

    assert_eq!(udp_punch_start_offset(active), Duration::from_millis(250));
    assert_eq!(udp_punch_start_offset(reverse), Duration::ZERO);
    assert_eq!(
        udp_punch_next_offset(
            Duration::from_millis(250),
            Duration::from_millis(260),
            Duration::from_secs(1),
        ),
        Some(Duration::from_millis(300))
    );
}

#[test]
fn udp_punch_missed_tick_skips_delayed_history_without_catch_up() {
    assert_eq!(
        udp_punch_next_offset(
            Duration::from_millis(50),
            Duration::from_secs(5),
            Duration::from_secs(10),
        ),
        Some(Duration::from_millis(5050))
    );
    assert_eq!(
        udp_punch_next_offset(
            Duration::from_millis(50),
            Duration::from_millis(5001),
            Duration::from_secs(10),
        ),
        Some(Duration::from_millis(5100))
    );
    assert_eq!(
        udp_punch_next_offset(
            Duration::from_millis(5050),
            Duration::from_millis(5050),
            Duration::from_secs(10),
        ),
        Some(Duration::from_millis(5100))
    );
}

#[test]
fn udp_punch_missed_tick_stops_at_deadline_and_on_duration_overflow() {
    assert_eq!(
        udp_punch_next_offset(
            Duration::from_millis(50),
            Duration::from_secs(5),
            Duration::from_secs(5),
        ),
        None
    );
    assert_eq!(
        udp_punch_next_offset(Duration::MAX, Duration::MAX, Duration::MAX),
        None
    );
    assert_eq!(
        udp_punch_next_offset(Duration::ZERO, Duration::MAX, Duration::MAX),
        None
    );
    assert_eq!(
        udp_punch_next_offset(
            Duration::ZERO,
            Duration::from_secs(60 * 60 * 24 * 365 * 7),
            Duration::MAX,
        ),
        None
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn udp_punch_runtime_skips_missed_ticks_for_active_and_reverse() {
    let intents = [
        (
            "active",
            TunnelConnectIntent::active_logical(crate::types::TunnelId::from(705)),
        ),
        (
            "reverse",
            TunnelConnectIntent::reverse_logical(crate::types::TunnelId::from(706)),
        ),
    ];
    for (intent_name, intent) in intents {
        let listener = new_listener();
        listener
            .bind(
                Endpoint::from((Protocol::Quic, "127.0.0.1:0".parse().unwrap())),
                None,
                None,
                false,
            )
            .await
            .unwrap();
        listener.start().await.unwrap();

        let send_times = Arc::new(Mutex::new(Vec::new()));
        let observed_send_times = send_times.clone();
        let send_notify = Arc::new(Notify::new());
        let observed_send_notify = send_notify.clone();
        listener.set_udp_punch_send_observer(Some(Arc::new(move || {
            observed_send_times
                .lock()
                .unwrap()
                .push(std::time::Instant::now());
            observed_send_notify.notify_one();
        })));

        let started_at = std::time::Instant::now()
            .checked_sub(Duration::from_secs(2))
            .unwrap();
        let punch_listener = listener.clone();
        let mut punch = tokio::spawn(async move {
            punch_listener
                .run_udp_punch_burst(
                    punch_remote(),
                    intent,
                    started_at,
                    Duration::from_secs(10),
                )
                .await;
        });
        let observed_three = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if send_times.lock().unwrap().len() >= 3 {
                    break;
                }
                send_notify.notified().await;
            }
        })
        .await;

        listener.close();
        let punch_result = match tokio::time::timeout(Duration::from_secs(1), &mut punch).await {
            Ok(result) => result,
            Err(_) => {
                punch.abort();
                punch.await
            }
        };
        listener.set_udp_punch_send_observer(None);

        assert!(
            observed_three.is_ok(),
            "{intent_name} punch must produce three observed sends"
        );
        assert!(
            punch_result.is_ok(),
            "{intent_name} punch task must stop after listener close"
        );
        let send_times = send_times.lock().unwrap();
        assert!(send_times.len() >= 3);
        for send_pair in send_times[..3].windows(2) {
            assert!(
                send_pair[1].duration_since(send_pair[0]) >= Duration::from_millis(25),
                "{intent_name} punch must not replay missed ticks back-to-back"
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn udp_punch_quic_nat_listener_close_wakes_full_deadline_future() {
    let listener = new_listener();
    listener
        .bind(
            Endpoint::from((Protocol::Quic, "127.0.0.1:0".parse().unwrap())),
            None,
            None,
            false,
        )
        .await
        .unwrap();
    listener.start().await.unwrap();

    let punch_listener = listener.clone();
    let punch = tokio::spawn(async move {
        punch_listener
            .run_udp_punch_burst(
                punch_remote(),
                TunnelConnectIntent::reverse_logical(crate::types::TunnelId::from(701)),
                std::time::Instant::now(),
                Duration::from_secs(10),
            )
            .await;
    });
    tokio::time::sleep(Duration::from_millis(75)).await;
    listener.close();

    tokio::time::timeout(Duration::from_secs(1), punch)
        .await
        .expect("listener close must wake punch before its ten-second deadline")
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn udp_punch_quic_nat_ineligible_and_missing_sender_paths_finish_without_background_work() {
    let listener = new_listener();
    let active = TunnelConnectIntent::active_logical(crate::types::TunnelId::from(702));
    let loopback = Endpoint::from((Protocol::Quic, "127.0.0.1:49153".parse().unwrap()));

    tokio::time::timeout(
        Duration::from_millis(100),
        listener.run_udp_punch_burst(
            loopback,
            active,
            std::time::Instant::now(),
            Duration::from_secs(10),
        ),
    )
    .await
    .expect("an ineligible candidate must not start punch work");
    tokio::time::timeout(
        Duration::from_millis(100),
        listener.run_udp_punch_burst(
            punch_remote(),
            active,
            std::time::Instant::now(),
            Duration::from_secs(10),
        ),
    )
    .await
    .expect("a listener without a registered source socket must not start punch work");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn udp_punch_quic_nat_connect_worker_task_is_aborted_with_owner_future() {
    let dropped = Arc::new(AtomicBool::new(false));
    let task_dropped = dropped.clone();
    let worker = tokio::spawn(async move {
        let _drop_flag = DropFlag(task_dropped);
        std::future::pending::<()>().await;
    });
    let owner = tokio::spawn(AbortOnDropTask::new(worker).join());
    tokio::task::yield_now().await;
    owner.abort();
    assert!(owner.await.unwrap_err().is_cancelled());

    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if dropped.load(Ordering::SeqCst) {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("worker task must observe owner cancellation within a bounded scheduler window");
    assert!(dropped.load(Ordering::SeqCst));
}

#[test]
fn worker_endpoint_guard_distinguishes_empty_and_closed_with_closed_priority() {
    assert!(ensure_worker_endpoints_available(false, false).is_ok());

    let empty = ensure_worker_endpoints_available(false, true).unwrap_err();
    assert_eq!(empty.code(), P2pErrorCode::ErrorState);

    let closed_with_endpoint = ensure_worker_endpoints_available(true, false).unwrap_err();
    assert_eq!(closed_with_endpoint.code(), P2pErrorCode::Interrupted);

    let closed_and_empty = ensure_worker_endpoints_available(true, true).unwrap_err();
    assert_eq!(closed_and_empty.code(), P2pErrorCode::Interrupted);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn close_and_bound_local_race_returns_errors_without_panicking() {
    let listener = new_listener();
    listener
        .bind(
            Endpoint::from((Protocol::Quic, "127.0.0.1:0".parse().unwrap())),
            None,
            None,
            false,
        )
        .await
        .unwrap();
    listener.start().await.unwrap();
    assert_ne!(listener.bound_local().unwrap().addr().port(), 0);

    const READERS: usize = 8;
    let barrier = Arc::new(Barrier::new(READERS + 1));
    let readers = (0..READERS)
        .map(|_| {
            let listener = listener.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                for _ in 0..1_000 {
                    if let Err(err) = listener.bound_local() {
                        assert_eq!(err.code(), P2pErrorCode::Interrupted);
                    }
                }
            })
        })
        .collect::<Vec<_>>();

    barrier.wait();
    listener.close();
    for reader in readers {
        reader.join().unwrap();
    }

    assert_eq!(
        listener.bound_local().unwrap_err().code(),
        P2pErrorCode::Interrupted
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn active_connect_and_close_race_returns_errors_without_panicking() {
    let listener = new_listener();
    let local_identity: P2pIdentityRef =
        Arc::new(generate_rsa_x509_identity(Some("quic-race-local".to_owned())).unwrap());
    let remote_identity: P2pIdentityRef =
        Arc::new(generate_rsa_x509_identity(Some("quic-race-remote".to_owned())).unwrap());
    let blackhole = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let remote = Endpoint::from((Protocol::Quic, blackhole.local_addr().unwrap()));

    let open_empty = listener
        .connect_with_owner_runtime(
            local_identity.clone(),
            Arc::new(X509IdentityCertFactory),
            remote_identity.get_id(),
            Some(remote_identity.get_name()),
            remote,
            QuicCongestionAlgorithm::Bbr,
            Duration::from_millis(50),
            Duration::from_secs(1),
        )
        .await
        .unwrap_err();
    assert_eq!(open_empty.code(), P2pErrorCode::ErrorState);

    listener
        .bind(
            Endpoint::from((Protocol::Quic, "127.0.0.1:0".parse().unwrap())),
            None,
            None,
            false,
        )
        .await
        .unwrap();
    listener.start().await.unwrap();

    let connects = (0..16)
        .map(|_| {
            let listener = listener.clone();
            let local_identity = local_identity.clone();
            let remote_id = remote_identity.get_id();
            let remote_name = remote_identity.get_name();
            tokio::spawn(async move {
                listener
                    .connect_with_owner_runtime(
                        local_identity,
                        Arc::new(X509IdentityCertFactory),
                        remote_id,
                        Some(remote_name),
                        remote,
                        QuicCongestionAlgorithm::Bbr,
                        Duration::from_millis(50),
                        Duration::from_secs(1),
                    )
                    .await
            })
        })
        .collect::<Vec<_>>();

    tokio::task::yield_now().await;
    listener.close();
    for connect in connects {
        let err = connect.await.unwrap().unwrap_err();
        assert!(matches!(
            err.code(),
            P2pErrorCode::ConnectFailed
                | P2pErrorCode::Interrupted
                | P2pErrorCode::ErrorState
        ));
    }

    let closed = listener
        .connect_with_owner_runtime(
            local_identity,
            Arc::new(X509IdentityCertFactory),
            remote_identity.get_id(),
            Some(remote_identity.get_name()),
            remote,
            QuicCongestionAlgorithm::Bbr,
            Duration::from_millis(50),
            Duration::from_secs(1),
        )
        .await
        .unwrap_err();
    assert_eq!(closed.code(), P2pErrorCode::Interrupted);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn network_listener_info_and_close_remain_compatible() {
    let network = new_network();
    let callback: IncomingTunnelCallback = Arc::new(|_| Box::pin(async {}));
    network
        .listen(
            &Endpoint::from((Protocol::Quic, "127.0.0.1:0".parse().unwrap())),
            None,
            None,
            callback,
        )
        .await
        .unwrap();

    let infos = network.listener_infos();
    assert_eq!(infos.len(), 1);
    assert_ne!(infos[0].local.addr().port(), 0);

    network.close_all_listener().await.unwrap();
    assert!(network.listener_infos().is_empty());
}

include!("rendezvous_prediction_tests.rs");
