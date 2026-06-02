use super::tunnel::QuicTunnel;
use crate::endpoint::{Endpoint, EndpointArea, Protocol, is_non_lan_ipv4_addr};
use crate::error::{P2pErrorCode, P2pResult, into_p2p_err, p2p_err};
use crate::executor::Executor;
use crate::finder::DeviceCache;
use crate::networks::{
    IncomingTunnelCallback, QuicCongestionAlgorithm, TunnelConnectIntent, TunnelForm, TunnelRef,
};
use crate::p2p_identity::{
    P2pId, P2pIdentityCertCacheRef, P2pIdentityCertFactoryRef, P2pIdentityRef,
};
use crate::runtime;
use crate::tls::{ServerCertResolverRef, TlsServerCertResolver};
use quinn::Incoming;
use quinn::crypto::rustls::{HandshakeData, QuicServerConfig};
use rand::{Rng, random};
use rustls::pki_types::CertificateDer;
use rustls::version::TLS13;
use sfo_reuseport::{
    Error as SfoReuseportError, QuicCidGenerator, QuicServer, ServerRuntime, SocketOptions,
    UdpServiceConfig, UdpSocket as SfoUdpSocket,
};
use std::io::{self, IoSliceMut};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::sync::{Notify, mpsc, oneshot};

const UDP_PUNCH_PAYLOAD_MIN_LEN: usize = 5;
const UDP_PUNCH_PAYLOAD_MAX_LEN: usize = 30;
const UDP_PUNCH_MAGIC: &[u8] = b"\x00#@$QUIC";
const UDP_PUNCH_INTERVAL: Duration = Duration::from_millis(50);
const UDP_PUNCH_ACTIVE_START_OFFSET: Duration = Duration::from_millis(250);
const UDP_PUNCH_REVERSE_START_OFFSET: Duration = Duration::ZERO;
const UDP_PUNCH_DEADLINE: Duration = Duration::from_secs(1);
const QUIC_ENDPOINT_READY_TIMEOUT: Duration = Duration::from_secs(2);

struct SfoQuicUdpSocket {
    socket: SfoUdpSocket,
    worker_id: usize,
    worker_count: Arc<AtomicUsize>,
}

impl SfoQuicUdpSocket {
    fn new(socket: SfoUdpSocket, worker_id: usize, worker_count: Arc<AtomicUsize>) -> Self {
        Self {
            socket,
            worker_id,
            worker_count,
        }
    }
}

fn quic_packet_worker_index_prefix(packet: &[u8]) -> Option<usize> {
    if packet.is_empty() {
        return None;
    }

    let dcid = if packet[0] & 0x80 != 0 {
        if matches!(packet[0] & 0x30, 0x00 | 0x10) {
            return None;
        }
        let dcid_len = usize::from(*packet.get(5)?);
        if dcid_len == 0 {
            return None;
        }
        packet.get(6..6 + dcid_len)?
    } else {
        packet.get(1..)?
    };

    let high = *dcid.first()?;
    let low = *dcid.get(1)?;
    Some((usize::from(high) << 8) | usize::from(low))
}

fn quic_packet_worker_index(packet: &[u8], worker_count: usize) -> Option<usize> {
    if worker_count == 0 {
        return None;
    }
    quic_packet_worker_index_prefix(packet).map(|worker_index| worker_index % worker_count)
}

fn quic_packet_prefix<'a>(
    bufs: &'a [IoSliceMut<'_>],
    len: usize,
    out: &'a mut Vec<u8>,
) -> &'a [u8] {
    if let Some(first) = bufs.first() {
        if first.len() >= len {
            return &first[..len];
        }
    }

    out.clear();
    out.reserve(len);
    let mut remaining = len;
    for buf in bufs {
        if remaining == 0 {
            break;
        }
        let copy_len = remaining.min(buf.len());
        out.extend_from_slice(&buf[..copy_len]);
        remaining -= copy_len;
    }
    out.as_slice()
}

fn is_udp_punch_payload(packet: &[u8]) -> bool {
    packet.starts_with(UDP_PUNCH_MAGIC)
}

impl std::fmt::Debug for SfoQuicUdpSocket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SfoQuicUdpSocket").finish_non_exhaustive()
    }
}

impl quinn::AsyncUdpSocket for SfoQuicUdpSocket {
    fn create_io_poller(self: Arc<Self>) -> Pin<Box<dyn quinn::UdpPoller>> {
        Box::pin(SfoQuicUdpPoller { socket: self })
    }

    fn try_send(&self, transmit: &quinn::udp::Transmit) -> io::Result<()> {
        match transmit.segment_size {
            Some(segment_size) if segment_size > 0 => {
                for chunk in transmit.contents.chunks(segment_size) {
                    let sent = self.socket.try_send_to(chunk, transmit.destination)?;
                    if sent != chunk.len() {
                        return Err(io::Error::new(
                            io::ErrorKind::WriteZero,
                            "short quic udp send",
                        ));
                    }
                }
                Ok(())
            }
            _ => {
                let sent = self
                    .socket
                    .try_send_to(transmit.contents, transmit.destination)?;
                if sent == transmit.contents.len() {
                    Ok(())
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "short quic udp send",
                    ))
                }
            }
        }
    }

    fn poll_recv(
        &self,
        cx: &mut Context,
        bufs: &mut [IoSliceMut<'_>],
        meta: &mut [quinn::udp::RecvMeta],
    ) -> Poll<io::Result<usize>> {
        if bufs.is_empty() || meta.is_empty() {
            return Poll::Ready(Ok(0));
        }
        loop {
            match self.socket.poll_recv_from_vectored(cx, bufs) {
                Poll::Ready(Ok((len, peer_addr))) => {
                    let mut packet = Vec::new();
                    let packet = quic_packet_prefix(bufs, len, &mut packet);
                    if is_udp_punch_payload(packet) {
                        continue;
                    }
                    let worker_count = self.worker_count.load(Ordering::Acquire);
                    if let Some(worker_index) = quic_packet_worker_index(packet, worker_count) {
                        assert_eq!(
                            worker_index, self.worker_id,
                            "quic packet dcid worker index does not match sfo worker socket"
                        );
                    }
                    meta[0] = quinn::udp::RecvMeta {
                        addr: peer_addr,
                        len,
                        stride: len,
                        ecn: None,
                        dst_ip: None,
                    };
                    return Poll::Ready(Ok(1));
                }
                Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
                Poll::Pending => return Poll::Pending,
            }
        }
    }

    fn local_addr(&self) -> io::Result<std::net::SocketAddr> {
        self.socket
            .local_addr()
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))
    }
}

struct SfoQuicUdpPoller {
    socket: Arc<SfoQuicUdpSocket>,
}

impl std::fmt::Debug for SfoQuicUdpPoller {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SfoQuicUdpPoller").finish()
    }
}

impl quinn::UdpPoller for SfoQuicUdpPoller {
    fn poll_writable(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        self.socket.socket.poll_send_ready(cx)
    }
}

#[derive(Clone, Debug)]
struct WorkerQuicCidGenerator {
    inner: QuicCidGenerator,
}

impl WorkerQuicCidGenerator {
    fn for_worker(worker_id: usize) -> P2pResult<Self> {
        let inner = QuicCidGenerator::for_worker(worker_id).map_err(into_p2p_err!(
            P2pErrorCode::InvalidParam,
            "create quic cid generator failed"
        ))?;
        Ok(Self { inner })
    }
}

impl quinn::ConnectionIdGenerator for WorkerQuicCidGenerator {
    fn generate_cid(&mut self) -> quinn::ConnectionId {
        let cid = self
            .inner
            .generate()
            .expect("sfo quic cid generation should not fail after validation");
        quinn::ConnectionId::new(cid.as_slice())
    }

    fn cid_len(&self) -> usize {
        self.inner.cid_len()
    }

    fn cid_lifetime(&self) -> Option<Duration> {
        None
    }
}

fn new_quic_endpoint_config(worker_id: usize) -> P2pResult<quinn::EndpointConfig> {
    let generator = WorkerQuicCidGenerator::for_worker(worker_id)?;
    let mut endpoint_config = quinn::EndpointConfig::default();
    endpoint_config.cid_generator(move || Box::new(generator.clone()));
    Ok(endpoint_config)
}

async fn wait_quic_endpoint_ready(listener: &QuicTunnelListener) -> P2pResult<()> {
    if !listener.state.read().unwrap().endpoints.is_empty() {
        return Ok(());
    }
    let ready = listener.endpoint_ready.notified();
    tokio::pin!(ready);
    match runtime::timeout(QUIC_ENDPOINT_READY_TIMEOUT, &mut ready).await {
        Ok(_) => Ok(()),
        Err(_) if listener.state.read().unwrap().endpoints.is_empty() => Err(p2p_err!(
            P2pErrorCode::QuicError,
            "quic endpoint did not become ready"
        )),
        Err(_) => Ok(()),
    }
}

struct QuicConnectRequest {
    local_identity_ref: P2pIdentityRef,
    cert_factory: P2pIdentityCertFactoryRef,
    remote_identity_id: P2pId,
    remote_name: Option<String>,
    remote: Endpoint,
    congestion_algorithm: QuicCongestionAlgorithm,
    timeout: Duration,
    idle_timeout: Duration,
    response: oneshot::Sender<P2pResult<quinn::Connection>>,
}

#[derive(Clone)]
struct WorkerQuicEndpoint {
    endpoint: quinn::Endpoint,
    connect_tx: mpsc::Sender<QuicConnectRequest>,
}

async fn wait_quic_endpoint_loop(
    listener: Arc<QuicTunnelListener>,
    endpoint: quinn::Endpoint,
    mut connect_rx: mpsc::Receiver<QuicConnectRequest>,
) {
    loop {
        tokio::select! {
            conn = endpoint.accept() => match conn {
                Some(conn) => {
                let result = listener.accept_connection(conn).await;
                (listener.on_incoming_tunnel)(result).await;
            }
                None => break,
            },
            request = connect_rx.recv() => {
                let Some(request) = request else {
                    break;
                };
                let endpoint = endpoint.clone();
                tokio::spawn(async move {
                    let result = connect_with_ep(
                        endpoint,
                        request.local_identity_ref,
                        request.cert_factory,
                        request.remote_identity_id,
                        request.remote_name,
                        request.remote,
                        request.congestion_algorithm,
                        request.timeout,
                        request.idle_timeout,
                    )
                    .await;
                    let _ = request.response.send(result);
                });
            }
        }
    }
}

struct QuicTunnelListenerState {
    local: Option<Endpoint>,
    outer: Option<Endpoint>,
    endpoints: Vec<WorkerQuicEndpoint>,
    server: Option<QuicServer>,
    punch_socket: Option<SfoUdpSocket>,
    mapping_port: Option<u16>,
    reuse_address: bool,
}

pub(crate) struct QuicTunnelListener {
    cert_cache: P2pIdentityCertCacheRef,
    cert_resolver: ServerCertResolverRef,
    cert_factory: P2pIdentityCertFactoryRef,
    congestion_algorithm: QuicCongestionAlgorithm,
    state: RwLock<QuicTunnelListenerState>,
    on_incoming_tunnel: IncomingTunnelCallback,
    close_notify: Notify,
    endpoint_ready: Notify,
    closed: AtomicBool,
    worker_count: Arc<AtomicUsize>,
    server_runtime: ServerRuntime,
    listener_connect_capacity: usize,
}

impl QuicTunnelListener {
    pub(crate) fn new(
        cert_cache: P2pIdentityCertCacheRef,
        cert_resolver: ServerCertResolverRef,
        cert_factory: P2pIdentityCertFactoryRef,
        congestion_algorithm: QuicCongestionAlgorithm,
        server_runtime: ServerRuntime,
        on_incoming_tunnel: IncomingTunnelCallback,
        listener_connect_capacity: usize,
    ) -> Arc<Self> {
        Arc::new(Self {
            cert_cache,
            cert_resolver,
            cert_factory,
            congestion_algorithm,
            state: RwLock::new(QuicTunnelListenerState {
                local: None,
                outer: None,
                endpoints: Vec::new(),
                server: None,
                punch_socket: None,
                mapping_port: None,
                reuse_address: false,
            }),
            on_incoming_tunnel,
            close_notify: Notify::new(),
            endpoint_ready: Notify::new(),
            closed: AtomicBool::new(false),
            worker_count: Arc::new(AtomicUsize::new(0)),
            server_runtime,
            listener_connect_capacity,
        })
    }

    pub(crate) fn local(&self) -> Endpoint {
        self.state.read().unwrap().local.unwrap()
    }

    pub(crate) fn bound_local(&self) -> Endpoint {
        let endpoint = self
            .state
            .read()
            .unwrap()
            .endpoints
            .first()
            .cloned()
            .unwrap();
        Endpoint::from((
            crate::endpoint::Protocol::Quic,
            endpoint.endpoint.local_addr().unwrap(),
        ))
    }

    pub(crate) fn mapping_port(&self) -> Option<u16> {
        self.state.read().unwrap().mapping_port
    }

    pub(crate) async fn connect_with_owner_runtime(
        &self,
        local_identity_ref: P2pIdentityRef,
        cert_factory: P2pIdentityCertFactoryRef,
        remote_identity_id: P2pId,
        remote_name: Option<String>,
        remote: Endpoint,
        congestion_algorithm: QuicCongestionAlgorithm,
        timeout: Duration,
        idle_timeout: Duration,
    ) -> P2pResult<quinn::Connection> {
        let endpoint = {
            let state = self.state.read().unwrap();
            let index = rand::rng().random_range(0..state.endpoints.len());
            state.endpoints[index].clone()
        };

        let (response, rx) = oneshot::channel();
        endpoint
            .connect_tx
            .send(QuicConnectRequest {
                local_identity_ref,
                cert_factory,
                remote_identity_id,
                remote_name,
                remote,
                congestion_algorithm,
                timeout,
                idle_timeout,
                response,
            })
            .await
            .map_err(|_| p2p_err!(P2pErrorCode::ErrorState, "quic endpoint worker closed"))?;
        rx.await
            .map_err(|_| p2p_err!(P2pErrorCode::ErrorState, "quic endpoint worker closed"))?
    }

    pub(crate) fn start_udp_punch_burst(
        &self,
        remote: Endpoint,
        intent: TunnelConnectIntent,
        max_duration: Duration,
    ) {
        if !udp_punch_enabled_for_endpoint(&remote) {
            return;
        }
        let offsets = udp_punch_offsets_for_deadline(intent, max_duration);
        if offsets.is_empty() {
            return;
        }
        let punch_socket = {
            let state = self.state.read().unwrap();
            state.punch_socket.clone()
        };
        let Some(punch_socket) = punch_socket else {
            log::trace!(
                "quic udp punch skipped remote={} because sender missing",
                remote
            );
            return;
        };
        let payload = udp_punch_payload(intent);
        Executor::spawn_ok(async move {
            let mut last_offset = Duration::ZERO;
            for (index, offset) in offsets.into_iter().enumerate() {
                let sleep_duration = offset.saturating_sub(last_offset);
                if !sleep_duration.is_zero() {
                    runtime::sleep(sleep_duration).await;
                }
                if let Err(err) =
                    try_send_udp_punch_packet(&punch_socket, *remote.addr(), payload.as_slice())
                        .await
                {
                    log::trace!(
                        "quic udp punch send failed remote={} index={} error={}",
                        remote,
                        index,
                        err
                    );
                }
                last_offset = offset;
            }
        });
    }

    pub(crate) fn close(&self) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        self.close_notify.notify_waiters();
        let (endpoints, server) = {
            let mut state = self.state.write().unwrap();
            let server = state.server.take();
            state.punch_socket.take();
            let endpoints = std::mem::take(&mut state.endpoints);
            (endpoints, server)
        };
        for ep in endpoints {
            ep.endpoint.close(0_u32.into(), b"close all listeners");
        }
        if let Some(server) = server {
            let _ = server.close();
        }
    }

    fn build_server_config(&self) -> P2pResult<quinn::ServerConfig> {
        let mut server_config =
            rustls::ServerConfig::builder_with_provider(crate::tls::provider().into())
                .with_protocol_versions(&[&TLS13])
                .map_err(into_p2p_err!(
                    P2pErrorCode::TlsError,
                    "Create server config error"
                ))?
                .with_client_cert_verifier(Arc::new(crate::tls::TlsClientCertVerifier::new(
                    self.cert_factory.clone(),
                )))
                .with_cert_resolver(self.cert_resolver.clone().get_resolves_server_cert());
        server_config.key_log = Arc::new(rustls::KeyLogFile::new());

        let mut server_config = quinn::ServerConfig::with_crypto(Arc::new(
            QuicServerConfig::try_from(server_config).map_err(into_p2p_err!(
                P2pErrorCode::TlsError,
                "create quic server config failed"
            ))?,
        ));
        let transport_config = Arc::get_mut(&mut server_config.transport).unwrap();
        transport_config
            .max_idle_timeout(Some(
                std::time::Duration::from_secs(600).try_into().unwrap(),
            ))
            .initial_rtt(Duration::from_millis(200));
        match self.congestion_algorithm {
            QuicCongestionAlgorithm::Bbr => {
                transport_config.congestion_controller_factory(Arc::new(
                    quinn::congestion::BbrConfig::default(),
                ));
            }
            QuicCongestionAlgorithm::Cubic => {
                transport_config.congestion_controller_factory(Arc::new(
                    quinn::congestion::CubicConfig::default(),
                ));
            }
            QuicCongestionAlgorithm::NewReno => {
                transport_config.congestion_controller_factory(Arc::new(
                    quinn::congestion::NewRenoConfig::default(),
                ));
            }
        }

        Ok(server_config)
    }

    pub(crate) async fn bind(
        self: &Arc<Self>,
        local: Endpoint,
        out: Option<Endpoint>,
        mapping_port: Option<u16>,
        reuse_address: bool,
    ) -> P2pResult<()> {
        {
            let mut state = self.state.write().unwrap();
            state.local = Some(local);
            state.outer = out;
            state.mapping_port = mapping_port;
            state.reuse_address = reuse_address;
        }

        Ok(())
    }

    pub(crate) async fn start(self: &Arc<Self>) -> P2pResult<()> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(p2p_err!(P2pErrorCode::ErrorState, "quic listener closed"));
        }

        let (local, reuse_address) = {
            let state = self.state.read().unwrap();
            if state.server.is_some() {
                return Err(p2p_err!(
                    P2pErrorCode::AlreadyExists,
                    "quic listener already started"
                ));
            }
            let local = state
                .local
                .ok_or_else(|| p2p_err!(P2pErrorCode::InvalidParam, "quic listener not bound"))?;
            (local, state.reuse_address)
        };

        let server_config = self.build_server_config()?;
        let config = UdpServiceConfig::new(*local.addr()).with_socket_options(SocketOptions {
            reuse_address,
            ..SocketOptions::default()
        });
        let listener = self.clone();
        let server =
            QuicServer::serve_socket(&self.server_runtime, config, move |socket, worker_id| {
                let listener = listener.clone();
                let server_config = server_config.clone();
                async move {
                    listener
                        .run_worker_endpoint(socket, worker_id, server_config)
                        .await
                }
            })
            .map_err(into_p2p_err!(
                P2pErrorCode::AlreadyExists,
                "bind quic listener {} error",
                local
            ))?;

        {
            let mut state = self.state.write().unwrap();
            state.server = Some(server.clone());
        }
        if let Err(err) = wait_quic_endpoint_ready(self).await {
            let _ = server.close();
            let endpoints = {
                let mut state = self.state.write().unwrap();
                state.server.take();
                state.punch_socket.take();
                std::mem::take(&mut state.endpoints)
            };
            for endpoint in endpoints {
                endpoint.endpoint.close(0_u32.into(), b"close listener");
            }
            return Err(err);
        }

        Ok(())
    }

    async fn run_worker_endpoint(
        self: Arc<Self>,
        socket: SfoUdpSocket,
        worker_id: usize,
        server_config: quinn::ServerConfig,
    ) -> Result<(), SfoReuseportError> {
        if self.closed.load(Ordering::SeqCst) {
            return Ok(());
        }
        self.worker_count
            .fetch_max(worker_id.saturating_add(1), Ordering::AcqRel);
        let endpoint_config = new_quic_endpoint_config(worker_id)
            .map_err(|err| SfoReuseportError::Runtime(err.to_string()))?;
        let endpoint = quinn::Endpoint::new_with_abstract_socket(
            endpoint_config,
            Some(server_config),
            Arc::new(SfoQuicUdpSocket::new(
                socket.clone(),
                worker_id,
                self.worker_count.clone(),
            )),
            Arc::new(quinn::TokioRuntime),
        )
        .map_err(|err| SfoReuseportError::Runtime(err.to_string()))?;

        let (connect_tx, connect_rx) = mpsc::channel(self.listener_connect_capacity);
        if !self.register_worker_endpoint(endpoint.clone(), connect_tx, socket) {
            endpoint.close(0_u32.into(), b"close listener");
            return Ok(());
        }
        wait_quic_endpoint_loop(self, endpoint, connect_rx).await;
        Ok(())
    }

    fn register_worker_endpoint(
        &self,
        endpoint: quinn::Endpoint,
        connect_tx: mpsc::Sender<QuicConnectRequest>,
        socket: SfoUdpSocket,
    ) -> bool {
        if self.closed.load(Ordering::SeqCst) {
            return false;
        }
        let mut state = self.state.write().unwrap();
        if state.punch_socket.is_none() {
            state.punch_socket = Some(socket);
        }
        state.endpoints.push(WorkerQuicEndpoint {
            endpoint,
            connect_tx,
        });
        drop(state);
        self.endpoint_ready.notify_waiters();
        true
    }

    async fn accept_connection(&self, conn: Incoming) -> P2pResult<TunnelRef> {
        let connection = conn.await.map_err(into_p2p_err!(
            P2pErrorCode::QuicError,
            "QuicTunnelListener accept error"
        ))?;
        let server_name = {
            let handshake_data = connection
                .handshake_data()
                .ok_or_else(|| p2p_err!(P2pErrorCode::TlsError, "no handshake data"))?;
            let handshake_data = handshake_data
                .as_ref()
                .downcast_ref::<HandshakeData>()
                .ok_or_else(|| p2p_err!(P2pErrorCode::TlsError, "no handshake data"))?;
            let server_name = handshake_data
                .server_name
                .as_ref()
                .ok_or_else(|| p2p_err!(P2pErrorCode::TlsError, "no server name"))?;
            parse_server_name(server_name).to_owned()
        };

        let remote_cert = {
            let peer_identity = connection
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

        let local_identity = self
            .cert_resolver
            .get_server_identity(server_name.as_str())
            .await
            .ok_or_else(|| p2p_err!(P2pErrorCode::CertError, "no local cert"))?;
        let remote_identity = self.cert_factory.create(&remote_cert)?;
        self.cert_cache
            .add(&remote_identity.get_id(), &remote_identity)
            .await?;

        let remote_addr = connection.remote_address();
        Ok(QuicTunnel::accept(
            connection,
            local_identity.get_id(),
            remote_identity.get_id(),
            self.local(),
            Endpoint::from((Protocol::Quic, remote_addr)),
        )
        .await?)
    }
}

fn udp_punch_enabled_for_endpoint(remote: &Endpoint) -> bool {
    remote.protocol() == Protocol::Quic
        && remote.get_area() == EndpointArea::ServerReflexive
        && is_non_lan_ipv4_addr(remote.addr())
        && remote.addr().port() != 0
}

fn udp_punch_payload(intent: TunnelConnectIntent) -> Vec<u8> {
    let _ = intent;
    let payload_len = rand::rng().random_range(
        UDP_PUNCH_PAYLOAD_MIN_LEN.max(UDP_PUNCH_MAGIC.len())..=UDP_PUNCH_PAYLOAD_MAX_LEN,
    );
    let mut payload = random::<[u8; UDP_PUNCH_PAYLOAD_MAX_LEN]>();
    payload[..UDP_PUNCH_MAGIC.len()].copy_from_slice(UDP_PUNCH_MAGIC);
    payload[..payload_len].to_vec()
}

fn udp_punch_start_offset(intent: TunnelConnectIntent) -> Duration {
    if intent.is_reverse {
        UDP_PUNCH_REVERSE_START_OFFSET
    } else {
        UDP_PUNCH_ACTIVE_START_OFFSET
    }
}

pub(crate) fn udp_punch_burst_window(intent: TunnelConnectIntent) -> Duration {
    let _ = intent;
    UDP_PUNCH_DEADLINE
}

fn udp_punch_offsets_for_deadline(
    intent: TunnelConnectIntent,
    max_duration: Duration,
) -> Vec<Duration> {
    let deadline = max_duration.min(UDP_PUNCH_DEADLINE);
    let start = udp_punch_start_offset(intent);
    if start > deadline {
        return Vec::new();
    }

    let mut offsets = Vec::new();
    let mut offset = start;
    loop {
        offsets.push(offset);
        let Some(next_offset) = offset.checked_add(UDP_PUNCH_INTERVAL) else {
            break;
        };
        if next_offset > deadline {
            break;
        }
        offset = next_offset;
    }
    offsets
}

async fn try_send_udp_punch_packet(
    socket: &SfoUdpSocket,
    remote: std::net::SocketAddr,
    payload: &[u8],
) -> Result<(), SfoReuseportError> {
    socket.send_to(payload, remote).await.map(|_| ())
}

fn validate_server_name(server_name: String) -> String {
    match rustls::pki_types::ServerName::try_from(server_name.as_str()) {
        Ok(_) => server_name,
        Err(_) => format!("p2p.{}.com", server_name),
    }
}

fn parse_server_name(server_name: &str) -> &str {
    if server_name.starts_with("p2p.") && server_name.ends_with(".com") {
        server_name
            .trim_start_matches("p2p.")
            .trim_end_matches(".com")
    } else {
        server_name
    }
}

pub(crate) async fn connect_with_ep(
    ep: quinn::Endpoint,
    local_identity_ref: P2pIdentityRef,
    cert_factory: P2pIdentityCertFactoryRef,
    remote_identity_id: P2pId,
    remote_name: Option<String>,
    remote: Endpoint,
    congestion_algorithm: QuicCongestionAlgorithm,
    timeout: std::time::Duration,
    idle_timeout: std::time::Duration,
) -> P2pResult<quinn::Connection> {
    let client_key = local_identity_ref.get_encoded_identity()?;
    let client_cert = local_identity_ref.get_identity_cert()?.get_encoded_cert()?;
    let mut config = rustls::ClientConfig::builder_with_provider(crate::tls::provider().into())
        .with_protocol_versions(&[&TLS13])
        .unwrap()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(crate::tls::TlsServerCertVerifier::new(
            cert_factory,
            remote_identity_id.clone(),
        )))
        .with_client_auth_cert(
            vec![CertificateDer::from(client_cert)],
            rustls::pki_types::PrivatePkcs8KeyDer::from(client_key).into(),
        )
        .map_err(into_p2p_err!(P2pErrorCode::TlsError))?;
    config.enable_early_data = true;

    let mut client_config = quinn::ClientConfig::new(Arc::new(
        quinn::crypto::rustls::QuicClientConfig::try_from(config).unwrap(),
    ));
    let mut transport_config = quinn::TransportConfig::default();
    transport_config
        .max_idle_timeout(Some(idle_timeout.try_into().unwrap()))
        .initial_rtt(Duration::from_millis(200));
    if idle_timeout > std::time::Duration::from_secs(15) {
        transport_config.keep_alive_interval(Some(std::time::Duration::from_secs(15)));
    }
    match congestion_algorithm {
        QuicCongestionAlgorithm::Bbr => {
            transport_config
                .congestion_controller_factory(Arc::new(quinn::congestion::BbrConfig::default()));
        }
        QuicCongestionAlgorithm::Cubic => {
            transport_config
                .congestion_controller_factory(Arc::new(quinn::congestion::CubicConfig::default()));
        }
        QuicCongestionAlgorithm::NewReno => {
            transport_config.congestion_controller_factory(Arc::new(
                quinn::congestion::NewRenoConfig::default(),
            ));
        }
    }
    client_config.transport_config(Arc::new(transport_config));

    let remote_name = remote_name.unwrap_or(remote_identity_id.to_string());
    let remote_name = validate_server_name(remote_name);
    runtime::timeout(
        timeout,
        ep.connect_with(
            client_config,
            remote.addr().to_owned(),
            remote_name.as_str(),
        )
        .unwrap(),
    )
    .await
    .map_err(into_p2p_err!(
        P2pErrorCode::ConnectFailed,
        "quic to {} connect failed",
        remote
    ))?
    .map_err(into_p2p_err!(
        P2pErrorCode::ConnectFailed,
        "quic to {} connect failed",
        remote
    ))
}

#[cfg(test)]
mod udp_punch_tests {
    use super::*;
    use crate::endpoint::EndpointArea;
    use crate::types::{TunnelCandidateId, TunnelId};
    use sfo_reuseport::{ServerRuntimeConfig, UdpServer};
    use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

    fn endpoint(protocol: Protocol, ip: Ipv4Addr, port: u16, area: EndpointArea) -> Endpoint {
        let mut ep = Endpoint::from((protocol, SocketAddr::V4(SocketAddrV4::new(ip, port))));
        ep.set_area(area);
        ep
    }

    #[test]
    fn udp_punch_policy_only_enables_server_reflexive_quic_non_lan_ipv4_candidates() {
        let server_reflexive_quic = endpoint(
            Protocol::Quic,
            Ipv4Addr::new(8, 8, 8, 8),
            10001,
            EndpointArea::ServerReflexive,
        );
        let public_quic_wan_area = endpoint(
            Protocol::Quic,
            Ipv4Addr::new(1, 1, 1, 1),
            10002,
            EndpointArea::Wan,
        );
        let public_quic_mapped_area = endpoint(
            Protocol::Quic,
            Ipv4Addr::new(8, 8, 4, 4),
            10007,
            EndpointArea::Mapped,
        );
        let public_quic_lan_area = endpoint(
            Protocol::Quic,
            Ipv4Addr::new(9, 9, 9, 9),
            10008,
            EndpointArea::Lan,
        );
        let private_quic = endpoint(
            Protocol::Quic,
            Ipv4Addr::new(192, 168, 1, 10),
            10003,
            EndpointArea::ServerReflexive,
        );
        let loopback_quic = endpoint(
            Protocol::Quic,
            Ipv4Addr::LOCALHOST,
            10004,
            EndpointArea::ServerReflexive,
        );
        let tcp = endpoint(
            Protocol::Tcp,
            Ipv4Addr::new(8, 8, 4, 4),
            10005,
            EndpointArea::ServerReflexive,
        );
        let zero_port_quic = endpoint(
            Protocol::Quic,
            Ipv4Addr::new(8, 8, 8, 8),
            0,
            EndpointArea::ServerReflexive,
        );
        let mut ipv6_quic = Endpoint::from((
            Protocol::Quic,
            "[2001:4860:4860::8888]:10006"
                .parse::<SocketAddr>()
                .unwrap(),
        ));
        ipv6_quic.set_area(EndpointArea::ServerReflexive);

        assert!(udp_punch_enabled_for_endpoint(&server_reflexive_quic));
        assert!(!udp_punch_enabled_for_endpoint(&public_quic_lan_area));
        assert!(!udp_punch_enabled_for_endpoint(&public_quic_wan_area));
        assert!(!udp_punch_enabled_for_endpoint(&public_quic_mapped_area));
        assert!(!udp_punch_enabled_for_endpoint(&private_quic));
        assert!(!udp_punch_enabled_for_endpoint(&loopback_quic));
        assert!(!udp_punch_enabled_for_endpoint(&tcp));
        assert!(!udp_punch_enabled_for_endpoint(&zero_port_quic));
        assert!(!udp_punch_enabled_for_endpoint(&ipv6_quic));
    }

    #[test]
    fn udp_punch_payload_is_random_private_probe_data() {
        let intent = TunnelConnectIntent::reverse(
            TunnelId::from(0x0102_0304),
            TunnelCandidateId::from(0x0506_0708),
        );
        let payloads = (0..64)
            .map(|_| udp_punch_payload(intent))
            .collect::<Vec<_>>();

        assert!(payloads.iter().all(|payload| {
            (UDP_PUNCH_PAYLOAD_MIN_LEN..=UDP_PUNCH_PAYLOAD_MAX_LEN).contains(&payload.len())
        }));
        assert!(
            payloads
                .iter()
                .all(|payload| payload.starts_with(UDP_PUNCH_MAGIC))
        );
        assert!(payloads.iter().all(|payload| payload[0] == 0));
        assert!(
            payloads
                .windows(2)
                .any(|pair| pair[0].len() != pair[1].len())
        );
        assert!(payloads.windows(2).any(|pair| pair[0] != pair[1]));
    }

    #[test]
    fn udp_punch_payload_magic_identifies_only_private_probe_data() {
        let payload = udp_punch_payload(TunnelConnectIntent::active_logical(TunnelId::from(7)));
        assert!(is_udp_punch_payload(&payload));
        assert!(!is_udp_punch_payload(&[0xc0, 0, 0, 0, 1, 8, 1, 2]));
        assert!(!is_udp_punch_payload(&[0x40, 0, 1, 2, 3]));
        assert!(!is_udp_punch_payload(b"\x00P2"));
    }

    #[test]
    fn quic_packet_worker_index_uses_dcid_prefix_for_long_and_short_packets() {
        let long_packet = [0xe0, 0, 0, 0, 1, 4, 0x12, 0x34, 0xaa, 0xbb];
        let short_packet = [0x40, 0x12, 0x35, 0xcc, 0xdd];

        assert_eq!(quic_packet_worker_index_prefix(&long_packet), Some(0x1234));
        assert_eq!(quic_packet_worker_index(&long_packet, 7), Some(0x1234 % 7));
        assert_eq!(quic_packet_worker_index_prefix(&short_packet), Some(0x1235));
        assert_eq!(quic_packet_worker_index(&short_packet, 7), Some(0x1235 % 7));
        assert_eq!(quic_packet_worker_index(&short_packet, 0), None);
        assert_eq!(
            quic_packet_worker_index_prefix(&[0xe0, 0, 0, 0, 1, 0]),
            None
        );
        assert_eq!(
            quic_packet_worker_index_prefix(&[0xc0, 0, 0, 0, 1, 2]),
            None
        );
    }

    #[test]
    fn udp_punch_schedule_delays_active_and_starts_reverse_immediately() {
        let active = TunnelConnectIntent::active_logical(TunnelId::from(7));
        let reverse = TunnelConnectIntent::reverse_logical(TunnelId::from(8));

        assert_eq!(udp_punch_burst_window(active), Duration::from_secs(1));
        assert_eq!(udp_punch_burst_window(reverse), Duration::from_secs(1));
        assert_eq!(
            udp_punch_offsets_for_deadline(active, Duration::from_secs(3)),
            vec![
                Duration::from_millis(250),
                Duration::from_millis(300),
                Duration::from_millis(350),
                Duration::from_millis(400),
                Duration::from_millis(450),
                Duration::from_millis(500),
                Duration::from_millis(550),
                Duration::from_millis(600),
                Duration::from_millis(650),
                Duration::from_millis(700),
                Duration::from_millis(750),
                Duration::from_millis(800),
                Duration::from_millis(850),
                Duration::from_millis(900),
                Duration::from_millis(950),
                Duration::from_millis(1000),
            ]
        );
        assert_eq!(
            udp_punch_offsets_for_deadline(active, Duration::from_millis(300)),
            vec![Duration::from_millis(250), Duration::from_millis(300)]
        );
        assert_eq!(
            udp_punch_offsets_for_deadline(reverse, Duration::from_secs(3)),
            vec![
                Duration::from_millis(0),
                Duration::from_millis(50),
                Duration::from_millis(100),
                Duration::from_millis(150),
                Duration::from_millis(200),
                Duration::from_millis(250),
                Duration::from_millis(300),
                Duration::from_millis(350),
                Duration::from_millis(400),
                Duration::from_millis(450),
                Duration::from_millis(500),
                Duration::from_millis(550),
                Duration::from_millis(600),
                Duration::from_millis(650),
                Duration::from_millis(700),
                Duration::from_millis(750),
                Duration::from_millis(800),
                Duration::from_millis(850),
                Duration::from_millis(900),
                Duration::from_millis(950),
                Duration::from_millis(1000),
            ]
        );
        assert_eq!(
            udp_punch_offsets_for_deadline(reverse, Duration::from_millis(100)),
            vec![
                Duration::from_millis(0),
                Duration::from_millis(50),
                Duration::from_millis(100),
            ]
        );
    }

    async fn sfo_udp_socket() -> (ServerRuntime, UdpServer, SfoUdpSocket) {
        let runtime = ServerRuntime::start(ServerRuntimeConfig::new().with_workers(1)).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let config = UdpServiceConfig::new("127.0.0.1:0".parse().unwrap());
        let server = UdpServer::serve_socket(&runtime, config, move |socket, _worker_id| {
            let tx = tx.clone();
            async move {
                let _ = tx.send(socket);
                std::future::pending::<Result<(), SfoReuseportError>>().await
            }
        })
        .unwrap();
        let socket = tokio::task::spawn_blocking(move || rx.recv_timeout(Duration::from_secs(2)))
            .await
            .unwrap()
            .unwrap();
        (runtime, server, socket)
    }

    #[tokio::test]
    async fn udp_punch_socket_preserves_listener_local_port() {
        let (_runtime, server, punch_socket) = sfo_udp_socket().await;
        assert_eq!(
            server.listener_socket().unwrap().local_addr().unwrap(),
            punch_socket.local_addr().unwrap()
        );
    }

    #[tokio::test]
    async fn udp_punch_send_failure_is_best_effort() {
        let (_runtime, _server, socket) = sfo_udp_socket().await;
        let invalid_remote = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0));
        let payload = udp_punch_payload(TunnelConnectIntent::active_logical(TunnelId::from(7)));

        assert!(
            try_send_udp_punch_packet(&socket, invalid_remote, payload.as_slice())
                .await
                .is_err()
        );
    }
}
