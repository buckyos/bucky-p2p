use crate::endpoint::Endpoint;
use crate::error::{P2pError, P2pErrorCode, P2pResult, into_p2p_err, p2p_err};
use crate::networks::{TunnelPurpose, TunnelStreamRead, TunnelStreamWrite, ValidateResult};
use crate::p2p_identity::{EncodedP2pIdentityCert, P2pId};
use crate::sn::directory::{
    OwnerHeartbeatRequest, OwnerHeartbeatResponse, OwnerMembership, OwnerPeerControlClient,
    OwnerSessionForward, OwnerSessionForwardResponse, OwnerSessionReplication,
    OwnerSessionReplicationResponse, OwnerVoteRequest, OwnerVoteResponse, ServingLease,
};
use crate::sn::protocol::{
    InterSnCommandCode, SnCall, SnDetailQuery, SnDetailResp, SnOwnerHeartbeat, SnPublishLease,
    SnQueryLease, SnQueryLeaseResp, SnRelayCall,
};
use crate::sn::types::{OwnerCmdPkgLen, SnTunnelRead, SnTunnelWrite};
use crate::ttp::{TtpConnector, TtpNodeRef, TtpPortListener, TtpTarget};
use async_trait::async_trait;
use bucky_raw_codec::{RawConvertTo, RawDecode, RawEncode, RawFrom};
use once_cell::sync::Lazy;
use sfo_cmd_server::errors::{CmdErrorCode, CmdResult, cmd_err, into_cmd_err};
use sfo_cmd_server::{CmdBody, CmdHeader, CmdNode, CmdTunnel, PeerId};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

pub type SnInterServiceValidatorRef = Arc<dyn SnInterServiceValidator>;
pub type TtpInterSnClientRef = Arc<TtpInterSnClient>;
const INTER_SN_SERVICE: &str = "sn_inter_service";
const INTER_SN_CMD_VERSION: u8 = 0;
const INTER_SN_CMD_TIMEOUT: Duration = Duration::from_secs(10);
const INTER_SN_CMD_TUNNEL_COUNT: u16 = 8;

type OwnerCmdNodeService = sfo_cmd_server::DefaultCmdNodeService<
    (),
    SnTunnelRead,
    SnTunnelWrite,
    OwnerTunnelFactory,
    OwnerCmdPkgLen,
    InterSnCommandCode,
>;

pub fn inter_sn_purpose() -> P2pResult<TunnelPurpose> {
    TunnelPurpose::from_value(&INTER_SN_SERVICE.to_string())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InterSnConnectionContext {
    pub remote_sn_id: P2pId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InterSnCommand {
    Heartbeat,
    PublishLease,
    QueryLease,
    QueryDetail,
    RelayCall,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InterSnCommandContext {
    pub remote_sn_id: P2pId,
    pub command: InterSnCommand,
    pub peer_id: P2pId,
}

#[async_trait]
pub trait SnInterServiceValidator: Send + Sync + 'static {
    async fn validate_connection(
        &self,
        ctx: &InterSnConnectionContext,
    ) -> P2pResult<ValidateResult>;

    async fn validate_command(&self, ctx: &InterSnCommandContext) -> P2pResult<ValidateResult>;
}

pub struct AllowAllSnInterServiceValidator;

#[async_trait]
impl SnInterServiceValidator for AllowAllSnInterServiceValidator {
    async fn validate_connection(
        &self,
        _ctx: &InterSnConnectionContext,
    ) -> P2pResult<ValidateResult> {
        Ok(ValidateResult::Accept)
    }

    async fn validate_command(&self, _ctx: &InterSnCommandContext) -> P2pResult<ValidateResult> {
        Ok(ValidateResult::Accept)
    }
}

pub fn allow_all_sn_inter_service_validator() -> SnInterServiceValidatorRef {
    Arc::new(AllowAllSnInterServiceValidator)
}

pub async fn require_accept(result: ValidateResult, operation: &str) -> P2pResult<()> {
    match result {
        ValidateResult::Accept => Ok(()),
        ValidateResult::Reject(reason) => Err(p2p_err!(
            P2pErrorCode::PermissionDenied,
            "inter-sn {} rejected: {}",
            operation,
            reason
        )),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServingPeerDetail {
    pub peer_info: EncodedP2pIdentityCert,
    pub endpoints: Vec<Endpoint>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelayCallOutcome {
    pub accepted: bool,
    pub to_peer_info: Option<EncodedP2pIdentityCert>,
}

#[derive(Clone, Debug, RawEncode, RawDecode)]
pub enum InterSnRequest {
    Heartbeat(SnOwnerHeartbeat),
    OwnerVote(OwnerVoteRequest),
    OwnerControlHeartbeat(OwnerHeartbeatRequest),
    OwnerSessionReplication(OwnerSessionReplication),
    OwnerSessionForward(OwnerSessionForward),
    PublishLease(SnPublishLease),
    QueryLease(SnQueryLease),
    QueryDetail(SnDetailQuery),
    RelayCall(SnRelayCall),
}

#[derive(Clone, Debug, RawEncode, RawDecode)]
pub enum InterSnResponse {
    Heartbeat(bool),
    OwnerVote(OwnerVoteResponse),
    OwnerControlHeartbeat(OwnerHeartbeatResponse),
    OwnerSessionReplication(OwnerSessionReplicationResponse),
    OwnerSessionForward(OwnerSessionForwardResponse),
    Published(bool),
    Leases(SnQueryLeaseResp),
    Detail(SnDetailResp),
    Relay(RelayCallResponse),
    Error(InterSnError),
}

#[derive(Clone, Debug, RawEncode, RawDecode)]
pub struct RelayCallResponse {
    pub accepted: bool,
    pub to_peer_info: Option<EncodedP2pIdentityCert>,
}

#[derive(Clone, Debug, RawEncode, RawDecode)]
pub struct InterSnError {
    pub code: P2pErrorCode,
    pub message: String,
}

#[async_trait]
pub trait InterSnPeer: Send + Sync + 'static {
    fn sn_id(&self) -> Option<P2pId>;

    async fn publish_lease_from_sn(
        &self,
        _remote_sn_id: P2pId,
        _lease: ServingLease,
    ) -> P2pResult<bool> {
        Err(p2p_err!(
            P2pErrorCode::NotSupport,
            "inter-sn peer does not support owner lease publish"
        ))
    }

    async fn heartbeat_from_sn(
        &self,
        _remote_sn_id: P2pId,
        _member_sn_id: P2pId,
    ) -> P2pResult<bool> {
        Err(p2p_err!(
            P2pErrorCode::NotSupport,
            "inter-sn peer does not support owner heartbeat"
        ))
    }

    async fn owner_vote_from_sn(
        &self,
        _remote_sn_id: P2pId,
        _request: OwnerVoteRequest,
    ) -> P2pResult<OwnerVoteResponse> {
        Err(p2p_err!(
            P2pErrorCode::NotSupport,
            "inter-sn peer does not support owner vote"
        ))
    }

    async fn owner_control_heartbeat_from_sn(
        &self,
        _remote_sn_id: P2pId,
        _request: OwnerHeartbeatRequest,
    ) -> P2pResult<OwnerHeartbeatResponse> {
        Err(p2p_err!(
            P2pErrorCode::NotSupport,
            "inter-sn peer does not support owner control heartbeat"
        ))
    }

    async fn owner_session_replication_from_sn(
        &self,
        _remote_sn_id: P2pId,
        _request: OwnerSessionReplication,
    ) -> P2pResult<OwnerSessionReplicationResponse> {
        Err(p2p_err!(
            P2pErrorCode::NotSupport,
            "inter-sn peer does not support owner session replication"
        ))
    }

    async fn owner_session_forward_from_sn(
        &self,
        _remote_sn_id: P2pId,
        _request: OwnerSessionForward,
    ) -> P2pResult<OwnerSessionForwardResponse> {
        Err(p2p_err!(
            P2pErrorCode::NotSupport,
            "inter-sn peer does not support owner session forward"
        ))
    }

    async fn query_leases_from_sn(
        &self,
        _remote_sn_id: P2pId,
        _peer_id: P2pId,
    ) -> P2pResult<Vec<ServingLease>> {
        Err(p2p_err!(
            P2pErrorCode::NotSupport,
            "inter-sn peer does not support owner lease query"
        ))
    }

    async fn query_detail_from_sn(
        &self,
        _remote_sn_id: P2pId,
        _peer_id: P2pId,
    ) -> P2pResult<Option<ServingPeerDetail>> {
        Err(p2p_err!(
            P2pErrorCode::NotSupport,
            "inter-sn peer does not support serving detail query"
        ))
    }

    async fn relay_call_from_sn(
        &self,
        _remote_sn_id: P2pId,
        _call_req: SnCall,
    ) -> P2pResult<RelayCallOutcome> {
        Err(p2p_err!(
            P2pErrorCode::NotSupport,
            "inter-sn peer does not support serving relay call"
        ))
    }
}

#[derive(Default)]
pub struct InterSnRegistry {
    peers: Mutex<HashMap<P2pId, Weak<dyn InterSnPeer>>>,
}

pub struct TtpInterSnClient {
    cmd_node_service: Arc<OwnerCmdNodeService>,
    targets: HashMap<P2pId, TtpTarget>,
    local_sn_id: Option<P2pId>,
}

impl TtpInterSnClient {
    pub async fn new(
        ttp_node: TtpNodeRef,
        membership: &OwnerMembership,
        local_peer: Arc<dyn InterSnPeer>,
    ) -> P2pResult<TtpInterSnClientRef> {
        let targets = owner_targets(membership);
        let local_sn_id = local_peer.sn_id();
        let factory = OwnerTunnelFactory::new(ttp_node.clone(), targets.clone());
        let cmd_node_service =
            sfo_cmd_server::DefaultCmdNodeService::new(factory, INTER_SN_CMD_TUNNEL_COUNT);
        register_owner_cmd_handlers(&cmd_node_service, local_peer);
        listen_owner_cmd_tunnels(ttp_node, cmd_node_service.clone()).await?;
        Ok(Arc::new(Self {
            cmd_node_service,
            targets,
            local_sn_id,
        }))
    }

    fn target(&self, sn_id: &P2pId) -> P2pResult<&TtpTarget> {
        self.targets.get(sn_id).ok_or_else(|| {
            p2p_err!(
                P2pErrorCode::NotFound,
                "inter-sn target endpoint missing for {}",
                sn_id
            )
        })
    }

    pub async fn heartbeat_to_sn(&self, remote_sn_id: &P2pId) -> P2pResult<bool> {
        let member_sn_id = self.local_sn_id.clone().ok_or_else(|| {
            p2p_err!(
                P2pErrorCode::InvalidParam,
                "local sn id missing for owner heartbeat"
            )
        })?;
        match self
            .request(
                remote_sn_id,
                InterSnRequest::Heartbeat(SnOwnerHeartbeat { member_sn_id }),
            )
            .await?
        {
            InterSnResponse::Heartbeat(refreshed) => Ok(refreshed),
            InterSnResponse::Error(err) => Err(err.into()),
            _ => Err(p2p_err!(
                P2pErrorCode::InvalidData,
                "unexpected inter-sn heartbeat response"
            )),
        }
    }

    async fn request_owner_vote_to_sn(
        &self,
        remote_sn_id: &P2pId,
        request: OwnerVoteRequest,
    ) -> P2pResult<OwnerVoteResponse> {
        match self
            .request(remote_sn_id, InterSnRequest::OwnerVote(request))
            .await?
        {
            InterSnResponse::OwnerVote(response) => Ok(response),
            InterSnResponse::Error(err) => Err(err.into()),
            _ => Err(p2p_err!(
                P2pErrorCode::InvalidData,
                "unexpected owner vote response"
            )),
        }
    }

    async fn send_owner_control_heartbeat_to_sn(
        &self,
        remote_sn_id: &P2pId,
        request: OwnerHeartbeatRequest,
    ) -> P2pResult<OwnerHeartbeatResponse> {
        match self
            .request(remote_sn_id, InterSnRequest::OwnerControlHeartbeat(request))
            .await?
        {
            InterSnResponse::OwnerControlHeartbeat(response) => Ok(response),
            InterSnResponse::Error(err) => Err(err.into()),
            _ => Err(p2p_err!(
                P2pErrorCode::InvalidData,
                "unexpected owner control heartbeat response"
            )),
        }
    }

    async fn replicate_owner_session_to_sn(
        &self,
        remote_sn_id: &P2pId,
        request: OwnerSessionReplication,
    ) -> P2pResult<OwnerSessionReplicationResponse> {
        match self
            .request(
                remote_sn_id,
                InterSnRequest::OwnerSessionReplication(request),
            )
            .await?
        {
            InterSnResponse::OwnerSessionReplication(response) => Ok(response),
            InterSnResponse::Error(err) => Err(err.into()),
            _ => Err(p2p_err!(
                P2pErrorCode::InvalidData,
                "unexpected owner session replication response"
            )),
        }
    }

    async fn forward_owner_session_to_sn(
        &self,
        remote_sn_id: &P2pId,
        request: OwnerSessionForward,
    ) -> P2pResult<OwnerSessionForwardResponse> {
        match self
            .request(remote_sn_id, InterSnRequest::OwnerSessionForward(request))
            .await?
        {
            InterSnResponse::OwnerSessionForward(response) => Ok(response),
            InterSnResponse::Error(err) => Err(err.into()),
            _ => Err(p2p_err!(
                P2pErrorCode::InvalidData,
                "unexpected owner session forward response"
            )),
        }
    }

    pub async fn publish_lease_to_sn(
        &self,
        remote_sn_id: &P2pId,
        lease: ServingLease,
    ) -> P2pResult<bool> {
        match self
            .request(remote_sn_id, InterSnRequest::PublishLease(lease.into()))
            .await?
        {
            InterSnResponse::Published(written) => Ok(written),
            InterSnResponse::Error(err) => Err(err.into()),
            _ => Err(p2p_err!(
                P2pErrorCode::InvalidData,
                "unexpected inter-sn publish response"
            )),
        }
    }

    pub async fn query_leases_from_sn(
        &self,
        remote_sn_id: &P2pId,
        peer_id: P2pId,
    ) -> P2pResult<Vec<ServingLease>> {
        match self
            .request(
                remote_sn_id,
                InterSnRequest::QueryLease(SnQueryLease { peer_id }),
            )
            .await?
        {
            InterSnResponse::Leases(resp) => {
                Ok(resp.leases.into_iter().map(ServingLease::from).collect())
            }
            InterSnResponse::Error(err) => Err(err.into()),
            _ => Err(p2p_err!(
                P2pErrorCode::InvalidData,
                "unexpected inter-sn query lease response"
            )),
        }
    }

    pub async fn query_detail_from_sn(
        &self,
        remote_sn_id: &P2pId,
        peer_id: P2pId,
    ) -> P2pResult<Option<ServingPeerDetail>> {
        match self
            .request(
                remote_sn_id,
                InterSnRequest::QueryDetail(SnDetailQuery { peer_id }),
            )
            .await?
        {
            InterSnResponse::Detail(resp) => {
                Ok(resp.peer_info.map(|peer_info| ServingPeerDetail {
                    peer_info,
                    endpoints: resp.end_point_array,
                }))
            }
            InterSnResponse::Error(err) => Err(err.into()),
            _ => Err(p2p_err!(
                P2pErrorCode::InvalidData,
                "unexpected inter-sn detail response"
            )),
        }
    }

    pub async fn relay_call_to_sn(
        &self,
        remote_sn_id: &P2pId,
        call_req: SnCall,
    ) -> P2pResult<RelayCallOutcome> {
        match self
            .request(
                remote_sn_id,
                InterSnRequest::RelayCall(SnRelayCall { call: call_req }),
            )
            .await?
        {
            InterSnResponse::Relay(resp) => Ok(RelayCallOutcome {
                accepted: resp.accepted,
                to_peer_info: resp.to_peer_info,
            }),
            InterSnResponse::Error(err) => Err(err.into()),
            _ => Err(p2p_err!(
                P2pErrorCode::InvalidData,
                "unexpected inter-sn relay response"
            )),
        }
    }

    async fn request(
        &self,
        remote_sn_id: &P2pId,
        request: InterSnRequest,
    ) -> P2pResult<InterSnResponse> {
        self.target(remote_sn_id)?;
        let command = inter_sn_command_code(&request);
        let request = request
            .to_vec()
            .map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
        let response = self
            .cmd_node_service
            .send_with_resp(
                &PeerId::from(remote_sn_id.as_slice()),
                command,
                INTER_SN_CMD_VERSION,
                request.as_slice(),
                INTER_SN_CMD_TIMEOUT,
            )
            .await
            .map_err(cmd_to_p2p_err)?
            .into_bytes()
            .await
            .map_err(cmd_to_p2p_err)?;
        InterSnResponse::clone_from_slice(response.as_slice())
            .map_err(into_p2p_err!(P2pErrorCode::RawCodecError))
    }
}

#[async_trait]
impl OwnerPeerControlClient for TtpInterSnClient {
    async fn request_vote(
        &self,
        remote_owner_id: &P2pId,
        request: OwnerVoteRequest,
    ) -> P2pResult<OwnerVoteResponse> {
        self.request_owner_vote_to_sn(remote_owner_id, request)
            .await
    }

    async fn send_heartbeat(
        &self,
        remote_owner_id: &P2pId,
        request: OwnerHeartbeatRequest,
    ) -> P2pResult<OwnerHeartbeatResponse> {
        self.send_owner_control_heartbeat_to_sn(remote_owner_id, request)
            .await
    }

    async fn replicate_session(
        &self,
        remote_owner_id: &P2pId,
        request: OwnerSessionReplication,
    ) -> P2pResult<OwnerSessionReplicationResponse> {
        self.replicate_owner_session_to_sn(remote_owner_id, request)
            .await
    }

    async fn forward_session(
        &self,
        remote_owner_id: &P2pId,
        request: OwnerSessionForward,
    ) -> P2pResult<OwnerSessionForwardResponse> {
        self.forward_owner_session_to_sn(remote_owner_id, request)
            .await
    }
}

fn inter_sn_command_code(request: &InterSnRequest) -> InterSnCommandCode {
    match request {
        InterSnRequest::Heartbeat(_) => InterSnCommandCode::Heartbeat,
        InterSnRequest::OwnerVote(_) => InterSnCommandCode::Heartbeat,
        InterSnRequest::OwnerControlHeartbeat(_) => InterSnCommandCode::Heartbeat,
        InterSnRequest::OwnerSessionReplication(_) => InterSnCommandCode::Heartbeat,
        InterSnRequest::OwnerSessionForward(_) => InterSnCommandCode::Heartbeat,
        InterSnRequest::PublishLease(_) => InterSnCommandCode::PublishLease,
        InterSnRequest::QueryLease(_) => InterSnCommandCode::QueryLease,
        InterSnRequest::QueryDetail(_) => InterSnCommandCode::QueryDetail,
        InterSnRequest::RelayCall(_) => InterSnCommandCode::RelayCall,
    }
}

fn owner_targets(membership: &OwnerMembership) -> HashMap<P2pId, TtpTarget> {
    let mut targets = HashMap::new();
    for member in membership.members() {
        if let Some(endpoint) = member.endpoints.first().copied() {
            targets.insert(
                member.sn_id.clone(),
                TtpTarget {
                    local_ep: None,
                    remote_ep: endpoint,
                    remote_id: member.sn_id.clone(),
                    remote_name: member.name.clone(),
                },
            );
        }
    }
    targets
}

#[derive(Clone)]
pub struct OwnerTunnelFactory {
    ttp_node: TtpNodeRef,
    targets: Arc<HashMap<P2pId, TtpTarget>>,
}

impl OwnerTunnelFactory {
    fn new(ttp_node: TtpNodeRef, targets: HashMap<P2pId, TtpTarget>) -> Self {
        Self {
            ttp_node,
            targets: Arc::new(targets),
        }
    }
}

#[async_trait]
impl sfo_cmd_server::CmdNodeTunnelFactory<(), SnTunnelRead, SnTunnelWrite> for OwnerTunnelFactory {
    async fn create_tunnel(
        &self,
        remote_id: &PeerId,
    ) -> CmdResult<CmdTunnel<SnTunnelRead, SnTunnelWrite>> {
        let remote_sn_id = P2pId::from(remote_id.as_slice());
        let target = self.targets.get(&remote_sn_id).ok_or_else(|| {
            cmd_err!(
                CmdErrorCode::Failed,
                "owner target endpoint missing for {}",
                remote_sn_id
            )
        })?;
        let accepted = self
            .ttp_node
            .open_control_stream(target, inter_sn_purpose().map_err(p2p_to_cmd_err)?)
            .await
            .map_err(p2p_to_cmd_err)?;
        Ok(into_owner_cmd_tunnel(accepted))
    }
}

async fn listen_owner_cmd_tunnels(
    ttp_node: TtpNodeRef,
    cmd_node_service: Arc<OwnerCmdNodeService>,
) -> P2pResult<()> {
    ttp_node
        .listen_control_stream(
            inter_sn_purpose()?,
            Arc::new(move |accepted| {
                let cmd_node_service = cmd_node_service.clone();
                Box::pin(async move {
                    let accepted = match accepted {
                        Ok(accepted) => accepted,
                        Err(err) => {
                            log::warn!("owner peer accept stopped: {:?}", err);
                            return;
                        }
                    };
                    let tunnel = into_owner_cmd_tunnel(accepted);
                    if let Err(err) = cmd_node_service.serve_tunnel(tunnel).await {
                        log::warn!("owner peer command tunnel failed: {:?}", err);
                    }
                })
            }),
        )
        .await
}

fn into_owner_cmd_tunnel(
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

fn register_owner_cmd_handlers(
    cmd_node_service: &Arc<OwnerCmdNodeService>,
    peer: Arc<dyn InterSnPeer>,
) {
    for command in [
        InterSnCommandCode::Heartbeat,
        InterSnCommandCode::PublishLease,
        InterSnCommandCode::QueryLease,
        InterSnCommandCode::QueryDetail,
        InterSnCommandCode::RelayCall,
    ] {
        let peer = peer.clone();
        cmd_node_service.register_cmd_handler(
            command,
            move |_local_id: PeerId,
                  remote_id: PeerId,
                  _tunnel_id,
                  header: CmdHeader<OwnerCmdPkgLen, InterSnCommandCode>,
                  mut body: CmdBody| {
                let peer = peer.clone();
                async move {
                    handle_owner_cmd(peer, P2pId::from(remote_id.as_slice()), header, &mut body)
                        .await
                }
            },
        );
    }
}

async fn handle_owner_cmd(
    peer: Arc<dyn InterSnPeer>,
    remote_sn_id: P2pId,
    header: CmdHeader<OwnerCmdPkgLen, InterSnCommandCode>,
    body: &mut CmdBody,
) -> CmdResult<Option<CmdBody>> {
    let request = InterSnRequest::clone_from_slice(
        body.read_all()
            .await
            .map_err(into_cmd_err!(CmdErrorCode::Failed, "read owner cmd failed"))?
            .as_slice(),
    )
    .map_err(into_cmd_err!(
        CmdErrorCode::Failed,
        "decode owner cmd failed"
    ))?;
    let response = dispatch_owner_cmd(peer, remote_sn_id, header.cmd_code(), request).await;
    let response = response.to_vec().map_err(into_cmd_err!(
        CmdErrorCode::Failed,
        "encode owner cmd response failed"
    ))?;
    Ok(Some(CmdBody::from_bytes(response)))
}

async fn dispatch_owner_cmd(
    peer: Arc<dyn InterSnPeer>,
    remote_sn_id: P2pId,
    command: InterSnCommandCode,
    request: InterSnRequest,
) -> InterSnResponse {
    let result = match (command, request) {
        (InterSnCommandCode::Heartbeat, InterSnRequest::Heartbeat(heartbeat)) => peer
            .heartbeat_from_sn(remote_sn_id, heartbeat.member_sn_id)
            .await
            .map(InterSnResponse::Heartbeat),
        (InterSnCommandCode::Heartbeat, InterSnRequest::OwnerVote(request)) => peer
            .owner_vote_from_sn(remote_sn_id, request)
            .await
            .map(InterSnResponse::OwnerVote),
        (InterSnCommandCode::Heartbeat, InterSnRequest::OwnerControlHeartbeat(request)) => peer
            .owner_control_heartbeat_from_sn(remote_sn_id, request)
            .await
            .map(InterSnResponse::OwnerControlHeartbeat),
        (InterSnCommandCode::Heartbeat, InterSnRequest::OwnerSessionReplication(request)) => peer
            .owner_session_replication_from_sn(remote_sn_id, request)
            .await
            .map(InterSnResponse::OwnerSessionReplication),
        (InterSnCommandCode::Heartbeat, InterSnRequest::OwnerSessionForward(request)) => peer
            .owner_session_forward_from_sn(remote_sn_id, request)
            .await
            .map(InterSnResponse::OwnerSessionForward),
        (InterSnCommandCode::PublishLease, InterSnRequest::PublishLease(lease)) => peer
            .publish_lease_from_sn(remote_sn_id, lease.into())
            .await
            .map(InterSnResponse::Published),
        (InterSnCommandCode::QueryLease, InterSnRequest::QueryLease(query)) => peer
            .query_leases_from_sn(remote_sn_id, query.peer_id)
            .await
            .map(|leases| {
                InterSnResponse::Leases(SnQueryLeaseResp {
                    leases: leases.into_iter().map(SnPublishLease::from).collect(),
                })
            }),
        (InterSnCommandCode::QueryDetail, InterSnRequest::QueryDetail(query)) => peer
            .query_detail_from_sn(remote_sn_id, query.peer_id)
            .await
            .map(|detail| {
                let (peer_info, end_point_array) = detail
                    .map(|detail| (Some(detail.peer_info), detail.endpoints))
                    .unwrap_or((None, Vec::new()));
                InterSnResponse::Detail(SnDetailResp {
                    peer_info,
                    end_point_array,
                })
            }),
        (InterSnCommandCode::RelayCall, InterSnRequest::RelayCall(relay)) => peer
            .relay_call_from_sn(remote_sn_id, relay.call)
            .await
            .map(|outcome| InterSnResponse::Relay(outcome.into())),
        _ => Err(p2p_err!(
            P2pErrorCode::InvalidData,
            "inter-sn command code and payload mismatch"
        )),
    };

    result.unwrap_or_else(|err| InterSnResponse::Error(err.into()))
}

fn p2p_to_cmd_err(err: P2pError) -> sfo_cmd_server::errors::CmdError {
    cmd_err!(CmdErrorCode::Failed, "{:?}", err)
}

fn cmd_to_p2p_err(err: sfo_cmd_server::errors::CmdError) -> P2pError {
    P2pError::new(P2pErrorCode::Failed, err.msg().to_owned())
}

impl From<ServingLease> for SnPublishLease {
    fn from(lease: ServingLease) -> Self {
        Self {
            peer_id: lease.peer_id,
            serving_sn_id: lease.serving_sn_id,
            sequence: lease.sequence,
            expires_at: lease.expires_at,
        }
    }
}

impl From<SnPublishLease> for ServingLease {
    fn from(lease: SnPublishLease) -> Self {
        Self {
            peer_id: lease.peer_id,
            serving_sn_id: lease.serving_sn_id,
            sequence: lease.sequence,
            expires_at: lease.expires_at,
        }
    }
}

impl From<RelayCallOutcome> for RelayCallResponse {
    fn from(outcome: RelayCallOutcome) -> Self {
        Self {
            accepted: outcome.accepted,
            to_peer_info: outcome.to_peer_info,
        }
    }
}

impl From<crate::error::P2pError> for InterSnError {
    fn from(err: crate::error::P2pError) -> Self {
        Self {
            code: err.code(),
            message: err.msg().to_owned(),
        }
    }
}

impl From<InterSnError> for crate::error::P2pError {
    fn from(err: InterSnError) -> Self {
        crate::error::P2pError::new(err.code, err.message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_id(seed: u8) -> P2pId {
        P2pId::from(vec![seed; 32])
    }

    struct TestInterSnPeer {
        local_sn_id: P2pId,
        heartbeats: Mutex<Vec<(P2pId, P2pId)>>,
        published: Mutex<Vec<(P2pId, ServingLease)>>,
        owner_events: Mutex<Vec<&'static str>>,
    }

    impl TestInterSnPeer {
        fn new(local_sn_id: P2pId) -> Arc<Self> {
            Arc::new(Self {
                local_sn_id,
                heartbeats: Mutex::new(Vec::new()),
                published: Mutex::new(Vec::new()),
                owner_events: Mutex::new(Vec::new()),
            })
        }
    }

    #[async_trait]
    impl InterSnPeer for TestInterSnPeer {
        fn sn_id(&self) -> Option<P2pId> {
            Some(self.local_sn_id.clone())
        }

        async fn publish_lease_from_sn(
            &self,
            remote_sn_id: P2pId,
            lease: ServingLease,
        ) -> P2pResult<bool> {
            self.published.lock().unwrap().push((remote_sn_id, lease));
            Ok(true)
        }

        async fn heartbeat_from_sn(
            &self,
            remote_sn_id: P2pId,
            member_sn_id: P2pId,
        ) -> P2pResult<bool> {
            self.heartbeats
                .lock()
                .unwrap()
                .push((remote_sn_id, member_sn_id));
            Ok(true)
        }

        async fn owner_vote_from_sn(
            &self,
            _remote_sn_id: P2pId,
            request: OwnerVoteRequest,
        ) -> P2pResult<OwnerVoteResponse> {
            self.owner_events.lock().unwrap().push("vote");
            Ok(OwnerVoteResponse {
                term: request.term,
                vote_granted: true,
                leader_id: None,
            })
        }

        async fn owner_control_heartbeat_from_sn(
            &self,
            _remote_sn_id: P2pId,
            request: OwnerHeartbeatRequest,
        ) -> P2pResult<OwnerHeartbeatResponse> {
            self.owner_events.lock().unwrap().push("control_heartbeat");
            Ok(OwnerHeartbeatResponse {
                term: request.term,
                accepted: true,
                leader_id: Some(request.leader_id),
            })
        }

        async fn owner_session_replication_from_sn(
            &self,
            _remote_sn_id: P2pId,
            request: OwnerSessionReplication,
        ) -> P2pResult<OwnerSessionReplicationResponse> {
            self.owner_events.lock().unwrap().push("replicate");
            Ok(OwnerSessionReplicationResponse {
                term: request.term,
                accepted: true,
                committed_index: 1,
                leader_id: Some(request.leader_id),
            })
        }

        async fn owner_session_forward_from_sn(
            &self,
            _remote_sn_id: P2pId,
            _request: OwnerSessionForward,
        ) -> P2pResult<OwnerSessionForwardResponse> {
            self.owner_events.lock().unwrap().push("forward");
            Ok(OwnerSessionForwardResponse {
                term: 1,
                accepted: true,
                committed_index: 1,
                leader_id: Some(self.local_sn_id.clone()),
            })
        }
    }

    #[test]
    fn sn_distributed_directory_maps_requests_to_owner_cmd_codes() {
        assert_eq!(
            inter_sn_command_code(&InterSnRequest::Heartbeat(SnOwnerHeartbeat {
                member_sn_id: test_id(19)
            })),
            InterSnCommandCode::Heartbeat
        );
        assert_eq!(
            inter_sn_command_code(&InterSnRequest::PublishLease(
                ServingLease {
                    peer_id: test_id(20),
                    serving_sn_id: test_id(21),
                    sequence: 9,
                    expires_at: 99,
                }
                .into(),
            )),
            InterSnCommandCode::PublishLease
        );
        assert_eq!(
            inter_sn_command_code(&InterSnRequest::QueryLease(SnQueryLease {
                peer_id: test_id(20)
            })),
            InterSnCommandCode::QueryLease
        );
        assert_eq!(
            inter_sn_command_code(&InterSnRequest::QueryDetail(SnDetailQuery {
                peer_id: test_id(20)
            })),
            InterSnCommandCode::QueryDetail
        );
        assert_eq!(
            inter_sn_command_code(&InterSnRequest::OwnerVote(OwnerVoteRequest {
                term: 1,
                candidate_id: test_id(22),
            })),
            InterSnCommandCode::Heartbeat
        );
    }

    #[tokio::test]
    async fn sn_distributed_directory_owner_cmd_dispatches_heartbeat() {
        let local_peer = TestInterSnPeer::new(test_id(10));
        let remote_sn_id = test_id(11);

        let response = dispatch_owner_cmd(
            local_peer.clone(),
            remote_sn_id.clone(),
            InterSnCommandCode::Heartbeat,
            InterSnRequest::Heartbeat(SnOwnerHeartbeat {
                member_sn_id: remote_sn_id.clone(),
            }),
        )
        .await;

        match response {
            InterSnResponse::Heartbeat(true) => {}
            other => panic!("unexpected response: {:?}", other),
        }
        assert_eq!(
            local_peer.heartbeats.lock().unwrap().as_slice(),
            &[(remote_sn_id.clone(), remote_sn_id)]
        );
    }

    #[tokio::test]
    async fn sn_distributed_directory_owner_cmd_dispatches_publish_lease() {
        let local_peer = TestInterSnPeer::new(test_id(10));
        let remote_sn_id = test_id(11);
        let lease = ServingLease {
            peer_id: test_id(20),
            serving_sn_id: test_id(21),
            sequence: 9,
            expires_at: 99,
        };

        let response = dispatch_owner_cmd(
            local_peer.clone(),
            remote_sn_id.clone(),
            InterSnCommandCode::PublishLease,
            InterSnRequest::PublishLease(lease.clone().into()),
        )
        .await;

        match response {
            InterSnResponse::Published(true) => {}
            other => panic!("unexpected response: {:?}", other),
        }
        assert_eq!(
            local_peer.published.lock().unwrap().as_slice(),
            &[(remote_sn_id, lease)]
        );
    }

    #[tokio::test]
    async fn sn_owner_network_ttp_command_dispatches_election_payloads() {
        let local_peer = TestInterSnPeer::new(test_id(10));
        let remote_sn_id = test_id(11);
        let leader_id = test_id(12);
        let serving_id = test_id(13);

        let vote = dispatch_owner_cmd(
            local_peer.clone(),
            remote_sn_id.clone(),
            InterSnCommandCode::Heartbeat,
            InterSnRequest::OwnerVote(OwnerVoteRequest {
                term: 3,
                candidate_id: remote_sn_id.clone(),
            }),
        )
        .await;
        assert!(matches!(
            vote,
            InterSnResponse::OwnerVote(OwnerVoteResponse {
                vote_granted: true,
                ..
            })
        ));

        let heartbeat = dispatch_owner_cmd(
            local_peer.clone(),
            remote_sn_id.clone(),
            InterSnCommandCode::Heartbeat,
            InterSnRequest::OwnerControlHeartbeat(OwnerHeartbeatRequest {
                term: 3,
                leader_id: leader_id.clone(),
                committed_index: 0,
            }),
        )
        .await;
        assert!(matches!(
            heartbeat,
            InterSnResponse::OwnerControlHeartbeat(OwnerHeartbeatResponse { accepted: true, .. })
        ));

        let entry = crate::sn::directory::OwnerSessionEntry::RenewServingSession(
            crate::sn::directory::ServingSnSession::online(
                serving_id.clone(),
                Duration::from_secs(30),
                1_000_000,
            ),
        );
        let replicated = dispatch_owner_cmd(
            local_peer.clone(),
            remote_sn_id.clone(),
            InterSnCommandCode::Heartbeat,
            InterSnRequest::OwnerSessionReplication(OwnerSessionReplication {
                term: 3,
                leader_id,
                entry: entry.clone(),
                now: 1_000_000,
                committed: true,
            }),
        )
        .await;
        assert!(matches!(
            replicated,
            InterSnResponse::OwnerSessionReplication(OwnerSessionReplicationResponse {
                accepted: true,
                ..
            })
        ));

        let forwarded = dispatch_owner_cmd(
            local_peer.clone(),
            remote_sn_id,
            InterSnCommandCode::Heartbeat,
            InterSnRequest::OwnerSessionForward(OwnerSessionForward {
                entry,
                now: 1_000_001,
            }),
        )
        .await;
        assert!(matches!(
            forwarded,
            InterSnResponse::OwnerSessionForward(OwnerSessionForwardResponse {
                accepted: true,
                ..
            })
        ));

        assert_eq!(
            local_peer.owner_events.lock().unwrap().as_slice(),
            &["vote", "control_heartbeat", "replicate", "forward"]
        );
    }
}

impl InterSnRegistry {
    pub fn global() -> &'static Self {
        static REGISTRY: Lazy<InterSnRegistry> = Lazy::new(InterSnRegistry::default);
        &REGISTRY
    }

    pub fn register(&self, peer: Arc<dyn InterSnPeer>) {
        if let Some(sn_id) = peer.sn_id() {
            self.peers
                .lock()
                .unwrap()
                .insert(sn_id, Arc::downgrade(&peer));
        }
    }

    pub fn get(&self, sn_id: &P2pId) -> Option<Arc<dyn InterSnPeer>> {
        let mut peers = self.peers.lock().unwrap();
        let Some(peer) = peers.get(sn_id).and_then(Weak::upgrade) else {
            peers.remove(sn_id);
            return None;
        };
        Some(peer)
    }
}
