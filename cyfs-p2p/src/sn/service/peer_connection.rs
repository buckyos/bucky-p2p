use bucky_objects::{DeviceId, Endpoint};
use bucky_raw_codec::{RawConvertTo, RawDecode, RawEncode, RawFixedBytes, RawFrom};
use quinn::{RecvStream, SendStream};
use tokio::io::AsyncWriteExt;
use crate::error::{BdtErrorCode, BdtResult, into_bdt_err};
use crate::executor::{Executor, SpawnHandle};
use crate::{IncreaseId, IncreaseIdGenerator, TempSeq};
use crate::protocol::{Package, PackageCmdCode, PackageHeader};
use crate::sockets::QuicSocket;

#[callback_trait::callback_trait]
pub trait PeerConnectionEvent: 'static + Send + Sync {
    async fn on_recv(&self, conn_id: TempSeq, cmd_code: PackageCmdCode, cmd_body: Vec<u8>) -> BdtResult<()>;
}

pub struct PeerConnection {
    conn_id: TempSeq,
    socket: QuicSocket,
    send: SendStream,
    handle: Option<SpawnHandle<BdtResult<()>>>,
}

impl PeerConnection {
    pub async fn accept(conn_id: TempSeq, socket: QuicSocket, listener: impl PeerConnectionEvent) -> BdtResult<Self> {
        let (send, recv) = socket.socket().accept_bi().await
            .map_err(into_bdt_err!(crate::error::BdtErrorCode::QuicError))?;

        log::info!("recv PeerConnection {:?} remote_id {} remote_ep {} local_id {} local_ep {}",
            conn_id, socket.remote_device_id().to_string(), socket.remote(), socket.local_device_id().to_string(), socket.local());

        let handle = Executor::spawn_with_handle(async move {
            if let Err(e) = Self::recv(conn_id, recv, listener).await {
                log::error!("recv error: {:?}", e);
            }
            Ok(())
        })?;

        Ok(Self {
            conn_id,
            socket,
            send,
            handle: Some(handle),
        })
    }

    pub async fn connect(conn_id: TempSeq, socket: QuicSocket, listener: impl PeerConnectionEvent) -> BdtResult<Self> {
        log::info!("new PeerConnection {:?} remote_id {} remote_ep {} local_id {} local_ep {}",
            conn_id, socket.remote_device_id().to_string(), socket.remote(), socket.local_device_id().to_string(), socket.local());
        let (send, recv) = socket.socket().open_bi().await
            .map_err(into_bdt_err!(crate::error::BdtErrorCode::QuicError))?;
        let handle = Executor::spawn_with_handle(async move {
            if let Err(e) = Self::recv(conn_id, recv, listener).await {
                log::error!("recv error: {:?}", e);
            }
            Ok(())
        })?;
        Ok(Self {
            conn_id,
            socket,
            send,
            handle: Some(handle),
        })
    }

    async fn read_pkg(recv: &mut RecvStream) -> BdtResult<(PackageCmdCode, Vec<u8>)> {
        let mut buf_header = [0u8; 16];
        recv.read_exact(&mut buf_header[0..PackageHeader::raw_bytes().unwrap()]).await.map_err(into_bdt_err!(BdtErrorCode::IoError))?;
        let header = PackageHeader::clone_from_slice(buf_header.as_slice()).map_err(into_bdt_err!(BdtErrorCode::RawCodecError))?;
        let cmd_code = match header.cmd_code() {
            Ok(cmd_code) => cmd_code,
            Err(err) => {
                return Err(err);
            }
        };
        let mut cmd_body = vec![0u8; header.pkg_len() as usize];
        recv.read_exact(cmd_body.as_mut_slice()).await.map_err(into_bdt_err!(BdtErrorCode::IoError))?;
        Ok((cmd_code, cmd_body))
    }

    async fn recv(conn_id: TempSeq, mut recv: RecvStream, listener: impl PeerConnectionEvent) -> BdtResult<()> {
        log::info!("enter recv loop");
        loop {
            let (cmd_code, cmd_body) = Self::read_pkg(&mut recv).await?;
            log::info!("conn {:?} recv {:?}", conn_id, cmd_code);
            if let Err(e) = listener.on_recv(conn_id, cmd_code, cmd_body).await {
                log::error!("on_recv error: {:?}", e);
            }
        }
    }

    pub fn conn_id(&self) -> TempSeq {
        self.conn_id
    }

    pub fn local(&self) -> &Endpoint {
        &self.socket.local()
    }

    pub fn remote(&self) -> &Endpoint {
        &self.socket.remote()
    }

    pub fn local_device_id(&self) -> &DeviceId {
        &self.socket.local_device_id()
    }

    pub fn remote_device_id(&self) -> &DeviceId {
        &self.socket.remote_device_id()
    }

    pub fn take_recv_handle(&mut self) -> Option<SpawnHandle<BdtResult<()>>> {
        self.handle.take()
    }

    pub async fn send<T: RawEncode + for <'a> RawDecode<'a>>(&mut self, pkg: Package<T>) -> BdtResult<()> {
        self.send.write_all(pkg.to_vec().map_err(into_bdt_err!(BdtErrorCode::RawCodecError))?.as_slice()).await
            .map_err(into_bdt_err!(crate::error::BdtErrorCode::IoError))?;
        self.send.flush().await
            .map_err(into_bdt_err!(crate::error::BdtErrorCode::IoError))?;
        Ok(())
    }

    pub(crate) async fn shutdown(&mut self) -> BdtResult<()> {
        if self.handle.is_some() {
            self.handle.take().unwrap().abort();
        }
        self.send.finish();
        self.send.stopped().await.map_err(into_bdt_err!(BdtErrorCode::IoError));
        self.socket.shutdown().await?;
        Ok(())
    }
}

impl Drop for PeerConnection {
    fn drop(&mut self) {
        log::info!("drop PeerConnection {:?}", self.conn_id);
        Executor::block_on(self.shutdown());
    }
}
