use std::fmt::{Debug, Display, Formatter};
use std::future::Future;
use std::io::{Error, ErrorKind};
use std::ops::DerefMut;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;
use async_named_locker::ObjectHolder;
use bucky_raw_codec::{RawConvertTo, RawEncode, RawFixedBytes, RawFrom};
use bucky_time::bucky_time_now;
use futures::future::abortable;
use futures::FutureExt;
use notify_future::NotifyFuture;
use num_traits::FromPrimitive;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::time::error::Elapsed;
use crate::endpoint::Endpoint;
use crate::error::{into_p2p_err, p2p_err, P2pErrorCode, P2pResult};
use crate::executor::{Executor, SpawnHandle};
use crate::p2p_connection::{P2pConnectionRef, P2pRead, P2pWrite};
use crate::p2p_identity::{P2pId, P2pIdentityCertFactoryRef, P2pIdentityRef};
use crate::protocol::{Package, PackageCmdCode, PackageHeader, MTU_LARGE};
use crate::protocol::v0::{AckClose, AckDatagram, AckReverseDatagram, AckReverseStream, AckStream, SynClose, SynDatagram, SynReverseDatagram, SynReverseStream, SynStream};
use crate::runtime;
use crate::sockets::tcp::{TCPConnection};
use crate::types::{SessionId, TunnelId};

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

pub enum TunnelSession {
    Stream(TunnelStream),
    Datagram(TunnelDatagramRecv),
    ReverseStream(TunnelStream),
    ReverseDatagram(TunnelDatagramSend),
}

impl Display for TunnelSession {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            TunnelSession::Stream(stream) => write!(f, "Stream(seq {:?} session {} port {})", stream.tunnel_id(), stream.session_id(), stream.port()),
            TunnelSession::Datagram(datagram) => write!(f, "Datagram({:?})", datagram.tunnel_id()),
            TunnelSession::ReverseStream(stream) => write!(f, "ReverseStream(seq {:?} session {} port {})", stream.tunnel_id(), stream.session_id(), stream.port()),
            TunnelSession::ReverseDatagram(datagram) => write!(f, "ReverseDatagram({:?})", datagram.tunnel_id()),
        }
    }
}

async fn accept_pkg(mut read: Box<dyn P2pRead>) -> P2pResult<(Box<dyn P2pRead>, PackageCmdCode, Vec<u8>)> {
    let mut buf_header = [0u8; 16];
    loop {
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
        if cmd_code == PackageCmdCode::SynStream || cmd_code == PackageCmdCode::AckStream
            || cmd_code == PackageCmdCode::SynDatagram || cmd_code == PackageCmdCode::AckDatagram
            || cmd_code == PackageCmdCode::SynReverseStream || cmd_code == PackageCmdCode::AckReverseStream
            || cmd_code == PackageCmdCode::SynReverseDatagram || cmd_code == PackageCmdCode::AckReverseDatagram {
            log::info!("accept first pkg cmd code {:?}", cmd_code);
            break Ok((read, cmd_code, cmd_body));
        } else {
            log::info!("accept error pkg cmd code {:?}, discard", cmd_code);
        }
    }
}

fn create_accept_handle(read: Box<dyn P2pRead>,
                        accept_future: TunnelFutureHolderRef,
                        recv_future: TunnelFutureHolderRef
) -> P2pResult<SpawnHandle<()>> {
    Executor::spawn_with_handle(async move {
        let ret = accept_pkg(read).await;
        if ret.is_ok() {
            {
                if let Some(ret) = recv_future.try_complete(ret) {
                    accept_future.try_complete(ret);
                }
            }
        } else {
            if let Some(ret) = recv_future.try_complete(ret) {
                accept_future.try_complete(ret);
            }
        }
    })
}

type ReadHolder = ObjectHolder<Option<Box<dyn P2pRead>>>;
type WriteHolder = ObjectHolder<Option<Box<dyn P2pWrite>>>;

pub struct TunnelStreamState {
}

async fn handle_cmd(cmd_code: PackageCmdCode, cmd_body: &[u8], read: &ReadHolder, write: &WriteHolder, tunnel: Arc<Mutex<TunnelConnectionInner>>) -> P2pResult<()> {
    if cmd_code == PackageCmdCode::SynClose {
        let syn_close = SynClose::clone_from_slice(cmd_body).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
        let ack = AckClose {
            tunnel_id: syn_close.tunnel_id,
        };
        let pkg = Package::new(PackageCmdCode::AckClose, ack);
        let (mut read, mut write) = {
            let write = write.get().await;
            let read = read.get().await;
            (read, write)
        };
        write.as_mut().unwrap().write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
        write.as_mut().unwrap().flush().await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
        let mut tunnel = tunnel.lock().unwrap();
        tunnel.tunnel_stat.decrease_work_instance();
        tunnel.data_socket.as_mut().unwrap().unsplit(read.take().unwrap(), write.take().unwrap());
        let _ = tunnel.enter_idle_mode();
        log::info!("close tunnel stream {:?} local_id {} remote_id {}",
                tunnel.tunnel_id, tunnel.local_identity.get_id().to_string(), tunnel.remote_id.to_string());
        let reason = P2pErrorCode::from_u8(syn_close.reason).unwrap_or(P2pErrorCode::Ok);
        if reason != P2pErrorCode::Ok {
            Err(p2p_err!(reason, ""))
        } else {
            Ok(())
        }
    } else {
        let tunnel = tunnel.lock().unwrap();
        Err(p2p_err!(P2pErrorCode::ErrorState, "tunnel {:?} invalid cmd code {:?}", tunnel.tunnel_id, cmd_code))
    }
}

pub struct TunnelStream {
    tunnel: Arc<Mutex<TunnelConnectionInner>>,
    session_id: SessionId,
    port: u16,
    read: ReadHolder,
    write: WriteHolder,
    remainder: u16,
    read_future: Option<Pin<Box<dyn Send + Future<Output=P2pResult<usize>>>>>,
    write_future: Option<Pin<Box<dyn Send + Future<Output=std::io::Result<()>>>>>,
    flush_future: Option<Pin<Box<dyn Send + Future<Output=std::io::Result<()>>>>>,
    shutdown_future: Option<Pin<Box<dyn Send + Future<Output=P2pResult<()>>>>>,
    recv_header_handle: Option<SpawnHandle<P2pResult<(PackageCmdCode, PackageHeader)>>>,
}

impl TunnelStream {
    pub(crate) fn new(
        tunnel: Arc<Mutex<TunnelConnectionInner>>,
        read: Box<dyn P2pRead>,
        write: Box<dyn P2pWrite>,
        session_id: SessionId,
        port: u16,) -> P2pResult<Self> {
        {
            let tunnel = tunnel.lock().unwrap();
            tunnel.tunnel_stat.increase_work_instance();
        }
        let mut this = Self {
            tunnel,
            session_id,
            port,
            read: ReadHolder::new(Some(read)),
            write: WriteHolder::new(Some(write)),
            remainder: 0,
            read_future: None,
            write_future: None,
            flush_future: None,
            shutdown_future: None,
            recv_header_handle: None,
        };
        this.enter_recv_header()?;
        Ok(this)
    }

    pub(crate) fn first(
        tunnel: Arc<Mutex<TunnelConnectionInner>>,
        read: Box<dyn P2pRead>,
        write: Box<dyn P2pWrite>,
        session_id: SessionId,
        port: u16,) -> P2pResult<Self> {
        {
            let tunnel = tunnel.lock().unwrap();
            tunnel.tunnel_stat.increase_work_instance();
        }
        let mut this = Self {
            tunnel,
            session_id,
            port,
            read: ReadHolder::new(Some(read)),
            write: WriteHolder::new(Some(write)),
            remainder: 0,
            read_future: None,
            write_future: None,
            flush_future: None,
            shutdown_future: None,
            recv_header_handle: None,
        };
        Ok(this)
    }

    pub(crate) fn enter_recv_header(&mut self) -> P2pResult<()> {
        if self.recv_header_handle.is_some() {
            return Ok(());
        }

        let write = self.write.clone();
        let read = self.read.clone();
        let tunnel = self.tunnel.clone();
        let recv_header_handle = Executor::spawn_with_handle(async move {
            let mut buf_header = [0u8; 16];
            read.get().await.as_mut().unwrap().read_exact(&mut buf_header[0..PackageHeader::raw_bytes().unwrap()]).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
            let header = PackageHeader::clone_from_slice(buf_header.as_slice()).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
            let cmd_code = match header.cmd_code() {
                Ok(cmd_code) => cmd_code,
                Err(err) => {
                    return Err(err);
                }
            };
            if cmd_code == PackageCmdCode::SynClose {
                let mut cmd_body = vec![0u8; header.pkg_len() as usize];
                read.get().await.as_mut().unwrap().read_exact(cmd_body.as_mut_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
                handle_cmd(cmd_code, cmd_body.as_slice(), &read, &write, tunnel.clone()).await?;
                Ok((cmd_code, header))
            } else if cmd_code == PackageCmdCode::PieceData {
                Ok((cmd_code, header))
            } else {
                let mut cmd_body = vec![0u8; header.pkg_len() as usize];
                read.get().await.as_mut().unwrap().read_exact(cmd_body.as_mut_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
                Ok((cmd_code, header))
            }
        }).map_err(into_p2p_err!(P2pErrorCode::Failed, "spawn error"))?;
        self.recv_header_handle = Some(recv_header_handle);
        Ok(())
    }

    async fn send_inner(&mut self, data: &[u8]) -> std::io::Result<()> {
        let mut write = self.write.get().await;
        if write.is_none() {
            return Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, format!("tunnel {:?} is closed", self.session_id)));
        }
        let mut remainder = data;
        while remainder.len() > 0 {
            let chunk = if remainder.len() > MTU_LARGE as usize {
                &remainder[..MTU_LARGE as usize]
            } else {
                remainder
            };
            let header = PackageHeader::new(PackageCmdCode::PieceData, chunk.len() as u16);
            write.as_mut().unwrap().write_all(header.to_vec().unwrap().as_slice()).await?;
            write.as_mut().unwrap().write_all(chunk).await?;
            write.as_mut().unwrap().flush().await?;
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
        if self.recv_header_handle.is_some() {
            let (cmd_code, header) = self.recv_header_handle.take().unwrap().await.map_err(into_p2p_err!(P2pErrorCode::Failed, "get spawn result"))??;
            if (cmd_code != PackageCmdCode::PieceData) {
                log::error!("tunnel {:?} invalid cmd code {:?}", self.session_id, cmd_code);
                return Ok(0);
            }
            self.remainder = header.pkg_len();
        }
        if self.remainder as usize > buf.len() {
            let recv_len = self.read.get().await.as_mut().unwrap().read(buf).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
            self.remainder -= recv_len as u16;
            if self.remainder == 0 {
                self.enter_recv_header()?;
            }
            Ok(recv_len)
        } else {
            let recv_len = self.remainder;
            {
                self.read.get().await.as_mut().unwrap().read_exact(&mut buf[..self.remainder as usize]).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
            }
            self.remainder = 0;
            if self.remainder == 0 {
                self.enter_recv_header()?;
            }
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
        let mut read = self.read.get().await;
        if read.is_none() {
            return Err(p2p_err!(P2pErrorCode::Failed, "tunnel {:?} is closed", self.session_id));
        }
        read.as_mut().unwrap().read_exact(&mut buf_header[0..PackageHeader::raw_bytes().unwrap()]).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
        let header = PackageHeader::clone_from_slice(buf_header.as_slice()).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
        let cmd_code = match header.cmd_code() {
            Ok(cmd_code) => cmd_code,
            Err(err) => {
                return Err(err);
            }
        };
        let mut cmd_body = vec![0u8; header.pkg_len() as usize];
        read.as_mut().unwrap().read_exact(cmd_body.as_mut_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
        Ok((cmd_code, cmd_body))
    }

    fn close_inner(&mut self, reason: P2pErrorCode) -> P2pResult<()> {
        let tunnel = self.tunnel.clone();
        let write = self.write.clone();
        let recv_header_handle = self.recv_header_handle.take();
        let read = self.read.clone();
        let session_id = self.session_id.clone();
        Executor::spawn(async move {
            let tunnel_out = tunnel.clone();
            let conn_timeout = {
                let tunnel = tunnel.lock().unwrap();
                tunnel.conn_timeout
            };

            let ret: Result<P2pResult<()>, Elapsed> = runtime::timeout(conn_timeout, async move {
                let sequence = {
                    tunnel.lock().unwrap().tunnel_id
                };
                let pkg = Package::new(PackageCmdCode::SynClose, SynClose {
                    reason: reason.as_u8(),
                    tunnel_id: sequence,
                });

                {
                    let mut write = write.get().await;
                    if write.is_none() {
                        return Ok(());
                    }
                    write.as_mut().unwrap().write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
                    write.as_mut().unwrap().flush().await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
                }
                if recv_header_handle.is_some() {
                    let (cmd_code, header) = recv_header_handle.unwrap().await.map_err(into_p2p_err!(P2pErrorCode::Failed, "get spawn result"))??;
                    if cmd_code != PackageCmdCode::AckClose && cmd_code != PackageCmdCode::SynClose {
                        loop {
                            let mut read = read.get().await;
                            if read.is_none() {
                                break;
                            }
                            let (cmd_code, cmd_body) = read_pkg(read.as_mut().unwrap()).await?;
                            if cmd_code == PackageCmdCode::AckClose {
                                let close = AckClose::clone_from_slice(cmd_body.as_slice()).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
                                if close.tunnel_id == sequence {
                                    break;
                                }
                            } else if cmd_code == PackageCmdCode::SynClose {
                                return Ok(());
                            }
                        }
                    }

                }


                {
                    let mut write = write.get().await;
                    let mut read = read.get().await;
                    if write.is_none() || read.is_none() {
                        return Ok(());
                    }

                    let mut tunnel = tunnel.lock().unwrap();
                    tunnel.tunnel_stat.decrease_work_instance();
                    tunnel.data_socket.as_mut().unwrap().unsplit(read.take().unwrap(), write.take().unwrap());
                    tunnel.enter_idle_mode()?;
                    log::info!("close stream {:?} local_id {} remote_id {}",
                tunnel.tunnel_id, tunnel.local_identity.get_id().to_string(), tunnel.remote_id.to_string());
                }
                Ok(())
            }).await;
            match ret {
                Ok(Ok(())) => {
                }
                Ok(Err(err)) => {
                    let mut tunnel = tunnel_out.lock().unwrap();
                    tunnel.tunnel_state = TunnelState::Error;
                    log::info!("close stream {:?} local_id {} remote_id {} err {}", session_id, tunnel.local_identity.get_id().to_string(), tunnel.remote_id.to_string(), err);
                }
                Err(err) => {
                    let mut tunnel = tunnel_out.lock().unwrap();
                    tunnel.tunnel_state = TunnelState::Error;
                    log::info!("close stream {:?} local_id {} remote_id {} timeout", session_id, tunnel.local_identity.get_id().to_string(), tunnel.remote_id.to_string());
                }
            }
        });

        Ok(())
    }

    async fn flush_inner(&mut self) -> std::io::Result<()> {
        let mut write = self.write.get().await;
        if write.is_some() {
            write.as_mut().unwrap().flush().await?;
        }
        Ok(())
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    pub fn tunnel_id(&self) -> TunnelId {
        let tunnel = self.tunnel.lock().unwrap();
        tunnel.tunnel_id
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

    pub(crate) fn close(&mut self, reason: P2pErrorCode) -> P2pResult<()> {
        self.close_inner(reason)
    }

    pub(crate) async fn send_pkg<T: RawEncode>(&mut self, pkg: Package<T>) -> P2pResult<()> {
        let data = pkg.to_vec().map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
        let mut write = self.write.get().await;
        if write.is_none() {
            return Err(p2p_err!(P2pErrorCode::IoError, "tunnel {:?} is closed", self.session_id));
        }
        write.as_mut().unwrap().write_all(data.as_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
        write.as_mut().unwrap().flush().await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
        Ok(())
    }
}

impl Drop for TunnelStream {
    fn drop(&mut self) {
        {
            let tunnel = self.tunnel.lock().unwrap();
            log::info!("drop tunnel stream {:?} local_id {} remote_id {}",
                tunnel.tunnel_id, tunnel.local_identity.get_id().to_string(), tunnel.remote_id.to_string());
        }
        let _ = self.close(P2pErrorCode::Ok);
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
        unsafe {
            if self.flush_future.is_none() {
                let this: &'static mut Self = std::mem::transmute(self.as_mut().deref_mut());
                let future = this.flush_inner();
                self.as_mut().flush_future = Some(Box::pin(future));
            }

            match Pin::new(self.as_mut().flush_future.as_mut().unwrap()).poll(cx) {
                Poll::Ready(ret) => {
                    self.as_mut().flush_future = None;
                    match ret {
                        Ok(()) => Poll::Ready(Ok(())),
                        Err(e) => Poll::Ready(Err(e))
                    }
                }
                Poll::Pending => {
                    Poll::Pending
                }
            }
        }
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        Poll::Ready(Ok(()))
    }
}

pub struct TunnelDatagramSend {
    tunnel: Arc<Mutex<TunnelConnectionInner>>,
    read: ReadHolder,
    write: WriteHolder,
    write_future: Option<Pin<Box<dyn Send + Future<Output=std::io::Result<usize>>>>>,
    port: u16,
    session_id: SessionId,
    recv_handle: Option<SpawnHandle<P2pResult<(PackageCmdCode, PackageHeader)>>>,
    flush_future: Option<Pin<Box<dyn Send + Future<Output=std::io::Result<()>>>>>,
    shutdown_future: Option<Pin<Box<dyn Send + Future<Output=P2pResult<()>>>>>,
}

async fn read_pkg(read: &mut Box<dyn P2pRead>) -> P2pResult<(PackageCmdCode, Vec<u8>)> {
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
    Ok((cmd_code, cmd_body))
}

impl TunnelDatagramSend {
    pub(crate) fn new(tunnel: Arc<Mutex<TunnelConnectionInner>>,
                      mut read: Box<dyn P2pRead>,
                      write: Box<dyn P2pWrite>,
                      port: u16,
                      session_id: SessionId) -> P2pResult<Self> {
        {
            let tunnel = tunnel.lock().unwrap();
            tunnel.tunnel_stat.increase_work_instance();
        }

        let mut this = Self {
            tunnel,
            read: ReadHolder::new(Some(read)),
            write: WriteHolder::new(Some(write)),
            write_future: None,
            port,
            session_id,
            recv_handle: None,
            flush_future: None,
            shutdown_future: None,
        };

        this.enter_recv_header()?;
        Ok(this)
    }

    pub(crate) fn first(tunnel: Arc<Mutex<TunnelConnectionInner>>,
                      mut read: Box<dyn P2pRead>,
                      write: Box<dyn P2pWrite>,
                      port: u16,
                      session_id: SessionId) -> P2pResult<Self> {
        {
            let tunnel = tunnel.lock().unwrap();
            tunnel.tunnel_stat.increase_work_instance();
        }

        let mut this = Self {
            tunnel,
            read: ReadHolder::new(Some(read)),
            write: WriteHolder::new(Some(write)),
            write_future: None,
            port,
            session_id,
            recv_handle: None,
            flush_future: None,
            shutdown_future: None,
        };

        Ok(this)
    }

    pub(crate) fn enter_recv_header(&mut self) -> P2pResult<()> {
        if self.recv_handle.is_some() {
            return Ok(());
        }

        let write = self.write.clone();
        let read = self.read.clone();
        let tunnel = self.tunnel.clone();
        let recv_header_handle = Executor::spawn_with_handle(async move {
            let mut buf_header = [0u8; 16];
            read.get().await.as_mut().unwrap().read_exact(&mut buf_header[0..PackageHeader::raw_bytes().unwrap()]).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
            let header = PackageHeader::clone_from_slice(buf_header.as_slice()).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
            let cmd_code = match header.cmd_code() {
                Ok(cmd_code) => cmd_code,
                Err(err) => {
                    return Err(err);
                }
            };
            let mut cmd_body = vec![0u8; header.pkg_len() as usize];
            read.get().await.as_mut().unwrap().read_exact(cmd_body.as_mut_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
            if cmd_code == PackageCmdCode::SynClose {
                handle_cmd(cmd_code, cmd_body.as_slice(), &read, &write, tunnel.clone()).await?;
                Ok((cmd_code, header))
            } else if cmd_code == PackageCmdCode::AckClose {
                Ok((cmd_code, header))
            } else {
                log::info!("recv error pkg cmd code {:?}", cmd_code);
                let mut tunnel = tunnel.lock().unwrap();
                tunnel.tunnel_state = TunnelState::Error;
                Err(p2p_err!(P2pErrorCode::Failed, "tunnel {:?} invalid cmd code {:?}", tunnel.tunnel_id, cmd_code))
            }
        }).map_err(into_p2p_err!(P2pErrorCode::Failed, "spawn error"))?;
        self.recv_handle = Some(recv_header_handle);
        Ok(())
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    async fn send_inner(&mut self, data: &[u8]) -> std::io::Result<usize> {
        let mut write = self.write.get().await;
        if write.is_none() {
            return Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "tunnel is closed"));
        }
        let data_len = data.len();
        let mut remainder = data;
        while remainder.len() > 0 {
            let chunk = if remainder.len() > MTU_LARGE as usize {
                &remainder[..MTU_LARGE as usize]
            } else {
                remainder
            };
            let header = PackageHeader::new(PackageCmdCode::PieceData, chunk.len() as u16);
            write.as_mut().unwrap().write_all(header.to_vec().unwrap().as_slice()).await?;
            write.as_mut().unwrap().write_all(chunk).await?;
            write.as_mut().unwrap().flush().await?;
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

    fn close_inner(&mut self, reason: P2pErrorCode) -> P2pResult<()> {
        let tunnel = self.tunnel.clone();
        let write = self.write.clone();
        let recv_handle = self.recv_handle.take();
        let session_id = self.session_id.clone();
        let read = self.read.clone();
        Executor::spawn(async move {
            let conn_timeout = {
                let tunnel = tunnel.lock().unwrap();
                tunnel.conn_timeout
            };
            let tunnel_out = tunnel.clone();
            let ret: Result<P2pResult<()>, Elapsed> = runtime::timeout(conn_timeout, async move {
                let sequence = {
                    tunnel.lock().unwrap().tunnel_id
                };
                let pkg = Package::new(PackageCmdCode::SynClose, SynClose {
                    reason: reason.as_u8(),
                    tunnel_id: sequence,
                });

                {
                    let mut write = write.get().await;
                    if write.is_none() {
                        return Ok(());
                    }
                    write.as_mut().unwrap().write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
                    write.as_mut().unwrap().flush().await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
                }
                if recv_handle.is_some() {
                    let (cmd_code, header) = recv_handle.unwrap().await.map_err(into_p2p_err!(P2pErrorCode::Failed, "get spawn result"))??;
                    if cmd_code != PackageCmdCode::AckClose && cmd_code != PackageCmdCode::SynClose {
                        return Err(p2p_err!(P2pErrorCode::Failed, "recv unexpect cmd {:?}", cmd_code));
                    }
                }


                {
                    let mut write = write.get().await;
                    let mut read = read.get().await;
                    if write.is_none() || read.is_none() {
                        return Ok(());
                    }

                    let mut tunnel = tunnel.lock().unwrap();
                    tunnel.tunnel_stat.decrease_work_instance();
                    tunnel.data_socket.as_mut().unwrap().unsplit(read.take().unwrap(), write.take().unwrap());
                    tunnel.enter_idle_mode()?;
                    log::info!("close stream {:?} local_id {} remote_id {}",
                tunnel.tunnel_id, tunnel.local_identity.get_id().to_string(), tunnel.remote_id.to_string());
                }

                Ok(())
            }).await;
            match ret {
                Ok(Ok(())) => {
                }
                Ok(Err(err)) => {
                    let mut tunnel = tunnel_out.lock().unwrap();
                    tunnel.tunnel_state = TunnelState::Error;
                    log::info!("close stream {:?} local_id {} remote_id {} err {}", session_id, tunnel.local_identity.get_id().to_string(), tunnel.remote_id.to_string(), err);
                }
                Err(err) => {
                    let mut tunnel = tunnel_out.lock().unwrap();
                    tunnel.tunnel_state = TunnelState::Error;
                    log::info!("close stream {:?} local_id {} remote_id {} timeout", session_id, tunnel.local_identity.get_id().to_string(), tunnel.remote_id.to_string());
                }
            }
        });
        Ok(())
    }

    async fn flush_inner(&mut self) -> std::io::Result<()> {
        let mut write = self.write.get().await;
        if write.is_some() {
            write.as_mut().unwrap().flush().await?;
        }
        Ok(())
    }

    pub(crate) async fn send_pkg<T: RawEncode>(&mut self, pkg: Package<T>) -> P2pResult<()> {
        let data = pkg.to_vec().map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
        let mut write = self.write.get().await;
        if write.is_none() {
            return Err(p2p_err!(P2pErrorCode::IoError, "tunnel {:?} is closed", self.session_id));
        }
        write.as_mut().unwrap().write_all(data.as_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
        write.as_mut().unwrap().flush().await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
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
        unsafe {
            if self.flush_future.is_none() {
                let this: &'static mut Self = std::mem::transmute(self.as_mut().deref_mut());
                let future = this.flush_inner();
                self.as_mut().flush_future = Some(Box::pin(future));
            }

            match Pin::new(self.as_mut().flush_future.as_mut().unwrap()).poll(cx) {
                Poll::Ready(ret) => {
                    self.as_mut().flush_future = None;
                    match ret {
                        Ok(()) => Poll::Ready(Ok(())),
                        Err(e) => Poll::Ready(Err(e))
                    }
                }
                Poll::Pending => {
                    Poll::Pending
                }
            }
        }
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        Poll::Ready(Ok(()))
    }
}

impl TunnelDatagramSend {
    pub fn tunnel_id(&self) -> TunnelId {
        let tunnel = self.tunnel.lock().unwrap();
        tunnel.tunnel_id
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

    pub(crate) fn close(&mut self, reason: P2pErrorCode) -> P2pResult<()> {
        self.close_inner(reason)
    }
}

impl Drop for TunnelDatagramSend {
    fn drop(&mut self) {
        {
            let tunnel = self.tunnel.lock().unwrap();
            log::info!("drop tunnel datagram {:?} local_id {} remote_id {}",
                tunnel.tunnel_id, tunnel.local_identity.get_id().to_string(), tunnel.remote_id.to_string());
        }
        self.close(P2pErrorCode::Ok);
    }
}

pub struct TunnelDatagramRecv {
    tunnel: Arc<Mutex<TunnelConnectionInner>>,
    read: Option<Box<dyn P2pRead>>,
    write: Option<Box<dyn P2pWrite>>,
    remainder: u16,
    read_future: Option<Pin<Box<dyn Send + Future<Output=std::io::Result<usize>>>>>,
    port: u16,
    session_id: SessionId,
    is_closed: bool,
}

impl TunnelDatagramRecv {
    pub(crate) fn new(tunnel: Arc<Mutex<TunnelConnectionInner>>, read: Box<dyn P2pRead>, write: Box<dyn P2pWrite>, port: u16, session_id: SessionId) -> Self {
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
            port,
            session_id,
            is_closed: false,
        }
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    async fn handle_cmd(&mut self, cmd_code: PackageCmdCode, cmd_body: &[u8]) -> P2pResult<()> {
        if cmd_code == PackageCmdCode::SynClose {
            let syn_close = SynClose::clone_from_slice(cmd_body).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
            let ack = AckClose {
                tunnel_id: syn_close.tunnel_id,
            };
            let pkg = Package::new(PackageCmdCode::AckClose, ack);
            self.write.as_mut().unwrap().write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
            self.write.as_mut().unwrap().flush().await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
            let mut tunnel = self.tunnel.lock().unwrap();
            tunnel.tunnel_stat.decrease_work_instance();
            tunnel.data_socket.as_mut().unwrap().unsplit(self.read.take().unwrap(), self.write.take().unwrap());
            tunnel.enter_idle_mode()?;
            self.is_closed = true;
            let reason = P2pErrorCode::from_u8(syn_close.reason).unwrap_or(P2pErrorCode::Ok);
            if reason != P2pErrorCode::Ok {
                Err(p2p_err!(reason, ""))
            } else {
                Ok(())
            }
        } else {
            let tunnel = self.tunnel.lock().unwrap();
            Err(p2p_err!(P2pErrorCode::ErrorState, "tunnel {:?} invalid cmd code {:?}", tunnel.tunnel_id, cmd_code))
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

    fn close_inner(&mut self, reason: P2pErrorCode) -> P2pResult<()> {
        let tunnel = self.tunnel.clone();
        let mut write = self.write.take();
        let mut read = self.read.take();
        let session_id = self.session_id.clone();
        Executor::spawn(async move {
            let tunnel_out = tunnel.clone();
            let conn_timeout = {
                let tunnel = tunnel.lock().unwrap();
                tunnel.conn_timeout
            };

            let ret: Result<P2pResult<()>, Elapsed> = runtime::timeout(conn_timeout, async move {
                let sequence = {
                    tunnel.lock().unwrap().tunnel_id
                };
                let pkg = Package::new(PackageCmdCode::SynClose, SynClose {
                    reason: reason.as_u8(),
                    tunnel_id: sequence,
                });

                if write.is_none() || read.is_none() {
                    return Ok(());
                }
                write.as_mut().unwrap().write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
                write.as_mut().unwrap().flush().await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;

                loop {
                    let (cmd_code, cmd_body) = read_pkg(read.as_mut().unwrap()).await?;
                    if cmd_code == PackageCmdCode::AckClose {
                        let close = AckClose::clone_from_slice(cmd_body.as_slice()).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
                        if close.tunnel_id == sequence {
                            break;
                        }
                    } else if cmd_code == PackageCmdCode::SynClose {
                        let close = SynClose::clone_from_slice(cmd_body.as_slice()).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
                        if close.tunnel_id != sequence {
                            continue;
                        }
                        let ack = AckClose {
                            tunnel_id: close.tunnel_id,
                        };
                        let pkg = Package::new(PackageCmdCode::AckClose, ack);
                        write.as_mut().unwrap().write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
                        write.as_mut().unwrap().flush().await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
                    }
                }

                {
                    let mut tunnel = tunnel.lock().unwrap();
                    tunnel.data_socket.as_mut().unwrap().unsplit(read.take().unwrap(), write.take().unwrap());
                    tunnel.enter_idle_mode()?;
                    tunnel.tunnel_stat.decrease_work_instance();
                    log::info!("close datagram {:?} local_id {} remote_id {}",
                tunnel.tunnel_id, tunnel.local_identity.get_id().to_string(), tunnel.remote_id.to_string());
                }
                Ok(())
            }).await;
            match ret {
                Ok(Ok(())) => {
                }
                Ok(Err(err)) => {
                    let mut tunnel = tunnel_out.lock().unwrap();
                    tunnel.tunnel_state = TunnelState::Error;
                    log::info!("close datagram recv {:?} local_id {} remote_id {} err {}", session_id, tunnel.local_identity.get_id().to_string(), tunnel.remote_id.to_string(), err);
                }
                Err(err) => {
                    let mut tunnel = tunnel_out.lock().unwrap();
                    tunnel.tunnel_state = TunnelState::Error;
                    log::info!("close datagram recv {:?} local_id {} remote_id {} timeout", session_id, tunnel.local_identity.get_id().to_string(), tunnel.remote_id.to_string());
                }
            }
        });

        Ok(())
    }

    pub(crate) async fn send_pkg<T: RawEncode>(&mut self, pkg: Package<T>) -> P2pResult<()> {
        let data = pkg.to_vec().map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
        self.write.as_mut().unwrap().write_all(data.as_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
        self.write.as_mut().unwrap().flush().await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
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
    pub(crate) fn tunnel_id(&self) -> TunnelId {
        let tunnel = self.tunnel.lock().unwrap();
        tunnel.tunnel_id
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

    pub(crate) fn close(&mut self, reason: P2pErrorCode) -> P2pResult<()> {
        if self.is_closed {
            return Ok(());
        }
        self.is_closed = true;
        self.close_inner(reason)
    }
}

impl Drop for TunnelDatagramRecv {
    fn drop(&mut self) {
        {
            let tunnel = self.tunnel.lock().unwrap();
            log::info!("drop tunnel datagram {:?} local_id {} remote_id {}",
                tunnel.tunnel_id, tunnel.local_identity.get_id().to_string(), tunnel.remote_id.to_string());
        }
        let _ = self.close(P2pErrorCode::Ok);
    }
}

pub struct FutureHolder<T> {
    id: String,
    future: Mutex<Option<NotifyFuture<T>>>
}

impl<T> FutureHolder<T> {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            id: format!("{}", rand::random::<u64>()),
            future: Mutex::new(None),
        })
    }

    pub fn get_id(&self) -> String {
        self.id.clone()
    }

    pub fn set_future(&self, future: NotifyFuture<T>) {
        let mut future_locked = self.future.lock().unwrap();
        assert!(future_locked.is_none());
        *future_locked = Some(future);
    }

    pub fn try_complete(&self, result: T) -> Option<T> {
        let mut future_locked = self.future.lock().unwrap();
        if future_locked.is_none() {
            return Some(result);
        }

        let future = future_locked.take().unwrap();
        future.set_complete(result);
        None
    }
}

pub type TunnelFutureHolder = FutureHolder<P2pResult<(Box<dyn P2pRead>, PackageCmdCode, Vec<u8>)>>;
pub type TunnelFutureHolderRef = Arc<FutureHolder<P2pResult<(Box<dyn P2pRead>, PackageCmdCode, Vec<u8>)>>>;

pub(crate) struct TunnelConnectionInner {
    tunnel_id: TunnelId,
    local_identity: P2pIdentityRef,
    remote_id: P2pId,
    remote_ep: Endpoint,
    conn_timeout: Duration,
    protocol_version: u8,
    stack_version: u32,
    data_socket: Option<P2pConnectionRef>,
    tunnel_state: TunnelState,
    accept_handle: Option<SpawnHandle<()>>,
    write: Option<Box<dyn P2pWrite>>,
    accept_future: TunnelFutureHolderRef,
    recv_future: TunnelFutureHolderRef,
    cert_factory: P2pIdentityCertFactoryRef,
    tunnel_stat: TunnelStatRef,
}

impl Drop for TunnelConnectionInner {
    fn drop(&mut self) {
        log::info!("drop tunnel connection inner {:?} local_id {} remote_id {}",
            self.tunnel_id, self.local_identity.get_id().to_string(), self.remote_id.to_string());
    }
}

impl TunnelConnectionInner {
    pub(crate) fn new(
        tunnel_id: TunnelId,
        local_identity: P2pIdentityRef,
        remote_id: P2pId,
        remote_ep: Endpoint,
        conn_timeout: Duration,
        protocol_version: u8,
        stack_version: u32,
        data_socket: Option<P2pConnectionRef>,
        cert_factory: P2pIdentityCertFactoryRef,) -> P2pResult<Self> {
        let accept_future = TunnelFutureHolder::new();
        let recv_future = TunnelFutureHolder::new();

        log::info!("create tunnel connection {:?} local_id {} remote_id {} remote_ep {}",
            tunnel_id, local_identity.get_id().to_string(), remote_id.to_string(), remote_ep);
        let mut obj = Self {
            tunnel_id,
            local_identity,
            remote_id,
            remote_ep,
            conn_timeout,
            protocol_version,
            stack_version,
            data_socket,
            tunnel_state: TunnelState::Init,
            accept_handle: None,
            write: None,
            accept_future,
            recv_future,
            tunnel_stat: TunnelStat::new(),
            cert_factory,
        };
        Ok(obj)
    }

    fn enter_idle_mode(&mut self) -> P2pResult<()> {
        if self.tunnel_state == TunnelState::Idle {
            return Ok(());
        }
        if self.data_socket.is_some() {
            let (read, write) = self.data_socket.as_ref().unwrap().split()?;
            let handle = create_accept_handle(read, self.accept_future.clone(), self.recv_future.clone())?;
            self.write = Some(write);
            self.accept_handle = Some(handle);
            self.tunnel_state = TunnelState::Idle;
            log::info!("enter idle mode tunnel {:?} local_id {} remote_id {}",
                self.tunnel_id, self.local_identity.get_id().to_string(), self.remote_id.to_string());
        }
        Ok(())
    }

    async fn open_stream_inner(tunnel_id: TunnelId,
                               write: &mut Box<dyn P2pWrite>,
                               recv_future: NotifyFuture<P2pResult<(Box<dyn P2pRead>, PackageCmdCode, Vec<u8>)>>,
                               vport: u16,
                               session_id: SessionId) -> P2pResult<Box<dyn P2pRead>> {
        let syn = SynStream {
            tunnel_id,
            to_vport: vport,
            session_id,
            payload: Vec::new(),
        };
        let pkg = Package::new(PackageCmdCode::SynStream, syn);

        write.write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
        write.flush().await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
        let (read, cmd_code, cmd_body) = recv_future.await?;

        if cmd_code != PackageCmdCode::AckStream {
            return Err(p2p_err!(P2pErrorCode::InvalidData, "tunnel {:?} invalid ack stream", tunnel_id));
        }

        let ack = AckStream::clone_from_slice(cmd_body.as_slice()).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
        if ack.result != 0 {
            return Err(p2p_err!(P2pErrorCode::ConnectionRefused, "tunnel {:?} open stream failed. return {}", tunnel_id, ack.result));
        }

        Ok(read)
    }

    async fn open_reverse_stream_inner(tunnel_id: TunnelId,
                                       write: &mut Box<dyn P2pWrite>,
                                       recv_future: NotifyFuture<P2pResult<(Box<dyn P2pRead>, PackageCmdCode, Vec<u8>)>>,
                                       vport: u16,
                                       session_id: SessionId) -> P2pResult<Box<dyn P2pRead>> {
        let syn = SynReverseStream {
            tunnel_id,
            session_id,
            vport,
            payload: Vec::new(),
        };
        let pkg = Package::new(PackageCmdCode::SynReverseStream, syn);

        write.write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
        write.flush().await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
        let (read, cmd_code, cmd_body) = recv_future.await?;

        if cmd_code != PackageCmdCode::AckReverseStream {
            return Err(p2p_err!(P2pErrorCode::InvalidData, "tunnel {:?} invalid ack stream", tunnel_id));
        }

        let ack = AckReverseStream::clone_from_slice(cmd_body.as_slice()).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
        if ack.result != 0 {
            return Err(p2p_err!(P2pErrorCode::ConnectionRefused, "tunnel {:?} open stream failed. return {}", tunnel_id, ack.result));
        }

        Ok(read)
    }

    async fn open_datagram_inner(tunnel_id: TunnelId,
                                 write: &mut Box<dyn P2pWrite>,
                                 recv_future: NotifyFuture<P2pResult<(Box<dyn P2pRead>, PackageCmdCode, Vec<u8>)>>,
                                 vport: u16,
                                 session_id: SessionId) -> P2pResult<Box<dyn P2pRead>> {
        let syn = SynDatagram {
            tunnel_id,
            to_vport: vport,
            session_id,
            payload: Vec::new(),
        };
        let pkg = Package::new(PackageCmdCode::SynDatagram, syn);

        write.write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
        write.flush().await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
        let (read, cmd_code, cmd_body) = recv_future.await?;

        if cmd_code != PackageCmdCode::AckDatagram {
            return Err(p2p_err!(P2pErrorCode::InvalidData, "tunnel {:?} invalid ack datagram", tunnel_id));
        }

        let ack = AckDatagram::clone_from_slice(cmd_body.as_slice()).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
        if ack.result != 0 {
            return Err(p2p_err!(P2pErrorCode::ConnectionRefused, "tunnel {:?} open datagram failed. return {}", tunnel_id, ack.result));
        }

        Ok(read)
    }

    async fn open_reverse_datagram_inner(tunnel_id: TunnelId,
                                         write: &mut Box<dyn P2pWrite>,
                                         recv_future: NotifyFuture<P2pResult<(Box<dyn P2pRead>, PackageCmdCode, Vec<u8>)>>,
                                         vport: u16,
                                         session_id: SessionId,) -> P2pResult<Box<dyn P2pRead>> {
        let syn = SynReverseDatagram {
            tunnel_id,
            to_vport: vport,
            session_id,
            payload: Vec::new(),
        };
        let pkg = Package::new(PackageCmdCode::SynReverseDatagram, syn);

        write.write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
        write.flush().await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
        let (read, cmd_code, cmd_body) = recv_future.await?;

        if cmd_code != PackageCmdCode::AckReverseDatagram {
            return Err(p2p_err!(P2pErrorCode::InvalidData, "tunnel {:?} invalid ack stream", tunnel_id));
        }

        let ack = AckReverseDatagram::clone_from_slice(cmd_body.as_slice()).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
        if ack.result != 0 {
            return Err(p2p_err!(P2pErrorCode::ConnectionRefused, "tunnel {:?} open stream failed. return {}", tunnel_id, ack.result));
        }

        Ok(read)
    }
}

pub struct TunnelConnection {
    inner: Arc<Mutex<TunnelConnectionInner>>
}

impl TunnelConnection {
    pub fn new(
        tunnel_id: TunnelId,
        local_identity: P2pIdentityRef,
        remote_id: P2pId,
        remote_ep: Endpoint,
        conn_timeout: Duration,
        protocol_version: u8,
        stack_version: u32,
        tcp_socket: Option<P2pConnectionRef>,
        cert_factory: P2pIdentityCertFactoryRef, ) -> P2pResult<Self> {
        Ok(Self {
            inner: Arc::new(Mutex::new(TunnelConnectionInner::new(
                tunnel_id,
                local_identity,
                remote_id,
                remote_ep,
                conn_timeout,
                protocol_version,
                stack_version,
                tcp_socket,
                cert_factory)?))
        })
    }

    pub fn get_tunnel_id(&self) -> TunnelId {
        let inner = self.inner.lock().unwrap();
        inner.tunnel_id
    }

    async fn open_reverse_stream(&self, vport: u16, session_id: SessionId) -> P2pResult<TunnelStream> {
        let (conn_timeout, sequence, mut write, recv_future) = {
            let mut inner = self.inner.lock().unwrap();
            if inner.tunnel_state != TunnelState::Idle {
                return Err(p2p_err!(P2pErrorCode::ErrorState, "tunnel {:?} invalid state {:?}", inner.tunnel_id, inner.tunnel_state));
            }
            inner.tunnel_state = TunnelState::Opening;

            let future = NotifyFuture::<P2pResult<(Box<dyn P2pRead>, PackageCmdCode, Vec<u8>)>>::new();
            inner.recv_future.set_future(future.clone());

            (inner.conn_timeout, inner.tunnel_id, inner.write.take().unwrap(), future)
        };
        match runtime::timeout(conn_timeout, TunnelConnectionInner::open_reverse_stream_inner(sequence, &mut write, recv_future, vport, session_id.clone())).await {
            Ok(Ok(read)) => {
                let mut inner = self.inner.lock().unwrap();
                inner.tunnel_state = TunnelState::Worked;
                Ok(TunnelStream::new(self.inner.clone(), read, write, session_id, vport)?)
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

    async fn open_reverse_datagram(&self, vport: u16, session_id: SessionId) -> P2pResult<TunnelDatagramRecv> {
        let (conn_timeout, sequence, mut write, recv_future) = {
            let mut inner = self.inner.lock().unwrap();
            if inner.tunnel_state != TunnelState::Idle {
                return Err(p2p_err!(P2pErrorCode::ErrorState, "tunnel {:?} invalid state {:?}", inner.tunnel_id, inner.tunnel_state));
            }
            inner.tunnel_state = TunnelState::Opening;
            let future = NotifyFuture::<P2pResult<(Box<dyn P2pRead>, PackageCmdCode, Vec<u8>)>>::new();
            inner.recv_future.set_future(future.clone());
            (inner.conn_timeout, inner.tunnel_id, inner.write.take().unwrap(), future)
        };
        match runtime::timeout(conn_timeout, TunnelConnectionInner::open_reverse_datagram_inner(sequence, &mut write, recv_future, vport, session_id)).await {
            Ok(Ok(read)) => {
                let mut inner = self.inner.lock().unwrap();
                inner.tunnel_state = TunnelState::Worked;
                Ok(TunnelDatagramRecv::new(self.inner.clone(), read, write, vport, session_id))
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

    pub(crate) async fn connect_stream(&self, vport: u16, session_id: SessionId) -> P2pResult<TunnelStream> {
        let (has_socket, local_identity, remote_ep, remote_id, conn_timeout, verifier) = {
            let mut inner = self.inner.lock().unwrap();
            inner.enter_idle_mode()?;
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

    pub(crate) async fn connect_datagram(&self, vport: u16, session_id: SessionId) -> P2pResult<TunnelDatagramSend> {
        let (has_socket, local_identity, remote_ep, remote_id, conn_timeout, verifier) = {
            let mut inner = self.inner.lock().unwrap();
            inner.enter_idle_mode()?;
            (inner.data_socket.is_some(), inner.local_identity.clone(), inner.remote_ep.clone(), inner.remote_id.clone(), inner.conn_timeout, inner.cert_factory.clone())
        };

        if has_socket {
            return Ok(self.open_datagram(vport, session_id).await?);
        }

        let tcp_socket = TCPConnection::connect(verifier, local_identity, remote_ep, remote_id, conn_timeout).await?;

        {
            let mut inner = self.inner.lock().unwrap();
            inner.data_socket = Some(Arc::new(tcp_socket));
            inner.enter_idle_mode()?;
        }

        Ok(self.open_datagram(vport, session_id).await?)
    }

    pub(crate) async fn connect_reverse_stream(&self, vport: u16, session_id: SessionId) -> P2pResult<TunnelStream> {
        let (has_socket, local_identity, remote_ep, remote_id, conn_timeout, verifier) = {
            let mut inner = self.inner.lock().unwrap();
            inner.enter_idle_mode()?;
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

    pub(crate) async fn connect_reverse_datagram(&self, vport: u16, session_id: SessionId) -> P2pResult<TunnelDatagramRecv> {
        let (has_socket, local_identity, remote_ep, remote_id, conn_timeout, verifier) = {
            let mut inner = self.inner.lock().unwrap();
            inner.enter_idle_mode()?;
            (inner.data_socket.is_some(), inner.local_identity.clone(), inner.remote_ep.clone(), inner.remote_id.clone(), inner.conn_timeout, inner.cert_factory.clone())
        };

        if has_socket {
            return Ok(self.open_reverse_datagram(vport, session_id).await?);
        }

        let tcp_socket = TCPConnection::connect(verifier, local_identity, remote_ep, remote_id, conn_timeout).await?;

        {
            let mut inner = self.inner.lock().unwrap();
            inner.data_socket = Some(Arc::new(tcp_socket));
            inner.enter_idle_mode()?;
        }

        Ok(self.open_reverse_datagram(vport, session_id).await?)
    }

    pub(crate) async fn open_stream(&self, vport: u16, session_id: SessionId) -> P2pResult<TunnelStream> {
        let (conn_timeout, sequence, mut write, recv_future) = {
            let mut inner = self.inner.lock().unwrap();
            if inner.tunnel_state != TunnelState::Idle {
                return Err(p2p_err!(P2pErrorCode::ErrorState, "tunnel {:?} invalid state {:?}", inner.tunnel_id, inner.tunnel_state));
            }
            inner.tunnel_state = TunnelState::Opening;

            let future = NotifyFuture::<P2pResult<(Box<dyn P2pRead>, PackageCmdCode, Vec<u8>)>>::new();
            inner.recv_future.set_future(future.clone());

            (inner.conn_timeout, inner.tunnel_id, inner.write.take().unwrap(), future)
        };
        match runtime::timeout(conn_timeout, TunnelConnectionInner::open_stream_inner(sequence, &mut write, recv_future, vport, session_id.clone())).await {
            Ok(Ok(read)) => {
                let inner = {
                    let mut inner = self.inner.lock().unwrap();
                    inner.tunnel_state = TunnelState::Worked;
                    self.inner.clone()
                };
                Ok(TunnelStream::new(inner, read, write, session_id, vport)?)
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

    pub(crate) async fn open_datagram(&self, vport: u16, session_id: SessionId) -> P2pResult<TunnelDatagramSend> {
        let (conn_timeout, sequence, mut write, recv_future) = {
            let mut inner = self.inner.lock().unwrap();
            if inner.tunnel_state != TunnelState::Idle {
                return Err(p2p_err!(P2pErrorCode::ErrorState, "tunnel {:?} invalid state {:?}", inner.tunnel_id, inner.tunnel_state));
            }
            inner.tunnel_state = TunnelState::Opening;
            let future = NotifyFuture::<P2pResult<(Box<dyn P2pRead>, PackageCmdCode, Vec<u8>)>>::new();
            inner.recv_future.set_future(future.clone());
            (inner.conn_timeout, inner.tunnel_id, inner.write.take().unwrap(), future)
        };
        match runtime::timeout(conn_timeout, TunnelConnectionInner::open_datagram_inner(sequence, &mut write, recv_future, vport, session_id)).await {
            Ok(Ok(read)) => {
                let inner = {
                    let mut inner = self.inner.lock().unwrap();
                    inner.tunnel_state = TunnelState::Worked;
                    self.inner.clone()
                };
                Ok(TunnelDatagramSend::new(inner, read, write, vport, session_id)?)
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

    pub(crate) async fn accept_next_session(&self) -> P2pResult<TunnelSession> {
        loop {
            let future = NotifyFuture::new();
            {
                let inner = self.inner.lock().unwrap();
                inner.accept_future.set_future(future.clone());
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
                        inner.tunnel_id = syn_stream.tunnel_id;
                        let write = inner.write.take().unwrap();
                        write
                    };
                    let ack = AckStream {
                        result: 0,
                    };
                    let pkg = Package::new(PackageCmdCode::AckStream, ack);
                    write.write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
                    write.flush().await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
                    {
                        let mut inner = self.inner.lock().unwrap();
                        inner.tunnel_state = TunnelState::Worked;
                    }
                    return Ok(TunnelSession::Stream(TunnelStream::new(self.inner.clone(), read, write, session_id, vport)?))
                }
                PackageCmdCode::SynReverseStream => {
                    let reserve_stream = SynReverseStream::clone_from_slice(cmd_body.as_slice()).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
                    let mut write = {
                        let mut inner = self.inner.lock().unwrap();
                        if inner.tunnel_state != TunnelState::Idle {
                            continue;
                        }
                        inner.tunnel_state = TunnelState::Accepting;
                        inner.tunnel_id = reserve_stream.tunnel_id;
                        let write = inner.write.take().unwrap();
                        write
                    };
                    let ack = AckReverseStream {
                        result: 0,
                    };
                    let pkg = Package::new(PackageCmdCode::AckReverseStream, ack);
                    write.write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
                    write.flush().await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
                    {
                        let mut inner = self.inner.lock().unwrap();
                        inner.tunnel_state = TunnelState::Worked;
                    }
                    return Ok(TunnelSession::ReverseStream(TunnelStream::new(self.inner.clone(), read, write, reserve_stream.session_id, 0)?));
                }
                PackageCmdCode::SynDatagram => {
                    let syn_datagram = SynDatagram::clone_from_slice(cmd_body.as_slice()).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
                    let mut write = {
                        let mut inner = self.inner.lock().unwrap();
                        if inner.tunnel_state != TunnelState::Idle {
                            continue;
                        }
                        inner.tunnel_state = TunnelState::Accepting;
                        inner.tunnel_id = syn_datagram.tunnel_id;
                        let write = inner.write.take().unwrap();
                        write
                    };
                    let ack = AckDatagram {
                        result: 0,
                    };
                    let pkg = Package::new(PackageCmdCode::AckDatagram, ack);
                    write.write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
                    write.flush().await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
                    {
                        let mut inner = self.inner.lock().unwrap();
                        inner.tunnel_state = TunnelState::Worked;
                    }
                    return Ok(TunnelSession::Datagram(TunnelDatagramRecv::new(self.inner.clone(), read, write, syn_datagram.to_vport, syn_datagram.session_id)))
                }
                PackageCmdCode::SynReverseDatagram => {
                    let reverse_datagram = SynReverseDatagram::clone_from_slice(cmd_body.as_slice()).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
                    let mut write = {
                        let mut inner = self.inner.lock().unwrap();
                        if inner.tunnel_state != TunnelState::Idle {
                            continue;
                        }
                        inner.tunnel_state = TunnelState::Accepting;
                        inner.tunnel_id = reverse_datagram.tunnel_id;
                        let write = inner.write.take().unwrap();
                        write
                    };
                    let ack = AckReverseDatagram {
                        result: 0,
                    };
                    let pkg = Package::new(PackageCmdCode::AckReverseDatagram, ack);
                    write.write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
                    write.flush().await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
                    {
                        let mut inner = self.inner.lock().unwrap();
                        inner.tunnel_state = TunnelState::Worked;
                    }
                    return Ok(TunnelSession::ReverseDatagram(TunnelDatagramSend::new(self.inner.clone(),
                                                                                     read,
                                                                                     write,
                                                                                     reverse_datagram.to_vport,
                                                                                     reverse_datagram.session_id)?));
                }
                _ => {
                    let inner = self.inner.lock().unwrap();
                    return Err(p2p_err!(P2pErrorCode::InvalidData, "tunnel {:?} invalid cmd code {:?}", inner.tunnel_id, cmd_code));
                }
            }
        }
    }

    pub(crate) async fn accept_first_session(&self) -> P2pResult<TunnelSession> {
        loop {
            let future = NotifyFuture::new();
            {
                let mut inner = self.inner.lock().unwrap();
                inner.accept_future.set_future(future.clone());
                inner.enter_idle_mode()?;
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
                        inner.tunnel_id = syn_stream.tunnel_id;
                        let write = inner.write.take().unwrap();
                        write
                    };
                    {
                        let mut inner = self.inner.lock().unwrap();
                        inner.tunnel_state = TunnelState::Worked;
                    }
                    return Ok(TunnelSession::Stream(TunnelStream::first(self.inner.clone(), read, write, session_id, vport)?))
                }
                PackageCmdCode::SynReverseStream => {
                    let reserve_stream = SynReverseStream::clone_from_slice(cmd_body.as_slice()).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
                    let mut write = {
                        let mut inner = self.inner.lock().unwrap();
                        if inner.tunnel_state != TunnelState::Idle {
                            continue;
                        }
                        inner.tunnel_state = TunnelState::Accepting;
                        inner.tunnel_id = reserve_stream.tunnel_id;
                        let write = inner.write.take().unwrap();
                        write
                    };
                    {
                        let mut inner = self.inner.lock().unwrap();
                        inner.tunnel_state = TunnelState::Worked;
                    }
                    return Ok(TunnelSession::ReverseStream(TunnelStream::first(self.inner.clone(), read, write, reserve_stream.session_id, 0)?));
                }
                PackageCmdCode::SynDatagram => {
                    let syn_datagram = SynDatagram::clone_from_slice(cmd_body.as_slice()).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
                    let mut write = {
                        let mut inner = self.inner.lock().unwrap();
                        if inner.tunnel_state != TunnelState::Idle {
                            continue;
                        }
                        inner.tunnel_state = TunnelState::Accepting;
                        inner.tunnel_id = syn_datagram.tunnel_id;
                        let write = inner.write.take().unwrap();
                        write
                    };
                    {
                        let mut inner = self.inner.lock().unwrap();
                        inner.tunnel_state = TunnelState::Worked;
                    }
                    return Ok(TunnelSession::Datagram(TunnelDatagramRecv::new(self.inner.clone(), read, write, syn_datagram.to_vport, syn_datagram.session_id)))
                }
                PackageCmdCode::SynReverseDatagram => {
                    let reverse_datagram = SynReverseDatagram::clone_from_slice(cmd_body.as_slice()).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
                    let mut write = {
                        let mut inner = self.inner.lock().unwrap();
                        if inner.tunnel_state != TunnelState::Idle {
                            continue;
                        }
                        inner.tunnel_state = TunnelState::Accepting;
                        inner.tunnel_id = reverse_datagram.tunnel_id;
                        let write = inner.write.take().unwrap();
                        write
                    };
                    {
                        let mut inner = self.inner.lock().unwrap();
                        inner.tunnel_state = TunnelState::Worked;
                    }
                    return Ok(TunnelSession::ReverseDatagram(TunnelDatagramSend::first(self.inner.clone(),
                                                                                     read,
                                                                                     write,
                                                                                     reverse_datagram.to_vport,
                                                                                     reverse_datagram.session_id)?));
                }
                _ => {
                    let inner = self.inner.lock().unwrap();
                    return Err(p2p_err!(P2pErrorCode::InvalidData, "tunnel {:?} invalid cmd code {:?}", inner.tunnel_id, cmd_code));
                }
            }
        }
    }
    pub(crate) async fn shutdown(&self) -> P2pResult<()> {
        {
            log::info!("tunnel {:?} shutdown.local {}", {self.inner.lock().unwrap().tunnel_id}, {self.inner.lock().unwrap().local_identity.get_id()});
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

        recv_future.try_complete(Err(p2p_err!(P2pErrorCode::Interrupted, "tunnel {:?} shutdown", self.inner.lock().unwrap().tunnel_id)));
        accept_future.try_complete(Err(p2p_err!(P2pErrorCode::Interrupted, "tunnel {:?} shutdown", self.inner.lock().unwrap().tunnel_id)));

        {
            log::info!("tunnel {:?} shutdowned.local {}", {self.inner.lock().unwrap().tunnel_id}, {self.inner.lock().unwrap().local_identity.get_id()});
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

    pub fn is_error(&self) -> bool {
        let inner = self.inner.lock().unwrap();
        inner.tunnel_state == TunnelState::Error
    }
}

impl Drop for TunnelConnection {
    fn drop(&mut self) {
        log::info!("drop tunnel {:?}", self.inner.lock().unwrap().tunnel_id);
        let _ = Executor::block_on(self.shutdown());
    }

}

pub async fn select_successful<T, E: Debug, F>(futures: Vec<F>) -> P2pResult<T>
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
            (Err(e), _index, remaining) => {
                log::debug!("select failed {:?}", e);
                futures = remaining;
            },
        }
    };
    Err(p2p_err!(P2pErrorCode::ConnectFailed, "connect failed"))
}
