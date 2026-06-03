use super::connection::{TcpTlsConnection, accept_connection, build_acceptor};
use super::protocol::{
    DataConnReady, DataConnReadyResult, TcpConnectionHello, TcpConnectionRole, write_raw_frame,
};
use super::tunnel::{TcpIncomingControlDecision, TcpTunnel, TcpTunnelConnector};
use crate::endpoint::Endpoint;
use crate::error::{P2pErrorCode, P2pResult, into_p2p_err, p2p_err};
use crate::executor::Executor;
use crate::networks::{IncomingTunnelCallback, Tunnel, TunnelForm, TunnelRef};
use crate::p2p_identity::{P2pId, P2pIdentityCertFactoryRef};
use crate::runtime;
use crate::tls::{ServerCertResolverRef, TlsServerCertResolver};
use crate::types::{TunnelCandidateId, TunnelId};
use sfo_reuseport::{ServerRuntime, SocketOptions, TcpServer, TcpServiceConfig};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

type TunnelKey = (P2pId, P2pId, TunnelId, TunnelCandidateId);
const REGISTRY_CLEANUP_INTERVAL: Duration = Duration::from_secs(300);

pub(crate) struct TcpTunnelRegistry {
    by_tunnel: Mutex<HashMap<TunnelKey, Weak<TcpTunnel>>>,
}

impl TcpTunnelRegistry {
    pub(crate) fn new() -> Arc<Self> {
        let registry = Arc::new(Self {
            by_tunnel: Mutex::new(HashMap::new()),
        });
        Self::start_cleanup_task(&registry);
        registry
    }

    fn start_cleanup_task(registry: &Arc<Self>) {
        let weak = Arc::downgrade(registry);
        let _ = Executor::spawn(async move {
            loop {
                runtime::sleep(REGISTRY_CLEANUP_INTERVAL).await;
                let Some(registry) = weak.upgrade() else {
                    break;
                };
                let removed = registry.cleanup_stale();
                if removed > 0 {
                    log::debug!("tcp tunnel registry cleaned {} stale entries", removed);
                }
            }
        });
    }

    fn cleanup_stale(&self) -> usize {
        let mut by_tunnel = self.by_tunnel.lock().unwrap();
        let before = by_tunnel.len();
        by_tunnel.retain(|_, weak| matches!(weak.upgrade(), Some(tunnel) if !tunnel.is_closed()));
        before.saturating_sub(by_tunnel.len())
    }

    pub(crate) fn register(
        &self,
        tunnel: Arc<TcpTunnel>,
        tunnel_id: TunnelId,
        candidate_id: TunnelCandidateId,
    ) {
        self.by_tunnel.lock().unwrap().insert(
            (
                tunnel.local_id(),
                tunnel.remote_id(),
                tunnel_id,
                candidate_id,
            ),
            Arc::downgrade(&tunnel),
        );
    }

    pub(crate) fn find_tunnel(
        &self,
        local_id: &P2pId,
        remote_id: &P2pId,
        tunnel_id: TunnelId,
        candidate_id: TunnelCandidateId,
    ) -> Option<Arc<TcpTunnel>> {
        let key = (local_id.clone(), remote_id.clone(), tunnel_id, candidate_id);
        let mut by_tunnel = self.by_tunnel.lock().unwrap();
        match by_tunnel.get(&key).cloned() {
            Some(weak) => match weak.upgrade() {
                Some(tunnel) if !tunnel.is_closed() => Some(tunnel),
                _ => {
                    by_tunnel.remove(&key);
                    None
                }
            },
            None => None,
        }
    }
}

struct TcpTunnelListenerState {
    local: Option<Endpoint>,
    outer: Option<Endpoint>,
    server: Option<TcpServer>,
    bound_local: Option<Endpoint>,
    mapping_port: Option<u16>,
}

pub(crate) struct TcpTunnelListener {
    cert_resolver: ServerCertResolverRef,
    cert_factory: P2pIdentityCertFactoryRef,
    acceptor: crate::runtime::TlsAcceptor,
    state: Mutex<TcpTunnelListenerState>,
    on_incoming_tunnel: IncomingTunnelCallback,
    closed: AtomicBool,
    registry: Arc<TcpTunnelRegistry>,
    timeout: Duration,
    heartbeat_interval: Duration,
    heartbeat_timeout: Duration,
    server_runtime: ServerRuntime,
}

impl TcpTunnelListener {
    pub(crate) fn new(
        cert_resolver: ServerCertResolverRef,
        cert_factory: P2pIdentityCertFactoryRef,
        registry: Arc<TcpTunnelRegistry>,
        timeout: Duration,
        heartbeat_interval: Duration,
        heartbeat_timeout: Duration,
        server_runtime: ServerRuntime,
        on_incoming_tunnel: IncomingTunnelCallback,
    ) -> Arc<Self> {
        Arc::new(Self {
            acceptor: build_acceptor(cert_resolver.clone(), cert_factory.clone()),
            cert_resolver,
            cert_factory,
            state: Mutex::new(TcpTunnelListenerState {
                local: None,
                outer: None,
                server: None,
                bound_local: None,
                mapping_port: None,
            }),
            on_incoming_tunnel,
            closed: AtomicBool::new(false),
            registry,
            timeout,
            heartbeat_interval,
            heartbeat_timeout,
            server_runtime,
        })
    }

    pub(crate) async fn start(
        self: &Arc<Self>,
        local: Endpoint,
        out: Option<Endpoint>,
        mapping_port: Option<u16>,
        reuse_address: bool,
    ) -> P2pResult<()> {
        let listener = self.clone();
        let bind_local = resolve_tcp_bind_endpoint(local)?;
        let config = TcpServiceConfig::new(*bind_local.addr()).with_socket_options(SocketOptions {
            reuse_address,
            ..SocketOptions::default()
        });
        let server = TcpServer::serve(&self.server_runtime, config, move |stream| {
            let listener = listener.clone();
            async move {
                listener.handle_accepted_stream(stream).await;
                Ok(())
            }
        })
        .map_err(into_p2p_err!(
            P2pErrorCode::AlreadyExists,
            "bind tcp listener {} error",
            local
        ))?;
        let mut state = self.state.lock().unwrap();
        state.local = Some(local);
        state.outer = out;
        state.mapping_port = mapping_port;
        state.bound_local = Some(bind_local);
        state.server = Some(server);
        Ok(())
    }

    pub(crate) fn local(&self) -> Endpoint {
        self.state.lock().unwrap().local.unwrap()
    }

    pub(crate) fn bound_local(&self) -> Endpoint {
        self.state.lock().unwrap().bound_local.unwrap()
    }

    pub(crate) fn mapping_port(&self) -> Option<u16> {
        self.state.lock().unwrap().mapping_port
    }

    pub(crate) fn close(&self) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        let server = {
            let mut state = self.state.lock().unwrap();
            state.server.take()
        };
        if let Some(server) = server {
            let _ = server.close();
        }
    }

    fn validate_control_hello(&self, hello: &TcpConnectionHello) -> TcpIncomingControlDecision {
        if hello.role != TcpConnectionRole::Control
            || hello.conn_id.is_some()
            || hello.open_request_id.is_some()
        {
            TcpIncomingControlDecision::ProtocolError
        } else {
            TcpIncomingControlDecision::Accept
        }
    }

    async fn on_control_connection(
        &self,
        hello: TcpConnectionHello,
        connection: TcpTlsConnection,
    ) -> P2pResult<Option<TunnelRef>> {
        let local_identity = self
            .cert_resolver
            .get_server_identity(connection.local_name.as_str())
            .await
            .ok_or_else(|| p2p_err!(P2pErrorCode::CertError, "local identity not found"))?;
        let connector = TcpTunnelConnector {
            cert_factory: self.cert_factory.clone(),
            local_identity,
            local_ep: self.local(),
            remote_ep: Arc::new(Mutex::new(connection.remote_ep)),
            remote_id: connection.remote_id.clone(),
            remote_name: Some(connection.remote_name.clone()),
            timeout: self.timeout,
            tunnel_id: hello.tunnel_id,
            candidate_id: hello.candidate_id,
        };

        let decision = self.validate_control_hello(&hello);

        let accepted = TcpTunnel::accept_control_connection(
            connection,
            connector,
            hello.tunnel_id,
            hello.candidate_id,
            hello.is_reverse,
            decision,
            self.heartbeat_interval,
            self.heartbeat_timeout,
        )
        .await?;

        if let Some(tunnel) = &accepted {
            self.registry
                .register(tunnel.clone(), hello.tunnel_id, hello.candidate_id);
        }
        Ok(accepted.map(|t| t as TunnelRef))
    }

    async fn on_data_connection(
        &self,
        hello: TcpConnectionHello,
        mut connection: TcpTlsConnection,
    ) -> P2pResult<()> {
        let Some(tunnel) = self.registry.find_tunnel(
            &connection.local_id,
            &connection.remote_id,
            hello.tunnel_id,
            hello.candidate_id,
        ) else {
            write_raw_frame(
                &mut connection.stream,
                &DataConnReady {
                    conn_id: hello.conn_id.unwrap_or_default(),
                    candidate_id: hello.candidate_id,
                    result: DataConnReadyResult::TunnelNotFound,
                },
            )
            .await?;
            return Err(p2p_err!(P2pErrorCode::NotFound, "tcp tunnel not found"));
        };
        tunnel.on_incoming_data_connection(hello, connection).await
    }

    async fn deliver_incoming(&self, result: P2pResult<TunnelRef>) {
        if self.closed.load(Ordering::SeqCst) {
            if let Ok(tunnel) = result {
                let _ = tunnel.close();
            }
            return;
        }
        (self.on_incoming_tunnel)(result).await;
    }

    async fn handle_accepted_stream(self: Arc<Self>, stream: crate::runtime::TcpStream) {
        match accept_connection(
            &self.acceptor,
            &self.cert_factory,
            &self.cert_resolver,
            stream,
        )
        .await
        {
            Ok((connection, hello)) => match hello.role {
                TcpConnectionRole::Control => {
                    match self.on_control_connection(hello, connection).await {
                        Ok(Some(tunnel)) => {
                            self.deliver_incoming(Ok(tunnel)).await;
                        }
                        Ok(None) => {}
                        Err(err) => {
                            self.deliver_incoming(Err(err)).await;
                        }
                    }
                }
                TcpConnectionRole::Data => {
                    if let Err(err) = self.on_data_connection(hello, connection).await {
                        log::warn!("tcp tunnel data accept failed: {:?}", err);
                    }
                }
            },
            Err(err) => {
                log::warn!("tcp tunnel listener handshake failed: {:?}", err);
            }
        }
    }
}

fn resolve_tcp_bind_endpoint(local: Endpoint) -> P2pResult<Endpoint> {
    if local.addr().port() != 0 {
        return Ok(local);
    }
    let probe = std::net::TcpListener::bind(local.addr()).map_err(into_p2p_err!(
        P2pErrorCode::AlreadyExists,
        "allocate tcp listener port {} error",
        local
    ))?;
    let bound_addr = probe.local_addr().map_err(into_p2p_err!(
        P2pErrorCode::IoError,
        "get allocated tcp listener address failed"
    ))?;
    drop(probe);
    Ok(Endpoint::from((crate::endpoint::Protocol::Tcp, bound_addr)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::p2p_identity::{
        EncodedP2pIdentityCert, P2pIdentityCert, P2pIdentityCertFactory, P2pIdentityCertRef,
        P2pIdentitySignType, P2pSignature,
    };
    use crate::tls::DefaultTlsServerCertResolver;
    use sfo_reuseport::{ServerRuntime, ServerRuntimeConfig};
    use std::sync::{Arc, Once};

    static EXECUTOR_INIT: Once = Once::new();

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

    fn listener() -> Arc<TcpTunnelListener> {
        EXECUTOR_INIT.call_once(crate::executor::Executor::init);
        let callback: IncomingTunnelCallback = Arc::new(|_| Box::pin(async {}));
        TcpTunnelListener::new(
            DefaultTlsServerCertResolver::new(),
            Arc::new(TestCertFactory),
            TcpTunnelRegistry::new(),
            Duration::from_secs(1),
            Duration::from_secs(5),
            Duration::from_secs(30),
            ServerRuntime::start(ServerRuntimeConfig::default())
                .expect("sfo reuseport server runtime should start"),
            callback,
        )
    }

    fn control_hello() -> TcpConnectionHello {
        TcpConnectionHello {
            role: TcpConnectionRole::Control,
            tunnel_id: TunnelId::from(7),
            candidate_id: TunnelCandidateId::from(11),
            is_reverse: false,
            conn_id: None,
            open_request_id: None,
        }
    }

    #[test]
    fn tcp_listener_accepts_only_control_hello_without_data_fields() {
        let listener = listener();
        let mut hello = control_hello();
        assert_eq!(
            listener.validate_control_hello(&hello),
            TcpIncomingControlDecision::Accept
        );

        hello.role = TcpConnectionRole::Data;
        assert_eq!(
            listener.validate_control_hello(&hello),
            TcpIncomingControlDecision::ProtocolError
        );

        hello = control_hello();
        hello.conn_id = Some(TunnelId::from(12));
        assert_eq!(
            listener.validate_control_hello(&hello),
            TcpIncomingControlDecision::ProtocolError
        );

        hello = control_hello();
        hello.open_request_id = Some(TunnelId::from(13));
        assert_eq!(
            listener.validate_control_hello(&hello),
            TcpIncomingControlDecision::ProtocolError
        );
    }

    #[test]
    fn tcp_listener_zero_port_bind_resolution_allocates_tcp_endpoint() {
        let local = Endpoint::from((
            crate::endpoint::Protocol::Tcp,
            "127.0.0.1:0".parse().unwrap(),
        ));
        let resolved = resolve_tcp_bind_endpoint(local).unwrap();

        assert_eq!(resolved.protocol(), crate::endpoint::Protocol::Tcp);
        assert!(resolved.addr().is_ipv4());
        assert_ne!(resolved.addr().port(), 0);
    }
}
