use super::{
    call_stub::CallStub,
    nat_probe_scheduler::{NatProbeAuthorityRemovalReason, NatProbeScheduler},
    peer_manager::PeerManager,
    rendezvous_state::{RendezvousBegin, RendezvousState},
};
use crate::endpoint::{
    Endpoint, EndpointArea, Protocol, endpoints_to_string, is_non_lan_ipv4_addr,
};
use crate::error::{P2pErrorCode, P2pResult, into_p2p_err, p2p_err};
use crate::executor::Executor;
use crate::finder::{DeviceCache, DeviceCacheConfig};
use crate::networks::{
    NetManager, NetManagerRef, QuicCongestionAlgorithm, QuicTunnelNetwork, TcpTunnelNetwork,
    TunnelNetwork, TunnelNetworkRef, TunnelStreamRead, TunnelStreamWrite, ValidateResult,
};
use crate::p2p_identity::{
    EncodedP2pIdentityCert, P2pId, P2pIdentityCertFactoryRef, P2pIdentityFactoryRef, P2pIdentityRef,
};
use crate::runtime;
use crate::sn::directory::{
    OwnerDirectoryClientRef, OwnerMembership, ServingLease, StaticOwnerDirectoryClient,
    noop_owner_directory_client,
};
use crate::sn::inter_sn::{
    InterSnCommand, InterSnCommandContext, InterSnConnectionContext, InterSnPeer, RelayCallOutcome,
    ServingPeerDetail, SnInterServiceValidatorRef, TtpInterSnClient, TtpInterSnClientRef,
    allow_all_sn_inter_service_validator, require_accept,
};
use crate::sn::nat_probe::{
    MAX_NAT_PROBE_ENDPOINTS, NatProbeReflector, NatProbeSigningContext,
};
use crate::sn::protocol::{v0::*, *};
use crate::sn::service::peer_manager::PeerManagerRef;
use crate::sn::types::{
    CmdTunnelId, SnCmdHeader, SnCmdPkgLen, SnTunnelRead, SnTunnelWrite, sn_cmd_purpose,
};
use crate::tls::{DefaultTlsServerCertResolver, TlsServerCertResolver, init_tls};
use crate::ttp::{TtpClient, TtpConnector, TtpNode, TtpPortListener, TtpServer, TtpServerRef};
use crate::types::{SequenceGenerator, Timestamp, TunnelId};
use bucky_raw_codec::{RawConvertTo, RawFrom};
use bucky_time::bucky_time_now;
use log::*;
use sfo_cmd_server::errors::{CmdErrorCode, CmdResult, cmd_err, into_cmd_err};
use sfo_cmd_server::server::{
    CmdServer, CmdServerEventListener, CmdTunnelService, DefaultCmdServerService,
};
use sfo_cmd_server::{CmdBody, CmdTunnel, PeerId};
use sfo_reuseport::ServerRuntime;
use std::{
    collections::HashSet,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    sync::{
        Arc, Mutex,
        atomic::{self, AtomicBool},
    },
    time::Duration,
};

// const TRACKER_INTERVAL: Duration = Duration::from_secs(60);
// struct CallTracker {
//     calls: HashMap<TempSeq, (u64, Instant, DeviceId)>, // <called_seq, (call_send_time, called_send_time)>
//     begin_time: Instant,
// }

type SnCmdService = DefaultCmdServerService<(), SnTunnelRead, SnTunnelWrite, SnCmdPkgLen, u8>;
pub type SnCmdServiceRef = Arc<SnCmdService>;
pub type SnServiceRef = Arc<SnService>;
pub type SnServerRef = Arc<SnServer>;

const NAT_PROBE_MAINTENANCE_INTERVAL: Duration = Duration::from_millis(250);
const MAX_REPORTED_LOCAL_ENDPOINTS: usize = 32;

#[derive(Clone, Debug)]
pub struct SnConnectionValidateContext {
    pub client_id: P2pId,
    pub client_cert: EncodedP2pIdentityCert,
}

#[async_trait::async_trait]
pub trait SnConnectionValidator: Send + Sync + 'static {
    async fn validate(&self, ctx: &SnConnectionValidateContext) -> P2pResult<ValidateResult>;
}

pub type SnConnectionValidatorRef = Arc<dyn SnConnectionValidator>;

pub struct AllowAllSnConnectionValidator;

#[async_trait::async_trait]
impl SnConnectionValidator for AllowAllSnConnectionValidator {
    async fn validate(&self, _ctx: &SnConnectionValidateContext) -> P2pResult<ValidateResult> {
        Ok(ValidateResult::Accept)
    }
}

pub fn allow_all_sn_connection_validator() -> SnConnectionValidatorRef {
    Arc::new(AllowAllSnConnectionValidator)
}

#[async_trait::async_trait]
trait SnInterClient: Send + Sync + 'static {
    async fn query_detail_from_sn(
        &self,
        remote_sn_id: &P2pId,
        peer_id: P2pId,
    ) -> P2pResult<Option<ServingPeerDetail>>;

    async fn relay_call_to_sn(
        &self,
        remote_sn_id: &P2pId,
        call_req: SnCall,
    ) -> P2pResult<RelayCallOutcome>;

    async fn relay_rendezvous_to_sn(
        &self,
        _remote_sn_id: &P2pId,
        _target_peer_id: P2pId,
        _notify: SnTunnelRendezvousNotify,
    ) -> P2pResult<SnTunnelRendezvousResp> {
        Err(p2p_err!(
            P2pErrorCode::NotSupport,
            "inter-SN rendezvous is not supported"
        ))
    }
}

type SnInterClientRef = Arc<dyn SnInterClient>;

struct RemotePeerDetails {
    details: Vec<ServingPeerDetail>,
    target_protocol_version: Option<u8>,
}

struct RendezvousLeaderGuard<'a> {
    state: &'a Mutex<RendezvousState>,
    authenticated_initiator: P2pId,
    request: SnTunnelRendezvous,
    armed: bool,
}

impl<'a> RendezvousLeaderGuard<'a> {
    fn new(
        state: &'a Mutex<RendezvousState>,
        authenticated_initiator: &P2pId,
        request: &SnTunnelRendezvous,
    ) -> Self {
        Self {
            state,
            authenticated_initiator: authenticated_initiator.clone(),
            request: request.clone(),
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for RendezvousLeaderGuard<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.state
                .lock()
                .unwrap()
                .fail_unanswered(&self.authenticated_initiator, &self.request);
        }
    }
}

#[async_trait::async_trait]
impl SnInterClient for TtpInterSnClient {
    async fn query_detail_from_sn(
        &self,
        remote_sn_id: &P2pId,
        peer_id: P2pId,
    ) -> P2pResult<Option<ServingPeerDetail>> {
        TtpInterSnClient::query_detail_from_sn(self, remote_sn_id, peer_id).await
    }

    async fn relay_call_to_sn(
        &self,
        remote_sn_id: &P2pId,
        call_req: SnCall,
    ) -> P2pResult<RelayCallOutcome> {
        TtpInterSnClient::relay_call_to_sn(self, remote_sn_id, call_req).await
    }

    async fn relay_rendezvous_to_sn(
        &self,
        remote_sn_id: &P2pId,
        target_peer_id: P2pId,
        notify: SnTunnelRendezvousNotify,
    ) -> P2pResult<SnTunnelRendezvousResp> {
        TtpInterSnClient::relay_rendezvous_to_sn(self, remote_sn_id, target_peer_id, notify).await
    }
}

pub struct SnService {
    seq_generator: Arc<SequenceGenerator>,
    peer_mgr: PeerManagerRef,
    call_stub: CallStub,
    cert_factory: P2pIdentityCertFactoryRef,
    cmd_server: SnCmdServiceRef,
    connection_validator: SnConnectionValidatorRef,
    inter_service_validator: SnInterServiceValidatorRef,
    inter_sn_client: Mutex<Option<SnInterClientRef>>,
    owner_client: OwnerDirectoryClientRef,
    local_sn_id: Option<P2pId>,
    local_identity: Mutex<Option<P2pIdentityRef>>,
    nat_probe_scheduler: Mutex<NatProbeScheduler>,
    rendezvous_state: Mutex<RendezvousState>,
    cmd_version: u8,
}

impl SnService {
    pub fn new(
        cert_factory: P2pIdentityCertFactoryRef,
        connection_validator: SnConnectionValidatorRef,
    ) -> SnServiceRef {
        Self::new_with_options(
            cert_factory,
            connection_validator,
            allow_all_sn_inter_service_validator(),
            noop_owner_directory_client(),
            None,
            None,
        )
    }

    pub fn new_with_options(
        cert_factory: P2pIdentityCertFactoryRef,
        connection_validator: SnConnectionValidatorRef,
        inter_service_validator: SnInterServiceValidatorRef,
        owner_client: OwnerDirectoryClientRef,
        inter_sn_client: Option<TtpInterSnClientRef>,
        local_sn_id: Option<P2pId>,
    ) -> SnServiceRef {
        Self::new_with_inter_sn_client(
            cert_factory,
            connection_validator,
            inter_service_validator,
            owner_client,
            inter_sn_client.map(|client| client as SnInterClientRef),
            local_sn_id,
        )
    }

    fn new_with_inter_sn_client(
        cert_factory: P2pIdentityCertFactoryRef,
        connection_validator: SnConnectionValidatorRef,
        inter_service_validator: SnInterServiceValidatorRef,
        owner_client: OwnerDirectoryClientRef,
        inter_sn_client: Option<SnInterClientRef>,
        local_sn_id: Option<P2pId>,
    ) -> SnServiceRef {
        let scheduler_sn_id = local_sn_id.clone().unwrap_or_default();
        let service = Arc::new(SnService {
            seq_generator: Arc::new(SequenceGenerator::new()),
            peer_mgr: PeerManager::new(),
            call_stub: CallStub::new(),
            cert_factory,
            cmd_server: DefaultCmdServerService::new(),
            connection_validator,
            inter_service_validator,
            owner_client,
            inter_sn_client: Mutex::new(inter_sn_client),
            local_sn_id,
            local_identity: Mutex::new(None),
            nat_probe_scheduler: Mutex::new(NatProbeScheduler::new(scheduler_sn_id)),
            rendezvous_state: Mutex::new(RendezvousState::new()),
            cmd_version: 0,
        });
        service.register_sn_cmd_handler();
        service
            .cmd_server
            .attach_event_listener(service.clone() as Arc<dyn CmdServerEventListener>);
        service
    }

    #[cfg(test)]
    fn new_with_test_inter_sn_client(
        cert_factory: P2pIdentityCertFactoryRef,
        connection_validator: SnConnectionValidatorRef,
        inter_service_validator: SnInterServiceValidatorRef,
        owner_client: OwnerDirectoryClientRef,
        inter_sn_client: SnInterClientRef,
        local_sn_id: P2pId,
    ) -> SnServiceRef {
        Self::new_with_inter_sn_client(
            cert_factory,
            connection_validator,
            inter_service_validator,
            owner_client,
            Some(inter_sn_client),
            Some(local_sn_id),
        )
    }

    fn set_local_identity(&self, local_identity: P2pIdentityRef) {
        *self.local_identity.lock().unwrap() = Some(local_identity);
    }

    pub fn get_cmd_server(&self) -> &SnCmdServiceRef {
        &self.cmd_server
    }

    pub fn set_inter_sn_client(&self, inter_sn_client: Option<TtpInterSnClientRef>) {
        *self.inter_sn_client.lock().unwrap() =
            inter_sn_client.map(|client| client as SnInterClientRef);
    }

    pub fn set_nat_probe_endpoints(&self, endpoints: Vec<Endpoint>) {
        let affected = self
            .nat_probe_scheduler
            .lock()
            .unwrap()
            .set_endpoints(endpoints);
        for peer_id in affected {
            self.peer_mgr.invalidate_net_profile(&peer_id);
        }
    }

    #[cfg(test)]
    pub(crate) fn force_nat_probe_period_due_for_test(
        &self,
        peer_id: &P2pId,
        now: Timestamp,
    ) -> bool {
        self.nat_probe_scheduler
            .lock()
            .unwrap()
            .force_periodic_due(peer_id, now)
    }

    fn inter_sn_client(&self) -> Option<SnInterClientRef> {
        self.inter_sn_client.lock().unwrap().clone()
    }

    fn register_sn_cmd_handler(self: &Arc<Self>) {
        let service = self.clone();
        self.cmd_server.register_cmd_handler(
            PackageCmdCode::SnCall as u8,
            move |local_id: PeerId,
                  peer_id: PeerId,
                  tunnel_id: CmdTunnelId,
                  _header: SnCmdHeader,
                  mut cmd_body: CmdBody| {
                let service = service.clone();
                async move {
                    let local_id = P2pId::from(local_id.as_slice());
                    let call_req = SnCall::clone_from_slice(cmd_body.read_all().await?.as_slice())
                        .map_err(into_cmd_err!(CmdErrorCode::RawCodecError))?;
                    let call_resp = service
                        .handle_call(call_req, &local_id, &peer_id, tunnel_id, bucky_time_now())
                        .await
                        .map_err(into_cmd_err!(CmdErrorCode::Failed, "handle sn call failed"))?;
                    Ok(Some(CmdBody::from(
                        call_resp
                            .to_vec()
                            .map_err(into_cmd_err!(CmdErrorCode::RawCodecError))?,
                    )))
                }
            },
        );

        let service = self.clone();
        self.cmd_server.register_cmd_handler(
            PackageCmdCode::SnCalledResp as u8,
            move |local_id: PeerId,
                  peer_id: PeerId,
                  tunnel_id: CmdTunnelId,
                  _header: SnCmdHeader,
                  mut cmd_body: CmdBody| {
                let service = service.clone();
                async move {
                    let local_id = P2pId::from(local_id.as_slice());
                    service
                        .observe_nat_probe_control(&local_id, &peer_id, tunnel_id)
                        .await;
                    let called_resp =
                        SnCalledResp::clone_from_slice(cmd_body.read_all().await?.as_slice())
                            .map_err(into_cmd_err!(CmdErrorCode::RawCodecError))?;
                    service.handle_called_resp(called_resp).await;
                    Ok(None)
                }
            },
        );

        let service = self.clone();
        self.cmd_server.register_cmd_handler(
            PackageCmdCode::ReportSn as u8,
            move |local_id: PeerId,
                  peer_id: PeerId,
                  tunnel_id: CmdTunnelId,
                  _header: SnCmdHeader,
                  mut cmd_body: CmdBody| {
                let service = service.clone();
                async move {
                    let local_id = P2pId::from(local_id.as_slice());
                    let report_sn =
                        ReportSn::clone_from_slice(cmd_body.read_all().await?.as_slice())
                            .map_err(into_cmd_err!(CmdErrorCode::RawCodecError))?;
                    let report_resp = service
                        .handle_report_sn(&local_id, &peer_id, tunnel_id, report_sn)
                        .await
                        .map_err(into_cmd_err!(
                            CmdErrorCode::Failed,
                            "handle report sn failed"
                        ))?;
                    Ok(Some(CmdBody::from(
                        report_resp
                            .to_vec()
                            .map_err(into_cmd_err!(CmdErrorCode::RawCodecError))?,
                    )))
                }
            },
        );

        let service = self.clone();
        self.cmd_server.register_cmd_handler(
            PackageCmdCode::SnQuery as u8,
            move |local_id: PeerId,
                  peer_id: PeerId,
                  tunnel_id: CmdTunnelId,
                  _header: SnCmdHeader,
                  mut cmd_body: CmdBody| {
                let service = service.clone();
                async move {
                    let local_id = P2pId::from(local_id.as_slice());
                    let query = SnQuery::clone_from_slice(cmd_body.read_all().await?.as_slice())
                        .map_err(into_cmd_err!(CmdErrorCode::RawCodecError))?;
                    let query_resp = service
                        .handle_query_sn(&local_id, &peer_id, tunnel_id, query)
                        .await
                        .map_err(into_cmd_err!(
                            CmdErrorCode::Failed,
                            "handle sn query failed"
                        ))?;
                    Ok(Some(CmdBody::from(
                        query_resp
                            .to_vec()
                            .map_err(into_cmd_err!(CmdErrorCode::RawCodecError))?,
                    )))
                }
            },
        );

        let service = self.clone();
        self.cmd_server.register_cmd_handler(
            PackageCmdCode::SnTunnelRendezvous as u8,
            move |local_id: PeerId,
                  peer_id: PeerId,
                  tunnel_id: CmdTunnelId,
                  header: SnCmdHeader,
                  mut cmd_body: CmdBody| {
                let service = service.clone();
                async move {
                    if header.version() != SN_TUNNEL_RENDEZVOUS_CMD_VERSION {
                        return Err(cmd_err!(
                            CmdErrorCode::InvalidParam,
                            "unsupported SN rendezvous command version: {}",
                            header.version()
                        ));
                    }
                    let local_id = P2pId::from(local_id.as_slice());
                    let request =
                        SnTunnelRendezvous::clone_from_slice(cmd_body.read_all().await?.as_slice())
                            .map_err(into_cmd_err!(CmdErrorCode::RawCodecError))?;
                    let response = service
                        .handle_rendezvous(request, &local_id, &peer_id, tunnel_id)
                        .await;
                    Ok(Some(CmdBody::from(
                        response
                            .to_vec()
                            .map_err(into_cmd_err!(CmdErrorCode::RawCodecError))?,
                    )))
                }
            },
        );
    }

    fn peer_manager(&self) -> &PeerManagerRef {
        &self.peer_mgr
    }

    fn effective_local_sn_id(&self, local_id: &P2pId) -> P2pId {
        self.local_sn_id.clone().unwrap_or_else(|| local_id.clone())
    }

    async fn validate_connection(&self, ctx: SnConnectionValidateContext) -> P2pResult<()> {
        match self.connection_validator.validate(&ctx).await? {
            ValidateResult::Accept => Ok(()),
            ValidateResult::Reject(reason) => Err(p2p_err!(
                P2pErrorCode::PermissionDenied,
                "sn connection validate failed client={} reason={}",
                ctx.client_id,
                reason
            )),
        }
    }

    async fn validate_inter_connection(&self, remote_sn_id: &P2pId) -> P2pResult<()> {
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

    async fn validate_inter_command(
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

    fn validate_context_from_cert(
        &self,
        peer_id: &PeerId,
        client_cert: EncodedP2pIdentityCert,
    ) -> P2pResult<SnConnectionValidateContext> {
        let client_id = P2pId::from(peer_id.as_slice());
        let parsed_cert = self.cert_factory.create(&client_cert)?;
        let cert_id = parsed_cert.get_id();
        if cert_id != client_id {
            return Err(p2p_err!(
                P2pErrorCode::PermissionDenied,
                "sn client cert id mismatch client={} cert={}",
                client_id,
                cert_id
            ));
        }
        Ok(SnConnectionValidateContext {
            client_id,
            client_cert,
        })
    }

    fn client_cert_from_request_or_cache(
        &self,
        client_id: &P2pId,
        request_cert: Option<EncodedP2pIdentityCert>,
    ) -> P2pResult<EncodedP2pIdentityCert> {
        if let Some(cert) = request_cert {
            return Ok(cert);
        }
        let cached = self.peer_manager().find_peer(client_id).ok_or_else(|| {
            p2p_err!(
                P2pErrorCode::PermissionDenied,
                "sn client cert missing client={}",
                client_id
            )
        })?;
        cached.desc.get_encoded_cert()
    }

    fn push_unique_endpoint(endpoints: &mut Vec<Endpoint>, endpoint: Endpoint) {
        if !endpoints.contains(&endpoint) {
            endpoints.push(endpoint);
        }
    }

    fn extend_unique_endpoints(endpoints: &mut Vec<Endpoint>, extras: &[Endpoint]) {
        for endpoint in extras {
            Self::push_unique_endpoint(endpoints, *endpoint);
        }
    }

    fn dedup_endpoints(endpoints: &mut Vec<Endpoint>) {
        let mut unique = Vec::with_capacity(endpoints.len());
        for endpoint in endpoints.drain(..) {
            Self::push_unique_endpoint(&mut unique, endpoint);
        }
        *endpoints = unique;
    }

    fn sanitize_reported_endpoints(
        endpoints: &[Endpoint],
        observed_tunnel: Option<&Endpoint>,
    ) -> P2pResult<Vec<Endpoint>> {
        if endpoints.len() > MAX_REPORTED_LOCAL_ENDPOINTS {
            return Err(p2p_err!(
                P2pErrorCode::OutOfLimit,
                "reported local endpoint count exceeds {}",
                MAX_REPORTED_LOCAL_ENDPOINTS
            ));
        }

        let observed_ip = observed_tunnel.map(|endpoint| endpoint.addr().ip());
        let mut sanitized = Vec::with_capacity(endpoints.len());
        for endpoint in endpoints.iter().copied() {
            if !matches!(endpoint.protocol(), Protocol::Tcp | Protocol::Quic)
                || endpoint.addr().port() == 0
            {
                continue;
            }

            let area = match endpoint.addr() {
                SocketAddr::V4(addr)
                    if addr.ip().is_private() || addr.ip().is_link_local() =>
                {
                    EndpointArea::Lan
                }
                SocketAddr::V6(addr)
                    if addr.ip().is_unique_local() || addr.ip().is_unicast_link_local() =>
                {
                    EndpointArea::Lan
                }
                addr if is_non_lan_ipv4_addr(addr)
                    && observed_ip == Some(endpoint.addr().ip()) =>
                {
                    EndpointArea::Wan
                }
                _ => continue,
            };

            let mut endpoint = endpoint;
            endpoint.set_area(area);
            Self::push_unique_endpoint(&mut sanitized, endpoint);
        }
        Ok(sanitized)
    }

    fn classify_observed_endpoint(mut endpoint: Endpoint, reported_eps: &[Endpoint]) -> Endpoint {
        if reported_eps.iter().any(|reported| {
            reported.protocol() == endpoint.protocol() && reported.addr() == endpoint.addr()
        }) {
            endpoint.set_area(EndpointArea::Wan);
        } else {
            endpoint.set_area(EndpointArea::ServerReflexive);
        }
        endpoint
    }

    fn mapped_endpoint_from_observed(ep: &Endpoint, protocol: Protocol, port: u16) -> Endpoint {
        let mut map_ep = Endpoint::from((protocol, ep.addr().ip(), port));
        map_ep.set_area(EndpointArea::Mapped);
        map_ep
    }

    fn is_direct_observed_candidate(endpoint: &Endpoint) -> bool {
        endpoint.protocol() != Protocol::Tcp
    }

    fn observed_endpoint_candidates(
        observed_ep: &[Endpoint],
        map_ports: &[(Protocol, u16)],
        reported_eps: &[Endpoint],
    ) -> Vec<Endpoint> {
        let mut remote_ep = observed_ep
            .iter()
            .copied()
            .filter(Self::is_direct_observed_candidate)
            .map(|remote| Self::classify_observed_endpoint(remote, reported_eps))
            .collect::<Vec<_>>();
        let mut map_eps = Vec::new();
        for ep in observed_ep.iter() {
            for (protocol, port) in map_ports.iter() {
                let map_ep = Self::mapped_endpoint_from_observed(ep, *protocol, *port);
                if remote_ep.contains(&map_ep) || map_eps.contains(&map_ep) {
                    continue;
                }
                map_eps.push(map_ep);
            }
        }
        Self::extend_unique_endpoints(&mut remote_ep, map_eps.as_slice());
        remote_ep
    }

    fn reported_endpoints_for_peer(
        peer: &crate::sn::service::peer_manager::CachedPeerInfo,
    ) -> Vec<Endpoint> {
        let mut endpoints = peer.desc.endpoints();
        Self::extend_unique_endpoints(&mut endpoints, peer.local_eps.as_slice());
        endpoints
    }

    fn local_peer_detail(&self, peer_id: &P2pId) -> Option<ServingPeerDetail> {
        let net_profile = self
            .nat_probe_scheduler
            .lock()
            .unwrap()
            .current_profile(peer_id, bucky_time_now());
        self.peer_manager().find_peer(peer_id).and_then(|peer| {
            let mut endpoints = Self::reported_endpoints_for_peer(&peer);
            Self::dedup_endpoints(&mut endpoints);
            peer.desc
                .get_encoded_cert()
                .ok()
                .map(|peer_info| ServingPeerDetail {
                    peer_info,
                    endpoints,
                    net_profile,
                    target_protocol_version: peer.protocol_version,
                })
        })
    }

    async fn query_serving_leases(
        &self,
        local_sn_id: &P2pId,
        peer_id: &P2pId,
    ) -> Vec<ServingLease> {
        match self
            .owner_client
            .query_serving_leases(local_sn_id, peer_id)
            .await
        {
            Ok(leases) => leases,
            Err(err) => {
                warn!(
                    "query serving leases failed local_sn={} peer={} err={:?}",
                    local_sn_id, peer_id, err
                );
                Vec::new()
            }
        }
    }

    async fn query_remote_details(
        &self,
        local_sn_id: &P2pId,
        peer_id: &P2pId,
    ) -> RemotePeerDetails {
        let leases = self.query_serving_leases(local_sn_id, peer_id).await;
        let mut details = Vec::new();
        let mut target_protocol_version = None;
        let mut version_is_complete = !leases.is_empty();
        for lease in leases {
            if lease.serving_sn_id == *local_sn_id {
                version_is_complete = false;
                continue;
            }
            if let Some(inter_sn_client) = self.inter_sn_client() {
                match inter_sn_client
                    .query_detail_from_sn(&lease.serving_sn_id, peer_id.clone())
                    .await
                {
                    Ok(Some(detail)) => {
                        match detail.target_protocol_version {
                            Some(version) => match target_protocol_version {
                                Some(expected) if expected != version => {
                                    version_is_complete = false;
                                }
                                Some(_) => {}
                                None => target_protocol_version = Some(version),
                            },
                            None => version_is_complete = false,
                        }
                        details.push(detail);
                        continue;
                    }
                    Ok(None) => {
                        version_is_complete = false;
                        continue;
                    }
                    Err(err) if err.code() == P2pErrorCode::NotFound => {
                        version_is_complete = false;
                        continue;
                    }
                    Err(err) => {
                        version_is_complete = false;
                        warn!(
                            "query remote detail failed peer={} serving_sn={} err={:?}",
                            peer_id, lease.serving_sn_id, err
                        );
                        continue;
                    }
                }
            }

            version_is_complete = false;
            warn!(
                "inter-sn detail query skipped because transport client is not configured peer={} serving_sn={}",
                peer_id, lease.serving_sn_id
            );
        }
        if !version_is_complete {
            target_protocol_version = None;
        }
        RemotePeerDetails {
            details,
            target_protocol_version,
        }
    }

    async fn relay_call_to_serving_sn(
        &self,
        local_sn_id: &P2pId,
        call_req: SnCall,
    ) -> Option<SnCallResp> {
        for lease in self
            .query_serving_leases(local_sn_id, &call_req.to_peer_id)
            .await
        {
            if lease.serving_sn_id == *local_sn_id {
                continue;
            }
            if let Some(inter_sn_client) = self.inter_sn_client() {
                match inter_sn_client
                    .relay_call_to_sn(&lease.serving_sn_id, call_req.clone())
                    .await
                {
                    Ok(outcome) if outcome.accepted => {
                        return Some(SnCallResp {
                            seq: call_req.seq,
                            sn_peer_id: local_sn_id.clone(),
                            result: P2pErrorCode::Ok.into_u8(),
                            to_peer_info: outcome.to_peer_info,
                        });
                    }
                    Ok(_) => continue,
                    Err(err) if err.code() == P2pErrorCode::NotFound => {}
                    Err(err) => {
                        warn!(
                            "relay call failed from={} to={} serving_sn={} err={:?}",
                            call_req.from_peer_id, call_req.to_peer_id, lease.serving_sn_id, err
                        );
                        continue;
                    }
                }
            } else {
                warn!(
                    "inter-sn relay skipped because transport client is not configured from={} to={} serving_sn={}",
                    call_req.from_peer_id, call_req.to_peer_id, lease.serving_sn_id
                );
            }
        }
        None
    }

    async fn rendezvous_endpoints_owned_by(
        &self,
        peer_id: &PeerId,
        tunnel_id: CmdTunnelId,
        endpoints: &[Endpoint],
    ) -> bool {
        if endpoints.is_empty() {
            return true;
        }
        let Some(observed_tunnel) = self.get_peer_tunnel_remote(peer_id, tunnel_id).await else {
            return false;
        };
        let observed_ip = observed_tunnel.addr().ip();
        endpoints
            .iter()
            .all(|endpoint| endpoint.addr().ip() == observed_ip)
    }

    async fn validate_rendezvous_response_owner(
        &self,
        target_peer_id: &P2pId,
        response: &SnTunnelRendezvousResp,
    ) -> P2pResult<()> {
        if response.predicted_endpoint_array.is_empty() {
            return Ok(());
        }

        let target = PeerId::from(target_peer_id.as_slice());
        let observed_ips = self
            .get_peer_observed_ep(&target)
            .await
            .into_iter()
            .map(|endpoint| endpoint.addr().ip())
            .collect::<HashSet<_>>();
        if observed_ips.is_empty()
            || response
                .predicted_endpoint_array
                .iter()
                .any(|endpoint| !observed_ips.contains(&endpoint.addr().ip()))
        {
            return Err(p2p_err!(
                P2pErrorCode::PermissionDenied,
                "rendezvous response contains an endpoint not observed for target {}",
                target_peer_id
            ));
        }

        Ok(())
    }

    async fn deliver_rendezvous_to_local_peer(
        &self,
        target_peer_id: &P2pId,
        notify: &SnTunnelRendezvousNotify,
    ) -> P2pResult<SnTunnelRendezvousResp> {
        if self.peer_manager().find_peer(target_peer_id).is_none() {
            return Err(p2p_err!(
                P2pErrorCode::TargetNotFound,
                "rendezvous target is not connected to this SN"
            ));
        }
        let bytes = notify
            .to_vec()
            .map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
        // The command server deliberately suppresses re-entrant QA sends from
        // the task currently handling A -> SN. Run SN -> B QA in a distinct
        // task while still awaiting it here so B's response remains ordered
        // before the response returned to A.
        let cmd_server = self.cmd_server.clone();
        let target = PeerId::from(target_peer_id.as_slice());
        let mut body = tokio::spawn(async move {
            cmd_server
                .send_with_resp(
                    &target,
                    PackageCmdCode::SnTunnelRendezvousNotify as u8,
                    SN_TUNNEL_RENDEZVOUS_CMD_VERSION,
                    bytes.as_slice(),
                    Duration::from_secs(10),
                )
                .await
        })
        .await
        .map_err(|err| {
            p2p_err!(
                P2pErrorCode::Aborted,
                "SN rendezvous target QA task failed: {}",
                err
            )
        })?
        .map_err(into_p2p_err!(P2pErrorCode::IoError))?;
        let response = SnTunnelRendezvousResp::clone_from_slice(
            body.read_all()
                .await
                .map_err(into_p2p_err!(P2pErrorCode::IoError))?
                .as_slice(),
        )
        .map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
        response.validate(notify.seq, notify.need_predict_endpoint)?;
        self.validate_rendezvous_response_owner(target_peer_id, &response)
            .await?;
        Ok(response)
    }

    async fn relay_rendezvous_to_serving_sn(
        &self,
        local_sn_id: &P2pId,
        target_peer_id: &P2pId,
        notify: &SnTunnelRendezvousNotify,
    ) -> P2pResult<SnTunnelRendezvousResp> {
        for lease in self.query_serving_leases(local_sn_id, target_peer_id).await {
            if lease.serving_sn_id == *local_sn_id {
                continue;
            }
            let Some(inter_sn_client) = self.inter_sn_client() else {
                continue;
            };
            match inter_sn_client
                .relay_rendezvous_to_sn(
                    &lease.serving_sn_id,
                    target_peer_id.clone(),
                    notify.clone(),
                )
                .await
            {
                Ok(response) => {
                    response.validate(notify.seq, notify.need_predict_endpoint)?;
                    return Ok(response);
                }
                Err(err) if err.code() == P2pErrorCode::NotFound => continue,
                Err(err) => {
                    warn!(
                        "event=sn_rendezvous_relay_failed seq={} target={} serving_sn={} reason={:?}",
                        notify.seq.value(),
                        target_peer_id,
                        lease.serving_sn_id,
                        err.code()
                    );
                }
            }
        }
        Err(p2p_err!(
            P2pErrorCode::NotFound,
            "no serving SN accepted rendezvous target"
        ))
    }

    async fn process_rendezvous_request(
        &self,
        authenticated_initiator: &P2pId,
        request: &SnTunnelRendezvous,
        notify: &SnTunnelRendezvousNotify,
        local_sn_id: &P2pId,
    ) -> SnTunnelRendezvousResp {
        let now = bucky_time_now();
        let begin =
            self.rendezvous_state
                .lock()
                .unwrap()
                .begin(authenticated_initiator, request, now);
        match begin {
            Ok(RendezvousBegin::Cached(response)) => return response,
            Ok(RendezvousBegin::InFlight(waiter)) => {
                return Self::await_inflight_rendezvous(request.seq, waiter).await;
            }
            Ok(RendezvousBegin::New) => {}
            Err(_) => return SnTunnelRendezvousResp::failure(request.seq),
        }

        let mut leader =
            RendezvousLeaderGuard::new(&self.rendezvous_state, authenticated_initiator, request);
        let response = match runtime::timeout(Duration::from_secs(10), async {
            match self
                .deliver_rendezvous_to_local_peer(&request.to_peer_id, notify)
                .await
            {
                Ok(response) => Ok(response),
                Err(local_err) if local_err.code() == P2pErrorCode::TargetNotFound => {
                    self.relay_rendezvous_to_serving_sn(local_sn_id, &request.to_peer_id, notify)
                        .await
                }
                Err(err) => Err(err),
            }
        })
        .await
        {
            Ok(Ok(response)) => response,
            Ok(Err(err)) => {
                warn!(
                    "event=sn_rendezvous_failed seq={} initiator={} target={} reason={:?}",
                    request.seq.value(),
                    authenticated_initiator,
                    request.to_peer_id,
                    err.code()
                );
                return SnTunnelRendezvousResp::failure(request.seq);
            }
            Err(_) => {
                warn!(
                    "event=sn_rendezvous_timeout seq={} initiator={} target={}",
                    request.seq.value(),
                    authenticated_initiator,
                    request.to_peer_id
                );
                return SnTunnelRendezvousResp::failure(request.seq);
            }
        };
        let cache_result = self.rendezvous_state.lock().unwrap().cache_response(
            authenticated_initiator,
            request,
            response.clone(),
            bucky_time_now(),
        );
        if cache_result.is_ok() {
            leader.disarm();
            response
        } else {
            SnTunnelRendezvousResp::failure(request.seq)
        }
    }

    async fn await_inflight_rendezvous(
        seq: crate::types::Sequence,
        waiter: tokio::sync::oneshot::Receiver<P2pResult<SnTunnelRendezvousResp>>,
    ) -> SnTunnelRendezvousResp {
        match runtime::timeout(Duration::from_secs(10), waiter).await {
            Ok(Ok(Ok(response))) => response,
            _ => SnTunnelRendezvousResp::failure(seq),
        }
    }

    async fn handle_rendezvous(
        &self,
        request: SnTunnelRendezvous,
        local_id: &P2pId,
        peer_id: &PeerId,
        tunnel_id: CmdTunnelId,
    ) -> SnTunnelRendezvousResp {
        let failure = || SnTunnelRendezvousResp::failure(request.seq);
        if request.validate().is_err() {
            return failure();
        }
        let client_id = P2pId::from(peer_id.as_slice());
        if client_id == request.to_peer_id {
            return failure();
        }
        let client_cert = match self.client_cert_from_request_or_cache(&client_id, None) {
            Ok(cert) => cert,
            Err(_) => return failure(),
        };
        let context = match self.validate_context_from_cert(peer_id, client_cert.clone()) {
            Ok(context) => context,
            Err(_) => return failure(),
        };
        if self.validate_connection(context).await.is_err() {
            return failure();
        }
        self.observe_nat_probe_control(local_id, peer_id, tunnel_id)
            .await;
        if !self
            .rendezvous_endpoints_owned_by(peer_id, tunnel_id, &request.end_point_array)
            .await
        {
            return failure();
        }
        let notify = SnTunnelRendezvousNotify {
            seq: request.seq,
            tunnel_id: request.tunnel_id,
            peer_info: client_cert,
            operation: request.operation,
            end_point_array: request.end_point_array.clone(),
            need_predict_endpoint: request.need_predict_endpoint,
        };
        if notify.validate().is_err() {
            return failure();
        }
        log::info!(
            "event=sn_rendezvous_request seq={} initiator={} target={} operation={:?} endpoint_count={} predict={}",
            request.seq.value(),
            client_id,
            request.to_peer_id,
            request.operation,
            request.end_point_array.len(),
            request.need_predict_endpoint
        );
        let local_sn_id = self.effective_local_sn_id(local_id);
        self.process_rendezvous_request(&client_id, &request, &notify, &local_sn_id)
            .await
    }

    async fn deliver_called_to_local_peer(
        &self,
        mut call_req: SnCall,
        local_sn_id: P2pId,
    ) -> P2pResult<RelayCallOutcome> {
        let Some(to_peer_cache) = self.peer_manager().find_peer(&call_req.to_peer_id) else {
            return Ok(RelayCallOutcome {
                accepted: false,
                to_peer_info: None,
            });
        };
        let Some(from_peer_info) = call_req.peer_info.clone() else {
            return Ok(RelayCallOutcome {
                accepted: false,
                to_peer_info: Some(to_peer_cache.desc.get_encoded_cert()?),
            });
        };
        let from_peer_desc = self.cert_factory.create(&from_peer_info)?;

        if self
            .call_stub
            .insert(&call_req.from_peer_id, &call_req.tunnel_id)
            && (call_req.is_always_call || !to_peer_cache.is_wan)
        {
            let called_seq = self.seq_generator.generate();
            let mut called_req = SnCalled {
                seq: called_seq,
                to_peer_id: call_req.to_peer_id.clone(),
                sn_peer_id: local_sn_id,
                peer_info: from_peer_desc.get_encoded_cert()?,
                tunnel_id: call_req.tunnel_id,
                call_send_time: call_req.send_time,
                call_type: call_req.call_type,
                payload: vec![],
                reverse_endpoint_array: vec![],
                active_pn_list: vec![],
                nat_context: call_req.nat_context.clone(),
            };

            std::mem::swap(&mut call_req.payload, &mut called_req.payload);
            if let Some(eps) = call_req.reverse_endpoint_array.as_mut() {
                std::mem::swap(eps, &mut called_req.reverse_endpoint_array);
            }
            if let Some(pn_list) = call_req.active_pn_list.as_mut() {
                std::mem::swap(pn_list, &mut called_req.active_pn_list);
            }
            Self::dedup_endpoints(&mut called_req.reverse_endpoint_array);

            self.cmd_server
                .send_by_all_tunnels(
                    &PeerId::from(call_req.to_peer_id.as_slice()),
                    PackageCmdCode::SnCalled as u8,
                    self.cmd_version,
                    called_req
                        .to_vec()
                        .map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?
                        .as_slice(),
                )
                .await
                .map_err(into_p2p_err!(P2pErrorCode::IoError))?;
        }

        Ok(RelayCallOutcome {
            accepted: true,
            to_peer_info: Some(to_peer_cache.desc.get_encoded_cert()?),
        })
    }

    async fn handle_call(
        &self,
        mut call_req: SnCall,
        local_id: &P2pId,
        peer_id: &PeerId,
        tunnel_id: CmdTunnelId,
        _send_time: Timestamp,
    ) -> P2pResult<SnCallResp> {
        let client_id = P2pId::from(peer_id.as_slice());
        let client_cert =
            self.client_cert_from_request_or_cache(&client_id, call_req.peer_info.clone())?;
        self.validate_connection(self.validate_context_from_cert(peer_id, client_cert)?)
            .await?;
        self.observe_nat_probe_control(local_id, peer_id, tunnel_id)
            .await;
        let local_sn_id = self.effective_local_sn_id(local_id);
        self.reconcile_nat_probe_authority(&call_req.to_peer_id)
            .await;
        let now = bucky_time_now();
        if self
            .nat_probe_scheduler
            .lock()
            .unwrap()
            .current_profile(&call_req.to_peer_id, now)
            .is_none()
        {
            self.nat_probe_scheduler
                .lock()
                .unwrap()
                .mark_demand(&call_req.to_peer_id, now);
        }

        let from_peer_id = &call_req.from_peer_id;
        let log_key = format!(
            "[call {}->{} seq({})]",
            from_peer_id.to_string(),
            call_req.to_peer_id.to_string(),
            call_req.seq.value()
        );
        info!("{}.", log_key);

        let call_resp =
            if let Some(from_peer_cache) = self.peer_manager().find_peer(&call_req.from_peer_id) {
                if let Some(to_peer_cache) = self.peer_manager().find_peer(&call_req.to_peer_id) {
                    // Self::call_stat_contract(to_peer_cache, &call_req);
                    let from_peer_desc = if call_req.peer_info.is_none() {
                        self.peer_manager().find_peer(from_peer_id).map(|c| c.desc)
                    } else {
                        call_req
                            .peer_info
                            .map(|info| self.cert_factory.create(&info).unwrap())
                    };

                    let from_reported_eps = Self::reported_endpoints_for_peer(&from_peer_cache);
                    let mut reverse_eps = self
                        .get_peer_wan_ep_with_map_port(
                            &peer_id,
                            from_peer_cache.map_ports.as_slice(),
                            from_reported_eps.as_slice(),
                        )
                        .await;
                    Self::extend_unique_endpoints(
                        &mut reverse_eps,
                        from_peer_cache.local_eps.as_slice(),
                    );

                    if let Some(from_peer_desc) = from_peer_desc {
                        info!(
                            "{} to-peer found, endpoints: {}, always_call: {}, to-peer.is_wan: {}.",
                            log_key,
                            endpoints_to_string(to_peer_cache.desc.endpoints().as_slice()),
                            call_req.is_always_call,
                            to_peer_cache.is_wan
                        );

                        if self.call_stub.insert(from_peer_id, &call_req.tunnel_id) {
                            if call_req.is_always_call || !to_peer_cache.is_wan {
                                let called_seq = self.seq_generator.generate();
                                let mut called_req = SnCalled {
                                    seq: called_seq,
                                    to_peer_id: call_req.to_peer_id.clone(),
                                    sn_peer_id: local_id.clone(),
                                    peer_info: from_peer_desc.get_encoded_cert().unwrap(),
                                    tunnel_id: call_req.tunnel_id,
                                    call_send_time: call_req.send_time,
                                    call_type: call_req.call_type,
                                    payload: vec![],
                                    reverse_endpoint_array: vec![],
                                    active_pn_list: vec![],
                                    nat_context: call_req.nat_context.clone(),
                                };

                                std::mem::swap(&mut call_req.payload, &mut called_req.payload);
                                if let Some(eps) = call_req.reverse_endpoint_array.as_mut() {
                                    std::mem::swap(eps, &mut called_req.reverse_endpoint_array);
                                }
                                if let Some(pn_list) = call_req.active_pn_list.as_mut() {
                                    std::mem::swap(pn_list, &mut called_req.active_pn_list);
                                }
                                Self::dedup_endpoints(&mut called_req.reverse_endpoint_array);
                                Self::extend_unique_endpoints(
                                    &mut called_req.reverse_endpoint_array,
                                    reverse_eps.as_slice(),
                                );

                                let called_log =
                                    format!("{} called-req seq({})", log_key, called_seq.value());
                                log::info!(
                                    "{} will send with payload(len={}) pn_list({:?}).",
                                    called_log,
                                    called_req.payload.len(),
                                    called_req.active_pn_list
                                );

                                self.cmd_server
                                    .send_by_all_tunnels(
                                        &PeerId::from(call_req.to_peer_id.as_slice()),
                                        PackageCmdCode::SnCalled as u8,
                                        self.cmd_version,
                                        called_req
                                            .to_vec()
                                            .map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?
                                            .as_slice(),
                                    )
                                    .await
                                    .map_err(into_p2p_err!(P2pErrorCode::IoError))?;
                            }
                        } else {
                            info!("{} ignore send called req for already exists.", log_key);
                        }

                        SnCallResp {
                            seq: call_req.seq,
                            sn_peer_id: local_id.clone(),
                            result: P2pErrorCode::Ok.into_u8(),
                            to_peer_info: Some(to_peer_cache.desc.get_encoded_cert().unwrap()),
                        }
                    } else {
                        warn!("{} without from-desc.", log_key);

                        SnCallResp {
                            seq: call_req.seq,
                            sn_peer_id: local_id.clone(),
                            result: P2pErrorCode::NotFound.into_u8(),
                            to_peer_info: None,
                        }
                    }
                } else {
                    warn!("{} to-peer not found.", log_key);
                    if let Some(relay_resp) = self
                        .relay_call_to_serving_sn(&local_sn_id, call_req.clone())
                        .await
                    {
                        relay_resp
                    } else {
                        SnCallResp {
                            seq: call_req.seq,
                            sn_peer_id: local_id.clone(),
                            result: P2pErrorCode::NotFound.into_u8(),
                            to_peer_info: None,
                        }
                    }
                }
            } else {
                warn!("{} from-peer not found.", log_key);
                SnCallResp {
                    seq: call_req.seq,
                    sn_peer_id: local_id.clone(),
                    result: P2pErrorCode::NotFound.into_u8(),
                    to_peer_info: None,
                }
            };

        Ok(call_resp)
    }

    async fn handle_called_resp(&self, called_resp: SnCalledResp) {
        info!("called-resp seq {}.", called_resp.seq.value());

        // 统计性能
        // if let Some((call_send_time, called_send_time, peerid)) = self.call_tracker.calls.remove(&called_resp.seq) {
        //     if let Some(cached_peer) = self.peer_mgr.find_peer(&peerid, FindPeerReason::Other) {
        //         let now_time_stamp = bucky_time_now();
        //         if now_time_stamp > call_send_time {
        //             let call_delay = (now_time_stamp - call_send_time) / 1000;
        //             cached_peer.receipt.call_delay = ((cached_peer.receipt.call_delay as u64 * 7 + call_delay) / 8) as u16;
        //         }

        //         let rto = Instant::now().duration_since(called_send_time).as_millis() as u32;
        //         cached_peer.receipt.rto = ((cached_peer.receipt.rto as u32 * 7 + rto) / 8) as u16;
        //     }
        // }
    }

    pub async fn get_peer_wan_classied_ep(
        &self,
        peer_id: &PeerId,
        reported_eps: &[Endpoint],
    ) -> Vec<Endpoint> {
        self.get_peer_wan_ep(peer_id)
            .await
            .into_iter()
            .map(|remote| Self::classify_observed_endpoint(remote, reported_eps))
            .collect()
    }

    pub async fn get_peer_wan_ep(&self, peer_id: &PeerId) -> Vec<Endpoint> {
        self.get_peer_observed_ep(peer_id)
            .await
            .into_iter()
            .filter(Self::is_direct_observed_candidate)
            .map(|mut remote| {
                remote.set_area(EndpointArea::Wan);
                remote
            })
            .collect()
    }

    pub async fn get_peer_observed_ep(&self, peer_id: &PeerId) -> Vec<Endpoint> {
        let tunnels = self.cmd_server.get_peer_tunnels(peer_id).await;
        let mut remotes = Vec::new();
        for tunnel in tunnels.iter() {
            let remote = tunnel.send.get().await.remote();
            #[cfg(feature = "test-real-socket-matrix")]
            let skip = false;
            #[cfg(not(feature = "test-real-socket-matrix"))]
            let skip = remote.is_loopback();
            if skip {
                continue;
            }
            if !remotes.contains(&remote) {
                remotes.push(remote);
            }
        }
        remotes
    }

    async fn get_peer_tunnel_remote(
        &self,
        peer_id: &PeerId,
        tunnel_id: CmdTunnelId,
    ) -> Option<Endpoint> {
        for tunnel in self.cmd_server.get_peer_tunnels(peer_id).await {
            if tunnel.conn_id == tunnel_id {
                return Some(tunnel.send.get().await.remote());
            }
        }
        None
    }

    async fn reconcile_nat_probe_authority(&self, peer_id: &P2pId) {
        let authority = self
            .nat_probe_scheduler
            .lock()
            .unwrap()
            .authority_tunnel(peer_id);
        let Some(authority) = authority else {
            return;
        };
        let cmd_peer_id = PeerId::from(peer_id.as_slice());
        let tunnels = self.cmd_server.get_peer_tunnels(&cmd_peer_id).await;
        if tunnels.iter().any(|tunnel| tunnel.conn_id == authority) {
            return;
        }
        self.nat_probe_scheduler
            .lock()
            .unwrap()
            .remove_peer(peer_id, NatProbeAuthorityRemovalReason::TunnelMissing);
        self.peer_mgr.invalidate_net_profile(peer_id);
    }

    fn apply_nat_probe_transition(
        &self,
        peer_id: &P2pId,
        transition: super::nat_probe_scheduler::ProbeTransition,
    ) -> Option<NatProbeDirective> {
        if let Some(profile_update) = transition.profile_update {
            match profile_update {
                Some(profile) => self.peer_mgr.set_net_profile(peer_id, profile),
                None => self.peer_mgr.invalidate_net_profile(peer_id),
            };
        }
        transition.directive
    }

    async fn observe_nat_probe_control(
        &self,
        local_id: &P2pId,
        peer_id: &PeerId,
        tunnel_id: CmdTunnelId,
    ) {
        let authenticated_peer_id = P2pId::from(peer_id.as_slice());
        self.reconcile_nat_probe_authority(&authenticated_peer_id)
            .await;
        let Some(remote_endpoint) = self.get_peer_tunnel_remote(peer_id, tunnel_id).await else {
            return;
        };
        let transition = {
            let mut scheduler = self.nat_probe_scheduler.lock().unwrap();
            scheduler.set_sn_peer_id(local_id);
            scheduler.observe_control(
                &authenticated_peer_id,
                tunnel_id,
                remote_endpoint,
                bucky_time_now(),
            )
        };
        self.apply_nat_probe_transition(&authenticated_peer_id, transition);
    }

    async fn maintain_nat_probe_state(&self) {
        let authorities = self.nat_probe_scheduler.lock().unwrap().authorities();
        for (peer_id, _) in authorities {
            self.reconcile_nat_probe_authority(&peer_id).await;
        }
        let invalidated = self
            .nat_probe_scheduler
            .lock()
            .unwrap()
            .expire_due(bucky_time_now());
        for peer_id in invalidated {
            self.peer_mgr.invalidate_net_profile(&peer_id);
        }
    }

    async fn get_peer_wan_ep_with_map_port(
        &self,
        peer_id: &PeerId,
        map_ports: &[(Protocol, u16)],
        reported_eps: &[Endpoint],
    ) -> Vec<Endpoint> {
        let observed_ep = self.get_peer_observed_ep(peer_id).await;
        Self::observed_endpoint_candidates(observed_ep.as_slice(), map_ports, reported_eps)
    }

    async fn handle_report_sn(
        &self,
        local_id: &P2pId,
        peer_id: &PeerId,
        tunnel_id: CmdTunnelId,
        mut report_sn: ReportSn,
    ) -> P2pResult<ReportSnResp> {
        self.validate_connection(self.validate_context_from_cert(
            peer_id,
            report_sn.peer_info.clone().ok_or_else(|| {
                p2p_err!(
                    P2pErrorCode::PermissionDenied,
                    "sn report missing client cert"
                )
            })?,
        )?)
        .await?;

        let authenticated_peer_id = P2pId::from(peer_id.as_slice());
        if let Some(from_peer_id) = report_sn.from_peer_id.as_ref() {
            if from_peer_id != &authenticated_peer_id {
                return Err(p2p_err!(
                    P2pErrorCode::PermissionDenied,
                    "sn report peer id mismatch authenticated={} reported={}",
                    authenticated_peer_id,
                    from_peer_id
                ));
            }
        }
        let local_identity = self.local_identity.lock().unwrap().clone().ok_or_else(|| {
            p2p_err!(
                P2pErrorCode::ErrorState,
                "SN report service has no local identity"
            )
        })?;
        let peer_info = Some(local_identity.get_identity_cert()?.get_encoded_cert()?);

        log::debug!(
            "event=sn_report_received peer_id={} tunnel_id={:?} local_endpoint_count={} map_port_count={} nat_probe_capability={:?} nat_probe_result={}",
            peer_id.to_base36(),
            tunnel_id,
            report_sn.local_eps.len(),
            report_sn.map_ports.len(),
            report_sn.nat_probe_control_version,
            report_sn.nat_probe_result.is_some()
        );
        let observed_tunnel = self.get_peer_tunnel_remote(peer_id, tunnel_id).await;
        let reported_eps = Self::sanitize_reported_endpoints(
            report_sn.local_eps.as_slice(),
            observed_tunnel.as_ref(),
        )?;
        let remote_ep = self
            .get_peer_wan_classied_ep(peer_id, reported_eps.as_slice())
            .await;

        let mut nat_probe_directive = None;
        self.peer_mgr.add_or_update_peer(
            &authenticated_peer_id,
            &report_sn
                .peer_info
                .map(|info| self.cert_factory.create(&info).unwrap()),
            report_sn.protocol_version,
            report_sn.map_ports,
            &reported_eps,
        );

        self.reconcile_nat_probe_authority(&authenticated_peer_id)
            .await;
        if let Some(observed_tunnel) = observed_tunnel {
            let transition = {
                let mut scheduler = self.nat_probe_scheduler.lock().unwrap();
                scheduler.set_sn_peer_id(local_id);
                scheduler.observe_capable_report(
                    &authenticated_peer_id,
                    tunnel_id,
                    observed_tunnel,
                    report_sn.nat_probe_control_version,
                    report_sn.nat_probe_result.take(),
                    bucky_time_now(),
                )
            };
            nat_probe_directive =
                self.apply_nat_probe_transition(&authenticated_peer_id, transition);
        }
        Ok(ReportSnResp {
            seq: report_sn.seq,
            sn_peer_id: local_id.clone(),
            result: P2pErrorCode::Ok.into_u8(),
            peer_info,
            end_point_array: remote_ep,
            receipt: None,
            nat_probe_endpoints: self
                .nat_probe_scheduler
                .lock()
                .unwrap()
                .endpoints()
                .to_vec(),
            nat_probe_directive,
        })
    }

    async fn handle_query_sn(
        &self,
        local_id: &P2pId,
        peer_id: &PeerId,
        tunnel_id: CmdTunnelId,
        query: SnQuery,
    ) -> P2pResult<SnQueryResp> {
        self.observe_nat_probe_control(local_id, peer_id, tunnel_id)
            .await;
        let requester_sn_id = self.effective_local_sn_id(local_id);
        self.reconcile_nat_probe_authority(&query.query_id).await;
        let now = bucky_time_now();
        let local_net_profile = self
            .nat_probe_scheduler
            .lock()
            .unwrap()
            .current_profile(&query.query_id, now);
        if local_net_profile.is_none() {
            self.nat_probe_scheduler
                .lock()
                .unwrap()
                .mark_demand(&query.query_id, now);
        }
        let device_info = self.peer_mgr.find_peer(&query.query_id);
        let remote = self
            .query_remote_details(&requester_sn_id, &query.query_id)
            .await;
        let mut remote_details = remote.details;
        let resp = if device_info.is_some() {
            let device_info = device_info.unwrap();
            let reported_eps = Self::reported_endpoints_for_peer(&device_info);
            let mut end_point_array = self
                .get_peer_wan_ep_with_map_port(
                    &PeerId::from(query.query_id.as_slice()),
                    device_info.map_ports.as_slice(),
                    reported_eps.as_slice(),
                )
                .await;
            Self::extend_unique_endpoints(&mut end_point_array, device_info.local_eps.as_slice());
            for detail in remote_details.drain(..) {
                Self::extend_unique_endpoints(&mut end_point_array, detail.endpoints.as_slice());
            }
            SnQueryResp {
                seq: query.seq,
                peer_info: Some(device_info.desc.get_encoded_cert().unwrap()),
                end_point_array,
                net_profile: local_net_profile,
                target_protocol_version: device_info.protocol_version,
            }
        } else if let Some(first_detail) = remote_details.first().cloned() {
            let mut end_point_array = Vec::new();
            for detail in remote_details.iter() {
                Self::extend_unique_endpoints(&mut end_point_array, detail.endpoints.as_slice());
            }
            SnQueryResp {
                seq: query.seq,
                peer_info: Some(first_detail.peer_info),
                end_point_array,
                net_profile: first_detail.net_profile,
                target_protocol_version: remote.target_protocol_version,
            }
        } else {
            SnQueryResp {
                seq: query.seq,
                peer_info: None,
                end_point_array: vec![],
                net_profile: None,
                target_protocol_version: None,
            }
        };

        Ok(resp)
    }
}

#[async_trait::async_trait]
impl CmdTunnelService<(), SnTunnelRead, SnTunnelWrite> for SnService {
    async fn handle_tunnel(&self, tunnel: CmdTunnel<SnTunnelRead, SnTunnelWrite>) -> CmdResult<()> {
        self.cmd_server.serve_tunnel(tunnel).await
    }
}

#[async_trait::async_trait]
impl CmdServerEventListener for SnService {
    async fn on_peer_connected(&self, _peer_id: &PeerId) -> CmdResult<()> {
        Ok(())
    }

    async fn on_peer_disconnected(&self, peer_id: &PeerId) -> CmdResult<()> {
        let peer_id = P2pId::from(peer_id.as_slice());
        self.nat_probe_scheduler
            .lock()
            .unwrap()
            .remove_peer(&peer_id, NatProbeAuthorityRemovalReason::PeerDisconnected);
        self.rendezvous_state.lock().unwrap().remove_peer(&peer_id);
        self.peer_mgr.remove_peer(peer_id);
        Ok(())
    }
}

#[async_trait::async_trait]
impl InterSnPeer for SnService {
    fn sn_id(&self) -> Option<P2pId> {
        self.local_sn_id.clone()
    }

    async fn query_detail_from_sn(
        &self,
        remote_sn_id: P2pId,
        peer_id: P2pId,
    ) -> P2pResult<Option<ServingPeerDetail>> {
        self.validate_inter_connection(&remote_sn_id).await?;
        self.validate_inter_command(&remote_sn_id, InterSnCommand::QueryDetail, &peer_id)
            .await?;
        self.reconcile_nat_probe_authority(&peer_id).await;
        Ok(self.local_peer_detail(&peer_id))
    }

    async fn relay_call_from_sn(
        &self,
        remote_sn_id: P2pId,
        call_req: SnCall,
    ) -> P2pResult<RelayCallOutcome> {
        self.validate_inter_connection(&remote_sn_id).await?;
        self.validate_inter_command(
            &remote_sn_id,
            InterSnCommand::RelayCall,
            &call_req.to_peer_id,
        )
        .await?;
        let local_sn_id = self
            .local_sn_id
            .clone()
            .unwrap_or_else(|| call_req.sn_peer_id.clone());
        self.deliver_called_to_local_peer(call_req, local_sn_id)
            .await
    }

    async fn relay_rendezvous_from_sn(
        &self,
        remote_sn_id: P2pId,
        target_peer_id: P2pId,
        notify: SnTunnelRendezvousNotify,
    ) -> P2pResult<SnTunnelRendezvousResp> {
        self.validate_inter_connection(&remote_sn_id).await?;
        self.validate_inter_command(
            &remote_sn_id,
            InterSnCommand::RelayRendezvous,
            &target_peer_id,
        )
        .await?;
        notify.validate()?;
        let initiator = self.cert_factory.create(&notify.peer_info)?.get_id();
        if initiator == target_peer_id {
            return Err(p2p_err!(
                P2pErrorCode::PermissionDenied,
                "rendezvous initiator and target must differ"
            ));
        }
        self.deliver_rendezvous_to_local_peer(&target_peer_id, &notify)
            .await
    }
}

pub struct SnServer {
    local_identity: P2pIdentityRef,
    net_manager: NetManagerRef,
    ttp_server: TtpServerRef,
    service: SnServiceRef,
    started: AtomicBool,
    stopped: AtomicBool,
    cmd_accept_task: Mutex<Option<crate::executor::SpawnHandle<()>>>,
    nat_probe_ports: Vec<u16>,
    nat_probe_tasks: Mutex<Vec<crate::executor::SpawnHandle<()>>>,
}

impl SnServer {
    pub(crate) async fn new(
        local_identity: P2pIdentityRef,
        identity_factory: P2pIdentityFactoryRef,
        cert_factory: P2pIdentityCertFactoryRef,
        connection_validator: SnConnectionValidatorRef,
        inter_service_validator: SnInterServiceValidatorRef,
        owner_client_membership: Option<OwnerMembership>,
        owner_client_override: Option<OwnerDirectoryClientRef>,
        congestion_algorithm: QuicCongestionAlgorithm,
        reuse_address: bool,
        server_runtime: ServerRuntime,
        nat_probe_ports: Vec<u16>,
        nat_probe_advertised_ipv4: Option<Ipv4Addr>,
    ) -> SnServerRef {
        init_tls(identity_factory);

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
            cert_factory.clone(),
            congestion_algorithm,
            Duration::from_secs(30),
            Duration::from_secs(30),
            server_runtime,
        ));
        TunnelNetwork::set_reuse_address(quic_network.as_ref(), reuse_address);
        let tunnel_networks = vec![
            tcp_network as TunnelNetworkRef,
            quic_network as TunnelNetworkRef,
        ];

        let net_manager = NetManager::new(tunnel_networks, cert_resolver).unwrap();
        let ttp_server = TtpServer::new(local_identity.clone(), net_manager.clone()).unwrap();
        let serving_connector = if owner_client_override.is_none() {
            owner_client_membership.as_ref().map(|_| {
                TtpClient::new(local_identity.clone(), net_manager.clone()) as Arc<dyn TtpConnector>
            })
        } else {
            None
        };
        let owner_client = owner_client_override.clone().unwrap_or_else(|| {
            owner_client_membership
                .clone()
                .map(|membership| {
                    StaticOwnerDirectoryClient::new_with_serving_connector(
                        membership,
                        serving_connector,
                        None,
                    )
                })
                .unwrap_or_else(noop_owner_directory_client)
        });
        let service = SnService::new_with_options(
            cert_factory,
            connection_validator,
            inter_service_validator,
            owner_client,
            None,
            Some(local_identity.get_id()),
        );
        service.set_local_identity(local_identity.clone());
        let nat_probe_endpoints = nat_probe_advertised_ipv4
            .map(|ip| {
                nat_probe_ports
                    .iter()
                    .map(|port| {
                        let mut endpoint = Endpoint::from((
                            Protocol::Quic,
                            SocketAddr::V4(SocketAddrV4::new(ip, *port)),
                        ));
                        endpoint.set_area(EndpointArea::Wan);
                        endpoint
                    })
                    .collect()
            })
            .unwrap_or_default();
        service.set_nat_probe_endpoints(nat_probe_endpoints);
        if owner_client_override.is_none() {
            if let Some(membership) = owner_client_membership.as_ref() {
                let ttp_node = TtpNode::new_with_runtime(ttp_server.runtime());
                match TtpInterSnClient::new(
                    ttp_node,
                    membership,
                    service.clone() as Arc<dyn InterSnPeer>,
                )
                .await
                {
                    Ok(inter_sn_client) => {
                        service.set_inter_sn_client(Some(inter_sn_client));
                    }
                    Err(err) => {
                        warn!("create inter-sn client failed: {:?}", err);
                    }
                }
            }
        }

        Arc::new(Self {
            local_identity,
            net_manager,
            ttp_server,
            service,
            started: AtomicBool::new(false),
            stopped: AtomicBool::new(false),
            cmd_accept_task: Mutex::new(None),
            nat_probe_ports,
            nat_probe_tasks: Mutex::new(Vec::new()),
        })
    }

    pub fn get_cmd_server(&self) -> &SnCmdServiceRef {
        self.service.get_cmd_server()
    }

    pub fn service(&self) -> &SnServiceRef {
        &self.service
    }

    pub fn ttp_server(&self) -> TtpServerRef {
        self.ttp_server.clone()
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
            log::debug!("sn server start skipped because already started");
            return Ok(());
        }

        log::info!(
            "sn server start begin local_id={}",
            self.local_identity.get_id()
        );

        if let Err(err) = self.start_inner().await {
            self.abort_accept_tasks();
            self.started.store(false, atomic::Ordering::SeqCst);
            log::error!(
                "sn server start failed local_id={} err={:?}",
                self.local_identity.get_id(),
                err
            );
            return Err(err);
        }

        log::info!(
            "sn server start success local_id={}",
            self.local_identity.get_id()
        );
        Ok(())
    }

    async fn start_inner(self: &Arc<Self>) -> P2pResult<()> {
        log::debug!(
            "sn server listen endpoints local_id={} endpoints={:?}",
            self.local_identity.get_id(),
            self.local_identity.endpoints()
        );
        self.net_manager
            .listen(self.local_identity.endpoints().as_slice(), None)
            .await?;
        log::debug!(
            "sn server net_manager listen ready local_id={}",
            self.local_identity.get_id()
        );
        self.start_nat_probe_reflectors().await?;
        self.start_nat_probe_maintenance()?;
        self.start_cmd_accept_loop().await?;
        Ok(())
    }

    async fn start_nat_probe_reflectors(&self) -> P2pResult<()> {
        if self.nat_probe_ports.is_empty() {
            return Ok(());
        }

        let signing_context = NatProbeSigningContext::new(self.local_identity.clone()).await?;
        let mut reflectors = Vec::with_capacity(self.nat_probe_ports.len());
        for port in self.nat_probe_ports.iter().copied() {
            let bind_addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port));
            reflectors.push(Arc::new(
                NatProbeReflector::bind_with_context(bind_addr, signing_context.clone()).await?,
            ));
        }

        let mut tasks = self.nat_probe_tasks.lock().unwrap();
        for reflector in reflectors {
            let task = Executor::spawn_with_handle(async move {
                if let Err(err) = reflector.run().await {
                    warn!("NAT probe reflector stopped: {:?}", err);
                }
            })?;
            tasks.push(task);
        }
        Ok(())
    }

    fn start_nat_probe_maintenance(&self) -> P2pResult<()> {
        let service = self.service.clone();
        let task = Executor::spawn_with_handle(async move {
            loop {
                runtime::sleep(NAT_PROBE_MAINTENANCE_INTERVAL).await;
                service.maintain_nat_probe_state().await;
            }
        })?;
        self.nat_probe_tasks.lock().unwrap().push(task);
        Ok(())
    }

    async fn start_cmd_accept_loop(self: &Arc<Self>) -> P2pResult<()> {
        let purpose = sn_cmd_purpose()?;
        log::debug!(
            "sn server start cmd accept loop local_id={} purpose={}",
            self.local_identity.get_id(),
            purpose
        );
        self.ttp_server
            .listen_control_stream(purpose, self.make_cmd_control_stream_callback())
            .await?;
        log::debug!(
            "sn server cmd accept loop started local_id={}",
            self.local_identity.get_id()
        );
        Ok(())
    }

    fn make_cmd_control_stream_callback(&self) -> crate::ttp::TtpIncomingControlStreamCallback {
        let service = self.service.clone();
        Arc::new(move |accepted| {
            let service = service.clone();
            Box::pin(async move {
                SnServer::handle_accepted_cmd_stream(service, accepted).await;
            }) as crate::ttp::TtpIncomingControlStreamCallbackFuture
        })
    }

    async fn handle_accepted_cmd_stream(
        service: SnServiceRef,
        accepted: P2pResult<(
            crate::ttp::TtpStreamMeta,
            TunnelStreamRead,
            TunnelStreamWrite,
        )>,
    ) {
        let accepted = match accepted {
            Ok(accepted) => accepted,
            Err(err) => {
                warn!("sn server cmd accept stopped: {:?}", err);
                return;
            }
        };
        let tunnel = Self::into_cmd_tunnel(accepted);
        Executor::spawn(async move {
            if let Err(err) = service.handle_tunnel(tunnel).await {
                error!("sn server handle cmd tunnel failed: {:?}", err);
            }
        });
    }

    fn into_cmd_tunnel(
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

    pub fn stop(&self) {
        self.stopped.store(true, atomic::Ordering::Relaxed);
        self.abort_accept_tasks();
    }

    pub fn is_stopped(&self) -> bool {
        self.stopped.load(atomic::Ordering::Relaxed)
    }

    fn abort_accept_tasks(&self) {
        if let Some(task) = self.cmd_accept_task.lock().unwrap().take() {
            task.abort();
        }
        for task in self.nat_probe_tasks.lock().unwrap().drain(..) {
            task.abort();
        }
    }
}

impl Drop for SnServer {
    fn drop(&mut self) {
        self.abort_accept_tasks();
    }
}

// #[async_trait::async_trait]
// impl TcpListenerEventListener for SnServer {
//     async fn on_new_connection(&self, socket: TCPSocket) -> BdtResult<()> {
//         self.handle(socket).await
//     }
// }

pub struct SnServiceConfig {
    local_identity: P2pIdentityRef,
    identity_factory: P2pIdentityFactoryRef,
    cert_factory: P2pIdentityCertFactoryRef,
    connection_validator: SnConnectionValidatorRef,
    inter_service_validator: SnInterServiceValidatorRef,
    owner_client_membership: Option<OwnerMembership>,
    owner_client_override: Option<OwnerDirectoryClientRef>,
    quic_congestion_algorithm: QuicCongestionAlgorithm,
    reuse_address: bool,
    server_runtime: ServerRuntime,
    nat_probe_ports: Vec<u16>,
}

impl SnServiceConfig {
    pub fn new(
        local_identity: P2pIdentityRef,
        identity_factory: P2pIdentityFactoryRef,
        cert_factory: P2pIdentityCertFactoryRef,
        server_runtime: ServerRuntime,
    ) -> Self {
        Self {
            local_identity,
            identity_factory,
            cert_factory,
            connection_validator: allow_all_sn_connection_validator(),
            inter_service_validator: allow_all_sn_inter_service_validator(),
            owner_client_membership: None,
            owner_client_override: None,
            quic_congestion_algorithm: QuicCongestionAlgorithm::Bbr,
            reuse_address: false,
            server_runtime,
            nat_probe_ports: Vec::new(),
        }
    }

    pub fn set_connection_validator(mut self, validator: SnConnectionValidatorRef) -> Self {
        self.connection_validator = validator;
        self
    }

    pub fn set_owner_client_membership(mut self, owner_client_membership: OwnerMembership) -> Self {
        self.owner_client_membership = Some(owner_client_membership);
        self
    }

    #[cfg(test)]
    pub(crate) fn set_owner_client_for_tests(
        mut self,
        owner_client: OwnerDirectoryClientRef,
    ) -> Self {
        self.owner_client_override = Some(owner_client);
        self
    }

    pub fn set_inter_service_validator(mut self, validator: SnInterServiceValidatorRef) -> Self {
        self.inter_service_validator = validator;
        self
    }

    pub fn set_quic_congestion_algorithm(
        mut self,
        quic_algorithm: QuicCongestionAlgorithm,
    ) -> Self {
        self.quic_congestion_algorithm = quic_algorithm;
        self
    }

    pub fn set_reuse_address(mut self, reuse_address: bool) -> Self {
        self.reuse_address = reuse_address;
        self
    }

    pub fn set_server_runtime(mut self, server_runtime: ServerRuntime) -> Self {
        self.server_runtime = server_runtime;
        self
    }

    pub fn set_nat_probe_ports(mut self, ports: Vec<u16>) -> Self {
        self.nat_probe_ports = ports;
        self
    }
}

fn unique_static_wan_ipv4(identity: &P2pIdentityRef) -> Option<Ipv4Addr> {
    let mut addresses = identity
        .endpoints()
        .into_iter()
        .filter(|endpoint| endpoint.is_static_wan())
        .filter_map(|endpoint| match endpoint.addr() {
            SocketAddr::V4(address) => Some(*address.ip()),
            SocketAddr::V6(_) => None,
        })
        .collect::<Vec<_>>();
    addresses.sort_unstable();
    addresses.dedup();
    (addresses.len() == 1).then(|| addresses[0])
}

pub async fn create_sn_service(config: SnServiceConfig) -> P2pResult<SnServerRef> {
    let nat_probe_advertised_ipv4 = unique_static_wan_ipv4(&config.local_identity);
    validate_nat_probe_config(config.nat_probe_ports.as_slice(), nat_probe_advertised_ipv4)?;
    let service = SnServer::new(
        config.local_identity,
        config.identity_factory,
        config.cert_factory,
        config.connection_validator,
        config.inter_service_validator,
        config.owner_client_membership,
        config.owner_client_override,
        config.quic_congestion_algorithm,
        config.reuse_address,
        config.server_runtime,
        config.nat_probe_ports,
        nat_probe_advertised_ipv4,
    )
    .await;
    Ok(service)
}

fn validate_nat_probe_config(ports: &[u16], advertised_ipv4: Option<Ipv4Addr>) -> P2pResult<()> {
    if ports.is_empty() {
        return Ok(());
    }
    if ports.len() < 2 || ports.len() > MAX_NAT_PROBE_ENDPOINTS {
        return Err(p2p_err!(
            P2pErrorCode::InvalidParam,
            "NAT probe requires between 2 and {} ports",
            MAX_NAT_PROBE_ENDPOINTS
        ));
    }
    let mut unique = HashSet::with_capacity(ports.len());
    if ports.iter().any(|port| *port == 0 || !unique.insert(*port)) {
        return Err(p2p_err!(
            P2pErrorCode::InvalidParam,
            "NAT probe ports must be non-zero and unique"
        ));
    }
    let Some(ip) = advertised_ipv4 else {
        return Err(p2p_err!(
            P2pErrorCode::InvalidParam,
            "NAT probe ports require one advertised static-WAN IPv4"
        ));
    };
    if ip.is_unspecified() || ip.is_multicast() || ip.is_broadcast() {
        return Err(p2p_err!(
            P2pErrorCode::InvalidParam,
            "NAT probe advertised IPv4 is not usable"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod nat_probe_config_tests {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/unit/sn_tests/service/service/nat_probe_config_tests.rs"
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{P2pErrorCode, p2p_err};
    use crate::executor::Executor;
    use crate::networks::{
        IncomingTunnelCallback, Tunnel, TunnelListenerInfo, TunnelNetwork, TunnelNetworkRef,
        TunnelState, TunnelStreamRead, TunnelStreamWrite,
    };
    use crate::p2p_identity::{
        EncodedP2pIdentity, EncodedP2pIdentityCert, P2pIdentity, P2pIdentityCert,
        P2pIdentityCertFactory, P2pIdentityCertRef, P2pIdentityFactory, P2pIdentityRef,
        P2pSignature, P2pSn,
    };
    use crate::sn::directory::server::OwnerServingEndpoint;
    use crate::sn::directory::{
        OwnerDirectoryClient, OwnerDirectoryServer, OwnerDirectoryServerRef,
    };
    use crate::tls::DefaultTlsServerCertResolver;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream, WriteHalf, split};
    use tokio::sync::{Mutex as AsyncMutex, mpsc};
    use tokio::time::{Duration, timeout};

    const TEST_CHANNEL_CAPACITY: usize = 8;

    struct DummyIdentity {
        id: P2pId,
        name: String,
        endpoints: Vec<Endpoint>,
    }

    impl P2pIdentity for DummyIdentity {
        fn get_identity_cert(&self) -> P2pResult<P2pIdentityCertRef> {
            Ok(Arc::new(TestIdentityCert {
                id: self.id.clone(),
                encoded: self.id.as_slice().to_vec(),
            }))
        }

        fn get_id(&self) -> P2pId {
            self.id.clone()
        }

        fn get_name(&self) -> String {
            self.name.clone()
        }

        fn sign_type(&self) -> crate::p2p_identity::P2pIdentitySignType {
            crate::p2p_identity::P2pIdentitySignType::Rsa
        }

        fn sign(&self, _message: &[u8]) -> P2pResult<P2pSignature> {
            Ok(vec![])
        }

        fn get_encoded_identity(&self) -> P2pResult<EncodedP2pIdentity> {
            Ok(vec![])
        }

        fn endpoints(&self) -> Vec<Endpoint> {
            self.endpoints.clone()
        }

        fn update_endpoints(&self, eps: Vec<Endpoint>) -> P2pIdentityRef {
            Arc::new(Self {
                id: self.id.clone(),
                name: self.name.clone(),
                endpoints: eps,
            })
        }
    }

    struct TestIdentityCert {
        id: P2pId,
        encoded: EncodedP2pIdentityCert,
    }

    impl P2pIdentityCert for TestIdentityCert {
        fn get_id(&self) -> P2pId {
            self.id.clone()
        }

        fn get_name(&self) -> String {
            "test-cert".to_owned()
        }

        fn sign_type(&self) -> crate::p2p_identity::P2pIdentitySignType {
            crate::p2p_identity::P2pIdentitySignType::Rsa
        }

        fn verify(&self, _message: &[u8], _sign: &P2pSignature) -> bool {
            true
        }

        fn verify_cert(&self, _name: &str) -> bool {
            true
        }

        fn get_encoded_cert(&self) -> P2pResult<EncodedP2pIdentityCert> {
            Ok(self.encoded.clone())
        }

        fn endpoints(&self) -> Vec<Endpoint> {
            vec![]
        }

        fn sn_list(&self) -> Vec<P2pSn> {
            vec![]
        }

        fn update_endpoints(&self, _eps: Vec<Endpoint>) -> P2pIdentityCertRef {
            Arc::new(Self {
                id: self.id.clone(),
                encoded: self.encoded.clone(),
            })
        }
    }

    struct TestIdentityCertFactory;

    impl P2pIdentityCertFactory for TestIdentityCertFactory {
        fn create(&self, cert: &EncodedP2pIdentityCert) -> P2pResult<P2pIdentityCertRef> {
            Ok(Arc::new(TestIdentityCert {
                id: P2pId::from(cert.clone()),
                encoded: cert.clone(),
            }))
        }
    }

    struct TestIdentityFactory;

    impl P2pIdentityFactory for TestIdentityFactory {
        fn create(&self, id: &EncodedP2pIdentity) -> P2pResult<P2pIdentityRef> {
            Ok(Arc::new(DummyIdentity {
                id: P2pId::from(id.clone()),
                name: "test-identity".to_owned(),
                endpoints: vec![],
            }))
        }
    }

    struct TestSnConnectionValidator {
        decision: ValidateResult,
        last_ctx: Mutex<Option<SnConnectionValidateContext>>,
    }

    impl TestSnConnectionValidator {
        fn new(decision: ValidateResult) -> Arc<Self> {
            Arc::new(Self {
                decision,
                last_ctx: Mutex::new(None),
            })
        }

        fn last_ctx(&self) -> Option<SnConnectionValidateContext> {
            self.last_ctx.lock().unwrap().clone()
        }
    }

    #[async_trait::async_trait]
    impl SnConnectionValidator for TestSnConnectionValidator {
        async fn validate(&self, ctx: &SnConnectionValidateContext) -> P2pResult<ValidateResult> {
            *self.last_ctx.lock().unwrap() = Some(ctx.clone());
            match &self.decision {
                ValidateResult::Accept => Ok(ValidateResult::Accept),
                ValidateResult::Reject(reason) => Ok(ValidateResult::Reject(reason.clone())),
            }
        }
    }

    struct TestInterServiceValidator {
        decision: ValidateResult,
        commands: Mutex<Vec<InterSnCommandContext>>,
    }

    impl TestInterServiceValidator {
        fn new(decision: ValidateResult) -> Arc<Self> {
            Arc::new(Self {
                decision,
                commands: Mutex::new(Vec::new()),
            })
        }

        fn command_count(&self) -> usize {
            self.commands.lock().unwrap().len()
        }
    }

    #[async_trait::async_trait]
    impl crate::sn::inter_sn::SnInterServiceValidator for TestInterServiceValidator {
        async fn validate_connection(
            &self,
            _ctx: &InterSnConnectionContext,
        ) -> P2pResult<ValidateResult> {
            match &self.decision {
                ValidateResult::Accept => Ok(ValidateResult::Accept),
                ValidateResult::Reject(reason) => Ok(ValidateResult::Reject(reason.clone())),
            }
        }

        async fn validate_command(&self, ctx: &InterSnCommandContext) -> P2pResult<ValidateResult> {
            self.commands.lock().unwrap().push(ctx.clone());
            match &self.decision {
                ValidateResult::Accept => Ok(ValidateResult::Accept),
                ValidateResult::Reject(reason) => Ok(ValidateResult::Reject(reason.clone())),
            }
        }
    }

    struct DirectOwnerDirectoryClient {
        owner: OwnerDirectoryServerRef,
    }

    #[async_trait::async_trait]
    impl OwnerDirectoryClient for DirectOwnerDirectoryClient {
        async fn publish_serving_lease(
            &self,
            local_sn_id: P2pId,
            peer_id: P2pId,
            sequence: u64,
        ) -> P2pResult<()> {
            let lease = ServingLease {
                peer_id: peer_id.clone(),
                serving_sn_id: local_sn_id.clone(),
                sequence,
                expires_at: bucky_time_now() + 60_000_000,
            };
            if self
                .owner
                .publish_lease_from_serving_sn(local_sn_id, lease)
                .await?
            {
                Ok(())
            } else {
                Err(p2p_err!(
                    P2pErrorCode::NotFound,
                    "owner rejected serving lease peer={}",
                    peer_id
                ))
            }
        }

        async fn query_serving_leases(
            &self,
            local_sn_id: &P2pId,
            peer_id: &P2pId,
        ) -> P2pResult<Vec<ServingLease>> {
            self.owner
                .query_leases_from_serving_sn(local_sn_id.clone(), peer_id.clone())
                .await
        }
    }

    fn direct_owner_client(owner: OwnerDirectoryServerRef) -> OwnerDirectoryClientRef {
        Arc::new(DirectOwnerDirectoryClient { owner })
    }

    struct FakeTunnel {
        tunnel_id: crate::types::TunnelId,
        candidate_id: crate::types::TunnelCandidateId,
        local_id: P2pId,
        remote_id: P2pId,
        local_ep: Endpoint,
        remote_ep: Endpoint,
        incoming_rx: Arc<
            AsyncMutex<
                mpsc::UnboundedReceiver<(
                    crate::networks::TunnelPurpose,
                    TunnelStreamRead,
                    TunnelStreamWrite,
                )>,
            >,
        >,
        incoming_control_rx: Arc<
            AsyncMutex<
                mpsc::UnboundedReceiver<(
                    crate::networks::TunnelPurpose,
                    TunnelStreamRead,
                    TunnelStreamWrite,
                )>,
            >,
        >,
    }

    impl FakeTunnel {
        fn new(
            local_id: P2pId,
            remote_id: P2pId,
            local_ep: Endpoint,
            remote_ep: Endpoint,
        ) -> (
            Arc<Self>,
            mpsc::UnboundedSender<(
                crate::networks::TunnelPurpose,
                TunnelStreamRead,
                TunnelStreamWrite,
            )>,
            mpsc::UnboundedSender<(
                crate::networks::TunnelPurpose,
                TunnelStreamRead,
                TunnelStreamWrite,
            )>,
        ) {
            let (tx, rx) = mpsc::unbounded_channel();
            let (control_tx, control_rx) = mpsc::unbounded_channel();
            (
                Arc::new(Self {
                    tunnel_id: crate::types::TunnelId::from(1),
                    candidate_id: crate::types::TunnelCandidateId::from(1),
                    local_id,
                    remote_id,
                    local_ep,
                    remote_ep,
                    incoming_rx: Arc::new(AsyncMutex::new(rx)),
                    incoming_control_rx: Arc::new(AsyncMutex::new(control_rx)),
                }),
                tx,
                control_tx,
            )
        }
    }

    #[async_trait::async_trait]
    impl Tunnel for FakeTunnel {
        fn tunnel_id(&self) -> crate::types::TunnelId {
            self.tunnel_id
        }

        fn candidate_id(&self) -> crate::types::TunnelCandidateId {
            self.candidate_id
        }

        fn form(&self) -> crate::networks::TunnelForm {
            crate::networks::TunnelForm::Active
        }

        fn is_reverse(&self) -> bool {
            false
        }

        fn protocol(&self) -> Protocol {
            self.local_ep.protocol()
        }

        fn local_id(&self) -> P2pId {
            self.local_id.clone()
        }

        fn remote_id(&self) -> P2pId {
            self.remote_id.clone()
        }

        fn local_ep(&self) -> Option<Endpoint> {
            Some(self.local_ep)
        }

        fn remote_ep(&self) -> Option<Endpoint> {
            Some(self.remote_ep)
        }

        fn state(&self) -> TunnelState {
            TunnelState::Connected
        }

        fn is_closed(&self) -> bool {
            false
        }

        fn close(&self) -> P2pResult<()> {
            Ok(())
        }

        async fn listen_stream(
            &self,
            _vports: crate::networks::ListenVPortsRef,
            callback: crate::networks::IncomingStreamCallback,
        ) -> P2pResult<()> {
            let incoming_rx = self.incoming_rx.clone();
            tokio::spawn(async move {
                let mut incoming_rx = incoming_rx.lock().await;
                while let Some((purpose, read, write)) = incoming_rx.recv().await {
                    callback(Ok((purpose, read, write))).await;
                }
            });
            Ok(())
        }

        async fn listen_datagram(
            &self,
            _vports: crate::networks::ListenVPortsRef,
            _callback: crate::networks::IncomingDatagramCallback,
        ) -> P2pResult<()> {
            Ok(())
        }

        async fn listen_control_stream(
            &self,
            _purposes: crate::networks::ListenVPortsRef,
            callback: crate::networks::IncomingControlStreamCallback,
        ) -> P2pResult<()> {
            let incoming_control_rx = self.incoming_control_rx.clone();
            tokio::spawn(async move {
                let mut incoming_control_rx = incoming_control_rx.lock().await;
                while let Some((purpose, read, write)) = incoming_control_rx.recv().await {
                    callback(Ok((purpose, read, write))).await;
                }
            });
            Ok(())
        }

        async fn open_stream(
            &self,
            _purpose: crate::networks::TunnelPurpose,
        ) -> P2pResult<(TunnelStreamRead, TunnelStreamWrite)> {
            Err(p2p_err!(P2pErrorCode::NotSupport, "unused in test"))
        }

        async fn open_datagram(
            &self,
            _purpose: crate::networks::TunnelPurpose,
        ) -> P2pResult<crate::networks::TunnelDatagramWrite> {
            Err(p2p_err!(P2pErrorCode::NotSupport, "unused in test"))
        }
    }

    struct FakeTunnelNetwork {
        protocol: Protocol,
        rx: AsyncMutex<Option<mpsc::UnboundedReceiver<P2pResult<crate::networks::TunnelRef>>>>,
        tx: mpsc::UnboundedSender<P2pResult<crate::networks::TunnelRef>>,
        infos: Mutex<Vec<TunnelListenerInfo>>,
    }

    impl FakeTunnelNetwork {
        fn new(protocol: Protocol) -> Arc<Self> {
            let (tx, rx) = mpsc::unbounded_channel();
            Arc::new(Self {
                protocol,
                rx: AsyncMutex::new(Some(rx)),
                tx,
                infos: Mutex::new(Vec::new()),
            })
        }

        fn push_tunnel(&self, tunnel: crate::networks::TunnelRef) {
            let _ = self.tx.send(Ok(tunnel));
        }
    }

    #[async_trait::async_trait]
    impl TunnelNetwork for FakeTunnelNetwork {
        fn protocol(&self) -> Protocol {
            self.protocol
        }

        fn is_udp(&self) -> bool {
            self.protocol == Protocol::Quic
        }

        async fn listen(
            &self,
            local: &Endpoint,
            _out: Option<Endpoint>,
            mapping_port: Option<u16>,
            on_incoming_tunnel: IncomingTunnelCallback,
        ) -> P2pResult<()> {
            *self.infos.lock().unwrap() = vec![TunnelListenerInfo {
                local: *local,
                mapping_port,
            }];
            let mut rx =
                self.rx.lock().await.take().ok_or_else(|| {
                    p2p_err!(P2pErrorCode::ErrorState, "fake listener already used")
                })?;
            Executor::spawn_ok(async move {
                loop {
                    match rx.recv().await {
                        Some(result) => on_incoming_tunnel(result).await,
                        None => break,
                    }
                }
            });
            Ok(())
        }

        async fn close_all_listener(&self) -> P2pResult<()> {
            Ok(())
        }

        fn listener_infos(&self) -> Vec<TunnelListenerInfo> {
            self.infos.lock().unwrap().clone()
        }

        async fn create_tunnel_with_intent(
            &self,
            _local_identity: &P2pIdentityRef,
            _remote: &Endpoint,
            _remote_id: &P2pId,
            _remote_name: Option<String>,
            _intent: crate::networks::TunnelConnectIntent,
        ) -> P2pResult<crate::networks::TunnelRef> {
            Err(p2p_err!(P2pErrorCode::NotSupport, "unused in test"))
        }

        async fn create_tunnel_with_local_ep_and_intent(
            &self,
            _local_identity: &P2pIdentityRef,
            _local_ep: &Endpoint,
            _remote: &Endpoint,
            _remote_id: &P2pId,
            _remote_name: Option<String>,
            _intent: crate::networks::TunnelConnectIntent,
        ) -> P2pResult<crate::networks::TunnelRef> {
            Err(p2p_err!(P2pErrorCode::NotSupport, "unused in test"))
        }
    }

    fn test_identity_for_id(id: P2pId, endpoints: Vec<Endpoint>) -> P2pIdentityRef {
        Arc::new(DummyIdentity {
            id,
            name: "local-test".to_owned(),
            endpoints,
        })
    }

    fn test_identity(local_ep: Endpoint) -> P2pIdentityRef {
        test_identity_for_id(P2pId::from(vec![1u8; 32]), vec![local_ep])
    }

    fn remote_id() -> P2pId {
        P2pId::from(vec![2u8; 32])
    }

    fn test_id(seed: u8) -> P2pId {
        P2pId::from(vec![seed; 32])
    }

    fn make_stream_pair() -> (
        (TunnelStreamRead, TunnelStreamWrite),
        WriteHalf<DuplexStream>,
    ) {
        let (test_end, tunnel_end) = tokio::io::duplex(64);
        let (_test_read, test_write) = split(test_end);
        let (tunnel_read, tunnel_write) = split(tunnel_end);
        ((Box::pin(tunnel_read), Box::pin(tunnel_write)), test_write)
    }

    fn test_sn_service(validator: SnConnectionValidatorRef) -> SnServiceRef {
        let service = SnService::new(Arc::new(TestIdentityCertFactory), validator);
        service.set_local_identity(test_identity_for_id(test_id(9), vec![]));
        service
    }

    fn test_sn_service_with_directory(
        local_sn_id: P2pId,
        membership: Option<OwnerMembership>,
        inter_validator: SnInterServiceValidatorRef,
    ) -> SnServiceRef {
        let owner_client = membership
            .map(|membership| StaticOwnerDirectoryClient::new(membership, None))
            .unwrap_or_else(noop_owner_directory_client);
        let service = SnService::new_with_options(
            Arc::new(TestIdentityCertFactory),
            allow_all_sn_connection_validator(),
            inter_validator,
            owner_client,
            None,
            Some(local_sn_id.clone()),
        );
        service.set_local_identity(test_identity_for_id(local_sn_id, vec![]));
        service
    }

    fn test_sn_service_with_owner_client(
        local_sn_id: P2pId,
        owner_client: OwnerDirectoryClientRef,
        inter_validator: SnInterServiceValidatorRef,
    ) -> SnServiceRef {
        SnService::new_with_options(
            Arc::new(TestIdentityCertFactory),
            allow_all_sn_connection_validator(),
            inter_validator,
            owner_client,
            None,
            Some(local_sn_id),
        )
    }

    fn test_owner_service(
        local_sn_id: P2pId,
        membership: OwnerMembership,
        inter_validator: SnInterServiceValidatorRef,
    ) -> OwnerDirectoryServerRef {
        OwnerDirectoryServer::new_detached(local_sn_id, membership, Some(inter_validator))
    }

    fn test_report(from_peer_id: P2pId, client_cert: EncodedP2pIdentityCert) -> ReportSn {
        ReportSn {
            protocol_version: 0,
            stack_version: 0,
            seq: 1u32.into(),
            sn_peer_id: P2pId::from(vec![9u8; 32]),
            from_peer_id: Some(from_peer_id),
            peer_info: Some(client_cert),
            send_time: 0,
            contract_id: None,
            receipt: None,
            map_ports: vec![],
            local_eps: vec![],
            net_profile: None,
            nat_probe_control_version: Some(NAT_PROBE_CONTROL_VERSION),
            nat_probe_result: None,
        }
    }

    #[test]
    fn reverse_endpoint_array_dedup_preserves_first_seen_endpoint() {
        let mut wan_ep = Endpoint::from((Protocol::Quic, "119.127.198.117:44325".parse().unwrap()));
        wan_ep.set_area(EndpointArea::Wan);
        let mut mapped_same_addr = wan_ep;
        mapped_same_addr.set_area(EndpointArea::Mapped);
        let local_ep = Endpoint::from((Protocol::Quic, "192.168.3.137:3622".parse().unwrap()));
        let ipv6_ep = Endpoint::from((
            Protocol::Quic,
            "[240e:3b1:d003:70a0:7817:42c0:a827:56b]:3622"
                .parse()
                .unwrap(),
        ));

        let mut endpoints = vec![wan_ep, wan_ep, local_ep];
        SnService::dedup_endpoints(&mut endpoints);
        SnService::extend_unique_endpoints(&mut endpoints, &[mapped_same_addr, ipv6_ep]);

        assert_eq!(endpoints, vec![wan_ep, local_ep, ipv6_ep]);
    }

    #[test]
    fn sn_observed_endpoint_matching_reported_addr_is_wan() {
        let reported = Endpoint::from((Protocol::Quic, "119.127.198.117:44325".parse().unwrap()));
        let observed = Endpoint::from((Protocol::Quic, "119.127.198.117:44325".parse().unwrap()));

        let classified = SnService::classify_observed_endpoint(observed, &[reported]);

        assert_eq!(classified.get_area(), EndpointArea::Wan);
    }

    #[test]
    fn sn_observed_endpoint_matching_reported_addr_ignores_area() {
        let mut reported =
            Endpoint::from((Protocol::Quic, "119.127.198.117:44325".parse().unwrap()));
        reported.set_area(EndpointArea::ServerReflexive);
        let observed = Endpoint::from((Protocol::Quic, "119.127.198.117:44325".parse().unwrap()));

        let classified = SnService::classify_observed_endpoint(observed, &[reported]);

        assert_eq!(classified.get_area(), EndpointArea::Wan);
    }

    #[test]
    fn sn_observed_endpoint_protocol_mismatch_is_server_reflexive() {
        let reported = Endpoint::from((Protocol::Tcp, "119.127.198.117:44325".parse().unwrap()));
        let observed = Endpoint::from((Protocol::Quic, "119.127.198.117:44325".parse().unwrap()));

        let classified = SnService::classify_observed_endpoint(observed, &[reported]);

        assert_eq!(classified.get_area(), EndpointArea::ServerReflexive);
    }

    #[test]
    fn sn_unique_endpoint_extension_keeps_wan_over_lan_duplicate() {
        let mut observed =
            Endpoint::from((Protocol::Quic, "119.127.198.117:44325".parse().unwrap()));
        observed.set_area(EndpointArea::Wan);
        let reported_lan =
            Endpoint::from((Protocol::Quic, "119.127.198.117:44325".parse().unwrap()));
        let mut endpoints = vec![observed];

        SnService::extend_unique_endpoints(&mut endpoints, &[reported_lan]);

        assert_eq!(endpoints, vec![observed]);
        assert_eq!(endpoints[0].get_area(), EndpointArea::Wan);
    }

    #[test]
    fn sn_map_port_candidate_from_observed_endpoint_is_mapped() {
        let observed = Endpoint::from((Protocol::Quic, "119.127.198.117:44325".parse().unwrap()));

        let mapped = SnService::mapped_endpoint_from_observed(&observed, Protocol::Quic, 7000);

        assert_eq!(mapped.get_area(), EndpointArea::Mapped);
        assert_eq!(mapped.protocol(), Protocol::Quic);
        assert_eq!(mapped.addr().ip(), observed.addr().ip());
        assert_eq!(mapped.addr().port(), 7000);
    }

    #[test]
    fn sn_tcp_observed_endpoint_without_map_ports_is_not_returned() {
        let observed_tcp =
            Endpoint::from((Protocol::Tcp, "119.127.198.117:44325".parse().unwrap()));
        let reported_tcp = observed_tcp;

        let endpoints =
            SnService::observed_endpoint_candidates(&[observed_tcp], &[], &[reported_tcp]);

        assert!(endpoints.is_empty());
    }

    #[test]
    fn sn_tcp_observed_endpoint_with_map_ports_returns_only_mapped_candidates() {
        let observed_tcp =
            Endpoint::from((Protocol::Tcp, "119.127.198.117:44325".parse().unwrap()));

        let endpoints = SnService::observed_endpoint_candidates(
            &[observed_tcp],
            &[(Protocol::Tcp, 7000), (Protocol::Quic, 7001)],
            &[],
        );

        let mut mapped_tcp =
            Endpoint::from((Protocol::Tcp, "119.127.198.117:7000".parse().unwrap()));
        mapped_tcp.set_area(EndpointArea::Mapped);
        let mut mapped_quic =
            Endpoint::from((Protocol::Quic, "119.127.198.117:7001".parse().unwrap()));
        mapped_quic.set_area(EndpointArea::Mapped);

        assert_eq!(endpoints, vec![mapped_tcp, mapped_quic]);
        assert!(
            endpoints
                .iter()
                .all(|ep| ep.get_area() == EndpointArea::Mapped)
        );
        assert!(endpoints.iter().all(|ep| ep.addr().port() != 44325));
    }

    #[test]
    fn sn_non_tcp_observed_endpoint_remains_direct_candidate() {
        let reported_quic =
            Endpoint::from((Protocol::Quic, "119.127.198.117:44325".parse().unwrap()));
        let observed_quic = reported_quic;

        let endpoints =
            SnService::observed_endpoint_candidates(&[observed_quic], &[], &[reported_quic]);

        assert_eq!(endpoints.len(), 1);
        assert_eq!(endpoints[0].protocol(), Protocol::Quic);
        assert_eq!(endpoints[0].addr(), observed_quic.addr());
        assert_eq!(endpoints[0].get_area(), EndpointArea::Wan);
    }

    #[test]
    fn sn_observed_endpoint_mismatch_is_server_reflexive() {
        let reported = Endpoint::from((Protocol::Quic, "192.168.1.10:44325".parse().unwrap()));
        let observed = Endpoint::from((Protocol::Quic, "119.127.198.117:44325".parse().unwrap()));

        let classified = SnService::classify_observed_endpoint(observed, &[reported]);

        assert_eq!(classified.get_area(), EndpointArea::ServerReflexive);
    }

    #[tokio::test]
    async fn sn_service_default_validator_allows_report() {
        let service = test_sn_service(allow_all_sn_connection_validator());
        let reported_peer = P2pId::from(vec![7u8; 32]);
        let peer_id = PeerId::from(reported_peer.as_slice());
        let local_id = P2pId::from(vec![9u8; 32]);

        let result = service
            .handle_report_sn(
                &local_id,
                &peer_id,
                1u32.into(),
                test_report(reported_peer.clone(), reported_peer.as_slice().to_vec()),
            )
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn sn_report_updates_local_detail_without_publishing_route() {
        let owner_sn = test_id(83);
        let serving_sn = test_id(84);
        let reported_peer = test_id(85);
        let membership =
            OwnerMembership::with_options(vec![owner_sn.clone()], 1, Duration::from_secs(60))
                .unwrap();
        let owner = test_owner_service(
            owner_sn,
            membership.clone(),
            allow_all_sn_inter_service_validator(),
        );
        let service = test_sn_service_with_directory(
            serving_sn.clone(),
            Some(membership),
            allow_all_sn_inter_service_validator(),
        );
        let peer_id = PeerId::from(reported_peer.as_slice());

        service
            .handle_report_sn(
                &serving_sn,
                &peer_id,
                1u32.into(),
                test_report(reported_peer.clone(), reported_peer.as_slice().to_vec()),
            )
            .await
            .unwrap();

        assert!(service.peer_manager().find_peer(&reported_peer).is_some());
        assert!(owner.query_serving_leases(&reported_peer).is_empty());
    }

    #[tokio::test]
    async fn sn_service_rejects_report_when_validator_rejects() {
        let validator =
            TestSnConnectionValidator::new(ValidateResult::Reject("blocked-by-test".to_owned()));
        let service = test_sn_service(validator.clone());
        let reported_peer = P2pId::from(vec![8u8; 32]);
        let client_peer = P2pId::from(vec![3u8; 32]);
        let peer_id = PeerId::from(client_peer.as_slice());
        let local_id = P2pId::from(vec![9u8; 32]);

        let err = service
            .handle_report_sn(
                &local_id,
                &peer_id,
                2u32.into(),
                test_report(reported_peer.clone(), client_peer.as_slice().to_vec()),
            )
            .await
            .unwrap_err();

        assert_eq!(err.code(), P2pErrorCode::PermissionDenied);
        assert!(service.peer_manager().find_peer(&reported_peer).is_none());
        let ctx = validator.last_ctx().unwrap();
        assert_eq!(ctx.client_id, client_peer);
        assert_eq!(ctx.client_cert, client_peer.as_slice().to_vec());
    }

    #[tokio::test]
    async fn sn_service_rejects_report_when_client_cert_mismatches_peer_id() {
        let validator = TestSnConnectionValidator::new(ValidateResult::Accept);
        let service = test_sn_service(validator.clone());
        let reported_peer = P2pId::from(vec![8u8; 32]);
        let client_peer = P2pId::from(vec![3u8; 32]);
        let mismatched_cert = P2pId::from(vec![4u8; 32]);
        let peer_id = PeerId::from(client_peer.as_slice());
        let local_id = P2pId::from(vec![9u8; 32]);

        let err = service
            .handle_report_sn(
                &local_id,
                &peer_id,
                2u32.into(),
                test_report(reported_peer.clone(), mismatched_cert.as_slice().to_vec()),
            )
            .await
            .unwrap_err();

        assert_eq!(err.code(), P2pErrorCode::PermissionDenied);
        assert!(service.peer_manager().find_peer(&reported_peer).is_none());
        assert!(validator.last_ctx().is_none());
    }

    #[tokio::test]
    async fn sn_distributed_directory_inter_validator_reject_blocks_owner_write() {
        let owner_sn = test_id(70);
        let serving_sn = test_id(71);
        let peer = test_id(72);
        let validator =
            TestInterServiceValidator::new(ValidateResult::Reject("blocked-by-test".to_owned()));
        let membership =
            OwnerMembership::with_options(vec![owner_sn.clone()], 1, Duration::from_secs(60))
                .unwrap();
        let owner = test_owner_service(owner_sn, membership, validator.clone());
        let lease = ServingLease {
            peer_id: peer.clone(),
            serving_sn_id: serving_sn.clone(),
            sequence: 1,
            expires_at: bucky_time_now() + 60_000_000,
        };

        let err = owner
            .publish_lease_from_sn(serving_sn, lease)
            .await
            .unwrap_err();

        assert_eq!(err.code(), P2pErrorCode::PermissionDenied);
        assert!(owner.query_serving_leases(&peer).is_empty());
        assert_eq!(validator.command_count(), 0);
    }

    #[tokio::test]
    async fn sn_directory_client_server_boundary_keeps_sn_service_serving_only() {
        let owner_sn = test_id(74);
        let local_sn = test_id(75);
        let peer = test_id(76);
        let membership =
            OwnerMembership::with_options(vec![owner_sn.clone()], 1, Duration::from_secs(60))
                .unwrap();
        let owner = test_owner_service(
            owner_sn,
            membership.clone(),
            allow_all_sn_inter_service_validator(),
        );
        owner
            .service()
            .election_node()
            .renew_serving_session(
                local_sn.clone(),
                0,
                Duration::from_secs(60),
                bucky_time_now(),
            )
            .await
            .unwrap();
        let service = test_sn_service_with_owner_client(
            local_sn.clone(),
            direct_owner_client(owner.clone()),
            allow_all_sn_inter_service_validator(),
        );

        service
            .owner_client
            .publish_serving_lease(local_sn.clone(), peer.clone(), 3)
            .await
            .unwrap();

        let leases = owner.query_serving_leases(&peer);
        assert_eq!(leases.len(), 1);
        assert_eq!(leases[0].serving_sn_id, local_sn);
        assert_eq!(leases[0].sequence, 3);
        assert!(service.local_peer_detail(&peer).is_none());

        service.peer_mgr.add_peer_info(
            peer.clone(),
            Arc::new(TestIdentityCert {
                id: peer.clone(),
                encoded: peer.as_slice().to_vec(),
            }),
        );
        assert_eq!(
            service.local_peer_detail(&peer).unwrap().peer_info,
            peer.as_slice().to_vec()
        );
    }

    #[tokio::test]
    async fn sn_distributed_directory_publish_query_requires_explicit_owner_client() {
        let owner_sn = test_id(80);
        let serving_sn = test_id(81);
        let peer = test_id(82);
        let membership =
            OwnerMembership::with_options(vec![owner_sn.clone()], 1, Duration::from_secs(60))
                .unwrap();
        let owner = test_owner_service(
            owner_sn.clone(),
            membership.clone(),
            allow_all_sn_inter_service_validator(),
        );
        owner
            .service()
            .election_node()
            .renew_serving_session(
                serving_sn.clone(),
                0,
                Duration::from_secs(60),
                bucky_time_now(),
            )
            .await
            .unwrap();
        let serving = test_sn_service_with_owner_client(
            serving_sn.clone(),
            direct_owner_client(owner.clone()),
            allow_all_sn_inter_service_validator(),
        );
        serving.peer_mgr.add_peer_info(
            peer.clone(),
            Arc::new(TestIdentityCert {
                id: peer.clone(),
                encoded: peer.as_slice().to_vec(),
            }),
        );

        serving
            .owner_client
            .publish_serving_lease(serving_sn.clone(), peer.clone(), 7)
            .await
            .unwrap();

        let leases = owner.query_serving_leases(&peer);
        assert_eq!(leases.len(), 1);
        assert_eq!(leases[0].serving_sn_id, serving_sn);
        assert_eq!(leases[0].sequence, 7);

        let detail = serving
            .query_detail_from_sn(owner_sn, peer.clone())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(detail.peer_info, peer.as_slice().to_vec());
    }

    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/unit/sn_tests/service/distributed_nat_profile_tests.rs"
    ));

    mod protocol_version_query_tests {
        use super::*;

        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/unit/sn_tests/service/protocol_version_query_tests.rs"
        ));
    }

    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/unit/sn_tests/service/service/nat_probe_scheduler_tests.rs"
    ));

    #[tokio::test]
    async fn sn_server_wraps_sn_control_stream_into_cmd_tunnel() {
        let local_ep = Endpoint::from((Protocol::Quic, "127.0.0.1:23101".parse().unwrap()));
        let remote_ep = Endpoint::from((Protocol::Quic, "127.0.0.1:23102".parse().unwrap()));
        let identity = test_identity(local_ep);
        let fake_network = FakeTunnelNetwork::new(Protocol::Quic);
        let net_manager = NetManager::new(
            vec![fake_network.clone() as TunnelNetworkRef],
            DefaultTlsServerCertResolver::new(),
        )
        .unwrap();
        net_manager.listen(&[local_ep], None).await.unwrap();
        let ttp_server = TtpServer::new(identity.clone(), net_manager.clone()).unwrap();
        let (accepted_tx, mut accepted_rx) = mpsc::channel(1);
        let callback: crate::ttp::TtpIncomingControlStreamCallback = Arc::new(move |accepted| {
            let accepted_tx = accepted_tx.clone();
            Box::pin(async move {
                let _ = accepted_tx.send(accepted).await;
            }) as crate::ttp::TtpIncomingControlStreamCallbackFuture
        });
        ttp_server
            .listen_control_stream(sn_cmd_purpose().unwrap(), callback)
            .await
            .unwrap();

        let (tunnel, _stream_tx, control_tx) =
            FakeTunnel::new(identity.get_id(), remote_id(), local_ep, remote_ep);
        let ((read, write), mut remote_write) = make_stream_pair();
        control_tx
            .send((sn_cmd_purpose().unwrap(), read, write))
            .unwrap();
        fake_network.push_tunnel(tunnel);

        let accepted = timeout(Duration::from_secs(1), accepted_rx.recv())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let cmd_tunnel = SnServer::into_cmd_tunnel(accepted);
        let (mut cmd_read, _cmd_write) = cmd_tunnel.split();

        remote_write.write_all(b"ctrl").await.unwrap();
        let mut buf = [0u8; 4];
        cmd_read.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"ctrl");
    }
}
