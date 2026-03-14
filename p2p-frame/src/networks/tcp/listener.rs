use super::connection::{TcpTlsConnection, accept_connection, bind_listener, build_acceptor};
use super::protocol::{
    DataConnReady, DataConnReadyResult, TcpConnectionHello, TcpConnectionRole, write_raw_frame,
};
use super::tunnel::{TcpIncomingControlDecision, TcpTunnel, TcpTunnelConnector};
use crate::endpoint::Endpoint;
use crate::error::{P2pErrorCode, P2pResult, p2p_err};
use crate::executor::{Executor, SpawnHandle};
use crate::networks::{Tunnel, TunnelForm, TunnelListener, TunnelRef};
use crate::p2p_identity::{P2pId, P2pIdentityCertFactoryRef};
use crate::runtime::{self, TcpListener};
use crate::tls::{ServerCertResolverRef, TlsServerCertResolver};
use crate::types::{TunnelCandidateId, TunnelId};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;
use tokio::sync::{Mutex as AsyncMutex, Notify, mpsc};

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

    #[cfg(test)]
    pub(crate) fn find_tunnels_for_test(
        &self,
        local_id: &P2pId,
        remote_id: &P2pId,
    ) -> Vec<Arc<TcpTunnel>> {
        let _ = self.cleanup_stale();
        let by_tunnel = self.by_tunnel.lock().unwrap();
        let mut tunnels = Vec::new();
        for ((key_local_id, key_remote_id, _, _), weak) in by_tunnel.iter() {
            if key_local_id != local_id || key_remote_id != remote_id {
                continue;
            }
            if let Some(tunnel) = weak.upgrade() {
                tunnels.push(tunnel);
            }
        }
        tunnels.sort_by_key(|tunnel| (tunnel.tunnel_id().value(), tunnel.candidate_id().value()));
        tunnels
    }
}

struct TcpTunnelListenerState {
    local: Option<Endpoint>,
    outer: Option<Endpoint>,
    socket: Option<Arc<TcpListener>>,
    mapping_port: Option<u16>,
}

pub(crate) struct TcpTunnelListener {
    cert_resolver: ServerCertResolverRef,
    cert_factory: P2pIdentityCertFactoryRef,
    acceptor: crate::runtime::TlsAcceptor,
    state: Mutex<TcpTunnelListenerState>,
    accepted_tx: Mutex<Option<mpsc::UnboundedSender<P2pResult<TunnelRef>>>>,
    accepted_rx: AsyncMutex<mpsc::UnboundedReceiver<P2pResult<TunnelRef>>>,
    accept_task: Mutex<Option<SpawnHandle<()>>>,
    close_notify: Notify,
    closed: AtomicBool,
    registry: Arc<TcpTunnelRegistry>,
    timeout: Duration,
    heartbeat_interval: Duration,
    heartbeat_timeout: Duration,
    #[cfg(test)]
    forced_control_decision: Mutex<Option<TcpIncomingControlDecision>>,
}

impl TcpTunnelListener {
    pub(crate) fn new(
        cert_resolver: ServerCertResolverRef,
        cert_factory: P2pIdentityCertFactoryRef,
        registry: Arc<TcpTunnelRegistry>,
        timeout: Duration,
        heartbeat_interval: Duration,
        heartbeat_timeout: Duration,
    ) -> Arc<Self> {
        let (accepted_tx, accepted_rx) = mpsc::unbounded_channel();
        Arc::new(Self {
            acceptor: build_acceptor(cert_resolver.clone(), cert_factory.clone()),
            cert_resolver,
            cert_factory,
            state: Mutex::new(TcpTunnelListenerState {
                local: None,
                outer: None,
                socket: None,
                mapping_port: None,
            }),
            accepted_tx: Mutex::new(Some(accepted_tx)),
            accepted_rx: AsyncMutex::new(accepted_rx),
            accept_task: Mutex::new(None),
            close_notify: Notify::new(),
            closed: AtomicBool::new(false),
            registry,
            timeout,
            heartbeat_interval,
            heartbeat_timeout,
            #[cfg(test)]
            forced_control_decision: Mutex::new(None),
        })
    }

    #[cfg(test)]
    pub(crate) fn set_incoming_control_decision_for_test(
        &self,
        decision: Option<TcpIncomingControlDecision>,
    ) {
        *self.forced_control_decision.lock().unwrap() = decision;
    }

    pub(crate) async fn bind(
        &self,
        local: Endpoint,
        out: Option<Endpoint>,
        mapping_port: Option<u16>,
        reuse_address: bool,
    ) -> P2pResult<()> {
        let socket = bind_listener(local, reuse_address).await?;
        let mut state = self.state.lock().unwrap();
        state.local = Some(local);
        state.outer = out;
        state.mapping_port = mapping_port;
        state.socket = Some(Arc::new(socket));
        Ok(())
    }

    pub(crate) fn local(&self) -> Endpoint {
        self.state.lock().unwrap().local.unwrap()
    }

    pub(crate) fn bound_local(&self) -> Endpoint {
        let socket = self.state.lock().unwrap().socket.as_ref().unwrap().clone();
        Endpoint::from((crate::endpoint::Protocol::Tcp, socket.local_addr().unwrap()))
    }

    pub(crate) fn mapping_port(&self) -> Option<u16> {
        self.state.lock().unwrap().mapping_port
    }

    pub(crate) fn close(&self) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        self.close_notify.notify_waiters();
        self.accepted_tx.lock().unwrap().take();
        if let Some(task) = self.accept_task.lock().unwrap().take() {
            task.abort();
        }
        let socket = {
            let mut state = self.state.lock().unwrap();
            state.socket.take()
        };
        if let Some(socket) = socket {
            close_tcp_listener_socket(socket.as_ref());
        }
    }

    fn send_accepted(&self, result: P2pResult<TunnelRef>) {
        if let Some(tx) = self.accepted_tx.lock().unwrap().clone() {
            let _ = tx.send(result);
        }
    }

    fn validate_control_hello(&self, hello: &TcpConnectionHello) -> TcpIncomingControlDecision {
        #[cfg(test)]
        if let Some(decision) = *self.forced_control_decision.lock().unwrap() {
            return decision;
        }
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

    pub(crate) fn start(self: &Arc<Self>) {
        if self.closed.load(Ordering::SeqCst) {
            return;
        }
        let this = self.clone();
        let socket = self.state.lock().unwrap().socket.as_ref().unwrap().clone();
        let handle = Executor::spawn_with_handle(async move {
            loop {
                let accepted = socket.accept().await;
                let (stream, _) = match accepted {
                    Ok(v) => v,
                    Err(err) => {
                        log::warn!("tcp tunnel listener accept failed: {:?}", err);
                        break;
                    }
                };
                let this2 = this.clone();
                let _ = Executor::spawn(async move {
                    match accept_connection(
                        &this2.acceptor,
                        &this2.cert_factory,
                        &this2.cert_resolver,
                        stream,
                    )
                    .await
                    {
                        Ok((connection, hello)) => match hello.role {
                            TcpConnectionRole::Control => {
                                match this2.on_control_connection(hello, connection).await {
                                    Ok(Some(tunnel)) => {
                                        this2.send_accepted(Ok(tunnel));
                                    }
                                    Ok(None) => {}
                                    Err(err) => {
                                        this2.send_accepted(Err(err));
                                    }
                                }
                            }
                            TcpConnectionRole::Data => {
                                if let Err(err) = this2.on_data_connection(hello, connection).await
                                {
                                    log::warn!("tcp tunnel data accept failed: {:?}", err);
                                }
                            }
                        },
                        Err(err) => {
                            log::warn!("tcp tunnel listener handshake failed: {:?}", err);
                        }
                    }
                });
            }
        })
        .unwrap();
        *self.accept_task.lock().unwrap() = Some(handle);
    }
}

#[async_trait::async_trait]
impl TunnelListener for TcpTunnelListener {
    async fn accept_tunnel(&self) -> P2pResult<TunnelRef> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(p2p_err!(P2pErrorCode::Interrupted, "accept tunnel closed"));
        }
        let mut rx = self.accepted_rx.lock().await;
        let closed = self.close_notify.notified();
        tokio::pin!(closed);
        tokio::select! {
            result = rx.recv() => {
                result.ok_or_else(|| p2p_err!(P2pErrorCode::Interrupted, "accept tunnel closed"))?
            }
            _ = &mut closed => {
                Err(p2p_err!(P2pErrorCode::Interrupted, "accept tunnel closed"))
            }
        }
    }
}

fn close_tcp_listener_socket(socket: &TcpListener) {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawSocket;
        use winapi::um::winsock2::closesocket;

        unsafe {
            let raw = socket.as_raw_socket();
            closesocket(raw.try_into().unwrap());
        }
    }
    #[cfg(not(windows))]
    {
        #[cfg(feature = "runtime-tokio")]
        {
            use std::os::fd::AsRawFd;

            unsafe {
                let raw = socket.as_raw_fd();
                libc::close(raw.try_into().unwrap());
            }
        }
    }
}
