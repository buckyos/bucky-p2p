use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bucky_time::bucky_time_now;

use crate::endpoint::{Endpoint, Protocol};
use crate::error::{P2pErrorCode, P2pResult, p2p_err};
use crate::p2p_identity::{P2pId, P2pIdentityCertFactory, P2pIdentityRef, P2pSn};
use crate::sn::directory::{
    OwnerDirectoryServer, OwnerDirectoryServerRef, OwnerElectionNodeRef, OwnerElectionRole,
    OwnerHeartbeatRequest, OwnerHeartbeatResponse, OwnerMembership, OwnerPeerControlClient,
    OwnerResolver, OwnerSessionForward, OwnerSessionForwardResponse, OwnerSessionReplication,
    OwnerSessionReplicationResponse, OwnerVoteRequest, OwnerVoteResponse, ServingLease,
    StaticOwnerDirectoryClient,
};
use crate::sn::protocol::v0::TunnelType;
use crate::sn::protocol::{InterSnCommandCode, PackageCmdCode};
use crate::sn::service::{SnServerRef, SnServiceConfig, create_sn_service};
use crate::stack::{P2pConfig, P2pStackConfig, P2pStackRef, create_p2p_env, create_p2p_stack};
use crate::types::TunnelId;
use crate::x509::{X509IdentityCertFactory, X509IdentityFactory, generate_rsa_x509_identity};
use sfo_reuseport::{ServerRuntime, ServerRuntimeConfig};

const MATRIX_ONLINE_TIMEOUT: Duration = Duration::from_secs(90);
const MATRIX_ROUNDS: usize = 3;
const MATRIX_ROUND_DELAY: Duration = Duration::from_secs(2);
const SETUP_MAX_RETRY: usize = 20;
const OWNER_REPLICA_COUNT: usize = 3;

fn next_port() -> u16 {
    UdpSocket::bind(SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn localhost_quic_endpoint(port: u16) -> Endpoint {
    Endpoint::from((
        Protocol::Quic,
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)),
    ))
}

fn build_identity(name: &str, endpoint: Endpoint) -> P2pIdentityRef {
    let identity = generate_rsa_x509_identity(Some(name.to_owned())).unwrap();
    let identity: P2pIdentityRef = Arc::new(identity);
    identity.update_endpoints(vec![endpoint])
}

fn build_sn_entry(sn_identity: &P2pIdentityRef) -> P2pSn {
    let sn_cert = sn_identity.get_identity_cert().unwrap();
    P2pSn::new(sn_cert.get_id(), sn_cert.get_name(), sn_cert.endpoints())
}

fn test_server_runtime() -> ServerRuntime {
    ServerRuntime::start(ServerRuntimeConfig::new().with_workers(1)).unwrap()
}

fn is_addr_bind_conflict(code: P2pErrorCode) -> bool {
    code == P2pErrorCode::AddrInUse
        || code == P2pErrorCode::AddrNotAvailable
        || code == P2pErrorCode::AlreadyExists
}

async fn start_sn_service_with_owner_client(
    sn_identity: P2pIdentityRef,
    identity_factory: Arc<X509IdentityFactory>,
    cert_factory: Arc<X509IdentityCertFactory>,
    owner_membership: OwnerMembership,
) -> P2pResult<SnServerRef> {
    let owner_client = StaticOwnerDirectoryClient::new(owner_membership, None);
    let service = create_sn_service(
        SnServiceConfig::new(
            sn_identity,
            identity_factory,
            cert_factory,
            test_server_runtime(),
        )
        .set_owner_client_for_tests(owner_client),
    )
    .await?;
    service.start().await?;
    Ok(service)
}

async fn start_client_stack(
    client_identity: P2pIdentityRef,
    sn_list: Vec<P2pSn>,
    identity_factory: Arc<X509IdentityFactory>,
    cert_factory: Arc<X509IdentityCertFactory>,
) -> P2pResult<P2pStackRef> {
    let endpoint = *client_identity.endpoints().first().unwrap();
    let env = create_p2p_env(
        P2pConfig::new(
            identity_factory,
            cert_factory,
            vec![endpoint],
            test_server_runtime(),
        )
        .set_tcp_accept_timout(Duration::from_secs(3))
        .set_tcp_connect_timout(Duration::from_secs(3))
        .set_quic_connect_timeout(Duration::from_secs(3))
        .set_quic_idle_time(Duration::from_secs(10)),
    )
    .await?;

    create_p2p_stack(
        P2pStackConfig::new(env, client_identity)
            .add_sn_list(sn_list)
            .set_conn_timeout(Duration::from_secs(3))
            .set_sn_ping_interval(Duration::from_millis(200))
            .set_sn_call_timeout(Duration::from_secs(3))
            .set_sn_query_interval(Duration::from_secs(1))
            .set_sn_tunnel_count(2),
    )
    .await
}

struct MatrixOwnerPeerClient {
    local_id: P2pId,
    nodes: Arc<HashMap<P2pId, OwnerElectionNodeRef>>,
}

#[async_trait]
impl OwnerPeerControlClient for MatrixOwnerPeerClient {
    async fn request_vote(
        &self,
        remote_owner_id: &P2pId,
        request: OwnerVoteRequest,
    ) -> P2pResult<OwnerVoteResponse> {
        let node = self.remote(remote_owner_id)?;
        Ok(node.receive_vote_request(self.local_id.clone(), request, bucky_time_now()))
    }

    async fn send_heartbeat(
        &self,
        remote_owner_id: &P2pId,
        request: OwnerHeartbeatRequest,
    ) -> P2pResult<OwnerHeartbeatResponse> {
        let node = self.remote(remote_owner_id)?;
        Ok(node.receive_heartbeat(self.local_id.clone(), request, bucky_time_now()))
    }

    async fn replicate_session(
        &self,
        remote_owner_id: &P2pId,
        request: OwnerSessionReplication,
    ) -> P2pResult<OwnerSessionReplicationResponse> {
        let node = self.remote(remote_owner_id)?;
        Ok(node.receive_session_replication(self.local_id.clone(), request, bucky_time_now()))
    }

    async fn forward_session(
        &self,
        remote_owner_id: &P2pId,
        request: OwnerSessionForward,
    ) -> P2pResult<OwnerSessionForwardResponse> {
        let node = self.remote(remote_owner_id)?;
        Ok(node
            .receive_session_forward(self.local_id.clone(), request)
            .await)
    }
}

impl MatrixOwnerPeerClient {
    fn remote(&self, remote_owner_id: &P2pId) -> P2pResult<OwnerElectionNodeRef> {
        self.nodes.get(remote_owner_id).cloned().ok_or_else(|| {
            p2p_err!(
                P2pErrorCode::NotFound,
                "matrix owner peer missing {}",
                remote_owner_id
            )
        })
    }
}

struct FiveByFiveNode {
    _sn_service: SnServerRef,
    stack: P2pStackRef,
    serving_id: P2pId,
    user_id: P2pId,
}

async fn setup_five_by_five_matrix() -> (
    Vec<OwnerDirectoryServerRef>,
    Vec<FiveByFiveNode>,
    OwnerMembership,
    Arc<X509IdentityCertFactory>,
) {
    for _ in 0..SETUP_MAX_RETRY {
        let identity_factory = Arc::new(X509IdentityFactory);
        let cert_factory = Arc::new(X509IdentityCertFactory);

        let owner_ids = (0..5)
            .map(|index| P2pId::from(vec![0xA0 + index; 32]))
            .collect::<Vec<_>>();
        let owner_client_membership = OwnerMembership::with_options(
            owner_ids.clone(),
            OWNER_REPLICA_COUNT,
            Duration::from_secs(120),
        )
        .unwrap();
        let owner_servers = owner_ids
            .iter()
            .cloned()
            .map(|owner_id| {
                OwnerDirectoryServer::new_detached(owner_id, owner_client_membership.clone(), None)
            })
            .collect::<Vec<_>>();
        attach_owner_peer_clients(owner_servers.as_slice());
        let elected_leader = owner_servers[0].start_owner_election().await.unwrap();
        for owner in owner_servers.iter() {
            assert_eq!(
                owner.service().election_node().current_leader(),
                Some(elected_leader.clone())
            );
        }

        let mut setup_failed_with_retryable_error = false;
        let mut nodes = Vec::new();
        for index in 0..5 {
            let sn_identity = build_identity(
                format!("sn-5x5-serving-{}", index).as_str(),
                localhost_quic_endpoint(next_port()),
            );
            let sn_service = match start_sn_service_with_owner_client(
                sn_identity.clone(),
                identity_factory.clone(),
                cert_factory.clone(),
                owner_client_membership.clone(),
            )
            .await
            {
                Ok(service) => service,
                Err(e) if is_addr_bind_conflict(e.code()) => {
                    setup_failed_with_retryable_error = true;
                    break;
                }
                Err(e) => panic!("start 5x5 serving SN failed: {:?}", e),
            };
            let client_identity = build_identity(
                format!("sn-5x5-user-{}", index).as_str(),
                localhost_quic_endpoint(next_port()),
            );
            let user_id = client_identity.get_id();
            let serving_id = sn_identity.get_id();
            let stack = match start_client_stack(
                client_identity,
                vec![build_sn_entry(&sn_identity)],
                identity_factory.clone(),
                cert_factory.clone(),
            )
            .await
            {
                Ok(stack) => stack,
                Err(e) if is_addr_bind_conflict(e.code()) => {
                    setup_failed_with_retryable_error = true;
                    break;
                }
                Err(e) => panic!("start 5x5 user stack failed: {:?}", e),
            };

            stack
                .wait_online(Some(MATRIX_ONLINE_TIMEOUT))
                .await
                .unwrap_or_else(|e| panic!("wait 5x5 user stack {index} online failed: {:?}", e));
            let active = stack.sn_client().get_active_sn_list();
            assert_eq!(active.len(), 1);
            assert_eq!(active[0].sn_peer_id, serving_id);

            nodes.push(FiveByFiveNode {
                _sn_service: sn_service,
                stack,
                serving_id,
                user_id,
            });
        }
        if setup_failed_with_retryable_error {
            continue;
        }
        assert_eq!(nodes.len(), 5);

        publish_matrix_routes(
            owner_client_membership.clone(),
            owner_servers.as_slice(),
            nodes.as_slice(),
        )
        .await;
        assert_owner_cluster_state(owner_servers.as_slice(), &elected_leader);
        assert_matrix_routes(
            owner_client_membership.clone(),
            owner_servers.as_slice(),
            nodes.as_slice(),
        );

        return (owner_servers, nodes, owner_client_membership, cert_factory);
    }

    panic!("setup 5x5 SN command matrix failed after retries");
}

fn attach_owner_peer_clients(owner_servers: &[OwnerDirectoryServerRef]) {
    let nodes = Arc::new(
        owner_servers
            .iter()
            .map(|owner| {
                (
                    owner.service().election_node().local_owner_id().clone(),
                    owner.service().election_node().clone(),
                )
            })
            .collect::<HashMap<_, _>>(),
    );
    for owner in owner_servers {
        let local_id = owner.service().election_node().local_owner_id().clone();
        owner
            .service()
            .election_node()
            .set_peer_client(Arc::new(MatrixOwnerPeerClient {
                local_id,
                nodes: nodes.clone(),
            }));
    }
}

async fn publish_matrix_routes(
    owner_membership: OwnerMembership,
    owner_servers: &[OwnerDirectoryServerRef],
    nodes: &[FiveByFiveNode],
) {
    let owners_by_id = owner_servers
        .iter()
        .map(|owner| {
            (
                owner.service().election_node().local_owner_id().clone(),
                owner.clone(),
            )
        })
        .collect::<HashMap<_, _>>();
    let resolver = OwnerResolver::new(Some(owner_membership));
    for (sequence, node) in nodes.iter().enumerate() {
        for owner in owner_servers {
            owner
                .service()
                .election_node()
                .renew_serving_session(
                    node.serving_id.clone(),
                    0,
                    Duration::from_secs(120),
                    bucky_time_now(),
                )
                .await
                .unwrap();
        }
        let lease = ServingLease::new(
            node.user_id.clone(),
            node.serving_id.clone(),
            (sequence + 1) as u64,
            Duration::from_secs(120),
            bucky_time_now(),
        );
        let owner_set = resolver.owner_set(&node.user_id, &node.serving_id);
        assert_eq!(owner_set.len(), OWNER_REPLICA_COUNT);
        for owner_id in owner_set {
            let owner = owners_by_id.get(&owner_id).unwrap_or_else(|| {
                panic!(
                    "expected route owner {} for peer {} is missing from matrix",
                    owner_id, node.user_id
                )
            });
            owner
                .service()
                .publish_lease_from_serving_sn(node.serving_id.clone(), lease.clone())
                .await
                .unwrap();
        }
    }
}

fn assert_owner_cluster_state(owner_servers: &[OwnerDirectoryServerRef], leader_id: &P2pId) {
    let leaders = owner_servers
        .iter()
        .filter(|owner| owner.service().election_node().role() == OwnerElectionRole::Leader)
        .collect::<Vec<_>>();
    assert_eq!(leaders.len(), 1);
    assert_eq!(
        leaders[0].service().election_node().local_owner_id(),
        leader_id
    );
    for owner in owner_servers {
        assert_eq!(
            owner.service().election_node().current_leader(),
            Some(leader_id.clone())
        );
    }
}

fn assert_matrix_routes(
    owner_membership: OwnerMembership,
    owner_servers: &[OwnerDirectoryServerRef],
    nodes: &[FiveByFiveNode],
) {
    let resolver = OwnerResolver::new(Some(owner_membership));
    let now = bucky_time_now();
    for node in nodes {
        for owner in owner_servers {
            assert!(
                owner
                    .service()
                    .election_node()
                    .control_plane()
                    .is_serving_online(&node.serving_id, now),
                "owner {} does not observe serving SN {} online",
                owner.service().election_node().local_owner_id(),
                node.serving_id
            );
        }
    }
    for (sequence, node) in nodes.iter().enumerate() {
        let expected_owners = resolver.owner_set(&node.user_id, &node.serving_id);
        assert_eq!(expected_owners.len(), OWNER_REPLICA_COUNT);
        for owner in owner_servers {
            let owner_id = owner.service().election_node().local_owner_id();
            let leases = owner.query_serving_leases(&node.user_id);
            if expected_owners.iter().any(|expected| expected == owner_id) {
                assert_eq!(leases.len(), 1);
                assert_eq!(leases[0].peer_id, node.user_id);
                assert_eq!(leases[0].serving_sn_id, node.serving_id);
                assert_eq!(leases[0].sequence, (sequence + 1) as u64);
            } else {
                assert!(
                    leases.is_empty(),
                    "non-owner {} unexpectedly stored route for peer {}",
                    owner_id,
                    node.user_id
                );
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sn_single_serving_with_owner_client_client_online() {
    for _ in 0..SETUP_MAX_RETRY {
        let identity_factory = Arc::new(X509IdentityFactory);
        let cert_factory = Arc::new(X509IdentityCertFactory);
        let owner_membership = OwnerMembership::with_options(
            vec![P2pId::from(vec![0xA0; 32])],
            1,
            Duration::from_secs(120),
        )
        .unwrap();
        let sn_identity =
            build_identity("sn-owner-membership", localhost_quic_endpoint(next_port()));
        let serving_id = sn_identity.get_id();
        let _sn_service = match start_sn_service_with_owner_client(
            sn_identity.clone(),
            identity_factory.clone(),
            cert_factory.clone(),
            owner_membership,
        )
        .await
        {
            Ok(service) => service,
            Err(e) if is_addr_bind_conflict(e.code()) => continue,
            Err(e) => panic!("start warmup serving SN failed: {:?}", e),
        };
        let client_identity =
            build_identity("sn-owner-client", localhost_quic_endpoint(next_port()));
        let stack = match start_client_stack(
            client_identity,
            vec![build_sn_entry(&sn_identity)],
            identity_factory,
            cert_factory,
        )
        .await
        {
            Ok(stack) => stack,
            Err(e) if is_addr_bind_conflict(e.code()) => continue,
            Err(e) => panic!("start warmup client stack failed: {:?}", e),
        };

        stack
            .wait_online(Some(MATRIX_ONLINE_TIMEOUT))
            .await
            .unwrap();
        let active = stack.sn_client().get_active_sn_list();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].sn_peer_id, serving_id);
        return;
    }

    panic!("setup warmup Serving SN/client online test failed after retries");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn sn_five_by_five_command_matrix_covers_all_sn_commands() {
    let (owner_servers, nodes, owner_membership, _cert_factory) = setup_five_by_five_matrix().await;
    assert_eq!(nodes.len(), 5);
    assert_owner_cluster_state(
        owner_servers.as_slice(),
        owner_servers[0].service().election_node().local_owner_id(),
    );
    assert_matrix_routes(
        owner_membership.clone(),
        owner_servers.as_slice(),
        nodes.as_slice(),
    );

    assert_eq!(
        [
            PackageCmdCode::SnCall,
            PackageCmdCode::SnCallResp,
            PackageCmdCode::SnCalled,
            PackageCmdCode::SnCalledResp,
            PackageCmdCode::ReportSn,
            PackageCmdCode::ReportSnResp,
            PackageCmdCode::SnQuery,
            PackageCmdCode::SnQueryResp,
        ]
        .iter()
        .filter(|command| command.is_sn())
        .count(),
        8
    );
    assert_eq!(
        [
            InterSnCommandCode::Heartbeat,
            InterSnCommandCode::PublishLease,
            InterSnCommandCode::QueryLease,
            InterSnCommandCode::QueryDetail,
            InterSnCommandCode::RelayCall,
        ]
        .iter()
        .map(|command| *command as u8)
        .collect::<Vec<_>>(),
        vec![0x80, 0x81, 0x82, 0x83, 0x84]
    );

    for round in 0..MATRIX_ROUNDS {
        for requester_index in 0..nodes.len() {
            for target_index in 0..nodes.len() {
                if requester_index == target_index {
                    continue;
                }

                let requester = &nodes[requester_index];
                let target = &nodes[target_index];
                assert_ne!(requester.serving_id, target.serving_id);

                let query_resp = requester
                    .stack
                    .sn_client()
                    .query(&target.user_id)
                    .await
                    .unwrap();
                assert!(query_resp.peer_info.is_none());
                assert!(query_resp.end_point_array.is_empty());

                let tunnel_id: TunnelId =
                    ((0x5000 + round * 100 + requester_index * 10 + target_index) as u32).into();
                let payload = format!(
                    "sn-5x5-round-{}-{}-to-{}",
                    round, requester_index, target_index
                )
                .into_bytes();
                let call_resp = requester
                    .stack
                    .sn_client()
                    .call(
                        tunnel_id,
                        Some(&[localhost_quic_endpoint(next_port())]),
                        &target.user_id,
                        TunnelType::Stream,
                        payload.clone(),
                    )
                    .await
                    .unwrap();
                assert_eq!(call_resp.result, P2pErrorCode::NotFound.as_u8());
                assert!(call_resp.to_peer_info.is_none());
            }
        }

        tokio::time::sleep(MATRIX_ROUND_DELAY).await;
    }

    for node in nodes.iter() {
        let active = node.stack.sn_client().get_active_sn_list();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].sn_peer_id, node.serving_id);
    }
}
