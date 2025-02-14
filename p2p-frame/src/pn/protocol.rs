use bucky_raw_codec::{RawDecode, RawEncode};
use crate::p2p_identity::P2pId;
use crate::types::{Sequence, TunnelId};

pub type PnCmdHeader = sfo_cmd_server::CmdHeader<u16, u8>;

#[derive(RawDecode, RawEncode)]
pub struct ToProxy {
    pub seq: u32,
    pub tunnel_id: TunnelId,
    pub to: P2pId,
}

#[derive(RawDecode, RawEncode)]
pub struct ToProxyResp {
    pub to: P2pId,
    pub tunnel_id: TunnelId,
    pub result: u8,
}

#[derive(RawDecode, RawEncode)]
pub struct FromProxy {
    pub seq: u32,
    pub tunnel_id: TunnelId,
    pub from: P2pId,
}

#[derive(RawDecode, RawEncode)]
pub struct FromProxyResp {
    pub from: P2pId,
    pub tunnel_id: TunnelId,
    pub result: u8,
}

#[derive(RawDecode, RawEncode)]
pub struct ProxyHeart {
    pub tunnel_id: TunnelId,
    pub from: P2pId,
    pub to: P2pId,
}

#[derive(RawDecode, RawEncode)]
pub struct ProxyHeartResp {
    pub tunnel_id: TunnelId,
    pub from: P2pId,
    pub to: P2pId,
}
