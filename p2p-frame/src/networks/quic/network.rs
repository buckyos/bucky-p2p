use super::listener::{QuicTunnelListener, connect_with_ep};
use super::tunnel::QuicTunnel;
use crate::endpoint::{Endpoint, Protocol};
use crate::error::{P2pError, P2pErrorCode, P2pResult, p2p_err};
use crate::networks::{
    QuicCongestionAlgorithm, TunnelConnectIntent, TunnelForm, TunnelListenerInfo,
    TunnelListenerRef, TunnelNetwork, TunnelRef,
};
use crate::p2p_identity::{
    P2pId, P2pIdentityCertCacheRef, P2pIdentityCertFactoryRef, P2pIdentityRef,
};
use crate::tls::ServerCertResolverRef;
use crate::types::{TunnelCandidateId, TunnelId, TunnelIdGenerator};
use rustls::pki_types::CertificateDer;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub struct QuicTunnelNetwork {
    listeners: Mutex<Vec<Arc<QuicTunnelListener>>>,
    cert_resolver: ServerCertResolverRef,
    cert_factory: P2pIdentityCertFactoryRef,
    cert_cache: P2pIdentityCertCacheRef,
    timeout: Duration,
    idle_timeout: Duration,
    congestion_algorithm: QuicCongestionAlgorithm,
    tunnel_id_gen: Mutex<TunnelIdGenerator>,
    reuse_address: AtomicBool,
}

impl QuicTunnelNetwork {
    pub fn new(
        cert_cache: P2pIdentityCertCacheRef,
        cert_resolver: ServerCertResolverRef,
        cert_factory: P2pIdentityCertFactoryRef,
        congestion_algorithm: QuicCongestionAlgorithm,
        timeout: Duration,
        idle_timeout: Duration,
    ) -> Self {
        Self {
            listeners: Mutex::new(Vec::new()),
            cert_resolver,
            cert_factory,
            cert_cache,
            timeout,
            idle_timeout,
            congestion_algorithm,
            tunnel_id_gen: Mutex::new(TunnelIdGenerator::new()),
            reuse_address: AtomicBool::new(false),
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

    async fn open_or_connect(
        &self,
        listener: &Arc<QuicTunnelListener>,
        local_identity: &P2pIdentityRef,
        remote: &Endpoint,
        remote_id: &P2pId,
        remote_name: Option<String>,
        intent: TunnelConnectIntent,
    ) -> P2pResult<TunnelRef> {
        let socket = connect_with_ep(
            listener.quic_ep(),
            local_identity.clone(),
            self.cert_factory.clone(),
            remote_id.clone(),
            remote_name.clone(),
            *remote,
            self.congestion_algorithm,
            self.timeout,
            self.idle_timeout,
        )
        .await?;
        let local_ep = Endpoint::from((
            Protocol::Quic,
            listener
                .quic_ep()
                .local_addr()
                .map_err(crate::error::into_p2p_err!(P2pErrorCode::TlsError))?,
        ));
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
    ) -> P2pResult<TunnelListenerRef> {
        let listener = QuicTunnelListener::new(
            self.cert_cache.clone(),
            self.cert_resolver.clone(),
            self.cert_factory.clone(),
            self.congestion_algorithm,
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
            Err(p2p_err!(
                P2pErrorCode::NotFound,
                "no listener found for remote: {}",
                remote
            ))
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

#[cfg(all(test, feature = "x509"))]
mod tests {
    use super::*;
    use crate::finder::{DeviceCache, DeviceCacheConfig};
    use crate::networks::{
        ListenVPortRegistry, Tunnel, TunnelListener, TunnelPurpose, allow_all_listen_vports,
    };
    use crate::runtime::{AsyncReadExt, AsyncWriteExt};
    use crate::tls::{DefaultTlsServerCertResolver, TlsServerCertResolver};
    use crate::x509::{
        X509IdentityCertFactory, X509IdentityFactory, generate_ed25519_x509_identity,
        generate_x509_identity,
    };
    use std::sync::Arc;
    use std::sync::Once;
    use tokio::time::timeout;

    static TLS_INIT: Once = Once::new();

    struct TestNetworkPair {
        client_network: QuicTunnelNetwork,
        client_identity: P2pIdentityRef,
        client_local_ep: Endpoint,
        server_network: QuicTunnelNetwork,
        server_identity: P2pIdentityRef,
        server_local_ep: Endpoint,
    }

    fn init_tls_once() {
        TLS_INIT.call_once(|| {
            crate::executor::Executor::init();
            crate::tls::init_tls(Arc::new(X509IdentityFactory));
        });
    }

    fn new_identity(name: &str) -> P2pIdentityRef {
        Arc::new(generate_x509_identity(Some(name.to_owned())).unwrap())
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
            .listen(&loopback_quic_ep(), None, None)
            .await
            .unwrap();
        server_network
            .listen(&loopback_quic_ep(), None, None)
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
        }
    }

    impl TestNetworkPair {
        async fn connect(&self) -> (TunnelRef, TunnelRef) {
            let server_listener = self.server_network.listeners()[0].clone();
            let accept_task =
                tokio::spawn(async move { server_listener.accept_tunnel().await.unwrap() });
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
            let accepted = accept_task.await.unwrap();
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
            .listen(&loopback_quic_ep(), None, None)
            .await
            .unwrap();
        server_network
            .listen(&loopback_quic_ep(), None, None)
            .await
            .unwrap();

        let server_listener = server_network.listeners()[0].clone();
        let server_ep = server_network.listener_infos()[0].local;

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
            .listen(&loopback_quic_ep(), None, None)
            .await
            .unwrap();
        server_network
            .listen(&loopback_quic_ep(), None, None)
            .await
            .unwrap();

        let server_listener = server_network.listeners()[0].clone();
        let server_ep = server_network.listener_infos()[0].local;

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
        assert_eq!(opened_a.remote_id(), server_identity_a.get_id());
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
        assert_eq!(opened_b.remote_id(), server_identity_b.get_id());
        assert_eq!(accepted_b.local_id(), server_identity_b.get_id());

        opened_a.close().await.unwrap();
        accepted_a.close().await.unwrap();
        opened_b.close().await.unwrap();
        accepted_b.close().await.unwrap();
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

        accepted
            .listen_stream(allow_all_listen_vports())
            .await
            .unwrap();

        let (opened_stream, accepted_stream) = tokio::join!(
            opened.open_stream(purpose_of(1001)),
            accepted.accept_stream()
        );
        let (mut read, mut write) = opened_stream.unwrap();
        let (purpose, mut peer_read, mut peer_write) = accepted_stream.unwrap();

        assert_eq!(purpose, purpose_of(1001));

        write.write_all(b"ping").await.unwrap();
        let mut buf = [0u8; 4];
        peer_read.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"ping");

        peer_write.write_all(b"pong").await.unwrap();
        let mut reply = [0u8; 4];
        read.read_exact(&mut reply).await.unwrap();
        assert_eq!(&reply, b"pong");

        opened.close().await.unwrap();
        accepted.close().await.unwrap();
    }

    #[tokio::test]
    async fn quic_tunnel_datagram_round_trip_ok() {
        let pair = setup_network_pair().await;
        let (opened, accepted) = pair.connect().await;

        accepted
            .listen_datagram(allow_all_listen_vports())
            .await
            .unwrap();

        let (opened_datagram, accepted_datagram) = tokio::join!(
            opened.open_datagram(purpose_of(2002)),
            accepted.accept_datagram()
        );
        let mut writer = opened_datagram.unwrap();
        let (purpose, mut peer_read) = accepted_datagram.unwrap();

        assert_eq!(purpose, purpose_of(2002));

        writer.write_all(b"hello").await.unwrap();
        writer.shutdown().await.unwrap();
        let mut buf = Vec::new();
        peer_read.read_to_end(&mut buf).await.unwrap();
        assert_eq!(buf, b"hello");

        opened.close().await.unwrap();
        accepted.close().await.unwrap();
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

        opened.close().await.unwrap();
        accepted.close().await.unwrap();
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

        opened.close().await.unwrap();
        accepted.close().await.unwrap();
    }

    #[tokio::test]
    async fn quic_tunnel_open_stream_to_unlistened_port_returns_error() {
        let pair = setup_network_pair().await;
        let (opened, accepted) = pair.connect().await;
        let empty_vports = ListenVPortRegistry::<()>::new();

        accepted
            .listen_stream(empty_vports.as_listen_vports_ref())
            .await
            .unwrap();

        let err = opened.open_stream(purpose_of(6553)).await.err().unwrap();
        assert_eq!(err.code(), P2pErrorCode::PortNotListen);

        opened.close().await.unwrap();
        accepted.close().await.unwrap();
    }

    #[tokio::test]
    async fn quic_tunnel_open_datagram_to_unlistened_port_returns_error() {
        let pair = setup_network_pair().await;
        let (opened, accepted) = pair.connect().await;
        let empty_vports = ListenVPortRegistry::<()>::new();

        accepted
            .listen_datagram(empty_vports.as_listen_vports_ref())
            .await
            .unwrap();

        let err = opened.open_datagram(purpose_of(6554)).await.err().unwrap();
        assert_eq!(err.code(), P2pErrorCode::PortNotListen);

        opened.close().await.unwrap();
        accepted.close().await.unwrap();
    }

    #[tokio::test]
    async fn quic_tunnel_accept_stream_requires_listen_first() {
        let pair = setup_network_pair().await;
        let (opened, accepted) = pair.connect().await;

        let err = accepted.accept_stream().await.err().unwrap();
        assert_eq!(err.code(), P2pErrorCode::Interrupted);

        opened.close().await.unwrap();
        accepted.close().await.unwrap();
    }

    #[tokio::test]
    async fn quic_tunnel_accept_datagram_requires_listen_first() {
        let pair = setup_network_pair().await;
        let (opened, accepted) = pair.connect().await;

        let err = accepted.accept_datagram().await.err().unwrap();
        assert_eq!(err.code(), P2pErrorCode::Interrupted);

        opened.close().await.unwrap();
        accepted.close().await.unwrap();
    }

    #[tokio::test]
    async fn quic_tunnel_create_tunnel_opens_new_connection_each_time() {
        let pair = setup_network_pair().await;

        let server_listener = pair.server_network.listeners()[0].clone();
        let first_accept_task =
            tokio::spawn(async move { server_listener.accept_tunnel().await.unwrap() });
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
        let first_accepted = first_accept_task.await.unwrap();

        let server_listener = pair.server_network.listeners()[0].clone();
        let second_accept_task =
            tokio::spawn(async move { server_listener.accept_tunnel().await.unwrap() });
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
        let second_accepted = second_accept_task.await.unwrap();

        assert_ne!(Arc::as_ptr(&first_opened), Arc::as_ptr(&second_opened));
        assert_ne!(Arc::as_ptr(&first_accepted), Arc::as_ptr(&second_accepted));

        first_opened.close().await.unwrap();
        second_opened.close().await.unwrap();
        first_accepted.close().await.unwrap();
        second_accepted.close().await.unwrap();
    }

    #[tokio::test]
    async fn quic_tunnel_close_all_listener_clears_listeners() {
        let pair = setup_network_pair().await;

        let server_listener = pair.server_network.listeners()[0].clone();
        let accept_task =
            tokio::spawn(async move { server_listener.accept_tunnel().await.unwrap() });
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
        let accepted = accept_task.await.unwrap();

        pair.client_network.close_all_listener().await.unwrap();
        assert!(pair.client_network.listeners().is_empty());
        assert!(pair.client_network.listener_infos().is_empty());

        accepted.close().await.unwrap();
    }

    #[tokio::test]
    async fn quic_tunnel_create_tunnel_no_listener_returns_not_found() {
        init_tls_once();

        let (network, _resolver) = new_network();
        let local_identity = new_identity("quic-no-listener-client");

        let ret = network
            .create_tunnel(
                &local_identity,
                &Endpoint::from((Protocol::Quic, "127.0.0.1:44500".parse().unwrap())),
                &P2pId::default(),
                None,
            )
            .await;

        assert!(ret.is_err());
        assert_eq!(ret.err().unwrap().code(), P2pErrorCode::NotFound);
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
        assert_eq!(pair.client_network.listeners().len(), 1);

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
            .listen(&loopback_quic_ep(), None, None)
            .await
            .unwrap();
        server_network
            .listen(&loopback_quic_ep(), None, None)
            .await
            .unwrap();

        let client_local_ep = client_network.listener_infos()[0].local;
        let server_local_ep = server_network.listener_infos()[0].local;
        let server_listener = server_network.listeners()[0].clone();
        let accept_task =
            tokio::spawn(async move { server_listener.accept_tunnel().await.unwrap() });
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
        let accepted = accept_task.await.unwrap();

        assert_eq!(opened.remote_id(), server_identity.get_id());
        assert_eq!(accepted.local_id(), server_identity.get_id());

        opened.close().await.unwrap();
        accepted.close().await.unwrap();
    }
}
