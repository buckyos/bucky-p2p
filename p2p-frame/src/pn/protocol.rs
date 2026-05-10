use crate::networks::{TunnelCommandBody, TunnelPurpose};
use crate::p2p_identity::P2pId;
use crate::types::TunnelId;
use bucky_raw_codec::{RawDecode, RawEncode};

pub type PnCmdHeader = sfo_cmd_server::CmdHeader<u16, u8>;

pub const PROXY_SERVICE: &str = "proxy_service";

pub const PN_COMMAND_PROXY_OPEN_REQ: u8 = 1;
pub const PN_COMMAND_PROXY_OPEN_RESP: u8 = 2;
pub const PN_COMMAND_PROXY_CONTROL_OPEN_REQ: u8 = 3;
pub const PN_COMMAND_PROXY_CONTROL_OPEN_RESP: u8 = 4;
pub const PN_COMMAND_CONTROL_PING: u8 = 5;
pub const PN_COMMAND_CONTROL_PONG: u8 = 6;
pub const PN_COMMAND_CONTROL_CLOSE: u8 = 7;

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

#[derive(Clone, Debug, RawDecode, RawEncode)]
pub struct ProxyControlOpenReq {
    pub tunnel_id: TunnelId,
    pub from: P2pId,
    pub to: P2pId,
}

impl TunnelCommandBody for ProxyControlOpenReq {
    const COMMAND_ID: u8 = PN_COMMAND_PROXY_CONTROL_OPEN_REQ;
}

#[derive(Clone, Debug, RawDecode, RawEncode)]
pub struct ProxyControlOpenResp {
    pub tunnel_id: TunnelId,
    pub result: u8,
}

impl TunnelCommandBody for ProxyControlOpenResp {
    const COMMAND_ID: u8 = PN_COMMAND_PROXY_CONTROL_OPEN_RESP;
}

#[derive(Clone, Debug, RawDecode, RawEncode)]
pub struct PnControlPing {
    pub seq: u64,
    pub send_time: u64,
}

impl TunnelCommandBody for PnControlPing {
    const COMMAND_ID: u8 = PN_COMMAND_CONTROL_PING;
}

#[derive(Clone, Debug, RawDecode, RawEncode)]
pub struct PnControlPong {
    pub seq: u64,
    pub send_time: u64,
}

impl TunnelCommandBody for PnControlPong {
    const COMMAND_ID: u8 = PN_COMMAND_CONTROL_PONG;
}

#[derive(Clone, Debug, RawDecode, RawEncode)]
pub struct PnControlClose {
    pub reason: u8,
}

impl TunnelCommandBody for PnControlClose {
    const COMMAND_ID: u8 = PN_COMMAND_CONTROL_CLOSE;
}
