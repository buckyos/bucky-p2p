use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::{Arc, Mutex, Once};
use std::time::Duration;

use bucky_time::bucky_time_now;
use p2p_frame::ConnectDirection;
use p2p_frame::endpoint::{EndpointArea, Protocol};
use p2p_frame::error::{P2pError, P2pErrorCode, P2pResult};
use p2p_frame::nat_type::{NatMappingObservation, NatProfile};
use p2p_frame::p2p_identity::{P2pId, P2pIdentityRef};
use p2p_frame::sn::service::SnServerRef;
use p2p_frame::x509::{X509IdentityCertFactory, X509IdentityFactory};
use tokio::net::UdpSocket;

use super::fixture::{
    AbsoluteDeadline, ConnectionInfoRecorder, DEFAULT_FLOW_TIMEOUT, DEFAULT_SETUP_TIMEOUT,
    RealNode, SETUP_MAX_RETRIES, assert_bidirectional_unique_payload, connect_stream_pair_from_id,
    dynamic_loopback_endpoint, fixture_error, sn_entry, start_node, start_sn, stop_partial,
    unique_purpose, x509_identity,
};

const PROBE_MAGIC: [u8; 4] = *b"PNAT";
const PROBE_VERSION: u8 = 2;
const PROBE_REQUEST: u8 = 1;
const PROBE_RESPONSE: u8 = 2;
const PROBE_TOKEN_OFFSET: usize = 8;
const PROBE_IPV4_OFFSET: usize = 24;
const PROBE_PORT_OFFSET: usize = 28;
const PROBE_SIGNATURE_LEN_OFFSET: usize = 30;
const PROBE_SIGNATURE_OFFSET: usize = 32;
const PROBE_PACKET_LEN: usize = 1200;
const PROBE_SIGNATURE_DOMAIN: &[u8] = b"CYFS-P2P/PNAT/RESPONSE/V2\0";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MappingStyle {
    Stable,
    Changed,
}

struct RendezvousTrace {
    records: Mutex<Vec<String>>,
}

static RENDEZVOUS_TRACE: RendezvousTrace = RendezvousTrace {
    records: Mutex::new(Vec::new()),
};
static RENDEZVOUS_TRACE_INIT: Once = Once::new();

struct RendezvousTraceLogger;

impl log::Log for RendezvousTraceLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Debug
    }

    fn log(&self, record: &log::Record<'_>) {
        let message = record.args().to_string();
        if message.contains("event=sn_rendezvous_") {
            RENDEZVOUS_TRACE.records.lock().unwrap().push(message);
        }
    }

    fn flush(&self) {}
}

fn ensure_rendezvous_trace() {
    RENDEZVOUS_TRACE_INIT.call_once(|| {
        log::set_logger(&RendezvousTraceLogger).unwrap();
        log::set_max_level(log::LevelFilter::Debug);
    });
}

fn take_rendezvous_trace() -> Vec<String> {
    std::mem::take(&mut *RENDEZVOUS_TRACE.records.lock().unwrap())
}

fn current_rendezvous_trace() -> Vec<String> {
    RENDEZVOUS_TRACE.records.lock().unwrap().clone()
}

async fn wait_for_rendezvous_event(
    deadline: AbsoluteDeadline,
    matcher: impl Fn(&str) -> bool,
    context: &str,
) -> P2pResult<String> {
    loop {
        if let Some(message) = current_rendezvous_trace()
            .iter()
            .find(|message| matcher(message))
        {
            return Ok(message.clone());
        }
        if deadline.remaining(context).is_err() {
            return Err(fixture_error(
                P2pErrorCode::Timeout,
                format!("rendezvous event not observed while {context}"),
            ));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

struct MappingState {
    styles: Mutex<HashMap<u16, MappingStyle>>,
}

impl MappingState {
    fn new() -> Self {
        Self {
            styles: Mutex::new(HashMap::new()),
        }
    }

    fn register(&self, port: u16, style: MappingStyle) -> P2pResult<()> {
        if style == MappingStyle::Changed {
            let max_observed = u32::from(port) + 2;
            if max_observed > u32::from(u16::MAX) {
                return Err(fixture_error(
                    P2pErrorCode::OutOfLimit,
                    format!("changed mapping port window overflows u16 at base {port}"),
                ));
            }
        }
        self.styles.lock().unwrap().insert(port, style);
        Ok(())
    }

    fn observed(&self, source: SocketAddr, reflector_index: usize) -> SocketAddr {
        let style = self
            .styles
            .lock()
            .unwrap()
            .get(&source.port())
            .copied()
            .unwrap_or(MappingStyle::Stable);
        let base = source.port();
        let observed = match style {
            MappingStyle::Stable => base,
            MappingStyle::Changed => base + 1 + reflector_index as u16,
        };
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, observed))
    }
}

struct MappingProbeFixture {
    sockets: Vec<Arc<UdpSocket>>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl Drop for MappingProbeFixture {
    fn drop(&mut self) {
        for task in self.tasks.drain(..) {
            task.abort();
        }
    }
}

impl MappingProbeFixture {
    async fn start(
        caller_port: u16,
        target_port: u16,
        caller_changed: bool,
        target_changed: bool,
        sn_identity: P2pIdentityRef,
    ) -> P2pResult<Self> {
        let state = Arc::new(MappingState::new());
        state.register(
            caller_port,
            if caller_changed {
                MappingStyle::Changed
            } else {
                MappingStyle::Stable
            },
        )?;
        state.register(
            target_port,
            if target_changed {
                MappingStyle::Changed
            } else {
                MappingStyle::Stable
            },
        )?;

        let mut sockets = Vec::with_capacity(2);
        let mut tasks = Vec::with_capacity(2);
        let signature_len = sn_identity.sign(b"PNAT matrix signature length")?.len();
        if signature_len == 0 || signature_len > PROBE_PACKET_LEN - PROBE_SIGNATURE_OFFSET {
            return Err(fixture_error(
                P2pErrorCode::OutOfLimit,
                format!("matrix PNAT signature length is invalid: {signature_len}"),
            ));
        }
        for index in 0..2 {
            let socket = Arc::new(
                UdpSocket::bind(SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)))
                    .await
                    .map_err(|err| fixture_error(P2pErrorCode::IoError, err.to_string()))?,
            );
            let task_state = state.clone();
            let task_socket = socket.clone();
            let task_identity = sn_identity.clone();
            tasks.push(tokio::spawn(async move {
                let _ =
                    reflector_loop(task_state, task_socket, index, task_identity, signature_len)
                        .await;
            }));
            sockets.push(socket);
        }
        Ok(Self { sockets, tasks })
    }

    fn ports(&self) -> Vec<u16> {
        self.sockets
            .iter()
            .filter_map(|socket| socket.local_addr().ok())
            .map(|addr| addr.port())
            .collect()
    }
}

async fn reflector_loop(
    state: Arc<MappingState>,
    socket: Arc<UdpSocket>,
    reflector_index: usize,
    sn_identity: P2pIdentityRef,
    signature_len: usize,
) -> P2pResult<()> {
    let mut packet = [0u8; PROBE_PACKET_LEN + 1];
    loop {
        let (len, source) = socket.recv_from(&mut packet).await.map_err(|err| {
            fixture_error(
                P2pErrorCode::IoError,
                format!("matrix probe reflector recv: {err}"),
            )
        })?;
        if len != PROBE_PACKET_LEN
            || packet[..4] != PROBE_MAGIC
            || packet[4] != PROBE_VERSION
            || packet[5] != PROBE_REQUEST
            || packet[6] != 0
            || packet[7] != 0
            || packet[24..].iter().any(|byte| *byte != 0)
            || !source.is_ipv4()
        {
            continue;
        }
        let token: [u8; 16] = packet[PROBE_TOKEN_OFFSET..PROBE_TOKEN_OFFSET + 16]
            .try_into()
            .expect("fixed-length PNAT token");
        let observed = state.observed(source, reflector_index);
        let mut response = [0u8; PROBE_PACKET_LEN];
        response[..4].copy_from_slice(&PROBE_MAGIC);
        response[4] = PROBE_VERSION;
        response[5] = PROBE_RESPONSE;
        response[PROBE_TOKEN_OFFSET..PROBE_TOKEN_OFFSET + 16].copy_from_slice(&token);
        let SocketAddr::V4(observed) = observed else {
            continue;
        };
        response[PROBE_IPV4_OFFSET..PROBE_IPV4_OFFSET + 4].copy_from_slice(&observed.ip().octets());
        response[PROBE_PORT_OFFSET..PROBE_PORT_OFFSET + 2]
            .copy_from_slice(&observed.port().to_be_bytes());
        response[PROBE_SIGNATURE_LEN_OFFSET..PROBE_SIGNATURE_OFFSET]
            .copy_from_slice(&(signature_len as u16).to_be_bytes());
        let signer_id = sn_identity.get_id();
        let mut preimage = PROBE_SIGNATURE_DOMAIN.to_vec();
        preimage.extend_from_slice(&(signer_id.as_slice().len() as u16).to_be_bytes());
        preimage.extend_from_slice(signer_id.as_slice());
        preimage.extend_from_slice(&response[..PROBE_SIGNATURE_OFFSET]);
        let signature = sn_identity.sign(&preimage)?;
        if signature.len() != signature_len {
            return Err(fixture_error(
                P2pErrorCode::InvalidSignature,
                format!(
                    "matrix PNAT signature length drifted: {} != {signature_len}",
                    signature.len()
                ),
            ));
        }
        response[PROBE_SIGNATURE_OFFSET..PROBE_SIGNATURE_OFFSET + signature.len()]
            .copy_from_slice(&signature);
        socket
            .send_to(&response, source)
            .await
            .map_err(|err| fixture_error(P2pErrorCode::IoError, err.to_string()))?;
    }
}

struct NatMatrixTopology {
    server: SnServerRef,
    caller: RealNode,
    target: RealNode,
    _probe: MappingProbeFixture,
}

impl Drop for NatMatrixTopology {
    fn drop(&mut self) {
        self.caller.stack.sn_client().stop();
        self.target.stack.sn_client().stop();
        self.server.stop();
    }
}

async fn wait_for_profiles(
    caller: &RealNode,
    target_id: &P2pId,
    expected_caller: NatMappingObservation,
    expected_target: NatMappingObservation,
    deadline: AbsoluteDeadline,
) -> P2pResult<(NatProfile, NatProfile)> {
    loop {
        let query = deadline
            .p2p(
                "querying real SN during matrix profile readiness",
                caller.stack.sn_client().query_with_context(target_id),
            )
            .await?;
        let now = bucky_time_now();
        let local = query.local_net_profile.mapping_at(now);
        let remote = query
            .response
            .net_profile
            .as_ref()
            .map(|profile| profile.mapping_at(now))
            .unwrap_or(NatMappingObservation::Unknown);
        if local == expected_caller && remote == expected_target {
            return Ok((
                query.local_net_profile,
                query
                    .response
                    .net_profile
                    .expect("expected target profile is present"),
            ));
        }
        if deadline
            .remaining("waiting for production NAT profiles")
            .is_err()
        {
            return Err(fixture_error(
                P2pErrorCode::Timeout,
                format!(
                    "production profiles never matched: local={local:?} remote={remote:?} \
                     expected_local={expected_caller:?} expected_remote={expected_target:?}"
                ),
            ));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn start_nat_matrix_topology(
    caller_changed: bool,
    target_changed: bool,
    caller_area: EndpointArea,
    target_area: EndpointArea,
    deadline: AbsoluteDeadline,
) -> P2pResult<NatMatrixTopology> {
    for attempt in 0..SETUP_MAX_RETRIES {
        let identity_factory = Arc::new(X509IdentityFactory);
        let cert_factory = Arc::new(X509IdentityCertFactory);
        let suffix = attempt;

        let caller_endpoint = match dynamic_loopback_endpoint(Protocol::Quic, caller_area) {
            Ok(endpoint) => endpoint,
            Err(error) => return Err(error),
        };
        let target_endpoint = match dynamic_loopback_endpoint(Protocol::Quic, target_area) {
            Ok(endpoint) => endpoint,
            Err(error) => return Err(error),
        };
        let sn_endpoint = match dynamic_loopback_endpoint(Protocol::Quic, EndpointArea::Lan) {
            Ok(endpoint) => endpoint,
            Err(error) => return Err(error),
        };
        let sn_identity = match x509_identity(format!("nat-matrix-sn-{suffix}"), sn_endpoint) {
            Ok(identity) => identity,
            Err(error) => return Err(error),
        };
        let probe = match MappingProbeFixture::start(
            caller_endpoint.addr().port(),
            target_endpoint.addr().port(),
            caller_changed,
            target_changed,
            sn_identity.clone(),
        )
        .await
        {
            Ok(probe) => probe,
            Err(error) => return Err(error),
        };
        let server = match deadline
            .p2p(
                "starting real NAT-matrix SN",
                start_sn(
                    sn_identity.clone(),
                    identity_factory.clone(),
                    cert_factory.clone(),
                ),
            )
            .await
        {
            Ok(server) => server,
            Err(error) if retryable(&error) => continue,
            Err(error) => return Err(error),
        };
        server.service().set_nat_probe_ports(probe.ports());

        let sn = match sn_entry(&sn_identity) {
            Ok(sn) => sn,
            Err(error) => return Err(error),
        };
        let caller_identity =
            match x509_identity(format!("nat-matrix-caller-{suffix}"), caller_endpoint) {
                Ok(identity) => identity,
                Err(error) => return Err(error),
            };
        let caller_id = caller_identity.get_id();
        let caller_info = ConnectionInfoRecorder::new();
        let caller_stack = match deadline
            .p2p(
                "starting real NAT-matrix caller",
                start_node(
                    caller_identity.clone(),
                    sn.clone(),
                    identity_factory.clone(),
                    cert_factory.clone(),
                    caller_info.clone(),
                ),
            )
            .await
        {
            Ok(stack) => stack,
            Err(error) if retryable(&error) => {
                server.stop();
                continue;
            }
            Err(error) => {
                server.stop();
                return Err(error);
            }
        };

        let target_identity =
            match x509_identity(format!("nat-matrix-target-{suffix}"), target_endpoint) {
                Ok(identity) => identity,
                Err(error) => {
                    stop_partial(&server, &[&caller_stack]);
                    return Err(error);
                }
            };
        let target_id = target_identity.get_id();
        let target_info = ConnectionInfoRecorder::new();
        let target_stack = match deadline
            .p2p(
                "starting real NAT-matrix target",
                start_node(
                    target_identity.clone(),
                    sn,
                    identity_factory.clone(),
                    cert_factory.clone(),
                    target_info.clone(),
                ),
            )
            .await
        {
            Ok(stack) => stack,
            Err(error) if retryable(&error) => {
                stop_partial(&server, &[&caller_stack]);
                continue;
            }
            Err(error) => {
                stop_partial(&server, &[&caller_stack]);
                return Err(error);
            }
        };

        let caller_online = deadline
            .p2p(
                "waiting for NAT-matrix caller online",
                caller_stack.wait_online(Some(
                    deadline.remaining("waiting for caller online budget")?,
                )),
            )
            .await;
        if let Err(error) = caller_online {
            stop_partial(&server, &[&caller_stack, &target_stack]);
            if retryable(&error) {
                continue;
            }
            return Err(error);
        }
        let target_online = deadline
            .p2p(
                "waiting for NAT-matrix target online",
                target_stack.wait_online(Some(
                    deadline.remaining("waiting for target online budget")?,
                )),
            )
            .await;
        if let Err(error) = target_online {
            stop_partial(&server, &[&caller_stack, &target_stack]);
            if retryable(&error) {
                continue;
            }
            return Err(error);
        }

        let wait_caller = RealNode {
            stack: caller_stack.clone(),
            identity: caller_identity.clone(),
            id: caller_id.clone(),
            endpoint: caller_endpoint,
            connection_info: caller_info.clone(),
        };
        let caller_profiles = wait_for_profiles(
            &wait_caller,
            &target_id,
            if caller_changed {
                NatMappingObservation::SymmetricLike
            } else {
                NatMappingObservation::NonSymmetricLike
            },
            if target_changed {
                NatMappingObservation::SymmetricLike
            } else {
                NatMappingObservation::NonSymmetricLike
            },
            deadline,
        )
        .await;
        if let Err(error) = caller_profiles {
            stop_partial(&server, &[&caller_stack, &target_stack]);
            if retryable(&error) {
                continue;
            }
            return Err(error);
        }

        return Ok(NatMatrixTopology {
            server,
            caller: RealNode {
                stack: caller_stack,
                identity: caller_identity,
                id: caller_id,
                endpoint: caller_endpoint,
                connection_info: caller_info,
            },
            target: RealNode {
                stack: target_stack,
                identity: target_identity,
                id: target_id,
                endpoint: target_endpoint,
                connection_info: target_info,
            },
            _probe: probe,
        });
    }

    Err(fixture_error(
        P2pErrorCode::AddrInUse,
        format!("NAT matrix topology exhausted {SETUP_MAX_RETRIES} retries"),
    ))
}

fn retryable(error: &P2pError) -> bool {
    matches!(
        error.code(),
        P2pErrorCode::AddrInUse | P2pErrorCode::AddrNotAvailable | P2pErrorCode::AlreadyExists
    )
}

fn assert_matrix_profile(profile: &NatProfile, changed: bool, label: &str) {
    let now = bucky_time_now();
    if changed {
        assert_eq!(
            profile.mapping_at(now),
            NatMappingObservation::SymmetricLike,
            "{label} must be SymmetricLike from real single-socket observations"
        );
        let hint = profile.prediction_hint.as_ref().unwrap_or_else(|| {
            panic!("{label} SymmetricLike profile must carry a prediction hint")
        });
        assert_eq!(
            hint.first_observed.addr().ip(),
            hint.last_observed.addr().ip(),
            "{label} changed mappings must stay on one destination IP"
        );
        assert_ne!(
            hint.first_observed.addr().port(),
            hint.last_observed.addr().port(),
            "{label} changed mappings must differ across targets"
        );
        assert!(
            profile.usable_prediction_hint(now).is_some(),
            "{label} SymmetricLike profile must expose a usable linear prediction hint"
        );
    } else {
        assert_eq!(
            profile.mapping_at(now),
            NatMappingObservation::NonSymmetricLike,
            "{label} must be NonSymmetricLike from real single-socket observations"
        );
        assert!(
            profile.prediction_hint.is_none(),
            "{label} NonSymmetricLike profile must not carry a prediction hint"
        );
    }
}

#[derive(Clone, Copy)]
struct MatrixCase {
    name: &'static str,
    caller_changed: bool,
    target_changed: bool,
    caller_area: EndpointArea,
    target_area: EndpointArea,
    expected_operation: &'static str,
    expect_predict: bool,
    expect_zero_request_endpoints: bool,
    require_connected: bool,
}

/// Bounded restart budget for one matrix row. A row whose flow resolves
/// without a production rendezvous request event is an invalid branch sample
/// (for example a transient profile-less internal SN lookup fell back to a
/// direct loopback tunnel), so the case is re-run with a fresh topology
/// instead of being treated as production branch evidence.
const MATRIX_EVIDENCE_MAX_RETRIES: usize = 5;

fn dump_rendezvous_trace() {
    for message in current_rendezvous_trace() {
        eprintln!("  trace: {message}");
    }
}

fn rendezvous_request_in_trace(target_id: &str) -> Option<String> {
    current_rendezvous_trace()
        .iter()
        .find(|message| {
            message.contains("event=sn_rendezvous_requesting")
                && message.contains(&format!("remote={target_id}"))
        })
        .cloned()
}

fn matrix_cases() -> Vec<MatrixCase> {
    vec![
        MatrixCase {
            name: "callee-public",
            caller_changed: false,
            target_changed: false,
            caller_area: EndpointArea::Lan,
            target_area: EndpointArea::Wan,
            expected_operation: "WaitIncoming",
            expect_predict: false,
            expect_zero_request_endpoints: true,
            require_connected: true,
        },
        MatrixCase {
            name: "caller-public",
            caller_changed: false,
            target_changed: false,
            caller_area: EndpointArea::Wan,
            target_area: EndpointArea::Lan,
            expected_operation: "ReverseConnectOnly",
            expect_predict: false,
            expect_zero_request_endpoints: false,
            require_connected: true,
        },
        MatrixCase {
            name: "non-symmetric/non-symmetric",
            caller_changed: false,
            target_changed: false,
            caller_area: EndpointArea::Lan,
            target_area: EndpointArea::Lan,
            expected_operation: "PunchOnly",
            expect_predict: false,
            expect_zero_request_endpoints: false,
            require_connected: true,
        },
        MatrixCase {
            name: "non-symmetric/symmetric",
            caller_changed: false,
            target_changed: true,
            caller_area: EndpointArea::Lan,
            target_area: EndpointArea::Lan,
            expected_operation: "PunchAndReverseConnect",
            expect_predict: true,
            expect_zero_request_endpoints: false,
            require_connected: false,
        },
        MatrixCase {
            name: "symmetric/non-symmetric",
            caller_changed: true,
            target_changed: false,
            caller_area: EndpointArea::Lan,
            target_area: EndpointArea::Lan,
            expected_operation: "PunchOnly",
            expect_predict: false,
            expect_zero_request_endpoints: false,
            require_connected: false,
        },
        MatrixCase {
            name: "symmetric/symmetric",
            caller_changed: true,
            target_changed: true,
            caller_area: EndpointArea::Lan,
            target_area: EndpointArea::Lan,
            expected_operation: "PunchOnly",
            expect_predict: true,
            expect_zero_request_endpoints: false,
            require_connected: false,
        },
    ]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_strategy_matrix_executes_production_branches() {
    ensure_rendezvous_trace();

    for case in matrix_cases() {
        let mut exhausted = true;
        'case_attempts: for attempt in 0..MATRIX_EVIDENCE_MAX_RETRIES {
            take_rendezvous_trace();
            let deadline = AbsoluteDeadline::after(DEFAULT_SETUP_TIMEOUT + DEFAULT_FLOW_TIMEOUT);
            let topology = start_nat_matrix_topology(
                case.caller_changed,
                case.target_changed,
                case.caller_area,
                case.target_area,
                deadline,
            )
            .await
            .unwrap_or_else(|error| {
                panic!(
                    "case={} setup failed: code={:?} message={}",
                    case.name,
                    error.code(),
                    error.msg()
                )
            });

            let now = bucky_time_now();
            let caller_profile = topology
                .caller
                .stack
                .sn_client()
                .query_with_context(&topology.target.id)
                .await
                .expect("production query for matrix profile evidence")
                .local_net_profile;
            let target_profile = topology
                .caller
                .stack
                .sn_client()
                .query_with_context(&topology.target.id)
                .await
                .expect("production query for matrix profile evidence")
                .response
                .net_profile
                .expect("target profile published through production Query");
            assert_matrix_profile(&caller_profile, case.caller_changed, "caller");
            assert_matrix_profile(&target_profile, case.target_changed, "target");
            let purpose = unique_purpose(case.name);
            let flow_deadline = AbsoluteDeadline::after(DEFAULT_FLOW_TIMEOUT);
            let flow = connect_stream_pair_from_id(
                &topology.caller,
                &topology.target,
                purpose,
                flow_deadline,
            )
            .await;

            let connected = match flow {
                Ok(mut pair) => {
                    assert_bidirectional_unique_payload(&mut pair, case.name, flow_deadline)
                        .await
                        .expect("reachable matrix row must complete bidirectional unique payload");
                    true
                }
                Err(error) => {
                    assert!(
                        !case.require_connected,
                        "case={} required connected but failed with code={:?} msg={}",
                        case.name,
                        error.code(),
                        error.msg()
                    );
                    eprintln!(
                        "case={} connected=false bounded-error-code={:?}",
                        case.name,
                        error.code()
                    );
                    false
                }
            };
            if connected {
                let info = topology
                    .caller
                    .connection_info
                    .latest(&topology.target.id)
                    .expect("connected matrix row must publish connection info");
                assert!(
                    matches!(
                        info.direct,
                        ConnectDirection::Direct | ConnectDirection::Reverse
                    ),
                    "case={} unexpected connection direction {:?}",
                    case.name,
                    info.direct
                );
            }

            let caller_id = topology.caller.id.to_string();
            let target_id = topology.target.id.to_string();

            // The request event is logged before the SN round trip, so once
            // the flow has resolved it is deterministic evidence either way.
            // A transient profile-less internal SN lookup can still make the
            // caller fall back to a direct loopback tunnel; that sample never
            // executed the production rendezvous branch, so restart the case
            // instead of treating it as branch evidence.
            if rendezvous_request_in_trace(&target_id).is_none() {
                eprintln!(
                    "case={} attempt={} connected={connected} invalid branch sample: \
                     production rendezvous request event not observed; restarting case",
                    case.name, attempt
                );
                continue 'case_attempts;
            }

            let event_deadline = AbsoluteDeadline::after(Duration::from_secs(8));
            let request = wait_for_rendezvous_event(
                event_deadline,
                |message| {
                    message.contains("event=sn_rendezvous_requesting")
                        && message.contains(&format!("remote={target_id}"))
                },
                "waiting for production rendezvous request evidence",
            )
            .await
            .unwrap_or_else(|error| {
                dump_rendezvous_trace();
                panic!(
                    "case={} production rendezvous request must be sent (request-sent evidence): {}",
                    case.name,
                    error.msg()
                )
            });
            let action = wait_for_rendezvous_event(
                event_deadline,
                |message| {
                    message.contains("event=sn_rendezvous_action_armed")
                        && message.contains(&format!("initiator={caller_id}"))
                        && message.contains(&format!("operation={}", case.expected_operation))
                },
                "waiting for production target action-armed evidence",
            )
            .await
            .unwrap_or_else(|error| {
                dump_rendezvous_trace();
                panic!(
                    "case={} production target action must be armed (action-armed evidence): {}",
                    case.name,
                    error.msg()
                )
            });
            let _ = wait_for_rendezvous_event(
                event_deadline,
                |message| {
                    message.contains("event=sn_rendezvous_target_finished")
                        && message.contains(&format!("remote={caller_id}"))
                },
                "waiting for production target action completion evidence",
            )
            .await
            .unwrap_or_else(|error| {
                dump_rendezvous_trace();
                panic!(
                    "case={} production target action must finish (bounded outcome evidence): {}",
                    case.name,
                    error.msg()
                )
            });
            assert!(
                request.contains(&format!("operation={}", case.expected_operation)),
                "case={} expected operation {} in request event: {}",
                case.name,
                case.expected_operation,
                request
            );
            assert!(
                action.contains(&format!("operation={}", case.expected_operation)),
                "case={} action-armed event operation mismatch: {}",
                case.name,
                action
            );
            assert_eq!(
                request.contains("predict=true"),
                case.expect_predict,
                "case={} prediction request flag mismatch: {}",
                case.name,
                request
            );
            assert_eq!(
                request.contains("endpoint_count=0"),
                case.expect_zero_request_endpoints,
                "case={} rendezvous request endpoint count mismatch: {}",
                case.name,
                request
            );
            eprintln!(
                "case={} selected=true request-sent=true action-armed=true connected={connected} \
                 profile-caller={:?} profile-target={:?} operation={} predict={} attempt={attempt}",
                case.name,
                caller_profile.mapping_at(now),
                target_profile.mapping_at(now),
                case.expected_operation,
                case.expect_predict
            );
            take_rendezvous_trace();
            exhausted = false;
            break 'case_attempts;
        }
        if exhausted {
            dump_rendezvous_trace();
            panic!(
                "case={} exhausted {MATRIX_EVIDENCE_MAX_RETRIES} attempts without production \
                 rendezvous branch evidence (request-sent evidence missing in every sample)",
                case.name
            );
        }
    }
}
