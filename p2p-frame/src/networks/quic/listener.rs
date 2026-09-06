use super::tunnel::QuicTunnel;
use crate::endpoint::{Endpoint, EndpointArea, Protocol, rendezvous_eligible_area};
use crate::error::{P2pErrorCode, P2pResult, into_p2p_err, p2p_err};
use crate::finder::DeviceCache;
use crate::nat_type::{MAX_NAT_PREDICTION_PORTS, NatMappingObservation, NatProfile};
use crate::networks::TraversalEndpointPrediction;
use crate::networks::{
    IncomingTunnelCallback, QuicCongestionAlgorithm, TunnelConnectIntent, TunnelForm, TunnelRef,
};
use crate::p2p_identity::{
    P2pId, P2pIdentityCertCacheRef, P2pIdentityCertFactoryRef, P2pIdentityCertRef, P2pIdentityRef,
};
use crate::runtime;
use crate::sn::nat_probe::{
    DecodedNatProbeResponse, NAT_PROBE_PACKET_LEN, NAT_PROBE_TOKEN_LEN, decode_response_datagram,
    encode_request,
};
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
use std::collections::HashMap;
use std::future::Future;
use std::io::{self, IoSliceMut};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::runtime::Handle as TokioRuntimeHandle;
use tokio::sync::{Notify, oneshot};

const UDP_PUNCH_PAYLOAD_MIN_LEN: usize = 5;
const UDP_PUNCH_PAYLOAD_MAX_LEN: usize = 30;
const UDP_PUNCH_MAGIC: &[u8] = b"\x00#@$QUIC";
const UDP_PUNCH_INTERVAL: Duration = Duration::from_millis(50);
const UDP_PUNCH_ACTIVE_START_OFFSET: Duration = Duration::from_millis(250);
const UDP_PUNCH_REVERSE_START_OFFSET: Duration = Duration::ZERO;
const UDP_PUNCH_EARLY_ERROR_WINDOW: Duration = Duration::from_secs(1);
const QUIC_ENDPOINT_READY_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_AUXILIARY_DATAGRAMS_PER_POLL: usize = 4;

#[cfg(test)]
type UdpPunchSendObserver = Arc<dyn Fn() + Send + Sync>;

struct AbortOnDropTask<T> {
    task: tokio::task::JoinHandle<T>,
}

impl<T> AbortOnDropTask<T> {
    fn new(task: tokio::task::JoinHandle<T>) -> Self {
        Self { task }
    }

    async fn join(mut self) -> Result<T, tokio::task::JoinError> {
        (&mut self.task).await
    }
}

impl<T> Drop for AbortOnDropTask<T> {
    fn drop(&mut self) {
        self.task.abort();
    }
}

struct SfoQuicUdpSocket {
    socket: SfoUdpSocket,
    worker_id: usize,
    worker_count: Arc<AtomicUsize>,
    nat_probe_waiters: Arc<NatProbeResponseWaiters>,
}

impl SfoQuicUdpSocket {
    fn new(
        socket: SfoUdpSocket,
        worker_id: usize,
        worker_count: Arc<AtomicUsize>,
        nat_probe_waiters: Arc<NatProbeResponseWaiters>,
    ) -> Self {
        Self {
            socket,
            worker_id,
            worker_count,
            nat_probe_waiters,
        }
    }
}

type NatProbeResponse = std::net::SocketAddr;

struct PendingNatProbeResponse {
    expected_source: std::net::SocketAddr,
    expected_signer: P2pIdentityCertRef,
    owner: Arc<()>,
    sender: oneshot::Sender<NatProbeResponse>,
}

#[derive(Default)]
struct NatProbeResponseWaiters {
    pending: Arc<Mutex<HashMap<[u8; NAT_PROBE_TOKEN_LEN], PendingNatProbeResponse>>>,
}

struct NatProbeResponseRegistration {
    pending: Arc<Mutex<HashMap<[u8; NAT_PROBE_TOKEN_LEN], PendingNatProbeResponse>>>,
    token: [u8; NAT_PROBE_TOKEN_LEN],
    owner: Arc<()>,
}

impl Drop for NatProbeResponseRegistration {
    fn drop(&mut self) {
        let mut pending = self.pending.lock().unwrap();
        if pending
            .get(&self.token)
            .is_some_and(|waiter| Arc::ptr_eq(&waiter.owner, &self.owner))
        {
            pending.remove(&self.token);
        }
    }
}

struct NatProbeResponseReceiver {
    receiver: oneshot::Receiver<NatProbeResponse>,
    _registration: NatProbeResponseRegistration,
}

impl std::fmt::Debug for NatProbeResponseReceiver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NatProbeResponseReceiver")
            .finish_non_exhaustive()
    }
}

impl Future for NatProbeResponseReceiver {
    type Output = Result<NatProbeResponse, oneshot::error::RecvError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.receiver).poll(cx)
    }
}

impl NatProbeResponseWaiters {
    fn register(
        &self,
        token: [u8; NAT_PROBE_TOKEN_LEN],
        expected_source: std::net::SocketAddr,
        expected_signer: &P2pIdentityCertRef,
    ) -> P2pResult<NatProbeResponseReceiver> {
        let (sender, receiver) = oneshot::channel();
        let owner = Arc::new(());
        let mut pending = self.pending.lock().unwrap();
        if pending.contains_key(&token) {
            return Err(p2p_err!(
                P2pErrorCode::AlreadyExists,
                "duplicate NAT probe token"
            ));
        }
        pending.insert(
            token,
            PendingNatProbeResponse {
                expected_source,
                expected_signer: expected_signer.clone(),
                owner: owner.clone(),
                sender,
            },
        );
        drop(pending);
        Ok(NatProbeResponseReceiver {
            receiver,
            _registration: NatProbeResponseRegistration {
                pending: self.pending.clone(),
                token,
                owner,
            },
        })
    }

    fn dispatch(&self, response: DecodedNatProbeResponse<'_>, source: std::net::SocketAddr) {
        let (expected_signer, owner) = {
            let pending = self.pending.lock().unwrap();
            let Some(waiter) = pending.get(&response.token) else {
                return;
            };
            if source != waiter.expected_source {
                return;
            }
            (waiter.expected_signer.clone(), waiter.owner.clone())
        };
        if !response.verify(&expected_signer) {
            return;
        }

        let sender = {
            let mut pending = self.pending.lock().unwrap();
            let Some(waiter) = pending.get(&response.token) else {
                return;
            };
            if !Arc::ptr_eq(&waiter.owner, &owner) {
                return;
            }
            pending
                .remove(&response.token)
                .expect("owned NAT probe waiter disappeared")
                .sender
        };
        let _ = sender.send(response.observed);
    }

    fn clear(&self) {
        self.pending.lock().unwrap().clear();
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
        let mut auxiliary_datagrams = 0;
        loop {
            match self.socket.poll_recv_from_vectored(cx, bufs) {
                Poll::Ready(Ok((len, peer_addr))) => {
                    let mut packet = Vec::new();
                    let packet = quic_packet_prefix(bufs, len, &mut packet);
                    if let Some(response) = decode_response_datagram(packet) {
                        self.nat_probe_waiters.dispatch(response, peer_addr);
                        auxiliary_datagrams += 1;
                        if auxiliary_datagrams == MAX_AUXILIARY_DATAGRAMS_PER_POLL {
                            cx.waker().wake_by_ref();
                            return Poll::Pending;
                        }
                        continue;
                    }
                    if is_udp_punch_payload(packet) {
                        auxiliary_datagrams += 1;
                        if auxiliary_datagrams == MAX_AUXILIARY_DATAGRAMS_PER_POLL {
                            cx.waker().wake_by_ref();
                            return Poll::Pending;
                        }
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

#[derive(Clone)]
struct WorkerQuicEndpoint {
    endpoint: quinn::Endpoint,
    worker_index: usize,
    runtime: TokioRuntimeHandle,
}

async fn wait_quic_endpoint_loop(listener: Arc<QuicTunnelListener>, endpoint: quinn::Endpoint) {
    loop {
        match endpoint.accept().await {
            Some(conn) => {
                let result = listener.accept_connection(conn).await;
                (listener.on_incoming_tunnel)(result).await;
            }
            None => break,
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

fn ensure_worker_endpoints_available(closed: bool, endpoints_empty: bool) -> P2pResult<()> {
    if closed {
        return Err(p2p_err!(P2pErrorCode::Interrupted, "quic listener closed"));
    }
    if endpoints_empty {
        return Err(p2p_err!(
            P2pErrorCode::ErrorState,
            "quic listener has no worker endpoint"
        ));
    }
    Ok(())
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
    socket_binding_generation: u64,
    nat_probe_waiters: Arc<NatProbeResponseWaiters>,
    server_runtime: ServerRuntime,
    #[cfg(test)]
    udp_punch_send_observer: Mutex<Option<UdpPunchSendObserver>>,
}

impl QuicTunnelListener {
    pub(crate) fn new(
        cert_cache: P2pIdentityCertCacheRef,
        cert_resolver: ServerCertResolverRef,
        cert_factory: P2pIdentityCertFactoryRef,
        congestion_algorithm: QuicCongestionAlgorithm,
        server_runtime: ServerRuntime,
        on_incoming_tunnel: IncomingTunnelCallback,
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
            socket_binding_generation: rand::random::<u64>().max(1),
            nat_probe_waiters: Arc::new(NatProbeResponseWaiters::default()),
            server_runtime,
            #[cfg(test)]
            udp_punch_send_observer: Mutex::new(None),
        })
    }

    #[cfg(test)]
    fn set_udp_punch_send_observer(&self, observer: Option<UdpPunchSendObserver>) {
        *self.udp_punch_send_observer.lock().unwrap() = observer;
    }

    async fn send_udp_punch_packet(
        &self,
        socket: &SfoUdpSocket,
        remote: std::net::SocketAddr,
        payload: &[u8],
    ) -> Result<(), SfoReuseportError> {
        #[cfg(test)]
        {
            let observer = self.udp_punch_send_observer.lock().unwrap().clone();
            if let Some(observer) = observer {
                observer();
                return Ok(());
            }
        }

        try_send_udp_punch_packet(socket, remote, payload).await
    }

    pub(crate) fn local(&self) -> Endpoint {
        self.state.read().unwrap().local.unwrap()
    }

    pub(crate) fn bound_local(&self) -> P2pResult<Endpoint> {
        let endpoint = {
            let state = self.state.read().unwrap();
            ensure_worker_endpoints_available(
                self.closed.load(Ordering::SeqCst),
                state.endpoints.is_empty(),
            )?;
            state.endpoints[0].clone()
        };
        Ok(Endpoint::from((
            crate::endpoint::Protocol::Quic,
            endpoint.endpoint.local_addr().map_err(|err| {
                p2p_err!(
                    P2pErrorCode::ErrorState,
                    "get quic listener worker local address failed: {}",
                    err
                )
            })?,
        )))
    }

    pub(crate) fn mapping_port(&self) -> Option<u16> {
        self.state.read().unwrap().mapping_port
    }

    pub(crate) fn socket_binding_generation(&self) -> u64 {
        self.socket_binding_generation
    }

    pub(crate) fn validate_traversal_prediction(
        &self,
        prediction: &TraversalEndpointPrediction,
        now: crate::types::Timestamp,
    ) -> P2pResult<()> {
        if prediction.socket_binding_generation != self.socket_binding_generation {
            return Err(p2p_err!(
                P2pErrorCode::Expired,
                "traversal prediction belongs to a stale QUIC listener generation"
            ));
        }
        if prediction.valid_until < now {
            return Err(p2p_err!(
                P2pErrorCode::Expired,
                "traversal prediction validity has expired"
            ));
        }

        let state = self.state.read().unwrap();
        ensure_worker_endpoints_available(
            self.closed.load(Ordering::SeqCst),
            state.endpoints.is_empty(),
        )?;
        if state.punch_socket.is_none() {
            return Err(p2p_err!(
                P2pErrorCode::ErrorState,
                "QUIC listener traversal socket is unavailable"
            ));
        }
        Ok(())
    }

    pub(crate) async fn probe_nat_profile(
        &self,
        probe_targets: &[Endpoint],
        expected_signer: &P2pIdentityCertRef,
        per_target_timeout: Duration,
        ttl: Duration,
    ) -> P2pResult<NatProfile> {
        if probe_targets.len() < 2
            || probe_targets.len() > MAX_NAT_PREDICTION_PORTS
            || per_target_timeout.is_zero()
            || ttl.is_zero()
        {
            return Err(p2p_err!(
                P2pErrorCode::InvalidParam,
                "invalid listener NAT probe target count or duration"
            ));
        }
        let first_ip = probe_targets[0].addr().ip();
        let mut target_addrs = std::collections::HashSet::with_capacity(probe_targets.len());
        for target in probe_targets {
            if target.protocol() != Protocol::Quic
                || !target.addr().is_ipv4()
                || target.addr().ip() != first_ip
                || target.addr().port() == 0
                || !target_addrs.insert(*target.addr())
            {
                return Err(p2p_err!(
                    P2pErrorCode::InvalidParam,
                    "listener NAT probe targets must be distinct IPv4 QUIC endpoints on one IP"
                ));
            }
        }

        let punch_socket = {
            let state = self.state.read().unwrap();
            ensure_worker_endpoints_available(
                self.closed.load(Ordering::SeqCst),
                state.endpoints.is_empty(),
            )?;
            state.punch_socket.clone().ok_or_else(|| {
                p2p_err!(
                    P2pErrorCode::ErrorState,
                    "listener traversal socket missing"
                )
            })?
        };
        let observed_at = bucky_time::bucky_time_now();
        let mut observations = Vec::with_capacity(probe_targets.len());
        for target in probe_targets {
            if self.closed.load(Ordering::SeqCst) {
                return Err(p2p_err!(P2pErrorCode::Interrupted, "quic listener closed"));
            }
            let token = loop {
                let token = rand::random::<[u8; NAT_PROBE_TOKEN_LEN]>();
                if token.iter().any(|byte| *byte != 0) {
                    break token;
                }
            };
            let receiver =
                self.nat_probe_waiters
                    .register(token, *target.addr(), expected_signer)?;
            let packet = encode_request(token);
            debug_assert_eq!(packet.len(), NAT_PROBE_PACKET_LEN);
            if let Err(err) = punch_socket.send_to(&packet, *target.addr()).await {
                return Err(p2p_err!(
                    P2pErrorCode::IoError,
                    "send listener NAT probe failed: {}",
                    err
                ));
            }
            let response = runtime::timeout(per_target_timeout, receiver).await;
            let observed = match response {
                Ok(Ok(response)) => response,
                Ok(Err(_)) => {
                    return Err(p2p_err!(
                        P2pErrorCode::Interrupted,
                        "listener NAT probe response channel closed"
                    ));
                }
                Err(_) => {
                    return Err(p2p_err!(
                        P2pErrorCode::Timeout,
                        "listener NAT probe timed out"
                    ));
                }
            };
            if !observed.is_ipv4() {
                return Err(p2p_err!(
                    P2pErrorCode::InvalidData,
                    "listener NAT probe response is not IPv4"
                ));
            }
            let mut endpoint = Endpoint::from((Protocol::Quic, observed));
            endpoint.set_area(EndpointArea::ServerReflexive);
            observations.push(endpoint);
        }

        if self.closed.load(Ordering::SeqCst) {
            return Err(p2p_err!(P2pErrorCode::Interrupted, "quic listener closed"));
        }
        let profile = NatProfile::from_observations(&observations, observed_at, ttl);
        if profile.observed_endpoint.is_none() {
            return Err(p2p_err!(
                P2pErrorCode::NotFound,
                "listener NAT probe produced no observed endpoint"
            ));
        }
        Ok(profile)
    }

    pub(crate) async fn predict_traversal_endpoints(
        &self,
        probe_targets: &[Endpoint],
        expected_signer: &P2pIdentityCertRef,
        per_target_timeout: Duration,
        ttl: Duration,
    ) -> P2pResult<TraversalEndpointPrediction> {
        let profile = self
            .probe_nat_profile(probe_targets, expected_signer, per_target_timeout, ttl)
            .await?;
        let base = profile.observed_endpoint.ok_or_else(|| {
            p2p_err!(
                P2pErrorCode::NotFound,
                "listener NAT probe produced no observed endpoint"
            )
        })?;
        let mut endpoints = Vec::new();
        match profile.observation {
            NatMappingObservation::NonSymmetricLike => endpoints.push(base),
            NatMappingObservation::SymmetricLike => {
                for port in profile.predicted_ports(profile.observed_at, MAX_NAT_PREDICTION_PORTS) {
                    let mut endpoint = Endpoint::from((Protocol::Quic, base.addr().ip(), port));
                    endpoint.set_area(EndpointArea::ServerReflexive);
                    if !endpoints.contains(&endpoint) {
                        endpoints.push(endpoint);
                    }
                }
            }
            NatMappingObservation::Unknown => {}
        }
        if endpoints.is_empty() {
            return Err(p2p_err!(
                P2pErrorCode::NotFound,
                "listener NAT prediction is unavailable"
            ));
        }
        endpoints.truncate(MAX_NAT_PREDICTION_PORTS);
        Ok(TraversalEndpointPrediction {
            endpoints,
            socket_binding_generation: self.socket_binding_generation(),
            valid_until: profile.valid_until,
            profile,
        })
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
            ensure_worker_endpoints_available(
                self.closed.load(Ordering::SeqCst),
                state.endpoints.is_empty(),
            )?;
            let index = rand::rng().random_range(0..state.endpoints.len());
            state.endpoints[index].clone()
        };

        let worker_index = endpoint.worker_index;
        log::trace!(
            "quic connect scheduled on listener worker {} remote={}",
            worker_index,
            remote
        );
        AbortOnDropTask::new(endpoint.runtime.spawn(async move {
            connect_with_ep(
                endpoint.endpoint,
                local_identity_ref,
                cert_factory,
                remote_identity_id,
                remote_name,
                remote,
                congestion_algorithm,
                timeout,
                idle_timeout,
            )
            .await
        }))
        .join()
        .await
        .map_err(|err| {
            p2p_err!(
                P2pErrorCode::ErrorState,
                "quic endpoint worker connect task failed: {}",
                err
            )
        })?
    }

    pub(crate) async fn run_udp_punch_burst(
        &self,
        remote: Endpoint,
        intent: TunnelConnectIntent,
        started_at: std::time::Instant,
        max_duration: Duration,
    ) {
        if !udp_punch_enabled_for_endpoint(&remote) {
            log::trace!(
                "quic udp punch stopped remote={} reason=candidate_policy",
                remote
            );
            return;
        }
        let mut next_offset = udp_punch_start_offset(intent);
        if next_offset > max_duration {
            log::trace!(
                "quic udp punch stopped remote={} reason=deadline_before_first_send",
                remote
            );
            return;
        }
        let punch_socket = {
            let state = self.state.read().unwrap();
            state.punch_socket.clone()
        };
        let Some(punch_socket) = punch_socket else {
            log::trace!(
                "quic udp punch stopped remote={} reason=sender_missing",
                remote
            );
            return;
        };
        let payload = udp_punch_payload(intent);
        let mut index = 0usize;
        loop {
            let elapsed = started_at.elapsed();
            if elapsed > max_duration {
                log::trace!("quic udp punch stopped remote={} reason=deadline", remote);
                return;
            }
            let wait_duration = next_offset.saturating_sub(elapsed);
            if !wait_duration.is_zero() {
                let closed = self.close_notify.notified();
                tokio::pin!(closed);
                if self.closed.load(Ordering::SeqCst) {
                    log::trace!(
                        "quic udp punch stopped remote={} reason=listener_close",
                        remote
                    );
                    return;
                }
                tokio::select! {
                    _ = runtime::sleep(wait_duration) => {}
                    _ = &mut closed => {
                        log::trace!(
                            "quic udp punch stopped remote={} reason=listener_close",
                            remote
                        );
                        return;
                    }
                }
            }

            if self.closed.load(Ordering::SeqCst) {
                log::trace!(
                    "quic udp punch stopped remote={} reason=listener_close",
                    remote
                );
                return;
            }
            if started_at.elapsed() > max_duration {
                log::trace!("quic udp punch stopped remote={} reason=deadline", remote);
                return;
            }

            let closed = self.close_notify.notified();
            tokio::pin!(closed);
            if self.closed.load(Ordering::SeqCst) {
                log::trace!(
                    "quic udp punch stopped remote={} reason=listener_close",
                    remote
                );
                return;
            }
            tokio::select! {
                result = self.send_udp_punch_packet(
                    &punch_socket,
                    *remote.addr(),
                    payload.as_slice(),
                ) => {
                    if let Err(err) = result {
                        log::trace!(
                            "quic udp punch send failed remote={} index={} error={}",
                            remote,
                            index,
                            err
                        );
                    }
                }
                _ = &mut closed => {
                    log::trace!(
                        "quic udp punch stopped remote={} reason=listener_close",
                        remote
                    );
                    return;
                }
            }

            let Some(offset) =
                udp_punch_next_offset(next_offset, started_at.elapsed(), max_duration)
            else {
                log::trace!("quic udp punch stopped remote={} reason=deadline", remote);
                return;
            };
            next_offset = offset;
            index += 1;
        }
    }

    pub(crate) async fn run_udp_punch_only(
        &self,
        remote: Endpoint,
        intent: TunnelConnectIntent,
        max_duration: Duration,
    ) {
        self.run_udp_punch_burst(remote, intent, std::time::Instant::now(), max_duration)
            .await;
    }

    pub(crate) fn close(&self) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        self.close_notify.notify_waiters();
        self.nat_probe_waiters.clear();
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
                self.nat_probe_waiters.clone(),
            )),
            Arc::new(quinn::TokioRuntime),
        )
        .map_err(|err| SfoReuseportError::Runtime(err.to_string()))?;

        let runtime = TokioRuntimeHandle::current();
        if !self.register_worker_endpoint(endpoint.clone(), worker_id, runtime, socket) {
            endpoint.close(0_u32.into(), b"close listener");
            return Ok(());
        }
        wait_quic_endpoint_loop(self, endpoint).await;
        Ok(())
    }

    fn register_worker_endpoint(
        &self,
        endpoint: quinn::Endpoint,
        worker_index: usize,
        runtime: TokioRuntimeHandle,
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
            worker_index,
            runtime,
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

pub(crate) fn udp_punch_enabled_for_endpoint(remote: &Endpoint) -> bool {
    remote.protocol() == Protocol::Quic
        && rendezvous_eligible_area(remote)
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

fn udp_punch_next_offset(
    current_offset: Duration,
    elapsed: Duration,
    max_duration: Duration,
) -> Option<Duration> {
    let regular_next = current_offset.checked_add(UDP_PUNCH_INTERVAL)?;
    let next_offset = if regular_next > elapsed {
        regular_next
    } else {
        let earliest_next = elapsed.checked_add(UDP_PUNCH_INTERVAL)?;
        let overdue = earliest_next.checked_sub(regular_next)?;
        let interval_nanos = UDP_PUNCH_INTERVAL.as_nanos();
        let overdue_nanos = overdue.as_nanos();
        let intervals_to_skip = 1 + (overdue_nanos - 1) / interval_nanos;
        let intervals_to_skip = u32::try_from(intervals_to_skip).ok()?;
        regular_next.checked_add(UDP_PUNCH_INTERVAL.checked_mul(intervals_to_skip)?)?
    };
    (next_offset <= max_duration).then_some(next_offset)
}

pub(crate) fn udp_punch_burst_window(intent: TunnelConnectIntent) -> Duration {
    let _ = intent;
    UDP_PUNCH_EARLY_ERROR_WINDOW
}

fn udp_punch_offsets_for_deadline(
    intent: TunnelConnectIntent,
    max_duration: Duration,
) -> Vec<Duration> {
    let deadline = max_duration;
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
    let connecting = ep
        .connect_with(
            client_config,
            remote.addr().to_owned(),
            remote_name.as_str(),
        )
        .map_err(into_p2p_err!(
            P2pErrorCode::ConnectFailed,
            "quic to {} connect failed",
            remote
        ))?;
    runtime::timeout(timeout, connecting)
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

    #[cfg(feature = "x509")]
    mod lifecycle_regression {
        include!("listener/tests.rs");
    }

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
        let active_offsets = udp_punch_offsets_for_deadline(active, Duration::from_secs(3));
        assert_eq!(active_offsets.first(), Some(&Duration::from_millis(250)));
        assert_eq!(active_offsets.last(), Some(&Duration::from_secs(3)));
        assert_eq!(active_offsets.len(), 56);
        assert!(
            active_offsets
                .windows(2)
                .all(|pair| pair[1] - pair[0] == Duration::from_millis(50))
        );
        assert_eq!(
            udp_punch_offsets_for_deadline(active, Duration::from_millis(300)),
            vec![Duration::from_millis(250), Duration::from_millis(300)]
        );
        let reverse_offsets = udp_punch_offsets_for_deadline(reverse, Duration::from_secs(3));
        assert_eq!(reverse_offsets.first(), Some(&Duration::ZERO));
        assert_eq!(reverse_offsets.last(), Some(&Duration::from_secs(3)));
        assert_eq!(reverse_offsets.len(), 61);
        assert!(
            reverse_offsets
                .windows(2)
                .all(|pair| pair[1] - pair[0] == Duration::from_millis(50))
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
        let punch_local = punch_socket.local_addr().unwrap();
        if let Ok(listener_socket) = server.listener_socket() {
            assert_eq!(listener_socket.local_addr().unwrap(), punch_local);
        } else {
            assert_ne!(punch_local.port(), 0);
        }
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
