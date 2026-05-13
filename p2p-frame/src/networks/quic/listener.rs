use super::tunnel::QuicTunnel;
use crate::endpoint::{Endpoint, EndpointArea, Protocol, is_non_lan_ipv4_addr};
use crate::error::{P2pErrorCode, P2pResult, into_p2p_err, p2p_err};
use crate::executor::{Executor, SpawnHandle};
use crate::finder::DeviceCache;
use crate::networks::{
    QuicCongestionAlgorithm, TunnelConnectIntent, TunnelForm, TunnelListener, TunnelRef,
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
use socket2::{Domain, Protocol as SocketProtocol, SockAddr, Socket, Type};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::sync::Mutex as AsyncMutex;
use tokio::sync::{Notify, mpsc};

const UDP_PUNCH_PAYLOAD_MIN_LEN: usize = 5;
const UDP_PUNCH_PAYLOAD_MAX_LEN: usize = 30;
const UDP_PUNCH_NON_QUIC_FIXED_BIT: u8 = 0x40;
const UDP_PUNCH_INTERVAL: Duration = Duration::from_millis(50);
const UDP_PUNCH_ACTIVE_START_OFFSET: Duration = Duration::from_millis(250);
const UDP_PUNCH_REVERSE_START_OFFSET: Duration = Duration::ZERO;
const UDP_PUNCH_DEADLINE: Duration = Duration::from_secs(1);

struct QuicTunnelListenerState {
    local: Option<Endpoint>,
    outer: Option<Endpoint>,
    socket: Option<quinn::Endpoint>,
    punch_socket: Option<Arc<UdpSocket>>,
    mapping_port: Option<u16>,
}

pub(crate) struct QuicTunnelListener {
    cert_cache: P2pIdentityCertCacheRef,
    cert_resolver: ServerCertResolverRef,
    cert_factory: P2pIdentityCertFactoryRef,
    congestion_algorithm: QuicCongestionAlgorithm,
    state: RwLock<QuicTunnelListenerState>,
    accepted: AsyncMutex<mpsc::UnboundedReceiver<P2pResult<TunnelRef>>>,
    accepted_tx: Mutex<Option<mpsc::UnboundedSender<P2pResult<TunnelRef>>>>,
    accept_task: Mutex<Option<SpawnHandle<()>>>,
    close_notify: Notify,
    closed: AtomicBool,
}

impl QuicTunnelListener {
    pub(crate) fn new(
        cert_cache: P2pIdentityCertCacheRef,
        cert_resolver: ServerCertResolverRef,
        cert_factory: P2pIdentityCertFactoryRef,
        congestion_algorithm: QuicCongestionAlgorithm,
    ) -> Arc<Self> {
        let (accepted_tx, accepted_rx) = mpsc::unbounded_channel();
        Arc::new(Self {
            cert_cache,
            cert_resolver,
            cert_factory,
            congestion_algorithm,
            state: RwLock::new(QuicTunnelListenerState {
                local: None,
                outer: None,
                socket: None,
                punch_socket: None,
                mapping_port: None,
            }),
            accepted: AsyncMutex::new(accepted_rx),
            accepted_tx: Mutex::new(Some(accepted_tx)),
            accept_task: Mutex::new(None),
            close_notify: Notify::new(),
            closed: AtomicBool::new(false),
        })
    }

    pub(crate) fn local(&self) -> Endpoint {
        self.state.read().unwrap().local.unwrap()
    }

    pub(crate) fn bound_local(&self) -> Endpoint {
        let endpoint = self.state.read().unwrap().socket.clone().unwrap();
        Endpoint::from((
            crate::endpoint::Protocol::Quic,
            endpoint.local_addr().unwrap(),
        ))
    }

    pub(crate) fn mapping_port(&self) -> Option<u16> {
        self.state.read().unwrap().mapping_port
    }

    pub(crate) fn quic_ep(&self) -> quinn::Endpoint {
        self.state.read().unwrap().socket.clone().unwrap()
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
                let sent = try_send_udp_punch_packet(
                    punch_socket.as_ref(),
                    *remote.addr(),
                    payload.as_slice(),
                )
                .await;
                if !sent {
                    log::trace!(
                        "quic udp punch send failed remote={} index={}",
                        remote,
                        index
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
        self.accepted_tx.lock().unwrap().take();
        if let Some(task) = self.accept_task.lock().unwrap().take() {
            task.abort();
        }
        let ep = {
            let mut state = self.state.write().unwrap();
            state.punch_socket.take();
            state.socket.take()
        };
        if let Some(ep) = ep {
            ep.close(0_u32.into(), b"close all listeners");
        }
    }

    pub(crate) async fn bind(
        &self,
        local: Endpoint,
        out: Option<Endpoint>,
        mapping_port: Option<u16>,
        reuse_address: bool,
    ) -> P2pResult<()> {
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

        let sockaddr: SockAddr = local.addr().to_owned().into();
        let domain = match local.addr() {
            std::net::SocketAddr::V4(_) => Domain::IPV4,
            std::net::SocketAddr::V6(_) => Domain::IPV6,
        };
        let socket = Socket::new(domain, Type::DGRAM, Some(SocketProtocol::UDP)).map_err(
            into_p2p_err!(P2pErrorCode::QuicError, "create quic socket failed"),
        )?;
        socket.set_nonblocking(true).map_err(into_p2p_err!(
            P2pErrorCode::QuicError,
            "set quic socket nonblocking failed"
        ))?;
        #[cfg(target_os = "linux")]
        if reuse_address {
            socket.set_reuse_address(true).map_err(into_p2p_err!(
                P2pErrorCode::QuicError,
                "set reuse address failed"
            ))?;
        }
        socket.bind(&sockaddr).map_err(into_p2p_err!(
            P2pErrorCode::AlreadyExists,
            "bind {} error",
            local
        ))?;
        #[cfg(unix)]
        let socket = unsafe {
            use std::os::fd::{FromRawFd, IntoRawFd};

            std::net::UdpSocket::from_raw_fd(socket.into_raw_fd())
        };
        #[cfg(windows)]
        let socket = unsafe {
            use std::os::windows::io::{FromRawSocket, IntoRawSocket};

            std::net::UdpSocket::from_raw_socket(socket.into_raw_socket())
        };
        let punch_socket = socket.try_clone().map_err(into_p2p_err!(
            P2pErrorCode::QuicError,
            "clone quic punch socket failed"
        ))?;
        let punch_socket = Arc::new(UdpSocket::from_std(punch_socket).map_err(into_p2p_err!(
            P2pErrorCode::QuicError,
            "create quic punch socket failed"
        ))?);
        let endpoint = quinn::Endpoint::new(
            quinn::EndpointConfig::default(),
            Some(server_config),
            socket,
            Arc::new(quinn::TokioRuntime),
        )
        .map_err(into_p2p_err!(
            P2pErrorCode::QuicError,
            "Create quic server error"
        ))?;

        let mut state = self.state.write().unwrap();
        state.local = Some(local);
        state.outer = out;
        state.socket = Some(endpoint);
        state.punch_socket = Some(punch_socket);
        state.mapping_port = mapping_port;

        Ok(())
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

    pub(crate) fn start(self: &Arc<Self>) {
        if self.closed.load(Ordering::SeqCst) {
            return;
        }
        let this = self.clone();
        let socket = self.quic_ep();
        let handle = Executor::spawn_with_handle(async move {
            loop {
                match socket.accept().await {
                    Some(conn) => {
                        let result = this.accept_connection(conn).await;
                        let sender = this.accepted_tx.lock().unwrap().clone();
                        let Some(tx) = sender else {
                            break;
                        };
                        if tx.send(result).is_err() {
                            break;
                        }
                    }
                    None => break,
                }
            }
        })
        .unwrap();
        *self.accept_task.lock().unwrap() = Some(handle);
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
    let payload_len =
        rand::rng().random_range(UDP_PUNCH_PAYLOAD_MIN_LEN..=UDP_PUNCH_PAYLOAD_MAX_LEN);
    let mut payload = random::<[u8; UDP_PUNCH_PAYLOAD_MAX_LEN]>();
    payload[0] &= !UDP_PUNCH_NON_QUIC_FIXED_BIT;
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
    socket: &UdpSocket,
    remote: std::net::SocketAddr,
    payload: &[u8],
) -> bool {
    match socket.send_to(payload, remote).await {
        Ok(_) => true,
        Err(_) => false,
    }
}

#[async_trait::async_trait]
impl TunnelListener for QuicTunnelListener {
    async fn accept_tunnel(&self) -> P2pResult<TunnelRef> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(p2p_err!(P2pErrorCode::ErrorState, "tunnel listener closed"));
        }
        let mut accepted = self.accepted.lock().await;
        let closed = self.close_notify.notified();
        tokio::pin!(closed);
        tokio::select! {
            result = accepted.recv() => {
                match result {
                    Some(result) => result,
                    None => Err(p2p_err!(P2pErrorCode::ErrorState, "tunnel listener closed")),
                }
            }
            _ = &mut closed => {
                Err(p2p_err!(P2pErrorCode::ErrorState, "tunnel listener closed"))
            }
        }
    }
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
    use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket as StdUdpSocket};

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
                .all(|payload| payload[0] & UDP_PUNCH_NON_QUIC_FIXED_BIT == 0)
        );
        assert!(
            payloads
                .windows(2)
                .any(|pair| pair[0].len() != pair[1].len())
        );
        assert!(payloads.windows(2).any(|pair| pair[0] != pair[1]));
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

    #[tokio::test]
    async fn udp_punch_async_socket_preserves_listener_local_port() {
        let socket = StdUdpSocket::bind("127.0.0.1:0").unwrap();
        socket.set_nonblocking(true).unwrap();
        let punch_socket = UdpSocket::from_std(socket.try_clone().unwrap()).unwrap();

        assert_eq!(
            socket.local_addr().unwrap(),
            punch_socket.local_addr().unwrap()
        );
    }

    #[tokio::test]
    async fn udp_punch_send_failure_is_best_effort() {
        let socket = StdUdpSocket::bind("127.0.0.1:0").unwrap();
        socket.set_nonblocking(true).unwrap();
        let socket = UdpSocket::from_std(socket).unwrap();
        let invalid_remote = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0));
        let payload = udp_punch_payload(TunnelConnectIntent::active_logical(TunnelId::from(7)));

        assert!(!try_send_udp_punch_packet(&socket, invalid_remote, payload.as_slice()).await);
    }
}
