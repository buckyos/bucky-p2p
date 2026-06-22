use super::{call_stub::CallStub, peer_manager::PeerManager};
use crate::endpoint::{Endpoint, EndpointArea, Protocol, endpoints_to_string};
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
use crate::sn::directory::{
    OwnerDirectoryClientRef, OwnerMembership, ServingLease, StaticOwnerDirectoryClient,
    noop_owner_directory_client,
};
use crate::sn::inter_sn::{
    InterSnCommand, InterSnCommandContext, InterSnConnectionContext, InterSnPeer, RelayCallOutcome,
    ServingPeerDetail, SnInterServiceValidatorRef, TtpInterSnClient, TtpInterSnClientRef,
    allow_all_sn_inter_service_validator, require_accept,
};
use crate::sn::protocol::{v0::*, *};
use crate::sn::service::peer_manager::PeerManagerRef;
use crate::sn::types::{CmdTunnelId, SnCmdHeader, SnTunnelRead, SnTunnelWrite, sn_cmd_purpose};
use crate::tls::{DefaultTlsServerCertResolver, TlsServerCertResolver, init_tls};
use crate::ttp::{TtpClient, TtpConnector, TtpNode, TtpPortListener, TtpServer, TtpServerRef};
use crate::types::{SequenceGenerator, Timestamp, TunnelId};
use bucky_raw_codec::{RawConvertTo, RawFrom};
use bucky_time::bucky_time_now;
use log::*;
use sfo_cmd_server::errors::{CmdErrorCode, CmdResult, into_cmd_err};
use sfo_cmd_server::server::{CmdServer, CmdTunnelService, DefaultCmdServerService};
use sfo_cmd_server::{CmdBody, CmdTunnel, PeerId};
use sfo_reuseport::{ServerRuntime, ServerRuntimeConfig};
use std::{
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

type SnCmdService = DefaultCmdServerService<(), SnTunnelRead, SnTunnelWrite, u16, u8>;
pub type SnCmdServiceRef = Arc<SnCmdService>;
pub type SnServiceRef = Arc<SnService>;
pub type SnServerRef = Arc<SnServer>;

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

pub struct SnService {
    seq_generator: Arc<SequenceGenerator>,
    peer_mgr: PeerManagerRef,
    call_stub: CallStub,
    cert_factory: P2pIdentityCertFactoryRef,
    cmd_server: SnCmdServiceRef,
    connection_validator: SnConnectionValidatorRef,
    inter_service_validator: SnInterServiceValidatorRef,
    inter_sn_client: Mutex<Option<TtpInterSnClientRef>>,
    owner_client: OwnerDirectoryClientRef,
    local_sn_id: Option<P2pId>,
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
            cmd_version: 0,
        });
        service.register_sn_cmd_handler();
        service
    }

    pub fn get_cmd_server(&self) -> &SnCmdServiceRef {
        &self.cmd_server
    }

    pub fn set_inter_sn_client(&self, inter_sn_client: Option<TtpInterSnClientRef>) {
        *self.inter_sn_client.lock().unwrap() = inter_sn_client;
    }

    fn inter_sn_client(&self) -> Option<TtpInterSnClientRef> {
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
                    service
                        .handle_call(call_req, &local_id, &peer_id, tunnel_id, bucky_time_now())
                        .await
                        .map_err(into_cmd_err!(CmdErrorCode::Failed, "handle sn call failed"))?;
                    Ok(None)
                }
            },
        );

        let service = self.clone();
        self.cmd_server.register_cmd_handler(
            PackageCmdCode::SnCalledResp as u8,
            move |_local_id: PeerId,
                  _peer_id: PeerId,
                  _tunnel_id: CmdTunnelId,
                  _header: SnCmdHeader,
                  mut cmd_body: CmdBody| {
                let service = service.clone();
                async move {
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
                    service
                        .handle_report_sn(&local_id, &peer_id, tunnel_id, report_sn)
                        .await
                        .map_err(into_cmd_err!(
                            CmdErrorCode::Failed,
                            "handle report sn failed"
                        ))?;
                    Ok(None)
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
                    service
                        .handle_query_sn(&local_id, &peer_id, tunnel_id, query)
                        .await;
                    Ok(None)
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

    fn reported_endpoints_for_peer(
        peer: &crate::sn::service::peer_manager::CachedPeerInfo,
    ) -> Vec<Endpoint> {
        let mut endpoints = peer.desc.endpoints();
        Self::extend_unique_endpoints(&mut endpoints, peer.local_eps.as_slice());
        endpoints
    }

    fn local_peer_detail(&self, peer_id: &P2pId) -> Option<ServingPeerDetail> {
        self.peer_manager().find_peer(peer_id).and_then(|peer| {
            let mut endpoints = Self::reported_endpoints_for_peer(&peer);
            Self::dedup_endpoints(&mut endpoints);
            peer.desc
                .get_encoded_cert()
                .ok()
                .map(|peer_info| ServingPeerDetail {
                    peer_info,
                    endpoints,
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
    ) -> Vec<ServingPeerDetail> {
        let mut details = Vec::new();
        for lease in self.query_serving_leases(local_sn_id, peer_id).await {
            if lease.serving_sn_id == *local_sn_id {
                continue;
            }
            if let Some(inter_sn_client) = self.inter_sn_client() {
                match inter_sn_client
                    .query_detail_from_sn(&lease.serving_sn_id, peer_id.clone())
                    .await
                {
                    Ok(Some(detail)) => {
                        details.push(detail);
                        continue;
                    }
                    Ok(None) => continue,
                    Err(err) if err.code() == P2pErrorCode::NotFound => {}
                    Err(err) => {
                        warn!(
                            "query remote detail failed peer={} serving_sn={} err={:?}",
                            peer_id, lease.serving_sn_id, err
                        );
                        continue;
                    }
                }
            } else {
                warn!(
                    "inter-sn detail query skipped because transport client is not configured peer={} serving_sn={}",
                    peer_id, lease.serving_sn_id
                );
            }
        }
        details
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
    ) -> P2pResult<()> {
        let client_id = P2pId::from(peer_id.as_slice());
        let client_cert =
            self.client_cert_from_request_or_cache(&client_id, call_req.peer_info.clone())?;
        self.validate_connection(self.validate_context_from_cert(peer_id, client_cert)?)
            .await?;
        let local_sn_id = self.effective_local_sn_id(local_id);

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

        self.cmd_server
            .send_by_specify_tunnel(
                peer_id,
                tunnel_id,
                PackageCmdCode::SnCallResp as u8,
                self.cmd_version,
                call_resp
                    .to_vec()
                    .map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?
                    .as_slice(),
            )
            .await
            .map_err(into_p2p_err!(P2pErrorCode::IoError))?;
        Ok(())
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

    pub async fn get_peer_wan_ep(
        &self,
        peer_id: &PeerId,
        reported_eps: &[Endpoint],
    ) -> Vec<Endpoint> {
        let tunnels = self.cmd_server.get_peer_tunnels(peer_id).await;
        let mut remotes = Vec::new();
        for tunnel in tunnels.iter() {
            let remote = tunnel.send.get().await.remote();
            if !remotes.contains(&remote) {
                remotes.push(remote);
            }
        }
        remotes
            .iter_mut()
            .filter(|remote| !remote.is_loopback())
            .map(|remote| Self::classify_observed_endpoint(*remote, reported_eps))
            .collect()
    }

    async fn get_peer_wan_ep_with_map_port(
        &self,
        peer_id: &PeerId,
        map_ports: &[(Protocol, u16)],
        reported_eps: &[Endpoint],
    ) -> Vec<Endpoint> {
        let mut remote_ep = self.get_peer_wan_ep(peer_id, reported_eps).await;
        let mut map_eps = Vec::new();
        for ep in remote_ep.iter() {
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

    async fn handle_report_sn(
        &self,
        local_id: &P2pId,
        peer_id: &PeerId,
        tunnel_id: CmdTunnelId,
        report_sn: ReportSn,
    ) -> P2pResult<()> {
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

        log::info!(
            "report sn from {}.eps: {:?} map_port: {:?}",
            peer_id.to_base36(),
            report_sn.local_eps,
            report_sn.map_ports
        );
        let mut reported_eps = report_sn.local_eps.clone();
        if let Some(peer_info) = report_sn.peer_info.as_ref() {
            let peer_desc = self.cert_factory.create(peer_info).unwrap();
            Self::extend_unique_endpoints(&mut reported_eps, peer_desc.endpoints().as_slice());
        }
        let remote_ep = self.get_peer_wan_ep(peer_id, reported_eps.as_slice()).await;

        if let Some(from_peer_id) = report_sn.from_peer_id.clone() {
            self.peer_mgr.add_or_update_peer(
                &from_peer_id,
                &report_sn
                    .peer_info
                    .map(|info| self.cert_factory.create(&info).unwrap()),
                report_sn.map_ports,
                &report_sn.local_eps,
            );
        }
        if let Err(e) = self
            .cmd_server
            .send_by_specify_tunnel(
                peer_id,
                tunnel_id,
                PackageCmdCode::ReportSnResp as u8,
                self.cmd_version,
                ReportSnResp {
                    seq: report_sn.seq,
                    sn_peer_id: local_id.clone(),
                    result: P2pErrorCode::Ok.into_u8(),
                    peer_info: None,
                    end_point_array: remote_ep,
                    receipt: None,
                }
                .to_vec()
                .map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?
                .as_slice(),
            )
            .await
        {
            log::error!(
                "send report-sn-resp failed, peer_id: {:?}, error: {:?}",
                peer_id,
                e
            );
        }
        Ok(())
    }

    async fn handle_query_sn(
        &self,
        local_id: &P2pId,
        peer_id: &PeerId,
        tunnel_id: CmdTunnelId,
        query: SnQuery,
    ) -> P2pResult<()> {
        let requester_sn_id = self.effective_local_sn_id(local_id);
        let device_info = self.peer_mgr.find_peer(&query.query_id);
        let mut remote_details = self
            .query_remote_details(&requester_sn_id, &query.query_id)
            .await;
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
            }
        } else {
            SnQueryResp {
                seq: query.seq,
                peer_info: None,
                end_point_array: vec![],
            }
        };

        self.cmd_server
            .send_by_specify_tunnel(
                peer_id,
                tunnel_id,
                PackageCmdCode::SnQueryResp as u8,
                self.cmd_version,
                resp.to_vec()
                    .map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?
                    .as_slice(),
            )
            .await
            .map_err(into_p2p_err!(P2pErrorCode::IoError))?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl CmdTunnelService<(), SnTunnelRead, SnTunnelWrite> for SnService {
    async fn handle_tunnel(&self, tunnel: CmdTunnel<SnTunnelRead, SnTunnelWrite>) -> CmdResult<()> {
        self.cmd_server.serve_tunnel(tunnel).await
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
}

pub struct SnServer {
    local_identity: P2pIdentityRef,
    net_manager: NetManagerRef,
    ttp_server: TtpServerRef,
    service: SnServiceRef,
    started: AtomicBool,
    stopped: AtomicBool,
    cmd_accept_task: Mutex<Option<crate::executor::SpawnHandle<()>>>,
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
        if owner_client_override.is_none() {
            if let Some(membership) = owner_client_membership.as_ref() {
                match TtpNode::new(local_identity.clone(), net_manager.clone()) {
                    Ok(ttp_node) => {
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
                    Err(err) => {
                        warn!("create inter-sn ttp node failed: {:?}", err);
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
        self.start_cmd_accept_loop().await?;
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
    server_runtime: Option<ServerRuntime>,
}

impl SnServiceConfig {
    pub fn new(
        local_identity: P2pIdentityRef,
        identity_factory: P2pIdentityFactoryRef,
        cert_factory: P2pIdentityCertFactoryRef,
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
            server_runtime: None,
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
        self.server_runtime = Some(server_runtime);
        self
    }
}

pub async fn create_sn_service(config: SnServiceConfig) -> SnServerRef {
    let server_runtime = config
        .server_runtime
        .unwrap_or_else(|| ServerRuntime::start(ServerRuntimeConfig::default()).unwrap());
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
        server_runtime,
    )
    .await;
    service
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
        P2pIdentityCertFactory, P2pIdentityCertRef, P2pIdentityRef, P2pSignature, P2pSn,
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
            Err(p2p_err!(P2pErrorCode::NotSupport, "unused in test"))
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

    fn test_identity(local_ep: Endpoint) -> P2pIdentityRef {
        Arc::new(DummyIdentity {
            id: P2pId::from(vec![1u8; 32]),
            name: "local-test".to_owned(),
            endpoints: vec![local_ep],
        })
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
        SnService::new(Arc::new(TestIdentityCertFactory), validator)
    }

    fn test_sn_service_with_directory(
        local_sn_id: P2pId,
        membership: Option<OwnerMembership>,
        inter_validator: SnInterServiceValidatorRef,
    ) -> SnServiceRef {
        let owner_client = membership
            .map(|membership| StaticOwnerDirectoryClient::new(membership, None))
            .unwrap_or_else(noop_owner_directory_client);
        SnService::new_with_options(
            Arc::new(TestIdentityCertFactory),
            allow_all_sn_connection_validator(),
            inter_validator,
            owner_client,
            None,
            Some(local_sn_id),
        )
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
