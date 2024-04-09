use std::cell::RefCell;
use std::ops::Deref;
use std::sync::Arc;
use std::time::Duration;
use async_std::net::{TcpListener, TcpStream};
use cyfs_base::{BuckyError, BuckyErrorCode, BuckyResult, DeviceDesc, DeviceId, Endpoint};
use futures::AsyncWriteExt;
use crate::types::MixAesKey;

#[derive(Clone)]
pub struct TCPSocket {
    socket: TcpStream,
    remote_device_id: DeviceId,
    local_device_id: DeviceId,
    local: Endpoint,
    remote: Endpoint,
    key: MixAesKey,
}

impl std::fmt::Display for TCPSocket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "TcpInterface{{local:{},remote:{}}}",
            self.local, self.remote
        )
    }
}

impl TCPSocket {
    pub fn new(socket: TcpStream, local_device_id: DeviceId, remote_device_id: DeviceId, local: Endpoint, remote: Endpoint, key: MixAesKey) -> Self {
        Self {
            socket,
            remote_device_id,
            local_device_id,
            local,
            remote,
            key,
        }
    }


    pub async fn connect(
        local_device_id: DeviceId,
        remote_ep: Endpoint,
        remote_device_id: DeviceId,
        remote_device_desc: DeviceDesc,
        key: MixAesKey,
        timeout: Duration,) -> BuckyResult<TCPSocket> {
        let socket = async_std::io::timeout(timeout, TcpStream::connect(remote_ep.addr()))
            .await
            .map_err(|err| {
                debug!(
                    "tcp socket to {} connect failed for {}",
                    /*from_ip, */ remote_ep, err
                );
                err
            })?;
        let local = socket.local_addr().map_err(|e| BuckyError::from(e))?;
        let local = Endpoint::from((cyfs_base::endpoint::Protocol::Tcp, local));
        let socket = Self {
            socket,
            local,
            remote: remote_ep,
            remote_device_id,
            key,
            local_device_id,
        };
        debug!("{} connected", socket);
        Ok(socket)
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

    pub fn local_device_id(&self) -> &DeviceId {
        &self.local_device_id
    }

    pub fn remote(&self) -> &Endpoint {
        &self.remote
    }

    pub fn local(&self) -> &Endpoint {
        &self.local
    }

    pub async fn send(&self, data: &[u8]) -> BuckyResult<usize> {
        (&mut &self.socket).write(data).await.map_err(|err| BuckyError::from(err))
    }
}
