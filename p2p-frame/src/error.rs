use bucky_raw_codec::{RawDecode, RawEncode};
use num_derive::{FromPrimitive, ToPrimitive};
use sfo_result::Result;
pub use sfo_result::err as bdt_err;
pub use sfo_result::into_err as into_bdt_err;

#[repr(u16)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default, FromPrimitive, ToPrimitive, RawEncode, RawDecode)]
pub enum BdtErrorCode {
    Ok = 0,

    #[default]
    Failed,
    InvalidParam,
    Timeout,
    NotFound,
    AlreadyExists,
    NotSupport,
    ErrorState,
    InvalidFormat,
    Expired,
    OutOfLimit,
    InternalError,

    PermissionDenied,
    ConnectionRefused,
    ConnectionReset,
    ConnectionAborted,
    NotConnected,
    AddrInUse,
    AddrNotAvailable,
    Interrupted,
    InvalidInput,
    InvalidData,
    WriteZero,
    UnexpectedEof,
    BrokenPipe,
    WouldBlock,

    UnSupport,
    Unmatch,
    ExecuteError,
    Reject,
    Ignored,
    InvalidSignature,
    AlreadyExistsAndSignatureMerged,
    TargetNotFound,
    Aborted,

    ConnectFailed,
    ConnectInterZoneFailed,
    InnerPathNotFound,
    RangeNotSatisfiable,
    UserCanceled,

    Conflict,

    OutofSessionLimit,

    Redirect,

    MongoDBError,
    SqliteError,
    UrlError,
    ZipError,
    HttpError,
    JsonError,
    HexError,
    RsaError,
    CryptoError,
    MpscSendError,
    MpscRecvError,
    IoError,
    NetworkError,

    CodeError, //TODO: cyfs-base的Code应该和BuckyErrorCode整合，现在先搞个特殊Type让能编过
    UnknownBdtError,
    UnknownIOError,
    Unknown,

    Pending,
    NotChange,

    NotMatch,
    NotImplement,
    NotInit,

    ParseError,
    NotHandled,
    RawCodecError,
    SignError,
    StreamPortAlreadyListen,
    CertError,
    TlsError,
    QuicError,
    TunnelNotConnected,
}

impl BdtErrorCode {
    pub fn as_u8(&self) -> u8 {
        let v: u16 = self.as_u16();
        v as u8
    }

    pub fn into_u8(self) -> u8 {
        let v: u16 = self.into_u16();
        v as u8
    }

    pub fn as_u16(&self) -> u16 {
        self.clone() as u16
    }

    pub fn into_u16(self) -> u16 {
        self as u16
    }
}

pub type BdtResult<T> = Result<T, BdtErrorCode>;
pub type BdtError = sfo_result::Error<BdtErrorCode>;

