use std::any::Any;
use std::sync::Arc;
use quinn::crypto::{ExportKeyingMaterialError, HeaderKey, KeyPair, Keys, PacketKey};
use quinn_proto::{ConnectError, ConnectionId, TransportError};
use quinn_proto::crypto::{Session, UnsupportedVersion};
use quinn_proto::transport_parameters::TransportParameters;
use rustls::quic::Version;

pub struct BuckySession {

}

impl quinn::crypto::Session for BuckySession {
    fn initial_keys(&self, dst_cid: &ConnectionId, side: quinn_proto::Side) -> Keys {
        todo!()
    }

    fn handshake_data(&self) -> Option<Box<dyn Any>> {
        todo!()
    }

    fn peer_identity(&self) -> Option<Box<dyn Any>> {
        todo!()
    }

    fn early_crypto(&self) -> Option<(Box<dyn HeaderKey>, Box<dyn PacketKey>)> {
        todo!()
    }

    fn early_data_accepted(&self) -> Option<bool> {
        todo!()
    }

    fn is_handshaking(&self) -> bool {
        todo!()
    }

    fn read_handshake(&mut self, buf: &[u8]) -> Result<bool, TransportError> {
        todo!()
    }

    fn transport_parameters(&self) -> Result<Option<TransportParameters>, TransportError> {
        todo!()
    }

    fn write_handshake(&mut self, buf: &mut Vec<u8>) -> Option<Keys> {
        todo!()
    }

    fn next_1rtt_keys(&mut self) -> Option<KeyPair<Box<dyn PacketKey>>> {
        todo!()
    }

    fn is_valid_retry(&self, orig_dst_cid: &ConnectionId, header: &[u8], payload: &[u8]) -> bool {
        todo!()
    }

    fn export_keying_material(&self, output: &mut [u8], label: &[u8], context: &[u8]) -> Result<(), ExportKeyingMaterialError> {
        todo!()
    }
}
fn interpret_version(version: u32) -> Result<Version, UnsupportedVersion> {
    match version {
        0xff00_001d..=0xff00_0020 => Ok(Version::V1Draft),
        0x0000_0001 | 0xff00_0021..=0xff00_0022 => Ok(Version::V1),
        _ => Err(UnsupportedVersion),
    }
}

pub struct BuckyServerConfig {

}

impl quinn::crypto::ServerConfig for BuckyServerConfig {
    fn initial_keys(&self, version: u32, dst_cid: &ConnectionId) -> Result<Keys, UnsupportedVersion> {
        todo!()
    }

    fn retry_tag(&self, version: u32, orig_dst_cid: &ConnectionId, packet: &[u8]) -> [u8; 16] {
        todo!()
    }

    fn start_session(self: Arc<Self>, version: u32, params: &TransportParameters) -> Box<dyn Session> {
        todo!()
    }
}

pub struct BuckyClientConfig {

}

impl quinn::crypto::ClientConfig for BuckyClientConfig {
    fn start_session(self: Arc<Self>, version: u32, server_name: &str, params: &TransportParameters) -> Result<Box<dyn Session>, ConnectError> {
        let version = interpret_version(version)?;
        todo!()
    }
}
