use super::connection::connect_with_optional_local;
use super::listener::{TcpTunnelListener, TcpTunnelRegistry};
use super::protocol::{
    ControlConnReady, ControlConnReadyResult, TcpConnectionHello, TcpConnectionRole, read_raw_frame,
};
use super::tunnel::{LocalTunnelPhase, TcpTunnel, TcpTunnelConnector};
use crate::endpoint::{Endpoint, Protocol};
use crate::error::{P2pErrorCode, P2pResult, p2p_err};
use crate::networks::{
    IncomingControlStream, IncomingDatagram, IncomingStream, IncomingTunnelAcceptance,
    IncomingTunnelAcceptanceCallback, IncomingTunnelCallback, Tunnel, TunnelConnectIntent,
    TunnelForm, TunnelListenerInfo, TunnelNetwork, TunnelRef,
};
use crate::p2p_identity::{P2pId, P2pIdentityCertFactoryRef, P2pIdentityRef};
use crate::runtime;
use crate::tls::ServerCertResolverRef;
use crate::types::{TunnelCandidateId, TunnelIdGenerator};
use sfo_reuseport::ServerRuntime;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub struct TcpTunnelNetwork {
    listeners: Mutex<Vec<Arc<TcpTunnelListener>>>,
    registry: Arc<TcpTunnelRegistry>,
    cert_resolver: ServerCertResolverRef,
    cert_factory: P2pIdentityCertFactoryRef,
    timeout: Duration,
    heartbeat_interval: Duration,
    heartbeat_timeout: Duration,
    tunnel_id_gen: Mutex<TunnelIdGenerator>,
    reuse_address: AtomicBool,
    server_runtime: ServerRuntime,
}

impl TcpTunnelNetwork {
    fn next_tunnel_id(&self, local_id: &P2pId, remote_id: &P2pId) -> crate::types::TunnelId {
        let creator_high_bit = if local_id.as_slice() > remote_id.as_slice() {
            1u32 << 31
        } else {
            0
        };
        loop {
            let seq = self.tunnel_id_gen.lock().unwrap().generate().value() & !(1u32 << 31);
            if seq != 0 {
                return crate::types::TunnelId::from(creator_high_bit | seq);
            }
        }
    }

    pub fn new(
        cert_resolver: ServerCertResolverRef,
        cert_factory: P2pIdentityCertFactoryRef,
        timeout: Duration,
        heartbeat_interval: Duration,
        heartbeat_timeout: Duration,
        server_runtime: ServerRuntime,
    ) -> Self {
        Self {
            listeners: Mutex::new(Vec::new()),
            registry: TcpTunnelRegistry::new(),
            cert_resolver,
            cert_factory,
            timeout,
            heartbeat_interval,
            heartbeat_timeout,
            tunnel_id_gen: Mutex::new(TunnelIdGenerator::new()),
            reuse_address: AtomicBool::new(false),
            server_runtime,
        }
    }

    async fn listen_with_incoming_acceptance(
        &self,
        local: &Endpoint,
        out: Option<Endpoint>,
        mapping_port: Option<u16>,
        on_incoming_tunnel: IncomingTunnelAcceptanceCallback,
    ) -> P2pResult<()> {
        let listener = TcpTunnelListener::new_with_acceptance(
            self.cert_resolver.clone(),
            self.cert_factory.clone(),
            self.registry.clone(),
            self.timeout,
            self.heartbeat_interval,
            self.heartbeat_timeout,
            self.server_runtime.clone(),
            on_incoming_tunnel,
        );
        listener
            .start(
                *local,
                out,
                mapping_port,
                self.reuse_address.load(Ordering::Relaxed),
            )
            .await?;
        self.listeners.lock().unwrap().push(listener.clone());
        Ok(())
    }

    async fn open_tunnel_inner(
        &self,
        local_identity: &P2pIdentityRef,
        local_ep: Option<&Endpoint>,
        remote: &Endpoint,
        remote_id: &P2pId,
        remote_name: Option<String>,
        intent: TunnelConnectIntent,
    ) -> P2pResult<TunnelRef> {
        let tunnel_id = if intent.tunnel_id == crate::types::TunnelId::default() {
            self.next_tunnel_id(&local_identity.get_id(), remote_id)
        } else {
            intent.tunnel_id
        };
        let candidate_id = if intent.candidate_id == TunnelCandidateId::default() {
            TunnelCandidateId::from(tunnel_id.value())
        } else {
            intent.candidate_id
        };
        let hello = TcpConnectionHello {
            role: TcpConnectionRole::Control,
            tunnel_id,
            candidate_id,
            is_reverse: intent.is_reverse,
            conn_id: None,
            open_request_id: None,
        };
        let mut conn = connect_with_optional_local(
            self.cert_factory.clone(),
            local_identity,
            local_ep,
            remote,
            remote_id,
            remote_name.clone(),
            self.timeout,
            self.reuse_address.load(Ordering::Relaxed),
            &hello,
        )
        .await?;

        let ready = runtime::timeout(
            self.timeout,
            read_raw_frame::<_, ControlConnReady>(&mut conn.stream),
        )
        .await
        .map_err(|_| p2p_err!(P2pErrorCode::Timeout, "wait control ready timeout"))??;
        if ready.tunnel_id != tunnel_id || ready.candidate_id != candidate_id {
            return Err(p2p_err!(
                P2pErrorCode::InvalidData,
                "control ready tunnel key mismatch"
            ));
        }
        if ready.result != ControlConnReadyResult::Success {
            return Err(p2p_err!(
                P2pErrorCode::Reject,
                "control ready failed: {:?}",
                ready.result
            ));
        }

        let connector = TcpTunnelConnector {
            cert_factory: self.cert_factory.clone(),
            local_identity: local_identity.clone(),
            local_ep: conn.local_ep,
            remote_ep: Arc::new(Mutex::new(*remote)),
            remote_id: conn.remote_id.clone(),
            remote_name: Some(conn.remote_name.clone()),
            timeout: self.timeout,
            tunnel_id,
            candidate_id,
        };
        let tunnel = TcpTunnel::new(
            conn,
            connector,
            tunnel_id,
            candidate_id,
            TunnelForm::Active,
            intent.is_reverse,
            self.heartbeat_interval,
            self.heartbeat_timeout,
            LocalTunnelPhase::Connected,
        );
        self.registry
            .register(tunnel.clone(), tunnel_id, candidate_id);
        Ok(tunnel)
    }
}

#[async_trait::async_trait]
impl TunnelNetwork for TcpTunnelNetwork {
    fn protocol(&self) -> Protocol {
        Protocol::Tcp
    }

    fn is_udp(&self) -> bool {
        false
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
        let callback: IncomingTunnelAcceptanceCallback = Arc::new(move |result| {
            let on_incoming_tunnel = on_incoming_tunnel.clone();
            Box::pin(async move {
                on_incoming_tunnel(result).await;
                IncomingTunnelAcceptance::Accepted
            })
        });
        self.listen_with_incoming_acceptance(local, out, mapping_port, callback)
            .await
    }

    async fn listen_with_acceptance(
        &self,
        local: &Endpoint,
        out: Option<Endpoint>,
        mapping_port: Option<u16>,
        on_incoming_tunnel: IncomingTunnelAcceptanceCallback,
    ) -> P2pResult<()> {
        self.listen_with_incoming_acceptance(local, out, mapping_port, on_incoming_tunnel)
            .await
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
        self.open_tunnel_inner(local_identity, None, remote, remote_id, remote_name, intent)
            .await
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
        self.open_tunnel_inner(
            local_identity,
            Some(local_ep),
            remote,
            remote_id,
            remote_name,
            intent,
        )
        .await
    }
}

#[cfg(test)]
mod construction_tests {
    use super::*;
    use crate::p2p_identity::{
        EncodedP2pIdentityCert, P2pIdentityCert, P2pIdentityCertFactory, P2pIdentityCertRef,
        P2pIdentitySignType, P2pSignature,
    };
    use crate::tls::DefaultTlsServerCertResolver;
    use sfo_reuseport::{ServerRuntime, ServerRuntimeConfig};
    use std::sync::Arc;

    struct TestCertFactory;

    impl P2pIdentityCertFactory for TestCertFactory {
        fn create(&self, cert: &EncodedP2pIdentityCert) -> P2pResult<P2pIdentityCertRef> {
            Ok(Arc::new(TestCert {
                id: P2pId::from(cert.clone()),
            }))
        }
    }

    struct TestCert {
        id: P2pId,
    }

    impl P2pIdentityCert for TestCert {
        fn get_id(&self) -> P2pId {
            self.id.clone()
        }

        fn get_name(&self) -> String {
            self.id.to_string()
        }

        fn sign_type(&self) -> P2pIdentitySignType {
            P2pIdentitySignType::Ed25519
        }

        fn verify(&self, _message: &[u8], _sign: &P2pSignature) -> bool {
            true
        }

        fn verify_cert(&self, _name: &str) -> bool {
            true
        }

        fn get_encoded_cert(&self) -> P2pResult<EncodedP2pIdentityCert> {
            Ok(self.id.as_slice().to_vec())
        }

        fn endpoints(&self) -> Vec<Endpoint> {
            Vec::new()
        }

        fn sn_list(&self) -> Vec<crate::p2p_identity::P2pSn> {
            Vec::new()
        }

        fn update_endpoints(&self, _eps: Vec<Endpoint>) -> P2pIdentityCertRef {
            Arc::new(TestCert {
                id: self.id.clone(),
            })
        }
    }

    fn network() -> TcpTunnelNetwork {
        TcpTunnelNetwork::new(
            DefaultTlsServerCertResolver::new(),
            Arc::new(TestCertFactory),
            Duration::from_secs(1),
            Duration::from_secs(5),
            Duration::from_secs(30),
            ServerRuntime::start(ServerRuntimeConfig::default())
                .expect("sfo reuseport server runtime should start"),
        )
    }

    #[tokio::test]
    async fn tcp_tunnel_network_exposes_tcp_protocol_and_listener_lifecycle_defaults() {
        let network = network();

        assert_eq!(network.protocol(), Protocol::Tcp);
        assert!(!network.is_udp());
        assert!(network.listener_infos().is_empty());

        network.set_reuse_address(true);
        network.close_all_listener().await.unwrap();
        assert!(network.listener_infos().is_empty());
    }

    #[tokio::test]
    async fn tcp_tunnel_network_generated_tunnel_id_sets_creator_high_bit() {
        let network = network();
        let low_local = P2pId::from(vec![1; 32]);
        let high_remote = P2pId::from(vec![2; 32]);
        let high_local = P2pId::from(vec![3; 32]);
        let low_remote = P2pId::from(vec![2; 32]);

        assert_eq!(
            network.next_tunnel_id(&low_local, &high_remote).value() & (1u32 << 31),
            0
        );
        assert_eq!(
            network.next_tunnel_id(&high_local, &low_remote).value() & (1u32 << 31),
            1u32 << 31
        );
    }
}

#[cfg(all(test, feature = "x509"))]
mod tests {
    use super::*;
    use crate::networks::{ListenVPortRegistry, TunnelPurpose, allow_all_listen_vports};
    use crate::runtime::{AsyncReadExt, AsyncWriteExt};
    use crate::tls::{DefaultTlsServerCertResolver, TlsServerCertResolver};
    use crate::x509::{X509IdentityCertFactory, X509IdentityFactory, generate_rsa_x509_identity};
    use sfo_reuseport::{ServerRuntime, ServerRuntimeConfig};
    use std::collections::HashMap;
    use std::sync::{Arc, LazyLock, Mutex as StdMutex, Once};
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

    struct TestNetworkPair {
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
        resolver
            .add_server_identity(identity.clone())
            .await
            .unwrap();
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

    async fn setup_network_pair() -> TestNetworkPair {
        init_tls_once();

        let (client_network, client_resolver) = new_network();
        let (server_network, server_resolver) = new_network();

        let client_identity = new_identity("tcp-client");
        let server_identity = new_identity("tcp-server");

        register_listener_identity(&client_resolver, client_identity.clone()).await;
        register_listener_identity(&server_resolver, server_identity.clone()).await;

        client_network
            .listen(&loopback_tcp_ep(), None, None, ignore_incoming())
            .await
            .unwrap();
        let (server_callback, server_incoming) = incoming_channel();
        server_network
            .listen(&loopback_tcp_ep(), None, None, server_callback)
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
                .create_tunnel(
                    &self.client_identity,
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
    async fn tcp_tunnel_stream_round_trip_ok() {
        let pair = setup_network_pair().await;
        let (opened, accepted) = pair.connect().await;

        assert_eq!(opened.protocol(), Protocol::Tcp);
        assert_eq!(accepted.protocol(), Protocol::Tcp);
        assert_eq!(opened.form(), TunnelForm::Active);
        assert_eq!(accepted.form(), TunnelForm::Passive);
        assert_eq!(opened.tunnel_id(), accepted.tunnel_id());
        assert_eq!(opened.candidate_id(), accepted.candidate_id());

        listen_stream_collect(&*accepted, allow_all_listen_vports()).await;

        let (opened_stream, accepted_stream) = tokio::join!(
            opened.open_stream(purpose_of(3101)),
            recv_stream(&*accepted)
        );
        let (mut read, mut write) = opened_stream.unwrap();
        let (purpose, mut peer_read, mut peer_write) = accepted_stream.unwrap();

        assert_eq!(purpose, purpose_of(3101));

        write.write_all(b"ping").await.unwrap();
        let mut buf = [0u8; 4];
        peer_read.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"ping");

        peer_write.write_all(b"pong").await.unwrap();
        let mut reply = [0u8; 4];
        read.read_exact(&mut reply).await.unwrap();
        assert_eq!(&reply, b"pong");

        write.shutdown().await.unwrap();
        peer_write.shutdown().await.unwrap();

        let mut active_tail = Vec::new();
        read.read_to_end(&mut active_tail).await.unwrap();
        assert!(active_tail.is_empty());
        let mut passive_tail = Vec::new();
        peer_read.read_to_end(&mut passive_tail).await.unwrap();
        assert!(passive_tail.is_empty());

        opened.close().unwrap();
        accepted.close().unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_can_open_multiple_datagram_channels_and_close_them() {
        let pair = setup_network_pair().await;
        let (opened, accepted) = pair.connect().await;

        listen_datagram_collect(&*accepted, allow_all_listen_vports()).await;
        let accepted_key = test_tunnel_key(&*accepted);
        let mut accepted_rx = TEST_DATAGRAM_RX
            .lock()
            .unwrap()
            .remove(&accepted_key)
            .expect("datagram receiver should be registered");

        let (first_write, first_accept) =
            tokio::join!(opened.open_datagram(purpose_of(3151)), accepted_rx.recv());
        let (second_write, second_accept) =
            tokio::join!(opened.open_datagram(purpose_of(3152)), accepted_rx.recv());

        let mut first_write = first_write.unwrap();
        let (first_purpose, mut first_read) = first_accept.unwrap().unwrap();
        let mut second_write = second_write.unwrap();
        let (second_purpose, mut second_read) = second_accept.unwrap().unwrap();

        assert_eq!(first_purpose, purpose_of(3151));
        assert_eq!(second_purpose, purpose_of(3152));

        first_write.write_all(b"first").await.unwrap();
        second_write.write_all(b"second").await.unwrap();
        first_write.shutdown().await.unwrap();
        second_write.shutdown().await.unwrap();

        let mut first_received = Vec::new();
        first_read.read_to_end(&mut first_received).await.unwrap();
        assert_eq!(first_received, b"first");

        let mut second_received = Vec::new();
        second_read.read_to_end(&mut second_received).await.unwrap();
        assert_eq!(second_received, b"second");

        opened.close().unwrap();
        accepted.close().unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_datagram_round_trip_ok() {
        let pair = setup_network_pair().await;
        let (opened, accepted) = pair.connect().await;

        listen_datagram_collect(&*accepted, allow_all_listen_vports()).await;

        let (opened_datagram, accepted_datagram) = tokio::join!(
            opened.open_datagram(purpose_of(3202)),
            recv_datagram(&*accepted)
        );
        let mut writer = opened_datagram.unwrap();
        let (purpose, mut peer_read) = accepted_datagram.unwrap();

        assert_eq!(purpose, purpose_of(3202));

        writer.write_all(b"hello").await.unwrap();
        writer.shutdown().await.unwrap();
        let mut buf = Vec::new();
        peer_read.read_to_end(&mut buf).await.unwrap();
        assert_eq!(buf, b"hello");

        opened.close().unwrap();
        accepted.close().unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_control_stream_round_trip_ok() {
        let pair = setup_network_pair().await;
        let (opened, accepted) = pair.connect().await;

        listen_control_stream_collect(&*accepted, allow_all_listen_vports()).await;

        let (opened_stream, accepted_stream) = tokio::join!(
            opened.open_control_stream(purpose_of(3252)),
            recv_control_stream(&*accepted)
        );
        let (mut read, mut write) = opened_stream.unwrap();
        let (purpose, mut peer_read, mut peer_write) = accepted_stream.unwrap();

        assert_eq!(purpose, purpose_of(3252));

        write.write_all(b"control-ping").await.unwrap();
        let mut buf = [0u8; 12];
        peer_read.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"control-ping");

        peer_write.write_all(b"control-pong").await.unwrap();
        let mut reply = [0u8; 12];
        read.read_exact(&mut reply).await.unwrap();
        assert_eq!(&reply, b"control-pong");

        write.shutdown().await.unwrap();
        peer_write.shutdown().await.unwrap();

        opened.close().unwrap();
        accepted.close().unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_open_stream_to_unlistened_port_returns_error() {
        let pair = setup_network_pair().await;
        let (opened, accepted) = pair.connect().await;
        let empty_vports = ListenVPortRegistry::<()>::new();

        listen_stream_collect(&*accepted, empty_vports.as_listen_vports_ref()).await;

        let err = opened.open_stream(purpose_of(3303)).await.err().unwrap();
        assert_eq!(err.code(), P2pErrorCode::PortNotListen);

        opened.close().unwrap();
        accepted.close().unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_open_without_listen_stays_pending_until_timeout() {
        let pair = setup_network_pair().await;
        let (opened, accepted) = pair.connect().await;

        let result = timeout(
            Duration::from_millis(200),
            opened.open_stream(purpose_of(3404)),
        )
        .await;
        assert!(result.is_err());

        opened.close().unwrap();
        accepted.close().unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_protocol_listener_infos_and_close_all_ok() {
        let pair = setup_network_pair().await;

        assert_eq!(pair.client_network.protocol(), Protocol::Tcp);
        assert!(!pair.client_network.is_udp());

        let infos = pair.client_network.listener_infos();
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].local, pair.client_local_ep);
        assert_eq!(infos[0].mapping_port, None);

        pair.client_network.close_all_listener().await.unwrap();
        assert!(pair.client_network.listener_infos().is_empty());
    }

    #[tokio::test]
    async fn tcp_tunnel_create_tunnel_to_unreachable_remote_returns_connect_failed() {
        init_tls_once();

        let (network, _resolver) = new_network();
        let identity = new_identity("tcp-unreachable-client");
        let unreachable = Endpoint::from((Protocol::Tcp, "127.0.0.1:9".parse().unwrap()));

        let ret = network
            .create_tunnel(&identity, &unreachable, &P2pId::default(), None)
            .await;

        assert!(ret.is_err());
        assert_eq!(ret.err().unwrap().code(), P2pErrorCode::ConnectFailed);
    }
}
