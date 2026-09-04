use crate::endpoint::{Endpoint, Protocol};
use crate::error::{P2pErrorCode, P2pResult, into_p2p_err, p2p_err};
use crate::executor::{Executor, SpawnHandle};
use crate::nat_type::{NatProfile, NatTraversalContext};
use crate::networks::{NetManagerRef, TunnelListenerInfo};
use crate::p2p_identity::{
    EncodedP2pIdentityCert, P2pId, P2pIdentityCertFactoryRef, P2pIdentityCertRef, P2pIdentityRef,
    P2pSn,
};
use crate::runtime;
use crate::sn::nat_probe::MAX_NAT_PROBE_ENDPOINTS;
use crate::sn::protocol::v0::{SnCallResp, SnCalled, SnCalledResp, TunnelType};
use crate::sn::protocol::{
    NatProbeDirective, NatProbeResult, Package, PackageCmdCode, ReportSn, ReportSnResp,
    SN_PROTOCOL_VERSION, SN_TUNNEL_RENDEZVOUS_CMD_VERSION, SnCall, SnQuery, SnQueryResp,
    SnTunnelRendezvous, SnTunnelRendezvousNotify, SnTunnelRendezvousOperation,
    SnTunnelRendezvousResp,
};
use crate::sn::types::{
    CmdTunnelId, SnCmdHeader, SnCmdPkgLen, SnTunnelClassification, SnTunnelRead, SnTunnelWrite,
    sn_cmd_purpose,
};
use crate::ttp::{TtpClient, TtpClientRef, TtpConnector, TtpTarget};
use crate::types::{Sequence, SequenceGenerator, TunnelId, TunnelIdGenerator};
use bucky_raw_codec::{RawConvertTo, RawFrom};
use bucky_time::bucky_time_now;
use chrono::Utc;
use sfo_cmd_server::client::{
    ClassifiedCmdClient, ClassifiedCmdSend, ClassifiedCmdTunnel, ClassifiedCmdTunnelFactory,
    CmdClient, DefaultClassifiedCmdClient,
};
use sfo_cmd_server::errors::{CmdErrorCode, CmdResult, cmd_err, into_cmd_err};
use sfo_cmd_server::{CmdBody, CmdTunnel, PeerId};
use std::collections::HashSet;
use std::net::IpAddr;
use std::ops::Add;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

fn sn_client_protocol_priority(protocol: Protocol) -> u8 {
    match protocol {
        Protocol::Quic => 0,
        Protocol::Tcp => 1,
        Protocol::Ext(_) => 2,
    }
}

fn sort_sn_client_listener_entries(
    mut listener_entries: Vec<(Protocol, Vec<TunnelListenerInfo>)>,
) -> Vec<(Protocol, Vec<TunnelListenerInfo>)> {
    listener_entries.sort_by_key(|(protocol, _)| sn_client_protocol_priority(*protocol));
    listener_entries
}

fn sn_client_protocol_candidates(
    listener_entries: Vec<(Protocol, Vec<TunnelListenerInfo>)>,
    supported_protocols: Vec<Protocol>,
) -> Vec<(Protocol, Vec<Option<Endpoint>>)> {
    let mut candidates = listener_entries
        .into_iter()
        .map(|(protocol, listeners)| {
            let mut local_eps = listeners
                .into_iter()
                .map(|listener| sn_client_local_ep_for_protocol(protocol, listener.local))
                .collect::<Vec<_>>();
            if local_eps.is_empty() {
                local_eps.push(None);
            }
            (protocol, local_eps)
        })
        .collect::<Vec<_>>();

    for protocol in supported_protocols {
        if !candidates
            .iter()
            .any(|(candidate_protocol, _)| *candidate_protocol == protocol)
        {
            candidates.push((protocol, vec![None]));
        }
    }

    candidates.sort_by_key(|(protocol, _)| sn_client_protocol_priority(*protocol));
    candidates
}

fn sn_client_local_ep_for_protocol(protocol: Protocol, local_ep: Endpoint) -> Option<Endpoint> {
    if protocol != Protocol::Tcp {
        return Some(local_ep);
    }

    if local_ep.addr().ip().is_unspecified() {
        return None;
    }

    let mut tcp_local_ep = local_ep;
    tcp_local_ep.mut_addr().set_port(0);
    Some(tcp_local_ep)
}

fn publish_active_sn(active_sn_list: &mut Vec<ActiveSN>, active_sn: ActiveSN) -> bool {
    if active_sn_list
        .iter()
        .any(|sn| sn.sn_peer_id == active_sn.sn_peer_id)
    {
        return false;
    }
    active_sn_list.push(active_sn);
    true
}

#[callback_trait::callback_trait]
pub trait SNEvent: 'static + Send + Sync {
    async fn on_called(&self, called: SnCalled) -> P2pResult<()>;
}
pub type SNEventRef = Arc<dyn SNEvent>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnTunnelRendezvousActionAck {
    pub predicted_endpoints: Vec<Endpoint>,
    pub socket_binding_generation: u64,
    pub valid_until: u64,
}

impl SnTunnelRendezvousActionAck {
    pub fn without_prediction() -> Self {
        Self {
            predicted_endpoints: Vec::new(),
            socket_binding_generation: 0,
            valid_until: 0,
        }
    }
}

#[callback_trait::callback_trait]
pub trait SNRendezvousEvent: 'static + Send + Sync {
    async fn on_rendezvous(
        &self,
        notify: SnTunnelRendezvousNotify,
        serving_sn_id: P2pId,
    ) -> P2pResult<SnTunnelRendezvousActionAck>;
}
pub type SNRendezvousEventRef = Arc<dyn SNRendezvousEvent>;

#[derive(Clone)]
pub struct ActiveSN {
    pub sn_peer_id: P2pId,
    pub latest_time: u64,
    pub conn_id: CmdTunnelId,
    pub protocol: Protocol,
    pub wan_ep_list: Vec<Endpoint>,
    pub nat_probe_endpoints: Vec<Endpoint>,
    pub nat_probe_signer: Option<P2pIdentityCertRef>,
    pub net_profile: NatProfile,
    pub nat_probe_registration_generation: u64,
    pub last_nat_probe_request_id: u64,
}

#[derive(Clone)]
pub struct SnNatProbeSnapshot {
    pub endpoints: Vec<Endpoint>,
    pub expected_signer: P2pIdentityCertRef,
}

#[derive(Clone, Debug)]
pub struct SnQueryResult {
    pub sn_peer_id: P2pId,
    pub local_net_profile: NatProfile,
    pub response: SnQueryResp,
}

const NAT_PROBE_TARGET_TIMEOUT: Duration = Duration::from_secs(2);
const NAT_PROFILE_TTL: Duration = Duration::from_secs(2 * 60 * 60);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NatProbeDirectiveRejectReason {
    TransportNotQuic,
    VersionUnsupported,
    SnMismatch,
    PeerMismatch,
    DeadlineExpired,
    Replay,
    EndpointCount,
    EndpointProtocol,
    EndpointIpv4,
    EndpointIpMismatch,
    EndpointPort,
    EndpointDuplicate,
}

impl NatProbeDirectiveRejectReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::TransportNotQuic => "transport_not_quic",
            Self::VersionUnsupported => "version_unsupported",
            Self::SnMismatch => "sn_mismatch",
            Self::PeerMismatch => "peer_mismatch",
            Self::DeadlineExpired => "deadline_expired",
            Self::Replay => "replay",
            Self::EndpointCount => "endpoint_count",
            Self::EndpointProtocol => "endpoint_protocol",
            Self::EndpointIpv4 => "endpoint_not_ipv4",
            Self::EndpointIpMismatch => "endpoint_ip_mismatch",
            Self::EndpointPort => "endpoint_port",
            Self::EndpointDuplicate => "endpoint_duplicate",
        }
    }
}

pub struct SNServiceState {
    pub pinging_handle: Option<SpawnHandle<()>>,
    pub active_sn_list: Vec<ActiveSN>,
    pub latest_sn_interval: u64,
}

pub struct SnList {
    sn_list: Mutex<Vec<P2pSn>>,
}

impl SnList {
    pub(crate) fn new(sn_list: Vec<P2pSn>) -> Self {
        Self {
            sn_list: Mutex::new(sn_list),
        }
    }

    pub fn get_sn_list(&self) -> Vec<P2pSn> {
        self.sn_list.lock().unwrap().clone()
    }

    pub fn update_sn_list(&self, sn_list: Vec<P2pSn>) {
        *self.sn_list.lock().unwrap() = sn_list;
    }
}

pub struct SnClientTunnelFactory {
    net_manager: NetManagerRef,
    sn_list: Arc<SnList>,
    ttp_client: TtpClientRef,
}

impl SnClientTunnelFactory {
    pub(crate) fn new(
        net_manager: NetManagerRef,
        sn_list: Arc<SnList>,
        ttp_client: TtpClientRef,
    ) -> Self {
        Self {
            net_manager,
            sn_list,
            ttp_client,
        }
    }

    async fn open_cmd_tunnel(
        &self,
        local_ep: Option<&Endpoint>,
        remote_ep: &Endpoint,
        remote_id: &P2pId,
        remote_name: String,
    ) -> CmdResult<ClassifiedCmdTunnel<SnTunnelRead, SnTunnelWrite>> {
        let purpose = sn_cmd_purpose().map_err(into_cmd_err!(
            CmdErrorCode::Failed,
            "encode sn cmd purpose failed"
        ))?;
        let classification = SnTunnelClassification::new(local_ep.copied(), *remote_ep);
        let target = TtpTarget {
            local_ep: local_ep.copied(),
            remote_ep: *remote_ep,
            remote_id: remote_id.clone(),
            remote_name: Some(remote_name.clone()),
        };
        self.ttp_client
            .connect_server(target.clone())
            .await
            .map_err(into_cmd_err!(
                CmdErrorCode::Failed,
                "connect sn ttp server failed"
            ))?;
        let (meta, read, write) = self
            .ttp_client
            .open_control_stream(&target, purpose)
            .await
            .map_err(into_cmd_err!(
                CmdErrorCode::Failed,
                "open sn cmd control stream failed"
            ))?;
        let local = meta
            .local_ep
            .unwrap_or(local_ep.copied().unwrap_or_default());
        let remote = meta.remote_ep.unwrap_or(*remote_ep);
        let local_id = meta.local_id;
        let remote_id = meta.remote_id;
        Ok(ClassifiedCmdTunnel::new(
            SnTunnelRead::new_with_classification(
                read,
                local,
                remote,
                local_id.clone(),
                remote_id.clone(),
                classification,
            ),
            SnTunnelWrite::new(write, local, remote, local_id, remote_id),
        ))
    }

    async fn open_cmd_tunnel_to_sn(
        &self,
        local_ep: Option<&Endpoint>,
        remote_ep: &Endpoint,
    ) -> CmdResult<ClassifiedCmdTunnel<SnTunnelRead, SnTunnelWrite>> {
        for sn_cert in self.sn_list.get_sn_list().iter() {
            for sn_ep in sn_cert.endpoints().iter() {
                if sn_ep.protocol() == remote_ep.protocol() && sn_ep == remote_ep {
                    return self
                        .open_cmd_tunnel(local_ep, sn_ep, &sn_cert.get_id(), sn_cert.get_name())
                        .await;
                }
            }
        }
        Err(cmd_err!(CmdErrorCode::Failed, "create tunnel failed"))
    }
}

#[async_trait::async_trait]
impl ClassifiedCmdTunnelFactory<SnTunnelClassification, (), SnTunnelRead, SnTunnelWrite>
    for SnClientTunnelFactory
{
    async fn create_tunnel(
        &self,
        classification: Option<SnTunnelClassification>,
    ) -> CmdResult<ClassifiedCmdTunnel<SnTunnelRead, SnTunnelWrite>> {
        if let Some(classification) = classification {
            if let Some(local_ep) = classification.local_ep.as_ref() {
                return self
                    .open_cmd_tunnel_to_sn(Some(local_ep), &classification.remote_ep)
                    .await;
            }

            return self
                .open_cmd_tunnel_to_sn(None, &classification.remote_ep)
                .await;
        }

        let protocol_candidates = sn_client_protocol_candidates(
            self.net_manager.listener_info_entries(),
            self.net_manager.protocols(),
        );
        for (protocol, local_eps) in protocol_candidates {
            for sn_cert in self.sn_list.get_sn_list().iter() {
                for sn_ep in sn_cert.endpoints().iter() {
                    if sn_ep.protocol() != protocol {
                        continue;
                    }
                    for local_ep in local_eps.iter() {
                        if let Ok(tunnel) = self
                            .open_cmd_tunnel(
                                local_ep.as_ref(),
                                sn_ep,
                                &sn_cert.get_id(),
                                sn_cert.get_name(),
                            )
                            .await
                        {
                            return Ok(tunnel);
                        }
                    }
                }
            }
        }
        Err(cmd_err!(CmdErrorCode::Failed, "create tunnel failed"))
    }
}

pub type SnCmdClient = DefaultClassifiedCmdClient<
    SnTunnelClassification,
    (),
    SnTunnelRead,
    SnTunnelWrite,
    SnClientTunnelFactory,
    SnCmdPkgLen,
    u8,
>;

pub type SnCmdClientRef = Arc<SnCmdClient>;

pub trait SnLocalIpProvider: 'static + Send + Sync {
    fn get_local_ips(&self) -> Vec<IpAddr>;
}

pub type SnLocalIpProviderRef = Arc<dyn SnLocalIpProvider>;

pub struct DefaultSnLocalIpProvider;

impl DefaultSnLocalIpProvider {
    fn should_ignore_interface(name: &str) -> bool {
        name.contains("VMware")
            || name.contains("VirtualBox")
            || name.contains("ZeroTier")
            || name.starts_with("zt")
            || name.contains("Tun")
            || name.contains("tun")
            || name.contains("utun")
            || name.contains("docker")
            || name.contains("lo")
            || name.contains("veth")
            || name.contains("feth")
            || name.contains("V-M")
            || name.contains("br-")
            || name.contains("vEthernet")
    }
}

impl SnLocalIpProvider for DefaultSnLocalIpProvider {
    fn get_local_ips(&self) -> Vec<IpAddr> {
        if_addrs::get_if_addrs()
            .map(|addrs| {
                addrs
                    .iter()
                    .filter(|addr| {
                        !Self::should_ignore_interface(&addr.name) && !addr.ip().is_loopback()
                    })
                    .map(|addr| addr.addr.ip())
                    .collect::<Vec<IpAddr>>()
            })
            .unwrap_or_default()
    }
}

pub struct SNClientService {
    net_manager: NetManagerRef,
    sn_list: Arc<SnList>,
    local_identity: P2pIdentityRef,
    gen_seq: Arc<SequenceGenerator>,
    gen_id: Arc<TunnelIdGenerator>,
    ping_timeout: Duration,
    call_timeout: Duration,
    conn_timeout: Duration,
    state: RwLock<SNServiceState>,
    listener: Mutex<Option<SNEventRef>>,
    rendezvous_listener: Mutex<Option<SNRendezvousEventRef>>,
    cert_factory: P2pIdentityCertFactoryRef,
    cmd_client: SnCmdClientRef,
    ttp_client: TtpClientRef,
    cmd_version: u8,
    local_ip_provider: SnLocalIpProviderRef,
}
pub type SNClientServiceRef = Arc<SNClientService>;

impl Drop for SNClientService {
    fn drop(&mut self) {
        log::info!(
            "SNClientService drop.device = {}",
            self.local_identity.get_id()
        );
    }
}

impl SNClientService {
    pub fn new(
        net_manager: NetManagerRef,
        sn_list: Vec<P2pSn>,
        local_identity: P2pIdentityRef,
        gen_seq: Arc<SequenceGenerator>,
        gen_id: Arc<TunnelIdGenerator>,
        cert_factory: P2pIdentityCertFactoryRef,
        tunnel_count: u16,
        ping_timeout: Duration,
        call_timeout: Duration,
        conn_timeout: Duration,
    ) -> Arc<Self> {
        Self::new_with_local_ip_provider(
            net_manager,
            sn_list,
            local_identity,
            gen_seq,
            gen_id,
            cert_factory,
            tunnel_count,
            ping_timeout,
            call_timeout,
            conn_timeout,
            Arc::new(DefaultSnLocalIpProvider),
        )
    }

    pub fn new_with_local_ip_provider(
        net_manager: NetManagerRef,
        sn_list: Vec<P2pSn>,
        local_identity: P2pIdentityRef,
        gen_seq: Arc<SequenceGenerator>,
        gen_id: Arc<TunnelIdGenerator>,
        cert_factory: P2pIdentityCertFactoryRef,
        tunnel_count: u16,
        ping_timeout: Duration,
        call_timeout: Duration,
        conn_timeout: Duration,
        local_ip_provider: SnLocalIpProviderRef,
    ) -> Arc<Self> {
        let sn_list = Arc::new(SnList::new(sn_list));
        let ttp_client = TtpClient::new(local_identity.clone(), net_manager.clone());
        let cmd_client = DefaultClassifiedCmdClient::new(
            SnClientTunnelFactory::new(net_manager.clone(), sn_list.clone(), ttp_client.clone()),
            tunnel_count,
        );
        let this = Arc::new(Self {
            net_manager,
            sn_list,
            local_identity,
            gen_seq,
            gen_id,
            ping_timeout,
            call_timeout,
            conn_timeout,
            state: RwLock::new(SNServiceState {
                pinging_handle: None,
                active_sn_list: vec![],
                latest_sn_interval: 0,
            }),
            listener: Mutex::new(None),
            rendezvous_listener: Mutex::new(None),
            cert_factory,
            cmd_client,
            ttp_client,
            cmd_version: 0,
            local_ip_provider,
        });
        this.register_cmd_handler();
        this
    }

    pub fn set_listener(&self, listener: impl SNEvent) {
        let mut _listener = self.listener.lock().unwrap();
        *_listener = Some(Arc::new(listener));
    }

    pub fn set_rendezvous_listener(&self, listener: impl SNRendezvousEvent) {
        *self.rendezvous_listener.lock().unwrap() = Some(Arc::new(listener));
    }

    pub fn get_cmd_client(&self) -> &SnCmdClientRef {
        &self.cmd_client
    }

    pub fn get_ttp_client(&self) -> TtpClientRef {
        self.ttp_client.clone()
    }

    pub fn get_net_manager(&self) -> NetManagerRef {
        self.net_manager.clone()
    }

    pub fn get_sn_list(&self) -> Vec<P2pSn> {
        self.sn_list.get_sn_list()
    }

    pub fn get_wan_ip_list(&self) -> Vec<Endpoint> {
        let mut wan_list = Vec::new();
        self.get_active_sn_list()
            .iter()
            .map(|v| v.wan_ep_list.as_slice())
            .flatten()
            .for_each(|ep| {
                wan_list.push(ep.clone());
            });
        wan_list
    }

    pub fn get_wan_ip_list_for_sn(&self, sn_peer_id: &P2pId) -> Vec<Endpoint> {
        self.get_active_sn_list()
            .into_iter()
            .find(|active| &active.sn_peer_id == sn_peer_id)
            .map(|active| active.wan_ep_list)
            .unwrap_or_default()
    }

    pub fn get_nat_probe_snapshot_for_sn(
        &self,
        sn_peer_id: &P2pId,
    ) -> Option<SnNatProbeSnapshot> {
        let state = self.state.read().unwrap();
        let active = state
            .active_sn_list
            .iter()
            .find(|active| &active.sn_peer_id == sn_peer_id)?;
        Some(SnNatProbeSnapshot {
            endpoints: active.nat_probe_endpoints.clone(),
            expected_signer: active.nat_probe_signer.clone()?,
        })
    }

    fn validate_nat_probe_signer(
        &self,
        expected_sn_id: &P2pId,
        peer_info: Option<&EncodedP2pIdentityCert>,
    ) -> Option<P2pIdentityCertRef> {
        let Some(peer_info) = peer_info else {
            log::warn!(
                "event=nat_probe_signer_rejected sn_id={} reason=missing_peer_info",
                expected_sn_id
            );
            return None;
        };
        let cert = match self.cert_factory.create(peer_info) {
            Ok(cert) => cert,
            Err(err) => {
                log::warn!(
                    "event=nat_probe_signer_rejected sn_id={} reason=malformed_peer_info err={:?}",
                    expected_sn_id,
                    err
                );
                return None;
            }
        };
        if &cert.get_id() != expected_sn_id {
            log::warn!(
                "event=nat_probe_signer_rejected sn_id={} reason=identity_mismatch actual_id={}",
                expected_sn_id,
                cert.get_id()
            );
            return None;
        }
        let cert_name = cert.get_name();
        if !cert.verify_cert(cert_name.as_str()) {
            log::warn!(
                "event=nat_probe_signer_rejected sn_id={} reason=self_verification_failed",
                expected_sn_id
            );
            return None;
        }
        Some(cert)
    }

    pub fn is_same_lan(&self, reverse_list: &Vec<Endpoint>) -> bool {
        let local_wan_list = self.get_wan_ip_list();
        for ep in reverse_list.iter() {
            for wan_ip in local_wan_list.iter() {
                if ep.is_same_ip_addr(wan_ip) {
                    return true;
                }
            }
        }
        false
    }

    fn register_cmd_handler(self: &Arc<Self>) {
        let this = self.clone();
        self.cmd_client.register_cmd_handler(
            PackageCmdCode::SnCalled as u8,
            move |_local_id: PeerId,
                  _peer_id: PeerId,
                  tunnel_id: CmdTunnelId,
                  _header: SnCmdHeader,
                  mut body: CmdBody| {
                let this = this.clone();
                async move {
                    let sn_called = SnCalled::clone_from_slice(body.read_all().await?.as_slice())
                        .map_err(into_cmd_err!(CmdErrorCode::RawCodecError))?;
                    this.on_called(tunnel_id, sn_called)
                        .await
                        .map_err(into_cmd_err!(
                            CmdErrorCode::Failed,
                            "sn called handle failed"
                        ))?;
                    Ok(None)
                }
            },
        );

        let this = self.clone();
        self.cmd_client.register_cmd_handler(
            PackageCmdCode::SnTunnelRendezvousNotify as u8,
            move |_local_id: PeerId,
                  peer_id: PeerId,
                  tunnel_id: CmdTunnelId,
                  header: SnCmdHeader,
                  mut body: CmdBody| {
                let this = this.clone();
                async move {
                    if header.version() != SN_TUNNEL_RENDEZVOUS_CMD_VERSION {
                        return Err(cmd_err!(
                            CmdErrorCode::InvalidParam,
                            "unsupported SN rendezvous command version: {}",
                            header.version()
                        ));
                    }
                    let notify = SnTunnelRendezvousNotify::clone_from_slice(
                        body.read_all().await?.as_slice(),
                    )
                    .map_err(into_cmd_err!(CmdErrorCode::RawCodecError))?;
                    let response = this.on_rendezvous_notify(tunnel_id, &peer_id, notify).await;
                    Ok(Some(CmdBody::from(
                        response
                            .to_vec()
                            .map_err(into_cmd_err!(CmdErrorCode::RawCodecError))?,
                    )))
                }
            },
        );
    }

    fn active_sn_matches(&self, sn_peer_id: &P2pId, conn_id: CmdTunnelId) -> bool {
        self.state
            .read()
            .unwrap()
            .active_sn_list
            .iter()
            .any(|active| &active.sn_peer_id == sn_peer_id && active.conn_id == conn_id)
    }

    async fn on_rendezvous_notify(
        &self,
        conn_id: CmdTunnelId,
        sn_peer: &PeerId,
        notify: SnTunnelRendezvousNotify,
    ) -> SnTunnelRendezvousResp {
        let failure = || SnTunnelRendezvousResp::failure(notify.seq);
        let serving_sn_id = P2pId::from(sn_peer.as_slice());
        if !self.active_sn_matches(&serving_sn_id, conn_id) {
            return failure();
        }
        if notify.validate().is_err() {
            return failure();
        }
        let initiator_id = match self.cert_factory.create(&notify.peer_info) {
            Ok(cert) if cert.get_id() != self.local_identity.get_id() => cert.get_id(),
            _ => return failure(),
        };
        let listener = self.rendezvous_listener.lock().unwrap().clone();
        let Some(listener) = listener else {
            return failure();
        };
        let ack = match listener.on_rendezvous(notify.clone(), serving_sn_id).await {
            Ok(ack) => ack,
            Err(_) => return failure(),
        };
        let now = bucky_time_now();
        if notify.need_predict_endpoint {
            if ack.socket_binding_generation == 0 || ack.valid_until < now {
                return failure();
            }
        } else if !ack.predicted_endpoints.is_empty()
            || ack.socket_binding_generation != 0
            || ack.valid_until != 0
        {
            return failure();
        }
        let response = SnTunnelRendezvousResp::success(notify.seq, ack.predicted_endpoints);
        if response
            .validate(notify.seq, notify.need_predict_endpoint)
            .is_err()
        {
            return failure();
        }
        log::info!(
            "event=sn_rendezvous_action_armed seq={} tunnel_id={:?} initiator={} operation={:?} endpoint_count={} predicted_count={}",
            notify.seq.value(),
            notify.tunnel_id,
            initiator_id,
            notify.operation,
            notify.end_point_array.len(),
            response.predicted_endpoint_array.len()
        );
        response
    }

    pub fn new_rendezvous_request(
        &self,
        tunnel_id: TunnelId,
        to_peer_id: &P2pId,
        operation: SnTunnelRendezvousOperation,
        end_point_array: Vec<Endpoint>,
        need_predict_endpoint: bool,
    ) -> P2pResult<SnTunnelRendezvous> {
        let seq = loop {
            let seq = self.gen_seq.generate();
            if seq.value() != 0 {
                break seq;
            }
        };
        let request = SnTunnelRendezvous {
            seq,
            tunnel_id,
            to_peer_id: to_peer_id.clone(),
            operation,
            end_point_array,
            need_predict_endpoint,
        };
        request.validate()?;
        Ok(request)
    }

    pub async fn rendezvous_via_sn(
        &self,
        sn_peer_id: &P2pId,
        request: &SnTunnelRendezvous,
    ) -> P2pResult<SnTunnelRendezvousResp> {
        request.validate()?;
        let active_sn = self
            .get_active_sn_list()
            .into_iter()
            .find(|active| &active.sn_peer_id == sn_peer_id)
            .ok_or_else(|| {
                p2p_err!(
                    P2pErrorCode::NotFound,
                    "rendezvous SN is not active: {}",
                    sn_peer_id
                )
            })?;
        let bytes = request
            .to_vec()
            .map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
        let mut body = self
            .cmd_client
            .send_by_specify_tunnel_with_resp(
                active_sn.conn_id,
                PackageCmdCode::SnTunnelRendezvous as u8,
                SN_TUNNEL_RENDEZVOUS_CMD_VERSION,
                bytes.as_slice(),
                self.call_timeout,
            )
            .await
            .map_err(into_p2p_err!(P2pErrorCode::IoError))?;
        let response = SnTunnelRendezvousResp::clone_from_slice(
            body.read_all()
                .await
                .map_err(into_p2p_err!(P2pErrorCode::IoError))?
                .as_slice(),
        )
        .map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
        response.validate(request.seq, request.need_predict_endpoint)?;
        if !response.is_success() {
            return Err(p2p_err!(
                P2pErrorCode::Failed,
                "SN rendezvous request failed"
            ));
        }
        Ok(response)
    }

    async fn on_called(&self, conn_id: CmdTunnelId, sn_called: SnCalled) -> P2pResult<()> {
        let listener = {
            let listener = self.listener.lock().unwrap();
            listener.clone()
        };
        let seq = sn_called.seq.clone();
        let sn_peer_id = sn_called.sn_peer_id.clone();
        let to_peer_id = sn_called.to_peer_id.clone();

        log::debug!(
            "sn called recv conn_id={:?} seq={} sn={} to={} reverse_eps={:?} pn_list={:?}",
            conn_id,
            seq.value(),
            sn_peer_id,
            to_peer_id,
            sn_called.reverse_endpoint_array,
            sn_called.active_pn_list
        );

        let resp = if to_peer_id == self.local_identity.get_id() {
            if listener.is_some() {
                log::debug!(
                    "sn called dispatch to listener seq={} conn_id={:?}",
                    seq.value(),
                    conn_id
                );
                match listener.as_ref().unwrap().on_called(sn_called).await {
                    Ok(_) => SnCalledResp {
                        seq,
                        sn_peer_id,
                        result: 0,
                    },
                    Err(e) => {
                        log::info!("on called to {} failed: {:?}", to_peer_id, e);
                        SnCalledResp {
                            seq,
                            sn_peer_id,
                            result: e.code().into_u8(),
                        }
                    }
                }
            } else {
                log::debug!(
                    "sn called seq={} has no listener, respond success directly",
                    seq.value()
                );
                SnCalledResp {
                    seq,
                    sn_peer_id,
                    result: 0,
                }
            }
        } else {
            SnCalledResp {
                seq,
                sn_peer_id,
                result: P2pErrorCode::TargetNotFound.into_u8(),
            }
        };

        log::debug!(
            "sn called resp conn_id={:?} seq={} result={}",
            conn_id,
            resp.seq.value(),
            resp.result
        );

        self.cmd_client
            .send_by_specify_tunnel(
                conn_id,
                PackageCmdCode::SnCalledResp as u8,
                self.cmd_version,
                resp.to_vec()
                    .map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?
                    .as_slice(),
            )
            .await
            .map_err(into_p2p_err!(
                P2pErrorCode::IoError,
                "send SnCalledResp failed"
            ))?;
        Ok(())
    }

    pub async fn start(self: &Arc<Self>) -> P2pResult<()> {
        let this = self.clone();
        let handle = Executor::spawn_with_handle(async move {
            this.ping_proc().await;
        })
        .map_err(into_p2p_err!(
            P2pErrorCode::Failed,
            "start sn ping proc failed"
        ))?;
        {
            let mut state = self.state.write().unwrap();
            state.pinging_handle = Some(handle);
        }
        Ok(())
    }

    pub fn stop(&self) {
        {
            let mut state = self.state.write().unwrap();
            state.active_sn_list.clear();
            if let Some(handle) = state.pinging_handle.take() {
                handle.abort();
            }
        }
    }

    pub async fn reset_sn(self: &Arc<Self>, sn_list: Vec<P2pSn>) {
        self.sn_list.update_sn_list(sn_list);
        self.stop();
        self.cmd_client.clear_all_tunnel().await;
        self.start().await;
    }

    async fn ping_proc(self: &Arc<Self>) {
        loop {
            {
                let (active_sn_count, latest_sn_interval, cur_sn_interval) = {
                    let mut state = self.state.write().unwrap();
                    if state.active_sn_list.len() > 0 {
                        state.latest_sn_interval = 10;
                        (
                            state.active_sn_list.len(),
                            state.latest_sn_interval,
                            state.latest_sn_interval,
                        )
                    } else {
                        let cur_sn_interval = state.latest_sn_interval;
                        if state.latest_sn_interval == 0 {
                            state.latest_sn_interval = 1;
                        } else if state.latest_sn_interval == 10 {
                            state.latest_sn_interval = 1;
                        } else {
                            state.latest_sn_interval = state.latest_sn_interval * 2;
                        }
                        if state.latest_sn_interval > 600 {
                            state.latest_sn_interval = 600;
                        }
                        (
                            state.active_sn_list.len(),
                            cur_sn_interval,
                            state.latest_sn_interval,
                        )
                    }
                };
                if latest_sn_interval != 0 {
                    runtime::sleep(Duration::from_secs(cur_sn_interval)).await;
                }
                if active_sn_count > 0 {
                    let mut ping_sn_list = Vec::new();
                    {
                        let mut state = self.state.write().unwrap();
                        for active_sn in state.active_sn_list.iter_mut() {
                            if bucky_time_now() - active_sn.latest_time > 600 * 1000 * 1000 {
                                active_sn.latest_time = bucky_time_now();
                                ping_sn_list.push(active_sn.clone());
                            }
                        }
                    }

                    for active_sn in ping_sn_list.iter() {
                        match self
                            .report(active_sn.conn_id, active_sn.sn_peer_id.clone(), None)
                            .await
                        {
                            Ok(mut resp) => {
                                let mut completed_probe = None;
                                let mut accepted_probe = None;
                                let mut nat_probe_signer = self.validate_nat_probe_signer(
                                    &active_sn.sn_peer_id,
                                    resp.peer_info.as_ref(),
                                );
                                if let Some(result) = self
                                    .execute_probe_directive(
                                        active_sn.sn_peer_id.clone(),
                                        active_sn.protocol,
                                        active_sn.nat_probe_registration_generation,
                                        active_sn.last_nat_probe_request_id,
                                        nat_probe_signer.clone(),
                                        resp.nat_probe_directive.take(),
                                    )
                                    .await
                                {
                                    accepted_probe =
                                        Some((result.registration_generation, result.request_id));
                                    match self
                                        .report(
                                            active_sn.conn_id,
                                            active_sn.sn_peer_id.clone(),
                                            Some(&result),
                                        )
                                        .await
                                    {
                                        Ok(follow_up) => {
                                            log::info!(
                                                "event=nat_probe_result_reported sn_id={} peer_id={} tunnel_id={:?} registration_generation={} config_generation={} request_id={} observation={:?}",
                                                active_sn.sn_peer_id,
                                                self.local_identity.get_id(),
                                                active_sn.conn_id,
                                                result.registration_generation,
                                                result.probe_config_generation,
                                                result.request_id,
                                                result.profile.observation
                                            );
                                            completed_probe = Some(result);
                                            resp = follow_up;
                                            nat_probe_signer = self.validate_nat_probe_signer(
                                                &active_sn.sn_peer_id,
                                                resp.peer_info.as_ref(),
                                            );
                                        }
                                        Err(err) => {
                                            log::warn!(
                                                "event=nat_probe_result_report_failed sn_id={} peer_id={} tunnel_id={:?} registration_generation={} config_generation={} request_id={} err={:?}",
                                                active_sn.sn_peer_id,
                                                self.local_identity.get_id(),
                                                active_sn.conn_id,
                                                result.registration_generation,
                                                result.probe_config_generation,
                                                result.request_id,
                                                err
                                            );
                                        }
                                    }
                                }
                                let mut state = self.state.write().unwrap();
                                if let Some(current) = state.active_sn_list.iter_mut().find(|sn| {
                                    sn.sn_peer_id == active_sn.sn_peer_id
                                        && sn.conn_id == active_sn.conn_id
                                }) {
                                    if let Some((generation, request_id)) = accepted_probe {
                                        current.nat_probe_registration_generation = generation;
                                        current.last_nat_probe_request_id = request_id;
                                    }
                                    if let Some(result) = completed_probe {
                                        current.net_profile = result.profile;
                                    }
                                    current.nat_probe_endpoints = resp.nat_probe_endpoints;
                                    current.nat_probe_signer = nat_probe_signer;
                                    current.wan_ep_list = resp.end_point_array;
                                }
                            }
                            Err(e) => {
                                log::error!("ping to {} failed: {:?}", active_sn.sn_peer_id, e);
                                continue;
                            }
                        }
                    }
                    continue;
                }
            }
            let protocol_candidates = sn_client_protocol_candidates(
                self.net_manager.listener_info_entries(),
                self.net_manager.protocols(),
            );
            for sn_cert in self.sn_list.get_sn_list().iter() {
                let mut sn_reported = false;
                for (protocol, local_eps) in protocol_candidates.iter() {
                    let protocol = *protocol;
                    for sn_ep in sn_cert.endpoints().iter() {
                        if sn_ep.protocol() != protocol {
                            continue;
                        }
                        for local_ep in local_eps.iter() {
                            let tunnel_id = match self
                                .cmd_client
                                .find_tunnel_id_by_classified(SnTunnelClassification::new(
                                    *local_ep,
                                    sn_ep.clone(),
                                ))
                                .await
                            {
                                Ok(tunnel_id) => tunnel_id,
                                Err(e) => {
                                    log::warn!(
                                        "sn client candidate tunnel failed sn_id={} protocol={:?} local_ep={:?} remote_ep={} err={:?}",
                                        sn_cert.get_id(),
                                        protocol,
                                        local_ep,
                                        sn_ep,
                                        e
                                    );
                                    continue;
                                }
                            };

                            let mut report_resp = match self
                                .report(tunnel_id, sn_cert.get_id(), None)
                                .await
                            {
                                Ok(resp) => resp,
                                Err(e) => {
                                    log::warn!(
                                        "sn client candidate report failed sn_id={} protocol={:?} local_ep={:?} remote_ep={} tunnel_id={:?} err={:?}",
                                        sn_cert.get_id(),
                                        protocol,
                                        local_ep,
                                        sn_ep,
                                        tunnel_id,
                                        e
                                    );
                                    continue;
                                }
                            };

                            let nat_probe_signer = self.validate_nat_probe_signer(
                                &sn_cert.get_id(),
                                report_resp.peer_info.as_ref(),
                            );
                            let directive = report_resp.nat_probe_directive.take();
                            let active_sn = ActiveSN {
                                sn_peer_id: sn_cert.get_id(),
                                latest_time: bucky_time_now(),
                                conn_id: tunnel_id,
                                protocol,
                                wan_ep_list: report_resp.end_point_array.clone(),
                                nat_probe_endpoints: report_resp.nat_probe_endpoints.clone(),
                                nat_probe_signer: nat_probe_signer.clone(),
                                net_profile: NatProfile::unknown(),
                                nat_probe_registration_generation: 0,
                                last_nat_probe_request_id: 0,
                            };
                            {
                                let mut state = self.state.write().unwrap();
                                publish_active_sn(&mut state.active_sn_list, active_sn);
                            }
                            sn_reported = true;

                            if let Some(result) = self
                                .execute_probe_directive(
                                    sn_cert.get_id(),
                                    protocol,
                                    0,
                                    0,
                                    nat_probe_signer,
                                    directive,
                                )
                                .await
                            {
                                {
                                    let mut state = self.state.write().unwrap();
                                    if let Some(current) =
                                        state.active_sn_list.iter_mut().find(|sn| {
                                            sn.sn_peer_id == sn_cert.get_id()
                                                && sn.conn_id == tunnel_id
                                        })
                                    {
                                        current.nat_probe_registration_generation =
                                            result.registration_generation;
                                        current.last_nat_probe_request_id = result.request_id;
                                    }
                                }
                                match self
                                    .report(tunnel_id, sn_cert.get_id(), Some(&result))
                                    .await
                                {
                                    Ok(resp) => {
                                        log::info!(
                                            "event=nat_probe_result_reported sn_id={} peer_id={} tunnel_id={:?} registration_generation={} config_generation={} request_id={} observation={:?}",
                                            sn_cert.get_id(),
                                            self.local_identity.get_id(),
                                            tunnel_id,
                                            result.registration_generation,
                                            result.probe_config_generation,
                                            result.request_id,
                                            result.profile.observation
                                        );
                                        let nat_probe_signer = self.validate_nat_probe_signer(
                                            &sn_cert.get_id(),
                                            resp.peer_info.as_ref(),
                                        );
                                        let mut state = self.state.write().unwrap();
                                        if let Some(current) =
                                            state.active_sn_list.iter_mut().find(|sn| {
                                                sn.sn_peer_id == sn_cert.get_id()
                                                    && sn.conn_id == tunnel_id
                                            })
                                        {
                                            current.net_profile = result.profile;
                                            current.nat_probe_endpoints =
                                                resp.nat_probe_endpoints;
                                            current.nat_probe_signer = nat_probe_signer;
                                            current.wan_ep_list = resp.end_point_array;
                                        }
                                    }
                                    Err(e) => {
                                        log::warn!(
                                            "event=nat_probe_result_report_failed sn_id={} peer_id={} tunnel_id={:?} registration_generation={} config_generation={} request_id={} err={:?}",
                                            sn_cert.get_id(),
                                            self.local_identity.get_id(),
                                            tunnel_id,
                                            result.registration_generation,
                                            result.probe_config_generation,
                                            result.request_id,
                                            e
                                        );
                                    }
                                }
                            }
                            break;
                        }
                        if sn_reported {
                            break;
                        }
                    }
                    if sn_reported {
                        break;
                    }
                }
            }
        }
    }

    fn remove_sn_conn(&self, conn_id: CmdTunnelId) {
        let mut state = self.state.write().unwrap();
        state.active_sn_list.retain(|sn| sn.conn_id != conn_id);
    }

    pub async fn wait_online(&self, timeout: Option<Duration>) -> P2pResult<()> {
        let expire = if timeout.is_some() {
            Some(Utc::now().add(timeout.unwrap()))
        } else {
            None
        };
        loop {
            {
                if expire.is_some() {
                    if Utc::now() > expire.unwrap() {
                        return Err(p2p_err!(P2pErrorCode::Timeout, "wait online timeout"));
                    }
                }
                let state = self.state.read().unwrap();
                if state.active_sn_list.len() > 0 {
                    break;
                }
            }
            runtime::sleep(Duration::from_secs(1)).await;
        }
        Ok(())
    }

    pub fn get_active_sn_list(&self) -> Vec<ActiveSN> {
        let state = self.state.read().unwrap();
        state.active_sn_list.clone()
    }

    async fn probe_endpoints(
        &self,
        directive: &NatProbeDirective,
        expected_signer: Option<P2pIdentityCertRef>,
    ) -> NatProfile {
        let started = Instant::now();
        log::info!(
            "event=nat_probe_client_started sn_id={} peer_id={} registration_generation={} config_generation={} request_id={} endpoint_count={}",
            directive.sn_peer_id,
            directive.peer_id,
            directive.registration_generation,
            directive.probe_config_generation,
            directive.request_id,
            directive.endpoints.len()
        );
        log::debug!(
            "event=nat_probe_client_endpoints sn_id={} peer_id={} registration_generation={} config_generation={} request_id={} endpoints={:?}",
            directive.sn_peer_id,
            directive.peer_id,
            directive.registration_generation,
            directive.probe_config_generation,
            directive.request_id,
            directive.endpoints
        );
        let Some(expected_signer) = expected_signer else {
            log::warn!(
                "event=nat_probe_client_failed sn_id={} peer_id={} registration_generation={} config_generation={} request_id={} elapsed_ms={} reason=missing_trusted_signer",
                directive.sn_peer_id,
                directive.peer_id,
                directive.registration_generation,
                directive.probe_config_generation,
                directive.request_id,
                started.elapsed().as_millis()
            );
            return NatProfile::unknown();
        };
        let profile = match self.net_manager.get_network(Protocol::Quic) {
            Ok(network) => match network.as_udp_tunnel_network() {
                Some(network) => network
                    .predict_traversal_endpoints(
                        directive.endpoints.as_slice(),
                        &expected_signer,
                        NAT_PROBE_TARGET_TIMEOUT,
                        NAT_PROFILE_TTL,
                    )
                    .await
                    .map(|prediction| prediction.profile),
                None => Err(p2p_err!(
                    P2pErrorCode::NotSupport,
                    "QUIC network does not support UDP traversal prediction"
                )),
            },
            Err(err) => Err(err),
        };
        match profile {
            Ok(profile) => {
                log::info!(
                    "event=nat_probe_client_completed sn_id={} peer_id={} registration_generation={} config_generation={} request_id={} observation={:?} elapsed_ms={}",
                    directive.sn_peer_id,
                    directive.peer_id,
                    directive.registration_generation,
                    directive.probe_config_generation,
                    directive.request_id,
                    profile.observation,
                    started.elapsed().as_millis()
                );
                profile
            }
            Err(err) => {
                log::warn!(
                    "event=nat_probe_client_failed sn_id={} peer_id={} registration_generation={} config_generation={} request_id={} elapsed_ms={} err={:?}",
                    directive.sn_peer_id,
                    directive.peer_id,
                    directive.registration_generation,
                    directive.probe_config_generation,
                    directive.request_id,
                    started.elapsed().as_millis(),
                    err
                );
                NatProfile::unknown()
            }
        }
    }

    async fn execute_probe_directive(
        &self,
        active_sn_id: P2pId,
        active_protocol: Protocol,
        last_registration_generation: u64,
        last_request_id: u64,
        expected_signer: Option<P2pIdentityCertRef>,
        directive: Option<NatProbeDirective>,
    ) -> Option<NatProbeResult> {
        let directive = directive?;
        let now = bucky_time_now();
        if let Err(reason) = Self::validate_probe_directive(
            &active_sn_id,
            &self.local_identity.get_id(),
            active_protocol,
            last_registration_generation,
            last_request_id,
            now,
            &directive,
        ) {
            log::debug!(
                "event=nat_probe_directive_rejected sn_id={} peer_id={} active_transport={:?} registration_generation={} config_generation={} request_id={} reason={}",
                directive.sn_peer_id,
                directive.peer_id,
                active_protocol,
                directive.registration_generation,
                directive.probe_config_generation,
                directive.request_id,
                reason.as_str()
            );
            return None;
        }
        log::debug!(
            "event=nat_probe_directive_accepted sn_id={} peer_id={} active_transport={:?} registration_generation={} config_generation={} request_id={} expires_at={}",
            directive.sn_peer_id,
            directive.peer_id,
            active_protocol,
            directive.registration_generation,
            directive.probe_config_generation,
            directive.request_id,
            directive.expires_at
        );
        let profile = self.probe_endpoints(&directive, expected_signer).await;
        Some(NatProbeResult::from_directive(&directive, profile))
    }

    #[cfg(test)]
    pub(crate) async fn execute_probe_directive_for_test(
        &self,
        active_sn_id: P2pId,
        active_protocol: Protocol,
        last_registration_generation: u64,
        last_request_id: u64,
        directive: Option<NatProbeDirective>,
    ) -> Option<NatProbeResult> {
        let expected_signer = self
            .get_nat_probe_snapshot_for_sn(&active_sn_id)
            .map(|snapshot| snapshot.expected_signer);
        self.execute_probe_directive(
            active_sn_id,
            active_protocol,
            last_registration_generation,
            last_request_id,
            expected_signer,
            directive,
        )
        .await
    }

    fn valid_probe_directive(
        active_sn_id: &P2pId,
        local_peer_id: &P2pId,
        active_protocol: Protocol,
        last_registration_generation: u64,
        last_request_id: u64,
        now: u64,
        directive: &NatProbeDirective,
    ) -> bool {
        Self::validate_probe_directive(
            active_sn_id,
            local_peer_id,
            active_protocol,
            last_registration_generation,
            last_request_id,
            now,
            directive,
        )
        .is_ok()
    }

    fn validate_probe_directive(
        active_sn_id: &P2pId,
        local_peer_id: &P2pId,
        active_protocol: Protocol,
        last_registration_generation: u64,
        last_request_id: u64,
        now: u64,
        directive: &NatProbeDirective,
    ) -> Result<(), NatProbeDirectiveRejectReason> {
        let replayed = directive.registration_generation < last_registration_generation
            || (directive.registration_generation == last_registration_generation
                && directive.request_id <= last_request_id);
        if active_protocol != Protocol::Quic {
            return Err(NatProbeDirectiveRejectReason::TransportNotQuic);
        }
        if !directive.is_supported() {
            return Err(NatProbeDirectiveRejectReason::VersionUnsupported);
        }
        if &directive.sn_peer_id != active_sn_id {
            return Err(NatProbeDirectiveRejectReason::SnMismatch);
        }
        if &directive.peer_id != local_peer_id {
            return Err(NatProbeDirectiveRejectReason::PeerMismatch);
        }
        if now > directive.expires_at {
            return Err(NatProbeDirectiveRejectReason::DeadlineExpired);
        }
        if replayed {
            return Err(NatProbeDirectiveRejectReason::Replay);
        }
        Self::validate_probe_directive_endpoints(directive.endpoints.as_slice())
    }

    fn valid_probe_directive_endpoints(endpoints: &[Endpoint]) -> bool {
        Self::validate_probe_directive_endpoints(endpoints).is_ok()
    }

    fn validate_probe_directive_endpoints(
        endpoints: &[Endpoint],
    ) -> Result<(), NatProbeDirectiveRejectReason> {
        if endpoints.len() < 2 || endpoints.len() > MAX_NAT_PROBE_ENDPOINTS {
            return Err(NatProbeDirectiveRejectReason::EndpointCount);
        }
        let first_ip = endpoints[0].addr().ip();
        let mut addresses = HashSet::with_capacity(endpoints.len());
        for endpoint in endpoints {
            if endpoint.protocol() != Protocol::Quic {
                return Err(NatProbeDirectiveRejectReason::EndpointProtocol);
            }
            if !endpoint.addr().is_ipv4() {
                return Err(NatProbeDirectiveRejectReason::EndpointIpv4);
            }
            if endpoint.addr().ip() != first_ip {
                return Err(NatProbeDirectiveRejectReason::EndpointIpMismatch);
            }
            if endpoint.addr().port() == 0 {
                return Err(NatProbeDirectiveRejectReason::EndpointPort);
            }
            if !addresses.insert(*endpoint.addr()) {
                return Err(NatProbeDirectiveRejectReason::EndpointDuplicate);
            }
        }
        Ok(())
    }

    async fn report(
        &self,
        tunnel_id: CmdTunnelId,
        sn_peer_id: P2pId,
        nat_probe_result: Option<&NatProbeResult>,
    ) -> P2pResult<ReportSnResp> {
        let seq = self.gen_seq.generate();
        let local_ips = self.local_ip_provider.get_local_ips();

        let mut local_eps = Vec::new();
        let mut map_ports = Vec::new();
        for (protocol, listeners) in self.net_manager.listener_info_entries() {
            for listener in listeners.iter() {
                if let Some(tcp_map_port) = listener.mapping_port {
                    map_ports.push((protocol, tcp_map_port));
                }
                if listener.local.addr().ip().is_unspecified() {
                    for ip in local_ips.iter() {
                        local_eps.push(Endpoint::from((
                            protocol,
                            *ip,
                            listener.local.addr().port(),
                        )));
                    }
                } else {
                    local_eps.push(listener.local);
                }
            }
        }

        let report = ReportSn {
            protocol_version: SN_PROTOCOL_VERSION,
            stack_version: 0,
            seq,
            sn_peer_id: sn_peer_id.clone(),
            from_peer_id: Some(self.local_identity.get_id()),
            peer_info: Some(
                self.local_identity
                    .get_identity_cert()?
                    .get_encoded_cert()?,
            ),
            send_time: bucky_time_now(),
            contract_id: None,
            receipt: None,
            map_ports,
            local_eps,
            net_profile: None,
            nat_probe_control_version: Some(crate::sn::protocol::NAT_PROBE_CONTROL_VERSION),
            nat_probe_result: nat_probe_result.cloned(),
        };
        let report_body = report
            .to_vec()
            .map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
        let mut resp_body = match self
            .cmd_client
            .send_by_specify_tunnel_with_resp(
                tunnel_id,
                PackageCmdCode::ReportSn as u8,
                self.cmd_version,
                report_body.as_slice(),
                self.call_timeout,
            )
            .await
        {
            Ok(resp_body) => resp_body,
            Err(e) => {
                if e.code() != CmdErrorCode::Timeout {
                    self.remove_sn_conn(tunnel_id);
                }
                return Err(p2p_err!(
                    P2pErrorCode::ConnectFailed,
                    "report qa failed sn={} tunnel_id={:?} err={:?}",
                    sn_peer_id,
                    tunnel_id,
                    e
                ));
            }
        };
        let resp = ReportSnResp::clone_from_slice(
            resp_body
                .read_all()
                .await
                .map_err(into_p2p_err!(
                    P2pErrorCode::IoError,
                    "read report qa response"
                ))?
                .as_slice(),
        )
        .map_err(into_p2p_err!(
            P2pErrorCode::RawCodecError,
            "decode report qa response"
        ))?;
        if resp.seq != seq || resp.sn_peer_id != sn_peer_id {
            return Err(p2p_err!(
                P2pErrorCode::InvalidData,
                "report qa response mismatch tunnel_id={:?} expected_seq={} actual_seq={} expected_sn={} actual_sn={}",
                tunnel_id,
                seq.value(),
                resp.seq.value(),
                sn_peer_id,
                resp.sn_peer_id
            ));
        }
        log::debug!(
            "event=sn_report_response_received sn_id={} tunnel_id={:?} observed_endpoint_count={} nat_probe_directive={}",
            resp.sn_peer_id,
            tunnel_id,
            resp.end_point_array.len(),
            resp.nat_probe_directive.is_some()
        );
        Ok(resp)
    }

    #[cfg(test)]
    pub(crate) async fn report_for_test(
        &self,
        tunnel_id: CmdTunnelId,
        sn_peer_id: P2pId,
        nat_probe_result: Option<&NatProbeResult>,
    ) -> P2pResult<ReportSnResp> {
        self.report(tunnel_id, sn_peer_id, nat_probe_result).await
    }

    pub async fn call(
        &self,
        tunnel_id: TunnelId,
        reverse_endpoints: Option<&[Endpoint]>,
        remote: &P2pId,
        call_type: TunnelType,
        payload_pkg: Vec<u8>,
    ) -> P2pResult<SnCallResp> {
        self.call_inner(
            None,
            tunnel_id,
            reverse_endpoints,
            remote,
            call_type,
            payload_pkg,
            None,
        )
        .await
    }

    pub(crate) fn call_timeout(&self) -> Duration {
        self.call_timeout
    }

    pub async fn call_via_sn(
        &self,
        sn_peer_id: &P2pId,
        tunnel_id: TunnelId,
        reverse_endpoints: Option<&[Endpoint]>,
        remote: &P2pId,
        call_type: TunnelType,
        payload_pkg: Vec<u8>,
        nat_context: Option<&NatTraversalContext>,
    ) -> P2pResult<SnCallResp> {
        self.call_inner(
            Some(sn_peer_id),
            tunnel_id,
            reverse_endpoints,
            remote,
            call_type,
            payload_pkg,
            nat_context,
        )
        .await
    }

    async fn call_inner(
        &self,
        preferred_sn_peer_id: Option<&P2pId>,
        tunnel_id: TunnelId,
        reverse_endpoints: Option<&[Endpoint]>,
        remote: &P2pId,
        call_type: TunnelType,
        payload_pkg: Vec<u8>,
        nat_context: Option<&NatTraversalContext>,
    ) -> P2pResult<SnCallResp> {
        let active_list = self
            .get_active_sn_list()
            .into_iter()
            .filter(|active| {
                preferred_sn_peer_id
                    .map(|sn_peer_id| &active.sn_peer_id == sn_peer_id)
                    .unwrap_or(true)
            })
            .collect::<Vec<_>>();
        for active in active_list.iter() {
            let seq = self.gen_seq.generate();
            let call = SnCall {
                protocol_version: SN_PROTOCOL_VERSION,
                stack_version: 0,
                seq,
                tunnel_id,
                sn_peer_id: active.sn_peer_id.clone(),
                to_peer_id: remote.clone(),
                from_peer_id: self.local_identity.get_id().clone(),
                reverse_endpoint_array: reverse_endpoints.map(|ep_list| Vec::from(ep_list)),
                active_pn_list: None,
                peer_info: Some(
                    self.local_identity
                        .get_identity_cert()?
                        .get_encoded_cert()?,
                ),
                send_time: bucky_time_now(),
                call_type,
                payload: payload_pkg.clone(),
                is_always_call: false,
                nat_context: nat_context.cloned(),
            };

            log::debug!(
                "sn call send sn={} conn_id={:?} seq={} tunnel_id={:?} remote={} reverse_eps={:?} payload_len={} call_type={:?}",
                active.sn_peer_id,
                active.conn_id,
                seq.value(),
                tunnel_id,
                remote,
                call.reverse_endpoint_array,
                call.payload.len(),
                call.call_type
            );

            let call_body = call
                .to_vec()
                .map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
            let mut resp_body = match self
                .cmd_client
                .send_by_specify_tunnel_with_resp(
                    active.conn_id,
                    PackageCmdCode::SnCall as u8,
                    self.cmd_version,
                    call_body.as_slice(),
                    self.call_timeout,
                )
                .await
            {
                Ok(resp_body) => resp_body,
                Err(e) => {
                    if e.code() != CmdErrorCode::Timeout {
                        self.remove_sn_conn(active.conn_id);
                    }
                    log::warn!(
                        "sn call qa failed sn={} conn_id={:?} seq={} remote={} timeout_ms={} err={:?}",
                        active.sn_peer_id,
                        active.conn_id,
                        seq.value(),
                        remote,
                        self.call_timeout.as_millis(),
                        e
                    );
                    continue;
                }
            };
            let resp = match resp_body.read_all().await {
                Ok(body) => match SnCallResp::clone_from_slice(body.as_slice()) {
                    Ok(resp) => resp,
                    Err(e) => {
                        log::error!("decode sn call qa response failed: {:?}", e);
                        continue;
                    }
                },
                Err(e) => {
                    log::error!("read sn call qa response failed: {:?}", e);
                    continue;
                }
            };
            if resp.seq != seq || resp.sn_peer_id != active.sn_peer_id {
                log::error!(
                    "sn call qa response mismatch conn_id={:?} expected_seq={} actual_seq={} expected_sn={} actual_sn={}",
                    active.conn_id,
                    seq.value(),
                    resp.seq.value(),
                    active.sn_peer_id,
                    resp.sn_peer_id
                );
                continue;
            }
            log::debug!(
                "sn call resp sn={} conn_id={:?} seq={} result={}",
                active.sn_peer_id,
                active.conn_id,
                resp.seq.value(),
                resp.result
            );

            return Ok(resp);
        }
        Err(p2p_err!(P2pErrorCode::ConnectFailed, "call timeout"))
    }

    pub async fn query(&self, device_id: &P2pId) -> P2pResult<SnQueryResp> {
        Ok(self.query_with_context(device_id).await?.response)
    }

    pub async fn query_with_context(&self, device_id: &P2pId) -> P2pResult<SnQueryResult> {
        let active_list = self.get_active_sn_list();
        for active in active_list.iter() {
            let seq = self.gen_seq.generate();
            let query = SnQuery {
                protocol_version: SN_PROTOCOL_VERSION,
                stack_version: 0,
                seq,
                query_id: device_id.clone(),
            };
            let query_body = query
                .to_vec()
                .map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
            let mut resp_body = match self
                .cmd_client
                .send_by_specify_tunnel_with_resp(
                    active.conn_id,
                    PackageCmdCode::SnQuery as u8,
                    self.cmd_version,
                    query_body.as_slice(),
                    self.call_timeout,
                )
                .await
            {
                Ok(resp_body) => resp_body,
                Err(e) => {
                    if e.code() != CmdErrorCode::Timeout {
                        self.remove_sn_conn(active.conn_id);
                    }
                    log::error!("query qa to {} failed: {:?}", active.sn_peer_id, e);
                    continue;
                }
            };
            let resp = match resp_body.read_all().await {
                Ok(body) => match SnQueryResp::clone_from_slice(body.as_slice()) {
                    Ok(resp) => resp,
                    Err(e) => {
                        log::error!("decode sn query qa response failed: {:?}", e);
                        continue;
                    }
                },
                Err(e) => {
                    log::error!("read sn query qa response failed: {:?}", e);
                    continue;
                }
            };
            if resp.seq != seq {
                log::error!(
                    "sn query qa response mismatch conn_id={:?} expected_seq={} actual_seq={}",
                    active.conn_id,
                    seq.value(),
                    resp.seq.value()
                );
                continue;
            }

            return Ok(SnQueryResult {
                sn_peer_id: active.sn_peer_id.clone(),
                local_net_profile: active.net_profile.clone(),
                response: resp,
            });
        }
        Err(p2p_err!(P2pErrorCode::ConnectFailed, "no active sn"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn endpoint(protocol: Protocol, addr: &str) -> Endpoint {
        Endpoint::from((protocol, addr.parse().unwrap()))
    }

    #[test]
    fn sn_client_listener_entries_are_quic_first_then_tcp() {
        let quic = endpoint(Protocol::Quic, "127.0.0.1:10001");
        let tcp = endpoint(Protocol::Tcp, "127.0.0.1:10002");
        let ext = endpoint(Protocol::Ext(7), "127.0.0.1:10003");

        let ordered = sort_sn_client_listener_entries(vec![
            (
                Protocol::Tcp,
                vec![TunnelListenerInfo {
                    local: tcp,
                    mapping_port: None,
                }],
            ),
            (
                Protocol::Ext(7),
                vec![TunnelListenerInfo {
                    local: ext,
                    mapping_port: None,
                }],
            ),
            (
                Protocol::Quic,
                vec![TunnelListenerInfo {
                    local: quic,
                    mapping_port: None,
                }],
            ),
        ]);

        let protocols: Vec<_> = ordered.into_iter().map(|(protocol, _)| protocol).collect();
        assert_eq!(
            protocols,
            vec![Protocol::Quic, Protocol::Tcp, Protocol::Ext(7)]
        );
    }

    #[test]
    fn sn_client_protocol_candidates_include_supported_protocol_without_listener() {
        let tcp = endpoint(Protocol::Tcp, "127.0.0.1:10002");
        let tcp_ephemeral = endpoint(Protocol::Tcp, "127.0.0.1:0");

        let candidates = sn_client_protocol_candidates(
            vec![(
                Protocol::Tcp,
                vec![TunnelListenerInfo {
                    local: tcp,
                    mapping_port: None,
                }],
            )],
            vec![Protocol::Quic, Protocol::Tcp],
        );

        assert_eq!(candidates[0], (Protocol::Quic, vec![None]));
        assert_eq!(candidates[1], (Protocol::Tcp, vec![Some(tcp_ephemeral)]));
    }

    #[test]
    fn sn_client_protocol_candidates_preserve_quic_listener_local_ep() {
        let quic = endpoint(Protocol::Quic, "127.0.0.1:10001");
        let tcp = endpoint(Protocol::Tcp, "127.0.0.1:10002");
        let tcp_ephemeral = endpoint(Protocol::Tcp, "127.0.0.1:0");

        let candidates = sn_client_protocol_candidates(
            vec![
                (
                    Protocol::Tcp,
                    vec![TunnelListenerInfo {
                        local: tcp,
                        mapping_port: None,
                    }],
                ),
                (
                    Protocol::Quic,
                    vec![TunnelListenerInfo {
                        local: quic,
                        mapping_port: None,
                    }],
                ),
            ],
            vec![Protocol::Quic, Protocol::Tcp],
        );

        assert_eq!(candidates[0], (Protocol::Quic, vec![Some(quic)]));
        assert_eq!(candidates[1], (Protocol::Tcp, vec![Some(tcp_ephemeral)]));
    }

    #[test]
    fn sn_client_protocol_candidates_do_not_bind_unspecified_tcp_listener_port() {
        let tcp = endpoint(Protocol::Tcp, "0.0.0.0:10002");

        let candidates = sn_client_protocol_candidates(
            vec![(
                Protocol::Tcp,
                vec![TunnelListenerInfo {
                    local: tcp,
                    mapping_port: None,
                }],
            )],
            vec![Protocol::Tcp],
        );

        assert_eq!(candidates[0], (Protocol::Tcp, vec![None]));
    }

    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/unit/sn_tests/client/nat_probe_directive_tests.rs"
    ));
}
