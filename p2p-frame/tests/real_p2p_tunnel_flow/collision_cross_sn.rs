use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bucky_raw_codec::{RawConvertTo, RawFrom};
use bucky_time::bucky_time_now;
use p2p_frame::endpoint::{Endpoint, EndpointArea, Protocol};
use p2p_frame::error::{P2pError, P2pErrorCode, P2pResult};
use p2p_frame::networks::QuicCongestionAlgorithm;
use p2p_frame::p2p_identity::{P2pId, P2pIdentityCertFactory, P2pIdentityRef, P2pSn};
use p2p_frame::sn::directory::{
    OwnerDirectoryListenConfig, OwnerDirectoryServer, OwnerDirectoryServerRef, OwnerMember,
    OwnerMembership, OwnerResolver, ServingLease,
};
use p2p_frame::sn::client::SnTunnelRendezvousActionAck;
use p2p_frame::sn::protocol::{
    NAT_PROBE_CONTROL_VERSION, PackageCmdCode, ReportSn, ReportSnResp, SN_PROTOCOL_VERSION,
    SnTunnelRendezvousOperation,
};
use p2p_frame::sn::service::{SnServerRef, SnServiceConfig, create_sn_service};
use p2p_frame::stack::{P2pConfig, P2pStackConfig, P2pStackRef, create_p2p_env, create_p2p_stack};
use p2p_frame::types::TunnelId;
use p2p_frame::x509::{X509IdentityCertFactory, X509IdentityFactory};
use sfo_cmd_server::client::CmdClient;

use super::fixture::{
    AbsoluteDeadline, ConnectionInfoRecorder, DEFAULT_FLOW_TIMEOUT, DEFAULT_SETUP_TIMEOUT,
    RealNode, RealStreamPair, assert_bidirectional_unique_payload, connect_and_exchange_from_id,
    dynamic_loopback_endpoint, single_worker_runtime, start_two_node_sn, unique_purpose,
    x509_identity,
};

const CROSS_SN_SETUP_TIMEOUT: Duration = Duration::from_secs(90);
const ROUTE_TTL: Duration = Duration::from_secs(60);

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn simultaneous_real_socket_connects_remain_payload_stable() {
    let topology = start_two_node_sn(
        Protocol::Tcp,
        EndpointArea::Lan,
        EndpointArea::Lan,
        AbsoluteDeadline::after(DEFAULT_SETUP_TIMEOUT),
    )
    .await
    .expect("start two real P2pStack nodes and their real SN");

    let caller_to_target_purpose = unique_purpose("simultaneous-caller-to-target");
    let target_to_caller_purpose = unique_purpose("simultaneous-target-to-caller");
    let mut target_listener = topology
        .target
        .stack
        .stream_manager()
        .listen(caller_to_target_purpose.clone())
        .await
        .expect("target listens before simultaneous connects");
    let mut caller_listener = topology
        .caller
        .stack
        .stream_manager()
        .listen(target_to_caller_purpose.clone())
        .await
        .expect("caller listens before simultaneous connects");

    let caller_stack = topology.caller.stack.clone();
    let target_stack = topology.target.stack.clone();
    let caller_id = topology.caller.id.clone();
    let target_id = topology.target.id.clone();
    let simultaneous_deadline = AbsoluteDeadline::after(DEFAULT_FLOW_TIMEOUT);
    let (caller_outbound, target_accept, target_outbound, caller_accept) = simultaneous_deadline
        .p2p(
            "running simultaneous real-socket connects and accepts",
            async move {
                tokio::try_join!(
                    caller_stack
                        .stream_manager()
                        .connect_from_id(&target_id, caller_to_target_purpose),
                    target_listener.accept(),
                    target_stack
                        .stream_manager()
                        .connect_from_id(&caller_id, target_to_caller_purpose),
                    caller_listener.accept(),
                )
            },
        )
        .await
        .expect("both connect directions finish before the absolute deadline");

    let (caller_outbound_read, caller_outbound_write) = caller_outbound;
    let (target_accept_read, target_accept_write) = target_accept;
    let (target_outbound_read, target_outbound_write) = target_outbound;
    let (caller_accept_read, caller_accept_write) = caller_accept;

    assert_eq!(caller_outbound_read.local_id(), topology.caller.id);
    assert_eq!(caller_outbound_read.remote_id(), topology.target.id);
    assert_eq!(target_accept_read.local_id(), topology.target.id);
    assert_eq!(target_accept_read.remote_id(), topology.caller.id);
    assert_eq!(target_outbound_read.local_id(), topology.target.id);
    assert_eq!(target_outbound_read.remote_id(), topology.caller.id);
    assert_eq!(caller_accept_read.local_id(), topology.caller.id);
    assert_eq!(caller_accept_read.remote_id(), topology.target.id);

    let mut caller_to_target = RealStreamPair {
        initiator_read: caller_outbound_read,
        initiator_write: caller_outbound_write,
        acceptor_read: target_accept_read,
        acceptor_write: target_accept_write,
    };
    let mut target_to_caller = RealStreamPair {
        initiator_read: target_outbound_read,
        initiator_write: target_outbound_write,
        acceptor_read: caller_accept_read,
        acceptor_write: caller_accept_write,
    };

    let payload_deadline = AbsoluteDeadline::after(DEFAULT_FLOW_TIMEOUT);
    assert_bidirectional_unique_payload(
        &mut caller_to_target,
        "simultaneous-caller-to-target",
        payload_deadline,
    )
    .await
    .expect("caller-originated stream remains bidirectionally usable");
    assert_bidirectional_unique_payload(
        &mut target_to_caller,
        "simultaneous-target-to-caller",
        payload_deadline,
    )
    .await
    .expect("target-originated stream remains bidirectionally usable");

    drop(caller_to_target);
    drop(target_to_caller);

    let retry_deadline = AbsoluteDeadline::after(DEFAULT_FLOW_TIMEOUT);
    let mut retry = connect_and_exchange_from_id(
        &topology.caller,
        &topology.target,
        "simultaneous-post-drop-retry",
        retry_deadline,
    )
    .await
    .expect("a fresh stream remains available after both racing streams are dropped");
    assert_bidirectional_unique_payload(
        &mut retry,
        "simultaneous-post-drop-retry-repeat",
        retry_deadline,
    )
    .await
    .expect("the post-race stream repeatedly carries unique payloads");

    eprintln!(
        "case=simultaneous-real-socket connected=true payload-complete=true cleanup=caller-visible-bounded owner-token-branch=not-claimed"
    );
}

fn cross_sn_error(code: P2pErrorCode, message: impl Into<String>) -> P2pError {
    P2pError::new(code, message.into())
}

fn trace_cross_sn_setup(stage: &str, started: Instant, deadline: AbsoluteDeadline) {
    let remaining_ms = deadline
        .remaining(stage)
        .map(|remaining| remaining.as_millis())
        .unwrap_or(0);
    eprintln!(
        "case=production-ttp-cross-sn setup-stage={stage} elapsed-ms={} remaining-ms={remaining_ms}",
        started.elapsed().as_millis()
    );
}

struct CrossSnTopology {
    owner: OwnerDirectoryServerRef,
    sn_a: SnServerRef,
    sn_b: SnServerRef,
    sn_a_id: P2pId,
    sn_b_id: P2pId,
    source: RealNode,
    target: RealNode,
    cert_factory: Arc<X509IdentityCertFactory>,
}

impl Drop for CrossSnTopology {
    fn drop(&mut self) {
        self.source.stack.sn_client().stop();
        self.target.stack.sn_client().stop();
        self.sn_a.stop();
        self.sn_b.stop();
        self.owner.stop_owner_control_loop();
    }
}

async fn start_membership_sn(
    identity: P2pIdentityRef,
    identity_factory: Arc<X509IdentityFactory>,
    cert_factory: Arc<X509IdentityCertFactory>,
    membership: OwnerMembership,
) -> P2pResult<SnServerRef> {
    let server = create_sn_service(
        SnServiceConfig::new(
            identity,
            identity_factory,
            cert_factory,
            single_worker_runtime(),
        )
        .set_owner_client_membership(membership),
    )
    .await?;
    server.start().await?;
    Ok(server)
}

async fn start_cross_sn_node(
    identity: P2pIdentityRef,
    sn: P2pSn,
    identity_factory: Arc<X509IdentityFactory>,
    cert_factory: Arc<X509IdentityCertFactory>,
    connection_info: Arc<ConnectionInfoRecorder>,
) -> P2pResult<P2pStackRef> {
    let advertised = identity.endpoints()[0];
    let listen_endpoint = Endpoint::from((
        advertised.protocol(),
        SocketAddr::V4(SocketAddrV4::new(
            Ipv4Addr::LOCALHOST,
            advertised.addr().port(),
        )),
    ));
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

struct SingleMembershipTopology {
    owner: OwnerDirectoryServerRef,
    sn: SnServerRef,
    stack: P2pStackRef,
    sn_id: P2pId,
    sn_command_endpoint: Endpoint,
}

impl Drop for SingleMembershipTopology {
    fn drop(&mut self) {
        self.stack.sn_client().stop();
        self.sn.stop();
        self.owner.stop_owner_control_loop();
    }
}

async fn start_single_membership_topology(
    deadline: AbsoluteDeadline,
) -> P2pResult<SingleMembershipTopology> {
    let identity_factory = Arc::new(X509IdentityFactory);
    let cert_factory = Arc::new(X509IdentityCertFactory);
    let owner_peer_endpoint = dynamic_loopback_endpoint(Protocol::Quic, EndpointArea::Lan)?;
    let owner_serving_endpoint = dynamic_loopback_endpoint(Protocol::Quic, EndpointArea::Lan)?;
    let owner_identity = x509_identity("real-flow-single-owner", owner_serving_endpoint)?;
    let owner_id = owner_identity.get_id();
    let owner = OwnerDirectoryServer::new(
        OwnerDirectoryListenConfig {
            local_identity: owner_identity,
            identity_factory: identity_factory.clone(),
            cert_factory: cert_factory.clone(),
            owner_peer_endpoints: vec![owner_peer_endpoint],
            serving_endpoints: vec![owner_serving_endpoint],
            congestion_algorithm: QuicCongestionAlgorithm::Bbr,
            reuse_address: false,
            server_runtime: single_worker_runtime(),
        },
        OwnerMembership::with_members(
            vec![OwnerMember::with_endpoint(
                owner_id.clone(),
                owner_peer_endpoint,
            )],
            1,
            ROUTE_TTL,
        )?,
        None,
    )?;
    deadline
        .p2p("starting single-SN owner directory", owner.start())
        .await?;

    let sn_command_endpoint = dynamic_loopback_endpoint(Protocol::Tcp, EndpointArea::Lan)?;
    let sn_inter_endpoint = dynamic_loopback_endpoint(Protocol::Quic, EndpointArea::Lan)?;
    let sn_identity = x509_identity("real-flow-single-membership-sn", sn_command_endpoint)?
        .update_endpoints(vec![sn_command_endpoint, sn_inter_endpoint]);
    let sn_id = sn_identity.get_id();
    let membership = OwnerMembership::with_members(
        vec![
            OwnerMember::with_endpoint(owner_id, owner_serving_endpoint),
            OwnerMember::with_endpoint(sn_id.clone(), sn_inter_endpoint),
        ],
        1,
        ROUTE_TTL,
    )?;
    let sn = match deadline
        .p2p(
            "starting membership-enabled serving SN",
            start_membership_sn(
                sn_identity.clone(),
                identity_factory.clone(),
                cert_factory.clone(),
                membership,
            ),
        )
        .await
    {
        Ok(sn) => sn,
        Err(error) => {
            owner.stop_owner_control_loop();
            return Err(error);
        }
    };

    let node_endpoint = dynamic_loopback_endpoint(Protocol::Tcp, EndpointArea::Lan)?;
    let node_identity = x509_identity("real-flow-single-membership-node", node_endpoint)?;
    let stack = match deadline
        .p2p(
            "starting single membership SN client",
            start_cross_sn_node(
                node_identity,
                P2pSn::new(
                    sn_id.clone(),
                    sn_identity.get_name(),
                    vec![sn_command_endpoint],
                ),
                identity_factory,
                cert_factory,
                ConnectionInfoRecorder::new(),
            ),
        )
        .await
    {
        Ok(stack) => stack,
        Err(error) => {
            sn.stop();
            owner.stop_owner_control_loop();
            return Err(error);
        }
    };
    let topology = SingleMembershipTopology {
        owner,
        sn,
        stack,
        sn_id,
        sn_command_endpoint,
    };
    topology
        .stack
        .wait_online(Some(
            deadline.remaining("waiting for single membership SN Report")?,
        ))
        .await?;
    Ok(topology)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn membership_enabled_single_sn_report_reaches_online_on_shared_runtime() {
    let topology =
        start_single_membership_topology(AbsoluteDeadline::after(CROSS_SN_SETUP_TIMEOUT))
            .await
            .expect("membership-enabled SN and real client complete Report/wait_online");
    let active = topology.stack.sn_client().get_active_sn_list();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].sn_peer_id, topology.sn_id);
    assert_eq!(active[0].protocol, Protocol::Tcp);
    assert_eq!(topology.sn_command_endpoint.protocol(), Protocol::Tcp);
    assert!(topology.sn_command_endpoint.addr().ip().is_loopback());
    assert_ne!(topology.sn_command_endpoint.addr().port(), 0);

    eprintln!(
        "case=membership-single-sn real-socket=true report-complete=true online=true shared-runtime=true"
    );
}

fn serving_membership(
    owner_id: P2pId,
    owner_endpoint: Endpoint,
    sn_a_id: P2pId,
    sn_a_endpoint: Endpoint,
    sn_b_id: P2pId,
    sn_b_endpoint: Endpoint,
) -> P2pResult<OwnerMembership> {
    OwnerMembership::with_members(
        vec![
            OwnerMember::with_endpoint(owner_id, owner_endpoint),
            OwnerMember::with_endpoint(sn_a_id, sn_a_endpoint),
            OwnerMember::with_endpoint(sn_b_id, sn_b_endpoint),
        ],
        1,
        ROUTE_TTL,
    )
}

async fn start_cross_sn_topology(deadline: AbsoluteDeadline) -> P2pResult<CrossSnTopology> {
    let started = Instant::now();
    let identity_factory = Arc::new(X509IdentityFactory);
    let cert_factory = Arc::new(X509IdentityCertFactory);
    let owner_peer_endpoint = dynamic_loopback_endpoint(Protocol::Quic, EndpointArea::Lan)?;
    let owner_serving_endpoint = dynamic_loopback_endpoint(Protocol::Quic, EndpointArea::Lan)?;
    let owner_identity = x509_identity("real-flow-owner", owner_serving_endpoint)?;
    let owner_id = owner_identity.get_id();
    let owner_membership = OwnerMembership::with_members(
        vec![OwnerMember::with_endpoint(
            owner_id.clone(),
            owner_peer_endpoint,
        )],
        1,
        ROUTE_TTL,
    )?;
    let owner = OwnerDirectoryServer::new(
        OwnerDirectoryListenConfig {
            local_identity: owner_identity,
            identity_factory: identity_factory.clone(),
            cert_factory: cert_factory.clone(),
            owner_peer_endpoints: vec![owner_peer_endpoint],
            serving_endpoints: vec![owner_serving_endpoint],
            congestion_algorithm: QuicCongestionAlgorithm::Bbr,
            reuse_address: false,
            server_runtime: single_worker_runtime(),
        },
        owner_membership,
        None,
    )?;
    deadline
        .p2p("starting owner-directory real sockets", owner.start())
        .await?;
    trace_cross_sn_setup("owner-directory-ready", started, deadline);

    let sn_a_command_endpoint = dynamic_loopback_endpoint(Protocol::Tcp, EndpointArea::Lan)?;
    let sn_a_inter_sn_endpoint = dynamic_loopback_endpoint(Protocol::Quic, EndpointArea::Lan)?;
    let sn_b_command_endpoint = dynamic_loopback_endpoint(Protocol::Tcp, EndpointArea::Lan)?;
    let sn_b_inter_sn_endpoint = dynamic_loopback_endpoint(Protocol::Quic, EndpointArea::Lan)?;
    let sn_a_identity = x509_identity("real-flow-sn-a", sn_a_command_endpoint)?
        .update_endpoints(vec![sn_a_command_endpoint, sn_a_inter_sn_endpoint]);
    let sn_b_identity = x509_identity("real-flow-sn-b", sn_b_command_endpoint)?
        .update_endpoints(vec![sn_b_command_endpoint, sn_b_inter_sn_endpoint]);
    let sn_a_id = sn_a_identity.get_id();
    let sn_b_id = sn_b_identity.get_id();
    let membership = serving_membership(
        owner_id.clone(),
        owner_serving_endpoint,
        sn_a_id.clone(),
        sn_a_inter_sn_endpoint,
        sn_b_id.clone(),
        sn_b_inter_sn_endpoint,
    )?;
    let sn_a = deadline
        .p2p(
            "starting serving SN A",
            start_membership_sn(
                sn_a_identity.clone(),
                identity_factory.clone(),
                cert_factory.clone(),
                membership.clone(),
            ),
        )
        .await?;
    trace_cross_sn_setup("serving-sn-a-ready", started, deadline);
    let sn_b = deadline
        .p2p(
            "starting serving SN B",
            start_membership_sn(
                sn_b_identity.clone(),
                identity_factory.clone(),
                cert_factory.clone(),
                membership.clone(),
            ),
        )
        .await?;
    trace_cross_sn_setup("serving-sn-b-ready", started, deadline);

    let source_endpoint = dynamic_loopback_endpoint(Protocol::Tcp, EndpointArea::Lan)?;
    let source_identity = x509_identity("real-flow-cross-sn-source", source_endpoint)?;
    let source_id = source_identity.get_id();
    let source_info = ConnectionInfoRecorder::new();
    let source_stack = deadline
        .p2p(
            "starting source on SN A",
            start_cross_sn_node(
                source_identity.clone(),
                P2pSn::new(
                    sn_a_id.clone(),
                    sn_a_identity.get_name(),
                    vec![sn_a_command_endpoint],
                ),
                identity_factory.clone(),
                cert_factory.clone(),
                source_info.clone(),
            ),
        )
        .await?;
    trace_cross_sn_setup("source-stack-created", started, deadline);

    let (target_identity, target_endpoint) = (0..64)
        .find_map(|attempt| {
            let endpoint = dynamic_loopback_endpoint(Protocol::Tcp, EndpointArea::Lan).ok()?;
            let identity =
                x509_identity(format!("real-flow-cross-sn-target-{attempt}"), endpoint).ok()?;
            let owners = OwnerResolver::new(Some(membership.clone()))
                .owner_set(&identity.get_id(), &sn_a_id);
            (owners == vec![owner_id.clone()]).then_some((identity, endpoint))
        })
        .ok_or_else(|| {
            cross_sn_error(
                P2pErrorCode::NotFound,
                "could not allocate a target mapped to the real owner-directory",
            )
        })?;
    let target_id = target_identity.get_id();
    let target_info = ConnectionInfoRecorder::new();
    let target_stack = deadline
        .p2p(
            "starting target on SN B",
            start_cross_sn_node(
                target_identity.clone(),
                P2pSn::new(
                    sn_b_id.clone(),
                    sn_b_identity.get_name(),
                    vec![sn_b_command_endpoint],
                ),
                identity_factory,
                cert_factory.clone(),
                target_info.clone(),
            ),
        )
        .await?;
    trace_cross_sn_setup("target-stack-created", started, deadline);
    source_stack
        .wait_online(Some(deadline.remaining("waiting for source online")?))
        .await?;
    trace_cross_sn_setup("source-online", started, deadline);
    target_stack
        .wait_online(Some(deadline.remaining("waiting for target online")?))
        .await?;
    trace_cross_sn_setup("target-online", started, deadline);

    let route_now = bucky_time_now();
    deadline
        .p2p(
            "renewing SN B serving session before fixture route seeding",
            owner.service().election_node().renew_serving_session(
                sn_b_id.clone(),
                0,
                ROUTE_TTL,
                route_now,
            ),
        )
        .await?;
    trace_cross_sn_setup("sn-b-serving-session-online", started, deadline);

    let route = ServingLease::new(target_id.clone(), sn_b_id.clone(), 1, ROUTE_TTL, route_now);
    if !owner
        .service()
        .publish_lease_from_serving_sn(sn_b_id.clone(), route)
        .await?
    {
        return Err(cross_sn_error(
            P2pErrorCode::Failed,
            "owner-directory rejected fixture serving lease",
        ));
    }
    trace_cross_sn_setup("target-route-seeded", started, deadline);

    Ok(CrossSnTopology {
        owner,
        sn_a,
        sn_b,
        sn_a_id,
        sn_b_id,
        source: RealNode {
            stack: source_stack,
            identity: source_identity,
            id: source_id,
            endpoint: source_endpoint,
            connection_info: source_info,
        },
        target: RealNode {
            stack: target_stack,
            identity: target_identity,
            id: target_id,
            endpoint: target_endpoint,
            connection_info: target_info,
        },
        cert_factory,
    })
}

async fn send_cross_sn_report_with_local_endpoints(
    stack: &P2pStackRef,
    sn_id: &P2pId,
    local_eps: Vec<Endpoint>,
    seq: u32,
) -> P2pResult<ReportSnResp> {
    let active = stack
        .sn_client()
        .get_active_sn_list()
        .into_iter()
        .find(|active| &active.sn_peer_id == sn_id)
        .ok_or_else(|| cross_sn_error(P2pErrorCode::NotFound, "source SN is not active"))?;
    let report = ReportSn {
        protocol_version: SN_PROTOCOL_VERSION,
        stack_version: 0,
        seq: seq.into(),
        sn_peer_id: sn_id.clone(),
        from_peer_id: Some(stack.local_identity().get_id()),
        peer_info: Some(
            stack
                .local_identity()
                .get_identity_cert()?
                .get_encoded_cert()?,
        ),
        send_time: bucky_time_now(),
        contract_id: None,
        receipt: None,
        map_ports: Vec::new(),
        local_eps,
        net_profile: None,
        nat_probe_control_version: Some(NAT_PROBE_CONTROL_VERSION),
        nat_probe_result: None,
    };
    let body = report
        .to_vec()
        .map_err(|error| cross_sn_error(P2pErrorCode::RawCodecError, error.to_string()))?;
    let mut response = stack
        .sn_client()
        .get_cmd_client()
        .send_by_specify_tunnel_with_resp(
            active.conn_id,
            PackageCmdCode::ReportSn as u8,
            0,
            body.as_slice(),
            DEFAULT_FLOW_TIMEOUT,
        )
        .await
        .map_err(|error| cross_sn_error(P2pErrorCode::Failed, error.to_string()))?;
    ReportSnResp::clone_from_slice(
        response
            .read_all()
            .await
            .map_err(|error| cross_sn_error(P2pErrorCode::IoError, error.to_string()))?
            .as_slice(),
    )
    .map_err(|error| cross_sn_error(P2pErrorCode::RawCodecError, error.to_string()))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn production_ttp_cross_sn_query_and_rendezvous_arm_target_action() {
    let topology = start_cross_sn_topology(AbsoluteDeadline::after(CROSS_SN_SETUP_TIMEOUT))
        .await
        .expect("start real owner-directory, serving SN A/B, source, and target sockets");
    let source_active = topology.source.stack.sn_client().get_active_sn_list();
    let target_active = topology.target.stack.sn_client().get_active_sn_list();
    assert_eq!(source_active.len(), 1);
    assert_eq!(target_active.len(), 1);
    assert_eq!(source_active[0].sn_peer_id, topology.sn_a_id);
    assert_eq!(target_active[0].sn_peer_id, topology.sn_b_id);

    let deadline = AbsoluteDeadline::after(DEFAULT_FLOW_TIMEOUT);
    let query = deadline
        .p2p(
            "querying target through production owner and TTP control streams",
            topology.source.stack.sn_client().query(&topology.target.id),
        )
        .await
        .expect("cross-SN Query reaches serving SN B");
    let cert = topology
        .cert_factory
        .create(&query.peer_info.expect("cross-SN Query returns target cert"))
        .expect("decode target cert returned across TTP");
    assert_eq!(cert.get_id(), topology.target.id);

    let request = topology
        .source
        .stack
        .sn_client()
        .new_rendezvous_request(
            TunnelId::from(0x0340_0001),
            &topology.target.id,
            SnTunnelRendezvousOperation::WaitIncoming,
            Vec::new(),
            false,
        )
        .expect("build valid WaitIncoming request");
    let response = deadline
        .p2p(
            "relaying rendezvous through production TTP inter-SN control stream",
            topology
                .source
                .stack
                .sn_client()
                .rendezvous_via_sn(&topology.sn_a_id, &request),
        )
        .await
        .expect("cross-SN rendezvous reaches target production listener");
    assert!(response.is_success());
    assert!(response.predicted_endpoint_array.is_empty());

    eprintln!(
        "case=production-ttp-cross-sn selected=WaitIncoming request-sent=true action-armed=true connected=not-claimed payload-complete=not-required route-seed=fixture-only"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn production_cross_sn_rejects_forged_report_before_target_action() {
    let topology = start_cross_sn_topology(AbsoluteDeadline::after(CROSS_SN_SETUP_TIMEOUT))
        .await
        .expect("start real owner-directory, serving SN A/B, source, and target sockets");
    let callback_count = Arc::new(AtomicUsize::new(0));
    let observed_count = callback_count.clone();
    topology.target.stack.sn_client().set_rendezvous_listener(
        move |_notify, _serving_sn_id| {
            let observed_count = observed_count.clone();
            async move {
                observed_count.fetch_add(1, Ordering::SeqCst);
                Ok(SnTunnelRendezvousActionAck::without_prediction())
            }
        },
    );
    let mut third_party = Endpoint::from((
        Protocol::Quic,
        "192.0.2.153:45153".parse::<SocketAddr>().unwrap(),
    ));
    third_party.set_area(EndpointArea::ServerReflexive);
    let report = send_cross_sn_report_with_local_endpoints(
        &topology.source.stack,
        &topology.sn_a_id,
        vec![third_party],
        0x5306,
    )
    .await
    .expect("authenticated source sends a forged ReportSn over its real SN A control tunnel");
    assert_eq!(report.result, P2pErrorCode::Ok.as_u8());

    let request = topology
        .source
        .stack
        .sn_client()
        .new_rendezvous_request(
            TunnelId::from(0x5307),
            &topology.target.id,
            SnTunnelRendezvousOperation::PunchOnly,
            vec![third_party],
            false,
        )
        .expect("build syntactically valid third-party rendezvous request");
    let error = topology
        .source
        .stack
        .sn_client()
        .rendezvous_via_sn(&topology.sn_a_id, &request)
        .await
        .expect_err("source SN must reject before relaying the request to target SN");

    assert_eq!(error.code(), P2pErrorCode::Failed);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(callback_count.load(Ordering::SeqCst), 0);
}
