mod dep {
    pub use std::any::Any;
    pub use std::collections::BTreeMap;
    pub use std::convert::TryFrom;
    pub use std::fmt;
    pub use std::rc::Rc;
    pub use std::str::FromStr;
}

use bucky_raw_codec::{RawDecode, RawEncode, RawFixedBytes};
pub use dep::*;
use crate::error::{P2pError, P2pErrorCode, P2pResult};
use crate::types::{Timestamp};

pub const MTU: usize = 1472;
pub const MTU_LARGE: usize = 1024*30;


#[repr(u16)]
#[derive(Debug, Clone, Copy, Eq, PartialEq, PartialOrd)]
pub enum PackageCmdCode {
    SynSession = 1,
    AckSession = 2,
    SynReverseSession = 3,
    AckReverseSession = 4,

    SnCall = 0x20,
    SnCallResp = 0x21,
    SnCalled = 0x22,
    SnCalledResp = 0x23,
    ReportSn = 0x24,
    ReportSnResp = 0x25,
    SnQuery = 0x26,
    SnQueryResp = 0x27,

    FromProxy = 0x50,
    FromProxyResp = 0x51,
    ToProxy = 0x52,
    ToProxyResp = 0x53,
    ProxyHeart = 0x54,
    ProxyHeartResp = 0x55,
    ProxyClosed = 0x56,


    PieceData = 0x60,
    PieceControl = 0x61,
    ChannelEstimate = 0x62,

    SynClose = 0x71,
}

impl PackageCmdCode {
    pub fn is_sn(&self) -> bool {
        (*self >= Self::SnCall) && (*self <= Self::ReportSnResp)
    }

    pub fn is_proxy(&self) -> bool {
        (*self >= Self::FromProxy) && (*self <= Self::ToProxyResp)
    }
}

impl TryFrom<u8> for PackageCmdCode {
    type Error = P2pError;
    fn try_from(v: u8) -> std::result::Result<Self, Self::Error> {
        match v {
            1u8 => Ok(Self::SynSession),
            2u8 => Ok(Self::AckSession),
            3u8 => Ok(Self::SynReverseSession),
            4u8 => Ok(Self::AckReverseSession),
            0x20u8 => Ok(Self::SnCall),
            0x21u8 => Ok(Self::SnCallResp),
            0x22u8 => Ok(Self::SnCalled),
            0x23u8 => Ok(Self::SnCalledResp),
            0x24u8 => Ok(Self::ReportSn),
            0x25u8 => Ok(Self::ReportSnResp),
            0x26u8 => Ok(Self::SnQuery),
            0x27u8 => Ok(Self::SnQueryResp),

            0x50u8 => Ok(Self::FromProxy),
            0x51u8 => Ok(Self::FromProxyResp),
            0x52u8 => Ok(Self::ToProxy),
            0x53u8 => Ok(Self::ToProxyResp),
            0x54u8 => Ok(Self::ProxyHeart),
            0x55u8 => Ok(Self::ProxyHeartResp),
            0x56u8 => Ok(Self::ProxyClosed),

            0x60u8 => Ok(Self::PieceData),
            0x61u8 => Ok(Self::PieceControl),
            0x62u8 => Ok(Self::ChannelEstimate),
            0x71u8 => Ok(Self::SynClose),

            _ => Err(P2pError::new(
                P2pErrorCode::InvalidParam,
                format!("invalid package command type value {}", v),
            )),
        }
    }
}

#[derive(RawDecode, RawEncode)]
pub struct PackageHeader {
    pkg_len: u16,
    version: u8,
    cmd_code: u8
}

impl PackageHeader {
    pub fn new(version: u8, cmd_code: PackageCmdCode, pkg_len: u16) -> Self {
        Self {
            pkg_len,
            version,
            cmd_code: cmd_code as u8
        }
    }

    pub fn cmd_code(&self) -> P2pResult<PackageCmdCode> {
        PackageCmdCode::try_from(self.cmd_code)
    }

    pub fn pkg_len(&self) -> u16 {
        self.pkg_len
    }

    pub fn version(&self) -> u8 {
        self.version
    }

    pub fn set_pkg_len(&mut self, pkg_len: u16) {
        self.pkg_len = pkg_len;
    }
}

impl RawFixedBytes for PackageHeader {
    fn raw_bytes() -> Option<usize> {
        Some(u16::raw_bytes().unwrap() + u8::raw_bytes().unwrap() + u8::raw_bytes().unwrap())
    }
}

#[derive(RawDecode, RawEncode)]
pub struct Package<T> {
    header: PackageHeader,
    body: T,
}

impl <T: RawEncode> Package<T> {
    pub fn new(version: u8, cmd_code: PackageCmdCode, body: T) -> Self {
        Self {
            header: PackageHeader::new(version, cmd_code, body.raw_measure(&None).unwrap() as u16),
            body
        }
    }

    pub fn cmd_code(&self) -> P2pResult<PackageCmdCode> {
        self.header.cmd_code()
    }

    pub fn body(&self) -> &T {
        &self.body
    }
}

#[derive(Clone, RawEncode, RawDecode)]
pub enum SynType {
    Datagram,
    Stream,
}

#[derive(Clone, RawEncode, RawDecode)]
pub struct SynTunnel {
    pub protocol_version: u8,
    pub stack_version: u32,
}

pub const ACK_TUNNEL_RESULT_OK: u8 = 0;
pub const ACK_TUNNEL_RESULT_REFUSED: u8 = 1;

#[derive(Debug, Clone)]
pub struct AckTunnel {
    pub protocol_version: u8,
    pub stack_version: u32,
    pub result: u8,
    pub send_time: Timestamp,
}

// #[derive(Clone)]
// pub struct SynProxy {
//     pub protocol_version: u8,
//     pub stack_version: u32,
//     pub seq: TempSeq,
//     pub to_peer_id: DeviceId,
//     pub to_peer_timestamp: Timestamp,
//     pub from_peer_info: P2pIdentityCert,
//     pub mix_key: AesKey,
// }
//
// impl std::fmt::Display for SynProxy {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         write!(
//             f,
//             "SynProxy:{{sequence:{:?}, to:{:?}, from:{}, mix_key:{:?}}}",
//             self.seq,
//             self.to_peer_id,
//             self.from_peer_info,
//             self.mix_key.to_hex(),
//         )
//     }
// }
//
// impl std::fmt::Debug for SynProxy {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         write!(
//             f,
//             "SynProxy:{{sequence:{:?}, to:{:?}, from:{}, mix_key:{:?}}}",
//             self.seq,
//             self.to_peer_id,
//             self.from_peer_info.desc().device_id(),
//             self.mix_key.to_hex(),
//         )
//     }
// }
