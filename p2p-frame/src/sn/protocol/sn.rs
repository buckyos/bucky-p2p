use super::v0::TunnelType;
use crate::endpoint::{
    Endpoint, Protocol, rendezvous_eligible_area, rendezvous_reverse_connect_eligible_area,
};
use crate::error::{P2pError, P2pErrorCode, P2pResult, p2p_err};
use crate::nat_type::{NAT_TRAVERSAL_CONTEXT_VERSION, NatProfile, NatTraversalContext};
use crate::p2p_identity::{EncodedP2pIdentityCert, P2pId, P2pIdentity, P2pSignature};
use crate::sn::nat_probe::MAX_NAT_PROBE_ENDPOINTS;
use crate::types::{Sequence, Timestamp, TunnelId};
use bucky_raw_codec::{
    CodecError, CodecErrorCode, RawDecode, RawEncode, RawEncodePurpose, RawFixedBytes,
};
use bucky_time::{MIN_BUCKY_TIME, bucky_time_to_system_time, system_time_to_bucky_time};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const SN_EXTENSION_VERSION: u8 = 1;
const REPORT_SN_EXTENSION_MAGIC: u32 = u32::from_be_bytes(*b"RSNP");
const REPORT_SN_PROBE_RESULT_EXTENSION_MAGIC: u32 = u32::from_be_bytes(*b"RSNR");
const REPORT_SN_RESP_EXTENSION_MAGIC: u32 = u32::from_be_bytes(*b"RSRP");
const REPORT_SN_RESP_PROBE_DIRECTIVE_EXTENSION_MAGIC: u32 = u32::from_be_bytes(*b"RSRD");
const SN_QUERY_RESP_EXTENSION_MAGIC: u32 = u32::from_be_bytes(*b"SQRP");
const SN_DETAIL_RESP_EXTENSION_MAGIC: u32 = u32::from_be_bytes(*b"SDRP");
const SN_QUERY_RESP_PROTOCOL_VERSION_EXTENSION_MAGIC: u32 = u32::from_be_bytes(*b"SQPV");
const SN_DETAIL_RESP_PROTOCOL_VERSION_EXTENSION_MAGIC: u32 = u32::from_be_bytes(*b"SDPV");
const SN_CALL_EXTENSION_MAGIC: u32 = u32::from_be_bytes(*b"SCAL");
pub(super) const SN_CALLED_EXTENSION_MAGIC: u32 = u32::from_be_bytes(*b"SCLD");

const SN_EXTENSION_HEADER_LEN: usize = 4 + 1 + 4;

/// Current coarse-grained SN application protocol baseline.
///
/// Version `0` remains a valid legacy version. Unknown target versions are
/// represented separately with `Option::None` in query/detail responses.
pub const SN_PROTOCOL_VERSION: u8 = 1;

/// Wire version shared by SN-owned NAT-probe directives and their correlated
/// client results.
pub const NAT_PROBE_CONTROL_VERSION: u8 = 1;

/// Command-header version of the independent tunnel rendezvous command family.
pub const SN_TUNNEL_RENDEZVOUS_CMD_VERSION: u8 = 1;

pub const SN_TUNNEL_RENDEZVOUS_RESULT_OK: u8 = P2pErrorCode::Ok as u8;
pub const SN_TUNNEL_RENDEZVOUS_RESULT_FAILED: u8 = P2pErrorCode::Failed as u8;

/// Hard per-message endpoint budget for rendezvous actions and prediction results.
pub const MAX_SN_TUNNEL_RENDEZVOUS_ENDPOINTS: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, RawEncode, RawDecode)]
pub enum SnTunnelRendezvousOperation {
    PunchOnly,
    PunchAndReverseConnect,
    ReverseConnectOnly,
    WaitIncoming,
}

impl SnTunnelRendezvousOperation {
    pub fn requires_endpoints(self) -> bool {
        !matches!(self, Self::WaitIncoming)
    }

    pub fn punches(self) -> bool {
        matches!(self, Self::PunchOnly | Self::PunchAndReverseConnect)
    }

    pub fn reverse_connects(self) -> bool {
        matches!(
            self,
            Self::PunchAndReverseConnect | Self::ReverseConnectOnly
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, RawEncode, RawDecode)]
pub struct SnTunnelRendezvous {
    pub seq: Sequence,
    pub tunnel_id: TunnelId,
    pub to_peer_id: P2pId,
    pub operation: SnTunnelRendezvousOperation,
    pub end_point_array: Vec<Endpoint>,
    pub need_predict_endpoint: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, RawEncode, RawDecode)]
pub struct SnTunnelRendezvousNotify {
    pub seq: Sequence,
    pub tunnel_id: TunnelId,
    pub peer_info: EncodedP2pIdentityCert,
    pub operation: SnTunnelRendezvousOperation,
    pub end_point_array: Vec<Endpoint>,
    pub need_predict_endpoint: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, RawEncode, RawDecode)]
pub struct SnTunnelRendezvousResp {
    pub seq: Sequence,
    pub result: u8,
    pub predicted_endpoint_array: Vec<Endpoint>,
}

fn validate_rendezvous_endpoints(
    endpoints: &[Endpoint],
    operation: SnTunnelRendezvousOperation,
) -> P2pResult<()> {
    if endpoints.len() > MAX_SN_TUNNEL_RENDEZVOUS_ENDPOINTS {
        return Err(p2p_err!(
            P2pErrorCode::OutOfLimit,
            "rendezvous endpoint count exceeds {}",
            MAX_SN_TUNNEL_RENDEZVOUS_ENDPOINTS
        ));
    }
    if operation.requires_endpoints() != !endpoints.is_empty() {
        return Err(p2p_err!(
            P2pErrorCode::InvalidParam,
            "rendezvous operation/endpoint mismatch"
        ));
    }
    let transport = endpoints.first().map(Endpoint::protocol);
    if operation.punches() && transport != Some(Protocol::Quic) {
        return Err(p2p_err!(
            P2pErrorCode::InvalidParam,
            "rendezvous punch operation requires QUIC endpoints"
        ));
    }
    let mut unique = HashSet::with_capacity(endpoints.len());
    for endpoint in endpoints {
        let eligible_area = if operation.reverse_connects() {
            rendezvous_reverse_connect_eligible_area(endpoint)
        } else {
            rendezvous_eligible_area(endpoint)
        };
        if Some(endpoint.protocol()) != transport
            || !matches!(endpoint.protocol(), Protocol::Quic | Protocol::Tcp)
            || !eligible_area
            || endpoint.addr().port() == 0
            || !unique.insert(*endpoint)
        {
            return Err(p2p_err!(
                P2pErrorCode::InvalidParam,
                "invalid or duplicate rendezvous endpoint"
            ));
        }
    }
    Ok(())
}

fn strict_rendezvous_decode<'de, T: RawDecode<'de>>(
    buf: &'de [u8],
    type_name: &str,
) -> Result<T, CodecError> {
    let (value, remainder) = T::raw_decode(buf)?;
    if !remainder.is_empty() {
        return Err(CodecError::new(
            CodecErrorCode::InvalidData,
            format!("{} contains trailing bytes", type_name),
        ));
    }
    Ok(value)
}

impl SnTunnelRendezvous {
    pub fn clone_from_slice(buf: &[u8]) -> Result<Self, CodecError> {
        strict_rendezvous_decode(buf, "SnTunnelRendezvous")
    }

    pub fn validate(&self) -> P2pResult<()> {
        if self.seq.value() == 0 {
            return Err(p2p_err!(
                P2pErrorCode::InvalidParam,
                "rendezvous sequence must not be zero"
            ));
        }
        validate_rendezvous_endpoints(&self.end_point_array, self.operation)
    }
}

impl SnTunnelRendezvousNotify {
    pub fn clone_from_slice(buf: &[u8]) -> Result<Self, CodecError> {
        strict_rendezvous_decode(buf, "SnTunnelRendezvousNotify")
    }

    pub fn validate(&self) -> P2pResult<()> {
        if self.seq.value() == 0 {
            return Err(p2p_err!(
                P2pErrorCode::InvalidParam,
                "rendezvous notify sequence must not be zero"
            ));
        }
        validate_rendezvous_endpoints(&self.end_point_array, self.operation)
    }
}

impl SnTunnelRendezvousResp {
    pub fn clone_from_slice(buf: &[u8]) -> Result<Self, CodecError> {
        strict_rendezvous_decode(buf, "SnTunnelRendezvousResp")
    }

    pub fn success(seq: Sequence, predicted_endpoint_array: Vec<Endpoint>) -> Self {
        Self {
            seq,
            result: SN_TUNNEL_RENDEZVOUS_RESULT_OK,
            predicted_endpoint_array,
        }
    }

    pub fn failure(seq: Sequence) -> Self {
        Self {
            seq,
            result: SN_TUNNEL_RENDEZVOUS_RESULT_FAILED,
            predicted_endpoint_array: Vec::new(),
        }
    }

    pub fn is_success(&self) -> bool {
        self.result == SN_TUNNEL_RENDEZVOUS_RESULT_OK
    }

    pub fn validate(&self, expected_seq: Sequence, need_predict_endpoint: bool) -> P2pResult<()> {
        if self.seq != expected_seq {
            return Err(p2p_err!(
                P2pErrorCode::Unmatch,
                "rendezvous response sequence mismatch"
            ));
        }
        if !matches!(
            self.result,
            SN_TUNNEL_RENDEZVOUS_RESULT_OK | SN_TUNNEL_RENDEZVOUS_RESULT_FAILED
        ) {
            return Err(p2p_err!(
                P2pErrorCode::InvalidData,
                "invalid rendezvous result"
            ));
        }
        if !self.is_success() {
            if !self.predicted_endpoint_array.is_empty() {
                return Err(p2p_err!(
                    P2pErrorCode::InvalidData,
                    "failed rendezvous response contains endpoints"
                ));
            }
            return Ok(());
        }
        if need_predict_endpoint != !self.predicted_endpoint_array.is_empty() {
            return Err(p2p_err!(
                P2pErrorCode::InvalidData,
                "rendezvous prediction response does not match request"
            ));
        }
        if need_predict_endpoint {
            validate_rendezvous_endpoints(
                &self.predicted_endpoint_array,
                SnTunnelRendezvousOperation::PunchOnly,
            )?;
        }
        Ok(())
    }
}

pub(super) fn extension_measure<T: RawEncode>(
    value: Option<&T>,
    purpose: &Option<RawEncodePurpose>,
) -> Result<usize, CodecError> {
    match value {
        Some(value) => Ok(SN_EXTENSION_HEADER_LEN + value.raw_measure(purpose)?),
        None => Ok(0),
    }
}

pub(super) fn extension_encode<'a, T: RawEncode>(
    value: Option<&T>,
    magic: u32,
    buf: &'a mut [u8],
    purpose: &Option<RawEncodePurpose>,
) -> Result<&'a mut [u8], CodecError> {
    let Some(value) = value else {
        return Ok(buf);
    };

    let payload_len = value.raw_measure(purpose)?;
    let payload_len = u32::try_from(payload_len).map_err(|_| {
        CodecError::new(
            CodecErrorCode::OutOfLimit,
            "SN extension payload is too large",
        )
    })?;
    let buf = magic.raw_encode(buf, purpose)?;
    let buf = SN_EXTENSION_VERSION.raw_encode(buf, purpose)?;
    let buf = payload_len.raw_encode(buf, purpose)?;
    value.raw_encode(buf, purpose)
}

/// Decode an optional extension without allowing a malformed extension to
/// invalidate an otherwise valid legacy message.
///
/// A non-matching magic is left untouched for an enclosing decoder. Once the
/// magic matches this message, the remainder belongs to the extension: known
/// envelopes are skipped and malformed/truncated envelopes are consumed while
/// yielding `None`.
pub(super) fn extension_decode<'de, T: RawDecode<'de>>(
    buf: &'de [u8],
    magic: u32,
) -> (Option<T>, &'de [u8]) {
    let original = buf;
    if buf.len() < u32::raw_bytes().unwrap() {
        return (None, original);
    }
    let Ok((tail_magic, after_magic)) = u32::raw_decode(buf) else {
        return (None, original);
    };
    if tail_magic != magic {
        return (None, original);
    }

    let Ok((version, after_version)) = u8::raw_decode(after_magic) else {
        return (None, &original[original.len()..]);
    };
    let Ok((payload_len, after_header)) = u32::raw_decode(after_version) else {
        return (None, &original[original.len()..]);
    };
    let Ok(payload_len) = usize::try_from(payload_len) else {
        return (None, &original[original.len()..]);
    };
    if after_header.len() < payload_len {
        return (None, &original[original.len()..]);
    }

    let (payload, remainder) = after_header.split_at(payload_len);
    if version != SN_EXTENSION_VERSION {
        return (None, remainder);
    }

    match T::raw_decode(payload) {
        Ok((value, payload_remainder)) if payload_remainder.is_empty() => (Some(value), remainder),
        _ => (None, remainder),
    }
}

fn supported_nat_profile(profile: Option<NatProfile>) -> Option<NatProfile> {
    profile.filter(NatProfile::is_supported)
}

fn supported_nat_probe_ports(ports: &[u16]) -> bool {
    (2..=MAX_NAT_PROBE_ENDPOINTS).contains(&ports.len())
        && ports.iter().all(|port| *port != 0)
        && ports.iter().copied().collect::<HashSet<_>>().len() == ports.len()
}

/// A single SN-authorized NAT-probe operation.
///
/// All identity and generation fields are echoed by [`NatProbeResult`]. This
/// lets the SN reject results from another registration, probe configuration,
/// peer, or SN even when a request identifier is replayed.
#[derive(Clone, Debug, Eq, PartialEq, RawEncode, RawDecode)]
pub struct NatProbeDirective {
    pub version: u8,
    pub sn_peer_id: P2pId,
    pub peer_id: P2pId,
    pub registration_generation: u64,
    pub request_id: u64,
    pub probe_config_generation: u64,
    pub expires_at: Timestamp,
    pub ports: Vec<u16>,
}

impl NatProbeDirective {
    pub fn is_supported(&self) -> bool {
        self.version == NAT_PROBE_CONTROL_VERSION && supported_nat_probe_ports(&self.ports)
    }
}

/// Correlated result for an SN-owned NAT-probe directive.
#[derive(Clone, Debug, Eq, PartialEq, RawEncode, RawDecode)]
pub struct NatProbeResult {
    pub version: u8,
    pub sn_peer_id: P2pId,
    pub peer_id: P2pId,
    pub registration_generation: u64,
    pub request_id: u64,
    pub probe_config_generation: u64,
    pub profile: NatProfile,
}

impl NatProbeResult {
    pub fn from_directive(directive: &NatProbeDirective, profile: NatProfile) -> Self {
        Self {
            version: directive.version,
            sn_peer_id: directive.sn_peer_id.clone(),
            peer_id: directive.peer_id.clone(),
            registration_generation: directive.registration_generation,
            request_id: directive.request_id,
            probe_config_generation: directive.probe_config_generation,
            profile,
        }
    }

    pub fn is_supported(&self) -> bool {
        self.version == NAT_PROBE_CONTROL_VERSION && self.profile.is_supported()
    }
}

pub(super) fn supported_nat_context(
    context: Option<NatTraversalContext>,
) -> Option<NatTraversalContext> {
    context.filter(|context| {
        context.version == NAT_TRAVERSAL_CONTEXT_VERSION
            && context.caller_profile.is_supported()
            && context.callee_profile.is_supported()
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(u8)]
pub enum InterSnCommandCode {
    Heartbeat = 0x80,
    PublishLease = 0x81,
    QueryLease = 0x82,
    QueryDetail = 0x83,
    RelayCall = 0x84,
    RelayRendezvousV2 = 0x86,
}

impl TryFrom<u8> for InterSnCommandCode {
    type Error = P2pError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x80 => Ok(Self::Heartbeat),
            0x81 => Ok(Self::PublishLease),
            0x82 => Ok(Self::QueryLease),
            0x83 => Ok(Self::QueryDetail),
            0x84 => Ok(Self::RelayCall),
            0x86 => Ok(Self::RelayRendezvousV2),
            _ => Err(P2pError::new(
                P2pErrorCode::InvalidParam,
                format!("invalid inter-sn command code: {}", value),
            )),
        }
    }
}

impl RawFixedBytes for InterSnCommandCode {
    fn raw_bytes() -> Option<usize> {
        Some(1)
    }
}

impl RawEncode for InterSnCommandCode {
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
                "not enough buffer for inter-sn command code",
            ));
        }
        buf[0] = *self as u8;
        Ok(&mut buf[1..])
    }
}

impl<'de> RawDecode<'de> for InterSnCommandCode {
    fn raw_decode(buf: &'de [u8]) -> Result<(Self, &'de [u8]), CodecError> {
        if buf.is_empty() {
            return Err(CodecError::new(
                CodecErrorCode::OutOfLimit,
                "not enough buffer for inter-sn command code",
            ));
        }
        let code = Self::try_from(buf[0]).map_err(|err| {
            CodecError::new(
                CodecErrorCode::Failed,
                format!("decode inter-sn command code failed: {:?}", err),
            )
        })?;
        Ok((code, &buf[1..]))
    }
}

#[derive(Clone, Debug, RawEncode, RawDecode)]
pub struct SnOwnerHeartbeat {
    pub member_sn_id: P2pId,
}

#[derive(Clone, Debug, RawEncode, RawDecode)]
pub struct SnPublishLease {
    pub peer_id: P2pId,
    pub serving_sn_id: P2pId,
    pub sequence: u64,
    pub expires_at: Timestamp,
}

#[derive(Clone, Debug, RawEncode, RawDecode)]
pub struct SnQueryLease {
    pub peer_id: P2pId,
}

#[derive(Clone, Debug, RawEncode, RawDecode)]
pub struct SnQueryLeaseResp {
    pub leases: Vec<SnPublishLease>,
}

#[derive(Clone, Debug, RawEncode, RawDecode)]
pub struct SnDetailQuery {
    pub peer_id: P2pId,
}

#[derive(Clone, Debug)]
pub struct SnDetailResp {
    pub peer_info: Option<EncodedP2pIdentityCert>,
    pub end_point_array: Vec<Endpoint>,
    pub net_profile: Option<NatProfile>,
    pub target_protocol_version: Option<u8>,
}

#[derive(Clone, Debug, RawEncode, RawDecode)]
struct LegacySnDetailResp {
    peer_info: Option<EncodedP2pIdentityCert>,
    end_point_array: Vec<Endpoint>,
}

impl From<&SnDetailResp> for LegacySnDetailResp {
    fn from(value: &SnDetailResp) -> Self {
        Self {
            peer_info: value.peer_info.clone(),
            end_point_array: value.end_point_array.clone(),
        }
    }
}

impl RawEncode for SnDetailResp {
    fn raw_measure(&self, purpose: &Option<RawEncodePurpose>) -> Result<usize, CodecError> {
        Ok(LegacySnDetailResp::from(self).raw_measure(purpose)?
            + extension_measure(self.net_profile.as_ref(), purpose)?
            + extension_measure(self.target_protocol_version.as_ref(), purpose)?)
    }

    fn raw_encode<'a>(
        &self,
        buf: &'a mut [u8],
        purpose: &Option<RawEncodePurpose>,
    ) -> Result<&'a mut [u8], CodecError> {
        let buf = LegacySnDetailResp::from(self).raw_encode(buf, purpose)?;
        let buf = extension_encode(
            self.net_profile.as_ref(),
            SN_DETAIL_RESP_EXTENSION_MAGIC,
            buf,
            purpose,
        )?;
        extension_encode(
            self.target_protocol_version.as_ref(),
            SN_DETAIL_RESP_PROTOCOL_VERSION_EXTENSION_MAGIC,
            buf,
            purpose,
        )
    }
}

impl<'de> RawDecode<'de> for SnDetailResp {
    fn raw_decode(buf: &'de [u8]) -> Result<(Self, &'de [u8]), CodecError> {
        let (legacy, buf) = LegacySnDetailResp::raw_decode(buf)?;
        let (net_profile, buf) = extension_decode(buf, SN_DETAIL_RESP_EXTENSION_MAGIC);
        let net_profile = supported_nat_profile(net_profile);
        let (target_protocol_version, buf) =
            extension_decode(buf, SN_DETAIL_RESP_PROTOCOL_VERSION_EXTENSION_MAGIC);
        Ok((
            Self {
                peer_info: legacy.peer_info,
                end_point_array: legacy.end_point_array,
                net_profile,
                target_protocol_version,
            },
            buf,
        ))
    }
}

#[derive(Clone, Debug, RawEncode, RawDecode)]
pub struct SnRelayCall {
    pub call: SnCall,
}

#[derive(Clone)]
pub struct SnCall {
    pub protocol_version: u8,
    pub stack_version: u32,
    pub seq: Sequence,
    pub tunnel_id: TunnelId,
    pub sn_peer_id: P2pId,
    pub to_peer_id: P2pId,
    pub from_peer_id: P2pId,
    pub reverse_endpoint_array: Option<Vec<Endpoint>>,
    pub active_pn_list: Option<Vec<P2pId>>,
    pub peer_info: Option<EncodedP2pIdentityCert>,
    pub send_time: Timestamp,
    pub call_type: TunnelType,
    pub payload: Vec<u8>,
    pub is_always_call: bool,
    pub nat_context: Option<NatTraversalContext>,
}

#[derive(Clone, RawEncode, RawDecode)]
struct LegacySnCall {
    protocol_version: u8,
    stack_version: u32,
    seq: Sequence,
    tunnel_id: TunnelId,
    sn_peer_id: P2pId,
    to_peer_id: P2pId,
    from_peer_id: P2pId,
    reverse_endpoint_array: Option<Vec<Endpoint>>,
    active_pn_list: Option<Vec<P2pId>>,
    peer_info: Option<EncodedP2pIdentityCert>,
    send_time: Timestamp,
    call_type: TunnelType,
    payload: Vec<u8>,
    is_always_call: bool,
}

impl From<&SnCall> for LegacySnCall {
    fn from(value: &SnCall) -> Self {
        Self {
            protocol_version: value.protocol_version,
            stack_version: value.stack_version,
            seq: value.seq,
            tunnel_id: value.tunnel_id,
            sn_peer_id: value.sn_peer_id.clone(),
            to_peer_id: value.to_peer_id.clone(),
            from_peer_id: value.from_peer_id.clone(),
            reverse_endpoint_array: value.reverse_endpoint_array.clone(),
            active_pn_list: value.active_pn_list.clone(),
            peer_info: value.peer_info.clone(),
            send_time: value.send_time,
            call_type: value.call_type,
            payload: value.payload.clone(),
            is_always_call: value.is_always_call,
        }
    }
}

impl RawEncode for SnCall {
    fn raw_measure(&self, purpose: &Option<RawEncodePurpose>) -> Result<usize, CodecError> {
        Ok(LegacySnCall::from(self).raw_measure(purpose)?
            + extension_measure(self.nat_context.as_ref(), purpose)?)
    }

    fn raw_encode<'a>(
        &self,
        buf: &'a mut [u8],
        purpose: &Option<RawEncodePurpose>,
    ) -> Result<&'a mut [u8], CodecError> {
        let buf = LegacySnCall::from(self).raw_encode(buf, purpose)?;
        extension_encode(
            self.nat_context.as_ref(),
            SN_CALL_EXTENSION_MAGIC,
            buf,
            purpose,
        )
    }
}

impl<'de> RawDecode<'de> for SnCall {
    fn raw_decode(buf: &'de [u8]) -> Result<(Self, &'de [u8]), CodecError> {
        let (legacy, buf) = LegacySnCall::raw_decode(buf)?;
        let (nat_context, buf) = extension_decode(buf, SN_CALL_EXTENSION_MAGIC);
        let nat_context = supported_nat_context(nat_context);
        Ok((
            Self {
                protocol_version: legacy.protocol_version,
                stack_version: legacy.stack_version,
                seq: legacy.seq,
                tunnel_id: legacy.tunnel_id,
                sn_peer_id: legacy.sn_peer_id,
                to_peer_id: legacy.to_peer_id,
                from_peer_id: legacy.from_peer_id,
                reverse_endpoint_array: legacy.reverse_endpoint_array,
                active_pn_list: legacy.active_pn_list,
                peer_info: legacy.peer_info,
                send_time: legacy.send_time,
                call_type: legacy.call_type,
                payload: legacy.payload,
                is_always_call: legacy.is_always_call,
                nat_context,
            },
            buf,
        ))
    }
}

impl std::fmt::Debug for SnCall {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "SnCall:{{seq:{:?}, tunnel_id:{:?}, sn_peer_id:{:?}, to_peer_id:{}, from_peer_id:{:?}, reverse_endpoint_array:{:?}, active_pn_list:{:?}, peer_info:{}, payload:{}, nat_context:{}}}",
            self.seq,
            self.tunnel_id,
            self.sn_peer_id,
            self.to_peer_id,
            self.from_peer_id,
            self.reverse_endpoint_array,
            self.active_pn_list,
            self.peer_info.is_some(),
            self.payload.len(),
            self.nat_context.is_some()
        )
    }
}

#[derive(Clone, Copy, PartialEq, PartialOrd, Ord, Eq, Debug)]
pub enum SnServiceGrade {
    None = 0,
    Discard = 1,
    Passable = 2,
    Normal = 3,
    Fine = 4,
    Wonderfull = 5,
}

impl SnServiceGrade {
    pub fn is_accept(&self) -> bool {
        *self >= SnServiceGrade::Passable
    }
    pub fn is_refuse(&self) -> bool {
        !self.is_accept()
    }
}

impl TryFrom<u8> for SnServiceGrade {
    type Error = P2pError;
    fn try_from(v: u8) -> Result<Self, Self::Error> {
        match v {
            0 => Ok(Self::None),
            1 => Ok(Self::Discard),
            2 => Ok(Self::Passable),
            3 => Ok(Self::Normal),
            4 => Ok(Self::Fine),
            5 => Ok(Self::Wonderfull),
            _ => Err(P2pError::new(
                P2pErrorCode::InvalidParam,
                "invalid SnServiceGrade value".to_string(),
            )),
        }
    }
}

impl RawFixedBytes for SnServiceGrade {
    fn raw_bytes() -> Option<usize> {
        Some(1)
    }
}

impl RawEncode for SnServiceGrade {
    fn raw_measure(&self, _purpose: &Option<RawEncodePurpose>) -> Result<usize, CodecError> {
        Ok(Self::raw_bytes().unwrap())
    }

    fn raw_encode<'a>(
        &self,
        buf: &'a mut [u8],
        _purpose: &Option<RawEncodePurpose>,
    ) -> Result<&'a mut [u8], CodecError> {
        let bytes = Self::raw_bytes().unwrap();
        if buf.len() < bytes {
            let msg = format!(
                "not enough buffer for encode SnServiceGrade, except={}, got={}",
                bytes,
                buf.len()
            );
            error!("{}", msg);

            return Err(CodecError::new(CodecErrorCode::OutOfLimit, msg));
        }
        buf[0] = (*self) as u8;
        Ok(&mut buf[1..])
    }
}

impl<'de> RawDecode<'de> for SnServiceGrade {
    fn raw_decode(buf: &'de [u8]) -> Result<(Self, &'de [u8]), CodecError> {
        let bytes = Self::raw_bytes().unwrap();
        if buf.len() < bytes {
            let msg = format!(
                "not enough buffer for decode SnServiceGrade, except={}, got={}",
                bytes,
                buf.len()
            );
            error!("{}", msg);

            return Err(CodecError::new(CodecErrorCode::OutOfLimit, msg));
        }
        let v = Self::try_from(buf[0]).map_err(|e| {
            CodecError::new(
                CodecErrorCode::Failed,
                format!("decode sn service grade failed.{:?}", e),
            )
        })?;
        Ok((v, &buf[Self::raw_bytes().unwrap()..]))
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum SnServiceReceiptVersion {
    Invalid = 0,
    Current = 1,
}

impl TryFrom<u8> for SnServiceReceiptVersion {
    type Error = P2pError;
    fn try_from(v: u8) -> Result<Self, Self::Error> {
        match v {
            0 => Ok(Self::Invalid),
            1 => Ok(Self::Current),
            _ => Err(P2pError::new(
                P2pErrorCode::UnSupport,
                format!("unsupport version({})", v),
            )),
        }
    }
}

impl RawFixedBytes for SnServiceReceiptVersion {
    fn raw_bytes() -> Option<usize> {
        Some(1)
    }
}

impl RawEncode for SnServiceReceiptVersion {
    fn raw_measure(&self, _purpose: &Option<RawEncodePurpose>) -> Result<usize, CodecError> {
        Ok(Self::raw_bytes().unwrap())
    }

    fn raw_encode<'a>(
        &self,
        buf: &'a mut [u8],
        _purpose: &Option<RawEncodePurpose>,
    ) -> Result<&'a mut [u8], CodecError> {
        let bytes = Self::raw_bytes().unwrap();
        if buf.len() < bytes {
            let msg = format!(
                "not enough buffer for encode SnServiceReceiptVersion, except={}, got={}",
                bytes,
                buf.len()
            );
            error!("{}", msg);

            return Err(CodecError::new(CodecErrorCode::OutOfLimit, msg));
        }
        buf[0] = (*self) as u8;
        Ok(&mut buf[1..])
    }
}

impl<'de> RawDecode<'de> for SnServiceReceiptVersion {
    fn raw_decode(buf: &'de [u8]) -> Result<(Self, &'de [u8]), CodecError> {
        let bytes = Self::raw_bytes().unwrap();
        if buf.len() < bytes {
            let msg = format!(
                "not enough buffer for decode SnServiceReceiptVersion, except={}, got={}",
                bytes,
                buf.len()
            );
            error!("{}", msg);

            return Err(CodecError::new(CodecErrorCode::OutOfLimit, msg));
        }
        let v = Self::try_from(buf[0]).map_err(|e| {
            CodecError::new(
                CodecErrorCode::Failed,
                format!("decode sn service receipt version failed.{:?}", e),
            )
        })?;
        Ok((v, &buf[Self::raw_bytes().unwrap()..]))
    }
}

struct SnServiceReceiptSignature {
    sn_peerid: P2pId,
    receipt: SnServiceReceipt,
}

impl RawEncode for SnServiceReceiptSignature {
    fn raw_measure(&self, purpose: &Option<RawEncodePurpose>) -> Result<usize, CodecError> {
        let len = self.sn_peerid.raw_measure(purpose)? + self.receipt.raw_measure(purpose)?;
        Ok(len)
    }

    fn raw_encode<'a>(
        &self,
        buf: &'a mut [u8],
        purpose: &Option<RawEncodePurpose>,
    ) -> Result<&'a mut [u8], CodecError> {
        let buf = self.sn_peerid.raw_encode(buf, purpose)?;
        self.receipt.raw_encode(buf, purpose)
    }
}

#[derive(Copy, Clone, Debug)]
pub struct SnServiceReceipt {
    pub version: SnServiceReceiptVersion,
    pub grade: SnServiceGrade,
    pub rto: u16,
    pub duration: Duration,
    pub start_time: SystemTime,
    pub ping_count: u32,
    pub ping_resp_count: u32,
    pub called_count: u32,
    pub call_peer_count: u32,
    pub connect_peer_count: u32,
    pub call_delay: u16,
}

impl SnServiceReceipt {
    pub fn sign(
        &self,
        sn_peerid: &P2pId,
        _private_key: &Arc<dyn P2pIdentity>,
    ) -> Result<P2pSignature, P2pError> {
        let _sig_fields = SnServiceReceiptSignature {
            sn_peerid: sn_peerid.clone(),
            receipt: self.clone(),
        };
        //FIMXE: sign
        unimplemented!()
        // Authorized::sign(&sig_fields, private_key)
    }

    pub fn verify(
        &self,
        sn_peerid: &P2pId,
        _sign: &P2pSignature,
        _const_info: &EncodedP2pIdentityCert,
    ) -> bool {
        let _sig_fields = SnServiceReceiptSignature {
            sn_peerid: sn_peerid.clone(),
            receipt: self.clone(),
        };
        //FIMXE: verify
        unimplemented!()
        //Authorized::verify(&sig_fields, sign, const_info)
    }
}

impl Default for SnServiceReceipt {
    fn default() -> Self {
        SnServiceReceipt {
            version: SnServiceReceiptVersion::Invalid,
            grade: SnServiceGrade::None,
            rto: 0,
            duration: Duration::from_secs(0),
            start_time: UNIX_EPOCH,
            ping_count: 0,
            ping_resp_count: 0,
            called_count: 0,
            call_peer_count: 0,
            connect_peer_count: 0,
            call_delay: 0,
        }
    }
}

impl RawEncode for SnServiceReceipt {
    fn raw_measure(&self, purpose: &Option<RawEncodePurpose>) -> Result<usize, CodecError> {
        let mut size = self.version.raw_measure(purpose)?;
        size += self.grade.raw_measure(purpose)?;
        size += self.rto.raw_measure(purpose)?;
        size += 0u32.raw_measure(purpose)?;
        size += 0u64.raw_measure(purpose)?;
        size += self.ping_count.raw_measure(purpose)?;
        size += self.ping_resp_count.raw_measure(purpose)?;
        size += self.called_count.raw_measure(purpose)?;
        size += self.call_peer_count.raw_measure(purpose)?;
        size += self.connect_peer_count.raw_measure(purpose)?;
        size += self.call_delay.raw_measure(purpose)?;
        Ok(size)
    }

    fn raw_encode<'a>(
        &self,
        buf: &'a mut [u8],
        purpose: &Option<RawEncodePurpose>,
    ) -> Result<&'a mut [u8], CodecError> {
        let buf = self.version.raw_encode(buf, purpose)?;
        let buf = self.grade.raw_encode(buf, purpose)?;
        let buf = self.rto.raw_encode(buf, purpose)?;
        let buf = (self.duration.as_millis() as u32).raw_encode(buf, purpose)?;
        let buf = system_time_to_bucky_time(&self.start_time).raw_encode(buf, purpose)?;
        let buf = self.ping_count.raw_encode(buf, purpose)?;
        let buf = self.ping_resp_count.raw_encode(buf, purpose)?;
        let buf = self.called_count.raw_encode(buf, purpose)?;
        let buf = self.call_peer_count.raw_encode(buf, purpose)?;
        let buf = self.connect_peer_count.raw_encode(buf, purpose)?;
        let buf = self.call_delay.raw_encode(buf, purpose)?;
        Ok(buf)
    }
}

impl<'de> RawDecode<'de> for SnServiceReceipt {
    fn raw_decode(buf: &'de [u8]) -> Result<(Self, &'de [u8]), CodecError> {
        let (version, buf) = SnServiceReceiptVersion::raw_decode(buf)?;
        let (grade, buf) = SnServiceGrade::raw_decode(buf)?;
        let (rto, buf) = u16::raw_decode(buf)?;
        let (duration, buf) = u32::raw_decode(buf)?;
        let duration = Duration::from_millis(duration as u64);
        let (timestamp, buf) = Timestamp::raw_decode(buf)?;
        if timestamp < MIN_BUCKY_TIME {
            return Err(CodecError::new(
                CodecErrorCode::InvalidData,
                "invalid timestamp",
            ));
        }
        let start_time = bucky_time_to_system_time(timestamp);
        let (ping_count, buf) = u32::raw_decode(buf)?;
        let (ping_resp_count, buf) = u32::raw_decode(buf)?;
        let (called_count, buf) = u32::raw_decode(buf)?;
        let (call_peer_count, buf) = u32::raw_decode(buf)?;
        let (connect_peer_count, buf) = u32::raw_decode(buf)?;
        let (call_delay, buf) = u16::raw_decode(buf)?;
        Ok((
            SnServiceReceipt {
                version,
                grade,
                rto,
                duration,
                start_time,
                ping_count,
                ping_resp_count,
                called_count,
                call_peer_count,
                connect_peer_count,
                call_delay,
            },
            buf,
        ))
    }
}

#[derive(Debug, Clone)]
pub struct ReceiptWithSignature(SnServiceReceipt, P2pSignature);

impl ReceiptWithSignature {
    pub fn receipt(&self) -> &SnServiceReceipt {
        &self.0
    }

    pub fn signature(&self) -> &P2pSignature {
        &self.1
    }
}

impl RawEncode for ReceiptWithSignature {
    fn raw_measure(&self, purpose: &Option<RawEncodePurpose>) -> Result<usize, CodecError> {
        Ok(self.0.raw_measure(purpose)? + self.1.raw_measure(purpose)?)
    }

    fn raw_encode<'a>(
        &self,
        buf: &'a mut [u8],
        purpose: &Option<RawEncodePurpose>,
    ) -> Result<&'a mut [u8], CodecError> {
        let buf = self.0.raw_encode(buf, purpose)?;
        let buf = self.1.raw_encode(buf, purpose)?;
        Ok(buf)
    }
}

impl<'de> RawDecode<'de> for ReceiptWithSignature {
    fn raw_decode(buf: &'de [u8]) -> Result<(Self, &'de [u8]), CodecError> {
        let (receipt, buf) = RawDecode::raw_decode(buf)?;
        let (sig, buf) = RawDecode::raw_decode(buf)?;
        Ok((Self(receipt, sig), buf))
    }
}

impl From<(SnServiceReceipt, P2pSignature)> for ReceiptWithSignature {
    fn from(v: (SnServiceReceipt, P2pSignature)) -> Self {
        Self(v.0, v.1)
    }
}

#[derive(Debug, Clone)]
pub struct ReportSn {
    pub protocol_version: u8,
    pub stack_version: u32,
    //ln与sn的keepalive包
    pub seq: Sequence,                             //序列号
    pub sn_peer_id: P2pId,                         //sn的设备id
    pub from_peer_id: Option<P2pId>,               //发送者设备id
    pub peer_info: Option<EncodedP2pIdentityCert>, //发送者设备信息
    pub send_time: Timestamp,                      //发送时间
    pub contract_id: Option<P2pId>,                //合约文件对象id
    pub receipt: Option<ReceiptWithSignature>,     //客户端提供的服务清单
    pub map_ports: Vec<(Protocol, u16)>,
    pub local_eps: Vec<Endpoint>,
    pub net_profile: Option<NatProfile>,
    pub nat_probe_control_version: Option<u8>,
    pub nat_probe_result: Option<NatProbeResult>,
}

#[derive(Clone, Debug, Eq, PartialEq, RawEncode, RawDecode)]
struct ReportSnProbeControl {
    version: u8,
    result: Option<NatProbeResult>,
}

#[derive(Debug, Clone, RawEncode, RawDecode)]
struct LegacyReportSn {
    protocol_version: u8,
    stack_version: u32,
    seq: Sequence,
    sn_peer_id: P2pId,
    from_peer_id: Option<P2pId>,
    peer_info: Option<EncodedP2pIdentityCert>,
    send_time: Timestamp,
    contract_id: Option<P2pId>,
    receipt: Option<ReceiptWithSignature>,
    map_ports: Vec<(Protocol, u16)>,
    local_eps: Vec<Endpoint>,
}

impl From<&ReportSn> for LegacyReportSn {
    fn from(value: &ReportSn) -> Self {
        Self {
            protocol_version: value.protocol_version,
            stack_version: value.stack_version,
            seq: value.seq,
            sn_peer_id: value.sn_peer_id.clone(),
            from_peer_id: value.from_peer_id.clone(),
            peer_info: value.peer_info.clone(),
            send_time: value.send_time,
            contract_id: value.contract_id.clone(),
            receipt: value.receipt.clone(),
            map_ports: value.map_ports.clone(),
            local_eps: value.local_eps.clone(),
        }
    }
}

impl RawEncode for ReportSn {
    fn raw_measure(&self, purpose: &Option<RawEncodePurpose>) -> Result<usize, CodecError> {
        let probe_control = (self.nat_probe_control_version.is_some()
            || self.nat_probe_result.is_some())
        .then(|| ReportSnProbeControl {
            version: self.nat_probe_control_version.unwrap_or(0),
            result: self.nat_probe_result.clone(),
        });
        Ok(LegacyReportSn::from(self).raw_measure(purpose)?
            + extension_measure(self.net_profile.as_ref(), purpose)?
            + extension_measure(probe_control.as_ref(), purpose)?)
    }

    fn raw_encode<'a>(
        &self,
        buf: &'a mut [u8],
        purpose: &Option<RawEncodePurpose>,
    ) -> Result<&'a mut [u8], CodecError> {
        let buf = LegacyReportSn::from(self).raw_encode(buf, purpose)?;
        let buf = extension_encode(
            self.net_profile.as_ref(),
            REPORT_SN_EXTENSION_MAGIC,
            buf,
            purpose,
        )?;
        let probe_control = (self.nat_probe_control_version.is_some()
            || self.nat_probe_result.is_some())
        .then(|| ReportSnProbeControl {
            version: self.nat_probe_control_version.unwrap_or(0),
            result: self.nat_probe_result.clone(),
        });
        extension_encode(
            probe_control.as_ref(),
            REPORT_SN_PROBE_RESULT_EXTENSION_MAGIC,
            buf,
            purpose,
        )
    }
}

impl<'de> RawDecode<'de> for ReportSn {
    fn raw_decode(buf: &'de [u8]) -> Result<(Self, &'de [u8]), CodecError> {
        let (legacy, buf) = LegacyReportSn::raw_decode(buf)?;
        let (net_profile, buf) = extension_decode(buf, REPORT_SN_EXTENSION_MAGIC);
        let net_profile = supported_nat_profile(net_profile);
        let (probe_control, buf) =
            extension_decode::<ReportSnProbeControl>(buf, REPORT_SN_PROBE_RESULT_EXTENSION_MAGIC);
        let probe_control =
            probe_control.filter(|control| control.version == NAT_PROBE_CONTROL_VERSION);
        let nat_probe_control_version = probe_control.as_ref().map(|control| control.version);
        let nat_probe_result = probe_control
            .and_then(|control| control.result)
            .filter(NatProbeResult::is_supported);
        Ok((
            Self {
                protocol_version: legacy.protocol_version,
                stack_version: legacy.stack_version,
                seq: legacy.seq,
                sn_peer_id: legacy.sn_peer_id,
                from_peer_id: legacy.from_peer_id,
                peer_info: legacy.peer_info,
                send_time: legacy.send_time,
                contract_id: legacy.contract_id,
                receipt: legacy.receipt,
                map_ports: legacy.map_ports,
                local_eps: legacy.local_eps,
                net_profile,
                nat_probe_control_version,
                nat_probe_result,
            },
            buf,
        ))
    }
}

#[derive(Debug, Clone)]
pub struct ReportSnResp {
    pub seq: Sequence,                             //包序列包
    pub sn_peer_id: P2pId,                         //sn的设备id
    pub result: u8,                                //是否接受device的接入
    pub peer_info: Option<EncodedP2pIdentityCert>, //sn的设备信息
    pub end_point_array: Vec<Endpoint>,            //外网地址列表
    pub receipt: Option<SnServiceReceipt>,         //返回sn的一些连接信息，如当前连接的peer数量
    pub nat_probe_ports: Vec<u16>,
    pub nat_probe_directive: Option<NatProbeDirective>,
}

#[derive(Debug, Clone, RawEncode, RawDecode)]
struct LegacyReportSnResp {
    seq: Sequence,
    sn_peer_id: P2pId,
    result: u8,
    peer_info: Option<EncodedP2pIdentityCert>,
    end_point_array: Vec<Endpoint>,
    receipt: Option<SnServiceReceipt>,
}

impl From<&ReportSnResp> for LegacyReportSnResp {
    fn from(value: &ReportSnResp) -> Self {
        Self {
            seq: value.seq,
            sn_peer_id: value.sn_peer_id.clone(),
            result: value.result,
            peer_info: value.peer_info.clone(),
            end_point_array: value.end_point_array.clone(),
            receipt: value.receipt.clone(),
        }
    }
}

impl RawEncode for ReportSnResp {
    fn raw_measure(&self, purpose: &Option<RawEncodePurpose>) -> Result<usize, CodecError> {
        let extension = (!self.nat_probe_ports.is_empty()).then_some(&self.nat_probe_ports);
        Ok(LegacyReportSnResp::from(self).raw_measure(purpose)?
            + extension_measure(extension, purpose)?
            + extension_measure(self.nat_probe_directive.as_ref(), purpose)?)
    }

    fn raw_encode<'a>(
        &self,
        buf: &'a mut [u8],
        purpose: &Option<RawEncodePurpose>,
    ) -> Result<&'a mut [u8], CodecError> {
        let buf = LegacyReportSnResp::from(self).raw_encode(buf, purpose)?;
        let extension = (!self.nat_probe_ports.is_empty()).then_some(&self.nat_probe_ports);
        let buf = extension_encode(extension, REPORT_SN_RESP_EXTENSION_MAGIC, buf, purpose)?;
        extension_encode(
            self.nat_probe_directive.as_ref(),
            REPORT_SN_RESP_PROBE_DIRECTIVE_EXTENSION_MAGIC,
            buf,
            purpose,
        )
    }
}

impl<'de> RawDecode<'de> for ReportSnResp {
    fn raw_decode(buf: &'de [u8]) -> Result<(Self, &'de [u8]), CodecError> {
        let (legacy, buf) = LegacyReportSnResp::raw_decode(buf)?;
        let (nat_probe_ports, buf) =
            extension_decode::<Vec<u16>>(buf, REPORT_SN_RESP_EXTENSION_MAGIC);
        let nat_probe_ports = nat_probe_ports
            .filter(|ports| supported_nat_probe_ports(ports))
            .unwrap_or_default();
        let (nat_probe_directive, buf) = extension_decode::<NatProbeDirective>(
            buf,
            REPORT_SN_RESP_PROBE_DIRECTIVE_EXTENSION_MAGIC,
        );
        let nat_probe_directive = nat_probe_directive.filter(NatProbeDirective::is_supported);
        Ok((
            Self {
                seq: legacy.seq,
                sn_peer_id: legacy.sn_peer_id,
                result: legacy.result,
                peer_info: legacy.peer_info,
                end_point_array: legacy.end_point_array,
                receipt: legacy.receipt,
                nat_probe_ports,
                nat_probe_directive,
            },
            buf,
        ))
    }
}

#[derive(Debug, Clone, RawEncode, RawDecode)]
pub struct SnQuery {
    pub protocol_version: u8,
    pub stack_version: u32,
    //ln与sn的keepalive包
    pub seq: Sequence, //序列号
    pub query_id: P2pId,
}

#[derive(Debug, Clone)]
pub struct SnQueryResp {
    pub seq: Sequence,                             //包序列包
    pub peer_info: Option<EncodedP2pIdentityCert>, //sn的设备信息
    pub end_point_array: Vec<Endpoint>,            //外网地址列表
    pub net_profile: Option<NatProfile>,
    pub target_protocol_version: Option<u8>,
}

#[derive(Debug, Clone, RawEncode, RawDecode)]
struct LegacySnQueryResp {
    seq: Sequence,
    peer_info: Option<EncodedP2pIdentityCert>,
    end_point_array: Vec<Endpoint>,
}

impl From<&SnQueryResp> for LegacySnQueryResp {
    fn from(value: &SnQueryResp) -> Self {
        Self {
            seq: value.seq,
            peer_info: value.peer_info.clone(),
            end_point_array: value.end_point_array.clone(),
        }
    }
}

impl RawEncode for SnQueryResp {
    fn raw_measure(&self, purpose: &Option<RawEncodePurpose>) -> Result<usize, CodecError> {
        Ok(LegacySnQueryResp::from(self).raw_measure(purpose)?
            + extension_measure(self.net_profile.as_ref(), purpose)?
            + extension_measure(self.target_protocol_version.as_ref(), purpose)?)
    }

    fn raw_encode<'a>(
        &self,
        buf: &'a mut [u8],
        purpose: &Option<RawEncodePurpose>,
    ) -> Result<&'a mut [u8], CodecError> {
        let buf = LegacySnQueryResp::from(self).raw_encode(buf, purpose)?;
        let buf = extension_encode(
            self.net_profile.as_ref(),
            SN_QUERY_RESP_EXTENSION_MAGIC,
            buf,
            purpose,
        )?;
        extension_encode(
            self.target_protocol_version.as_ref(),
            SN_QUERY_RESP_PROTOCOL_VERSION_EXTENSION_MAGIC,
            buf,
            purpose,
        )
    }
}

impl<'de> RawDecode<'de> for SnQueryResp {
    fn raw_decode(buf: &'de [u8]) -> Result<(Self, &'de [u8]), CodecError> {
        let (legacy, buf) = LegacySnQueryResp::raw_decode(buf)?;
        let (net_profile, buf) = extension_decode(buf, SN_QUERY_RESP_EXTENSION_MAGIC);
        let net_profile = supported_nat_profile(net_profile);
        let (target_protocol_version, buf) =
            extension_decode(buf, SN_QUERY_RESP_PROTOCOL_VERSION_EXTENSION_MAGIC);
        Ok((
            Self {
                seq: legacy.seq,
                peer_info: legacy.peer_info,
                end_point_array: legacy.end_point_array,
                net_profile,
                target_protocol_version,
            },
            buf,
        ))
    }
}

#[cfg(test)]
mod nat_type_wire_tests {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/unit/sn_tests/protocol/common/nat_type_wire_tests.rs"
    ));
}
