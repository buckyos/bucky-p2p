use bucky_raw_codec::{RawConvertTo, RawDecode, RawEncode, RawFixedBytes, RawFrom};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use crate::endpoint::Endpoint;
use crate::error::{P2pErrorCode, P2pResult, into_p2p_err};
use crate::executor::{Executor, SpawnHandle};
use crate::p2p_connection::{P2pConnectionRef, P2pRead, P2pWrite};
use crate::p2p_identity::P2pId;
use crate::sn::protocol::{Package, PackageCmdCode, PackageHeader};
use crate::runtime;
use crate::types::TunnelId;

#[callback_trait::callback_trait]
pub trait PeerConnectionEvent: 'static + Send + Sync {
    async fn on_recv(&self, conn_id: TunnelId, cmd_code: PackageCmdCode, cmd_body: Vec<u8>) -> P2pResult<()>;
}

pub struct PeerConnection {
    conn_id: TunnelId,
    socket: P2pConnectionRef,
    send: Box<dyn P2pWrite>,
    handle: Option<SpawnHandle<P2pResult<()>>>,
}

impl PeerConnection {
    pub async fn accept(conn_id: TunnelId, socket: P2pConnectionRef, listener: impl PeerConnectionEvent) -> P2pResult<Self> {
        let (recv, send) = socket.split()?;

        log::info!("recv PeerConnection {:?} remote_id {} remote_ep {} local_id {} local_ep {}",
            conn_id, socket.remote_id().to_string(), socket.remote(), socket.local_id().to_string(), socket.local());

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

    pub async fn connect(conn_id: TunnelId, socket: P2pConnectionRef, listener: impl PeerConnectionEvent) -> P2pResult<Self> {
        log::info!("new PeerConnection {:?} remote_id {} remote_ep {} local_id {} local_ep {}",
            conn_id, socket.remote_id().to_string(), socket.remote(), socket.local_id().to_string(), socket.local());
        let (recv, send) = socket.split()?;
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

    async fn read_pkg(recv: &mut Box<dyn P2pRead>) -> P2pResult<(PackageCmdCode, Vec<u8>)> {
        let mut buf_header = [0u8; 16];
        recv.read_exact(&mut buf_header[0..PackageHeader::raw_bytes().unwrap()]).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
        let header = PackageHeader::clone_from_slice(buf_header.as_slice()).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
        let cmd_code = match header.cmd_code() {
            Ok(cmd_code) => cmd_code,
            Err(err) => {
                return Err(err);
            }
        };
        let mut cmd_body = vec![0u8; header.pkg_len() as usize];
        recv.read_exact(cmd_body.as_mut_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
        Ok((cmd_code, cmd_body))
    }

    async fn recv(conn_id: TunnelId, mut recv: Box<dyn P2pRead>, listener: impl PeerConnectionEvent) -> P2pResult<()> {
        log::info!("enter recv loop");
        loop {
            let (cmd_code, cmd_body) = Self::read_pkg(&mut recv).await?;
            log::info!("conn {:?} recv {:?}", conn_id, cmd_code);
            if let Err(e) = listener.on_recv(conn_id, cmd_code, cmd_body).await {
                log::error!("on_recv error: {:?}", e);
            }
        }
    }

    pub fn conn_id(&self) -> TunnelId {
        self.conn_id
    }

    pub fn local(&self) -> Endpoint {
        self.socket.local()
    }

    pub fn remote(&self) -> Endpoint {
        self.socket.remote()
    }

    pub fn local_identity_id(&self) -> P2pId {
        self.socket.local_id()
    }

    pub fn remote_identity_id(&self) -> P2pId {
        self.socket.remote_id()
    }

    pub fn take_recv_handle(&mut self) -> Option<SpawnHandle<P2pResult<()>>> {
        self.handle.take()
    }

    pub async fn send<T: RawEncode + for <'a> RawDecode<'a>>(&mut self, pkg: Package<T>) -> P2pResult<()> {
        self.send.write_all(pkg.to_vec().map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?.as_slice()).await
            .map_err(into_p2p_err!(crate::error::P2pErrorCode::IoError))?;
        self.send.flush().await
            .map_err(into_p2p_err!(crate::error::P2pErrorCode::IoError))?;
        Ok(())
    }

    pub async fn shutdown(&mut self) -> P2pResult<()> {
        if self.handle.is_some() {
            self.handle.take().unwrap().abort();
        }
        Ok(())
    }
}

impl Drop for PeerConnection {
    fn drop(&mut self) {
        log::info!("drop PeerConnection {:?}", self.conn_id);
        let _ = Executor::block_on(self.shutdown());
    }
}
