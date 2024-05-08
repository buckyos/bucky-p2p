use std::cell::RefCell;
use std::ops::Deref;
use std::sync::{Arc};
use std::time::Duration;
use async_std::net::{TcpListener, TcpStream};
use async_std::sync::Mutex;
use cyfs_base::{BuckyError, BuckyErrorCode, BuckyResult, DeviceDesc, DeviceId, Endpoint};
use futures::{AsyncReadExt, AsyncWriteExt};
use crate::types::MixAesKey;

pub struct TCPSocket {
    socket: TcpStream,
    remote_device_id: DeviceId,
    local_device_id: DeviceId,
    local: Endpoint,
    remote: Endpoint,
    key: MixAesKey,
    write_lock: Mutex<u8>,
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
            write_lock: Mutex::new(0),
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
            write_lock: Mutex::new(0),
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

    pub async fn send(&self, data: &[u8]) -> BuckyResult<()> {
        let _locker = self.write_lock.lock().await;
        (&mut &self.socket).write_all(data).await.map_err(|err| BuckyError::from(err))
    }

    pub async fn recv(&self, buf: &mut [u8]) -> BuckyResult<usize> {
        (&mut &self.socket).read(buf).await.map_err(|err| BuckyError::from(err))
    }

    pub async fn recv_exact(&self, buf: &mut [u8]) -> BuckyResult<()> {
        (&mut &self.socket).read_exact(buf).await.map_err(|err| BuckyError::from(err))
    }
}
