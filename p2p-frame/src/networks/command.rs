use crate::error::{P2pErrorCode, P2pResult, into_p2p_err, p2p_err};
use crate::runtime;
use bucky_raw_codec::{RawConvertTo, RawDecode, RawEncode, RawFrom};

pub const TUNNEL_COMMAND_VERSION: u8 = 1;

#[derive(Clone, Debug, RawEncode, RawDecode)]
pub struct TunnelCommandHeader {
    pub version: u8,
    pub command_id: u8,
    pub data_len: u32,
}

pub struct TunnelCommand<T>
where
    T: TunnelCommandBody,
{
    pub header: TunnelCommandHeader,
    pub body: T,
}

pub trait TunnelCommandBody: RawEncode + for<'de> RawDecode<'de> + Sized {
    const COMMAND_ID: u8;
    const VERSION: u8 = TUNNEL_COMMAND_VERSION;
}

impl<T> TunnelCommand<T>
where
    T: TunnelCommandBody,
{
    pub fn new(body: T) -> P2pResult<Self> {
        let data_len = body
            .raw_measure(&None)
            .map_err(into_p2p_err!(P2pErrorCode::RawCodecError))? as u32;
        Ok(Self {
            header: TunnelCommandHeader {
                version: T::VERSION,
                command_id: T::COMMAND_ID,
                data_len,
            },
            body,
        })
    }

    pub fn validate(&self) -> P2pResult<()> {
        if self.header.version != T::VERSION {
            return Err(p2p_err!(
                P2pErrorCode::InvalidData,
                "invalid tunnel command version {}, expect {}",
                self.header.version,
                T::VERSION
            ));
        }
        if self.header.command_id != T::COMMAND_ID {
            return Err(p2p_err!(
                P2pErrorCode::InvalidData,
                "invalid tunnel command id {:?}, expect {:?}",
                self.header.command_id,
                T::COMMAND_ID
            ));
        }
        let data_len = self
            .body
            .raw_measure(&None)
            .map_err(into_p2p_err!(P2pErrorCode::RawCodecError))? as u32;
        if self.header.data_len != data_len {
            return Err(p2p_err!(
                P2pErrorCode::InvalidData,
                "invalid tunnel command data len {}, expect {}",
                self.header.data_len,
                data_len
            ));
        }
        Ok(())
    }

    pub fn from_header_and_body(header: TunnelCommandHeader, body: T) -> P2pResult<Self> {
        let command = Self { header, body };
        command.validate()?;
        Ok(command)
    }
}

fn header_len() -> P2pResult<usize> {
    TunnelCommandHeader {
        version: 0,
        command_id: 0,
        data_len: 0,
    }
    .raw_measure(&None)
    .map_err(into_p2p_err!(P2pErrorCode::RawCodecError))
}

pub async fn write_tunnel_command<W, T>(write: &mut W, value: &TunnelCommand<T>) -> P2pResult<()>
where
    W: runtime::AsyncWrite + Unpin,
    T: TunnelCommandBody,
{
    value.validate()?;
    let header = value
        .header
        .to_vec()
        .map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
    let body = value
        .body
        .to_vec()
        .map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
    runtime::AsyncWriteExt::write_all(write, header.as_slice())
        .await
        .map_err(into_p2p_err!(P2pErrorCode::IoError))?;
    runtime::AsyncWriteExt::write_all(write, body.as_slice())
        .await
        .map_err(into_p2p_err!(P2pErrorCode::IoError))?;
    runtime::AsyncWriteExt::flush(write)
        .await
        .map_err(into_p2p_err!(P2pErrorCode::IoError))?;
    Ok(())
}

pub async fn read_tunnel_command_header<R>(read: &mut R) -> P2pResult<TunnelCommandHeader>
where
    R: runtime::AsyncRead + Unpin,
{
    let mut buf = vec![0u8; header_len()?];
    runtime::AsyncReadExt::read_exact(read, buf.as_mut_slice())
        .await
        .map_err(into_p2p_err!(P2pErrorCode::IoError))?;
    TunnelCommandHeader::clone_from_slice(buf.as_slice())
        .map_err(into_p2p_err!(P2pErrorCode::RawCodecError))
}

pub async fn read_tunnel_command_body<R, T>(
    read: &mut R,
    header: TunnelCommandHeader,
) -> P2pResult<TunnelCommand<T>>
where
    R: runtime::AsyncRead + Unpin,
    T: TunnelCommandBody,
{
    if header.command_id != T::COMMAND_ID {
        return Err(p2p_err!(
            P2pErrorCode::InvalidData,
            "unexpected tunnel command id {:?}, expect {:?}",
            header.command_id,
            T::COMMAND_ID
        ));
    }
    let mut body = vec![0u8; header.data_len as usize];
    runtime::AsyncReadExt::read_exact(read, body.as_mut_slice())
        .await
        .map_err(into_p2p_err!(P2pErrorCode::IoError))?;
    let body =
        T::clone_from_slice(body.as_slice()).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
    TunnelCommand::from_header_and_body(header, body)
}
