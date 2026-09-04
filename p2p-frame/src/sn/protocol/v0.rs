use super::sn::*;
use crate::endpoint::Endpoint;
use crate::nat_type::NatTraversalContext;
use crate::p2p_identity::{EncodedP2pIdentityCert, P2pId};
use crate::types::{Sequence, Timestamp, TunnelId};
use bucky_raw_codec::{CodecError, RawDecode, RawEncode, RawEncodePurpose};

#[derive(Copy, Clone, Debug, RawEncode, RawDecode, Eq, PartialEq, Hash)]
pub enum TunnelType {
    Datagram,
    Stream,
}

#[derive(Debug, Clone, RawEncode, RawDecode)]
pub struct SnCallResp {
    //sn call的响应包
    pub seq: Sequence,                                //序列事情
    pub sn_peer_id: P2pId,                            //sn设备id
    pub result: u8,                                   //
    pub to_peer_info: Option<EncodedP2pIdentityCert>, //
}

#[derive(Clone, Debug)]
pub struct SnCalled {
    pub seq: Sequence,
    pub sn_peer_id: P2pId,
    pub to_peer_id: P2pId,
    pub reverse_endpoint_array: Vec<Endpoint>,
    pub active_pn_list: Vec<P2pId>,
    pub peer_info: EncodedP2pIdentityCert,
    pub tunnel_id: TunnelId,
    pub call_send_time: Timestamp,
    pub call_type: TunnelType,
    pub payload: Vec<u8>,
    pub nat_context: Option<NatTraversalContext>,
}

#[derive(Clone, Debug, RawEncode, RawDecode)]
struct LegacySnCalled {
    seq: Sequence,
    sn_peer_id: P2pId,
    to_peer_id: P2pId,
    reverse_endpoint_array: Vec<Endpoint>,
    active_pn_list: Vec<P2pId>,
    peer_info: EncodedP2pIdentityCert,
    tunnel_id: TunnelId,
    call_send_time: Timestamp,
    call_type: TunnelType,
    payload: Vec<u8>,
}

impl From<&SnCalled> for LegacySnCalled {
    fn from(value: &SnCalled) -> Self {
        Self {
            seq: value.seq,
            sn_peer_id: value.sn_peer_id.clone(),
            to_peer_id: value.to_peer_id.clone(),
            reverse_endpoint_array: value.reverse_endpoint_array.clone(),
            active_pn_list: value.active_pn_list.clone(),
            peer_info: value.peer_info.clone(),
            tunnel_id: value.tunnel_id,
            call_send_time: value.call_send_time,
            call_type: value.call_type,
            payload: value.payload.clone(),
        }
    }
}

impl RawEncode for SnCalled {
    fn raw_measure(&self, purpose: &Option<RawEncodePurpose>) -> Result<usize, CodecError> {
        Ok(LegacySnCalled::from(self).raw_measure(purpose)?
            + extension_measure(self.nat_context.as_ref(), purpose)?)
    }

    fn raw_encode<'a>(
        &self,
        buf: &'a mut [u8],
        purpose: &Option<RawEncodePurpose>,
    ) -> Result<&'a mut [u8], CodecError> {
        let buf = LegacySnCalled::from(self).raw_encode(buf, purpose)?;
        extension_encode(
            self.nat_context.as_ref(),
            SN_CALLED_EXTENSION_MAGIC,
            buf,
            purpose,
        )
    }
}

impl<'de> RawDecode<'de> for SnCalled {
    fn raw_decode(buf: &'de [u8]) -> Result<(Self, &'de [u8]), CodecError> {
        let (legacy, buf) = LegacySnCalled::raw_decode(buf)?;
        let (nat_context, buf) = extension_decode(buf, SN_CALLED_EXTENSION_MAGIC);
        let nat_context = supported_nat_context(nat_context);
        Ok((
            Self {
                seq: legacy.seq,
                sn_peer_id: legacy.sn_peer_id,
                to_peer_id: legacy.to_peer_id,
                reverse_endpoint_array: legacy.reverse_endpoint_array,
                active_pn_list: legacy.active_pn_list,
                peer_info: legacy.peer_info,
                tunnel_id: legacy.tunnel_id,
                call_send_time: legacy.call_send_time,
                call_type: legacy.call_type,
                payload: legacy.payload,
                nat_context,
            },
            buf,
        ))
    }
}

#[derive(Debug, Clone, RawEncode, RawDecode)]
pub struct SnCalledResp {
    //sn called的应答报文
    pub seq: Sequence,     //序列号
    pub sn_peer_id: P2pId, //sn的设备id
    pub result: u8,        //
}

#[cfg(test)]
mod nat_type_wire_tests {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/unit/sn_tests/protocol/v0/nat_type_wire_tests.rs"
    ));
}
