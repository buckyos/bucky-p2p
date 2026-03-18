use super::connection::connect_with_optional_local;
use super::listener::{TcpTunnelListener, TcpTunnelRegistry};
use super::protocol::{
    ControlConnReady, ControlConnReadyResult, TcpConnectionHello, TcpConnectionRole, read_raw_frame,
};
use super::tunnel::{LocalTunnelPhase, TcpTunnel, TcpTunnelConnector};
use crate::endpoint::{Endpoint, Protocol};
use crate::error::{P2pErrorCode, P2pResult, p2p_err};
use crate::networks::{
    Tunnel, TunnelConnectIntent, TunnelForm, TunnelListenerInfo, TunnelListenerRef, TunnelNetwork,
    TunnelRef,
};
use crate::p2p_identity::{P2pId, P2pIdentityCertFactoryRef, P2pIdentityRef};
use crate::runtime;
use crate::tls::ServerCertResolverRef;
use crate::types::{TunnelCandidateId, TunnelIdGenerator};
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
        }
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
    ) -> P2pResult<TunnelListenerRef> {
        let listener = TcpTunnelListener::new(
            self.cert_resolver.clone(),
            self.cert_factory.clone(),
            self.registry.clone(),
            self.timeout,
            self.heartbeat_interval,
            self.heartbeat_timeout,
        );
        listener
            .bind(
                *local,
                out,
                mapping_port,
                self.reuse_address.load(Ordering::Relaxed),
            )
            .await?;
        listener.start();
        self.listeners.lock().unwrap().push(listener.clone());
        Ok(listener)
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

    fn listeners(&self) -> Vec<TunnelListenerRef> {
        self.listeners
            .lock()
            .unwrap()
            .iter()
            .map(|v| v.clone() as TunnelListenerRef)
            .collect()
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

#[cfg(all(test, feature = "x509"))]
mod tests {
    use super::*;
    use crate::networks::{
        ListenVPortRegistry, Tunnel, TunnelListener, TunnelPurpose, allow_all_listen_vports,
    };
    use crate::runtime::{AsyncReadExt, AsyncWriteExt};
    use crate::tls::{DefaultTlsServerCertResolver, TlsServerCertResolver};
    use crate::x509::{
        X509IdentityCertFactory, X509IdentityFactory, generate_ed25519_x509_identity,
        generate_rsa_x509_identity,
    };
    use std::sync::Arc;
    use std::sync::Once;
    use tokio::time::{Instant, sleep, timeout};

    static TLS_INIT: Once = Once::new();

    fn init_tls_once() {
        TLS_INIT.call_once(|| {
            crate::executor::Executor::init();
            crate::tls::init_tls(Arc::new(X509IdentityFactory));
        });
    }

    fn new_identity(name: &str) -> P2pIdentityRef {
        Arc::new(generate_rsa_x509_identity(Some(name.to_owned())).unwrap())
    }

    fn new_ed25519_identity(name: &str) -> P2pIdentityRef {
        Arc::new(generate_ed25519_x509_identity(Some(name.to_owned())).unwrap())
    }

    async fn register_listener_identity(
        resolver: &Arc<DefaultTlsServerCertResolver>,
        identity: P2pIdentityRef,
    ) {
        resolver.add_server_identity(identity).await.unwrap();
    }

    fn loopback_tcp_ep() -> Endpoint {
        Endpoint::from((Protocol::Tcp, "127.0.0.1:0".parse().unwrap()))
    }

    fn purpose_of(vport: u16) -> TunnelPurpose {
        TunnelPurpose::from_value(&vport).unwrap()
    }

    fn tcp_id(value: u32) -> crate::types::TunnelId {
        crate::types::TunnelId::from(value)
    }

    fn claim_req(
        channel_id: u32,
        kind: super::super::protocol::TcpChannelKind,
        purpose_vport: u16,
        conn_id: super::super::protocol::TcpConnId,
        lease_seq: super::super::protocol::TcpLeaseSeq,
        claim_nonce: u64,
    ) -> super::super::protocol::ClaimConnReq {
        super::super::protocol::ClaimConnReq {
            channel_id: tcp_id(channel_id),
            kind,
            purpose: purpose_of(purpose_vport),
            conn_id,
            lease_seq,
            claim_nonce,
        }
    }

    fn new_network() -> (TcpTunnelNetwork, Arc<DefaultTlsServerCertResolver>) {
        new_network_full(
            Duration::from_secs(3),
            Duration::from_millis(200),
            Duration::from_secs(5),
        )
    }

    fn new_network_with_timeout(
        timeout: Duration,
    ) -> (TcpTunnelNetwork, Arc<DefaultTlsServerCertResolver>) {
        new_network_full(timeout, Duration::from_millis(200), Duration::from_secs(5))
    }

    fn new_network_full(
        timeout: Duration,
        heartbeat_interval: Duration,
        heartbeat_timeout: Duration,
    ) -> (TcpTunnelNetwork, Arc<DefaultTlsServerCertResolver>) {
        let resolver = DefaultTlsServerCertResolver::new();
        let cert_factory: P2pIdentityCertFactoryRef = Arc::new(X509IdentityCertFactory);
        (
            TcpTunnelNetwork::new(
                resolver.clone(),
                cert_factory,
                timeout,
                heartbeat_interval,
                heartbeat_timeout,
            ),
            resolver,
        )
    }

    async fn setup_pair() -> (Arc<TcpTunnel>, Arc<TcpTunnel>) {
        setup_pair_full(
            Duration::from_secs(3),
            Duration::from_millis(200),
            Duration::from_secs(5),
            true,
        )
        .await
    }

    async fn setup_pair_with_timeout(timeout: Duration) -> (Arc<TcpTunnel>, Arc<TcpTunnel>) {
        setup_pair_full(
            timeout,
            Duration::from_millis(200),
            Duration::from_secs(5),
            true,
        )
        .await
    }

    async fn setup_pair_without_listen() -> (Arc<TcpTunnel>, Arc<TcpTunnel>) {
        setup_pair_full(
            Duration::from_secs(3),
            Duration::from_millis(200),
            Duration::from_secs(5),
            false,
        )
        .await
    }

    async fn setup_pair_with_timeout_without_listen(
        timeout: Duration,
    ) -> (Arc<TcpTunnel>, Arc<TcpTunnel>) {
        setup_pair_full(
            timeout,
            Duration::from_millis(200),
            Duration::from_secs(5),
            false,
        )
        .await
    }

    async fn setup_pair_full(
        timeout: Duration,
        heartbeat_interval: Duration,
        heartbeat_timeout: Duration,
        preload_listen: bool,
    ) -> (Arc<TcpTunnel>, Arc<TcpTunnel>) {
        init_tls_once();

        let (client_network, client_resolver) =
            new_network_full(timeout, heartbeat_interval, heartbeat_timeout);
        let (server_network, server_resolver) =
            new_network_full(timeout, heartbeat_interval, heartbeat_timeout);

        let client_identity = new_identity("client");
        let server_identity = new_identity("server");

        register_listener_identity(&client_resolver, client_identity.clone()).await;
        register_listener_identity(&server_resolver, server_identity.clone()).await;

        client_network
            .listen(&loopback_tcp_ep(), None, None)
            .await
            .unwrap();
        server_network
            .listen(&loopback_tcp_ep(), None, None)
            .await
            .unwrap();

        let client_listener = client_network.listeners.lock().unwrap()[0].clone();
        let server_listener = server_network.listeners.lock().unwrap()[0].clone();
        let client_bound_local = client_listener.bound_local();
        let server_bound_local = server_listener.bound_local();

        let remote_ep = server_bound_local;
        let accept_task =
            tokio::spawn(async move { server_listener.accept_tunnel().await.unwrap() });
        let _ = client_network
            .create_tunnel(
                &client_identity,
                &remote_ep,
                &server_identity.get_id(),
                Some(server_identity.get_name()),
            )
            .await
            .unwrap();
        let _accepted = accept_task.await.unwrap();

        let mut active_tunnels = client_network
            .registry
            .find_tunnels_for_test(&client_identity.get_id(), &server_identity.get_id());
        let mut reverse_tunnels = server_network
            .registry
            .find_tunnels_for_test(&server_identity.get_id(), &client_identity.get_id());
        assert_eq!(active_tunnels.len(), 1);
        assert_eq!(reverse_tunnels.len(), 1);

        let active = active_tunnels.pop().unwrap();
        let reverse = reverse_tunnels.pop().unwrap();

        active.set_data_remote_ep_for_test(server_bound_local);
        reverse.set_data_remote_ep_for_test(client_bound_local);

        if preload_listen {
            active
                .listen_stream(allow_all_listen_vports())
                .await
                .unwrap();
            active
                .listen_datagram(allow_all_listen_vports())
                .await
                .unwrap();
            reverse
                .listen_stream(allow_all_listen_vports())
                .await
                .unwrap();
            reverse
                .listen_datagram(allow_all_listen_vports())
                .await
                .unwrap();
        }

        (active, reverse)
    }

    async fn wait_for_idle_pool_len(tunnel: &Arc<TcpTunnel>, expected: usize) {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let current = tunnel.idle_pool_len_for_test();
            if current == expected {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for idle pool sync: expected {}, got {}, states {:?}",
                expected,
                current,
                tunnel.data_conn_states_for_test(),
            );
            sleep(Duration::from_millis(10)).await;
        }
    }

    async fn wait_for_data_conn_len(tunnel: &Arc<TcpTunnel>, expected: usize) {
        let deadline = Instant::now() + Duration::from_secs(4);
        loop {
            let current = tunnel.data_conn_count_for_test();
            if current == expected {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for data conn sync: expected {}, got {}, states {:?}",
                expected,
                current,
                tunnel.data_conn_states_for_test(),
            );
            sleep(Duration::from_millis(10)).await;
        }
    }

    async fn wait_for_registry_tunnels(
        registry: &Arc<TcpTunnelRegistry>,
        local_id: &crate::p2p_identity::P2pId,
        remote_id: &crate::p2p_identity::P2pId,
        expected: usize,
    ) -> Vec<Arc<TcpTunnel>> {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let tunnels = registry.find_tunnels_for_test(local_id, remote_id);
            if tunnels.len() == expected {
                return tunnels;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for tunnel registry sync: expected {}, got {}",
                expected,
                tunnels.len(),
            );
            sleep(Duration::from_millis(10)).await;
        }
    }

    async fn wait_for_tunnel_state(
        tunnel: &Arc<TcpTunnel>,
        expected: crate::networks::TunnelState,
    ) {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let current = tunnel.state();
            if current == expected {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for tunnel state: expected {:?}, got {:?}",
                expected,
                current,
            );
            sleep(Duration::from_millis(10)).await;
        }
    }

    async fn wait_for_pending_drain_len(tunnel: &Arc<TcpTunnel>, expected: usize) {
        let deadline = Instant::now() + Duration::from_secs(4);
        loop {
            let current = tunnel.pending_drain_count_for_test();
            if current == expected {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for pending drain sync: expected {}, got {}",
                expected,
                current,
            );
            sleep(Duration::from_millis(10)).await;
        }
    }

    async fn wait_for_conn_state_fragments(tunnel: &Arc<TcpTunnel>, fragments: &[&str]) -> String {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let states = tunnel.data_conn_states_for_test();
            if let Some(state) = states
                .iter()
                .find(|state| fragments.iter().all(|fragment| state.contains(fragment)))
            {
                return state.clone();
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for conn state fragments {:?}, states {:?}",
                fragments,
                states,
            );
            sleep(Duration::from_millis(10)).await;
        }
    }

    async fn wait_for_claiming(
        tunnel: &Arc<TcpTunnel>,
        conn_id: super::super::protocol::TcpConnId,
    ) -> (
        super::super::protocol::TcpChannelId,
        super::super::protocol::TcpLeaseSeq,
        u64,
        super::super::protocol::TcpChannelKind,
        TunnelPurpose,
    ) {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Some(claiming) = tunnel.current_claiming_for_test(conn_id) {
                return claiming;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for claiming state, states {:?}",
                tunnel.data_conn_states_for_test(),
            );
            sleep(Duration::from_millis(10)).await;
        }
    }

    async fn seed_idle_stream(active: &Arc<TcpTunnel>, reverse: &Arc<TcpTunnel>) {
        active
            .listen_stream(allow_all_listen_vports())
            .await
            .unwrap();
        reverse
            .listen_stream(allow_all_listen_vports())
            .await
            .unwrap();
        let (opened, accepted) = tokio::join!(
            active.open_stream(purpose_of(3990)),
            reverse.accept_stream()
        );
        let (read, mut write) = opened.unwrap();
        let (_vport, mut peer_read, peer_write) = accepted.unwrap();

        write.write_all(b"seed").await.unwrap();
        let mut buf = [0u8; 4];
        peer_read.read_exact(&mut buf).await.unwrap();

        drop(write);
        drop(read);
        drop(peer_read);
        drop(peer_write);

        wait_for_idle_pool_len(active, 1).await;
        wait_for_idle_pool_len(reverse, 1).await;
    }

    #[tokio::test]
    async fn tcp_tunnel_mixed_rsa_and_ed25519_handshake_ok() {
        init_tls_once();

        let (client_network, client_resolver) = new_network();
        let (server_network, server_resolver) = new_network();
        let client_identity = new_ed25519_identity("client-ed25519");
        let server_identity = new_identity("server-rsa");

        register_listener_identity(&client_resolver, client_identity.clone()).await;
        register_listener_identity(&server_resolver, server_identity.clone()).await;

        client_network
            .listen(&loopback_tcp_ep(), None, None)
            .await
            .unwrap();
        server_network
            .listen(&loopback_tcp_ep(), None, None)
            .await
            .unwrap();

        let server_listener = server_network.listeners.lock().unwrap()[0].clone();
        let server_ep = server_listener.bound_local();

        let accept_task =
            tokio::spawn(async move { server_listener.accept_tunnel().await.unwrap() });
        let opened = client_network
            .create_tunnel(
                &client_identity,
                &server_ep,
                &server_identity.get_id(),
                Some(server_identity.get_name()),
            )
            .await
            .unwrap();
        let accepted = accept_task.await.unwrap();

        assert_eq!(opened.local_id(), client_identity.get_id());
        assert_eq!(opened.remote_id(), server_identity.get_id());
        assert_eq!(accepted.local_id(), server_identity.get_id());
        assert_eq!(accepted.remote_id(), client_identity.get_id());

        opened.close().await.unwrap();
        accepted.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_listener_accepts_multiple_local_identities_on_one_port() {
        init_tls_once();

        let (client_network, client_resolver) = new_network();
        let (server_network, server_resolver) = new_network();
        let client_identity = new_identity("client-multi-identity");
        let server_identity_a = new_identity("server-multi-identity-a");
        let server_identity_b = new_identity("server-multi-identity-b");

        register_listener_identity(&client_resolver, client_identity.clone()).await;
        register_listener_identity(&server_resolver, server_identity_a.clone()).await;
        register_listener_identity(&server_resolver, server_identity_b.clone()).await;

        client_network
            .listen(&loopback_tcp_ep(), None, None)
            .await
            .unwrap();
        server_network
            .listen(&loopback_tcp_ep(), None, None)
            .await
            .unwrap();

        let server_listener = server_network.listeners.lock().unwrap()[0].clone();
        let server_ep = server_listener.bound_local();

        let accept_a = tokio::spawn({
            let server_listener = server_listener.clone();
            async move { server_listener.accept_tunnel().await.unwrap() }
        });
        let opened_a = client_network
            .create_tunnel(
                &client_identity,
                &server_ep,
                &server_identity_a.get_id(),
                Some(server_identity_a.get_name()),
            )
            .await
            .unwrap();
        let accepted_a = accept_a.await.unwrap();
        assert_eq!(accepted_a.local_id(), server_identity_a.get_id());

        let accept_b = tokio::spawn(async move { server_listener.accept_tunnel().await.unwrap() });
        let opened_b = client_network
            .create_tunnel(
                &client_identity,
                &server_ep,
                &server_identity_b.get_id(),
                Some(server_identity_b.get_name()),
            )
            .await
            .unwrap();
        let accepted_b = accept_b.await.unwrap();
        assert_eq!(accepted_b.local_id(), server_identity_b.get_id());

        opened_a.close().await.unwrap();
        accepted_a.close().await.unwrap();
        opened_b.close().await.unwrap();
        accepted_b.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_stream_returns_connection_after_drain() {
        let (active, reverse) = setup_pair().await;

        let (opened, accepted) = tokio::join!(
            active.open_stream(purpose_of(1001)),
            reverse.accept_stream()
        );
        let (read, mut write) = opened.unwrap();
        let (_vport, mut peer_read, peer_write) = accepted.unwrap();

        write.write_all(b"ping").await.unwrap();
        let mut buf = [0u8; 4];
        peer_read.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"ping");

        assert_eq!(active.idle_pool_len_for_test(), 0);
        assert_eq!(reverse.idle_pool_len_for_test(), 0);

        drop(write);
        drop(read);
        drop(peer_read);
        drop(peer_write);

        wait_for_idle_pool_len(&active, 1).await;
        wait_for_idle_pool_len(&reverse, 1).await;
    }

    #[tokio::test]
    async fn tcp_tunnel_datagram_returns_connection_after_drain() {
        let (active, reverse) = setup_pair().await;

        let (opened, accepted) = timeout(Duration::from_secs(2), async {
            tokio::join!(
                active.open_datagram(purpose_of(2002)),
                reverse.accept_datagram()
            )
        })
        .await
        .unwrap();
        let mut writer = opened.unwrap();
        let (_vport, mut peer_read) = accepted.unwrap();

        writer.write_all(b"hello").await.unwrap();
        let mut buf = [0u8; 5];
        timeout(Duration::from_secs(2), peer_read.read_exact(&mut buf))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&buf, b"hello");

        drop(writer);
        drop(peer_read);

        wait_for_idle_pool_len(&active, 1).await;
        wait_for_idle_pool_len(&reverse, 1).await;
    }

    #[tokio::test]
    async fn tcp_tunnel_open_data_conn_req_creates_reverse_first_claim_connection() {
        let (active, reverse) = setup_pair().await;

        active
            .request_reverse_data_connection_for_test()
            .await
            .unwrap();

        let (opened, accepted) = tokio::join!(
            reverse.open_stream(purpose_of(2100)),
            active.accept_stream()
        );
        let (read, mut write) = opened.unwrap();
        let (_vport, mut peer_read, peer_write) = accepted.unwrap();

        write.write_all(b"back").await.unwrap();
        let mut buf = [0u8; 4];
        peer_read.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"back");

        drop(write);
        drop(read);
        drop(peer_read);
        drop(peer_write);

        wait_for_idle_pool_len(&active, 1).await;
        wait_for_idle_pool_len(&reverse, 1).await;
    }

    #[tokio::test]
    async fn tcp_tunnel_open_data_conn_req_timeout_allows_late_arrival_reuse() {
        let (active, reverse) = setup_pair_with_timeout(Duration::from_millis(100)).await;
        reverse.set_reverse_open_delay_for_test(Duration::from_millis(250));

        let err = active.request_reverse_data_connection_for_test().await;
        let err = match err {
            Ok(_) => panic!("control ready mismatch should fail"),
            Err(err) => err,
        };
        assert_eq!(err.code(), P2pErrorCode::Timeout);

        sleep(Duration::from_millis(400)).await;
        assert_eq!(active.data_conn_count_for_test(), 1);
        assert_eq!(reverse.data_conn_count_for_test(), 1);

        let (opened, accepted) = tokio::join!(
            reverse.open_stream(purpose_of(2200)),
            active.accept_stream()
        );
        let (read, mut write) = opened.unwrap();
        let (_vport, mut peer_read, peer_write) = accepted.unwrap();

        write.write_all(b"late").await.unwrap();
        let mut buf = [0u8; 4];
        peer_read.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"late");

        drop(write);
        drop(read);
        drop(peer_read);
        drop(peer_write);

        wait_for_idle_pool_len(&active, 1).await;
        wait_for_idle_pool_len(&reverse, 1).await;
    }

    #[tokio::test]
    async fn tcp_tunnel_open_data_conn_req_timeout_rejects_late_arrival_over_budget() {
        let (active, reverse) = setup_pair_with_timeout(Duration::from_millis(100)).await;
        reverse.set_reverse_open_delay_for_test(Duration::from_millis(250));
        active.set_unclaimed_data_conn_budget_for_test(0);

        let err = active
            .request_reverse_data_connection_for_test()
            .await
            .unwrap_err();
        assert_eq!(err.code(), P2pErrorCode::Timeout);

        sleep(Duration::from_millis(400)).await;
        assert_eq!(active.data_conn_count_for_test(), 0);
        assert_eq!(reverse.data_conn_count_for_test(), 0);
    }

    #[tokio::test]
    async fn tcp_tunnel_unknown_tunnel_data_connection_returns_tunnel_not_found() {
        init_tls_once();

        let (client_network, client_resolver) = new_network();
        let (server_network, server_resolver) = new_network();

        let client_identity = new_identity("client-unknown-data");
        let server_identity = new_identity("server-unknown-data");

        register_listener_identity(&client_resolver, client_identity.clone()).await;
        register_listener_identity(&server_resolver, server_identity.clone()).await;

        client_network
            .listen(&loopback_tcp_ep(), None, None)
            .await
            .unwrap();
        server_network
            .listen(&loopback_tcp_ep(), None, None)
            .await
            .unwrap();

        let server_listener = server_network.listeners.lock().unwrap()[0].clone();
        let server_ep = server_listener.bound_local();
        let cert_factory: P2pIdentityCertFactoryRef = Arc::new(X509IdentityCertFactory);
        let hello = super::super::protocol::TcpConnectionHello {
            role: super::super::protocol::TcpConnectionRole::Data,
            tunnel_id: crate::types::TunnelId::from(0x1234),
            candidate_id: crate::types::TunnelCandidateId::from(1),
            is_reverse: false,
            conn_id: Some(tcp_id(0x5678)),
            open_request_id: None,
        };

        let mut connection = connect_with_optional_local(
            cert_factory,
            &client_identity,
            None,
            &server_ep,
            &server_identity.get_id(),
            Some(server_identity.get_name()),
            Duration::from_secs(3),
            false,
            &hello,
        )
        .await
        .unwrap();

        let ready =
            read_raw_frame::<_, super::super::protocol::DataConnReady>(&mut connection.stream)
                .await
                .unwrap();
        assert_eq!(ready.conn_id, tcp_id(0x5678));
        assert_eq!(
            ready.result,
            super::super::protocol::DataConnReadyResult::TunnelNotFound
        );
    }

    #[tokio::test]
    async fn tcp_tunnel_invalid_control_hello_returns_protocol_error() {
        init_tls_once();

        let (server_network, server_resolver) = new_network();
        let client_identity = new_identity("client-invalid-control");
        let server_identity = new_identity("server-invalid-control");

        register_listener_identity(&server_resolver, server_identity.clone()).await;

        server_network
            .listen(&loopback_tcp_ep(), None, None)
            .await
            .unwrap();

        let server_listener = server_network.listeners.lock().unwrap()[0].clone();
        let server_ep = server_listener.bound_local();
        let cert_factory: P2pIdentityCertFactoryRef = Arc::new(X509IdentityCertFactory);
        let tunnel_id = crate::types::TunnelId::from(0x1234);
        let hello = super::super::protocol::TcpConnectionHello {
            role: super::super::protocol::TcpConnectionRole::Control,
            tunnel_id,
            candidate_id: crate::types::TunnelCandidateId::from(2),
            is_reverse: false,
            conn_id: Some(tcp_id(1)),
            open_request_id: None,
        };

        let mut connection = connect_with_optional_local(
            cert_factory,
            &client_identity,
            None,
            &server_ep,
            &server_identity.get_id(),
            Some(server_identity.get_name()),
            Duration::from_secs(3),
            false,
            &hello,
        )
        .await
        .unwrap();

        let ready =
            read_raw_frame::<_, super::super::protocol::ControlConnReady>(&mut connection.stream)
                .await
                .unwrap();
        assert_eq!(ready.tunnel_id, tunnel_id);
        assert_eq!(
            ready.result,
            super::super::protocol::ControlConnReadyResult::ProtocolError
        );

        sleep(Duration::from_millis(50)).await;
        assert!(
            server_network
                .registry
                .find_tunnels_for_test(&server_identity.get_id(), &client_identity.get_id())
                .is_empty()
        );
    }

    #[tokio::test]
    async fn tcp_tunnel_incoming_control_conflict_lost_returns_reject() {
        init_tls_once();

        let (client_network, _client_resolver) = new_network();
        let (server_network, server_resolver) = new_network();
        let client_identity = new_identity("client-control-conflict-lost");
        let server_identity = new_identity("server-control-conflict-lost");

        register_listener_identity(&server_resolver, server_identity.clone()).await;

        server_network
            .listen(&loopback_tcp_ep(), None, None)
            .await
            .unwrap();

        let server_listener = server_network.listeners.lock().unwrap()[0].clone();
        server_listener.set_incoming_control_decision_for_test(Some(
            super::super::tunnel::TcpIncomingControlDecision::TunnelConflictLost,
        ));
        let server_ep = server_listener.bound_local();

        let err = match client_network
            .create_tunnel(
                &client_identity,
                &server_ep,
                &server_identity.get_id(),
                Some(server_identity.get_name()),
            )
            .await
        {
            Ok(_) => panic!("incoming control conflict lost should fail"),
            Err(err) => err,
        };
        assert_eq!(err.code(), P2pErrorCode::Reject);
        assert!(
            server_network
                .registry
                .find_tunnels_for_test(&server_identity.get_id(), &client_identity.get_id())
                .is_empty()
        );
    }

    #[tokio::test]
    async fn tcp_tunnel_passive_ready_waits_for_active_followup_before_connected() {
        init_tls_once();

        let (server_network, server_resolver) = new_network();
        let client_identity = new_identity("client-passive-ready");
        let server_identity = new_identity("server-passive-ready");

        register_listener_identity(&server_resolver, server_identity.clone()).await;

        server_network
            .listen(&loopback_tcp_ep(), None, None)
            .await
            .unwrap();

        let server_listener = server_network.listeners.lock().unwrap()[0].clone();
        let server_ep = server_listener.bound_local();
        let cert_factory: P2pIdentityCertFactoryRef = Arc::new(X509IdentityCertFactory);
        let tunnel_id = crate::types::TunnelId::from(0x2345);
        let hello = super::super::protocol::TcpConnectionHello {
            role: super::super::protocol::TcpConnectionRole::Control,
            tunnel_id,
            candidate_id: crate::types::TunnelCandidateId::from(3),
            is_reverse: false,
            conn_id: None,
            open_request_id: None,
        };

        let mut connection = connect_with_optional_local(
            cert_factory,
            &client_identity,
            None,
            &server_ep,
            &server_identity.get_id(),
            Some(server_identity.get_name()),
            Duration::from_secs(3),
            false,
            &hello,
        )
        .await
        .unwrap();

        let ready =
            read_raw_frame::<_, super::super::protocol::ControlConnReady>(&mut connection.stream)
                .await
                .unwrap();
        assert_eq!(
            ready.result,
            super::super::protocol::ControlConnReadyResult::Success
        );

        let reverse = wait_for_registry_tunnels(
            &server_network.registry,
            &server_identity.get_id(),
            &client_identity.get_id(),
            1,
        )
        .await
        .pop()
        .unwrap();

        assert_eq!(reverse.state(), crate::networks::TunnelState::Connecting);
        sleep(Duration::from_millis(100)).await;
        assert_eq!(reverse.state(), crate::networks::TunnelState::Connecting);

        super::super::protocol::write_raw_frame(
            &mut connection.stream,
            &super::super::protocol::TcpControlCmd::Ping(super::super::protocol::PingCmd {
                seq: 1,
                send_time: 1,
            }),
        )
        .await
        .unwrap();

        let pong =
            read_raw_frame::<_, super::super::protocol::TcpControlCmd>(&mut connection.stream)
                .await
                .unwrap();
        assert!(matches!(
            pong,
            super::super::protocol::TcpControlCmd::Pong(super::super::protocol::PongCmd {
                seq: 1,
                send_time: 1,
            })
        ));

        wait_for_tunnel_state(&reverse, crate::networks::TunnelState::Connected).await;
        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_control_ready_tunnel_id_mismatch_fails() {
        init_tls_once();

        let (client_network, _client_resolver) = new_network();
        let client_identity = new_identity("client-control-ready-mismatch");
        let server_identity = new_identity("server-control-ready-mismatch");
        let cert_factory: P2pIdentityCertFactoryRef = Arc::new(X509IdentityCertFactory);
        let resolver: crate::tls::ServerCertResolverRef = DefaultTlsServerCertResolver::new();
        resolver
            .add_server_identity(server_identity.clone())
            .await
            .unwrap();
        let acceptor =
            super::super::connection::build_acceptor(resolver.clone(), cert_factory.clone());
        let listener = super::super::connection::bind_listener(loopback_tcp_ep(), false)
            .await
            .unwrap();
        let server_ep = Endpoint::from((Protocol::Tcp, listener.local_addr().unwrap()));

        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let (mut connection, hello) = super::super::connection::accept_connection(
                &acceptor,
                &cert_factory,
                &resolver,
                socket,
            )
            .await
            .unwrap();
            let mismatch = if hello.tunnel_id.value() == u32::MAX {
                crate::types::TunnelId::from(1)
            } else {
                crate::types::TunnelId::from(hello.tunnel_id.value() + 1)
            };
            super::super::protocol::write_raw_frame(
                &mut connection.stream,
                &super::super::protocol::ControlConnReady {
                    tunnel_id: mismatch,
                    candidate_id: hello.candidate_id,
                    result: super::super::protocol::ControlConnReadyResult::Success,
                },
            )
            .await
            .unwrap();
        });

        let err = client_network
            .create_tunnel(
                &client_identity,
                &server_ep,
                &server_identity.get_id(),
                Some(server_identity.get_name()),
            )
            .await;
        let err = match err {
            Ok(_) => panic!("control ready mismatch should fail"),
            Err(err) => err,
        };
        assert_eq!(err.code(), P2pErrorCode::InvalidData);

        server.await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_control_ready_timeout_does_not_reuse_tunnel_id() {
        init_tls_once();

        let (client_network, _client_resolver) =
            new_network_with_timeout(Duration::from_millis(100));
        let client_identity = new_identity("client-control-timeout");
        let server_identity = new_identity("server-control-timeout");
        let cert_factory: P2pIdentityCertFactoryRef = Arc::new(X509IdentityCertFactory);
        let resolver: crate::tls::ServerCertResolverRef = DefaultTlsServerCertResolver::new();
        resolver
            .add_server_identity(server_identity.clone())
            .await
            .unwrap();
        let acceptor =
            super::super::connection::build_acceptor(resolver.clone(), cert_factory.clone());
        let listener = super::super::connection::bind_listener(loopback_tcp_ep(), false)
            .await
            .unwrap();
        let server_ep = Endpoint::from((Protocol::Tcp, listener.local_addr().unwrap()));

        let server = tokio::spawn(async move {
            let mut tunnel_ids = Vec::new();
            for index in 0..2 {
                let (socket, _) = listener.accept().await.unwrap();
                let (mut connection, hello) = super::super::connection::accept_connection(
                    &acceptor,
                    &cert_factory,
                    &resolver,
                    socket,
                )
                .await
                .unwrap();
                tunnel_ids.push(hello.tunnel_id.value());
                if index == 0 {
                    sleep(Duration::from_millis(250)).await;
                } else {
                    super::super::protocol::write_raw_frame(
                        &mut connection.stream,
                        &super::super::protocol::ControlConnReady {
                            tunnel_id: hello.tunnel_id,
                            candidate_id: hello.candidate_id,
                            result: super::super::protocol::ControlConnReadyResult::Success,
                        },
                    )
                    .await
                    .unwrap();
                }
            }
            tunnel_ids
        });

        let err = client_network
            .create_tunnel(
                &client_identity,
                &server_ep,
                &server_identity.get_id(),
                Some(server_identity.get_name()),
            )
            .await;
        let err = match err {
            Ok(_) => panic!("control ready timeout should fail"),
            Err(err) => err,
        };
        assert_eq!(err.code(), P2pErrorCode::Timeout);

        sleep(Duration::from_millis(300)).await;

        let tunnel = client_network
            .create_tunnel(
                &client_identity,
                &server_ep,
                &server_identity.get_id(),
                Some(server_identity.get_name()),
            )
            .await
            .unwrap();

        let tunnel_ids = server.await.unwrap();
        assert_eq!(tunnel_ids.len(), 2);
        assert_ne!(tunnel_ids[0], tunnel_ids[1]);

        let _ = tunnel.close().await;
    }

    #[tokio::test]
    async fn tcp_tunnel_active_create_waits_for_control_ready_before_returning() {
        init_tls_once();

        let (client_network, _client_resolver) = new_network();
        let client_identity = new_identity("client-control-ready-gate");
        let server_identity = new_identity("server-control-ready-gate");
        let cert_factory: P2pIdentityCertFactoryRef = Arc::new(X509IdentityCertFactory);
        let resolver: crate::tls::ServerCertResolverRef = DefaultTlsServerCertResolver::new();
        resolver
            .add_server_identity(server_identity.clone())
            .await
            .unwrap();
        let acceptor =
            super::super::connection::build_acceptor(resolver.clone(), cert_factory.clone());
        let listener = super::super::connection::bind_listener(loopback_tcp_ep(), false)
            .await
            .unwrap();
        let server_ep = Endpoint::from((Protocol::Tcp, listener.local_addr().unwrap()));
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<()>();
        let (done_tx, done_rx) = tokio::sync::oneshot::channel::<()>();

        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let (mut connection, hello) = super::super::connection::accept_connection(
                &acceptor,
                &cert_factory,
                &resolver,
                socket,
            )
            .await
            .unwrap();
            let _ = ready_rx.await;
            super::super::protocol::write_raw_frame(
                &mut connection.stream,
                &super::super::protocol::ControlConnReady {
                    tunnel_id: hello.tunnel_id,
                    candidate_id: hello.candidate_id,
                    result: super::super::protocol::ControlConnReadyResult::Success,
                },
            )
            .await
            .unwrap();
            let _ = done_rx.await;
        });

        let server_id = server_identity.get_id();
        let server_name = server_identity.get_name();
        let mut pending = Box::pin(client_network.create_tunnel(
            &client_identity,
            &server_ep,
            &server_id,
            Some(server_name),
        ));

        assert!(
            timeout(Duration::from_millis(100), &mut pending)
                .await
                .is_err()
        );
        ready_tx.send(()).unwrap();

        let tunnel = pending.await.unwrap();
        assert!(matches!(
            tunnel.state(),
            crate::networks::TunnelState::Connected
        ));
        done_tx.send(()).unwrap();
        tunnel.close().await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_duplicate_control_ready_frame_closes_tunnel_with_error() {
        init_tls_once();

        let (client_network, _client_resolver) = new_network();
        let client_identity = new_identity("client-duplicate-control-ready");
        let server_identity = new_identity("server-duplicate-control-ready");
        let cert_factory: P2pIdentityCertFactoryRef = Arc::new(X509IdentityCertFactory);
        let resolver: crate::tls::ServerCertResolverRef = DefaultTlsServerCertResolver::new();
        resolver
            .add_server_identity(server_identity.clone())
            .await
            .unwrap();
        let acceptor =
            super::super::connection::build_acceptor(resolver.clone(), cert_factory.clone());
        let listener = super::super::connection::bind_listener(loopback_tcp_ep(), false)
            .await
            .unwrap();
        let server_ep = Endpoint::from((Protocol::Tcp, listener.local_addr().unwrap()));

        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let (mut connection, hello) = super::super::connection::accept_connection(
                &acceptor,
                &cert_factory,
                &resolver,
                socket,
            )
            .await
            .unwrap();
            let ready = super::super::protocol::ControlConnReady {
                tunnel_id: hello.tunnel_id,
                candidate_id: hello.candidate_id,
                result: super::super::protocol::ControlConnReadyResult::Success,
            };
            super::super::protocol::write_raw_frame(&mut connection.stream, &ready)
                .await
                .unwrap();
            sleep(Duration::from_millis(100)).await;
            super::super::protocol::write_raw_frame(&mut connection.stream, &ready)
                .await
                .unwrap();
        });

        let tunnel = client_network
            .create_tunnel(
                &client_identity,
                &server_ep,
                &server_identity.get_id(),
                Some(server_identity.get_name()),
            )
            .await
            .unwrap();

        let active = wait_for_registry_tunnels(
            &client_network.registry,
            &client_identity.get_id(),
            &server_identity.get_id(),
            1,
        )
        .await
        .pop()
        .unwrap();

        timeout(Duration::from_secs(2), async {
            loop {
                if matches!(
                    active.state(),
                    crate::networks::TunnelState::Error | crate::networks::TunnelState::Closed
                ) {
                    break;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();

        let _ = tunnel.close().await;
        server.await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_control_ready_reject_result_fails_create_tunnel() {
        init_tls_once();

        let (client_network, _client_resolver) = new_network();
        let client_identity = new_identity("client-control-ready-reject");
        let server_identity = new_identity("server-control-ready-reject");
        let cert_factory: P2pIdentityCertFactoryRef = Arc::new(X509IdentityCertFactory);
        let resolver: crate::tls::ServerCertResolverRef = DefaultTlsServerCertResolver::new();
        resolver
            .add_server_identity(server_identity.clone())
            .await
            .unwrap();
        let acceptor =
            super::super::connection::build_acceptor(resolver.clone(), cert_factory.clone());
        let listener = super::super::connection::bind_listener(loopback_tcp_ep(), false)
            .await
            .unwrap();
        let server_ep = Endpoint::from((Protocol::Tcp, listener.local_addr().unwrap()));

        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let (mut connection, hello) = super::super::connection::accept_connection(
                &acceptor,
                &cert_factory,
                &resolver,
                socket,
            )
            .await
            .unwrap();
            super::super::protocol::write_raw_frame(
                &mut connection.stream,
                &super::super::protocol::ControlConnReady {
                    tunnel_id: hello.tunnel_id,
                    candidate_id: hello.candidate_id,
                    result: super::super::protocol::ControlConnReadyResult::TunnelConflictLost,
                },
            )
            .await
            .unwrap();
        });

        let err = match client_network
            .create_tunnel(
                &client_identity,
                &server_ep,
                &server_identity.get_id(),
                Some(server_identity.get_name()),
            )
            .await
        {
            Ok(_) => panic!("control ready reject should fail"),
            Err(err) => err,
        };
        assert_eq!(err.code(), P2pErrorCode::Reject);

        server.await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_control_ready_internal_error_fails_create_tunnel() {
        init_tls_once();

        let (client_network, _client_resolver) = new_network();
        let client_identity = new_identity("client-control-ready-internal");
        let server_identity = new_identity("server-control-ready-internal");
        let cert_factory: P2pIdentityCertFactoryRef = Arc::new(X509IdentityCertFactory);
        let resolver: crate::tls::ServerCertResolverRef = DefaultTlsServerCertResolver::new();
        resolver
            .add_server_identity(server_identity.clone())
            .await
            .unwrap();
        let acceptor =
            super::super::connection::build_acceptor(resolver.clone(), cert_factory.clone());
        let listener = super::super::connection::bind_listener(loopback_tcp_ep(), false)
            .await
            .unwrap();
        let server_ep = Endpoint::from((Protocol::Tcp, listener.local_addr().unwrap()));

        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let (mut connection, hello) = super::super::connection::accept_connection(
                &acceptor,
                &cert_factory,
                &resolver,
                socket,
            )
            .await
            .unwrap();
            super::super::protocol::write_raw_frame(
                &mut connection.stream,
                &super::super::protocol::ControlConnReady {
                    tunnel_id: hello.tunnel_id,
                    candidate_id: hello.candidate_id,
                    result: super::super::protocol::ControlConnReadyResult::InternalError,
                },
            )
            .await
            .unwrap();
        });

        let err = match client_network
            .create_tunnel(
                &client_identity,
                &server_ep,
                &server_identity.get_id(),
                Some(server_identity.get_name()),
            )
            .await
        {
            Ok(_) => panic!("control ready internal error should fail"),
            Err(err) => err,
        };
        assert_eq!(err.code(), P2pErrorCode::Reject);

        server.await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_invalid_data_hello_without_conn_id_returns_protocol_error() {
        init_tls_once();

        let (server_network, server_resolver) = new_network();
        let client_identity = new_identity("client-invalid-data");
        let server_identity = new_identity("server-invalid-data");

        register_listener_identity(&server_resolver, server_identity.clone()).await;

        server_network
            .listen(&loopback_tcp_ep(), None, None)
            .await
            .unwrap();

        let server_listener = server_network.listeners.lock().unwrap()[0].clone();
        let server_ep = server_listener.bound_local();
        let cert_factory: P2pIdentityCertFactoryRef = Arc::new(X509IdentityCertFactory);
        let tunnel_id = crate::types::TunnelId::from(0x3456);

        let mut control_connection = connect_with_optional_local(
            cert_factory.clone(),
            &client_identity,
            None,
            &server_ep,
            &server_identity.get_id(),
            Some(server_identity.get_name()),
            Duration::from_secs(3),
            false,
            &super::super::protocol::TcpConnectionHello {
                role: super::super::protocol::TcpConnectionRole::Control,
                tunnel_id,
                candidate_id: crate::types::TunnelCandidateId::from(4),
                is_reverse: false,
                conn_id: None,
                open_request_id: None,
            },
        )
        .await
        .unwrap();
        let _ = read_raw_frame::<_, super::super::protocol::ControlConnReady>(
            &mut control_connection.stream,
        )
        .await
        .unwrap();

        let mut data_connection = connect_with_optional_local(
            cert_factory,
            &client_identity,
            None,
            &server_ep,
            &server_identity.get_id(),
            Some(server_identity.get_name()),
            Duration::from_secs(3),
            false,
            &super::super::protocol::TcpConnectionHello {
                role: super::super::protocol::TcpConnectionRole::Data,
                tunnel_id,
                candidate_id: crate::types::TunnelCandidateId::from(4),
                is_reverse: false,
                conn_id: None,
                open_request_id: None,
            },
        )
        .await
        .unwrap();

        let ready =
            read_raw_frame::<_, super::super::protocol::DataConnReady>(&mut data_connection.stream)
                .await
                .unwrap();
        assert_eq!(ready.conn_id, tcp_id(0));
        assert_eq!(
            ready.result,
            super::super::protocol::DataConnReadyResult::ProtocolError
        );
    }

    #[tokio::test]
    async fn tcp_tunnel_duplicate_conn_id_returns_conn_id_conflict() {
        init_tls_once();

        let (server_network, server_resolver) = new_network();
        let client_identity = new_identity("client-conn-conflict");
        let server_identity = new_identity("server-conn-conflict");

        register_listener_identity(&server_resolver, server_identity.clone()).await;

        server_network
            .listen(&loopback_tcp_ep(), None, None)
            .await
            .unwrap();

        let server_listener = server_network.listeners.lock().unwrap()[0].clone();
        let server_ep = server_listener.bound_local();
        let cert_factory: P2pIdentityCertFactoryRef = Arc::new(X509IdentityCertFactory);
        let tunnel_id = crate::types::TunnelId::from(0x4567);
        let conn_id = tcp_id(42);

        let mut control_connection = connect_with_optional_local(
            cert_factory.clone(),
            &client_identity,
            None,
            &server_ep,
            &server_identity.get_id(),
            Some(server_identity.get_name()),
            Duration::from_secs(3),
            false,
            &super::super::protocol::TcpConnectionHello {
                role: super::super::protocol::TcpConnectionRole::Control,
                tunnel_id,
                candidate_id: crate::types::TunnelCandidateId::from(5),
                is_reverse: false,
                conn_id: None,
                open_request_id: None,
            },
        )
        .await
        .unwrap();
        let _ = read_raw_frame::<_, super::super::protocol::ControlConnReady>(
            &mut control_connection.stream,
        )
        .await
        .unwrap();

        let mut first_data_connection = connect_with_optional_local(
            cert_factory.clone(),
            &client_identity,
            None,
            &server_ep,
            &server_identity.get_id(),
            Some(server_identity.get_name()),
            Duration::from_secs(3),
            false,
            &super::super::protocol::TcpConnectionHello {
                role: super::super::protocol::TcpConnectionRole::Data,
                tunnel_id,
                candidate_id: crate::types::TunnelCandidateId::from(5),
                is_reverse: false,
                conn_id: Some(conn_id),
                open_request_id: None,
            },
        )
        .await
        .unwrap();
        let first_ready = read_raw_frame::<_, super::super::protocol::DataConnReady>(
            &mut first_data_connection.stream,
        )
        .await
        .unwrap();
        assert_eq!(
            first_ready.result,
            super::super::protocol::DataConnReadyResult::Success
        );

        let reverse = wait_for_registry_tunnels(
            &server_network.registry,
            &server_identity.get_id(),
            &client_identity.get_id(),
            1,
        )
        .await
        .pop()
        .unwrap();
        assert_eq!(reverse.data_conn_count_for_test(), 1);

        let mut second_data_connection = connect_with_optional_local(
            cert_factory,
            &client_identity,
            None,
            &server_ep,
            &server_identity.get_id(),
            Some(server_identity.get_name()),
            Duration::from_secs(3),
            false,
            &super::super::protocol::TcpConnectionHello {
                role: super::super::protocol::TcpConnectionRole::Data,
                tunnel_id,
                candidate_id: crate::types::TunnelCandidateId::from(5),
                is_reverse: false,
                conn_id: Some(conn_id),
                open_request_id: None,
            },
        )
        .await
        .unwrap();
        let second_ready = read_raw_frame::<_, super::super::protocol::DataConnReady>(
            &mut second_data_connection.stream,
        )
        .await
        .unwrap();
        assert_eq!(second_ready.conn_id, conn_id);
        assert_eq!(
            second_ready.result,
            super::super::protocol::DataConnReadyResult::ConnIdConflict
        );

        assert_eq!(reverse.data_conn_count_for_test(), 1);
        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_data_ready_conn_id_mismatch_rejects_connection() {
        init_tls_once();

        let (client_network, client_resolver) = new_network();
        let (server_network, server_resolver) = new_network();
        let client_identity = new_identity("client-data-ready-mismatch");
        let server_identity = new_identity("server-data-ready-mismatch");

        register_listener_identity(&client_resolver, client_identity.clone()).await;
        register_listener_identity(&server_resolver, server_identity.clone()).await;

        client_network
            .listen(&loopback_tcp_ep(), None, None)
            .await
            .unwrap();
        server_network
            .listen(&loopback_tcp_ep(), None, None)
            .await
            .unwrap();

        let client_listener = client_network.listeners.lock().unwrap()[0].clone();
        let server_listener = server_network.listeners.lock().unwrap()[0].clone();
        let client_bound_local = client_listener.bound_local();
        let server_bound_local = server_listener.bound_local();

        let accept_task =
            tokio::spawn(async move { server_listener.accept_tunnel().await.unwrap() });
        let _ = client_network
            .create_tunnel(
                &client_identity,
                &server_bound_local,
                &server_identity.get_id(),
                Some(server_identity.get_name()),
            )
            .await
            .unwrap();
        let _accepted = accept_task.await.unwrap();

        let mut active_tunnels = client_network
            .registry
            .find_tunnels_for_test(&client_identity.get_id(), &server_identity.get_id());
        let mut reverse_tunnels = server_network
            .registry
            .find_tunnels_for_test(&server_identity.get_id(), &client_identity.get_id());
        let active = active_tunnels.pop().unwrap();
        let reverse = reverse_tunnels.pop().unwrap();
        reverse.set_data_remote_ep_for_test(client_bound_local);

        let cert_factory: P2pIdentityCertFactoryRef = Arc::new(X509IdentityCertFactory);
        let resolver: crate::tls::ServerCertResolverRef = DefaultTlsServerCertResolver::new();
        resolver
            .add_server_identity(server_identity.clone())
            .await
            .unwrap();
        let acceptor =
            super::super::connection::build_acceptor(resolver.clone(), cert_factory.clone());
        let listener = super::super::connection::bind_listener(loopback_tcp_ep(), false)
            .await
            .unwrap();
        let fake_data_ep = Endpoint::from((Protocol::Tcp, listener.local_addr().unwrap()));
        active.set_data_remote_ep_for_test(fake_data_ep);

        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let (mut connection, hello) = super::super::connection::accept_connection(
                &acceptor,
                &cert_factory,
                &resolver,
                socket,
            )
            .await
            .unwrap();
            super::super::protocol::write_raw_frame(
                &mut connection.stream,
                &super::super::protocol::DataConnReady {
                    conn_id: tcp_id(hello.conn_id.unwrap().value().wrapping_add(1)),
                    candidate_id: hello.candidate_id,
                    result: super::super::protocol::DataConnReadyResult::Success,
                },
            )
            .await
            .unwrap();
        });

        let err = active.create_data_connection_for_test().await.unwrap_err();
        assert_eq!(err.code(), P2pErrorCode::InvalidData);
        assert_eq!(active.data_conn_count_for_test(), 0);

        server.await.unwrap();
        active.close().await.unwrap();
        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_concurrent_open_data_conn_req_associates_each_request() {
        let (active, reverse) = setup_pair().await;
        reverse.set_reverse_open_delay_for_test(Duration::from_millis(100));

        let first = tokio::spawn({
            let active = active.clone();
            async move { active.request_reverse_data_connection_for_test().await }
        });
        let second = tokio::spawn({
            let active = active.clone();
            async move { active.request_reverse_data_connection_for_test().await }
        });

        first.await.unwrap().unwrap();
        second.await.unwrap().unwrap();

        wait_for_data_conn_len(&active, 2).await;
        wait_for_data_conn_len(&reverse, 2).await;
        assert_eq!(active.idle_pool_len_for_test(), 0);
        assert_eq!(reverse.idle_pool_len_for_test(), 0);
        assert_eq!(
            active
                .data_conn_states_for_test()
                .iter()
                .filter(|state| state.contains("FirstClaimPending"))
                .count(),
            2
        );
        assert_eq!(
            reverse
                .data_conn_states_for_test()
                .iter()
                .filter(|state| state.contains("FirstClaimPending"))
                .count(),
            2
        );

        active.close().await.unwrap();
        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_direct_data_connections_without_open_request_id_register_on_both_sides() {
        let (active, reverse) = setup_pair().await;

        let (active_conn_id, reverse_conn_id) = tokio::join!(
            active.create_data_connection_for_test(),
            reverse.create_data_connection_for_test()
        );
        assert!(active_conn_id.is_ok());
        assert!(reverse_conn_id.is_ok());

        wait_for_data_conn_len(&active, 2).await;
        wait_for_data_conn_len(&reverse, 2).await;
        assert_eq!(
            active
                .data_conn_states_for_test()
                .iter()
                .filter(|state| state.contains("FirstClaimPending"))
                .count(),
            2
        );
        assert_eq!(
            reverse
                .data_conn_states_for_test()
                .iter()
                .filter(|state| state.contains("FirstClaimPending"))
                .count(),
            2
        );

        active.close().await.unwrap();
        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_data_ready_internal_error_rejects_connection() {
        init_tls_once();

        let (client_network, client_resolver) = new_network();
        let (server_network, server_resolver) = new_network();
        let client_identity = new_identity("client-data-ready-internal");
        let server_identity = new_identity("server-data-ready-internal");

        register_listener_identity(&client_resolver, client_identity.clone()).await;
        register_listener_identity(&server_resolver, server_identity.clone()).await;

        client_network
            .listen(&loopback_tcp_ep(), None, None)
            .await
            .unwrap();
        server_network
            .listen(&loopback_tcp_ep(), None, None)
            .await
            .unwrap();

        let client_listener = client_network.listeners.lock().unwrap()[0].clone();
        let server_listener = server_network.listeners.lock().unwrap()[0].clone();
        let client_bound_local = client_listener.bound_local();
        let server_bound_local = server_listener.bound_local();

        let accept_task =
            tokio::spawn(async move { server_listener.accept_tunnel().await.unwrap() });
        let _ = client_network
            .create_tunnel(
                &client_identity,
                &server_bound_local,
                &server_identity.get_id(),
                Some(server_identity.get_name()),
            )
            .await
            .unwrap();
        let _accepted = accept_task.await.unwrap();

        let mut active_tunnels = client_network
            .registry
            .find_tunnels_for_test(&client_identity.get_id(), &server_identity.get_id());
        let mut reverse_tunnels = server_network
            .registry
            .find_tunnels_for_test(&server_identity.get_id(), &client_identity.get_id());
        let active = active_tunnels.pop().unwrap();
        let reverse = reverse_tunnels.pop().unwrap();
        reverse.set_data_remote_ep_for_test(client_bound_local);

        let cert_factory: P2pIdentityCertFactoryRef = Arc::new(X509IdentityCertFactory);
        let resolver: crate::tls::ServerCertResolverRef = DefaultTlsServerCertResolver::new();
        resolver
            .add_server_identity(server_identity.clone())
            .await
            .unwrap();
        let acceptor =
            super::super::connection::build_acceptor(resolver.clone(), cert_factory.clone());
        let listener = super::super::connection::bind_listener(loopback_tcp_ep(), false)
            .await
            .unwrap();
        let fake_data_ep = Endpoint::from((Protocol::Tcp, listener.local_addr().unwrap()));
        active.set_data_remote_ep_for_test(fake_data_ep);

        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let (mut connection, hello) = super::super::connection::accept_connection(
                &acceptor,
                &cert_factory,
                &resolver,
                socket,
            )
            .await
            .unwrap();
            super::super::protocol::write_raw_frame(
                &mut connection.stream,
                &super::super::protocol::DataConnReady {
                    conn_id: hello.conn_id.unwrap(),
                    candidate_id: hello.candidate_id,
                    result: super::super::protocol::DataConnReadyResult::InternalError,
                },
            )
            .await
            .unwrap();
        });

        let err = match active.create_data_connection_for_test().await {
            Ok(_) => panic!("data ready internal error should fail"),
            Err(err) => err,
        };
        assert_eq!(err.code(), P2pErrorCode::Reject);
        assert_eq!(active.data_conn_count_for_test(), 0);

        server.await.unwrap();
        active.close().await.unwrap();
        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_data_ready_protocol_error_rejects_connection() {
        init_tls_once();

        let (client_network, client_resolver) = new_network();
        let (server_network, server_resolver) = new_network();
        let client_identity = new_identity("client-data-ready-protocol");
        let server_identity = new_identity("server-data-ready-protocol");

        register_listener_identity(&client_resolver, client_identity.clone()).await;
        register_listener_identity(&server_resolver, server_identity.clone()).await;

        client_network
            .listen(&loopback_tcp_ep(), None, None)
            .await
            .unwrap();
        server_network
            .listen(&loopback_tcp_ep(), None, None)
            .await
            .unwrap();

        let client_listener = client_network.listeners.lock().unwrap()[0].clone();
        let server_listener = server_network.listeners.lock().unwrap()[0].clone();
        let client_bound_local = client_listener.bound_local();
        let server_bound_local = server_listener.bound_local();

        let accept_task =
            tokio::spawn(async move { server_listener.accept_tunnel().await.unwrap() });
        let _ = client_network
            .create_tunnel(
                &client_identity,
                &server_bound_local,
                &server_identity.get_id(),
                Some(server_identity.get_name()),
            )
            .await
            .unwrap();
        let _accepted = accept_task.await.unwrap();

        let mut active_tunnels = client_network
            .registry
            .find_tunnels_for_test(&client_identity.get_id(), &server_identity.get_id());
        let mut reverse_tunnels = server_network
            .registry
            .find_tunnels_for_test(&server_identity.get_id(), &client_identity.get_id());
        let active = active_tunnels.pop().unwrap();
        let reverse = reverse_tunnels.pop().unwrap();
        reverse.set_data_remote_ep_for_test(client_bound_local);

        let cert_factory: P2pIdentityCertFactoryRef = Arc::new(X509IdentityCertFactory);
        let resolver: crate::tls::ServerCertResolverRef = DefaultTlsServerCertResolver::new();
        resolver
            .add_server_identity(server_identity.clone())
            .await
            .unwrap();
        let acceptor =
            super::super::connection::build_acceptor(resolver.clone(), cert_factory.clone());
        let listener = super::super::connection::bind_listener(loopback_tcp_ep(), false)
            .await
            .unwrap();
        let fake_data_ep = Endpoint::from((Protocol::Tcp, listener.local_addr().unwrap()));
        active.set_data_remote_ep_for_test(fake_data_ep);

        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let (mut connection, hello) = super::super::connection::accept_connection(
                &acceptor,
                &cert_factory,
                &resolver,
                socket,
            )
            .await
            .unwrap();
            super::super::protocol::write_raw_frame(
                &mut connection.stream,
                &super::super::protocol::DataConnReady {
                    conn_id: hello.conn_id.unwrap(),
                    candidate_id: hello.candidate_id,
                    result: super::super::protocol::DataConnReadyResult::ProtocolError,
                },
            )
            .await
            .unwrap();
        });

        let err = match active.create_data_connection_for_test().await {
            Ok(_) => panic!("data ready protocol error should fail"),
            Err(err) => err,
        };
        assert_eq!(err.code(), P2pErrorCode::Reject);
        assert_eq!(active.data_conn_count_for_test(), 0);

        server.await.unwrap();
        active.close().await.unwrap();
        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_data_ready_conn_id_conflict_rejects_connection() {
        init_tls_once();

        let (client_network, client_resolver) = new_network();
        let (server_network, server_resolver) = new_network();
        let client_identity = new_identity("client-data-ready-conflict");
        let server_identity = new_identity("server-data-ready-conflict");

        register_listener_identity(&client_resolver, client_identity.clone()).await;
        register_listener_identity(&server_resolver, server_identity.clone()).await;

        client_network
            .listen(&loopback_tcp_ep(), None, None)
            .await
            .unwrap();
        server_network
            .listen(&loopback_tcp_ep(), None, None)
            .await
            .unwrap();

        let client_listener = client_network.listeners.lock().unwrap()[0].clone();
        let server_listener = server_network.listeners.lock().unwrap()[0].clone();
        let client_bound_local = client_listener.bound_local();
        let server_bound_local = server_listener.bound_local();

        let accept_task =
            tokio::spawn(async move { server_listener.accept_tunnel().await.unwrap() });
        let _ = client_network
            .create_tunnel(
                &client_identity,
                &server_bound_local,
                &server_identity.get_id(),
                Some(server_identity.get_name()),
            )
            .await
            .unwrap();
        let _accepted = accept_task.await.unwrap();

        let mut active_tunnels = client_network
            .registry
            .find_tunnels_for_test(&client_identity.get_id(), &server_identity.get_id());
        let mut reverse_tunnels = server_network
            .registry
            .find_tunnels_for_test(&server_identity.get_id(), &client_identity.get_id());
        let active = active_tunnels.pop().unwrap();
        let reverse = reverse_tunnels.pop().unwrap();
        reverse.set_data_remote_ep_for_test(client_bound_local);

        let cert_factory: P2pIdentityCertFactoryRef = Arc::new(X509IdentityCertFactory);
        let resolver: crate::tls::ServerCertResolverRef = DefaultTlsServerCertResolver::new();
        resolver
            .add_server_identity(server_identity.clone())
            .await
            .unwrap();
        let acceptor =
            super::super::connection::build_acceptor(resolver.clone(), cert_factory.clone());
        let listener = super::super::connection::bind_listener(loopback_tcp_ep(), false)
            .await
            .unwrap();
        let fake_data_ep = Endpoint::from((Protocol::Tcp, listener.local_addr().unwrap()));
        active.set_data_remote_ep_for_test(fake_data_ep);

        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let (mut connection, hello) = super::super::connection::accept_connection(
                &acceptor,
                &cert_factory,
                &resolver,
                socket,
            )
            .await
            .unwrap();
            super::super::protocol::write_raw_frame(
                &mut connection.stream,
                &super::super::protocol::DataConnReady {
                    conn_id: hello.conn_id.unwrap(),
                    candidate_id: hello.candidate_id,
                    result: super::super::protocol::DataConnReadyResult::ConnIdConflict,
                },
            )
            .await
            .unwrap();
        });

        let err = match active.create_data_connection_for_test().await {
            Ok(_) => panic!("data ready conn id conflict should fail"),
            Err(err) => err,
        };
        assert_eq!(err.code(), P2pErrorCode::Reject);
        assert_eq!(active.data_conn_count_for_test(), 0);

        server.await.unwrap();
        active.close().await.unwrap();
        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_duplicate_success_data_ready_after_success_rejects_connection() {
        init_tls_once();

        let (client_network, client_resolver) = new_network();
        let (server_network, server_resolver) = new_network();
        let client_identity = new_identity("client-data-ready-duplicate-success");
        let server_identity = new_identity("server-data-ready-duplicate-success");

        register_listener_identity(&client_resolver, client_identity.clone()).await;
        register_listener_identity(&server_resolver, server_identity.clone()).await;

        client_network
            .listen(&loopback_tcp_ep(), None, None)
            .await
            .unwrap();
        server_network
            .listen(&loopback_tcp_ep(), None, None)
            .await
            .unwrap();

        let client_listener = client_network.listeners.lock().unwrap()[0].clone();
        let server_listener = server_network.listeners.lock().unwrap()[0].clone();
        let client_bound_local = client_listener.bound_local();
        let server_bound_local = server_listener.bound_local();

        let accept_task =
            tokio::spawn(async move { server_listener.accept_tunnel().await.unwrap() });
        let _ = client_network
            .create_tunnel(
                &client_identity,
                &server_bound_local,
                &server_identity.get_id(),
                Some(server_identity.get_name()),
            )
            .await
            .unwrap();
        let _accepted = accept_task.await.unwrap();

        let mut active_tunnels = client_network
            .registry
            .find_tunnels_for_test(&client_identity.get_id(), &server_identity.get_id());
        let mut reverse_tunnels = server_network
            .registry
            .find_tunnels_for_test(&server_identity.get_id(), &client_identity.get_id());
        let active = active_tunnels.pop().unwrap();
        let reverse = reverse_tunnels.pop().unwrap();
        reverse.set_data_remote_ep_for_test(client_bound_local);

        let cert_factory: P2pIdentityCertFactoryRef = Arc::new(X509IdentityCertFactory);
        let resolver: crate::tls::ServerCertResolverRef = DefaultTlsServerCertResolver::new();
        resolver
            .add_server_identity(server_identity.clone())
            .await
            .unwrap();
        let acceptor =
            super::super::connection::build_acceptor(resolver.clone(), cert_factory.clone());
        let listener = super::super::connection::bind_listener(loopback_tcp_ep(), false)
            .await
            .unwrap();
        let fake_data_ep = Endpoint::from((Protocol::Tcp, listener.local_addr().unwrap()));
        active.set_data_remote_ep_for_test(fake_data_ep);

        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let (mut connection, hello) = super::super::connection::accept_connection(
                &acceptor,
                &cert_factory,
                &resolver,
                socket,
            )
            .await
            .unwrap();
            let ready = super::super::protocol::DataConnReady {
                conn_id: hello.conn_id.unwrap(),
                candidate_id: hello.candidate_id,
                result: super::super::protocol::DataConnReadyResult::Success,
            };
            super::super::protocol::write_raw_frame(&mut connection.stream, &ready)
                .await
                .unwrap();
            super::super::protocol::write_raw_frame(&mut connection.stream, &ready)
                .await
                .unwrap();
        });

        let err = match active.create_data_connection_for_test().await {
            Ok(_) => panic!("duplicate success data ready should fail"),
            Err(err) => err,
        };
        assert_eq!(err.code(), P2pErrorCode::InvalidData);
        assert_eq!(active.data_conn_count_for_test(), 0);

        server.await.unwrap();
        active.close().await.unwrap();
        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_conflicting_second_data_ready_after_success_rejects_connection() {
        init_tls_once();

        let (client_network, client_resolver) = new_network();
        let (server_network, server_resolver) = new_network();
        let client_identity = new_identity("client-data-ready-second-conflict");
        let server_identity = new_identity("server-data-ready-second-conflict");

        register_listener_identity(&client_resolver, client_identity.clone()).await;
        register_listener_identity(&server_resolver, server_identity.clone()).await;

        client_network
            .listen(&loopback_tcp_ep(), None, None)
            .await
            .unwrap();
        server_network
            .listen(&loopback_tcp_ep(), None, None)
            .await
            .unwrap();

        let client_listener = client_network.listeners.lock().unwrap()[0].clone();
        let server_listener = server_network.listeners.lock().unwrap()[0].clone();
        let client_bound_local = client_listener.bound_local();
        let server_bound_local = server_listener.bound_local();

        let accept_task =
            tokio::spawn(async move { server_listener.accept_tunnel().await.unwrap() });
        let _ = client_network
            .create_tunnel(
                &client_identity,
                &server_bound_local,
                &server_identity.get_id(),
                Some(server_identity.get_name()),
            )
            .await
            .unwrap();
        let _accepted = accept_task.await.unwrap();

        let mut active_tunnels = client_network
            .registry
            .find_tunnels_for_test(&client_identity.get_id(), &server_identity.get_id());
        let mut reverse_tunnels = server_network
            .registry
            .find_tunnels_for_test(&server_identity.get_id(), &client_identity.get_id());
        let active = active_tunnels.pop().unwrap();
        let reverse = reverse_tunnels.pop().unwrap();
        reverse.set_data_remote_ep_for_test(client_bound_local);

        let cert_factory: P2pIdentityCertFactoryRef = Arc::new(X509IdentityCertFactory);
        let resolver: crate::tls::ServerCertResolverRef = DefaultTlsServerCertResolver::new();
        resolver
            .add_server_identity(server_identity.clone())
            .await
            .unwrap();
        let acceptor =
            super::super::connection::build_acceptor(resolver.clone(), cert_factory.clone());
        let listener = super::super::connection::bind_listener(loopback_tcp_ep(), false)
            .await
            .unwrap();
        let fake_data_ep = Endpoint::from((Protocol::Tcp, listener.local_addr().unwrap()));
        active.set_data_remote_ep_for_test(fake_data_ep);

        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let (mut connection, hello) = super::super::connection::accept_connection(
                &acceptor,
                &cert_factory,
                &resolver,
                socket,
            )
            .await
            .unwrap();
            super::super::protocol::write_raw_frame(
                &mut connection.stream,
                &super::super::protocol::DataConnReady {
                    conn_id: hello.conn_id.unwrap(),
                    candidate_id: hello.candidate_id,
                    result: super::super::protocol::DataConnReadyResult::Success,
                },
            )
            .await
            .unwrap();
            super::super::protocol::write_raw_frame(
                &mut connection.stream,
                &super::super::protocol::DataConnReady {
                    conn_id: hello.conn_id.unwrap(),
                    candidate_id: hello.candidate_id,
                    result: super::super::protocol::DataConnReadyResult::ProtocolError,
                },
            )
            .await
            .unwrap();
        });

        let err = match active.create_data_connection_for_test().await {
            Ok(_) => panic!("conflicting second data ready should fail"),
            Err(err) => err,
        };
        assert_eq!(err.code(), P2pErrorCode::InvalidData);
        assert_eq!(active.data_conn_count_for_test(), 0);

        server.await.unwrap();
        active.close().await.unwrap();
        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_unknown_conn_claim_req_returns_protocol_error() {
        let (active, reverse) = setup_pair().await;

        let response = active
            .simulate_claim_req_for_test(claim_req(
                7000,
                super::super::protocol::TcpChannelKind::Stream,
                3099,
                tcp_id(0xdead_beef),
                tcp_id(1),
                1,
            ))
            .await
            .unwrap();

        assert!(matches!(
            response,
            super::super::protocol::TcpControlCmd::ClaimConnAck(
                super::super::protocol::ClaimConnAck {
                    result,
                    ..
                }
            ) if result == super::super::protocol::ClaimConnAckResult::ProtocolError as u8
        ));

        active.close().await.unwrap();
        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_first_claim_pending_rejects_remote_claim_for_local_connection() {
        let (active, reverse) = setup_pair().await;

        let conn_id = active.create_data_connection_for_test().await.unwrap();
        assert!(
            active
                .data_conn_states_for_test()
                .iter()
                .any(|state| state.contains("FirstClaimPending"))
        );

        let response = active
            .simulate_claim_req_for_test(claim_req(
                7003,
                super::super::protocol::TcpChannelKind::Stream,
                3203,
                conn_id,
                tcp_id(1),
                100,
            ))
            .await
            .unwrap();

        assert!(matches!(
            response,
            super::super::protocol::TcpControlCmd::ClaimConnAck(
                super::super::protocol::ClaimConnAck {
                    result,
                    ..
                }
            ) if result == super::super::protocol::ClaimConnAckResult::ProtocolError as u8
        ));
        assert_eq!(active.data_conn_count_for_test(), 1);

        active.close().await.unwrap();
        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_first_successful_claim_commits_lease_seq_one() {
        let (active, reverse) = setup_pair().await;

        let (opened, accepted) = tokio::join!(
            active.open_stream(purpose_of(3204)),
            reverse.accept_stream()
        );
        let (read, write) = opened.unwrap();
        let (_vport, peer_read, peer_write) = accepted.unwrap();

        let conn_id = active.first_conn_id_for_test().unwrap();
        let (_channel_id, lease_seq, _tx_bytes, _rx_bytes) =
            active.current_lease_for_test(conn_id).unwrap();
        assert_eq!(lease_seq, tcp_id(1));

        drop(write);
        drop(read);
        drop(peer_read);
        drop(peer_write);
        active.close().await.unwrap();
        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_first_claim_timeout_retires_unclaimed_connection() {
        let (active, reverse) = setup_pair_full(
            Duration::from_secs(1),
            Duration::from_secs(5),
            Duration::from_millis(150),
            true,
        )
        .await;

        active.create_data_connection_for_test().await.unwrap();

        wait_for_data_conn_len(&active, 0).await;
        wait_for_data_conn_len(&reverse, 0).await;

        active.close().await.unwrap();
        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_conflict_claim_higher_local_nonce_keeps_local_claim() {
        let (active, reverse) = setup_pair().await;
        seed_idle_stream(&active, &reverse).await;

        let conn_id = active.first_conn_id_for_test().unwrap();
        let (channel_id, lease_seq) = active
            .start_claim_for_test(
                conn_id,
                super::super::protocol::TcpChannelKind::Stream,
                purpose_of(3301),
                11,
            )
            .unwrap();

        let response = active
            .simulate_claim_req_for_test(claim_req(
                7004,
                super::super::protocol::TcpChannelKind::Stream,
                3302,
                conn_id,
                lease_seq,
                10,
            ))
            .await
            .unwrap();

        assert!(matches!(
            response,
            super::super::protocol::TcpControlCmd::ClaimConnAck(
                super::super::protocol::ClaimConnAck {
                    result,
                    ..
                }
            ) if result == super::super::protocol::ClaimConnAckResult::ConflictLost as u8
        ));
        assert_eq!(
            active.current_claiming_for_test(conn_id),
            Some((
                channel_id,
                lease_seq,
                11,
                super::super::protocol::TcpChannelKind::Stream,
                purpose_of(3301),
            ))
        );
        assert!(
            timeout(Duration::from_millis(100), active.accept_stream())
                .await
                .is_err()
        );

        active.close().await.unwrap();
        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_conflict_claim_equal_nonce_uses_p2pid_tiebreak() {
        let (active, reverse) = setup_pair().await;
        seed_idle_stream(&active, &reverse).await;

        let loser = if active.local_id().as_slice() < active.remote_id().as_slice() {
            active.clone()
        } else {
            reverse.clone()
        };
        let peer = if Arc::ptr_eq(&loser, &active) {
            reverse.clone()
        } else {
            active.clone()
        };

        let conn_id = loser.first_conn_id_for_test().unwrap();
        let (_local_channel_id, lease_seq) = loser
            .start_claim_for_test(
                conn_id,
                super::super::protocol::TcpChannelKind::Stream,
                purpose_of(3303),
                77,
            )
            .unwrap();

        let response = loser
            .simulate_claim_req_for_test(claim_req(
                7005,
                super::super::protocol::TcpChannelKind::Stream,
                3304,
                conn_id,
                lease_seq,
                77,
            ))
            .await
            .unwrap();

        assert!(matches!(
            response,
            super::super::protocol::TcpControlCmd::ClaimConnAck(
                super::super::protocol::ClaimConnAck {
                    result,
                    ..
                }
            ) if result == super::super::protocol::ClaimConnAckResult::Success as u8
        ));
        assert_eq!(
            loser.current_lease_for_test(conn_id).unwrap().0,
            tcp_id(7005)
        );

        let accepted = timeout(Duration::from_millis(100), loser.accept_stream())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(accepted.0, purpose_of(3304));
        let (_vport, read, write) = accepted;
        drop(read);
        drop(write);

        loser.close().await.unwrap();
        peer.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_conflict_loser_pending_claim_fails_but_remote_channel_stays_bound() {
        let (active, reverse) = setup_pair().await;
        seed_idle_stream(&active, &reverse).await;

        let loser = if active.local_id().as_slice() < active.remote_id().as_slice() {
            active.clone()
        } else {
            reverse.clone()
        };
        let peer = if Arc::ptr_eq(&loser, &active) {
            reverse.clone()
        } else {
            active.clone()
        };

        let conn_id = loser.first_conn_id_for_test().unwrap();
        let (local_channel_id, lease_seq) = loser
            .start_claim_for_test(
                conn_id,
                super::super::protocol::TcpChannelKind::Stream,
                purpose_of(3307),
                7,
            )
            .unwrap();
        let pending_claim = loser.register_pending_claim_for_test(local_channel_id);

        let response = loser
            .simulate_claim_req_for_test(claim_req(
                7009,
                super::super::protocol::TcpChannelKind::Stream,
                3308,
                conn_id,
                lease_seq,
                7,
            ))
            .await
            .unwrap();

        assert!(matches!(
            response,
            super::super::protocol::TcpControlCmd::ClaimConnAck(
                super::super::protocol::ClaimConnAck {
                    result,
                    ..
                }
            ) if result == super::super::protocol::ClaimConnAckResult::Success as u8
        ));
        let err = match pending_claim.await.unwrap() {
            Ok(_) => panic!("local pending claim should fail"),
            Err(err) => err,
        };
        assert_eq!(err.code(), P2pErrorCode::Conflict);
        assert_eq!(
            loser.current_lease_for_test(conn_id).unwrap().0,
            tcp_id(7009)
        );

        let accepted = timeout(Duration::from_millis(100), loser.accept_stream())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(accepted.0, purpose_of(3308));
        let (_vport, read, write) = accepted;
        drop(read);
        drop(write);

        loser.close().await.unwrap();
        peer.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_duplicate_claim_req_replays_ack_once() {
        let (active, reverse) = setup_pair().await;
        active
            .request_reverse_data_connection_for_test()
            .await
            .unwrap();
        let conn_id = active.first_conn_id_for_test().unwrap();
        let req = claim_req(
            7001,
            super::super::protocol::TcpChannelKind::Stream,
            3100,
            conn_id,
            tcp_id(1),
            42,
        );

        let first = active
            .simulate_claim_req_for_test(req.clone())
            .await
            .unwrap();
        let second = active.simulate_claim_req_for_test(req).await.unwrap();
        assert!(matches!(
            first,
            super::super::protocol::TcpControlCmd::ClaimConnAck(
                super::super::protocol::ClaimConnAck {
                    result,
                    ..
                }
            ) if result == super::super::protocol::ClaimConnAckResult::Success as u8
        ));
        assert!(matches!(
            second,
            super::super::protocol::TcpControlCmd::ClaimConnAck(
                super::super::protocol::ClaimConnAck {
                    result,
                    ..
                }
            ) if result == super::super::protocol::ClaimConnAckResult::Success as u8
        ));

        let accepted = timeout(Duration::from_millis(100), active.accept_stream())
            .await
            .unwrap()
            .unwrap();
        let (_vport, read, write) = accepted;
        drop(read);
        drop(write);
        match timeout(Duration::from_millis(100), active.accept_stream()).await {
            Err(_) => {}
            Ok(Err(_)) => {}
            Ok(Ok(_)) => panic!("duplicate claim replay unexpectedly accepted a second channel"),
        }

        active.close().await.unwrap();
        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_duplicate_invalid_claim_req_replays_same_ack_error() {
        let (active, reverse) = setup_pair().await;
        active
            .request_reverse_data_connection_for_test()
            .await
            .unwrap();
        let conn_id = active.first_conn_id_for_test().unwrap();
        let req = claim_req(
            7002,
            super::super::protocol::TcpChannelKind::Stream,
            3200,
            conn_id,
            tcp_id(2),
            99,
        );

        let first = active
            .simulate_claim_req_for_test(req.clone())
            .await
            .unwrap();
        let second = active.simulate_claim_req_for_test(req).await.unwrap();
        assert!(matches!(
            first,
            super::super::protocol::TcpControlCmd::ClaimConnAck(
                super::super::protocol::ClaimConnAck {
                    result,
                    ..
                }
            ) if result == super::super::protocol::ClaimConnAckResult::LeaseMismatch as u8
        ));
        assert!(matches!(
            second,
            super::super::protocol::TcpControlCmd::ClaimConnAck(
                super::super::protocol::ClaimConnAck {
                    result,
                    ..
                }
            ) if result == super::super::protocol::ClaimConnAckResult::LeaseMismatch as u8
        ));

        active.close().await.unwrap();
        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_idle_claim_with_wrong_lease_seq_returns_lease_mismatch() {
        let (active, reverse) = setup_pair().await;
        seed_idle_stream(&active, &reverse).await;

        let conn_id = active.first_conn_id_for_test().unwrap();
        let response = active
            .simulate_claim_req_for_test(claim_req(
                7006,
                super::super::protocol::TcpChannelKind::Stream,
                3205,
                conn_id,
                tcp_id(3),
                1,
            ))
            .await
            .unwrap();

        assert!(matches!(
            response,
            super::super::protocol::TcpControlCmd::ClaimConnAck(
                super::super::protocol::ClaimConnAck {
                    result,
                    ..
                }
            ) if result == super::super::protocol::ClaimConnAckResult::LeaseMismatch as u8
        ));

        active.close().await.unwrap();
        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_claiming_with_wrong_lease_seq_returns_lease_mismatch() {
        let (active, reverse) = setup_pair().await;
        seed_idle_stream(&active, &reverse).await;

        let conn_id = active.first_conn_id_for_test().unwrap();
        let (channel_id, lease_seq) = active
            .start_claim_for_test(
                conn_id,
                super::super::protocol::TcpChannelKind::Stream,
                purpose_of(3206),
                9,
            )
            .unwrap();

        let response = active
            .simulate_claim_req_for_test(claim_req(
                7007,
                super::super::protocol::TcpChannelKind::Stream,
                3207,
                conn_id,
                tcp_id(lease_seq.value().wrapping_add(1)),
                2,
            ))
            .await
            .unwrap();

        assert!(matches!(
            response,
            super::super::protocol::TcpControlCmd::ClaimConnAck(
                super::super::protocol::ClaimConnAck {
                    result,
                    ..
                }
            ) if result == super::super::protocol::ClaimConnAckResult::LeaseMismatch as u8
        ));
        assert_eq!(
            active.current_claiming_for_test(conn_id),
            Some((
                channel_id,
                lease_seq,
                9,
                super::super::protocol::TcpChannelKind::Stream,
                purpose_of(3206),
            ))
        );

        active.close().await.unwrap();
        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_claim_req_returns_accept_queue_full_when_no_slot() {
        let (active, reverse) = setup_pair().await;
        active
            .request_reverse_data_connection_for_test()
            .await
            .unwrap();
        active.set_accept_queue_limit_for_test(super::super::protocol::TcpChannelKind::Stream, 0);

        let conn_id = active.first_conn_id_for_test().unwrap();
        let response = active
            .simulate_claim_req_for_test(claim_req(
                7010,
                super::super::protocol::TcpChannelKind::Stream,
                3210,
                conn_id,
                tcp_id(1),
                1,
            ))
            .await
            .unwrap();

        assert!(matches!(
            response,
            super::super::protocol::TcpControlCmd::ClaimConnAck(
                super::super::protocol::ClaimConnAck {
                    result,
                    ..
                }
            ) if result == super::super::protocol::ClaimConnAckResult::AcceptQueueFull as u8
        ));

        active.close().await.unwrap();
        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_claim_req_returns_not_idle_when_connection_is_bound() {
        let (active, reverse) = setup_pair().await;

        let (opened, accepted) = tokio::join!(
            active.open_stream(purpose_of(3211)),
            reverse.accept_stream()
        );
        let (read, write) = opened.unwrap();
        let (_vport, peer_read, peer_write) = accepted.unwrap();

        let conn_id = active.first_conn_id_for_test().unwrap();
        let (_channel_id, lease_seq, _tx_bytes, _rx_bytes) =
            active.current_lease_for_test(conn_id).unwrap();

        let response = active
            .simulate_claim_req_for_test(claim_req(
                7012,
                super::super::protocol::TcpChannelKind::Stream,
                3212,
                conn_id,
                lease_seq,
                2,
            ))
            .await
            .unwrap();

        assert!(matches!(
            response,
            super::super::protocol::TcpControlCmd::ClaimConnAck(
                super::super::protocol::ClaimConnAck {
                    result,
                    ..
                }
            ) if result == super::super::protocol::ClaimConnAckResult::NotIdle as u8
        ));

        drop(write);
        drop(read);
        drop(peer_read);
        drop(peer_write);
        active.close().await.unwrap();
        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_duplicate_claim_req_with_conflicting_fields_returns_protocol_error() {
        let (active, reverse) = setup_pair().await;
        active
            .request_reverse_data_connection_for_test()
            .await
            .unwrap();

        let conn_id = active.first_conn_id_for_test().unwrap();
        let first_req = claim_req(
            7013,
            super::super::protocol::TcpChannelKind::Stream,
            3213,
            conn_id,
            tcp_id(1),
            11,
        );

        let first = active
            .simulate_claim_req_for_test(first_req.clone())
            .await
            .unwrap();
        assert!(matches!(
            first,
            super::super::protocol::TcpControlCmd::ClaimConnAck(
                super::super::protocol::ClaimConnAck {
                    result,
                    ..
                }
            ) if result == super::super::protocol::ClaimConnAckResult::Success as u8
        ));

        let second = active
            .simulate_claim_req_for_test(super::super::protocol::ClaimConnReq {
                purpose: purpose_of(3214),
                ..first_req
            })
            .await
            .unwrap();
        assert!(matches!(
            second,
            super::super::protocol::TcpControlCmd::ClaimConnAck(
                super::super::protocol::ClaimConnAck {
                    result,
                    ..
                }
            ) if result == super::super::protocol::ClaimConnAckResult::ProtocolError as u8
        ));

        let accepted = active.accept_stream().await.unwrap();
        let (_vport, read, write) = accepted;
        drop(write);
        drop(read);
        active.close().await.unwrap();
        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_claim_req_returns_listener_closed_when_accept_closed() {
        let (active, reverse) = setup_pair().await;
        active
            .request_reverse_data_connection_for_test()
            .await
            .unwrap();
        active
            .close_accept_queue_for_test(super::super::protocol::TcpChannelKind::Stream)
            .await;

        let conn_id = active.first_conn_id_for_test().unwrap();
        let response = active
            .simulate_claim_req_for_test(claim_req(
                7011,
                super::super::protocol::TcpChannelKind::Stream,
                3211,
                conn_id,
                tcp_id(1),
                2,
            ))
            .await
            .unwrap();

        assert!(matches!(
            response,
            super::super::protocol::TcpControlCmd::ClaimConnAck(
                super::super::protocol::ClaimConnAck {
                    result,
                    ..
                }
            ) if result == super::super::protocol::ClaimConnAckResult::ListenerClosed as u8
        ));

        active.close().await.unwrap();
        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_open_stream_to_unlistened_port_returns_error() {
        let (active, reverse) = setup_pair().await;
        let empty_vports = ListenVPortRegistry::<()>::new();

        reverse
            .listen_stream(empty_vports.as_listen_vports_ref())
            .await
            .unwrap();

        let err = active.open_stream(purpose_of(3924)).await.err().unwrap();
        assert_eq!(err.code(), P2pErrorCode::PortNotListen);

        active.close().await.unwrap();
        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_first_stream_claim_waits_for_late_listen() {
        let (active, reverse) = setup_pair_without_listen().await;

        let mut pending_open = Box::pin({
            let active = active.clone();
            async move { active.open_stream(purpose_of(3926)).await }
        });

        assert!(
            timeout(Duration::from_millis(100), &mut pending_open)
                .await
                .is_err()
        );

        reverse
            .listen_stream(allow_all_listen_vports())
            .await
            .unwrap();

        let opened = pending_open.await.unwrap();
        let (vport, mut peer_read, mut peer_write) = reverse.accept_stream().await.unwrap();
        let (mut read, mut write) = opened;

        assert_eq!(vport, purpose_of(3926));

        write.write_all(b"ping").await.unwrap();
        let mut buf = [0u8; 4];
        peer_read.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"ping");

        peer_write.write_all(b"pong").await.unwrap();
        let mut reply = [0u8; 4];
        read.read_exact(&mut reply).await.unwrap();
        assert_eq!(&reply, b"pong");

        active.close().await.unwrap();
        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_stream_first_wait_is_independent_from_datagram_listen() {
        let (active, reverse) = setup_pair_without_listen().await;

        let mut pending_open = Box::pin({
            let active = active.clone();
            async move { active.open_stream(purpose_of(3927)).await }
        });

        reverse
            .listen_datagram(allow_all_listen_vports())
            .await
            .unwrap();

        assert!(
            timeout(Duration::from_millis(100), &mut pending_open)
                .await
                .is_err()
        );

        reverse
            .listen_stream(allow_all_listen_vports())
            .await
            .unwrap();

        let opened = pending_open.await.unwrap();
        let (vport, _peer_read, _peer_write) = reverse.accept_stream().await.unwrap();
        assert_eq!(vport, purpose_of(3927));
        drop(opened);

        active.close().await.unwrap();
        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_multiple_stream_claims_wait_for_same_late_listen() {
        let (active, reverse) = setup_pair_without_listen().await;

        let mut pending_open_1 = Box::pin({
            let active = active.clone();
            async move { active.open_stream(purpose_of(3928)).await }
        });
        let mut pending_open_2 = Box::pin({
            let active = active.clone();
            async move { active.open_stream(purpose_of(3929)).await }
        });

        assert!(
            timeout(Duration::from_millis(100), &mut pending_open_1)
                .await
                .is_err()
        );
        assert!(
            timeout(Duration::from_millis(100), &mut pending_open_2)
                .await
                .is_err()
        );

        reverse
            .listen_stream(allow_all_listen_vports())
            .await
            .unwrap();

        let _opened_1 = pending_open_1.await.unwrap();
        let _opened_2 = pending_open_2.await.unwrap();

        let accepted_1 = reverse.accept_stream().await.unwrap();
        let accepted_2 = reverse.accept_stream().await.unwrap();
        let accepted_vports = vec![accepted_1.0, accepted_2.0];
        assert!(accepted_vports.contains(&purpose_of(3928)));
        assert!(accepted_vports.contains(&purpose_of(3929)));

        active.close().await.unwrap();
        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_accept_stream_requires_listen_first() {
        let (active, reverse) = setup_pair_without_listen().await;

        let err = reverse.accept_stream().await.err().unwrap();
        assert_eq!(err.code(), P2pErrorCode::Interrupted);

        active.close().await.unwrap();
        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_accept_datagram_requires_listen_first() {
        let (active, reverse) = setup_pair_without_listen().await;

        let err = reverse.accept_datagram().await.err().unwrap();
        assert_eq!(err.code(), P2pErrorCode::Interrupted);

        active.close().await.unwrap();
        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_datagram_hidden_direction_nonzero_fin_retires_connection() {
        let (active, reverse) = setup_pair().await;

        let (opened, accepted) = tokio::join!(
            active.open_datagram(purpose_of(3220)),
            reverse.accept_datagram()
        );
        let mut writer = opened.unwrap();
        let (_vport, peer_read) = accepted.unwrap();

        writer.write_all(b"hello").await.unwrap();
        let conn_id = active.first_conn_id_for_test().unwrap();
        let (channel_id, lease_seq, _tx_bytes, _rx_bytes) =
            active.current_lease_for_test(conn_id).unwrap();

        active.simulate_write_fin_for_test(super::super::protocol::WriteFin {
            channel_id,
            conn_id,
            lease_seq,
            final_tx_bytes: 1,
        });

        sleep(Duration::from_millis(50)).await;
        assert_eq!(active.data_conn_count_for_test(), 0);

        drop(writer);
        drop(peer_read);
        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_datagram_hidden_zero_path_sends_auto_zero_fin_and_read_done() {
        let (active, reverse) = setup_pair().await;

        let (opened, accepted) = tokio::join!(
            active.open_datagram(purpose_of(3221)),
            reverse.accept_datagram()
        );
        let writer = opened.unwrap();
        let (_vport, reader) = accepted.unwrap();

        let _reverse_state =
            wait_for_conn_state_fragments(&reverse, &["local_fin=true", "tx=0"]).await;

        let _active_state = wait_for_conn_state_fragments(
            &active,
            &["local_done=true", "rx=0", "peer_final=Some(0)"],
        )
        .await;

        drop(writer);
        drop(reader);

        wait_for_idle_pool_len(&active, 1).await;
        wait_for_idle_pool_len(&reverse, 1).await;

        active.close().await.unwrap();
        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_datagram_hidden_direction_nonzero_bytes_retires_connection() {
        let (active, reverse) = setup_pair().await;

        let (opened, accepted) = tokio::join!(
            active.open_datagram(purpose_of(3222)),
            reverse.accept_datagram()
        );
        let writer = opened.unwrap();
        let reader = accepted.unwrap().1;

        let conn_id = active.first_conn_id_for_test().unwrap();
        reverse
            .write_hidden_bytes_for_test(conn_id, b"x")
            .await
            .unwrap();

        wait_for_data_conn_len(&active, 0).await;

        drop(writer);
        drop(reader);
        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_pending_open_request_fails_immediately_on_close() {
        let (active, reverse) = setup_pair_with_timeout(Duration::from_secs(2)).await;
        reverse.set_reverse_open_delay_for_test(Duration::from_millis(400));

        let pending = tokio::spawn({
            let active = active.clone();
            async move { active.request_reverse_data_connection_for_test().await }
        });

        sleep(Duration::from_millis(50)).await;
        active.close().await.unwrap();

        let err = match pending.await.unwrap() {
            Ok(_) => panic!("pending open request should fail after close"),
            Err(err) => err,
        };
        assert_eq!(err.code(), P2pErrorCode::Interrupted);

        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_pending_claim_fails_immediately_on_close() {
        let (active, reverse) = setup_pair().await;
        seed_idle_stream(&active, &reverse).await;
        reverse.set_claim_req_delay_for_test(Duration::from_millis(400));

        let pending = tokio::spawn({
            let active = active.clone();
            async move { active.open_stream(purpose_of(3900)).await }
        });

        sleep(Duration::from_millis(50)).await;
        active.close().await.unwrap();

        let err = match pending.await.unwrap() {
            Ok(_) => panic!("pending claim should fail after close"),
            Err(err) => err,
        };
        assert_eq!(err.code(), P2pErrorCode::Interrupted);

        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_conflict_claim_checks_accept_queue_before_conflict_resolution() {
        let (active, reverse) = setup_pair().await;
        seed_idle_stream(&active, &reverse).await;
        reverse.set_claim_req_delay_for_test(Duration::from_millis(400));
        active.set_accept_queue_limit_for_test(super::super::protocol::TcpChannelKind::Stream, 0);

        let pending_open = tokio::spawn({
            let active = active.clone();
            async move { active.open_stream(purpose_of(3930)).await }
        });

        let conn_id = active.first_conn_id_for_test().unwrap();
        let (_local_channel_id, lease_seq, claim_nonce, kind, _vport) =
            timeout(Duration::from_secs(1), async {
                loop {
                    if let Some(claiming) = active.current_claiming_for_test(conn_id) {
                        break claiming;
                    }
                    sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
        let response = active
            .simulate_claim_req_for_test(claim_req(
                7030,
                kind,
                3931,
                conn_id,
                lease_seq,
                claim_nonce.saturating_add(1),
            ))
            .await
            .unwrap();

        assert!(matches!(
            response,
            super::super::protocol::TcpControlCmd::ClaimConnAck(
                super::super::protocol::ClaimConnAck {
                    result,
                    ..
                }
            ) if result == super::super::protocol::ClaimConnAckResult::AcceptQueueFull as u8
        ));
        assert!(
            timeout(Duration::from_millis(100), pending_open)
                .await
                .is_err()
        );

        active.close().await.unwrap();
        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_pending_read_done_before_local_fin_is_applied_after_fin() {
        let (active, reverse) = setup_pair().await;

        let (opened, accepted) = tokio::join!(
            active.open_stream(purpose_of(3910)),
            reverse.accept_stream()
        );
        let (read, write) = opened.unwrap();
        let (_vport, peer_read, peer_write) = accepted.unwrap();

        let conn_id = active.first_conn_id_for_test().unwrap();
        let (channel_id, lease_seq, tx_bytes, _rx_bytes) =
            active.current_lease_for_test(conn_id).unwrap();
        assert_eq!(tx_bytes, 0);

        active.simulate_read_done_for_test(super::super::protocol::ReadDone {
            channel_id,
            conn_id,
            lease_seq,
            final_rx_bytes: 0,
        });

        drop(write);
        drop(read);
        drop(peer_read);
        drop(peer_write);

        timeout(Duration::from_secs(2), wait_for_idle_pool_len(&active, 1))
            .await
            .unwrap();
        timeout(Duration::from_secs(2), wait_for_idle_pool_len(&reverse, 1))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_write_fin_before_tail_data_still_returns_connection_to_idle() {
        let (active, reverse) = setup_pair().await;

        let (opened, accepted) = tokio::join!(
            active.open_stream(purpose_of(3911)),
            reverse.accept_stream()
        );
        let (mut read, write) = opened.unwrap();
        let (_vport, peer_read, mut peer_write) = accepted.unwrap();

        let conn_id = active.first_conn_id_for_test().unwrap();
        let (channel_id, lease_seq, _tx_bytes, _rx_bytes) =
            active.current_lease_for_test(conn_id).unwrap();

        active.simulate_write_fin_for_test(super::super::protocol::WriteFin {
            channel_id,
            conn_id,
            lease_seq,
            final_tx_bytes: 4,
        });

        peer_write.write_all(b"late").await.unwrap();
        let mut buf = [0u8; 4];
        read.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"late");

        drop(peer_write);
        drop(peer_read);
        drop(write);
        drop(read);

        wait_for_idle_pool_len(&active, 1).await;
        wait_for_idle_pool_len(&reverse, 1).await;
    }

    #[tokio::test]
    async fn tcp_tunnel_local_read_drop_with_matching_peer_fin_still_reuses_connection() {
        let (active, reverse) = setup_pair().await;

        let (opened, accepted) = tokio::join!(
            active.open_stream(purpose_of(3912)),
            reverse.accept_stream()
        );
        let (mut read, write) = opened.unwrap();
        let (_vport, peer_read, mut peer_write) = accepted.unwrap();

        peer_write.write_all(b"abc").await.unwrap();
        let mut buf = [0u8; 3];
        read.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"abc");

        drop(read);
        drop(peer_write);
        drop(write);
        drop(peer_read);

        wait_for_idle_pool_len(&active, 1).await;
        wait_for_idle_pool_len(&reverse, 1).await;

        active.close().await.unwrap();
        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_local_read_drop_with_mismatched_peer_fin_retires_connection() {
        let (active, reverse) = setup_pair().await;

        let (opened, accepted) = tokio::join!(
            active.open_stream(purpose_of(3913)),
            reverse.accept_stream()
        );
        let (mut read, write) = opened.unwrap();
        let (_vport, peer_read, mut peer_write) = accepted.unwrap();

        peer_write.write_all(b"abc").await.unwrap();
        let mut buf = [0u8; 3];
        read.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"abc");

        drop(read);
        peer_write.write_all(b"d").await.unwrap();
        drop(peer_write);

        wait_for_data_conn_len(&active, 0).await;
        assert_eq!(active.idle_pool_len_for_test(), 0);

        drop(write);
        drop(peer_read);
        active.close().await.unwrap();
        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_local_write_drop_with_failed_write_fin_retires_connection() {
        let (active, reverse) = setup_pair().await;

        let (opened, accepted) = tokio::join!(
            active.open_stream(purpose_of(3914)),
            reverse.accept_stream()
        );
        let (read, mut write) = opened.unwrap();
        let (_vport, peer_read, peer_write) = accepted.unwrap();

        write.write_all(b"abc").await.unwrap();
        active.fail_next_control_send_for_test();
        drop(write);

        wait_for_data_conn_len(&active, 0).await;
        assert_eq!(active.idle_pool_len_for_test(), 0);

        drop(read);
        drop(peer_read);
        drop(peer_write);
        active.close().await.unwrap();
        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_unknown_conn_write_fin_closes_tunnel_with_error() {
        let (active, reverse) = setup_pair().await;

        let err = active
            .simulate_control_cmd_for_test(super::super::protocol::TcpControlCmd::WriteFin(
                super::super::protocol::WriteFin {
                    channel_id: tcp_id(9001),
                    conn_id: tcp_id(0xdead_beef),
                    lease_seq: tcp_id(1),
                    final_tx_bytes: 0,
                },
            ))
            .await
            .unwrap_err();
        assert_eq!(err.code(), P2pErrorCode::InvalidData);

        timeout(Duration::from_secs(2), async {
            while !active.is_closed() {
                sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        assert!(matches!(
            active.state(),
            crate::networks::TunnelState::Error
        ));

        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_unknown_conn_read_done_closes_tunnel_with_error() {
        let (active, reverse) = setup_pair().await;

        let err = active
            .simulate_control_cmd_for_test(super::super::protocol::TcpControlCmd::ReadDone(
                super::super::protocol::ReadDone {
                    channel_id: tcp_id(9002),
                    conn_id: tcp_id(0xdead_beef),
                    lease_seq: tcp_id(1),
                    final_rx_bytes: 0,
                },
            ))
            .await
            .unwrap_err();
        assert_eq!(err.code(), P2pErrorCode::InvalidData);

        timeout(Duration::from_secs(2), async {
            while !active.is_closed() {
                sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        assert!(matches!(
            active.state(),
            crate::networks::TunnelState::Error
        ));

        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_future_write_fin_on_idle_connection_retires_it() {
        let (active, reverse) = setup_pair().await;
        seed_idle_stream(&active, &reverse).await;

        let conn_id = active.first_conn_id_for_test().unwrap();
        active.simulate_write_fin_for_test(super::super::protocol::WriteFin {
            channel_id: tcp_id(9901),
            conn_id,
            lease_seq: tcp_id(2),
            final_tx_bytes: 0,
        });

        sleep(Duration::from_millis(50)).await;
        assert_eq!(active.data_conn_count_for_test(), 0);
        assert_eq!(active.idle_pool_len_for_test(), 0);

        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_future_read_done_on_idle_connection_retires_it() {
        let (active, reverse) = setup_pair().await;
        seed_idle_stream(&active, &reverse).await;

        let conn_id = active.first_conn_id_for_test().unwrap();
        active.simulate_read_done_for_test(super::super::protocol::ReadDone {
            channel_id: tcp_id(9902),
            conn_id,
            lease_seq: tcp_id(2),
            final_rx_bytes: 0,
        });

        sleep(Duration::from_millis(50)).await;
        assert_eq!(active.data_conn_count_for_test(), 0);
        assert_eq!(active.idle_pool_len_for_test(), 0);

        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_heartbeat_timeout_closes_silent_peer() {
        let (active, reverse) = setup_pair_full(
            Duration::from_secs(1),
            Duration::from_millis(50),
            Duration::from_millis(150),
            true,
        )
        .await;
        reverse.set_suppress_pong_for_test(true);
        reverse.set_suppress_ping_for_test(true);

        timeout(Duration::from_secs(2), async {
            loop {
                if active.is_closed() {
                    break;
                }
                sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();

        assert!(matches!(
            active.state(),
            crate::networks::TunnelState::Error
        ));
        assert!(active.is_closed());
        assert!(
            reverse.is_closed()
                || !matches!(reverse.state(), crate::networks::TunnelState::Connected)
        );
    }

    #[tokio::test]
    async fn tcp_tunnel_write_fin_without_drain_progress_retires_connection() {
        let (active, reverse) = setup_pair_full(
            Duration::from_secs(1),
            Duration::from_millis(50),
            Duration::from_millis(150),
            true,
        )
        .await;

        let (opened, accepted) = tokio::join!(
            active.open_stream(purpose_of(3920)),
            reverse.accept_stream()
        );
        let (read, write) = opened.unwrap();
        let (_vport, peer_read, peer_write) = accepted.unwrap();

        let conn_id = active.first_conn_id_for_test().unwrap();
        let (channel_id, lease_seq, _tx_bytes, rx_bytes) =
            active.current_lease_for_test(conn_id).unwrap();
        assert_eq!(rx_bytes, 0);

        active.simulate_write_fin_for_test(super::super::protocol::WriteFin {
            channel_id,
            conn_id,
            lease_seq,
            final_tx_bytes: 1,
        });

        timeout(Duration::from_secs(3), async {
            loop {
                if active.data_conn_count_for_test() == 0 {
                    break;
                }
                sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();

        drop(write);
        drop(read);
        drop(peer_read);
        drop(peer_write);
        active.close().await.unwrap();
        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_claim_timeout_retires_connection() {
        let (active, reverse) = setup_pair_full(
            Duration::from_secs(1),
            Duration::from_secs(5),
            Duration::from_millis(150),
            true,
        )
        .await;
        seed_idle_stream(&active, &reverse).await;
        reverse.set_claim_req_delay_for_test(Duration::from_millis(400));

        let err = match active.open_stream(purpose_of(3925)).await {
            Ok(_) => panic!("claim should time out"),
            Err(err) => err,
        };
        assert_eq!(err.code(), P2pErrorCode::Timeout);

        wait_for_data_conn_len(&active, 0).await;

        active.close().await.unwrap();
        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_pending_write_fin_timeout_retires_connection_and_clears_pending() {
        let (active, reverse) = setup_pair_full(
            Duration::from_secs(1),
            Duration::from_secs(5),
            Duration::from_millis(150),
            true,
        )
        .await;
        seed_idle_stream(&active, &reverse).await;

        let conn_id = active.first_conn_id_for_test().unwrap();
        let (channel_id, lease_seq) = active
            .start_claim_for_test(
                conn_id,
                super::super::protocol::TcpChannelKind::Stream,
                purpose_of(3926),
                1,
            )
            .unwrap();

        active.simulate_write_fin_for_test(super::super::protocol::WriteFin {
            channel_id,
            conn_id,
            lease_seq,
            final_tx_bytes: 0,
        });

        assert_eq!(active.pending_drain_count_for_test(), 1);

        wait_for_data_conn_len(&active, 0).await;
        wait_for_pending_drain_len(&active, 0).await;

        active.close().await.unwrap();
        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_pending_read_done_timeout_retires_connection_and_clears_pending() {
        let (active, reverse) = setup_pair_full(
            Duration::from_secs(1),
            Duration::from_secs(5),
            Duration::from_millis(150),
            true,
        )
        .await;
        seed_idle_stream(&active, &reverse).await;
        reverse.set_claim_req_delay_for_test(Duration::from_millis(400));

        let pending_open = tokio::spawn({
            let active = active.clone();
            async move { active.open_stream(purpose_of(3927)).await }
        });

        let conn_id = active.first_conn_id_for_test().unwrap();
        let (channel_id, lease_seq, _claim_nonce, _kind, _vport) =
            wait_for_claiming(&active, conn_id).await;

        active.simulate_read_done_for_test(super::super::protocol::ReadDone {
            channel_id,
            conn_id,
            lease_seq,
            final_rx_bytes: 0,
        });

        assert_eq!(active.pending_drain_count_for_test(), 1);

        let err = match pending_open.await.unwrap() {
            Ok(_) => panic!("pending claim should time out"),
            Err(err) => err,
        };
        assert_eq!(err.code(), P2pErrorCode::Timeout);

        wait_for_data_conn_len(&active, 0).await;
        wait_for_pending_drain_len(&active, 0).await;

        active.close().await.unwrap();
        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_write_fin_channel_mismatch_retires_connection() {
        let (active, reverse) = setup_pair().await;

        let (opened, accepted) = tokio::join!(
            active.open_stream(purpose_of(3310)),
            reverse.accept_stream()
        );
        let (read, write) = opened.unwrap();
        let (_vport, peer_read, peer_write) = accepted.unwrap();

        let conn_id = active.first_conn_id_for_test().unwrap();
        let (channel_id, lease_seq, _tx_bytes, _rx_bytes) =
            active.current_lease_for_test(conn_id).unwrap();

        active.simulate_write_fin_for_test(super::super::protocol::WriteFin {
            channel_id: tcp_id(channel_id.value().wrapping_add(1)),
            conn_id,
            lease_seq,
            final_tx_bytes: 0,
        });

        wait_for_data_conn_len(&active, 0).await;

        drop(write);
        drop(read);
        drop(peer_read);
        drop(peer_write);
        active.close().await.unwrap();
        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_conflicting_duplicate_write_fin_retires_connection() {
        let (active, reverse) = setup_pair().await;

        let (opened, accepted) = tokio::join!(
            active.open_stream(purpose_of(3300)),
            reverse.accept_stream()
        );
        let (read, mut write) = opened.unwrap();
        let (_vport, mut peer_read, peer_write) = accepted.unwrap();

        write.write_all(b"fin!").await.unwrap();
        let mut buf = [0u8; 4];
        peer_read.read_exact(&mut buf).await.unwrap();

        let conn_id = reverse.first_conn_id_for_test().unwrap();
        let (channel_id, lease_seq, _tx_bytes, rx_bytes) =
            reverse.current_lease_for_test(conn_id).unwrap();

        reverse.simulate_write_fin_for_test(super::super::protocol::WriteFin {
            channel_id,
            conn_id,
            lease_seq,
            final_tx_bytes: rx_bytes,
        });
        reverse.simulate_write_fin_for_test(super::super::protocol::WriteFin {
            channel_id,
            conn_id,
            lease_seq,
            final_tx_bytes: rx_bytes + 1,
        });

        sleep(Duration::from_millis(50)).await;
        assert_eq!(reverse.data_conn_count_for_test(), 0);

        drop(write);
        drop(read);
        drop(peer_read);
        drop(peer_write);
        active.close().await.unwrap();
        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_duplicate_write_fin_with_same_value_is_idempotent() {
        let (active, reverse) = setup_pair().await;

        let (opened, accepted) = tokio::join!(
            active.open_stream(purpose_of(3301)),
            reverse.accept_stream()
        );
        let (read, mut write) = opened.unwrap();
        let (_vport, mut peer_read, peer_write) = accepted.unwrap();

        write.write_all(b"same").await.unwrap();
        let mut buf = [0u8; 4];
        peer_read.read_exact(&mut buf).await.unwrap();

        let conn_id = reverse.first_conn_id_for_test().unwrap();
        let (channel_id, lease_seq, _tx_bytes, rx_bytes) =
            reverse.current_lease_for_test(conn_id).unwrap();

        reverse.simulate_write_fin_for_test(super::super::protocol::WriteFin {
            channel_id,
            conn_id,
            lease_seq,
            final_tx_bytes: rx_bytes,
        });
        reverse.simulate_write_fin_for_test(super::super::protocol::WriteFin {
            channel_id,
            conn_id,
            lease_seq,
            final_tx_bytes: rx_bytes,
        });

        assert_eq!(reverse.data_conn_count_for_test(), 1);

        drop(write);
        drop(read);
        drop(peer_read);
        drop(peer_write);

        wait_for_idle_pool_len(&active, 1).await;
        wait_for_idle_pool_len(&reverse, 1).await;

        active.close().await.unwrap();
        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_conflicting_duplicate_read_done_retires_connection() {
        let (active, reverse) = setup_pair().await;

        let (opened, accepted) = tokio::join!(
            active.open_stream(purpose_of(3400)),
            reverse.accept_stream()
        );
        let (read, mut write) = opened.unwrap();
        let (_vport, mut peer_read, peer_write) = accepted.unwrap();

        write.write_all(b"done").await.unwrap();
        let mut buf = [0u8; 4];
        peer_read.read_exact(&mut buf).await.unwrap();

        let conn_id = active.first_conn_id_for_test().unwrap();
        let (channel_id, lease_seq, tx_bytes, _rx_bytes) =
            active.current_lease_for_test(conn_id).unwrap();

        drop(write);
        sleep(Duration::from_millis(50)).await;

        active.simulate_read_done_for_test(super::super::protocol::ReadDone {
            channel_id,
            conn_id,
            lease_seq,
            final_rx_bytes: tx_bytes,
        });
        active.simulate_read_done_for_test(super::super::protocol::ReadDone {
            channel_id,
            conn_id,
            lease_seq,
            final_rx_bytes: tx_bytes + 1,
        });

        sleep(Duration::from_millis(50)).await;
        assert_eq!(active.data_conn_count_for_test(), 0);

        drop(read);
        drop(peer_read);
        drop(peer_write);
        active.close().await.unwrap();
        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_duplicate_read_done_with_same_value_is_idempotent() {
        let (active, reverse) = setup_pair().await;

        let (opened, accepted) = tokio::join!(
            active.open_stream(purpose_of(3401)),
            reverse.accept_stream()
        );
        let (read, mut write) = opened.unwrap();
        let (_vport, mut peer_read, peer_write) = accepted.unwrap();

        write.write_all(b"same").await.unwrap();
        let mut buf = [0u8; 4];
        peer_read.read_exact(&mut buf).await.unwrap();

        let conn_id = active.first_conn_id_for_test().unwrap();
        let (channel_id, lease_seq, tx_bytes, _rx_bytes) =
            active.current_lease_for_test(conn_id).unwrap();

        active.simulate_read_done_for_test(super::super::protocol::ReadDone {
            channel_id,
            conn_id,
            lease_seq,
            final_rx_bytes: tx_bytes,
        });
        active.simulate_read_done_for_test(super::super::protocol::ReadDone {
            channel_id,
            conn_id,
            lease_seq,
            final_rx_bytes: tx_bytes,
        });

        assert_eq!(active.data_conn_count_for_test(), 1);

        drop(write);
        drop(read);
        drop(peer_read);
        drop(peer_write);

        wait_for_idle_pool_len(&active, 1).await;
        wait_for_idle_pool_len(&reverse, 1).await;

        active.close().await.unwrap();
        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_stale_write_fin_and_read_done_are_ignored_after_reuse() {
        let (active, reverse) = setup_pair().await;

        let (first_opened, first_accepted) = tokio::join!(
            active.open_stream(purpose_of(3500)),
            reverse.accept_stream()
        );
        let (first_read, mut first_write) = first_opened.unwrap();
        let (_vport1, mut first_peer_read, first_peer_write) = first_accepted.unwrap();

        first_write.write_all(b"old").await.unwrap();
        let mut old_buf = [0u8; 3];
        first_peer_read.read_exact(&mut old_buf).await.unwrap();

        let conn_id = reverse.first_conn_id_for_test().unwrap();
        let (old_channel_id, old_lease_seq, old_tx_bytes, old_rx_bytes) =
            reverse.current_lease_for_test(conn_id).unwrap();

        drop(first_write);
        drop(first_read);
        drop(first_peer_read);
        drop(first_peer_write);

        wait_for_idle_pool_len(&active, 1).await;
        wait_for_idle_pool_len(&reverse, 1).await;

        let (second_opened, second_accepted) = tokio::join!(
            active.open_stream(purpose_of(3501)),
            reverse.accept_stream()
        );
        let (second_read, mut second_write) = second_opened.unwrap();
        let (_vport2, mut second_peer_read, second_peer_write) = second_accepted.unwrap();

        let (_new_channel_id, new_lease_seq, _new_tx_bytes, _new_rx_bytes) =
            reverse.current_lease_for_test(conn_id).unwrap();
        assert!(new_lease_seq > old_lease_seq);

        reverse.simulate_write_fin_for_test(super::super::protocol::WriteFin {
            channel_id: old_channel_id,
            conn_id,
            lease_seq: old_lease_seq,
            final_tx_bytes: old_rx_bytes,
        });
        reverse.simulate_read_done_for_test(super::super::protocol::ReadDone {
            channel_id: old_channel_id,
            conn_id,
            lease_seq: old_lease_seq,
            final_rx_bytes: old_tx_bytes,
        });

        second_write.write_all(b"new").await.unwrap();
        let mut new_buf = [0u8; 3];
        second_peer_read.read_exact(&mut new_buf).await.unwrap();
        assert_eq!(&new_buf, b"new");

        drop(second_write);
        drop(second_read);
        drop(second_peer_read);
        drop(second_peer_write);

        wait_for_idle_pool_len(&active, 1).await;
        wait_for_idle_pool_len(&reverse, 1).await;
    }

    #[tokio::test]
    async fn tcp_tunnel_late_claim_ack_is_ignored_after_channel_settles() {
        let (active, reverse) = setup_pair().await;

        let (opened, accepted) = tokio::join!(
            active.open_stream(purpose_of(3600)),
            reverse.accept_stream()
        );
        let (read, write) = opened.unwrap();
        let (_vport, peer_read, peer_write) = accepted.unwrap();

        let conn_id = active.first_conn_id_for_test().unwrap();
        let (channel_id, lease_seq, _tx_bytes, _rx_bytes) =
            active.current_lease_for_test(conn_id).unwrap();

        drop(write);
        drop(read);
        drop(peer_read);
        drop(peer_write);

        wait_for_idle_pool_len(&active, 1).await;
        wait_for_idle_pool_len(&reverse, 1).await;

        active
            .simulate_claim_ack_for_test(super::super::protocol::ClaimConnAck {
                channel_id,
                conn_id,
                lease_seq,
                result: super::super::protocol::ClaimConnAckResult::Success as u8,
            })
            .await
            .unwrap();

        assert_eq!(active.idle_pool_len_for_test(), 1);

        let (reopened, reaccepted) = tokio::join!(
            active.open_stream(purpose_of(3601)),
            reverse.accept_stream()
        );
        let (read2, write2) = reopened.unwrap();
        let (_vport2, peer_read2, peer_write2) = reaccepted.unwrap();
        drop(write2);
        drop(read2);
        drop(peer_read2);
        drop(peer_write2);

        wait_for_idle_pool_len(&active, 1).await;
        wait_for_idle_pool_len(&reverse, 1).await;
    }

    #[tokio::test]
    async fn tcp_tunnel_late_claim_error_ack_is_ignored_after_channel_settles() {
        let (active, reverse) = setup_pair().await;

        let (opened, accepted) = tokio::join!(
            active.open_stream(purpose_of(3700)),
            reverse.accept_stream()
        );
        let (read, write) = opened.unwrap();
        let (_vport, peer_read, peer_write) = accepted.unwrap();

        let conn_id = active.first_conn_id_for_test().unwrap();
        let (channel_id, lease_seq, _tx_bytes, _rx_bytes) =
            active.current_lease_for_test(conn_id).unwrap();

        drop(write);
        drop(read);
        drop(peer_read);
        drop(peer_write);

        wait_for_idle_pool_len(&active, 1).await;
        wait_for_idle_pool_len(&reverse, 1).await;

        active
            .simulate_claim_error_for_test(
                channel_id,
                conn_id,
                lease_seq,
                super::super::protocol::ClaimConnAckResult::ConflictLost,
            )
            .await
            .unwrap();

        assert_eq!(active.idle_pool_len_for_test(), 1);

        let (reopened, reaccepted) = tokio::join!(
            active.open_stream(purpose_of(3701)),
            reverse.accept_stream()
        );
        let (read2, write2) = reopened.unwrap();
        let (_vport2, peer_read2, peer_write2) = reaccepted.unwrap();
        drop(write2);
        drop(read2);
        drop(peer_read2);
        drop(peer_write2);

        wait_for_idle_pool_len(&active, 1).await;
        wait_for_idle_pool_len(&reverse, 1).await;
    }

    #[tokio::test]
    async fn tcp_tunnel_claim_retry_uses_new_channel_and_ignores_old_ack_results() {
        let (active, reverse) = setup_pair_full(
            Duration::from_secs(3),
            Duration::from_secs(5),
            Duration::from_secs(10),
            true,
        )
        .await;
        seed_idle_stream(&active, &reverse).await;
        reverse.set_claim_req_delay_for_test(Duration::from_secs(2));

        let pending_open = tokio::spawn({
            let active = active.clone();
            async move { active.open_stream(purpose_of(3702)).await }
        });

        let conn_id = active.first_conn_id_for_test().unwrap();
        let (old_channel_id, lease_seq, _old_claim_nonce, _kind, _vport) =
            wait_for_claiming(&active, conn_id).await;

        active
            .simulate_claim_error_for_test(
                old_channel_id,
                conn_id,
                lease_seq,
                super::super::protocol::ClaimConnAckResult::ConflictLost,
            )
            .await
            .unwrap();

        let (new_channel_id, new_lease_seq, _new_claim_nonce, _kind2, _vport2) =
            wait_for_claiming(&active, conn_id).await;
        assert_eq!(new_lease_seq, lease_seq);
        assert_ne!(new_channel_id, old_channel_id);

        active
            .simulate_claim_ack_for_test(super::super::protocol::ClaimConnAck {
                channel_id: old_channel_id,
                conn_id,
                lease_seq,
                result: super::super::protocol::ClaimConnAckResult::Success as u8,
            })
            .await
            .unwrap();
        active
            .simulate_claim_error_for_test(
                old_channel_id,
                conn_id,
                lease_seq,
                super::super::protocol::ClaimConnAckResult::ConflictLost,
            )
            .await
            .unwrap();

        assert_eq!(
            active.current_claiming_for_test(conn_id).unwrap().0,
            new_channel_id
        );

        active
            .simulate_claim_ack_for_test(super::super::protocol::ClaimConnAck {
                channel_id: new_channel_id,
                conn_id,
                lease_seq: new_lease_seq,
                result: super::super::protocol::ClaimConnAckResult::Success as u8,
            })
            .await
            .unwrap();

        let (read, write) = pending_open.await.unwrap().unwrap();
        drop(write);
        drop(read);

        active.close().await.unwrap();
        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_old_claim_ack_results_do_not_disturb_retried_bound_channel() {
        let (active, reverse) = setup_pair_full(
            Duration::from_secs(3),
            Duration::from_secs(5),
            Duration::from_secs(10),
            true,
        )
        .await;
        seed_idle_stream(&active, &reverse).await;
        reverse.set_claim_req_delay_for_test(Duration::from_secs(2));

        let pending_open = tokio::spawn({
            let active = active.clone();
            async move { active.open_stream(purpose_of(3705)).await }
        });

        let conn_id = active.first_conn_id_for_test().unwrap();
        let (old_channel_id, lease_seq, _old_claim_nonce, _kind, _vport) =
            wait_for_claiming(&active, conn_id).await;

        active
            .simulate_claim_error_for_test(
                old_channel_id,
                conn_id,
                lease_seq,
                super::super::protocol::ClaimConnAckResult::ConflictLost,
            )
            .await
            .unwrap();

        let (new_channel_id, new_lease_seq, _new_claim_nonce, _kind2, _vport2) =
            wait_for_claiming(&active, conn_id).await;
        assert_ne!(new_channel_id, old_channel_id);

        active
            .simulate_claim_ack_for_test(super::super::protocol::ClaimConnAck {
                channel_id: new_channel_id,
                conn_id,
                lease_seq: new_lease_seq,
                result: super::super::protocol::ClaimConnAckResult::Success as u8,
            })
            .await
            .unwrap();

        let (read, write) = pending_open.await.unwrap().unwrap();
        assert_eq!(
            active.current_lease_for_test(conn_id).unwrap().0,
            new_channel_id
        );

        active
            .simulate_claim_ack_for_test(super::super::protocol::ClaimConnAck {
                channel_id: old_channel_id,
                conn_id,
                lease_seq,
                result: super::super::protocol::ClaimConnAckResult::Success as u8,
            })
            .await
            .unwrap();
        active
            .simulate_claim_error_for_test(
                old_channel_id,
                conn_id,
                lease_seq,
                super::super::protocol::ClaimConnAckResult::ConflictLost,
            )
            .await
            .unwrap();

        assert_eq!(
            active.current_lease_for_test(conn_id).unwrap().0,
            new_channel_id
        );

        drop(write);
        drop(read);
        active.close().await.unwrap();
        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_late_claim_ack_results_after_timeout_are_ignored() {
        let (active, reverse) = setup_pair_full(
            Duration::from_secs(1),
            Duration::from_secs(5),
            Duration::from_millis(150),
            true,
        )
        .await;
        seed_idle_stream(&active, &reverse).await;
        reverse.set_claim_req_delay_for_test(Duration::from_millis(400));

        let pending_open = tokio::spawn({
            let active = active.clone();
            async move { active.open_stream(purpose_of(3703)).await }
        });

        let conn_id = active.first_conn_id_for_test().unwrap();
        let (channel_id, lease_seq, _claim_nonce, _kind, _vport) =
            wait_for_claiming(&active, conn_id).await;

        let err = match pending_open.await.unwrap() {
            Ok(_) => panic!("pending claim should time out"),
            Err(err) => err,
        };
        assert_eq!(err.code(), P2pErrorCode::Timeout);

        wait_for_data_conn_len(&active, 0).await;

        active
            .simulate_claim_ack_for_test(super::super::protocol::ClaimConnAck {
                channel_id,
                conn_id,
                lease_seq,
                result: super::super::protocol::ClaimConnAckResult::Success as u8,
            })
            .await
            .unwrap();
        active
            .simulate_claim_error_for_test(
                channel_id,
                conn_id,
                lease_seq,
                super::super::protocol::ClaimConnAckResult::ConflictLost,
            )
            .await
            .unwrap();

        assert_eq!(active.data_conn_count_for_test(), 0);
        assert_eq!(active.idle_pool_len_for_test(), 0);

        active.close().await.unwrap();
        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_late_claim_ack_results_after_close_are_ignored() {
        let (active, reverse) = setup_pair().await;
        seed_idle_stream(&active, &reverse).await;
        reverse.set_claim_req_delay_for_test(Duration::from_millis(400));

        let pending_open = tokio::spawn({
            let active = active.clone();
            async move { active.open_stream(purpose_of(3704)).await }
        });

        let conn_id = active.first_conn_id_for_test().unwrap();
        let (channel_id, lease_seq, _claim_nonce, _kind, _vport) =
            wait_for_claiming(&active, conn_id).await;

        active.close().await.unwrap();

        let err = match pending_open.await.unwrap() {
            Ok(_) => panic!("pending claim should fail after close"),
            Err(err) => err,
        };
        assert_eq!(err.code(), P2pErrorCode::Interrupted);

        active
            .simulate_claim_ack_for_test(super::super::protocol::ClaimConnAck {
                channel_id,
                conn_id,
                lease_seq,
                result: super::super::protocol::ClaimConnAckResult::Success as u8,
            })
            .await
            .unwrap();
        active
            .simulate_claim_error_for_test(
                channel_id,
                conn_id,
                lease_seq,
                super::super::protocol::ClaimConnAckResult::ConflictLost,
            )
            .await
            .unwrap();

        assert!(active.is_closed());
        assert_eq!(active.idle_pool_len_for_test(), 0);
        assert!(matches!(
            active.state(),
            crate::networks::TunnelState::Closed
        ));

        reverse.close().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_tunnel_control_close_keeps_current_channel_but_disables_reuse() {
        let (active, reverse) = setup_pair().await;

        let (opened, accepted) = tokio::join!(
            active.open_stream(purpose_of(3800)),
            reverse.accept_stream()
        );
        let (read, mut write) = opened.unwrap();
        let (_vport, mut peer_read, peer_write) = accepted.unwrap();

        active.close().await.unwrap();
        sleep(Duration::from_millis(100)).await;

        write.write_all(b"stay").await.unwrap();
        let mut buf = [0u8; 4];
        peer_read.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"stay");

        drop(write);
        drop(read);
        drop(peer_read);
        drop(peer_write);

        sleep(Duration::from_millis(200)).await;
        assert_eq!(active.idle_pool_len_for_test(), 0);
        assert_eq!(reverse.idle_pool_len_for_test(), 0);
        assert_eq!(active.data_conn_count_for_test(), 0);
        assert_eq!(reverse.data_conn_count_for_test(), 0);
    }

    #[tokio::test]
    async fn tcp_tunnel_concurrent_create_creates_distinct_tunnels() {
        init_tls_once();

        let (client_network, client_resolver) = new_network();
        let (server_network, server_resolver) = new_network();

        let client_identity = new_identity("client-concurrent");
        let server_identity = new_identity("server-concurrent");

        register_listener_identity(&client_resolver, client_identity.clone()).await;
        register_listener_identity(&server_resolver, server_identity.clone()).await;

        client_network
            .listen(&loopback_tcp_ep(), None, None)
            .await
            .unwrap();
        server_network
            .listen(&loopback_tcp_ep(), None, None)
            .await
            .unwrap();

        let client_listener = client_network.listeners.lock().unwrap()[0].clone();
        let server_listener = server_network.listeners.lock().unwrap()[0].clone();
        let client_ep = client_listener.bound_local();
        let server_ep = server_listener.bound_local();
        let client_id = client_identity.get_id();
        let server_id = server_identity.get_id();
        let client_name = client_identity.get_name();
        let server_name = server_identity.get_name();

        let (client_created, server_created) = tokio::join!(
            client_network.create_tunnel(
                &client_identity,
                &server_ep,
                &server_id,
                Some(server_name.clone())
            ),
            server_network.create_tunnel(
                &server_identity,
                &client_ep,
                &client_id,
                Some(client_name.clone())
            )
        );

        let client_tunnel = client_created.unwrap();
        let server_tunnel = server_created.unwrap();
        assert_eq!(client_tunnel.remote_id(), server_id);
        assert_eq!(server_tunnel.remote_id(), client_id);

        let client_tunnels = client_network
            .registry
            .find_tunnels_for_test(&client_id, &server_id);
        let server_tunnels = server_network
            .registry
            .find_tunnels_for_test(&server_id, &client_id);

        assert_eq!(client_tunnels.len(), 2);
        assert_eq!(server_tunnels.len(), 2);
        assert_ne!(client_tunnels[0].tunnel_id(), client_tunnels[1].tunnel_id());
        assert_ne!(server_tunnels[0].tunnel_id(), server_tunnels[1].tunnel_id());
    }
}
