use super::listener::{QuicTunnelListener, connect_with_ep, udp_punch_burst_window};
use super::tunnel::QuicTunnel;
use crate::endpoint::{Endpoint, Protocol};
use crate::error::{P2pError, P2pErrorCode, P2pResult, p2p_err};
use crate::networks::{
    IncomingControlStream, IncomingDatagram, IncomingStream, IncomingTunnelCallback,
    QuicCongestionAlgorithm, TunnelConnectIntent, TunnelForm, TunnelListenerInfo, TunnelNetwork,
    TunnelRef,
};
use crate::p2p_identity::{
    P2pId, P2pIdentityCertCacheRef, P2pIdentityCertFactoryRef, P2pIdentityRef,
};
use crate::tls::ServerCertResolverRef;
use crate::types::{TunnelCandidateId, TunnelId, TunnelIdGenerator};
use rustls::pki_types::CertificateDer;
use sfo_reuseport::ServerRuntime;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const UDP_PUNCH_CONNECT_RETRY_INTERVAL: Duration = Duration::from_millis(50);

pub struct QuicTunnelNetwork {
    listeners: Mutex<Vec<Arc<QuicTunnelListener>>>,
    client_endpoints: Mutex<QuicClientEndpoints>,
    cert_resolver: ServerCertResolverRef,
    cert_factory: P2pIdentityCertFactoryRef,
    cert_cache: P2pIdentityCertCacheRef,
    timeout: Duration,
    idle_timeout: Duration,
    congestion_algorithm: QuicCongestionAlgorithm,
    tunnel_id_gen: Mutex<TunnelIdGenerator>,
    reuse_address: AtomicBool,
    server_runtime: ServerRuntime,
}

#[derive(Default)]
struct QuicClientEndpoints {
    ipv4: Option<quinn::Endpoint>,
    ipv6: Option<quinn::Endpoint>,
}

fn connect_timeout_for_intent(base_timeout: Duration, intent: TunnelConnectIntent) -> Duration {
    if intent.udp_punch_enabled {
        base_timeout.max(udp_punch_burst_window(intent))
    } else {
        base_timeout
    }
}

fn udp_punch_retry_delay_after_error(
    elapsed: Duration,
    connect_timeout: Duration,
    intent: TunnelConnectIntent,
) -> Option<Duration> {
    if !intent.udp_punch_enabled {
        return None;
    }
    let min_failure_window = udp_punch_burst_window(intent).min(connect_timeout);
    if elapsed >= min_failure_window {
        return None;
    }
    Some((min_failure_window - elapsed).min(UDP_PUNCH_CONNECT_RETRY_INTERVAL))
}

impl QuicTunnelNetwork {
    pub fn new(
        cert_cache: P2pIdentityCertCacheRef,
        cert_resolver: ServerCertResolverRef,
        cert_factory: P2pIdentityCertFactoryRef,
        congestion_algorithm: QuicCongestionAlgorithm,
        timeout: Duration,
        idle_timeout: Duration,
        server_runtime: ServerRuntime,
    ) -> Self {
        Self {
            listeners: Mutex::new(Vec::new()),
            client_endpoints: Mutex::new(QuicClientEndpoints::default()),
            cert_resolver,
            cert_factory,
            cert_cache,
            timeout,
            idle_timeout,
            congestion_algorithm,
            tunnel_id_gen: Mutex::new(TunnelIdGenerator::new()),
            reuse_address: AtomicBool::new(false),
            server_runtime,
        }
    }

    fn next_tunnel_id(&self, local_id: &P2pId, remote_id: &P2pId) -> TunnelId {
        let creator_high_bit = if local_id.as_slice() > remote_id.as_slice() {
            1u32 << 31
        } else {
            0
        };
        loop {
            let seq = self.tunnel_id_gen.lock().unwrap().generate().value() & !(1u32 << 31);
            if seq != 0 {
                return TunnelId::from(creator_high_bit | seq);
            }
        }
    }

    async fn resolve_remote_identity(
        &self,
        socket: &quinn::Connection,
        remote_id: &P2pId,
        remote_name: Option<String>,
    ) -> P2pResult<P2pId> {
        if remote_id.is_default() {
            let remote_cert = {
                let peer_identity = socket
                    .peer_identity()
                    .ok_or_else(|| p2p_err!(P2pErrorCode::CertError, "no peer identity"))?;
                let remote_cert = peer_identity
                    .as_ref()
                    .downcast_ref::<Vec<CertificateDer>>()
                    .ok_or_else(|| p2p_err!(P2pErrorCode::CertError, "peer cert type invalid"))?;
                if remote_cert.is_empty() {
                    return Err(p2p_err!(P2pErrorCode::CertError, "no cert"));
                }
                remote_cert[0].as_ref().to_vec()
            };
            let cert = self.cert_factory.create(&remote_cert)?;
            self.cert_cache.add(&cert.get_id(), &cert).await?;
            Ok(cert.get_id())
        } else {
            let _ = remote_name;
            Ok(remote_id.clone())
        }
    }

    fn client_endpoint(&self, remote: &Endpoint) -> P2pResult<quinn::Endpoint> {
        let mut client_endpoints = self.client_endpoints.lock().unwrap();
        let endpoint = if remote.addr().is_ipv4() {
            &mut client_endpoints.ipv4
        } else {
            &mut client_endpoints.ipv6
        };
        if let Some(endpoint) = endpoint.as_ref() {
            return Ok(endpoint.clone());
        }

        let local = Endpoint::default_udp(remote);
        let created = quinn::Endpoint::client(*local.addr()).map_err(|err| {
            p2p_err!(
                P2pErrorCode::ConnectFailed,
                "create quic client endpoint {} failed: {}",
                local,
                err
            )
        })?;
        log::info!(
            "quic client endpoint created local={} remote_ip_version={}",
            Endpoint::from((Protocol::Quic, created.local_addr().unwrap_or(*local.addr()))),
            if remote.addr().is_ipv4() { "ipv4" } else { "ipv6" }
        );
        *endpoint = Some(created.clone());
        Ok(created)
    }

    async fn finish_connect(
        &self,
        socket: quinn::Connection,
        local_identity: &P2pIdentityRef,
        local_ep: Endpoint,
        remote_id: &P2pId,
        remote_name: Option<String>,
        intent: TunnelConnectIntent,
    ) -> P2pResult<TunnelRef> {
        let remote_ep = Endpoint::from((Protocol::Quic, socket.remote_address()));
        let resolved_remote_id = self
            .resolve_remote_identity(&socket, remote_id, remote_name)
            .await?;

        let tunnel_id = if intent.tunnel_id == TunnelId::default() {
            self.next_tunnel_id(&local_identity.get_id(), &resolved_remote_id)
        } else {
            intent.tunnel_id
        };
        let candidate_id = if intent.candidate_id == TunnelCandidateId::default() {
            TunnelCandidateId::from(tunnel_id.value())
        } else {
            intent.candidate_id
        };
        Ok(QuicTunnel::connect(
            socket,
            tunnel_id,
            candidate_id,
            intent.is_reverse,
            local_identity.get_id(),
            resolved_remote_id,
            local_ep,
            remote_ep,
        )
        .await?)
    }

    async fn open_with_client_endpoint(
        &self,
        local_identity: &P2pIdentityRef,
        remote: &Endpoint,
        remote_id: &P2pId,
        remote_name: Option<String>,
        intent: TunnelConnectIntent,
    ) -> P2pResult<TunnelRef> {
        let endpoint = self.client_endpoint(remote)?;
        let local_ep = Endpoint::from((
            Protocol::Quic,
            endpoint.local_addr().map_err(|err| {
                p2p_err!(
                    P2pErrorCode::IoError,
                    "get quic client endpoint local address failed: {}",
                    err
                )
            })?,
        ));
        let socket = connect_with_ep(
            endpoint,
            local_identity.clone(),
            self.cert_factory.clone(),
            remote_id.clone(),
            remote_name.clone(),
            *remote,
            self.congestion_algorithm,
            connect_timeout_for_intent(self.timeout, intent),
            self.idle_timeout,
        )
        .await
        .map_err(|err| {
            log::error!(
                "quic client endpoint connect failed local_id={} local_ep={} remote={} remote_id={} remote_name={:?} code={:?} msg={}",
                local_identity.get_id(),
                local_ep,
                remote,
                remote_id,
                remote_name,
                err.code(),
                err
            );
            err
        })?;
        let mut local_ep = local_ep;
        if let Some(local_ip) = socket.local_ip() {
            local_ep.mut_addr().set_ip(local_ip);
        }
        self.finish_connect(
            socket,
            local_identity,
            local_ep,
            remote_id,
            remote_name,
            intent,
        )
        .await
    }

    async fn open_or_connect(
        &self,
        listener: &Arc<QuicTunnelListener>,
        local_identity: &P2pIdentityRef,
        remote: &Endpoint,
        remote_id: &P2pId,
        remote_name: Option<String>,
        intent: TunnelConnectIntent,
    ) -> P2pResult<TunnelRef> {
        let connect_timeout = connect_timeout_for_intent(self.timeout, intent);
        if intent.udp_punch_enabled {
            listener.start_udp_punch_burst(*remote, intent, connect_timeout);
        }
        let connect_started_at = Instant::now();
        let socket = loop {
            let elapsed = connect_started_at.elapsed();
            let Some(attempt_timeout) = connect_timeout.checked_sub(elapsed) else {
                return Err(p2p_err!(
                    P2pErrorCode::ConnectFailed,
                    "quic to {} connect failed",
                    remote
                ));
            };
            if attempt_timeout.is_zero() {
                return Err(p2p_err!(
                    P2pErrorCode::ConnectFailed,
                    "quic to {} connect failed",
                    remote
                ));
            }
            match listener
                .connect_with_owner_runtime(
                    local_identity.clone(),
                    self.cert_factory.clone(),
                    remote_id.clone(),
                    remote_name.clone(),
                    *remote,
                    self.congestion_algorithm,
                    attempt_timeout,
                    self.idle_timeout,
                )
                .await
            {
                Ok(socket) => break socket,
                Err(err) => {
                    if let Some(delay) = udp_punch_retry_delay_after_error(
                        connect_started_at.elapsed(),
                        connect_timeout,
                        intent,
                    ) {
                        log::trace!(
                            "quic tunnel connect early failure during udp punch window local_id={} local_ep={} remote={} remote_id={} retry_delay_ms={} code={:?} msg={}",
                            local_identity.get_id(),
                            listener.bound_local(),
                            remote,
                            remote_id,
                            delay.as_millis(),
                            err.code(),
                            err
                        );
                        crate::runtime::sleep(delay).await;
                        continue;
                    }
                    log::error!(
                        "quic tunnel connect failed local_id={} local_ep={} remote={} remote_id={} remote_name={:?} code={:?} msg={}",
                        local_identity.get_id(),
                        listener.bound_local(),
                        remote,
                        remote_id,
                        remote_name,
                        err.code(),
                        err
                    );
                    return Err(err);
                }
            }
        };
        self.finish_connect(
            socket,
            local_identity,
            listener.bound_local(),
            remote_id,
            remote_name,
            intent,
        )
        .await
    }
}

#[async_trait::async_trait]
impl TunnelNetwork for QuicTunnelNetwork {
    fn protocol(&self) -> Protocol {
        Protocol::Quic
    }

    fn is_udp(&self) -> bool {
        true
    }

    fn set_reuse_address(&self, reuse_address: bool) {
        self.reuse_address.store(reuse_address, Ordering::Relaxed);
    }

    async fn listen(
        &self,
        local: &Endpoint,
        out: Option<Endpoint>,
        mapping_port: Option<u16>,
        on_incoming_tunnel: IncomingTunnelCallback,
    ) -> P2pResult<()> {
        let listener = QuicTunnelListener::new(
            self.cert_cache.clone(),
            self.cert_resolver.clone(),
            self.cert_factory.clone(),
            self.congestion_algorithm,
            self.server_runtime.clone(),
            on_incoming_tunnel,
        );
        listener
            .bind(
                *local,
                out,
                mapping_port,
                self.reuse_address.load(Ordering::Relaxed),
            )
            .await?;
        listener.start().await?;
        self.listeners.lock().unwrap().push(listener.clone());
        Ok(())
    }

    async fn close_all_listener(&self) -> P2pResult<()> {
        let listeners = {
            let mut listeners = self.listeners.lock().unwrap();
            let cloned = listeners.clone();
            listeners.clear();
            cloned
        };
        for listener in listeners {
            listener.close();
        }
        Ok(())
    }

    fn listener_infos(&self) -> Vec<TunnelListenerInfo> {
        self.listeners
            .lock()
            .unwrap()
            .iter()
            .map(|listener| TunnelListenerInfo {
                local: listener.bound_local(),
                mapping_port: listener.mapping_port(),
            })
            .collect()
    }

    async fn create_tunnel_with_intent(
        &self,
        local_identity: &P2pIdentityRef,
        remote: &Endpoint,
        remote_id: &P2pId,
        remote_name: Option<String>,
        intent: TunnelConnectIntent,
    ) -> P2pResult<TunnelRef> {
        let listeners = { self.listeners.lock().unwrap().clone() };
        let mut last_err: Option<P2pError> = None;
        for listener in listeners.iter() {
            if !listener.local().is_same_ip_version(remote) {
                continue;
            }
            match self
                .open_or_connect(
                    listener,
                    local_identity,
                    remote,
                    remote_id,
                    remote_name.clone(),
                    intent,
                )
                .await
            {
                Ok(tunnel) => return Ok(tunnel),
                Err(err) => last_err = Some(err),
            }
        }
        if let Some(err) = last_err {
            Err(err)
        } else {
            self.open_with_client_endpoint(
                local_identity,
                remote,
                remote_id,
                remote_name,
                intent,
            )
            .await
        }
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
        let listeners = { self.listeners.lock().unwrap().clone() };
        for listener in listeners.iter() {
            if (&listener.local() != local_ep && &listener.bound_local() != local_ep)
                || !listener.local().is_same_ip_version(remote)
            {
                continue;
            }
            return self
                .open_or_connect(
                    listener,
                    local_identity,
                    remote,
                    remote_id,
                    remote_name,
                    intent,
                )
                .await;
        }
        Err(p2p_err!(
            P2pErrorCode::NotFound,
            "no listener found for local ep: {}",
            local_ep
        ))
    }
}

#[cfg(test)]
mod udp_punch_timeout_tests {
    use super::*;

    #[test]
    fn udp_punch_intent_extends_connect_timeout_to_punch_window() {
        let base_timeout = Duration::from_millis(100);
        let no_punch = TunnelConnectIntent::active_logical(crate::types::TunnelId::from(11));
        let active_punch = no_punch.set_udp_punch_enabled(true);
        let reverse_punch = TunnelConnectIntent::reverse_logical(crate::types::TunnelId::from(12))
            .set_udp_punch_enabled(true);

        assert_eq!(
            connect_timeout_for_intent(base_timeout, no_punch),
            base_timeout
        );
        assert_eq!(
            connect_timeout_for_intent(base_timeout, active_punch),
            Duration::from_secs(1)
        );
        assert_eq!(
            connect_timeout_for_intent(base_timeout, reverse_punch),
            Duration::from_secs(1)
        );
        assert_eq!(
            connect_timeout_for_intent(Duration::from_secs(3), active_punch),
            Duration::from_secs(3)
        );
    }

    #[test]
    fn udp_punch_early_error_retries_until_punch_window_is_covered() {
        let no_punch = TunnelConnectIntent::active_logical(crate::types::TunnelId::from(12));
        let active_punch = no_punch.set_udp_punch_enabled(true);
        let reverse_punch = TunnelConnectIntent::reverse_logical(crate::types::TunnelId::from(13))
            .set_udp_punch_enabled(true);

        assert_eq!(
            udp_punch_retry_delay_after_error(
                Duration::from_millis(100),
                Duration::from_secs(3),
                no_punch,
            ),
            None
        );
        assert_eq!(
            udp_punch_retry_delay_after_error(
                Duration::from_millis(100),
                Duration::from_secs(3),
                active_punch,
            ),
            Some(Duration::from_millis(50))
        );
        assert_eq!(
            udp_punch_retry_delay_after_error(
                Duration::from_millis(200),
                Duration::from_secs(3),
                reverse_punch,
            ),
            Some(Duration::from_millis(50))
        );
        assert_eq!(
            udp_punch_retry_delay_after_error(
                Duration::from_millis(950),
                Duration::from_secs(3),
                reverse_punch,
            ),
            Some(Duration::from_millis(50))
        );
        assert_eq!(
            udp_punch_retry_delay_after_error(
                Duration::from_millis(1000),
                Duration::from_secs(3),
                reverse_punch,
            ),
            None
        );
    }
}

#[cfg(all(test, feature = "x509"))]
mod tests {
    use super::*;
    use crate::finder::{DeviceCache, DeviceCacheConfig};
    use crate::networks::{ListenVPortRegistry, Tunnel, TunnelPurpose, allow_all_listen_vports};
    use crate::runtime::{AsyncReadExt, AsyncWriteExt};
    use crate::tls::{DefaultTlsServerCertResolver, TlsServerCertResolver};
    use crate::x509::{
        X509IdentityCertFactory, X509IdentityFactory, generate_ed25519_x509_identity,
        generate_rsa_x509_identity,
    };
    use std::collections::HashMap;
    use std::sync::{Arc, LazyLock};
    use std::sync::{Mutex as StdMutex, Once};
    use tokio::sync::{Mutex as AsyncMutex, mpsc};
    use tokio::time::timeout;

    static TLS_INIT: Once = Once::new();
    type TestStreamRx = mpsc::Receiver<P2pResult<IncomingStream>>;
    type TestDatagramRx = mpsc::Receiver<P2pResult<IncomingDatagram>>;
    type TestControlStreamRx = mpsc::Receiver<P2pResult<IncomingControlStream>>;
    static TEST_STREAM_RX: LazyLock<StdMutex<HashMap<String, TestStreamRx>>> =
        LazyLock::new(|| StdMutex::new(HashMap::new()));
    static TEST_DATAGRAM_RX: LazyLock<StdMutex<HashMap<String, TestDatagramRx>>> =
        LazyLock::new(|| StdMutex::new(HashMap::new()));
    static TEST_CONTROL_STREAM_RX: LazyLock<StdMutex<HashMap<String, TestControlStreamRx>>> =
        LazyLock::new(|| StdMutex::new(HashMap::new()));
    const TEST_CHANNEL_CAPACITY: usize = 8;

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

    async fn listen_datagram_collect(
        tunnel: &dyn Tunnel,
        vports: crate::networks::ListenVPortsRef,
    ) {
        let (tx, rx) = mpsc::channel(TEST_CHANNEL_CAPACITY);
        let callback: crate::networks::IncomingDatagramCallback = Arc::new(move |accepted| {
            let tx = tx.clone();
            Box::pin(async move {
                let _ = tx.send(accepted).await;
            })
        });
        TEST_DATAGRAM_RX
            .lock()
            .unwrap()
            .insert(test_tunnel_key(tunnel), rx);
        tunnel.listen_datagram(vports, callback).await.unwrap();
    }

    async fn listen_control_stream_collect(
        tunnel: &dyn Tunnel,
        vports: crate::networks::ListenVPortsRef,
    ) {
        let (tx, rx) = mpsc::channel(TEST_CHANNEL_CAPACITY);
        let callback: crate::networks::IncomingControlStreamCallback = Arc::new(move |accepted| {
            let tx = tx.clone();
            Box::pin(async move {
                let _ = tx.send(accepted).await;
            })
        });
        TEST_CONTROL_STREAM_RX
            .lock()
            .unwrap()
            .insert(test_tunnel_key(tunnel), rx);
        tunnel
            .listen_control_stream(vports, callback)
            .await
            .unwrap();
    }

    async fn recv_stream(tunnel: &dyn Tunnel) -> P2pResult<IncomingStream> {
        let key = test_tunnel_key(tunnel);
        let mut rx =
            TEST_STREAM_RX.lock().unwrap().remove(&key).ok_or_else(|| {
                p2p_err!(P2pErrorCode::Interrupted, "test stream receiver missing")
            })?;
        let accepted = rx
            .recv()
            .await
            .ok_or_else(|| p2p_err!(P2pErrorCode::Interrupted, "test stream receiver closed"))?;
        TEST_STREAM_RX.lock().unwrap().insert(key, rx);
        accepted
    }

    async fn recv_datagram(tunnel: &dyn Tunnel) -> P2pResult<IncomingDatagram> {
        let key = test_tunnel_key(tunnel);
        let mut rx = TEST_DATAGRAM_RX
            .lock()
            .unwrap()
            .remove(&key)
            .ok_or_else(|| p2p_err!(P2pErrorCode::Interrupted, "test datagram receiver missing"))?;
        let accepted = rx
            .recv()
            .await
            .ok_or_else(|| p2p_err!(P2pErrorCode::Interrupted, "test datagram receiver closed"))?;
        TEST_DATAGRAM_RX.lock().unwrap().insert(key, rx);
        accepted
    }

    async fn recv_control_stream(tunnel: &dyn Tunnel) -> P2pResult<IncomingControlStream> {
        let key = test_tunnel_key(tunnel);
        let mut rx = TEST_CONTROL_STREAM_RX
            .lock()
            .unwrap()
            .remove(&key)
            .ok_or_else(|| {
                p2p_err!(
                    P2pErrorCode::Interrupted,
                    "test control stream receiver missing"
                )
            })?;
        let accepted = rx.recv().await.ok_or_else(|| {
            p2p_err!(
                P2pErrorCode::Interrupted,
                "test control stream receiver closed"
            )
        })?;
        TEST_CONTROL_STREAM_RX.lock().unwrap().insert(key, rx);
        accepted
    }

    struct TestNetworkPair {
        client_network: QuicTunnelNetwork,
        client_identity: P2pIdentityRef,
        client_local_ep: Endpoint,
        server_network: QuicTunnelNetwork,
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

    async fn accept_incoming(rx: &AsyncMutex<mpsc::Receiver<P2pResult<TunnelRef>>>) -> TunnelRef {
        let mut rx = rx.lock().await;
        accept_incoming_rx(&mut rx).await
    }

    async fn accept_incoming_rx(rx: &mut mpsc::Receiver<P2pResult<TunnelRef>>) -> TunnelRef {
        rx.recv().await.unwrap().unwrap()
    }

    fn new_identity(name: &str) -> P2pIdentityRef {
        Arc::new(generate_rsa_x509_identity(Some(name.to_owned())).unwrap())
    }

    fn new_ed25519_identity(name: &str) -> P2pIdentityRef {
        Arc::new(generate_ed25519_x509_identity(Some(name.to_owned())).unwrap())
    }

    fn new_cert_cache() -> crate::p2p_identity::P2pIdentityCertCacheRef {
        Arc::new(DeviceCache::new(
            &DeviceCacheConfig {
                expire: Duration::from_secs(60),
                capacity: 64,
            },
            None,
        ))
    }

    fn loopback_quic_ep() -> Endpoint {
        Endpoint::from((Protocol::Quic, "127.0.0.1:0".parse().unwrap()))
    }

    fn purpose_of(vport: u16) -> TunnelPurpose {
        TunnelPurpose::from_value(&vport).unwrap()
    }

    async fn register_listener_identity(
        resolver: &Arc<DefaultTlsServerCertResolver>,
        identity: P2pIdentityRef,
    ) {
        resolver
            .add_server_identity(identity.clone())
            .await
            .unwrap();
    }

    fn new_network() -> (QuicTunnelNetwork, Arc<DefaultTlsServerCertResolver>) {
        let resolver = DefaultTlsServerCertResolver::new();
        let cert_factory: P2pIdentityCertFactoryRef = Arc::new(X509IdentityCertFactory);
        (
            QuicTunnelNetwork::new(
                new_cert_cache(),
                resolver.clone(),
                cert_factory,
                QuicCongestionAlgorithm::Bbr,
                Duration::from_secs(3),
                Duration::from_secs(10),
                ServerRuntime::start(sfo_reuseport::ServerRuntimeConfig::default())
                    .expect("sfo reuseport server runtime should start"),
            ),
            resolver,
        )
    }

    async fn setup_network_pair() -> TestNetworkPair {
        init_tls_once();

        let (client_network, client_resolver) = new_network();
        let (server_network, server_resolver) = new_network();

        let client_identity = new_identity("quic-client");
        let server_identity = new_identity("quic-server");

        register_listener_identity(&client_resolver, client_identity.clone()).await;
        register_listener_identity(&server_resolver, server_identity.clone()).await;

        client_network
            .listen(&loopback_quic_ep(), None, None, ignore_incoming())
            .await
            .unwrap();
        let (server_callback, server_incoming) = incoming_channel();
        server_network
            .listen(&loopback_quic_ep(), None, None, server_callback)
            .await
            .unwrap();

        let client_local_ep = client_network.listener_infos()[0].local;
        let server_local_ep = server_network.listener_infos()[0].local;

        TestNetworkPair {
            client_network,
            client_identity,
            client_local_ep,
            server_network,
            server_identity,
            server_local_ep,
            server_incoming: AsyncMutex::new(server_incoming),
        }
    }

    impl TestNetworkPair {
        async fn connect(&self) -> (TunnelRef, TunnelRef) {
            let opened = self
                .client_network
                .create_tunnel_with_local_ep(
                    &self.client_identity,
                    &self.client_local_ep,
                    &self.server_local_ep,
                    &self.server_identity.get_id(),
                    Some(self.server_identity.get_name()),
                )
                .await
                .unwrap();
            let accepted = accept_incoming(&self.server_incoming).await;
            (opened, accepted)
        }
    }

    #[tokio::test]
    async fn quic_tunnel_mixed_rsa_and_ed25519_handshake_ok() {
        init_tls_once();

        let (client_network, client_resolver) = new_network();
        let (server_network, server_resolver) = new_network();
        let client_identity = new_ed25519_identity("quic-client-ed25519");
        let server_identity = new_identity("quic-server-rsa");

        register_listener_identity(&client_resolver, client_identity.clone()).await;
        register_listener_identity(&server_resolver, server_identity.clone()).await;

        client_network
            .listen(&loopback_quic_ep(), None, None, ignore_incoming())
            .await
            .unwrap();
        let (server_callback, mut server_incoming) = incoming_channel();
        server_network
            .listen(&loopback_quic_ep(), None, None, server_callback)
            .await
            .unwrap();

        let client_ep = client_network.listener_infos()[0].local;
        let server_ep = server_network.listener_infos()[0].local;

        let opened = client_network
            .create_tunnel(
                &client_identity,
                &server_ep,
                &server_identity.get_id(),
                Some(server_identity.get_name()),
            )
            .await
            .unwrap();
        let accepted = accept_incoming_rx(&mut server_incoming).await;

        assert_eq!(opened.local_id(), client_identity.get_id());
        assert_eq!(opened.remote_id(), server_identity.get_id());
        assert_eq!(opened.local_ep(), Some(client_ep));
        assert_eq!(accepted.local_id(), server_identity.get_id());
        assert_eq!(accepted.remote_id(), client_identity.get_id());

        opened.close().unwrap();
        accepted.close().unwrap();
    }

    #[tokio::test]
    async fn quic_tunnel_listener_accepts_multiple_local_identities_on_one_port() {
        init_tls_once();

        let (client_network, client_resolver) = new_network();
        let (server_network, server_resolver) = new_network();
        let client_identity = new_identity("quic-client-multi-identity");
        let server_identity_a = new_identity("quic-server-multi-identity-a");
        let server_identity_b = new_identity("quic-server-multi-identity-b");

        register_listener_identity(&client_resolver, client_identity.clone()).await;
        register_listener_identity(&server_resolver, server_identity_a.clone()).await;
        register_listener_identity(&server_resolver, server_identity_b.clone()).await;

        client_network
            .listen(&loopback_quic_ep(), None, None, ignore_incoming())
            .await
            .unwrap();
        let (server_callback, mut server_incoming) = incoming_channel();
        server_network
            .listen(&loopback_quic_ep(), None, None, server_callback)
            .await
            .unwrap();

        let server_ep = server_network.listener_infos()[0].local;

        let opened_a = client_network
            .create_tunnel(
                &client_identity,
                &server_ep,
                &server_identity_a.get_id(),
                Some(server_identity_a.get_name()),
            )
            .await
            .unwrap();
        let accepted_a = accept_incoming_rx(&mut server_incoming).await;
        assert_eq!(opened_a.remote_id(), server_identity_a.get_id());
        assert_eq!(accepted_a.local_id(), server_identity_a.get_id());

        let opened_b = client_network
            .create_tunnel(
                &client_identity,
                &server_ep,
                &server_identity_b.get_id(),
                Some(server_identity_b.get_name()),
            )
            .await
            .unwrap();
        let accepted_b = accept_incoming_rx(&mut server_incoming).await;
        assert_eq!(opened_b.remote_id(), server_identity_b.get_id());
        assert_eq!(accepted_b.local_id(), server_identity_b.get_id());

        opened_a.close().unwrap();
        accepted_a.close().unwrap();
        opened_b.close().unwrap();
        accepted_b.close().unwrap();
    }

    #[tokio::test]
    async fn quic_tunnel_stream_round_trip_ok() {
        let pair = setup_network_pair().await;
        let (opened, accepted) = pair.connect().await;

        assert_eq!(opened.protocol(), Protocol::Quic);
        assert_eq!(accepted.protocol(), Protocol::Quic);
        assert_eq!(opened.form(), TunnelForm::Active);
        assert_eq!(accepted.form(), TunnelForm::Passive);
        assert_eq!(opened.state(), crate::networks::TunnelState::Connected);
        assert_eq!(accepted.state(), crate::networks::TunnelState::Connected);
        assert_eq!(opened.tunnel_id(), accepted.tunnel_id());
        assert!(!opened.is_reverse());
        assert!(!accepted.is_reverse());

        listen_stream_collect(&*accepted, allow_all_listen_vports()).await;

        let (opened_stream, accepted_stream) = tokio::join!(
            opened.open_stream(purpose_of(1001)),
            recv_stream(&*accepted)
        );
        let (mut read, mut write) = opened_stream.unwrap();
        let (purpose, mut peer_read, mut peer_write) = accepted_stream.unwrap();

        assert_eq!(purpose, purpose_of(1001));

        write.write_all(b"ping").await.unwrap();
        write.shutdown().await.unwrap();
        let mut buf = Vec::new();
        peer_read.read_to_end(&mut buf).await.unwrap();
        assert_eq!(buf, b"ping");

        peer_write.write_all(b"pong").await.unwrap();
        peer_write.shutdown().await.unwrap();
        let mut reply = Vec::new();
        read.read_to_end(&mut reply).await.unwrap();
        assert_eq!(reply, b"pong");

        opened.close().unwrap();
        accepted.close().unwrap();
    }

    #[tokio::test]
    async fn quic_tunnel_control_stream_round_trip_ok() {
        let pair = setup_network_pair().await;
        let (opened, accepted) = pair.connect().await;

        listen_control_stream_collect(&*accepted, allow_all_listen_vports()).await;

        let (opened_stream, accepted_stream) = tokio::join!(
            opened.open_control_stream(purpose_of(1050)),
            recv_control_stream(&*accepted)
        );
        let (mut read, mut write) = opened_stream.unwrap();
        let (purpose, mut peer_read, mut peer_write) = accepted_stream.unwrap();

        assert_eq!(purpose, purpose_of(1050));

        write.write_all(b"control-ping").await.unwrap();
        write.shutdown().await.unwrap();
        let mut buf = Vec::new();
        peer_read.read_to_end(&mut buf).await.unwrap();
        assert_eq!(buf, b"control-ping");

        peer_write.write_all(b"control-pong").await.unwrap();
        peer_write.shutdown().await.unwrap();
        let mut reply = Vec::new();
        read.read_to_end(&mut reply).await.unwrap();
        assert_eq!(reply, b"control-pong");

        opened.close().unwrap();
        accepted.close().unwrap();
    }

    #[tokio::test]
    async fn quic_tunnel_can_open_new_stream_channel_after_previous_channel_closes() {
        let pair = setup_network_pair().await;
        let (opened, accepted) = pair.connect().await;

        listen_stream_collect(&*accepted, allow_all_listen_vports()).await;

        for (vport, payload) in [(1051, b"first".as_slice()), (1052, b"second".as_slice())] {
            let (opened_stream, accepted_stream) = tokio::join!(
                opened.open_stream(purpose_of(vport)),
                recv_stream(&*accepted)
            );
            let (_read, mut write) = opened_stream.unwrap();
            let (purpose, mut peer_read, _peer_write) = accepted_stream.unwrap();

            assert_eq!(purpose, purpose_of(vport));

            write.write_all(payload).await.unwrap();
            write.shutdown().await.unwrap();

            let mut received = Vec::new();
            peer_read.read_to_end(&mut received).await.unwrap();
            assert_eq!(received, payload);
        }

        opened.close().unwrap();
        accepted.close().unwrap();
    }

    #[tokio::test]
    async fn quic_tunnel_datagram_round_trip_ok() {
        let pair = setup_network_pair().await;
        let (opened, accepted) = pair.connect().await;

        listen_datagram_collect(&*accepted, allow_all_listen_vports()).await;

        let (opened_datagram, accepted_datagram) = tokio::join!(
            opened.open_datagram(purpose_of(2002)),
            recv_datagram(&*accepted)
        );
        let mut writer = opened_datagram.unwrap();
        let (purpose, mut peer_read) = accepted_datagram.unwrap();

        assert_eq!(purpose, purpose_of(2002));

        writer.write_all(b"hello").await.unwrap();
        writer.shutdown().await.unwrap();
        let mut buf = Vec::new();
        peer_read.read_to_end(&mut buf).await.unwrap();
        assert_eq!(buf, b"hello");

        opened.close().unwrap();
        accepted.close().unwrap();
    }

    #[tokio::test]
    async fn quic_tunnel_open_stream_without_listen_stays_pending() {
        let pair = setup_network_pair().await;
        let (opened, accepted) = pair.connect().await;

        let result = timeout(
            Duration::from_millis(200),
            opened.open_stream(purpose_of(6001)),
        )
        .await;
        assert!(result.is_err());

        opened.close().unwrap();
        accepted.close().unwrap();
    }

    #[tokio::test]
    async fn quic_tunnel_open_stream_timeout_closes_tunnel() {
        let pair = setup_network_pair().await;
        let (opened, accepted) = pair.connect().await;

        let err = timeout(
            Duration::from_secs(12),
            opened.open_stream(purpose_of(6101)),
        )
        .await
        .unwrap()
        .err()
        .unwrap();
        assert_eq!(err.code(), P2pErrorCode::Timeout);
        assert!(opened.is_closed());
        assert_eq!(opened.state(), crate::networks::TunnelState::Closed);

        accepted.close().unwrap();
    }

    #[tokio::test]
    async fn quic_tunnel_open_datagram_without_listen_stays_pending() {
        let pair = setup_network_pair().await;
        let (opened, accepted) = pair.connect().await;

        let result = timeout(
            Duration::from_millis(200),
            opened.open_datagram(purpose_of(6002)),
        )
        .await;
        assert!(result.is_err());

        opened.close().unwrap();
        accepted.close().unwrap();
    }

    #[tokio::test]
    async fn quic_tunnel_open_datagram_timeout_closes_tunnel() {
        let pair = setup_network_pair().await;
        let (opened, accepted) = pair.connect().await;

        let err = timeout(
            Duration::from_secs(12),
            opened.open_datagram(purpose_of(6102)),
        )
        .await
        .unwrap()
        .err()
        .unwrap();
        assert_eq!(err.code(), P2pErrorCode::Timeout);
        assert!(opened.is_closed());
        assert_eq!(opened.state(), crate::networks::TunnelState::Closed);

        accepted.close().unwrap();
    }

    #[tokio::test]
    async fn quic_tunnel_open_stream_to_unlistened_port_returns_error() {
        let pair = setup_network_pair().await;
        let (opened, accepted) = pair.connect().await;
        let empty_vports = ListenVPortRegistry::<()>::new();

        listen_stream_collect(&*accepted, empty_vports.as_listen_vports_ref()).await;

        let err = opened.open_stream(purpose_of(6553)).await.err().unwrap();
        assert_eq!(err.code(), P2pErrorCode::PortNotListen);

        opened.close().unwrap();
        accepted.close().unwrap();
    }

    #[tokio::test]
    async fn quic_tunnel_open_datagram_to_unlistened_port_returns_error() {
        let pair = setup_network_pair().await;
        let (opened, accepted) = pair.connect().await;
        let empty_vports = ListenVPortRegistry::<()>::new();

        listen_datagram_collect(&*accepted, empty_vports.as_listen_vports_ref()).await;

        let err = opened.open_datagram(purpose_of(6554)).await.err().unwrap();
        assert_eq!(err.code(), P2pErrorCode::PortNotListen);

        opened.close().unwrap();
        accepted.close().unwrap();
    }

    #[tokio::test]
    async fn quic_tunnel_accept_stream_requires_listen_first() {
        let pair = setup_network_pair().await;
        let (opened, accepted) = pair.connect().await;

        let err = recv_stream(&*accepted).await.err().unwrap();
        assert_eq!(err.code(), P2pErrorCode::Interrupted);

        opened.close().unwrap();
        accepted.close().unwrap();
    }

    #[tokio::test]
    async fn quic_tunnel_accept_datagram_requires_listen_first() {
        let pair = setup_network_pair().await;
        let (opened, accepted) = pair.connect().await;

        let err = recv_datagram(&*accepted).await.err().unwrap();
        assert_eq!(err.code(), P2pErrorCode::Interrupted);

        opened.close().unwrap();
        accepted.close().unwrap();
    }

    #[tokio::test]
    async fn quic_tunnel_create_tunnel_opens_new_connection_each_time() {
        let pair = setup_network_pair().await;

        let first_opened = pair
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
        let first_accepted = accept_incoming(&pair.server_incoming).await;

        let second_opened = pair
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
        let second_accepted = accept_incoming(&pair.server_incoming).await;

        assert_ne!(Arc::as_ptr(&first_opened), Arc::as_ptr(&second_opened));
        assert_ne!(Arc::as_ptr(&first_accepted), Arc::as_ptr(&second_accepted));

        first_opened.close().unwrap();
        second_opened.close().unwrap();
        first_accepted.close().unwrap();
        second_accepted.close().unwrap();
    }

    #[tokio::test]
    async fn quic_tunnel_close_all_listener_clears_listeners() {
        let pair = setup_network_pair().await;

        let _ = pair
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

        pair.client_network.close_all_listener().await.unwrap();
        assert!(pair.client_network.listener_infos().is_empty());

        accepted.close().unwrap();
    }

    #[tokio::test]
    async fn quic_tunnel_create_tunnel_no_listener_uses_client_endpoint() {
        init_tls_once();

        let (client_network, _client_resolver) = new_network();
        let (server_network, server_resolver) = new_network();
        let local_identity = new_identity("quic-no-listener-client");
        let server_identity = new_identity("quic-no-listener-server");
        register_listener_identity(&server_resolver, server_identity.clone()).await;

        let (server_callback, mut server_incoming) = incoming_channel();
        server_network
            .listen(&loopback_quic_ep(), None, None, server_callback)
            .await
            .unwrap();
        let server_ep = server_network.listener_infos()[0].local;
        let server_id = server_identity.get_id();
        let server_name = server_identity.get_name();

        assert!(client_network.listener_infos().is_empty());

        let first = client_network.create_tunnel(
            &local_identity,
            &server_ep,
            &server_id,
            Some(server_name.clone()),
        );
        let second = client_network.create_tunnel(
            &local_identity,
            &server_ep,
            &server_id,
            Some(server_name.clone()),
        );
        let (first, second) = tokio::join!(first, second);
        let first = first.unwrap();
        let second = second.unwrap();
        let first_accepted = accept_incoming_rx(&mut server_incoming).await;
        let second_accepted = accept_incoming_rx(&mut server_incoming).await;

        let client_local_ep = first.local_ep().unwrap();
        assert_ne!(client_local_ep.addr().port(), 0);
        assert_eq!(second.local_ep(), Some(client_local_ep));
        assert!(client_network.listener_infos().is_empty());

        client_network.close_all_listener().await.unwrap();
        let third = client_network
            .create_tunnel(
                &local_identity,
                &server_ep,
                &server_id,
                Some(server_name),
            )
            .await
            .unwrap();
        let third_accepted = accept_incoming_rx(&mut server_incoming).await;

        assert_eq!(third.local_ep(), Some(client_local_ep));
        assert!(client_network.listener_infos().is_empty());

        first.close().unwrap();
        second.close().unwrap();
        third.close().unwrap();
        first_accepted.close().unwrap();
        second_accepted.close().unwrap();
        third_accepted.close().unwrap();
    }

    #[tokio::test]
    async fn quic_tunnel_create_tunnel_no_listener_unreachable_returns_connect_error() {
        init_tls_once();

        let (network, _resolver) = new_network();
        let local_identity = new_identity("quic-no-listener-unreachable-client");

        let ret = network
            .create_tunnel(
                &local_identity,
                &Endpoint::from((Protocol::Quic, "127.0.0.1:44500".parse().unwrap())),
                &P2pId::default(),
                None,
            )
            .await;

        assert!(ret.is_err());
        assert_eq!(ret.err().unwrap().code(), P2pErrorCode::ConnectFailed);
    }

    #[tokio::test]
    async fn quic_tunnel_create_tunnel_with_local_ep_not_found() {
        let pair = setup_network_pair().await;
        let wrong_local_ep = Endpoint::from((Protocol::Quic, "127.0.0.1:45432".parse().unwrap()));

        let ret = pair
            .client_network
            .create_tunnel_with_local_ep(
                &pair.client_identity,
                &wrong_local_ep,
                &pair.server_local_ep,
                &pair.server_identity.get_id(),
                Some(pair.server_identity.get_name()),
            )
            .await;

        assert!(ret.is_err());
        assert_eq!(ret.err().unwrap().code(), P2pErrorCode::NotFound);
    }

    #[tokio::test]
    async fn quic_tunnel_protocol_udp_and_listener_infos_ok() {
        let pair = setup_network_pair().await;

        assert_eq!(pair.client_network.protocol(), Protocol::Quic);
        assert!(pair.client_network.is_udp());

        let infos = pair.client_network.listener_infos();
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].local, pair.client_local_ep);
        assert_eq!(infos[0].mapping_port, None);
    }

    #[tokio::test]
    async fn quic_tunnel_create_tunnel_with_local_ep_ok() {
        init_tls_once();

        let (client_network, client_resolver) = new_network();
        let (server_network, server_resolver) = new_network();
        let client_identity = new_identity("quic-local-ep-client");
        let server_identity = new_identity("quic-local-ep-server");

        register_listener_identity(&client_resolver, client_identity.clone()).await;
        register_listener_identity(&server_resolver, server_identity.clone()).await;

        client_network
            .listen(&loopback_quic_ep(), None, None, ignore_incoming())
            .await
            .unwrap();
        let (server_callback, mut server_incoming) = incoming_channel();
        server_network
            .listen(&loopback_quic_ep(), None, None, server_callback)
            .await
            .unwrap();

        let client_local_ep = client_network.listener_infos()[0].local;
        let server_local_ep = server_network.listener_infos()[0].local;
        let opened = client_network
            .create_tunnel_with_local_ep(
                &client_identity,
                &client_local_ep,
                &server_local_ep,
                &server_identity.get_id(),
                Some(server_identity.get_name()),
            )
            .await
            .unwrap();
        let accepted = accept_incoming_rx(&mut server_incoming).await;

        assert_eq!(opened.remote_id(), server_identity.get_id());
        assert_eq!(accepted.local_id(), server_identity.get_id());

        opened.close().unwrap();
        accepted.close().unwrap();
    }
}
