use crate::error::{P2pErrorCode, P2pResult, into_p2p_err, p2p_err};
use crate::networks::{
    TunnelCommand, TunnelCommandBody, TunnelCommandHeader, TunnelPurpose, read_tunnel_command_body,
    read_tunnel_command_header,
};
use crate::runtime;
use crate::types::{Timestamp, TunnelCandidateId, TunnelId};
use bucky_raw_codec::{RawConvertTo, RawDecode, RawEncode, RawFrom};

pub type TcpConnId = TunnelId;
pub type TcpLeaseSeq = TunnelId;
pub type TcpChannelId = TunnelId;
pub type TcpRequestId = TunnelId;
pub type TcpTunnelCandidateId = TunnelCandidateId;

const MAX_TCP_TUNNEL_COMMAND_BODY_LEN: u32 = 1024 * 1024;

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TcpCommandId {
    ConnectionHello = 1,
    ControlConnReady = 2,
    DataConnReady = 3,
    Ping = 4,
    Pong = 5,
    OpenDataConnReq = 6,
    ClaimConnReq = 7,
    ClaimConnAck = 8,
    WriteFin = 9,
    ReadDone = 10,
}

impl TcpCommandId {
    fn from_u8(value: u8) -> Option<Self> {
        match value {
            x if x == Self::ConnectionHello as u8 => Some(Self::ConnectionHello),
            x if x == Self::ControlConnReady as u8 => Some(Self::ControlConnReady),
            x if x == Self::DataConnReady as u8 => Some(Self::DataConnReady),
            x if x == Self::Ping as u8 => Some(Self::Ping),
            x if x == Self::Pong as u8 => Some(Self::Pong),
            x if x == Self::OpenDataConnReq as u8 => Some(Self::OpenDataConnReq),
            x if x == Self::ClaimConnReq as u8 => Some(Self::ClaimConnReq),
            x if x == Self::ClaimConnAck as u8 => Some(Self::ClaimConnAck),
            x if x == Self::WriteFin as u8 => Some(Self::WriteFin),
            x if x == Self::ReadDone as u8 => Some(Self::ReadDone),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, RawEncode, RawDecode)]
pub enum TcpConnectionRole {
    Control,
    Data,
}

#[derive(Clone, Debug, RawEncode, RawDecode)]
pub struct TcpConnectionHello {
    pub role: TcpConnectionRole,
    pub tunnel_id: TunnelId,
    pub candidate_id: TcpTunnelCandidateId,
    pub is_reverse: bool,
    pub conn_id: Option<TcpConnId>,
    pub open_request_id: Option<TcpRequestId>,
}

impl TunnelCommandBody for TcpConnectionHello {
    const COMMAND_ID: u8 = TcpCommandId::ConnectionHello as u8;
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, RawEncode, RawDecode)]
pub enum ControlConnReadyResult {
    Success = 0,
    TunnelConflictLost = 1,
    ProtocolError = 2,
    InternalError = 3,
}

#[derive(Clone, Debug, RawEncode, RawDecode)]
pub struct ControlConnReady {
    pub tunnel_id: TunnelId,
    pub candidate_id: TcpTunnelCandidateId,
    pub result: ControlConnReadyResult,
}

impl TunnelCommandBody for ControlConnReady {
    const COMMAND_ID: u8 = TcpCommandId::ControlConnReady as u8;
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, RawEncode, RawDecode)]
pub enum DataConnReadyResult {
    Success = 0,
    TunnelNotFound = 1,
    ConnIdConflict = 2,
    ProtocolError = 3,
    InternalError = 4,
}

#[derive(Clone, Debug, RawEncode, RawDecode)]
pub struct DataConnReady {
    pub conn_id: TcpConnId,
    pub candidate_id: TcpTunnelCandidateId,
    pub result: DataConnReadyResult,
}

impl TunnelCommandBody for DataConnReady {
    const COMMAND_ID: u8 = TcpCommandId::DataConnReady as u8;
}

#[derive(Clone, Debug, RawEncode, RawDecode)]
pub struct PingCmd {
    pub seq: u64,
    pub send_time: Timestamp,
}

impl TunnelCommandBody for PingCmd {
    const COMMAND_ID: u8 = TcpCommandId::Ping as u8;
}

#[derive(Clone, Debug, RawEncode, RawDecode)]
pub struct PongCmd {
    pub seq: u64,
    pub send_time: Timestamp,
}

impl TunnelCommandBody for PongCmd {
    const COMMAND_ID: u8 = TcpCommandId::Pong as u8;
}

#[derive(Clone, Debug, RawEncode, RawDecode)]
pub struct OpenDataConnReq {
    pub request_id: TcpRequestId,
}

impl TunnelCommandBody for OpenDataConnReq {
    const COMMAND_ID: u8 = TcpCommandId::OpenDataConnReq as u8;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, RawEncode, RawDecode)]
pub enum TcpChannelKind {
    Stream,
    Datagram,
}

#[derive(Clone, Debug, RawEncode, RawDecode)]
pub struct ClaimConnReq {
    pub channel_id: TcpChannelId,
    pub kind: TcpChannelKind,
    pub purpose: TunnelPurpose,
    pub conn_id: TcpConnId,
    pub lease_seq: TcpLeaseSeq,
    pub claim_nonce: u64,
}

impl TunnelCommandBody for ClaimConnReq {
    const COMMAND_ID: u8 = TcpCommandId::ClaimConnReq as u8;
}

#[derive(Clone, Debug, RawEncode, RawDecode)]
pub struct ClaimConnAck {
    pub channel_id: TcpChannelId,
    pub conn_id: TcpConnId,
    pub lease_seq: TcpLeaseSeq,
    pub result: u8,
}

impl TunnelCommandBody for ClaimConnAck {
    const COMMAND_ID: u8 = TcpCommandId::ClaimConnAck as u8;
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, RawEncode, RawDecode)]
pub enum ClaimConnAckResult {
    Success = 0,
    VportNotFound = 1,
    ListenerClosed = 2,
    NotIdle = 3,
    LeaseMismatch = 4,
    ConflictLost = 5,
    Retired = 6,
    ProtocolError = 7,
    AcceptQueueFull = 8,
}

impl TryFrom<u8> for ClaimConnAckResult {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            x if x == Self::Success as u8 => Ok(Self::Success),
            x if x == Self::VportNotFound as u8 => Ok(Self::VportNotFound),
            x if x == Self::ListenerClosed as u8 => Ok(Self::ListenerClosed),
            x if x == Self::NotIdle as u8 => Ok(Self::NotIdle),
            x if x == Self::LeaseMismatch as u8 => Ok(Self::LeaseMismatch),
            x if x == Self::ConflictLost as u8 => Ok(Self::ConflictLost),
            x if x == Self::Retired as u8 => Ok(Self::Retired),
            x if x == Self::ProtocolError as u8 => Ok(Self::ProtocolError),
            x if x == Self::AcceptQueueFull as u8 => Ok(Self::AcceptQueueFull),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Debug, RawEncode, RawDecode)]
pub struct WriteFin {
    pub channel_id: TcpChannelId,
    pub conn_id: TcpConnId,
    pub lease_seq: TcpLeaseSeq,
    pub final_tx_bytes: u64,
}

impl TunnelCommandBody for WriteFin {
    const COMMAND_ID: u8 = TcpCommandId::WriteFin as u8;
}

#[derive(Clone, Debug, RawEncode, RawDecode)]
pub struct ReadDone {
    pub channel_id: TcpChannelId,
    pub conn_id: TcpConnId,
    pub lease_seq: TcpLeaseSeq,
    pub final_rx_bytes: u64,
}

impl TunnelCommandBody for ReadDone {
    const COMMAND_ID: u8 = TcpCommandId::ReadDone as u8;
}

#[derive(Clone, Debug)]
pub enum TcpControlCmd {
    Ping(PingCmd),
    Pong(PongCmd),
    OpenDataConnReq(OpenDataConnReq),
    ClaimConnReq(ClaimConnReq),
    ClaimConnAck(ClaimConnAck),
    WriteFin(WriteFin),
    ReadDone(ReadDone),
}

pub trait TcpTunnelWireEncode {
    fn encode_wire(&self) -> P2pResult<Vec<u8>>;
}

#[async_trait::async_trait]
pub trait TcpTunnelWireDecode: Sized {
    async fn read_from_wire<R>(read: &mut R) -> P2pResult<Self>
    where
        R: runtime::AsyncRead + Unpin + Send;
}

fn validate_header_len(header: &TunnelCommandHeader) -> P2pResult<()> {
    if header.data_len == 0 || header.data_len > MAX_TCP_TUNNEL_COMMAND_BODY_LEN {
        return Err(p2p_err!(
            P2pErrorCode::OutOfLimit,
            "invalid tcp tunnel command body len {}",
            header.data_len
        ));
    }
    Ok(())
}

fn encode_tunnel_command_bytes<T>(body: &T) -> P2pResult<Vec<u8>>
where
    T: TunnelCommandBody + Clone,
{
    let command = TunnelCommand::new(body.clone())?;
    let mut bytes = command
        .header
        .to_vec()
        .map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
    bytes.extend_from_slice(
        command
            .body
            .to_vec()
            .map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?
            .as_slice(),
    );
    Ok(bytes)
}

async fn decode_tunnel_command_body<R, T>(read: &mut R, header: TunnelCommandHeader) -> P2pResult<T>
where
    R: runtime::AsyncRead + Unpin + Send,
    T: TunnelCommandBody,
{
    validate_header_len(&header)?;
    Ok(read_tunnel_command_body::<_, T>(read, header).await?.body)
}

macro_rules! impl_tcp_wire_for_body {
    ($ty:ty) => {
        impl TcpTunnelWireEncode for $ty {
            fn encode_wire(&self) -> P2pResult<Vec<u8>> {
                encode_tunnel_command_bytes(self)
            }
        }

        #[async_trait::async_trait]
        impl TcpTunnelWireDecode for $ty {
            async fn read_from_wire<R>(read: &mut R) -> P2pResult<Self>
            where
                R: runtime::AsyncRead + Unpin + Send,
            {
                let header = read_tunnel_command_header(read).await?;
                decode_tunnel_command_body::<_, Self>(read, header).await
            }
        }
    };
}

impl_tcp_wire_for_body!(TcpConnectionHello);
impl_tcp_wire_for_body!(ControlConnReady);
impl_tcp_wire_for_body!(DataConnReady);

impl TcpTunnelWireEncode for TcpControlCmd {
    fn encode_wire(&self) -> P2pResult<Vec<u8>> {
        match self {
            Self::Ping(cmd) => encode_tunnel_command_bytes(cmd),
            Self::Pong(cmd) => encode_tunnel_command_bytes(cmd),
            Self::OpenDataConnReq(cmd) => encode_tunnel_command_bytes(cmd),
            Self::ClaimConnReq(cmd) => encode_tunnel_command_bytes(cmd),
            Self::ClaimConnAck(cmd) => encode_tunnel_command_bytes(cmd),
            Self::WriteFin(cmd) => encode_tunnel_command_bytes(cmd),
            Self::ReadDone(cmd) => encode_tunnel_command_bytes(cmd),
        }
    }
}

#[async_trait::async_trait]
impl TcpTunnelWireDecode for TcpControlCmd {
    async fn read_from_wire<R>(read: &mut R) -> P2pResult<Self>
    where
        R: runtime::AsyncRead + Unpin + Send,
    {
        let header = read_tunnel_command_header(read).await?;
        validate_header_len(&header)?;
        let command_id = TcpCommandId::from_u8(header.command_id).ok_or_else(|| {
            p2p_err!(
                P2pErrorCode::InvalidData,
                "unknown tcp tunnel command id {}",
                header.command_id
            )
        })?;
        match command_id {
            TcpCommandId::ConnectionHello
            | TcpCommandId::ControlConnReady
            | TcpCommandId::DataConnReady => Err(p2p_err!(
                P2pErrorCode::InvalidData,
                "unexpected tcp handshake command id {} on control command path",
                header.command_id
            )),
            TcpCommandId::Ping => Ok(Self::Ping(
                decode_tunnel_command_body::<_, PingCmd>(read, header).await?,
            )),
            TcpCommandId::Pong => Ok(Self::Pong(
                decode_tunnel_command_body::<_, PongCmd>(read, header).await?,
            )),
            TcpCommandId::OpenDataConnReq => Ok(Self::OpenDataConnReq(
                decode_tunnel_command_body::<_, OpenDataConnReq>(read, header).await?,
            )),
            TcpCommandId::ClaimConnReq => Ok(Self::ClaimConnReq(
                decode_tunnel_command_body::<_, ClaimConnReq>(read, header).await?,
            )),
            TcpCommandId::ClaimConnAck => Ok(Self::ClaimConnAck(
                decode_tunnel_command_body::<_, ClaimConnAck>(read, header).await?,
            )),
            TcpCommandId::WriteFin => Ok(Self::WriteFin(
                decode_tunnel_command_body::<_, WriteFin>(read, header).await?,
            )),
            TcpCommandId::ReadDone => Ok(Self::ReadDone(
                decode_tunnel_command_body::<_, ReadDone>(read, header).await?,
            )),
        }
    }
}

pub async fn write_raw_frame<W: runtime::AsyncWrite + Unpin>(
    write: &mut W,
    value: &impl TcpTunnelWireEncode,
) -> P2pResult<()> {
    let bytes = value.encode_wire()?;
    runtime::AsyncWriteExt::write_all(write, bytes.as_slice())
        .await
        .map_err(into_p2p_err!(P2pErrorCode::IoError))?;
    runtime::AsyncWriteExt::flush(write)
        .await
        .map_err(into_p2p_err!(P2pErrorCode::IoError))?;
    Ok(())
}

pub async fn read_raw_frame<R, T>(read: &mut R) -> P2pResult<T>
where
    R: runtime::AsyncRead + Unpin + Send,
    T: TcpTunnelWireDecode,
{
    T::read_from_wire(read).await
}
