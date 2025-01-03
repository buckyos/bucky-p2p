use std::fmt::{Display, Formatter};
use std::future::Future;
use std::io::{Error, ErrorKind};
use std::ops::DerefMut;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;
use bucky_raw_codec::{RawConvertTo, RawFixedBytes, RawFrom};
use bucky_time::bucky_time_now;
use futures::FutureExt;
use notify_future::NotifyFuture;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use crate::endpoint::Endpoint;
use crate::error::{into_p2p_err, p2p_err, P2pErrorCode, P2pResult};
use crate::executor::{Executor, SpawnHandle};
use crate::p2p_connection::P2pConnectionRef;
use crate::p2p_identity::{P2pId, P2pIdentityCertFactoryRef, P2pIdentityRef};
use crate::protocol::{Package, PackageCmdCode, PackageHeader, MTU_LARGE};
use crate::protocol::v0::{AckClose, AckDatagram, AckReverseDatagram, AckReverseStream, AckStream, SynClose, SynDatagram, SynReverseDatagram, SynReverseStream, SynStream};
use crate::runtime;
use crate::sockets::tcp::{TCPConnection};
use crate::types::{IncreaseId, TempSeq};

pub trait TunnelListenPorts: 'static + Send + Sync {
    fn is_listen(&self, port: u16) -> bool;
}
pub type TunnelListenPortsRef = Arc<dyn TunnelListenPorts>;

#[derive(Debug, Clone, Copy, Ord, PartialOrd, PartialEq, Eq)]
pub enum TunnelType {
    IDLE,
    TUNNEL,
    STREAM(u16),
    ERROR
}

#[derive(Copy, Clone, Eq, PartialEq)]
pub enum SocketType {
    TCP,
    UDP
}

impl TunnelType {
    pub fn is_tunnel(&self) -> bool {
        matches!(self, TunnelType::IDLE)
    }

    pub fn is_stream(&self) -> bool {
        matches!(self, TunnelType::STREAM(_))
    }

    pub fn get_vport(&self) -> Option<u16> {
        match self {
            TunnelType::STREAM(vport) => Some(*vport),
            _ => None,
        }
    }
}

struct TunnelStatState {
    pub work_instance_num: u32,
    pub latest_active_time: u64,
}

pub struct TunnelStat {
    state: Mutex<TunnelStatState>
}
pub type TunnelStatRef = Arc<TunnelStat>;

impl TunnelStat {
    pub fn new() -> TunnelStatRef {
        Arc::new(TunnelStat {
            state: Mutex::new(TunnelStatState {
                work_instance_num: 0,
                latest_active_time: 0,
            })
        })
    }

    pub fn increase_work_instance(&self) {
        let mut state = self.state.lock().unwrap();
        state.work_instance_num += 1;
        state.latest_active_time = bucky_time_now();
    }

    pub fn decrease_work_instance(&self) {
        let mut state = self.state.lock().unwrap();
        state.work_instance_num = state.work_instance_num - 1;
        state.latest_active_time = bucky_time_now();
    }

    pub fn get_work_instance_num(&self) -> u32 {
        let state = self.state.lock().unwrap();
        state.work_instance_num
    }

    pub fn get_latest_active_time(&self) -> u64 {
        let state = self.state.lock().unwrap();
        state.latest_active_time
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum TunnelState {
    Init,
    Idle,
    Opening,
    Worked,
    Accepting,
    Error,
}

pub enum TunnelInstance {
    Stream(TunnelStream),
    Datagram(TunnelDatagramRecv),
    ReverseStream(TunnelStream),
    ReverseDatagram(TunnelDatagramSend),
}

impl Display for TunnelInstance {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            TunnelInstance::Stream(stream) => write!(f, "Stream(seq {:?} session {} port {})", stream.sequence(), stream.session_id(), stream.port()),
            TunnelInstance::Datagram(datagram) => write!(f, "Datagram({:?})", datagram.sequence()),
            TunnelInstance::ReverseStream(stream) => write!(f, "ReverseStream(seq {:?} session {} port {})", stream.sequence(), stream.session_id(), stream.port()),
            TunnelInstance::ReverseDatagram(datagram) => write!(f, "ReverseDatagram({:?})", datagram.sequence()),
        }
    }
}

async fn accept_first_pkg(mut read: Box<dyn runtime::AsyncRead + Send + Sync + 'static + Unpin>) -> P2pResult<(Box<dyn runtime::AsyncRead + Send + Sync + 'static + Unpin>, PackageCmdCode, Vec<u8>)> {
    let mut buf_header = [0u8; 16];
    read.read_exact(&mut buf_header[0..PackageHeader::raw_bytes().unwrap()]).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
    let header = PackageHeader::clone_from_slice(buf_header.as_slice()).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
    let cmd_code = match header.cmd_code() {
        Ok(cmd_code) => cmd_code,
        Err(err) => {
            return Err(err);
        }
    };
    let mut cmd_body = vec![0u8; header.pkg_len() as usize];
    read.read_exact(cmd_body.as_mut_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
    log::info!("accept first pkg cmd code {:?}", cmd_code);
    Ok((read, cmd_code, cmd_body))
}

fn create_accept_handle(read: Box<dyn runtime::AsyncRead + Send + Sync + 'static + Unpin>,
                        accept_future: Arc<Mutex<Option<NotifyFuture<P2pResult<(Box<dyn runtime::AsyncRead + Send + Sync + 'static + Unpin>, PackageCmdCode, Vec<u8>)>>>>>,
                        recv_future: Arc<Mutex<Option<NotifyFuture<P2pResult<(Box<dyn runtime::AsyncRead + Send + Sync + 'static + Unpin>, PackageCmdCode, Vec<u8>)>>>>>
) -> P2pResult<SpawnHandle<()>> {
    Executor::spawn_with_handle(async move {
        let ret = accept_first_pkg(read).await;
        if ret.is_ok() {
            {
                let mut recv_future = recv_future.lock().unwrap();
                if recv_future.is_some() {
                    recv_future.take().unwrap().set_complete(ret);
                    return;
                }
            }
            {
                let mut accept_future = accept_future.lock().unwrap();
                if accept_future.is_some() {
                    accept_future.take().unwrap().set_complete(ret);
                }
            }
        } else {
            {
                let mut recv_future = recv_future.lock().unwrap();
                if recv_future.is_some() {
                    recv_future.take().unwrap().set_complete(ret);
                }
            }
            {
                let mut accept_future = accept_future.lock().unwrap();
                if accept_future.is_some() {
                    accept_future.take().unwrap().set_complete(Err(p2p_err!(P2pErrorCode::Interrupted, "accept handle abort")));
                }
            }
        }
    })
}

pub struct TunnelStream {
    tunnel: Arc<Mutex<TunnelConnectionInner>>,
    session_id: IncreaseId,
    port: u16,
    read: Option<Box<dyn runtime::AsyncRead + Send + Sync + 'static + Unpin>>,
    write: Option<Box<dyn runtime::AsyncWrite + Send + Sync + 'static + Unpin>>,
    remainder: u16,
    read_future: Option<Pin<Box<dyn Send + Future<Output=P2pResult<usize>>>>>,
    write_future: Option<Pin<Box<dyn Send + Future<Output=std::io::Result<()>>>>>,
}

impl TunnelStream {
    pub(crate) fn new(
        tunnel: Arc<Mutex<TunnelConnectionInner>>,
        read: Box<dyn runtime::AsyncRead + Send + Sync + 'static + Unpin>,
        write: Box<dyn runtime::AsyncWrite + Send + Sync + 'static + Unpin>,
        session_id: IncreaseId,
        port: u16,) -> Self {
        {
            let tunnel = tunnel.lock().unwrap();
            tunnel.tunnel_stat.increase_work_instance();
        }
        Self {
            tunnel,
            session_id,
            port,
            read: Some(read),
            write: Some(write),
            remainder: 0,
            read_future: None,
            write_future: None,
        }
    }

    async fn handle_cmd(&mut self, cmd_code: PackageCmdCode, cmd_body: &[u8]) -> P2pResult<()> {
        if cmd_code == PackageCmdCode::SynClose {
            let syn_close = SynClose::clone_from_slice(cmd_body).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
            let ack = AckClose {
                sequence: syn_close.sequence,
            };
            let pkg = Package::new(PackageCmdCode::AckClose, ack);
            self.write.as_mut().unwrap().write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
            let mut tunnel = self.tunnel.lock().unwrap();
            tunnel.data_socket.as_mut().unwrap().unsplit(self.read.take().unwrap(), self.write.take().unwrap());
            let _ = tunnel.enter_idle_mode();
            Ok(())
        } else {
            let tunnel = self.tunnel.lock().unwrap();
            Err(p2p_err!(P2pErrorCode::ErrorState, "tunnel {:?} invalid cmd code {:?}", tunnel.sequence, cmd_code))
        }
    }

    async fn send_inner(&mut self, data: &[u8]) -> std::io::Result<()> {
        let mut remainder = data;
        while remainder.len() > 0 {
            let chunk = if remainder.len() > MTU_LARGE as usize {
                &remainder[..MTU_LARGE as usize]
            } else {
                remainder
            };
            let header = PackageHeader::new(PackageCmdCode::PieceData, chunk.len() as u16);
            self.write.as_mut().unwrap().write_all(header.to_vec().unwrap().as_slice()).await?;
            self.write.as_mut().unwrap().write_all(chunk).await?;
            remainder = &remainder[chunk.len()..];
        }
        Ok(())
    }

    async fn send(&mut self, data: &[u8]) -> std::io::Result<()> {
        match self.send_inner(data).await {
            Ok(()) => {
                Ok(())
            }
            Err(err) => {
                let mut tunnel = self.tunnel.lock().unwrap();
                tunnel.tunnel_state = TunnelState::Error;
                Err(err)
            }
        }
    }

    async fn recv_inner(&mut self, buf: &mut [u8]) -> P2pResult<usize> {
        {
            let tunnel = self.tunnel.lock().unwrap();
            if tunnel.tunnel_state != TunnelState::Worked {
                return Ok(0);
            }
        }
        if self.remainder == 0 {
            let mut buf_header = [0u8; 16];
            self.read.as_mut().unwrap().read_exact(&mut buf_header[0..PackageHeader::raw_bytes().unwrap()]).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
            let header = PackageHeader::clone_from_slice(buf_header.as_slice()).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
            let cmd_code = match header.cmd_code() {
                Ok(cmd_code) => cmd_code,
                Err(err) => {
                    return Err(err);
                }
            };
            if cmd_code != PackageCmdCode::PieceData {
                let mut cmd_body = vec![0u8; header.pkg_len() as usize];
                self.read.as_mut().unwrap().read_exact(cmd_body.as_mut_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
                self.handle_cmd(cmd_code, cmd_body.as_slice()).await?;
                return Ok(0);
            } else {
                self.remainder = header.pkg_len();
            }
        }
        if self.remainder as usize > buf.len() {
            let recv_len = self.read.as_mut().unwrap().read(buf).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
            self.remainder -= recv_len as u16;
            Ok(recv_len)
        } else {
            let recv_len = self.remainder;
            self.read.as_mut().unwrap().read_exact(&mut buf[..self.remainder as usize]).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
            self.remainder = 0;
            Ok(recv_len as usize)
        }
    }

    async fn recv(&mut self, buf: &mut [u8]) -> P2pResult<usize> {
        match self.recv_inner(buf).await {
            Ok(size) => {
                Ok(size)
            }
            Err(err) => {
                let mut tunnel = self.tunnel.lock().unwrap();
                tunnel.tunnel_state = TunnelState::Error;
                Err(err)
            }
        }
    }

    async fn read_pkg(&mut self) -> P2pResult<(PackageCmdCode, Vec<u8>)> {
        let mut buf_header = [0u8; 16];
        self.read.as_mut().unwrap().read_exact(&mut buf_header[0..PackageHeader::raw_bytes().unwrap()]).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
        let header = PackageHeader::clone_from_slice(buf_header.as_slice()).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
        let cmd_code = match header.cmd_code() {
            Ok(cmd_code) => cmd_code,
            Err(err) => {
                return Err(err);
            }
        };
        let mut cmd_body = vec![0u8; header.pkg_len() as usize];
        self.read.as_mut().unwrap().read_exact(cmd_body.as_mut_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
        Ok((cmd_code, cmd_body))
    }

    async fn close_inner(&mut self) -> P2pResult<()> {
        let sequence = {
            self.tunnel.lock().unwrap().sequence
        };
        let pkg = Package::new(PackageCmdCode::SynClose, SynClose {
            sequence: sequence,
        });

        self.write.as_mut().unwrap().write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;

        loop {
            let (cmd_code, cmd_body) = self.read_pkg().await?;
            if cmd_code == PackageCmdCode::AckClose {
                let close = AckClose::clone_from_slice(cmd_body.as_slice()).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
                if close.sequence == sequence {
                    break;
                }
            } else if cmd_code == PackageCmdCode::SynClose {
                let close = SynClose::clone_from_slice(cmd_body.as_slice()).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
                if close.sequence != sequence {
                    continue;
                }
                let ack = AckClose {
                    sequence: close.sequence,
                };
                let pkg = Package::new(PackageCmdCode::AckClose, ack);
                self.write.as_mut().unwrap().write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
            }
        }

        {
            let mut tunnel = self.tunnel.lock().unwrap();
            tunnel.data_socket.as_mut().unwrap().unsplit(self.read.take().unwrap(), self.write.take().unwrap());
            tunnel.enter_idle_mode()?;
        }

        Ok(())
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn session_id(&self) -> IncreaseId {
        self.session_id
    }

    pub fn sequence(&self) -> TempSeq {
        let tunnel = self.tunnel.lock().unwrap();
        tunnel.sequence
    }

    pub fn remote_identity_id(&self) -> P2pId {
        let tunnel = self.tunnel.lock().unwrap();
        tunnel.remote_id.clone()
    }

    pub fn local_identity_id(&self) -> P2pId {
        let tunnel = self.tunnel.lock().unwrap();
        tunnel.local_identity.get_id()
    }

    pub fn remote_endpoint(&self) -> Endpoint {
        let tunnel = self.tunnel.lock().unwrap();
        tunnel.remote_ep.clone()
    }

    pub fn local_endpoint(&self) -> Endpoint {
        let tunnel = self.tunnel.lock().unwrap();
        tunnel.data_socket.as_ref().unwrap().local().clone()
    }

    pub async fn close(&mut self) -> P2pResult<()> {
        let conn_timeout = {
            self.tunnel.lock().unwrap().conn_timeout
        };
        match runtime::timeout(conn_timeout, self.close_inner()).await {
            Ok(Ok(())) => {
                Ok(())
            }
            Ok(Err(err)) => {
                Err(err)
            }
            Err(err) => {
                Err(p2p_err!(P2pErrorCode::Timeout, "{}", err))
            }
        }
    }
}

impl Drop for TunnelStream {
    fn drop(&mut self) {
        {
            let tunnel = self.tunnel.lock().unwrap();
            log::info!("drop tcp tunnel stream {:?} local_id {} remote_id {}",
                tunnel.sequence, tunnel.local_identity.get_id().to_string(), tunnel.remote_id.to_string());
        }
        {
            let tunnel = self.tunnel.lock().unwrap();
            tunnel.tunnel_stat.decrease_work_instance();
        }
        let _ = Executor::block_on(self.close());
    }
}
impl runtime::AsyncRead for TunnelStream {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        unsafe {
            if self.read_future.is_none() {
                let this: &'static mut Self = std::mem::transmute(self.as_mut().deref_mut());
                let buf: &'static mut [u8] = std::mem::transmute(buf.initialize_unfilled());
                let future = this.recv(buf);
                self.as_mut().read_future = Some(Box::pin(future));
            }

            match Pin::new(self.as_mut().read_future.as_mut().unwrap()).poll(cx) {
                Poll::Ready(Ok(n)) => {
                    self.as_mut().read_future = None;
                    buf.advance(n);
                    Poll::Ready(Ok(()))
                }
                Poll::Ready(Err(e)) => {
                    self.as_mut().read_future = None;
                    Poll::Ready(Err(Error::new(ErrorKind::Other, e)))
                }
                Poll::Pending => {
                    Poll::Pending
                }
            }
        }
    }
}

impl runtime::AsyncWrite for TunnelStream {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<Result<usize, Error>> {
        unsafe {
            if self.write_future.is_none() {
                let this: &'static mut Self = std::mem::transmute(self.as_mut().deref_mut());
                let buf: &'static [u8] = std::mem::transmute(buf);
                let future = this.send(buf);
                self.as_mut().write_future = Some(Box::pin(future));
            }

            match Pin::new(self.as_mut().write_future.as_mut().unwrap()).poll(cx) {
                Poll::Ready(ret) => {
                    self.as_mut().write_future = None;
                    if let Err(e) = ret {
                        Poll::Ready(Err(std::io::Error::new(std::io::ErrorKind::Other, e)))
                    } else {
                        Poll::Ready(Ok(buf.len()))
                    }
                }
                Poll::Pending => {
                    Poll::Pending
                }
            }
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        Pin::new(self.write.as_mut().unwrap()).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        Pin::new(self.write.as_mut().unwrap()).poll_shutdown(cx)
    }
}

pub struct TunnelDatagramSend {
    tunnel: Arc<Mutex<TunnelConnectionInner>>,
    read: Option<Box<dyn AsyncRead + Send + Sync + 'static + Unpin>>,
    write: Option<Box<dyn AsyncWrite + Send + Sync + 'static + Unpin>>,
    write_future: Option<Pin<Box<dyn Send + Future<Output=std::io::Result<usize>>>>>,
}

impl TunnelDatagramSend {
    pub(crate) fn new(tunnel: Arc<Mutex<TunnelConnectionInner>>, read: Box<dyn AsyncRead + Send + Sync + 'static + Unpin>, write: Box<dyn AsyncWrite + Send + Sync + 'static + Unpin>) -> Self {
        {
            let tunnel = tunnel.lock().unwrap();
            tunnel.tunnel_stat.increase_work_instance();
        }
        Self {
            tunnel,
            read: Some(read),
            write: Some(write),
            write_future: None,
        }
    }

    async fn handle_cmd(&mut self, cmd_code: PackageCmdCode, cmd_body: &[u8]) -> P2pResult<()> {
        if cmd_code == PackageCmdCode::SynClose {
            let syn_close = SynClose::clone_from_slice(cmd_body).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
            let ack = AckClose {
                sequence: syn_close.sequence,
            };
            let pkg = Package::new(PackageCmdCode::AckClose, ack);
            self.write.as_mut().unwrap().write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
            let mut tunnel = self.tunnel.lock().unwrap();
            tunnel.data_socket.as_mut().unwrap().unsplit(self.read.take().unwrap(), self.write.take().unwrap());
            tunnel.enter_idle_mode()?;
            Ok(())
        } else {
            let tunnel = self.tunnel.lock().unwrap();
            Err(p2p_err!(P2pErrorCode::ErrorState, "tunnel {:?} invalid cmd code {:?}", tunnel.sequence, cmd_code))
        }
    }

    async fn send_inner(&mut self, data: &[u8]) -> std::io::Result<usize> {
        let data_len = data.len();
        let mut remainder = data;
        while remainder.len() > 0 {
            let chunk = if remainder.len() > MTU_LARGE as usize {
                &remainder[..MTU_LARGE as usize]
            } else {
                remainder
            };
            let header = PackageHeader::new(PackageCmdCode::PieceData, chunk.len() as u16);
            self.write.as_mut().unwrap().write_all(header.to_vec().unwrap().as_slice()).await?;
            self.write.as_mut().unwrap().write_all(chunk).await?;
            remainder = &remainder[chunk.len()..];
        }
        Ok(data_len)
    }

    async fn send(&mut self, data: &[u8]) -> std::io::Result<usize> {
        match self.send_inner(data).await {
            Ok(len) => {
                Ok(len)
            }
            Err(err) => {
                let mut tunnel = self.tunnel.lock().unwrap();
                tunnel.tunnel_state = TunnelState::Error;
                Err(err)
            }
        }
    }

    async fn read_pkg(&mut self) -> P2pResult<(PackageCmdCode, Vec<u8>)> {
        let mut buf_header = [0u8; 16];
        self.read.as_mut().unwrap().read_exact(&mut buf_header[0..PackageHeader::raw_bytes().unwrap()]).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
        let header = PackageHeader::clone_from_slice(buf_header.as_slice()).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
        let cmd_code = match header.cmd_code() {
            Ok(cmd_code) => cmd_code,
            Err(err) => {
                return Err(err);
            }
        };
        let mut cmd_body = vec![0u8; header.pkg_len() as usize];
        self.read.as_mut().unwrap().read_exact(cmd_body.as_mut_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
        Ok((cmd_code, cmd_body))
    }

    async fn close_inner(&mut self) -> P2pResult<()> {
        let sequence = {
            self.tunnel.lock().unwrap().sequence
        };
        let pkg = Package::new(PackageCmdCode::SynClose, SynClose {
            sequence: sequence,
        });

        self.write.as_mut().unwrap().write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;

        loop {
            let (cmd_code, cmd_body) = self.read_pkg().await?;
            if cmd_code == PackageCmdCode::AckClose {
                let close = AckClose::clone_from_slice(cmd_body.as_slice()).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
                if close.sequence == sequence {
                    break;
                }
            } else if cmd_code == PackageCmdCode::SynClose {
                let close = SynClose::clone_from_slice(cmd_body.as_slice()).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
                if close.sequence != sequence {
                    continue;
                }
                let ack = AckClose {
                    sequence: close.sequence,
                };
                let pkg = Package::new(PackageCmdCode::AckClose, ack);
                self.write.as_mut().unwrap().write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
            }
        }

        {
            let mut tunnel = self.tunnel.lock().unwrap();
            tunnel.data_socket.as_mut().unwrap().unsplit(self.read.take().unwrap(), self.write.take().unwrap());
            tunnel.enter_idle_mode()?;
        }

        Ok(())
    }

}

#[async_trait::async_trait]
impl AsyncWrite for TunnelDatagramSend {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<Result<usize, Error>> {
        unsafe {
            if self.write_future.is_none() {
                let this: &'static mut Self = std::mem::transmute(self.as_mut().deref_mut());
                let buf: &'static [u8] = std::mem::transmute(buf);
                let future = this.send(buf);
                self.as_mut().write_future = Some(Box::pin(future));
            }

            match Pin::new(self.as_mut().write_future.as_mut().unwrap()).poll(cx) {
                Poll::Ready(ret) => {
                    self.write_future = None;
                    Poll::Ready(ret.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e)))
                }
                Poll::Pending => {
                    Poll::Pending
                }
            }
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        Pin::new(self.write.as_mut().unwrap()).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        Pin::new(self.write.as_mut().unwrap()).poll_shutdown(cx)
    }
}

impl TunnelDatagramSend {
    pub fn sequence(&self) -> TempSeq {
        let tunnel = self.tunnel.lock().unwrap();
        tunnel.sequence
    }

    pub fn remote_identity_id(&self) -> P2pId {
        let tunnel = self.tunnel.lock().unwrap();
        tunnel.remote_id.clone()
    }

    pub fn local_identity_id(&self) -> P2pId {
        let tunnel = self.tunnel.lock().unwrap();
        tunnel.local_identity.get_id()
    }

    pub fn remote_endpoint(&self) -> Endpoint {
        let tunnel = self.tunnel.lock().unwrap();
        tunnel.remote_ep.clone()
    }

    pub fn local_endpoint(&self) -> Endpoint {
        let tunnel = self.tunnel.lock().unwrap();
        tunnel.data_socket.as_ref().unwrap().local().clone()
    }

    pub async fn close(&mut self) -> P2pResult<()> {
        let conn_timeout = {
            self.tunnel.lock().unwrap().conn_timeout
        };
        match runtime::timeout(conn_timeout, self.close_inner()).await {
            Ok(Ok(())) => {
                Ok(())
            }
            Ok(Err(err)) => {
                Err(err)
            }
            Err(err) => {
                Err(p2p_err!(P2pErrorCode::Timeout, "{}", err))
            }
        }
    }
}

impl Drop for TunnelDatagramSend {
    fn drop(&mut self) {
        {
            let tunnel = self.tunnel.lock().unwrap();
            log::info!("drop tcp tunnel datagram {:?} local_id {} remote_id {}",
                tunnel.sequence, tunnel.local_identity.get_id().to_string(), tunnel.remote_id.to_string());
        }
        {
            let tunnel = self.tunnel.lock().unwrap();
            tunnel.tunnel_stat.decrease_work_instance();
        }
        let _ = Executor::block_on(self.close());
    }
}

pub struct TunnelDatagramRecv {
    tunnel: Arc<Mutex<TunnelConnectionInner>>,
    read: Option<Box<dyn AsyncRead + Send + Sync + 'static + Unpin>>,
    write: Option<Box<dyn AsyncWrite + Send + Sync + 'static + Unpin>>,
    remainder: u16,
    read_future: Option<Pin<Box<dyn Send + Future<Output=std::io::Result<usize>>>>>,
}

impl TunnelDatagramRecv {
    pub(crate) fn new(tunnel: Arc<Mutex<TunnelConnectionInner>>, read: Box<dyn AsyncRead + Send + Sync + 'static + Unpin>, write: Box<dyn AsyncWrite + Send + Sync + 'static + Unpin>) -> Self {
        {
            let tunnel = tunnel.lock().unwrap();
            tunnel.tunnel_stat.increase_work_instance();
        }
        Self {
            tunnel,
            read: Some(read),
            write: Some(write),
            remainder: 0,
            read_future: None,
        }
    }

    async fn handle_cmd(&mut self, cmd_code: PackageCmdCode, cmd_body: &[u8]) -> P2pResult<()> {
        if cmd_code == PackageCmdCode::SynClose {
            let syn_close = SynClose::clone_from_slice(cmd_body).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
            let ack = AckClose {
                sequence: syn_close.sequence,
            };
            let pkg = Package::new(PackageCmdCode::AckClose, ack);
            self.write.as_mut().unwrap().write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
            let mut tunnel = self.tunnel.lock().unwrap();
            tunnel.data_socket.as_mut().unwrap().unsplit(self.read.take().unwrap(), self.write.take().unwrap());
            tunnel.enter_idle_mode()?;
            Ok(())
        } else {
            let tunnel = self.tunnel.lock().unwrap();
            Err(p2p_err!(P2pErrorCode::ErrorState, "tunnel {:?} invalid cmd code {:?}", tunnel.sequence, cmd_code))
        }
    }

    async fn recv_inner(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.remainder == 0 {
            let mut buf_header = [0u8; 16];
            self.read.as_mut().unwrap().read_exact(&mut buf_header[0..PackageHeader::raw_bytes().unwrap()]).await?;
            let header = PackageHeader::clone_from_slice(buf_header.as_slice()).map_err(|e| std::io::Error::new(ErrorKind::Other, e))?;
            let cmd_code = match header.cmd_code() {
                Ok(cmd_code) => cmd_code,
                Err(err) => {
                    return Err(Error::new(ErrorKind::Other, err));
                }
            };
            if cmd_code != PackageCmdCode::PieceData {
                let mut cmd_body = vec![0u8; header.pkg_len() as usize];
                self.read.as_mut().unwrap().read_exact(cmd_body.as_mut_slice()).await?;
                self.handle_cmd(cmd_code, cmd_body.as_slice()).await.map_err(|e| Error::new(ErrorKind::Other, e))?;
                return Ok(0);
            } else {
                self.remainder = header.pkg_len();
            }
        }
        if self.remainder as usize > buf.len() {
            let recv_len = self.read.as_mut().unwrap().read(buf).await?;
            self.remainder -= recv_len as u16;
            Ok(recv_len)
        } else {
            let recv_len = self.remainder;
            self.read.as_mut().unwrap().read_exact(&mut buf[..self.remainder as usize]).await?;
            self.remainder = 0;
            Ok(recv_len as usize)
        }
    }

    async fn recv(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self.recv_inner(buf).await {
            Ok(size) => {
                Ok(size)
            }
            Err(err) => {
                let mut tunnel = self.tunnel.lock().unwrap();
                tunnel.tunnel_state = TunnelState::Error;
                Err(err)
            }
        }
    }
    async fn read_pkg(&mut self) -> P2pResult<(PackageCmdCode, Vec<u8>)> {
        let mut buf_header = [0u8; 16];
        self.read.as_mut().unwrap().read_exact(&mut buf_header[0..PackageHeader::raw_bytes().unwrap()]).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
        let header = PackageHeader::clone_from_slice(buf_header.as_slice()).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
        let cmd_code = match header.cmd_code() {
            Ok(cmd_code) => cmd_code,
            Err(err) => {
                return Err(err);
            }
        };
        let mut cmd_body = vec![0u8; header.pkg_len() as usize];
        self.read.as_mut().unwrap().read_exact(cmd_body.as_mut_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
        Ok((cmd_code, cmd_body))
    }

    async fn close_inner(&mut self) -> P2pResult<()> {
        let sequence = {
            self.tunnel.lock().unwrap().sequence
        };
        let pkg = Package::new(PackageCmdCode::SynClose, SynClose {
            sequence: sequence,
        });

        self.write.as_mut().unwrap().write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;

        loop {
            let (cmd_code, cmd_body) = self.read_pkg().await?;
            if cmd_code == PackageCmdCode::AckClose {
                let close = AckClose::clone_from_slice(cmd_body.as_slice()).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
                if close.sequence == sequence {
                    break;
                }
            } else if cmd_code == PackageCmdCode::SynClose {
                let close = SynClose::clone_from_slice(cmd_body.as_slice()).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
                if close.sequence != sequence {
                    continue;
                }
                let ack = AckClose {
                    sequence: close.sequence,
                };
                let pkg = Package::new(PackageCmdCode::AckClose, ack);
                self.write.as_mut().unwrap().write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
            }
        }

        {
            let mut tunnel = self.tunnel.lock().unwrap();
            tunnel.data_socket.as_mut().unwrap().unsplit(self.read.take().unwrap(), self.write.take().unwrap());
            tunnel.enter_idle_mode()?;
        }

        Ok(())
    }

}

impl AsyncRead for TunnelDatagramRecv {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        unsafe {
            if self.read_future.is_none() {
                let this: &'static mut Self = std::mem::transmute(self.as_mut().deref_mut());
                let buf: &'static mut [u8] = std::mem::transmute(buf.initialize_unfilled());
                let future = this.recv(buf);
                self.as_mut().read_future = Some(Box::pin(future));
            }

            match Pin::new(self.as_mut().read_future.as_mut().unwrap()).poll(cx) {
                Poll::Ready(Ok(n)) => {
                    self.as_mut().read_future = None;
                    buf.advance(n);
                    Poll::Ready(Ok(()))
                }
                Poll::Ready(Err(e)) => {
                    self.as_mut().read_future = None;
                    Poll::Ready(Err(e))
                }
                Poll::Pending => {
                    Poll::Pending
                }
            }
        }
    }
}

impl TunnelDatagramRecv {
    pub(crate) fn sequence(&self) -> TempSeq {
        let tunnel = self.tunnel.lock().unwrap();
        tunnel.sequence
    }

    pub(crate) fn remote_identity_id(&self) -> P2pId {
        let tunnel = self.tunnel.lock().unwrap();
        tunnel.remote_id.clone()
    }

    pub(crate) fn local_identity_id(&self) -> P2pId {
        let tunnel = self.tunnel.lock().unwrap();
        tunnel.local_identity.get_id()
    }

    pub(crate) fn remote_endpoint(&self) -> Endpoint {
        let tunnel = self.tunnel.lock().unwrap();
        tunnel.remote_ep.clone()
    }

    pub(crate) fn local_endpoint(&self) -> Endpoint {
        let tunnel = self.tunnel.lock().unwrap();
        tunnel.data_socket.as_ref().unwrap().local().clone()
    }

    async fn close(&mut self) -> P2pResult<()> {
        let conn_timeout = {
            self.tunnel.lock().unwrap().conn_timeout
        };
        match runtime::timeout(conn_timeout, self.close_inner()).await {
            Ok(Ok(())) => {
                Ok(())
            }
            Ok(Err(err)) => {
                Err(err)
            }
            Err(err) => {
                Err(p2p_err!(P2pErrorCode::Timeout, "{}", err))
            }
        }
    }
}

impl Drop for TunnelDatagramRecv {
    fn drop(&mut self) {
        {
            let tunnel = self.tunnel.lock().unwrap();
            log::info!("drop tcp tunnel datagram {:?} local_id {} remote_id {}",
                tunnel.sequence, tunnel.local_identity.get_id().to_string(), tunnel.remote_id.to_string());
        }
        {
            let tunnel = self.tunnel.lock().unwrap();
            tunnel.tunnel_stat.decrease_work_instance();
        }
        let _ = Executor::block_on(self.close());
    }
}

pub(crate) struct TunnelConnectionInner {
    sequence: TempSeq,
    local_identity: P2pIdentityRef,
    remote_id: P2pId,
    remote_ep: Endpoint,
    conn_timeout: Duration,
    protocol_version: u8,
    stack_version: u32,
    data_socket: Option<P2pConnectionRef>,
    tunnel_state: TunnelState,
    accept_handle: Option<SpawnHandle<()>>,
    write: Option<Box<dyn runtime::AsyncWrite + Send + Sync + 'static + Unpin>>,
    accept_future: Arc<Mutex<Option<NotifyFuture<P2pResult<(Box<dyn runtime::AsyncRead + Send + Sync + 'static + Unpin>, PackageCmdCode, Vec<u8>)>>>>>,
    recv_future: Arc<Mutex<Option<NotifyFuture<P2pResult<(Box<dyn runtime::AsyncRead + Send + Sync + 'static + Unpin>, PackageCmdCode, Vec<u8>)>>>>>,
    cert_factory: P2pIdentityCertFactoryRef,
    lister_ports: TunnelListenPortsRef,
    tunnel_stat: TunnelStatRef,
}

impl TunnelConnectionInner {
    pub(crate) fn new(
        sequence: TempSeq,
        local_identity: P2pIdentityRef,
        remote_id: P2pId,
        remote_ep: Endpoint,
        conn_timeout: Duration,
        protocol_version: u8,
        stack_version: u32,
        data_socket: Option<P2pConnectionRef>,
        lister_ports: TunnelListenPortsRef,
        cert_factory: P2pIdentityCertFactoryRef,) -> P2pResult<Self> {
        let mut obj = Self {
            sequence,
            local_identity,
            remote_id,
            remote_ep,
            conn_timeout,
            protocol_version,
            stack_version,
            data_socket,
            tunnel_state: TunnelState::Init,
            lister_ports,
            accept_handle: None,
            write: None,
            accept_future: Arc::new(Mutex::new(None)),
            recv_future: Arc::new(Mutex::new(None)),
            tunnel_stat: TunnelStat::new(),
            cert_factory,
        };
        obj.enter_idle_mode()?;
        Ok(obj)
    }

    fn enter_idle_mode(&mut self) -> P2pResult<()> {
        if self.data_socket.is_some() {
            let (read, write) = self.data_socket.as_ref().unwrap().split()?;
            let handle = create_accept_handle(read, self.recv_future.clone(), self.accept_future.clone())?;
            self.write = Some(write);
            self.accept_handle = Some(handle);
            self.tunnel_state = TunnelState::Idle;
        }
        Ok(())
    }

    async fn open_stream_inner(sequence: TempSeq,
                               write: &mut Box<dyn runtime::AsyncWrite + Send + Sync + 'static + Unpin>,
                               recv_future: NotifyFuture<P2pResult<(Box<dyn runtime::AsyncRead + Send + Sync + 'static + Unpin>, PackageCmdCode, Vec<u8>)>>,
                               vport: u16,
                               session_id: IncreaseId) -> P2pResult<Box<dyn runtime::AsyncRead + Send + Sync + 'static + Unpin>> {
        let syn = SynStream {
            sequence,
            to_vport: vport,
            session_id,
            payload: Vec::new(),
        };
        let pkg = Package::new(PackageCmdCode::SynStream, syn);

        write.write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
        let (read, cmd_code, cmd_body) = recv_future.await?;

        if cmd_code != PackageCmdCode::AckStream {
            return Err(p2p_err!(P2pErrorCode::InvalidData, "tunnel {:?} invalid ack stream", sequence));
        }

        let ack = AckStream::clone_from_slice(cmd_body.as_slice()).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
        if ack.result != 0 {
            return Err(p2p_err!(P2pErrorCode::ConnectionRefused, "tunnel {:?} open stream failed. return {}", sequence, ack.result));
        }

        Ok(read)
    }

    async fn open_reverse_stream_inner(sequence: TempSeq,
                                       write: &mut Box<dyn runtime::AsyncWrite + Send + Sync + 'static + Unpin>,
                                       recv_future: NotifyFuture<P2pResult<(Box<dyn runtime::AsyncRead + Send + Sync + 'static + Unpin>, PackageCmdCode, Vec<u8>)>>,
                                       vport: u16,
                                       session_id: IncreaseId) -> P2pResult<Box<dyn runtime::AsyncRead + Send + Sync + 'static + Unpin>> {
        let syn = SynReverseStream {
            sequence,
            session_id,
            vport,
            payload: Vec::new(),
        };
        let pkg = Package::new(PackageCmdCode::SynReverseStream, syn);

        write.write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
        let (read, cmd_code, cmd_body) = recv_future.await?;

        if cmd_code != PackageCmdCode::AckReverseStream {
            return Err(p2p_err!(P2pErrorCode::InvalidData, "tunnel {:?} invalid ack stream", sequence));
        }

        let ack = AckReverseStream::clone_from_slice(cmd_body.as_slice()).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
        if ack.result != 0 {
            return Err(p2p_err!(P2pErrorCode::ConnectionRefused, "tunnel {:?} open stream failed. return {}", sequence, ack.result));
        }

        Ok(read)
    }

    async fn open_datagram_inner(sequence: TempSeq,
                                 write: &mut Box<dyn runtime::AsyncWrite + Send + Sync + 'static + Unpin>,
                                 recv_future: NotifyFuture<P2pResult<(Box<dyn runtime::AsyncRead + Send + Sync + 'static + Unpin>, PackageCmdCode, Vec<u8>)>>,) -> P2pResult<Box<dyn runtime::AsyncRead + Send + Sync + 'static + Unpin>> {
        let syn = SynDatagram {
            sequence: sequence,
            payload: Vec::new(),
        };
        let pkg = Package::new(PackageCmdCode::SynDatagram, syn);

        write.write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
        let (read, cmd_code, cmd_body) = recv_future.await?;

        if cmd_code != PackageCmdCode::AckDatagram {
            return Err(p2p_err!(P2pErrorCode::InvalidData, "tunnel {:?} invalid ack stream", sequence));
        }

        let ack = AckDatagram::clone_from_slice(cmd_body.as_slice()).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
        if ack.result != 0 {
            return Err(p2p_err!(P2pErrorCode::ConnectionRefused, "tunnel {:?} open stream failed. return {}", sequence, ack.result));
        }

        Ok(read)
    }

    async fn open_reverse_datagram_inner(sequence: TempSeq,
                                         write: &mut Box<dyn runtime::AsyncWrite + Send + Sync + 'static + Unpin>,
                                         recv_future: NotifyFuture<P2pResult<(Box<dyn runtime::AsyncRead + Send + Sync + 'static + Unpin>, PackageCmdCode, Vec<u8>)>>,) -> P2pResult<Box<dyn runtime::AsyncRead + Send + Sync + 'static + Unpin>> {
        let syn = SynReverseDatagram {
            sequence,
            payload: Vec::new(),
        };
        let pkg = Package::new(PackageCmdCode::SynReverseDatagram, syn);

        write.write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
        let (read, cmd_code, cmd_body) = recv_future.await?;

        if cmd_code != PackageCmdCode::AckReverseDatagram {
            return Err(p2p_err!(P2pErrorCode::InvalidData, "tunnel {:?} invalid ack stream", sequence));
        }

        let ack = AckReverseDatagram::clone_from_slice(cmd_body.as_slice()).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
        if ack.result != 0 {
            return Err(p2p_err!(P2pErrorCode::ConnectionRefused, "tunnel {:?} open stream failed. return {}", sequence, ack.result));
        }

        Ok(read)
    }
}

pub struct TunnelConnection {
    inner: Arc<Mutex<TunnelConnectionInner>>
}

impl TunnelConnection {
    pub fn new(
        sequence: TempSeq,
        local_identity: P2pIdentityRef,
        remote_id: P2pId,
        remote_ep: Endpoint,
        conn_timeout: Duration,
        protocol_version: u8,
        stack_version: u32,
        tcp_socket: Option<P2pConnectionRef>,
        lister_ports: TunnelListenPortsRef,
        cert_factory: P2pIdentityCertFactoryRef, ) -> P2pResult<Self> {
        Ok(Self {
            inner: Arc::new(Mutex::new(TunnelConnectionInner::new(
                sequence,
                local_identity,
                remote_id,
                remote_ep,
                conn_timeout,
                protocol_version,
                stack_version,
                tcp_socket,
                lister_ports,
                cert_factory)?))
        })
    }

    pub fn get_sequence(&self) -> TempSeq {
        let inner = self.inner.lock().unwrap();
        inner.sequence
    }

    async fn open_reverse_stream(&self, vport: u16, session_id: IncreaseId) -> P2pResult<TunnelStream> {
        let (conn_timeout, sequence, mut write, recv_future) = {
            let mut inner = self.inner.lock().unwrap();
            if inner.tunnel_state != TunnelState::Idle {
                return Err(p2p_err!(P2pErrorCode::ErrorState, "tunnel {:?} invalid state {:?}", inner.sequence, inner.tunnel_state));
            }
            inner.tunnel_state = TunnelState::Opening;

            let future = NotifyFuture::<P2pResult<(Box<dyn runtime::AsyncRead + Send + Sync + 'static + Unpin>, PackageCmdCode, Vec<u8>)>>::new();
            {
                let mut recv_future = inner.recv_future.lock().unwrap();
                *recv_future = Some(future.clone());
            }

            (inner.conn_timeout, inner.sequence, inner.write.take().unwrap(), future)
        };
        match runtime::timeout(conn_timeout, TunnelConnectionInner::open_reverse_stream_inner(sequence, &mut write, recv_future, vport, session_id.clone())).await {
            Ok(Ok(read)) => {
                let mut inner = self.inner.lock().unwrap();
                inner.tunnel_state = TunnelState::Worked;
                Ok(TunnelStream::new(self.inner.clone(), read, write, session_id, vport))
            }
            Ok(Err(err)) => {
                let mut inner = self.inner.lock().unwrap();
                inner.tunnel_state = TunnelState::Error;
                Err(err)
            }
            Err(err) => {
                let mut inner = self.inner.lock().unwrap();
                inner.tunnel_state = TunnelState::Error;
                Err(p2p_err!(P2pErrorCode::Timeout, "{}", err))
            }
        }
    }

    async fn open_reverse_datagram(&self) -> P2pResult<TunnelDatagramRecv> {
        let (conn_timeout, sequence, mut write, recv_future) = {
            let mut inner = self.inner.lock().unwrap();
            if inner.tunnel_state != TunnelState::Idle {
                return Err(p2p_err!(P2pErrorCode::ErrorState, "tunnel {:?} invalid state {:?}", inner.sequence, inner.tunnel_state));
            }
            inner.tunnel_state = TunnelState::Opening;
            let future = NotifyFuture::<P2pResult<(Box<dyn runtime::AsyncRead + Send + Sync + 'static + Unpin>, PackageCmdCode, Vec<u8>)>>::new();
            {
                let mut recv_future = inner.recv_future.lock().unwrap();
                *recv_future = Some(future.clone());
            }
            (inner.conn_timeout, inner.sequence, inner.write.take().unwrap(), future)
        };
        match runtime::timeout(conn_timeout, TunnelConnectionInner::open_reverse_datagram_inner(sequence, &mut write, recv_future)).await {
            Ok(Ok(read)) => {
                let mut inner = self.inner.lock().unwrap();
                inner.tunnel_state = TunnelState::Worked;
                Ok(TunnelDatagramRecv::new(self.inner.clone(), read, write))
            }
            Ok(Err(err)) => {
                let mut inner = self.inner.lock().unwrap();
                inner.tunnel_state = TunnelState::Error;
                Err(err)
            }
            Err(err) => {
                let mut inner = self.inner.lock().unwrap();
                inner.tunnel_state = TunnelState::Error;
                Err(p2p_err!(P2pErrorCode::Timeout, "{}", err))
            }
        }
    }

    pub(crate) async fn connect_stream(&self, vport: u16, session_id: IncreaseId) -> P2pResult<TunnelStream> {
        let (has_socket, local_identity, remote_ep, remote_id, conn_timeout, verifier) = {
            let inner = self.inner.lock().unwrap();
            (inner.data_socket.is_some(), inner.local_identity.clone(), inner.remote_ep.clone(), inner.remote_id.clone(), inner.conn_timeout, inner.cert_factory.clone())
        };
        if has_socket {
            return Ok(self.open_stream(vport, session_id).await?);
        }
        let tcp_socket = TCPConnection::connect(verifier, local_identity, remote_ep, remote_id, conn_timeout).await?;

        {
            let mut inner = self.inner.lock().unwrap();
            inner.data_socket = Some(Arc::new(tcp_socket));
            inner.enter_idle_mode()?;
        }

        Ok(self.open_stream(vport, session_id).await?)
    }

    pub(crate) async fn connect_datagram(&self) -> P2pResult<TunnelDatagramSend> {
        let (has_socket, local_identity, remote_ep, remote_id, conn_timeout, verifier) = {
            let inner = self.inner.lock().unwrap();
            (inner.data_socket.is_some(), inner.local_identity.clone(), inner.remote_ep.clone(), inner.remote_id.clone(), inner.conn_timeout, inner.cert_factory.clone())
        };

        if has_socket {
            return Ok(self.open_datagram().await?);
        }

        let tcp_socket = TCPConnection::connect(verifier, local_identity, remote_ep, remote_id, conn_timeout).await?;

        {
            let mut inner = self.inner.lock().unwrap();
            inner.data_socket = Some(Arc::new(tcp_socket));
            inner.enter_idle_mode()?;
        }

        Ok(self.open_datagram().await?)
    }

    pub(crate) async fn connect_reverse_stream(&self, vport: u16, session_id: IncreaseId) -> P2pResult<TunnelStream> {
        let (has_socket, local_identity, remote_ep, remote_id, conn_timeout, verifier) = {
            let inner = self.inner.lock().unwrap();
            (inner.data_socket.is_some(), inner.local_identity.clone(), inner.remote_ep.clone(), inner.remote_id.clone(), inner.conn_timeout, inner.cert_factory.clone())
        };

        if has_socket {
            return Ok(self.open_reverse_stream(vport, session_id).await?);
        }

        let tcp_socket = TCPConnection::connect(verifier, local_identity, remote_ep, remote_id, conn_timeout).await?;

        {
            let mut inner = self.inner.lock().unwrap();
            inner.data_socket = Some(Arc::new(tcp_socket));
            inner.enter_idle_mode()?;
        }

        Ok(self.open_reverse_stream(vport, session_id).await?)
    }

    pub(crate) async fn connect_reverse_datagram(&self) -> P2pResult<TunnelDatagramRecv> {
        let (has_socket, local_identity, remote_ep, remote_id, conn_timeout, verifier) = {
            let inner = self.inner.lock().unwrap();
            (inner.data_socket.is_some(), inner.local_identity.clone(), inner.remote_ep.clone(), inner.remote_id.clone(), inner.conn_timeout, inner.cert_factory.clone())
        };

        if has_socket {
            return Ok(self.open_reverse_datagram().await?);
        }

        let tcp_socket = TCPConnection::connect(verifier, local_identity, remote_ep, remote_id, conn_timeout).await?;

        {
            let mut inner = self.inner.lock().unwrap();
            inner.data_socket = Some(Arc::new(tcp_socket));
            inner.enter_idle_mode()?;
        }

        Ok(self.open_reverse_datagram().await?)
    }

    pub(crate) async fn open_stream(&self, vport: u16, session_id: IncreaseId) -> P2pResult<TunnelStream> {
        let (conn_timeout, sequence, mut write, recv_future) = {
            let mut inner = self.inner.lock().unwrap();
            if inner.tunnel_state != TunnelState::Idle {
                return Err(p2p_err!(P2pErrorCode::ErrorState, "tunnel {:?} invalid state {:?}", inner.sequence, inner.tunnel_state));
            }
            inner.tunnel_state = TunnelState::Opening;

            let future = NotifyFuture::<P2pResult<(Box<dyn AsyncRead + Send + Sync + 'static + Unpin>, PackageCmdCode, Vec<u8>)>>::new();
            {
                let mut recv_future = inner.recv_future.lock().unwrap();
                *recv_future = Some(future.clone());
            }

            (inner.conn_timeout, inner.sequence, inner.write.take().unwrap(), future)
        };
        match runtime::timeout(conn_timeout, TunnelConnectionInner::open_stream_inner(sequence, &mut write, recv_future, vport, session_id.clone())).await {
            Ok(Ok(read)) => {
                let mut inner = self.inner.lock().unwrap();
                inner.tunnel_state = TunnelState::Worked;
                Ok(TunnelStream::new(self.inner.clone(), read, write, session_id, vport))
            }
            Ok(Err(err)) => {
                let mut inner = self.inner.lock().unwrap();
                inner.tunnel_state = TunnelState::Error;
                Err(err)
            }
            Err(err) => {
                let mut inner = self.inner.lock().unwrap();
                inner.tunnel_state = TunnelState::Error;
                Err(p2p_err!(P2pErrorCode::Timeout, "{}", err))
            }
        }
    }

    pub(crate) async fn open_datagram(&self) -> P2pResult<TunnelDatagramSend> {
        let (conn_timeout, sequence, mut write, recv_future) = {
            let mut inner = self.inner.lock().unwrap();
            if inner.tunnel_state != TunnelState::Idle {
                return Err(p2p_err!(P2pErrorCode::ErrorState, "tunnel {:?} invalid state {:?}", inner.sequence, inner.tunnel_state));
            }
            inner.tunnel_state = TunnelState::Opening;
            let future = NotifyFuture::<P2pResult<(Box<dyn AsyncRead + Send + Sync + 'static + Unpin>, PackageCmdCode, Vec<u8>)>>::new();
            {
                let mut recv_future = inner.recv_future.lock().unwrap();
                *recv_future = Some(future.clone());
            }
            (inner.conn_timeout, inner.sequence, inner.write.take().unwrap(), future)
        };
        match runtime::timeout(conn_timeout, TunnelConnectionInner::open_datagram_inner(sequence, &mut write, recv_future)).await {
            Ok(Ok(read)) => {
                let mut inner = self.inner.lock().unwrap();
                inner.tunnel_state = TunnelState::Worked;
                Ok(TunnelDatagramSend::new(self.inner.clone(), read, write))
            }
            Ok(Err(err)) => {
                let mut inner = self.inner.lock().unwrap();
                inner.tunnel_state = TunnelState::Error;
                Err(err)
            }
            Err(err) => {
                let mut inner = self.inner.lock().unwrap();
                inner.tunnel_state = TunnelState::Error;
                Err(p2p_err!(P2pErrorCode::Timeout, "{}", err))
            }
        }
    }

    pub async fn accept_instance(&self) -> P2pResult<TunnelInstance> {
        loop {
            let future = NotifyFuture::new();
            {
                let inner = self.inner.lock().unwrap();
                let mut accept_future = inner.accept_future.lock().unwrap();
                *accept_future = Some(future.clone());
            };

            let (read, cmd_code, cmd_body) = future.await?;
            match cmd_code {
                PackageCmdCode::SynStream => {
                    let syn_stream = SynStream::clone_from_slice(cmd_body.as_slice()).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
                    let vport = syn_stream.to_vport;
                    let session_id = syn_stream.session_id;
                    let mut write = {
                        let mut inner = self.inner.lock().unwrap();
                        if inner.tunnel_state != TunnelState::Idle {
                            continue;
                        }
                        inner.tunnel_state = TunnelState::Accepting;
                        inner.sequence = syn_stream.sequence;
                        let write = inner.write.take().unwrap();
                        write
                    };
                    let ack = AckStream {
                        result: 0,
                    };
                    let pkg = Package::new(PackageCmdCode::AckStream, ack);
                    write.write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
                    {
                        let mut inner = self.inner.lock().unwrap();
                        inner.tunnel_state = TunnelState::Worked;
                    }
                    return Ok(TunnelInstance::Stream(TunnelStream::new(self.inner.clone(), read, write, session_id, vport)))
                }
                PackageCmdCode::SynReverseStream => {
                    let reserve_stream = SynReverseStream::clone_from_slice(cmd_body.as_slice()).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
                    let mut write = {
                        let mut inner = self.inner.lock().unwrap();
                        if inner.tunnel_state != TunnelState::Idle {
                            continue;
                        }
                        inner.tunnel_state = TunnelState::Accepting;
                        inner.sequence = reserve_stream.sequence;
                        let write = inner.write.take().unwrap();
                        write
                    };
                    let ack = AckReverseStream {
                        result: 0,
                    };
                    let pkg = Package::new(PackageCmdCode::AckReverseStream, ack);
                    write.write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
                    {
                        let mut inner = self.inner.lock().unwrap();
                        inner.tunnel_state = TunnelState::Worked;
                    }
                    return Ok(TunnelInstance::ReverseStream(TunnelStream::new(self.inner.clone(), read, write, reserve_stream.session_id, 0)));
                }
                PackageCmdCode::SynDatagram => {
                    let syn_datagram = SynDatagram::clone_from_slice(cmd_body.as_slice()).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
                    let mut write = {
                        let mut inner = self.inner.lock().unwrap();
                        if inner.tunnel_state != TunnelState::Idle {
                            continue;
                        }
                        inner.tunnel_state = TunnelState::Accepting;
                        inner.sequence = syn_datagram.sequence;
                        let write = inner.write.take().unwrap();
                        write
                    };
                    let ack = AckDatagram {
                        result: 0,
                    };
                    let pkg = Package::new(PackageCmdCode::AckDatagram, ack);
                    write.write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
                    {
                        let mut inner = self.inner.lock().unwrap();
                        inner.tunnel_state = TunnelState::Worked;
                    }
                    return Ok(TunnelInstance::Datagram(TunnelDatagramRecv::new(self.inner.clone(), read, write)))
                }
                PackageCmdCode::SynReverseDatagram => {
                    let reverse_datagram = SynReverseDatagram::clone_from_slice(cmd_body.as_slice()).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
                    let mut write = {
                        let mut inner = self.inner.lock().unwrap();
                        if inner.tunnel_state != TunnelState::Idle {
                            continue;
                        }
                        inner.tunnel_state = TunnelState::Accepting;
                        inner.sequence = reverse_datagram.sequence;
                        let write = inner.write.take().unwrap();
                        write
                    };
                    let ack = AckReverseDatagram {
                        result: 0,
                    };
                    let pkg = Package::new(PackageCmdCode::AckReverseDatagram, ack);
                    write.write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
                    {
                        let mut inner = self.inner.lock().unwrap();
                        inner.tunnel_state = TunnelState::Worked;
                    }
                    return Ok(TunnelInstance::ReverseDatagram(TunnelDatagramSend::new(self.inner.clone(), read, write)));
                }
                _ => {
                    let inner = self.inner.lock().unwrap();
                    return Err(p2p_err!(P2pErrorCode::InvalidData, "tunnel {:?} invalid cmd code {:?}", inner.sequence, cmd_code));
                }
            }
        }
    }

    pub(crate) async fn shutdown(&self) -> P2pResult<()> {
        {
            log::info!("tunnel {:?} shutdown", self.inner.lock().unwrap().sequence);
        }
        let (accept_handle, recv_future, accept_future) = {
            let mut inner = self.inner.lock().unwrap();
            if inner.tunnel_state != TunnelState::Idle {
                return Ok(());
            }
            (inner.accept_handle.take(), inner.recv_future.clone(), inner.accept_future.clone())
        };

        if accept_handle.is_some() {
            accept_handle.unwrap().abort();
        }

        {
            let mut future = recv_future.lock().unwrap();
            if future.is_some() {
                future.take().unwrap().set_complete(Err(p2p_err!(P2pErrorCode::Interrupted, "tunnel {:?} shutdown", self.inner.lock().unwrap().sequence)));
            }
        }
        {
            let mut future = accept_future.lock().unwrap();
            if future.is_some() {
                future.take().unwrap().set_complete(Err(p2p_err!(P2pErrorCode::Interrupted, "tunnel {:?} shutdown", self.inner.lock().unwrap().sequence)));

            }
        }
        // inner.data_socket.as_ref().unwrap().shutdown().await?;
        Ok(())
    }

    pub fn tunnel_stat(&self) -> TunnelStatRef {
        self.inner.lock().unwrap().tunnel_stat.clone()
    }

    pub fn is_idle(&self) -> bool {
        let inner = self.inner.lock().unwrap();
        inner.tunnel_state == TunnelState::Idle
    }
}

impl Drop for TunnelConnection {
    fn drop(&mut self) {
        log::info!("drop tunnel {:?}", self.inner.lock().unwrap().sequence);
        let _ = Executor::block_on(self.shutdown());
    }

}

pub async fn select_successful<T, E, F>(futures: Vec<F>) -> P2pResult<T>
where
    F: Future<Output=Result<T, E>> + Unpin,
{
    let mut futures = futures.into_iter().map(FutureExt::fuse).collect::<Vec<_>>();

    while futures.len() > 0 {
        let select_all = futures::future::select_all(futures);
        match select_all.await {
            (Ok(result), _index, _remaining) => {
                return Ok(result);
            },
            (Err(_), _index, remaining) => {
                futures = remaining;
            },
        }
    };
    Err(p2p_err!(P2pErrorCode::ConnectFailed, "connect failed"))
}
