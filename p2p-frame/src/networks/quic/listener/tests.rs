use super::*;
use crate::finder::{DeviceCache, DeviceCacheConfig};
use crate::networks::quic::QuicTunnelNetwork;
use crate::networks::TunnelNetwork;
use crate::tls::DefaultTlsServerCertResolver;
use crate::x509::{
    X509IdentityCertFactory, X509IdentityFactory, generate_rsa_x509_identity,
};
use sfo_reuseport::ServerRuntimeConfig;
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
