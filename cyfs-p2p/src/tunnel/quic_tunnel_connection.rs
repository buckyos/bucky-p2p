use std::cmp::PartialEq;
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::io::{Error, ErrorKind};
use std::net::Shutdown;
use std::pin::Pin;
use std::sync::{Arc, Mutex, Weak};
use std::task::{Context, Poll};
use std::time::Duration;
use bucky_error::BuckyErrorCode;
use bucky_objects::{DeviceDesc, DeviceId, Endpoint};
use bucky_raw_codec::{RawConvertTo, RawFixedBytes, RawFrom, TailedOwnedData};
use bucky_time::bucky_time_now;
use callback_result::SingleCallbackWaiter;
use notify_future::NotifyFuture;
use quinn::{ReadError, RecvStream, SendStream};
use quinn_proto::VarInt;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use crate::sockets::{NetManagerRef, QuicSocket};
use crate::{IncreaseId, LocalDeviceRef, MixAesKey, runtime, TempSeq};
use crate::error::{bdt_err, BdtErrorCode, BdtResult, into_bdt_err};
use crate::executor::Executor;
use crate::history::keystore::EncryptedKey;
use crate::protocol::{AckTunnel, Package, MTU, PackageCmdCode, SynTunnel, PackageHeader, MTU_LARGE};
use crate::protocol::v0::{AckStream, SynStream};
use crate::tunnel::{SocketType, TunnelConnection, TunnelDatagramRecv, TunnelDatagramSend, TunnelInstance, TunnelListenPorts, TunnelListenPortsRef, TunnelStream, TunnelType};

#[derive(Debug, Eq, PartialEq)]
enum TunnelState {
    Init,
    Idle,
    Shutdown,
    Error,
}

pub struct QuicTunnelConnectionImpl {
    net_manager: NetManagerRef,
    sequence: TempSeq,
    local_device: LocalDeviceRef,
    remote_id: DeviceId,
    remote_ep: Endpoint,
    conn_timeout: Duration,
    data_socket: Option<QuicSocket>,
    protocol_version: u8,
    stack_version: u32,
    remainder: u16,
    local_ep: Endpoint,
    tunnel_state: TunnelState,
    listen_ports: TunnelListenPortsRef,
}

impl QuicTunnelConnectionImpl {
    pub fn new(net_manager: NetManagerRef,
               sequence: TempSeq,
               local_device: LocalDeviceRef,
               remote_id: DeviceId,
               remote_ep: Endpoint,
               conn_timeout: Duration,
               protocol_version: u8,
               stack_version: u32,
               local_ep: Endpoint,
               data_sender: Option<QuicSocket>,
               listen_ports: TunnelListenPortsRef,) -> Self {
        let tunnel_state = if data_sender.is_some() {
            TunnelState::Idle
        } else {
            TunnelState::Init
        };
        Self {
            net_manager,
            sequence,
            local_device,
            remote_id,
            remote_ep,
            conn_timeout,
            data_socket: data_sender,
            protocol_version,
            stack_version,
            remainder: 0,
            local_ep,
            tunnel_state,
            listen_ports,
        }
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

    async fn open_stream_inner(socket: &quinn::Connection, sequence: TempSeq, vport: u16, session_id: IncreaseId) -> BdtResult<Box<dyn TunnelStream>> {
        let (mut send, mut recv) = socket.open_bi().await.map_err(into_bdt_err!(BdtErrorCode::ConnectFailed))?;
        let syn = SynStream {
            sequence,
            to_vport: vport,
            session_id,
            payload: Vec::new(),
        };
        let pkg = Package::new(PackageCmdCode::SynStream, syn);
        send.write_all(pkg.to_vec().map_err(into_bdt_err!(BdtErrorCode::RawCodecError))?.as_slice()).await.map_err(into_bdt_err!(BdtErrorCode::IoError))?;
        send.flush().await.map_err(into_bdt_err!(BdtErrorCode::IoError))?;

        let (cmd_code, cmd_body) = Self::read_pkg(&mut recv).await?;
        if cmd_code != PackageCmdCode::AckStream {
            return Err(bdt_err!(BdtErrorCode::InvalidData, "tunnel {:?} invalid ack stream", sequence));
        }

        let ack = AckStream::clone_from_slice(cmd_body.as_slice()).map_err(into_bdt_err!(BdtErrorCode::RawCodecError))?;
        if ack.result != 0 {
            return Err(bdt_err!(BdtErrorCode::ConnectionRefused, "tunnel {:?} open stream failed. return {}", sequence, ack.result));
        }

        Ok(Box::new(QuicTunnelStream::new(vport, session_id, send, recv)))
    }

    async fn open_datagram_inner(socket: &quinn::Connection) -> BdtResult<Box<dyn TunnelDatagramSend>> {
        let send = socket.open_uni().await.map_err(into_bdt_err!(BdtErrorCode::ConnectFailed))?;

        Ok(Box::new(QuicTunnelDatagramSend::new(send)))
    }
}

pub struct QuicTunnelConnection {
    inner: Arc<Mutex<QuicTunnelConnectionImpl>>,
}

impl QuicTunnelConnection {
    pub fn new(net_manager: NetManagerRef,
               sequence: TempSeq,
               local_device: LocalDeviceRef,
               remote_id: DeviceId,
               remote_ep: Endpoint,
               conn_timeout: Duration,
               protocol_version: u8,
               stack_version: u32,
               local_ep: Endpoint,
               data_sender: Option<QuicSocket>,
               listen_ports: TunnelListenPortsRef,) -> Self {
        let inner = QuicTunnelConnectionImpl::new(
            net_manager,
            sequence,
            local_device,
            remote_id,
            remote_ep,
            conn_timeout,
            protocol_version,
            stack_version,
            local_ep,
            data_sender,
            listen_ports);
        Self {
            inner: Arc::new(Mutex::new(inner)),
        }
    }
}
#[async_trait::async_trait]
impl TunnelConnection for QuicTunnelConnection {
    fn socket_type(&self) -> SocketType {
        SocketType::UDP
    }

    fn is_idle(&self) -> bool {
        let inner = self.inner.lock().unwrap();
        inner.tunnel_state == TunnelState::Idle
    }

    async fn connect(&self) -> BdtResult<()> {
        let (local_device, remote_id, remote_ep) = {
            let mut inner = self.inner.lock().unwrap();
            if inner.data_socket.is_some() {
                return Ok(());
            }
            (inner.local_device.clone(), inner.remote_id.clone(), inner.remote_ep.clone())
        };

        let mut socket = QuicSocket::connect(local_device, remote_id, remote_ep).await?;
        {
            let mut inner = self.inner.lock().unwrap();
            inner.data_socket = Some(socket);
        }

        Ok(())
    }

    async fn open_stream(&self, vport: u16, session_id: IncreaseId) -> BdtResult<Box<dyn TunnelStream>> {
        let (conn, sequence) = {
            let mut inner = self.inner.lock().unwrap();
            if inner.tunnel_state != TunnelState::Idle {
                return Err(bdt_err!(BdtErrorCode::ErrorState, "tunnel state error"));
            }
            (inner.data_socket.as_ref().unwrap().socket().clone(), inner.sequence.clone())
        };
        match QuicTunnelConnectionImpl::open_stream_inner(&conn, sequence, vport, session_id).await {
            Ok(stream) => {
                Ok(stream)
            }
            Err(e) => {
                let mut inner = self.inner.lock().unwrap();
                inner.tunnel_state = TunnelState::Error;
                Err(e)
            }
        }
    }

    async fn open_datagram(&self) -> BdtResult<Box<dyn TunnelDatagramSend>> {
        let conn = {
            let inner = self.inner.lock().unwrap();
            if inner.tunnel_state != TunnelState::Idle {
                return Err(bdt_err!(BdtErrorCode::ErrorState, "tunnel state error"));
            }
            inner.data_socket.as_ref().unwrap().socket().clone()
        };
        match QuicTunnelConnectionImpl::open_datagram_inner(&conn).await {
            Ok(data) => {
                Ok(data)
            }
            Err(e) => {
                let mut inner = self.inner.lock().unwrap();
                inner.tunnel_state = TunnelState::Error;
                Err(e)
            }
        }
    }

    async fn accept_instance(&self) -> BdtResult<TunnelInstance> {
        let socket = {
            let mut inner = self.inner.lock().unwrap();
            inner.data_socket.as_ref().unwrap().socket().clone()
        };
        let (bi_accept, uni_accept) = {
            let bi_accept = socket.accept_bi();
            let uni_accept = socket.accept_uni();
            (bi_accept, uni_accept)
        };

        runtime::select!{
            ret = bi_accept => {
                match ret {
                    Ok((send, mut recv)) => {
                        let (cmd_code, cmd_body) = QuicTunnelConnectionImpl::read_pkg(&mut recv).await.map_err(into_bdt_err!(BdtErrorCode::IoError))?;
                        if cmd_code != PackageCmdCode::SynStream {
                            return Err(bdt_err!(BdtErrorCode::InvalidData, "tunnel invalid syn stream"));
                        }
                        let syn = SynStream::clone_from_slice(cmd_body.as_slice()).map_err(into_bdt_err!(BdtErrorCode::RawCodecError))?;
                        return Ok(TunnelInstance::Stream(Box::new(QuicTunnelStream::new(syn.to_vport, syn.session_id, send, recv))))
                    }
                    Err(e) => {
                        let mut inner = self.inner.lock().unwrap();
                        inner.tunnel_state = TunnelState::Error;
                        return Err(bdt_err!(BdtErrorCode::IoError, "{}", e));
                    }}
            },
            ret = uni_accept => {
                match ret {
                    Ok(recv) => {
                        return Ok(TunnelInstance::Datagram(Box::new(QuicTunnelDatagramRecv::new(recv))))
                    }
                    Err(e) => {
                        let mut inner = self.inner.lock().unwrap();
                        inner.tunnel_state = TunnelState::Error;
                        return Err(bdt_err!(BdtErrorCode::IoError, "{}", e));
                    }
                }
            }
        };
    }

    async fn shutdown(&self) -> BdtResult<()> {
        let data_socket = {
            let mut inner = self.inner.lock().unwrap();
            inner.data_socket.clone()
        };
        match data_socket.as_ref().unwrap().shutdown().await {
            Ok(_) => {
                Ok(())
            }
            Err(e) => {
                let mut inner = self.inner.lock().unwrap();
                inner.tunnel_state = TunnelState::Error;
                Err(e)
            }
        }
    }
}

pub struct QuicTunnelStream {
    port: u16,
    session_id: IncreaseId,
    send: SendStream,
    recv: RecvStream,
}

impl QuicTunnelStream {
    fn new(port: u16, session_id: IncreaseId, send: SendStream, recv: RecvStream) -> Self {
        Self {
            port,
            session_id,
            send,
            recv,
        }
    }
}

#[async_trait::async_trait]
impl AsyncWrite for QuicTunnelStream {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<Result<usize, Error>> {
        match Pin::new(&mut self.as_mut().send).poll_write(cx, buf) {
            Poll::Ready(Ok(size)) => {
                Poll::Ready(Ok(size))
            }
            Poll::Ready(Err(e)) => {
                Poll::Ready(Err(Error::new(ErrorKind::Other, e)))
            }
            Poll::Pending => {
                Poll::Pending
            }
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        match Pin::new(&mut self.as_mut().send).poll_flush(cx) {
            Poll::Ready(Ok(())) => {
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(e)) => {
                Poll::Ready(Err(Error::new(ErrorKind::Other, e)))
            }
            Poll::Pending => {
                Poll::Pending
            }
        }
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        match Pin::new(&mut self.as_mut().send).poll_shutdown(cx) {
            Poll::Ready(Ok(())) => {
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(e)) => {
                Poll::Ready(Err(Error::new(ErrorKind::Other, e)))
            }
            Poll::Pending => {
                Poll::Pending
            }
        }
    }
}

#[async_trait::async_trait]
impl AsyncRead for QuicTunnelStream {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        match self.as_mut().recv.poll_read(cx, buf.initialize_unfilled()) {
            Poll::Ready(Ok(size)) => {
                buf.advance(size);
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(e)) => {
                Poll::Ready(Err(Error::new(ErrorKind::Other, e)))
            }
            Poll::Pending => {
                Poll::Pending
            }
        }
    }
}

#[async_trait::async_trait]
impl TunnelStream for QuicTunnelStream {
    fn port(&self) -> u16 {
        self.port
    }

    async fn close(&mut self) -> BdtResult<()> {
        self.send.finish().map_err(into_bdt_err!(BdtErrorCode::IoError))?;
        self.send.stopped().await.map_err(into_bdt_err!(BdtErrorCode::IoError))?;
        self.recv.stop(VarInt::from_u32(0)).map_err(into_bdt_err!(BdtErrorCode::IoError))?;
        Ok(())
    }
}

impl Drop for QuicTunnelStream {
    fn drop(&mut self) {
        let _ = Executor::block_on(self.close());
    }
}

pub struct QuicTunnelDatagramSend {
    send: SendStream
}

impl QuicTunnelDatagramSend {
    fn new(send: SendStream) -> Self {
        Self {
            send
        }
    }
}

#[async_trait::async_trait]
impl AsyncWrite for QuicTunnelDatagramSend {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<Result<usize, Error>> {
        match Pin::new(&mut self.as_mut().send).poll_write(cx, buf) {
            Poll::Ready(Ok(size)) => {
                Poll::Ready(Ok(size))
            }
            Poll::Ready(Err(e)) => {
                Poll::Ready(Err(Error::new(ErrorKind::Other, e)))
            }
            Poll::Pending => {
                Poll::Pending
            }
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        match Pin::new(&mut self.as_mut().send).poll_flush(cx) {
            Poll::Ready(Ok(())) => {
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(e)) => {
                Poll::Ready(Err(Error::new(ErrorKind::Other, e)))
            }
            Poll::Pending => {
                Poll::Pending
            }
        }
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        match Pin::new(&mut self.as_mut().send).poll_shutdown(cx) {
            Poll::Ready(Ok(())) => {
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(e)) => {
                Poll::Ready(Err(Error::new(ErrorKind::Other, e)))
            }
            Poll::Pending => {
                Poll::Pending
            }
        }
    }
}

#[async_trait::async_trait]
impl TunnelDatagramSend for QuicTunnelDatagramSend {
    async fn close(&mut self) -> BdtResult<()> {
        self.send.finish().map_err(into_bdt_err!(BdtErrorCode::IoError))?;
        self.send.stopped().await.map_err(into_bdt_err!(BdtErrorCode::IoError))?;
        Ok(())
    }
}

impl Drop for QuicTunnelDatagramSend {
    fn drop(&mut self) {
        let _ = Executor::block_on(self.close());
    }

}

pub struct QuicTunnelDatagramRecv {
    recv: RecvStream,
}

impl QuicTunnelDatagramRecv {
    fn new(recv: RecvStream) -> Self {
        Self {
            recv
        }
    }
}

#[async_trait::async_trait]
impl AsyncRead for QuicTunnelDatagramRecv {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        match self.as_mut().recv.poll_read(cx, buf.initialize_unfilled()) {
            Poll::Ready(Ok(size)) => {
                buf.advance(size);
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(e)) => {
                Poll::Ready(Err(Error::new(ErrorKind::Other, e)))
            }
            Poll::Pending => {
                Poll::Pending
            }
        }
    }
}

#[async_trait::async_trait]
impl TunnelDatagramRecv for QuicTunnelDatagramRecv {
    async fn close(&mut self) -> BdtResult<()> {
        self.recv.stop(VarInt::from_u32(0)).map_err(into_bdt_err!(BdtErrorCode::IoError))?;
        Ok(())
    }
}

impl Drop for QuicTunnelDatagramRecv {
    fn drop(&mut self) {
        let _ = self.recv.stop(VarInt::from_u32(0)).map_err(into_bdt_err!(BdtErrorCode::IoError));
    }
}
