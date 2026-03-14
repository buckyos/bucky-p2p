use crate::networks::{TunnelCommandBody, TunnelPurpose};
use crate::p2p_identity::P2pId;
use crate::types::TunnelId;
use bucky_raw_codec::{RawDecode, RawEncode};

pub type PnCmdHeader = sfo_cmd_server::CmdHeader<u16, u8>;

pub const PN_PROXY_VPORT: u16 = 0xfff1;

pub const PN_COMMAND_PROXY_OPEN_REQ: u8 = 1;
pub const PN_COMMAND_PROXY_OPEN_RESP: u8 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq, RawDecode, RawEncode)]
pub enum PnChannelKind {
    Stream = 1,
    Datagram = 2,
}

#[derive(Clone, Debug, RawDecode, RawEncode)]
pub struct ProxyOpenReq {
    pub tunnel_id: TunnelId,
    pub from: P2pId,
    pub to: P2pId,
    pub kind: PnChannelKind,
    pub purpose: TunnelPurpose,
}

impl TunnelCommandBody for ProxyOpenReq {
    const COMMAND_ID: u8 = PN_COMMAND_PROXY_OPEN_REQ;
}

#[derive(Clone, Debug, RawDecode, RawEncode)]
pub struct ProxyOpenResp {
    pub tunnel_id: TunnelId,
    pub result: u8,
}

impl TunnelCommandBody for ProxyOpenResp {
    const COMMAND_ID: u8 = PN_COMMAND_PROXY_OPEN_RESP;
}
