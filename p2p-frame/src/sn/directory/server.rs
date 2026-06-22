use super::{
    LocalOwnerDirectory, LocalOwnerDirectoryRef, LocalSnDirectory, OwnerDirectoryServerRef,
    OwnerDirectoryServiceRef, OwnerElectionNode, OwnerElectionNodeRef, OwnerElectionRole,
    OwnerHeartbeatRequest, OwnerHeartbeatResponse, OwnerMembership, OwnerSessionForward,
    OwnerSessionForwardResponse, OwnerSessionReplication, OwnerSessionReplicationResponse,
    OwnerVoteRequest, OwnerVoteResponse, ServingLease,
};
use crate::endpoint::Endpoint;
use crate::error::{P2pErrorCode, P2pResult, p2p_err};
use crate::executor::{Executor, SpawnHandle};
use crate::finder::{DeviceCache, DeviceCacheConfig};
use crate::networks::{
    NetManager, NetManagerRef, QuicCongestionAlgorithm, QuicTunnelNetwork, TcpTunnelNetwork,
    TunnelNetwork, TunnelNetworkRef, TunnelPurpose, TunnelStreamRead, TunnelStreamWrite,
};
use crate::p2p_identity::{
    P2pId, P2pIdentityCertFactoryRef, P2pIdentityFactoryRef, P2pIdentityRef,
};
use crate::runtime;
use crate::sn::inter_sn::{
    InterSnCommand, InterSnCommandContext, InterSnConnectionContext, InterSnError, InterSnPeer,
    SnInterServiceValidatorRef, TtpInterSnClient, TtpInterSnClientRef,
    allow_all_sn_inter_service_validator, require_accept,
};
use crate::sn::protocol::{SnPublishLease, SnQueryLease, SnQueryLeaseResp};
use crate::sn::types::{SnTunnelRead, SnTunnelWrite};
use crate::tls::{DefaultTlsServerCertResolver, TlsServerCertResolver, init_tls};
use crate::ttp::{TtpNode, TtpNodeRef, TtpPortListener, TtpServer, TtpServerRef};
use async_trait::async_trait;
use bucky_raw_codec::{
    CodecError, CodecErrorCode, RawConvertTo, RawDecode, RawEncode, RawEncodePurpose,
    RawFixedBytes, RawFrom,
};
use bucky_time::bucky_time_now;
use once_cell::sync::Lazy;
use sfo_cmd_server::errors::{CmdErrorCode, CmdResult, into_cmd_err};
use sfo_cmd_server::server::CmdServer;
use sfo_cmd_server::{CmdBody, CmdHeader, CmdTunnel, PeerId};
use sfo_reuseport::{ServerRuntime, ServerRuntimeConfig};
use std::collections::HashMap;
use std::sync::{
    Arc, Mutex, Weak,
    atomic::{self, AtomicBool},
};
use std::time::Duration;

const OWNER_SERVING_DIRECTORY_SERVICE: &str = "sn_owner_directory_serving";
const OWNER_CONTROL_LOOP_INTERVAL: Duration = Duration::from_secs(5);

type OwnerServingCmdServerService = sfo_cmd_server::server::DefaultCmdServerService<
    (),
    SnTunnelRead,
    SnTunnelWrite,
    u32,
    OwnerServingCommandCode,
>;

pub(crate) fn owner_serving_purpose() -> P2pResult<TunnelPurpose> {
    TunnelPurpose::from_value(&OWNER_SERVING_DIRECTORY_SERVICE.to_string())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(u8)]
pub(crate) enum OwnerServingCommandCode {
    PublishLease = 0x90,
    QueryLease = 0x91,
}

impl TryFrom<u8> for OwnerServingCommandCode {
    type Error = crate::error::P2pError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x90 => Ok(Self::PublishLease),
            0x91 => Ok(Self::QueryLease),
            _ => Err(p2p_err!(
                P2pErrorCode::InvalidParam,
                "invalid owner serving command code: {}",
                value
            )),
        }
    }
}

impl RawFixedBytes for OwnerServingCommandCode {
    fn raw_bytes() -> Option<usize> {
        Some(1)
    }
}

impl RawEncode for OwnerServingCommandCode {
    fn raw_measure(&self, _purpose: &Option<RawEncodePurpose>) -> Result<usize, CodecError> {
        Ok(Self::raw_bytes().unwrap())
    }

    fn raw_encode<'a>(
        &self,
        buf: &'a mut [u8],
        _purpose: &Option<RawEncodePurpose>,
    ) -> Result<&'a mut [u8], CodecError> {
        if buf.is_empty() {
            return Err(CodecError::new(
                CodecErrorCode::OutOfLimit,
                "not enough buffer for owner serving command code",
            ));
        }
        buf[0] = *self as u8;
        Ok(&mut buf[1..])
    }
}

impl<'de> RawDecode<'de> for OwnerServingCommandCode {
    fn raw_decode(buf: &'de [u8]) -> Result<(Self, &'de [u8]), CodecError> {
        if buf.is_empty() {
            return Err(CodecError::new(
                CodecErrorCode::OutOfLimit,
                "not enough buffer for owner serving command code",
            ));
        }
        let code = Self::try_from(buf[0]).map_err(|err| {
            CodecError::new(
                CodecErrorCode::Failed,
                format!("decode owner serving command code failed: {:?}", err),
            )
        })?;
        Ok((code, &buf[1..]))
    }
}

#[derive(Clone, Debug, RawEncode, RawDecode)]
pub(crate) enum OwnerServingResponse {
    Published(bool),
    Leases(SnQueryLeaseResp),
    Error(InterSnError),
}

pub struct OwnerDirectoryService {
    owner_directory: LocalOwnerDirectoryRef,
    election_node: OwnerElectionNodeRef,
    inter_service_validator: SnInterServiceValidatorRef,
}

impl OwnerDirectoryService {
    pub fn new(
        local_sn_id: P2pId,
        membership: OwnerMembership,
        inter_service_validator: Option<SnInterServiceValidatorRef>,
    ) -> OwnerDirectoryServiceRef {
        let directory = LocalSnDirectory::new(Some(membership.clone()));
        let election_node = OwnerElectionNode::new(
            local_sn_id,
            membership,
            directory.control_plane().clone(),
            bucky_time_now(),
        );
        Arc::new(Self {
            owner_directory: LocalOwnerDirectory::new(directory),
            election_node,
            inter_service_validator: inter_service_validator
                .unwrap_or_else(allow_all_sn_inter_service_validator),
        })
    }

    pub fn election_node(&self) -> &OwnerElectionNodeRef {
        &self.election_node
    }

    pub fn query_serving_leases(&self, peer_id: &P2pId) -> Vec<ServingLease> {
        self.owner_directory.query_serving_leases(peer_id)
    }

    pub async fn publish_lease_from_sn(
        &self,
        remote_sn_id: P2pId,
        lease: ServingLease,
    ) -> P2pResult<bool> {
        self.validate_connection(&remote_sn_id).await?;
        self.validate_command(&remote_sn_id, InterSnCommand::PublishLease, &lease.peer_id)
            .await?;
        self.owner_directory.refresh_owner_member(&remote_sn_id);
        Ok(self.owner_directory.publish_serving_lease(lease))
    }

    pub async fn query_leases_from_sn(
        &self,
        remote_sn_id: P2pId,
        peer_id: P2pId,
    ) -> P2pResult<Vec<ServingLease>> {
        self.validate_connection(&remote_sn_id).await?;
        self.validate_command(&remote_sn_id, InterSnCommand::QueryLease, &peer_id)
            .await?;
        self.owner_directory.refresh_owner_member(&remote_sn_id);
        Ok(self.owner_directory.query_serving_leases(&peer_id))
    }

    pub async fn publish_lease_from_serving_sn(
        &self,
        serving_sn_id: P2pId,
        lease: ServingLease,
    ) -> P2pResult<bool> {
        self.validate_connection(&serving_sn_id).await?;
        if lease.serving_sn_id != serving_sn_id {
            return Err(p2p_err!(
                P2pErrorCode::PermissionDenied,
                "serving lease serving_sn_id does not match remote serving sn id"
            ));
        }
        self.validate_command(&serving_sn_id, InterSnCommand::PublishLease, &lease.peer_id)
            .await?;
        Ok(self.owner_directory.publish_serving_lease(lease))
    }

    pub async fn query_leases_from_serving_sn(
        &self,
        serving_sn_id: P2pId,
        peer_id: P2pId,
    ) -> P2pResult<Vec<ServingLease>> {
        self.validate_connection(&serving_sn_id).await?;
        self.validate_command(&serving_sn_id, InterSnCommand::QueryLease, &peer_id)
            .await?;
        Ok(self.owner_directory.query_serving_leases(&peer_id))
    }

    pub async fn heartbeat_from_sn(
        &self,
        remote_sn_id: P2pId,
        member_sn_id: P2pId,
    ) -> P2pResult<bool> {
        self.validate_connection(&remote_sn_id).await?;
        if remote_sn_id != member_sn_id {
            return Err(p2p_err!(
                P2pErrorCode::PermissionDenied,
                "owner heartbeat member id does not match remote sn id"
            ));
        }
        self.validate_command(&remote_sn_id, InterSnCommand::Heartbeat, &member_sn_id)
            .await?;
        Ok(self.owner_directory.refresh_owner_member(&member_sn_id))
    }

    pub fn receive_owner_vote(
        &self,
        remote_sn_id: P2pId,
        request: OwnerVoteRequest,
    ) -> OwnerVoteResponse {
        self.election_node
            .receive_vote_request(remote_sn_id, request, bucky_time_now())
    }

    pub fn receive_owner_control_heartbeat(
        &self,
        remote_sn_id: P2pId,
        request: OwnerHeartbeatRequest,
    ) -> OwnerHeartbeatResponse {
        self.election_node
            .receive_heartbeat(remote_sn_id, request, bucky_time_now())
    }

    pub fn receive_owner_session_replication(
        &self,
        remote_sn_id: P2pId,
        request: OwnerSessionReplication,
    ) -> OwnerSessionReplicationResponse {
        self.election_node
            .receive_session_replication(remote_sn_id, request, bucky_time_now())
    }

    pub async fn receive_owner_session_forward(
        &self,
        remote_sn_id: P2pId,
        request: OwnerSessionForward,
    ) -> OwnerSessionForwardResponse {
        self.election_node
            .receive_session_forward(remote_sn_id, request)
            .await
    }

    async fn validate_connection(&self, remote_sn_id: &P2pId) -> P2pResult<()> {
        require_accept(
            self.inter_service_validator
                .validate_connection(&InterSnConnectionContext {
                    remote_sn_id: remote_sn_id.clone(),
                })
                .await?,
            "connection",
        )
        .await
    }

    async fn validate_command(
        &self,
        remote_sn_id: &P2pId,
        command: InterSnCommand,
        peer_id: &P2pId,
    ) -> P2pResult<()> {
        require_accept(
            self.inter_service_validator
                .validate_command(&InterSnCommandContext {
                    remote_sn_id: remote_sn_id.clone(),
                    command,
                    peer_id: peer_id.clone(),
                })
                .await?,
            "command",
        )
        .await
    }
}

pub struct OwnerDirectoryServer {
    local_sn_id: P2pId,
    service: OwnerDirectoryServiceRef,
    membership: OwnerMembership,
    local_identity: Option<P2pIdentityRef>,
    identity_factory: Option<P2pIdentityFactoryRef>,
    cert_factory: Option<P2pIdentityCertFactoryRef>,
    congestion_algorithm: QuicCongestionAlgorithm,
    reuse_address: bool,
    server_runtime: Option<ServerRuntime>,
    owner_peer_net_manager: Mutex<Option<NetManagerRef>>,
    serving_net_manager: Mutex<Option<NetManagerRef>>,
    owner_peer_endpoints: Vec<Endpoint>,
    serving_endpoints: Vec<Endpoint>,
    owner_peer_node: Mutex<Option<TtpNodeRef>>,
    serving_ttp_server: Mutex<Option<TtpServerRef>>,
    peer_transport: Mutex<Option<TtpInterSnClientRef>>,
    serving_cmd_service: Mutex<Option<Arc<OwnerServingCmdServerService>>>,
    owner_control_task: Mutex<Option<SpawnHandle<()>>>,
    started: AtomicBool,
    stopped: AtomicBool,
}

#[derive(Clone)]
pub struct OwnerDirectoryListenConfig {
    pub local_identity: P2pIdentityRef,
    pub identity_factory: P2pIdentityFactoryRef,
    pub cert_factory: P2pIdentityCertFactoryRef,
    pub owner_peer_endpoints: Vec<Endpoint>,
    pub serving_endpoints: Vec<Endpoint>,
    pub congestion_algorithm: QuicCongestionAlgorithm,
    pub reuse_address: bool,
    pub server_runtime: ServerRuntime,
}

impl OwnerDirectoryServer {
    pub fn new_with_default_runtime(
        local_identity: P2pIdentityRef,
        identity_factory: P2pIdentityFactoryRef,
        cert_factory: P2pIdentityCertFactoryRef,
        owner_peer_endpoints: Vec<Endpoint>,
        serving_endpoints: Vec<Endpoint>,
        membership: OwnerMembership,
        inter_service_validator: Option<SnInterServiceValidatorRef>,
    ) -> P2pResult<OwnerDirectoryServerRef> {
        let server_runtime =
            ServerRuntime::start(ServerRuntimeConfig::default()).map_err(|err| {
                p2p_err!(
                    P2pErrorCode::Failed,
                    "start owner directory server runtime failed: {:?}",
                    err
                )
            })?;
        Self::new(
            OwnerDirectoryListenConfig {
                local_identity,
                identity_factory,
                cert_factory,
                owner_peer_endpoints,
                serving_endpoints,
                congestion_algorithm: QuicCongestionAlgorithm::Bbr,
                reuse_address: false,
                server_runtime,
            },
            membership,
            inter_service_validator,
        )
    }

    pub fn new(
        listen_config: OwnerDirectoryListenConfig,
        membership: OwnerMembership,
        inter_service_validator: Option<SnInterServiceValidatorRef>,
    ) -> P2pResult<OwnerDirectoryServerRef> {
        if listen_config.owner_peer_endpoints.is_empty() {
            return Err(p2p_err!(
                P2pErrorCode::InvalidParam,
                "owner directory owner-peer listen endpoints must not be empty"
            ));
        }
        if listen_config.serving_endpoints.is_empty() {
            return Err(p2p_err!(
                P2pErrorCode::InvalidParam,
                "owner directory serving listen endpoints must not be empty"
            ));
        }

        Ok(Self::new_inner(
            listen_config.local_identity.get_id(),
            membership,
            inter_service_validator,
            Some(listen_config.local_identity),
            Some(listen_config.identity_factory),
            Some(listen_config.cert_factory),
            listen_config.congestion_algorithm,
            listen_config.reuse_address,
            Some(listen_config.server_runtime),
            listen_config.owner_peer_endpoints,
            listen_config.serving_endpoints,
        ))
    }

    pub fn new_detached(
        local_sn_id: P2pId,
        membership: OwnerMembership,
        inter_service_validator: Option<SnInterServiceValidatorRef>,
    ) -> OwnerDirectoryServerRef {
        Self::new_inner(
            local_sn_id,
            membership,
            inter_service_validator,
            None,
            None,
            None,
            QuicCongestionAlgorithm::Bbr,
            false,
            None,
            Vec::new(),
            Vec::new(),
        )
    }

    fn new_inner(
        local_sn_id: P2pId,
        membership: OwnerMembership,
        inter_service_validator: Option<SnInterServiceValidatorRef>,
        local_identity: Option<P2pIdentityRef>,
        identity_factory: Option<P2pIdentityFactoryRef>,
        cert_factory: Option<P2pIdentityCertFactoryRef>,
        congestion_algorithm: QuicCongestionAlgorithm,
        reuse_address: bool,
        server_runtime: Option<ServerRuntime>,
        owner_peer_endpoints: Vec<Endpoint>,
        serving_endpoints: Vec<Endpoint>,
    ) -> OwnerDirectoryServerRef {
        let server = Arc::new(Self {
            local_sn_id: local_sn_id.clone(),
            membership: membership.clone(),
            service: OwnerDirectoryService::new(
                local_sn_id.clone(),
                membership,
                inter_service_validator,
            ),
            local_identity,
            identity_factory,
            cert_factory,
            congestion_algorithm,
            reuse_address,
            server_runtime,
            owner_peer_net_manager: Mutex::new(None),
            serving_net_manager: Mutex::new(None),
            owner_peer_endpoints,
            serving_endpoints,
            owner_peer_node: Mutex::new(None),
            serving_ttp_server: Mutex::new(None),
            peer_transport: Mutex::new(None),
            serving_cmd_service: Mutex::new(None),
            owner_control_task: Mutex::new(None),
            started: AtomicBool::new(false),
            stopped: AtomicBool::new(false),
        });
        server
    }

    pub fn service(&self) -> &OwnerDirectoryServiceRef {
        &self.service
    }

    pub fn query_serving_leases(&self, peer_id: &P2pId) -> Vec<ServingLease> {
        self.service.query_serving_leases(peer_id)
    }

    pub fn refresh_member_heartbeat(&self, member_sn_id: &P2pId) -> bool {
        self.service
            .owner_directory
            .refresh_owner_member(member_sn_id)
    }

    pub async fn start_owner_election(&self) -> P2pResult<P2pId> {
        self.service
            .election_node()
            .start_election(bucky_time_now())
            .await
    }

    pub async fn tick_owner_election(&self) -> P2pResult<Option<P2pId>> {
        self.service.election_node().tick(bucky_time_now()).await
    }

    pub async fn send_owner_heartbeat(&self) -> P2pResult<usize> {
        self.service
            .election_node()
            .send_heartbeat(bucky_time_now())
            .await
    }

    pub async fn start(self: &Arc<Self>) -> P2pResult<()> {
        if self
            .started
            .compare_exchange(
                false,
                true,
                atomic::Ordering::SeqCst,
                atomic::Ordering::SeqCst,
            )
            .is_err()
        {
            return Ok(());
        }

        if let Err(err) = self.start_inner().await {
            self.started.store(false, atomic::Ordering::SeqCst);
            self.stop_owner_control_loop();
            return Err(err);
        }
        Ok(())
    }

    async fn start_inner(self: &Arc<Self>) -> P2pResult<()> {
        let local_identity = self.local_identity.clone().ok_or_else(|| {
            p2p_err!(
                P2pErrorCode::InvalidParam,
                "owner directory server has no local identity"
            )
        })?;
        let identity_factory = self.identity_factory.clone().ok_or_else(|| {
            p2p_err!(
                P2pErrorCode::InvalidParam,
                "owner directory server has no identity factory"
            )
        })?;
        let cert_factory = self.cert_factory.clone().ok_or_else(|| {
            p2p_err!(
                P2pErrorCode::InvalidParam,
                "owner directory server has no certificate factory"
            )
        })?;
        let server_runtime = self.server_runtime.clone().ok_or_else(|| {
            p2p_err!(
                P2pErrorCode::InvalidParam,
                "owner directory server has no server runtime"
            )
        })?;
        if self.owner_peer_endpoints.is_empty() {
            return Err(p2p_err!(
                P2pErrorCode::InvalidParam,
                "owner directory owner-peer listen endpoints must not be empty"
            ));
        }
        if self.serving_endpoints.is_empty() {
            return Err(p2p_err!(
                P2pErrorCode::InvalidParam,
                "owner directory serving listen endpoints must not be empty"
            ));
        }

        init_tls(identity_factory);
        let owner_peer_net_manager = create_owner_directory_net_manager(
            local_identity.clone(),
            cert_factory.clone(),
            self.congestion_algorithm,
            self.reuse_address,
            server_runtime.clone(),
        )
        .await?;
        let serving_net_manager = create_owner_directory_net_manager(
            local_identity.clone(),
            cert_factory,
            self.congestion_algorithm,
            self.reuse_address,
            server_runtime,
        )
        .await?;
        let owner_peer_node = TtpNode::new(local_identity.clone(), owner_peer_net_manager.clone())?;
        let serving_ttp_server = TtpServer::new(local_identity, serving_net_manager.clone())?;

        self.attach_peer_transport(owner_peer_node.clone(), &self.membership)
            .await?;
        self.attach_serving_listener(serving_ttp_server.clone())
            .await?;

        owner_peer_net_manager
            .listen(self.owner_peer_endpoints.as_slice(), None)
            .await?;
        serving_net_manager
            .listen(self.serving_endpoints.as_slice(), None)
            .await?;

        *self.owner_peer_net_manager.lock().unwrap() = Some(owner_peer_net_manager);
        *self.serving_net_manager.lock().unwrap() = Some(serving_net_manager);
        *self.owner_peer_node.lock().unwrap() = Some(owner_peer_node);
        *self.serving_ttp_server.lock().unwrap() = Some(serving_ttp_server);

        self.start_owner_control_loop()
    }

    pub fn start_owner_control_loop(self: &Arc<Self>) -> P2pResult<()> {
        let mut task = self.owner_control_task.lock().unwrap();
        if task.is_some() {
            return Ok(());
        }
        self.stopped.store(false, atomic::Ordering::SeqCst);
        let server = self.clone();
        *task = Some(Executor::spawn_with_handle(async move {
            server.run_owner_control_loop().await;
        })?);
        Ok(())
    }

    async fn run_owner_control_loop(self: Arc<Self>) {
        loop {
            if self.stopped.load(atomic::Ordering::SeqCst) {
                break;
            }

            let result = if self.service.election_node().role() == OwnerElectionRole::Leader {
                self.send_owner_heartbeat().await.map(|_| ())
            } else if self.service.election_node().current_leader().is_none() {
                self.start_owner_election().await.map(|_| ())
            } else {
                self.tick_owner_election().await.map(|_| ())
            };
            if let Err(err) = result {
                log::debug!(
                    "owner directory control loop local_id={} err={:?}",
                    self.local_sn_id,
                    err
                );
            }

            runtime::sleep(OWNER_CONTROL_LOOP_INTERVAL).await;
        }
    }

    pub fn stop_owner_control_loop(&self) {
        self.started.store(false, atomic::Ordering::SeqCst);
        self.stopped.store(true, atomic::Ordering::SeqCst);
        if let Some(task) = self.owner_control_task.lock().unwrap().take() {
            task.abort();
        }
    }

    pub async fn attach_peer_transport(
        self: &Arc<Self>,
        ttp_node: TtpNodeRef,
        membership: &OwnerMembership,
    ) -> P2pResult<TtpInterSnClientRef> {
        let transport =
            TtpInterSnClient::new(ttp_node, membership, self.clone() as Arc<dyn InterSnPeer>)
                .await?;
        self.service
            .election_node()
            .set_peer_client(transport.clone());
        *self.peer_transport.lock().unwrap() = Some(transport.clone());
        Ok(transport)
    }

    pub fn peer_transport(&self) -> Option<TtpInterSnClientRef> {
        self.peer_transport.lock().unwrap().clone()
    }

    pub async fn attach_serving_listener(
        self: &Arc<Self>,
        ttp_server: TtpServerRef,
    ) -> P2pResult<()> {
        let cmd_service = sfo_cmd_server::server::DefaultCmdServerService::new();
        register_owner_serving_cmd_handlers(&cmd_service, self.service.clone());
        listen_owner_serving_cmd_tunnels(ttp_server, cmd_service.clone()).await?;
        *self.serving_cmd_service.lock().unwrap() = Some(cmd_service);
        Ok(())
    }

    pub fn has_serving_listener(&self) -> bool {
        self.serving_cmd_service.lock().unwrap().is_some()
    }

    pub fn owner_peer_node(&self) -> Option<TtpNodeRef> {
        self.owner_peer_node.lock().unwrap().clone()
    }

    pub fn serving_ttp_server(&self) -> Option<TtpServerRef> {
        self.serving_ttp_server.lock().unwrap().clone()
    }
}

impl Drop for OwnerDirectoryServer {
    fn drop(&mut self) {
        self.stopped.store(true, atomic::Ordering::SeqCst);
        if let Some(task) = self.owner_control_task.lock().unwrap().take() {
            task.abort();
        }
    }
}

async fn create_owner_directory_net_manager(
    local_identity: P2pIdentityRef,
    cert_factory: P2pIdentityCertFactoryRef,
    congestion_algorithm: QuicCongestionAlgorithm,
    reuse_address: bool,
    server_runtime: ServerRuntime,
) -> P2pResult<NetManagerRef> {
    let device_cache = Arc::new(DeviceCache::new(
        &DeviceCacheConfig {
            expire: Duration::from_secs(240),
            capacity: 10240,
        },
        None,
    ));
    let cert_resolver = DefaultTlsServerCertResolver::new();
    let _ = cert_resolver
        .add_server_identity(local_identity.clone())
        .await;
    let tcp_network = Arc::new(TcpTunnelNetwork::new(
        cert_resolver.clone(),
        cert_factory.clone(),
        Duration::from_secs(30),
        Duration::from_secs(5),
        Duration::from_secs(15),
        server_runtime.clone(),
    ));
    TunnelNetwork::set_reuse_address(tcp_network.as_ref(), reuse_address);
    let quic_network = Arc::new(QuicTunnelNetwork::new(
        device_cache,
        cert_resolver.clone(),
        cert_factory,
        congestion_algorithm,
        Duration::from_secs(30),
        Duration::from_secs(30),
        server_runtime,
    ));
    TunnelNetwork::set_reuse_address(quic_network.as_ref(), reuse_address);
    NetManager::new(
        vec![
            tcp_network as TunnelNetworkRef,
            quic_network as TunnelNetworkRef,
        ],
        cert_resolver,
    )
}

#[async_trait]
pub(crate) trait OwnerServingEndpoint: Send + Sync + 'static {
    fn sn_id(&self) -> P2pId;

    async fn publish_lease_from_serving_sn(
        &self,
        serving_sn_id: P2pId,
        lease: ServingLease,
    ) -> P2pResult<bool>;

    async fn query_leases_from_serving_sn(
        &self,
        serving_sn_id: P2pId,
        peer_id: P2pId,
    ) -> P2pResult<Vec<ServingLease>>;
}

#[derive(Default)]
pub(crate) struct OwnerServingRegistry {
    endpoints: Mutex<HashMap<P2pId, Weak<dyn OwnerServingEndpoint>>>,
}

impl OwnerServingRegistry {
    pub(crate) fn global() -> &'static Self {
        static REGISTRY: Lazy<OwnerServingRegistry> = Lazy::new(OwnerServingRegistry::default);
        &REGISTRY
    }

    pub(crate) fn register(&self, endpoint: Arc<dyn OwnerServingEndpoint>) {
        self.endpoints
            .lock()
            .unwrap()
            .insert(endpoint.sn_id(), Arc::downgrade(&endpoint));
    }

    pub(crate) fn get(&self, sn_id: &P2pId) -> Option<Arc<dyn OwnerServingEndpoint>> {
        let mut endpoints = self.endpoints.lock().unwrap();
        let Some(endpoint) = endpoints.get(sn_id).and_then(Weak::upgrade) else {
            endpoints.remove(sn_id);
            return None;
        };
        Some(endpoint)
    }
}

#[async_trait]
impl InterSnPeer for OwnerDirectoryServer {
    fn sn_id(&self) -> Option<P2pId> {
        Some(self.local_sn_id.clone())
    }

    async fn publish_lease_from_sn(
        &self,
        remote_sn_id: P2pId,
        lease: ServingLease,
    ) -> P2pResult<bool> {
        self.service
            .publish_lease_from_sn(remote_sn_id, lease)
            .await
    }

    async fn heartbeat_from_sn(&self, remote_sn_id: P2pId, member_sn_id: P2pId) -> P2pResult<bool> {
        self.service
            .heartbeat_from_sn(remote_sn_id, member_sn_id)
            .await
    }

    async fn owner_vote_from_sn(
        &self,
        remote_sn_id: P2pId,
        request: OwnerVoteRequest,
    ) -> P2pResult<OwnerVoteResponse> {
        Ok(self.service.receive_owner_vote(remote_sn_id, request))
    }

    async fn owner_control_heartbeat_from_sn(
        &self,
        remote_sn_id: P2pId,
        request: OwnerHeartbeatRequest,
    ) -> P2pResult<OwnerHeartbeatResponse> {
        Ok(self
            .service
            .receive_owner_control_heartbeat(remote_sn_id, request))
    }

    async fn owner_session_replication_from_sn(
        &self,
        remote_sn_id: P2pId,
        request: OwnerSessionReplication,
    ) -> P2pResult<OwnerSessionReplicationResponse> {
        Ok(self
            .service
            .receive_owner_session_replication(remote_sn_id, request))
    }

    async fn owner_session_forward_from_sn(
        &self,
        remote_sn_id: P2pId,
        request: OwnerSessionForward,
    ) -> P2pResult<OwnerSessionForwardResponse> {
        Ok(self
            .service
            .receive_owner_session_forward(remote_sn_id, request)
            .await)
    }

    async fn query_leases_from_sn(
        &self,
        remote_sn_id: P2pId,
        peer_id: P2pId,
    ) -> P2pResult<Vec<ServingLease>> {
        self.service
            .query_leases_from_sn(remote_sn_id, peer_id)
            .await
    }
}

#[async_trait]
impl OwnerServingEndpoint for OwnerDirectoryServer {
    fn sn_id(&self) -> P2pId {
        self.local_sn_id.clone()
    }

    async fn publish_lease_from_serving_sn(
        &self,
        serving_sn_id: P2pId,
        lease: ServingLease,
    ) -> P2pResult<bool> {
        self.service
            .publish_lease_from_serving_sn(serving_sn_id, lease)
            .await
    }

    async fn query_leases_from_serving_sn(
        &self,
        serving_sn_id: P2pId,
        peer_id: P2pId,
    ) -> P2pResult<Vec<ServingLease>> {
        self.service
            .query_leases_from_serving_sn(serving_sn_id, peer_id)
            .await
    }
}

async fn listen_owner_serving_cmd_tunnels(
    ttp_server: TtpServerRef,
    cmd_service: Arc<OwnerServingCmdServerService>,
) -> P2pResult<()> {
    ttp_server
        .listen_control_stream(
            owner_serving_purpose()?,
            Arc::new(move |accepted| {
                let cmd_service = cmd_service.clone();
                Box::pin(async move {
                    let accepted = match accepted {
                        Ok(accepted) => accepted,
                        Err(err) => {
                            log::warn!("owner serving-facing directory accept stopped: {:?}", err);
                            return;
                        }
                    };
                    let tunnel = into_owner_serving_cmd_tunnel(accepted);
                    if let Err(err) = cmd_service.serve_tunnel(tunnel).await {
                        log::warn!("owner serving-facing command tunnel failed: {:?}", err);
                    }
                })
            }),
        )
        .await
}

pub(crate) fn into_owner_serving_cmd_tunnel(
    accepted: (
        crate::ttp::TtpStreamMeta,
        TunnelStreamRead,
        TunnelStreamWrite,
    ),
) -> CmdTunnel<SnTunnelRead, SnTunnelWrite> {
    let (meta, read, write) = accepted;
    let local = meta.local_ep.unwrap_or_default();
    let remote = meta.remote_ep.unwrap_or_default();
    let local_id = meta.local_id;
    let remote_id = meta.remote_id;
    CmdTunnel::new(
        SnTunnelRead::new(read, local, remote, local_id.clone(), remote_id.clone()),
        SnTunnelWrite::new(write, local, remote, local_id, remote_id),
    )
}

fn register_owner_serving_cmd_handlers(
    cmd_service: &Arc<OwnerServingCmdServerService>,
    service: OwnerDirectoryServiceRef,
) {
    for command in [
        OwnerServingCommandCode::PublishLease,
        OwnerServingCommandCode::QueryLease,
    ] {
        let service = service.clone();
        cmd_service.register_cmd_handler(
            command,
            move |_local_id: PeerId,
                  remote_id: PeerId,
                  _tunnel_id,
                  header: CmdHeader<u32, OwnerServingCommandCode>,
                  mut body: CmdBody| {
                let service = service.clone();
                async move {
                    handle_owner_serving_cmd(
                        service,
                        P2pId::from(remote_id.as_slice()),
                        header,
                        &mut body,
                    )
                    .await
                }
            },
        );
    }
}

async fn handle_owner_serving_cmd(
    service: OwnerDirectoryServiceRef,
    remote_sn_id: P2pId,
    header: CmdHeader<u32, OwnerServingCommandCode>,
    body: &mut CmdBody,
) -> CmdResult<Option<CmdBody>> {
    let response = dispatch_owner_serving_cmd(service, remote_sn_id, header.cmd_code(), body).await;
    let response = response.to_vec().map_err(into_cmd_err!(
        CmdErrorCode::Failed,
        "encode owner serving response failed"
    ))?;
    Ok(Some(CmdBody::from_bytes(response)))
}

async fn dispatch_owner_serving_cmd(
    service: OwnerDirectoryServiceRef,
    remote_sn_id: P2pId,
    command: OwnerServingCommandCode,
    body: &mut CmdBody,
) -> OwnerServingResponse {
    let bytes = match body.read_all().await {
        Ok(bytes) => bytes,
        Err(err) => {
            return OwnerServingResponse::Error(
                p2p_err!(
                    P2pErrorCode::Failed,
                    "read owner serving command body failed: {:?}",
                    err
                )
                .into(),
            );
        }
    };

    let result = match command {
        OwnerServingCommandCode::PublishLease => {
            match SnPublishLease::clone_from_slice(bytes.as_slice()) {
                Ok(lease) => service
                    .publish_lease_from_serving_sn(remote_sn_id, lease.into())
                    .await
                    .map(OwnerServingResponse::Published),
                Err(err) => Err(p2p_err!(
                    P2pErrorCode::RawCodecError,
                    "decode serving publish lease failed: {:?}",
                    err
                )),
            }
        }
        OwnerServingCommandCode::QueryLease => {
            match SnQueryLease::clone_from_slice(bytes.as_slice()) {
                Ok(query) => service
                    .query_leases_from_serving_sn(remote_sn_id, query.peer_id)
                    .await
                    .map(|leases| {
                        OwnerServingResponse::Leases(SnQueryLeaseResp {
                            leases: leases.into_iter().map(SnPublishLease::from).collect(),
                        })
                    }),
                Err(err) => Err(p2p_err!(
                    P2pErrorCode::RawCodecError,
                    "decode serving query lease failed: {:?}",
                    err
                )),
            }
        }
    };

    result.unwrap_or_else(|err| OwnerServingResponse::Error(err.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn test_id(seed: u8) -> P2pId {
        P2pId::from(vec![seed; 32])
    }

    fn test_service(owner_sn_id: P2pId) -> OwnerDirectoryServiceRef {
        let membership =
            OwnerMembership::with_options(vec![owner_sn_id.clone()], 1, Duration::from_secs(60))
                .unwrap();
        OwnerDirectoryService::new(owner_sn_id, membership, None)
    }

    #[tokio::test]
    async fn owner_serving_listener_dispatches_publish() {
        let service = test_service(test_id(10));
        let serving_sn_id = test_id(20);
        let peer_id = test_id(30);
        let lease = ServingLease {
            peer_id: peer_id.clone(),
            serving_sn_id: serving_sn_id.clone(),
            sequence: 7,
            expires_at: bucky_time::bucky_time_now() + 60_000_000,
        };
        let mut body = CmdBody::from_bytes(SnPublishLease::from(lease.clone()).to_vec().unwrap());

        let response = dispatch_owner_serving_cmd(
            service.clone(),
            serving_sn_id.clone(),
            OwnerServingCommandCode::PublishLease,
            &mut body,
        )
        .await;

        match response {
            OwnerServingResponse::Published(false) => {}
            other => panic!("unexpected response: {:?}", other),
        }
        assert!(service.query_serving_leases(&peer_id).is_empty());

        service
            .election_node()
            .renew_serving_session(
                serving_sn_id.clone(),
                0,
                Duration::from_secs(60),
                bucky_time::bucky_time_now(),
            )
            .await
            .unwrap();
        let mut body = CmdBody::from_bytes(SnPublishLease::from(lease.clone()).to_vec().unwrap());
        let response = dispatch_owner_serving_cmd(
            service.clone(),
            serving_sn_id,
            OwnerServingCommandCode::PublishLease,
            &mut body,
        )
        .await;

        match response {
            OwnerServingResponse::Published(true) => {}
            other => panic!("unexpected response after online renew: {:?}", other),
        }
        let leases = service.query_serving_leases(&peer_id);
        assert_eq!(leases.len(), 1);
        assert_eq!(leases[0].peer_id, lease.peer_id);
        assert_eq!(leases[0].serving_sn_id, lease.serving_sn_id);
        assert_eq!(leases[0].sequence, lease.sequence);
        assert!(leases[0].expires_at >= lease.expires_at);
    }

    #[tokio::test]
    async fn owner_serving_listener_rejects_publish_for_different_serving_sn() {
        let service = test_service(test_id(11));
        let serving_sn_id = test_id(21);
        let peer_id = test_id(31);
        let lease = ServingLease {
            peer_id: peer_id.clone(),
            serving_sn_id: test_id(22),
            sequence: 8,
            expires_at: bucky_time::bucky_time_now() + 60_000_000,
        };
        let mut body = CmdBody::from_bytes(SnPublishLease::from(lease).to_vec().unwrap());

        let response = dispatch_owner_serving_cmd(
            service.clone(),
            serving_sn_id,
            OwnerServingCommandCode::PublishLease,
            &mut body,
        )
        .await;

        match response {
            OwnerServingResponse::Error(err) => {
                assert_eq!(err.code, P2pErrorCode::PermissionDenied);
            }
            other => panic!("unexpected response: {:?}", other),
        }
        assert!(service.query_serving_leases(&peer_id).is_empty());
    }
}
