use super::sn::*;
use crate::endpoint::Endpoint;
use crate::p2p_identity::{EncodedP2pIdentityCert, P2pId};
use crate::types::{Sequence, Timestamp, TunnelId};
use bucky_raw_codec::{RawDecode, RawEncode};

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

#[derive(Clone, Debug, RawEncode, RawDecode)]
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
}

#[derive(Debug, Clone, RawEncode, RawDecode)]
pub struct SnCalledResp {
    //sn called的应答报文
    pub seq: Sequence,     //序列号
    pub sn_peer_id: P2pId, //sn的设备id
    pub result: u8,        //
}
