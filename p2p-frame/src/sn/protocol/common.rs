mod dep {
    pub use std::any::Any;
    pub use std::collections::BTreeMap;
    pub use std::convert::TryFrom;
    pub use std::fmt;
    pub use std::rc::Rc;
    pub use std::str::FromStr;
}

use crate::error::{P2pError, P2pErrorCode, P2pResult};
use bucky_raw_codec::{RawDecode, RawEncode, RawFixedBytes};
pub use dep::*;

pub const MTU: usize = 1472;
pub const MTU_LARGE: usize = 0xFFFF;

#[repr(u16)]
#[derive(Debug, Clone, Copy, Eq, PartialEq, PartialOrd)]
pub enum PackageCmdCode {
    SnCall = 0x20,
    SnCallResp = 0x21,
    SnCalled = 0x22,
    SnCalledResp = 0x23,
    ReportSn = 0x24,
    ReportSnResp = 0x25,
    SnQuery = 0x26,
    SnQueryResp = 0x27,
}

impl PackageCmdCode {
    pub fn is_sn(&self) -> bool {
        (*self >= Self::SnCall) && (*self <= Self::SnQueryResp)
    }
}

impl TryFrom<u8> for PackageCmdCode {
    type Error = P2pError;
    fn try_from(v: u8) -> std::result::Result<Self, Self::Error> {
        match v {
            0x20u8 => Ok(Self::SnCall),
            0x21u8 => Ok(Self::SnCallResp),
            0x22u8 => Ok(Self::SnCalled),
            0x23u8 => Ok(Self::SnCalledResp),
            0x24u8 => Ok(Self::ReportSn),
            0x25u8 => Ok(Self::ReportSnResp),
            0x26u8 => Ok(Self::SnQuery),
            0x27u8 => Ok(Self::SnQueryResp),

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
    cmd_code: u8,
}

impl PackageHeader {
    pub fn new(version: u8, cmd_code: PackageCmdCode, pkg_len: u16) -> Self {
        Self {
            pkg_len,
            version,
            cmd_code: cmd_code as u8,
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

impl<T: RawEncode> Package<T> {
    pub fn new(version: u8, cmd_code: PackageCmdCode, body: T) -> Self {
        Self {
            header: PackageHeader::new(version, cmd_code, body.raw_measure(&None).unwrap() as u16),
            body,
        }
    }

    pub fn cmd_code(&self) -> P2pResult<PackageCmdCode> {
        self.header.cmd_code()
    }

    pub fn body(&self) -> &T {
        &self.body
    }
}
