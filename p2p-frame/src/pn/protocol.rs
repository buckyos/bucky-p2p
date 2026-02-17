use crate::p2p_identity::P2pId;
use crate::types::TunnelId;
use bucky_raw_codec::{RawDecode, RawEncode};

pub type PnCmdHeader = sfo_cmd_server::CmdHeader<u16, u8>;

pub const PN_DATA_FRAME_PROXY_OPEN_REQ: u8 = 1;
pub const PN_DATA_FRAME_PROXY_OPEN_READY: u8 = 2;
pub const PN_DATA_FRAME_PROXY_OPEN_RESP: u8 = 3;

#[derive(RawDecode, RawEncode)]
pub struct ProxyOpenReq {
    pub tunnel_id: TunnelId,
    pub to: P2pId,
}

#[derive(RawDecode, RawEncode)]
pub struct ProxyOpenReady {
    pub tunnel_id: TunnelId,
    pub from: P2pId,
    pub to: P2pId,
}

#[derive(RawDecode, RawEncode)]
pub struct ProxyOpenResp {
    pub tunnel_id: TunnelId,
    pub result: u8,
}

#[derive(RawDecode, RawEncode)]
pub struct ProxyOpenNotify {
    pub tunnel_id: TunnelId,
    pub from: P2pId,
}
