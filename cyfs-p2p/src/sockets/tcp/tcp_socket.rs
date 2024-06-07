use std::cell::RefCell;
use std::net::Shutdown;
use std::ops::Deref;
use std::sync::{Arc};
use std::time::Duration;
use crate::runtime::{Mutex, TcpStream};
use cyfs_base::{BuckyError, BuckyErrorCode, DeviceDesc, DeviceId, Endpoint, RawDecode, RawDecodeWithContext, RawFixedBytes};
use futures::{AsyncReadExt, AsyncWriteExt};
use crate::error::{bdt_err, BdtError, BdtErrorCode, BdtResult, into_bdt_err};
use crate::protocol::{OtherBoxTcpDecodeContext, PackageBox};
use crate::runtime;
use crate::types::MixAesKey;

#[derive(Eq, PartialEq)]
enum BoxType {
    Package,
    RawData,
}

pub enum RecvBox<'a> {
    Package(PackageBox),
    RawData(&'a [u8]),
}

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
            "TcpSocket {{local:{},remote:{}}}",
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
        timeout: Duration,) -> BdtResult<TCPSocket> {
        let socket = runtime::timeout(timeout, TcpStream::connect(remote_ep.addr()))
            .await
            .map_err(into_bdt_err!(BdtErrorCode::ConnectFailed, "tcp socket to {} connect failed", remote_ep))?
            .map_err(into_bdt_err!(BdtErrorCode::ConnectFailed, "tcp socket to {} connect failed", remote_ep))?;
        let local = socket.local_addr().map_err(into_bdt_err!(BdtErrorCode::Failed))?;
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

    pub async fn send(&self, data: &[u8]) -> BdtResult<()> {
        let _locker = self.write_lock.lock().await;
        #[cfg(feature = "runtime-async-std")]
        {
            (&mut &self.socket).write_all(data).await.map_err(into_bdt_err!(BdtErrorCode::IoError, "{}", self))
        }
        #[cfg(feature = "runtime-tokio")]
        {
            let mut buf = data;
            self.socket.writable().await.map_err(into_bdt_err!(BdtErrorCode::IoError, "{}", self))?;
            while buf.len() > 0 {
                match self.socket.try_write(buf) {
                    Ok(n) => {
                        buf = &buf[n..];
                        break;
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        continue;
                    }
                    Err(e) => {
                        return Err(BdtError::from((BdtErrorCode::IoError, "", e)));
                    }
                }
            }
            Ok(())
        }
    }

    pub async fn flush(&self) -> BdtResult<()> {
        #[cfg(feature = "runtime-async-std")]
        {
            (&mut &self.socket).flush().await.map_err(into_bdt_err!(BdtErrorCode::IoError, "{}", self))
        }
        #[cfg(feature = "runtime-tokio")]
        {
            Ok(())
        }
    }

    pub async fn recv(&self, buf: &mut [u8]) -> BdtResult<usize> {
        #[cfg(feature = "runtime-async-std")]
        {
            (&mut &self.socket).read(buf).await.map_err(into_bdt_err!(BdtErrorCode::IoError, "{}", self))
        }
        #[cfg(feature = "runtime-tokio")]
        {
            loop {
                self.socket.writable().await.map_err(into_bdt_err!(BdtErrorCode::IoError, "{}", self))?;
                match self.socket.try_read(buf) {
                    Ok(n) => return Ok(n),
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        continue;
                    }
                    Err(e) => {
                        return Err(BdtError::from((BdtErrorCode::IoError, "", e)));
                    }
                }
            }
        }
    }

    pub async fn recv_exact(&self, buf: &mut [u8]) -> BdtResult<()> {
        #[cfg(feature = "runtime-async-std")]
        {
            (&mut &self.socket).read_exact(buf).await.map_err(into_bdt_err!(BdtErrorCode::IoError, "{}", self))
        }
        #[cfg(feature = "runtime-tokio")]
        {
            let mut data: &mut [u8] = buf;
            while data.len() > 0 {
                self.socket.readable().await.map_err(into_bdt_err!(BdtErrorCode::IoError, "{}", self))?;
                match self.socket.try_read(data) {
                    Ok(n) => {
                        data = &mut data[n..];
                    },
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        continue;
                    }
                    Err(e) => {
                        return Err(BdtError::from((BdtErrorCode::IoError, "", e)));
                    }
                }
            }
            Ok(())
        }
    }

    pub(crate) async fn receive_package<'a>(&self, recv_buf: &'a mut [u8]) -> BdtResult<RecvBox<'a>> {
        let (box_type, box_buf) = self.receive_box(recv_buf).await?;
        match box_type {
            BoxType::Package => {
                let context = OtherBoxTcpDecodeContext::new_inplace(
                    box_buf.as_mut_ptr(),
                    box_buf.len(),
                    self.local_device_id(),
                    self.remote_device_id(),
                    self.key(),
                );
                let package = PackageBox::raw_decode_with_context(box_buf, context)
                    .map(|(package_box, _)| package_box).map_err(into_bdt_err!(BdtErrorCode::RawCodecError, "{}", self))?;
                Ok(RecvBox::Package(package))
            }
            BoxType::RawData => Ok(RecvBox::RawData(box_buf)),
        }
    }

    async fn receive_box<'a>(&self, recv_buf: &'a mut [u8]) -> BdtResult<(BoxType, &'a mut [u8])> {
        let header_len = u16::raw_bytes().unwrap();
        let box_header = &mut recv_buf[..header_len];
        self.recv_exact(box_header).await?;
        let mut box_len = u16::raw_decode(box_header).map(|(v, _)| v as usize).map_err(into_bdt_err!(BdtErrorCode::RawCodecError, "{}", self))?;
        let box_type = if box_len > 32768 {
            box_len -= 32768;
            BoxType::RawData
        } else {
            BoxType::Package
        };
        if box_len + header_len > recv_buf.len() {
            return Err(bdt_err!(
                BdtErrorCode::OutOfLimit,
                "buffer not enough",
            ));
        }
        let box_buf = &mut recv_buf[header_len..(header_len + box_len)];
        self.recv_exact(box_buf).await?;
        Ok((box_type, box_buf))
    }

    pub async fn shutdown(&self, how: Shutdown) -> BdtResult<()> {
        #[cfg(feature = "runtime-async-std")]
        {
            self.socket.shutdown(how).map_err(into_bdt_err!(BdtErrorCode::IoError))
        }
        #[cfg(feature = "runtime-tokio")]
        {
            Ok(())
        }
    }
}
