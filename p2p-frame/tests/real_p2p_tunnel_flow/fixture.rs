use std::collections::HashMap;
use std::future::Future;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpListener, UdpSocket};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use p2p_frame::endpoint::{Endpoint, EndpointArea, Protocol};
use p2p_frame::error::{P2pError, P2pErrorCode, P2pResult};
use p2p_frame::networks::TunnelPurpose;
use p2p_frame::p2p_identity::{P2pId, P2pIdentityRef, P2pSn};
use p2p_frame::sn::service::{SnServerRef, SnServiceConfig, create_sn_service};
use p2p_frame::stack::{P2pConfig, P2pStackConfig, P2pStackRef, create_p2p_env, create_p2p_stack};
use p2p_frame::stream::{StreamRead, StreamWrite};
use p2p_frame::x509::{X509IdentityCertFactory, X509IdentityFactory, generate_rsa_x509_identity};
use p2p_frame::{ConnectDirection, P2pConnectionInfo, P2pConnectionInfoCache};
use sfo_reuseport::{ServerRuntime, ServerRuntimeConfig};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Notify;
use tokio::time::Instant;

pub(crate) const DEFAULT_SETUP_TIMEOUT: Duration = Duration::from_secs(30);
pub(crate) const DEFAULT_FLOW_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) const SETUP_MAX_RETRIES: usize = 20;
static UNIQUE_VALUE: AtomicU64 = AtomicU64::new(1);

pub(crate) fn fixture_error(code: P2pErrorCode, context: impl Into<String>) -> P2pError {
    P2pError::new(code, context.into())
}

fn io_error(context: &str, error: std::io::Error) -> P2pError {
    fixture_error(P2pErrorCode::IoError, format!("{context}: {error}"))
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct AbsoluteDeadline {
    at: Instant,
}

impl AbsoluteDeadline {
    pub(crate) fn after(duration: Duration) -> Self {
        Self {
            at: Instant::now() + duration,
        }
    }

    pub(crate) fn remaining(self, context: &str) -> P2pResult<Duration> {
        let remaining = self.at.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            Err(fixture_error(
                P2pErrorCode::Timeout,
                format!("absolute deadline expired while {context}"),
            ))
        } else {
            Ok(remaining)
        }
    }

    pub(crate) async fn p2p<T, F>(self, context: &str, future: F) -> P2pResult<T>
    where
        F: Future<Output = P2pResult<T>>,
    {
        tokio::time::timeout_at(self.at, future)
            .await
            .map_err(|_| {
                fixture_error(
                    P2pErrorCode::Timeout,
                    format!("absolute deadline expired while {context}"),
                )
            })?
    }

    pub(crate) async fn io<T, F>(self, context: &str, future: F) -> P2pResult<T>
    where
        F: Future<Output = std::io::Result<T>>,
    {
        tokio::time::timeout_at(self.at, future)
            .await
            .map_err(|_| {
                fixture_error(
                    P2pErrorCode::Timeout,
                    format!("absolute deadline expired while {context}"),
                )
            })?
            .map_err(|error| io_error(context, error))
    }
}

#[derive(Default)]
pub(crate) struct ConnectionInfoRecorder {
    entries: Mutex<HashMap<P2pId, P2pConnectionInfo>>,
    changed: Notify,
}

impl ConnectionInfoRecorder {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub(crate) fn latest(&self, remote_id: &P2pId) -> Option<P2pConnectionInfo> {
        self.entries.lock().unwrap().get(remote_id).cloned()
    }

    pub(crate) async fn wait_for_direction(
        &self,
        remote_id: &P2pId,
        expected: ConnectDirection,
        deadline: AbsoluteDeadline,
    ) -> P2pResult<P2pConnectionInfo> {
        loop {
            // Register before inspecting state so an add between the inspection
            // and await cannot be lost.
            let changed = self.changed.notified();
            if let Some(info) = self.latest(remote_id) {
                if info.direct == expected {
                    return Ok(info);
                }
            }
            deadline
                .p2p("waiting for connection-info direction", async {
                    changed.await;
                    Ok(())
                })
                .await?;
        }
    }
}

#[async_trait::async_trait]
impl P2pConnectionInfoCache for ConnectionInfoRecorder {
    async fn get(&self, conn_id: &P2pId) -> Option<P2pConnectionInfo> {
        self.latest(conn_id)
    }

    async fn add(&self, conn_id: P2pId, info: P2pConnectionInfo) {
        self.entries.lock().unwrap().insert(conn_id, info);
        self.changed.notify_waiters();
    }
}

pub(crate) struct RealNode {
    pub(crate) stack: P2pStackRef,
    pub(crate) identity: P2pIdentityRef,
    pub(crate) id: P2pId,
    pub(crate) endpoint: Endpoint,
    pub(crate) connection_info: Arc<ConnectionInfoRecorder>,
}

pub(crate) struct RealSnFixture {
    pub(crate) server: SnServerRef,
    pub(crate) sn_identity: P2pIdentityRef,
    pub(crate) sn_id: P2pId,
    pub(crate) sn_endpoint: Endpoint,
    pub(crate) caller: RealNode,
    pub(crate) target: RealNode,
    pub(crate) identity_factory: Arc<X509IdentityFactory>,
    pub(crate) cert_factory: Arc<X509IdentityCertFactory>,
}

impl Drop for RealSnFixture {
    fn drop(&mut self) {
        self.caller.stack.sn_client().stop();
        self.target.stack.sn_client().stop();
        self.server.stop();
    }
}

pub(crate) struct RealStreamPair {
    pub(crate) initiator_read: StreamRead,
    pub(crate) initiator_write: StreamWrite,
    pub(crate) acceptor_read: StreamRead,
    pub(crate) acceptor_write: StreamWrite,
}

pub(crate) fn single_worker_runtime() -> ServerRuntime {
    ServerRuntime::start(ServerRuntimeConfig::new().with_workers(1))
        .expect("start fixture server runtime")
}

pub(crate) fn dynamic_loopback_endpoint(
    protocol: Protocol,
    area: EndpointArea,
) -> P2pResult<Endpoint> {
    let bind_addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0);
    let port = match protocol {
        Protocol::Tcp => TcpListener::bind(bind_addr)
            .and_then(|listener| listener.local_addr())
            .map_err(|error| io_error("reserve dynamic TCP port", error))?
            .port(),
        Protocol::Quic => UdpSocket::bind(bind_addr)
            .and_then(|socket| socket.local_addr())
            .map_err(|error| io_error("reserve dynamic UDP port", error))?
            .port(),
        Protocol::Ext(_) => {
            return Err(fixture_error(
                P2pErrorCode::NotSupport,
                format!("unsupported fixture protocol: {protocol:?}"),
            ));
        }
    };
    let mut endpoint = Endpoint::from((
        protocol,
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)),
    ));
    endpoint.set_area(area);
    Ok(endpoint)
}

pub(crate) fn x509_identity(
    name: impl Into<String>,
    endpoint: Endpoint,
) -> P2pResult<P2pIdentityRef> {
    let identity = generate_rsa_x509_identity(Some(name.into())).map_err(|error| {
        fixture_error(
            P2pErrorCode::Failed,
            format!("generate fixture X509 identity: {error}"),
        )
    })?;
    let identity: P2pIdentityRef = Arc::new(identity);
    Ok(identity.update_endpoints(vec![endpoint]))
}

pub(crate) fn sn_entry(identity: &P2pIdentityRef) -> P2pResult<P2pSn> {
    let cert = identity.get_identity_cert()?;
    Ok(P2pSn::new(cert.get_id(), cert.get_name(), cert.endpoints()))
}

pub(crate) async fn start_sn(
    identity: P2pIdentityRef,
    identity_factory: Arc<X509IdentityFactory>,
    cert_factory: Arc<X509IdentityCertFactory>,
) -> P2pResult<SnServerRef> {
    let server = create_sn_service(SnServiceConfig::new(
        identity,
        identity_factory,
        cert_factory,
        single_worker_runtime(),
    ))
    .await?;
    server.start().await?;
    Ok(server)
}

pub(crate) async fn start_node(
    identity: P2pIdentityRef,
    sn: P2pSn,
    identity_factory: Arc<X509IdentityFactory>,
    cert_factory: Arc<X509IdentityCertFactory>,
    connection_info: Arc<ConnectionInfoRecorder>,
) -> P2pResult<P2pStackRef> {
    let advertised = identity.endpoints()[0];
    let listen_addr = match advertised.protocol() {
        Protocol::Tcp => SocketAddr::V4(SocketAddrV4::new(
            Ipv4Addr::UNSPECIFIED,
            advertised.addr().port(),
        )),
        Protocol::Quic => *advertised.addr(),
        Protocol::Ext(_) => {
            return Err(fixture_error(
                P2pErrorCode::NotSupport,
                format!("unsupported fixture protocol: {:?}", advertised.protocol()),
            ));
        }
    };
    // Binding uses a neutral LAN endpoint while the identity retains the
    // requested advertised area (for example Wan in public-plan cases).
    let listen_endpoint = Endpoint::from((advertised.protocol(), listen_addr));
    let env = create_p2p_env(
        P2pConfig::new(
            identity_factory,
            cert_factory,
            vec![listen_endpoint],
            single_worker_runtime(),
        )
        .set_connection_info_cache(connection_info)
        .set_tcp_accept_timout(Duration::from_secs(3))
        .set_tcp_connect_timout(Duration::from_secs(3))
        .set_quic_connect_timeout(Duration::from_secs(3))
        .set_quic_idle_time(Duration::from_secs(10)),
    )
    .await?;

    create_p2p_stack(
        P2pStackConfig::new(env, identity)
            .add_sn_list(vec![sn])
            .set_conn_timeout(Duration::from_secs(3))
            .set_sn_ping_interval(Duration::from_millis(100))
            .set_sn_call_timeout(Duration::from_secs(3))
            .set_sn_query_interval(Duration::from_millis(200))
            .set_sn_tunnel_count(2),
    )
    .await
}

fn retryable_bind_error(error: &P2pError) -> bool {
    matches!(
        error.code(),
        P2pErrorCode::AddrInUse | P2pErrorCode::AddrNotAvailable | P2pErrorCode::AlreadyExists
    )
}

pub(crate) fn stop_partial(server: &SnServerRef, stacks: &[&P2pStackRef]) {
    for stack in stacks {
        stack.sn_client().stop();
    }
    server.stop();
}

pub(crate) async fn start_two_node_sn(
    protocol: Protocol,
    caller_area: EndpointArea,
    target_area: EndpointArea,
    deadline: AbsoluteDeadline,
) -> P2pResult<RealSnFixture> {
    for attempt in 0..SETUP_MAX_RETRIES {
        let identity_factory = Arc::new(X509IdentityFactory);
        let cert_factory = Arc::new(X509IdentityCertFactory);
        let suffix = UNIQUE_VALUE.fetch_add(1, Ordering::Relaxed);

        let sn_endpoint = dynamic_loopback_endpoint(protocol, EndpointArea::Lan)?;
        let sn_identity = x509_identity(format!("real-flow-sn-{suffix}-{attempt}"), sn_endpoint)?;
        let sn_id = sn_identity.get_id();
        let server = match deadline
            .p2p(
                "starting real SN server",
                start_sn(
                    sn_identity.clone(),
                    identity_factory.clone(),
                    cert_factory.clone(),
                ),
            )
            .await
        {
            Ok(server) => server,
            Err(error) if retryable_bind_error(&error) => continue,
            Err(error) => return Err(error),
        };
        let sn = match sn_entry(&sn_identity) {
            Ok(sn) => sn,
            Err(error) => {
                server.stop();
                return Err(error);
            }
        };

        let caller_endpoint = match dynamic_loopback_endpoint(protocol, caller_area) {
            Ok(endpoint) => endpoint,
            Err(error) => {
                server.stop();
                return Err(error);
            }
        };
        let caller_identity = match x509_identity(
            format!("real-flow-caller-{suffix}-{attempt}"),
            caller_endpoint,
        ) {
            Ok(identity) => identity,
            Err(error) => {
                server.stop();
                return Err(error);
            }
        };
        let caller_id = caller_identity.get_id();
        let caller_connection_info = ConnectionInfoRecorder::new();
        let caller_stack = match deadline
            .p2p(
                "starting real caller stack",
                start_node(
                    caller_identity.clone(),
                    sn.clone(),
                    identity_factory.clone(),
                    cert_factory.clone(),
                    caller_connection_info.clone(),
                ),
            )
            .await
        {
            Ok(stack) => stack,
            Err(error) if retryable_bind_error(&error) => {
                stop_partial(&server, &[]);
                continue;
            }
            Err(error) => {
                stop_partial(&server, &[]);
                return Err(error);
            }
        };

        let target_endpoint = match dynamic_loopback_endpoint(protocol, target_area) {
            Ok(endpoint) => endpoint,
            Err(error) => {
                stop_partial(&server, &[&caller_stack]);
                return Err(error);
            }
        };
        let target_identity = match x509_identity(
            format!("real-flow-target-{suffix}-{attempt}"),
            target_endpoint,
        ) {
            Ok(identity) => identity,
            Err(error) => {
                stop_partial(&server, &[&caller_stack]);
                return Err(error);
            }
        };
        let target_id = target_identity.get_id();
        let target_connection_info = ConnectionInfoRecorder::new();
        let target_stack = match deadline
            .p2p(
                "starting real target stack",
                start_node(
                    target_identity.clone(),
                    sn,
                    identity_factory.clone(),
                    cert_factory.clone(),
                    target_connection_info.clone(),
                ),
            )
            .await
        {
            Ok(stack) => stack,
            Err(error) if retryable_bind_error(&error) => {
                stop_partial(&server, &[&caller_stack]);
                continue;
            }
            Err(error) => {
                stop_partial(&server, &[&caller_stack]);
                return Err(error);
            }
        };

        let caller_readiness_budget = match deadline.remaining("waiting for caller SN readiness") {
            Ok(remaining) => remaining,
            Err(error) => {
                stop_partial(&server, &[&caller_stack, &target_stack]);
                return Err(error);
            }
        };
        if let Err(error) = caller_stack
            .wait_online(Some(caller_readiness_budget))
            .await
        {
            stop_partial(&server, &[&caller_stack, &target_stack]);
            return Err(error);
        }
        let target_readiness_budget = match deadline.remaining("waiting for target SN readiness") {
            Ok(remaining) => remaining,
            Err(error) => {
                stop_partial(&server, &[&caller_stack, &target_stack]);
                return Err(error);
            }
        };
        if let Err(error) = target_stack
            .wait_online(Some(target_readiness_budget))
            .await
        {
            stop_partial(&server, &[&caller_stack, &target_stack]);
            return Err(error);
        }

        return Ok(RealSnFixture {
            server,
            sn_identity,
            sn_id,
            sn_endpoint,
            caller: RealNode {
                stack: caller_stack,
                identity: caller_identity,
                id: caller_id,
                endpoint: caller_endpoint,
                connection_info: caller_connection_info,
            },
            target: RealNode {
                stack: target_stack,
                identity: target_identity,
                id: target_id,
                endpoint: target_endpoint,
                connection_info: target_connection_info,
            },
            identity_factory,
            cert_factory,
        });
    }

    Err(fixture_error(
        P2pErrorCode::AddrInUse,
        format!("real socket topology exhausted {SETUP_MAX_RETRIES} bind retries"),
    ))
}

pub(crate) fn unique_purpose(label: &str) -> TunnelPurpose {
    let value = UNIQUE_VALUE.fetch_add(1, Ordering::Relaxed);
    TunnelPurpose::from_bytes(format!("real-p2p-flow/{label}/{value}").into_bytes())
}

pub(crate) async fn connect_stream_pair_from_id(
    initiator: &RealNode,
    acceptor: &RealNode,
    purpose: TunnelPurpose,
    deadline: AbsoluteDeadline,
) -> P2pResult<RealStreamPair> {
    let mut listener = acceptor
        .stack
        .stream_manager()
        .listen(purpose.clone())
        .await?;
    let initiator_stack = initiator.stack.clone();
    let acceptor_id = acceptor.id.clone();
    let ((initiator_read, initiator_write), (acceptor_read, acceptor_write)) = deadline
        .p2p("connecting and accepting a real stream", async move {
            tokio::try_join!(
                initiator_stack
                    .stream_manager()
                    .connect_from_id(&acceptor_id, purpose),
                listener.accept(),
            )
        })
        .await?;

    if initiator_read.local_id() != initiator.id
        || initiator_read.remote_id() != acceptor.id
        || acceptor_read.local_id() != acceptor.id
        || acceptor_read.remote_id() != initiator.id
    {
        return Err(fixture_error(
            P2pErrorCode::Unmatch,
            "real stream identity mismatch",
        ));
    }

    Ok(RealStreamPair {
        initiator_read,
        initiator_write,
        acceptor_read,
        acceptor_write,
    })
}

pub(crate) async fn assert_bidirectional_unique_payload(
    streams: &mut RealStreamPair,
    label: &str,
    deadline: AbsoluteDeadline,
) -> P2pResult<()> {
    let value = UNIQUE_VALUE.fetch_add(1, Ordering::Relaxed);
    let outbound = format!("real-p2p-flow/{label}/{value}/initiator-to-acceptor").into_bytes();
    let inbound = format!("real-p2p-flow/{label}/{value}/acceptor-to-initiator").into_bytes();
    debug_assert_ne!(outbound, inbound);

    deadline
        .io(
            "writing initiator-to-acceptor payload",
            streams.initiator_write.write_all(&outbound),
        )
        .await?;
    deadline
        .io(
            "flushing initiator-to-acceptor payload",
            streams.initiator_write.flush(),
        )
        .await?;
    let mut received_outbound = vec![0; outbound.len()];
    deadline
        .io(
            "reading initiator-to-acceptor payload",
            streams.acceptor_read.read_exact(&mut received_outbound),
        )
        .await?;
    if received_outbound != outbound {
        return Err(fixture_error(
            P2pErrorCode::Unmatch,
            "initiator-to-acceptor payload mismatch",
        ));
    }

    deadline
        .io(
            "writing acceptor-to-initiator payload",
            streams.acceptor_write.write_all(&inbound),
        )
        .await?;
    deadline
        .io(
            "flushing acceptor-to-initiator payload",
            streams.acceptor_write.flush(),
        )
        .await?;
    let mut received_inbound = vec![0; inbound.len()];
    deadline
        .io(
            "reading acceptor-to-initiator payload",
            streams.initiator_read.read_exact(&mut received_inbound),
        )
        .await?;
    if received_inbound != inbound {
        return Err(fixture_error(
            P2pErrorCode::Unmatch,
            "acceptor-to-initiator payload mismatch",
        ));
    }

    Ok(())
}

pub(crate) async fn connect_and_exchange_from_id(
    initiator: &RealNode,
    acceptor: &RealNode,
    label: &str,
    deadline: AbsoluteDeadline,
) -> P2pResult<RealStreamPair> {
    let mut streams =
        connect_stream_pair_from_id(initiator, acceptor, unique_purpose(label), deadline).await?;
    assert_bidirectional_unique_payload(&mut streams, label, deadline).await?;
    Ok(streams)
}
