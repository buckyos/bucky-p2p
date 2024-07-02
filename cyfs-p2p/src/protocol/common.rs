mod dep {
    pub use std::any::Any;
    pub use std::collections::BTreeMap;
    pub use std::convert::TryFrom;
    pub use std::fmt;
    pub use std::rc::Rc;
    pub use std::str::FromStr;

    pub use crate::types::*;
}

use std::time::{Duration, SystemTime, UNIX_EPOCH};
use bucky_crypto::{AesKey, HashValue, PrivateKey};
use bucky_error::{BuckyError, BuckyErrorCode};
use bucky_objects::{Area, Device, DeviceCategory, DeviceId, NamedObject, RsaCPUObjectVerifier, Signature, SignatureSource, Signer, SingleKeyObjectDesc, UniqueId, Verifier};
use bucky_raw_codec::{RawConvertTo, RawDecode, RawDecodeWithContext, RawEncode, RawEncodePurpose, RawEncodeWithContext, RawFixedBytes};
use bucky_time::bucky_time_now;
pub(super) use dep::*;
use crate::error::{BdtError, BdtErrorCode, BdtResult, into_bdt_err};
use crate::tunnel::TunnelType;

pub const MTU: usize = 1472;
pub const MTU_LARGE: usize = 1024*30;


#[repr(u16)]
#[derive(Debug, Clone, Copy, Eq, PartialEq, PartialOrd)]
pub enum PackageCmdCode {
    SynStream = 1,
    AckStream = 2,

    SynDatagram = 0x11,
    AckDatagram = 0x12,

    SnCall = 0x20,
    SnCallResp = 0x21,
    SnCalled = 0x22,
    SnCalledResp = 0x23,
    SnPing = 0x24,
    SnPingResp = 0x25,

    SessionData = 0x40,
    TcpAckAckConnection = 0x43,

    SynProxy = 0x50,
    AckProxy = 0x51,

    PieceData = 0x60,
    PieceControl = 0x61,
    ChannelEstimate = 0x62,

    SynClose = 0x71,
    AckClose = 0x72,
}

impl PackageCmdCode {
    pub fn is_sn(&self) -> bool {
        (*self >= Self::SnCall) && (*self <= Self::SnPingResp)
    }

    pub fn is_proxy(&self) -> bool {
        (*self >= Self::SynProxy) && (*self <= Self::AckProxy)
    }
}

impl TryFrom<u8> for PackageCmdCode {
    type Error = BdtError;
    fn try_from(v: u8) -> std::result::Result<Self, Self::Error> {
        match v {
            1u8 => Ok(Self::SynStream),
            2u8 => Ok(Self::AckStream),
            0x11u8 => Ok(Self::SynDatagram),
            0x12u8 => Ok(Self::AckDatagram),
            0x20u8 => Ok(Self::SnCall),
            0x21u8 => Ok(Self::SnCallResp),
            0x22u8 => Ok(Self::SnCalled),
            0x23u8 => Ok(Self::SnCalledResp),
            0x24u8 => Ok(Self::SnPing),
            0x25u8 => Ok(Self::SnPingResp),

            0x50u8 => Ok(Self::SynProxy),
            0x51u8 => Ok(Self::AckProxy),

            0x60u8 => Ok(Self::PieceData),
            0x61u8 => Ok(Self::PieceControl),
            0x62u8 => Ok(Self::ChannelEstimate),
            0x71u8 => Ok(Self::SynClose),
            0x72u8 => Ok(Self::AckClose),

            _ => Err(BdtError::new(
                BdtErrorCode::InvalidParam,
                format!("invalid package command type value {}", v),
            )),
        }
    }
}

#[derive(RawDecode, RawEncode)]
pub struct PackageHeader {
    pkg_len: u16,
    cmd_code: u8
}

impl PackageHeader {
    pub fn new(cmd_code: PackageCmdCode, pkg_len: u16) -> Self {
        Self {
            pkg_len,
            cmd_code: cmd_code as u8
        }
    }

    pub fn cmd_code(&self) -> BdtResult<PackageCmdCode> {
        PackageCmdCode::try_from(self.cmd_code)
    }

    pub fn pkg_len(&self) -> u16 {
        self.pkg_len
    }

    pub fn set_pkg_len(&mut self, pkg_len: u16) {
        self.pkg_len = pkg_len;
    }
}

impl RawFixedBytes for PackageHeader {
    fn raw_bytes() -> Option<usize> {
        Some(u16::raw_bytes().unwrap() + u8::raw_bytes().unwrap())
    }
}

#[derive(RawDecode, RawEncode)]
pub struct Package<T> {
    header: PackageHeader,
    body: T,
}

impl <T: RawEncode> Package<T> {
    pub fn new(cmd_code: PackageCmdCode, body: T) -> Self {
        Self {
            header: PackageHeader::new(cmd_code, body.raw_measure(&None).unwrap() as u16),
            body
        }
    }

    pub fn cmd_code(&self) -> BdtResult<PackageCmdCode> {
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

#[derive(Clone)]
pub struct SynProxy {
    pub protocol_version: u8,
    pub stack_version: u32,
    pub seq: TempSeq,
    pub to_peer_id: DeviceId,
    pub to_peer_timestamp: Timestamp,
    pub from_peer_info: Device,
    pub mix_key: AesKey,
}

impl std::fmt::Display for SynProxy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "SynProxy:{{sequence:{:?}, to:{:?}, from:{}, mix_key:{:?}}}",
            self.seq,
            self.to_peer_id,
            self.from_peer_info.desc().device_id(),
            self.mix_key.to_hex(),
        )
    }
}

impl std::fmt::Debug for SynProxy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "SynProxy:{{sequence:{:?}, to:{:?}, from:{}, mix_key:{:?}}}",
            self.seq,
            self.to_peer_id,
            self.from_peer_info.desc().device_id(),
            self.mix_key.to_hex(),
        )
    }
}
