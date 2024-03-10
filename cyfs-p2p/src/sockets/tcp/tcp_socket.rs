use std::sync::Arc;
use async_std::net::{TcpListener, TcpStream};
use cyfs_base::{BuckyError, BuckyErrorCode, BuckyResult, DeviceId, Endpoint};
use crate::types::MixAesKey;

pub struct TCPSocket {
    socket: TcpStream,
    remote_device_id: DeviceId,
    local: Endpoint,
    remote: Endpoint,
    key: MixAesKey,
}

impl TCPSocket {
    pub fn new(socket: TcpStream, remote_device_id: DeviceId, local: Endpoint, remote: Endpoint, key: MixAesKey) -> Self {
        Self {
            socket,
            remote_device_id,
            local,
            remote,
            key,
        }
    }


    pub async fn connect(&mut self,) -> BuckyResult<()> {
        todo!()
    }

    pub fn socket(&self) -> &TcpStream {
        &self.socket
    }

    pub fn key(&self) -> &MixAesKey {
        &self.key
    }

    pub fn remote_device_id(&self) -> &DeviceId {
        &self.remote_device_id
    }

    pub fn remote(&self) -> &Endpoint {
        &self.remote
    }

    pub fn local(&self) -> &Endpoint {
        &self.local
    }
}
