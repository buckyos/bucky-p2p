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
use notify_future::{Notify, NotifyWaiter};
use num_traits::FromPrimitive;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::TcpSocket;
use tokio::time::error::Elapsed;
use crate::endpoint::Endpoint;
use crate::error::{into_p2p_err, p2p_err, P2pErrorCode, P2pResult};
use crate::executor::{Executor, SpawnHandle};
use crate::p2p_connection::{P2pConnection, P2pRead, P2pReadHalf, P2pWrite, P2pWriteHalf};
use crate::p2p_identity::{P2pId, P2pIdentityCertFactoryRef, P2pIdentityRef};
use crate::protocol::{Package, PackageCmdCode, PackageHeader, MTU_LARGE};
use crate::protocol::v0::{AckClose, AckDatagram, AckReverseDatagram, AckReverseSession, AckSession, SynClose, SynDatagram, SynReverseDatagram, SynReverseSession, SynSession, TunnelType};
use crate::runtime;
use crate::sockets::tcp::{TCPConnection};
use crate::types::{SessionId, TunnelId};

pub trait TunnelListenPorts: 'static + Send + Sync {
    fn is_listen(&self, port: u16) -> bool;
}
pub type TunnelListenPortsRef = Arc<dyn TunnelListenPorts>;

#[derive(Copy, Clone, Eq, PartialEq)]
pub enum SocketType {
    TCP,
    UDP
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
    ReadClosed,
    WriteClosed,
    Error,
}

pub enum TunnelSession {
    Forward((SynSession, TunnelConnectionRead, TunnelConnectionWrite)),
    Reverse((SynReverseSession, TunnelConnectionRead, TunnelConnectionWrite)),
}

impl Display for TunnelSession {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            TunnelSession::Forward((session, _, _)) => write!(f, "Forward(seq {:?} session {} port {})", session.tunnel_id, session.session_id, session.to_vport),
            TunnelSession::Reverse((session, _, _)) => write!(f, "Reverse(seq {:?} session {} port {})", session.tunnel_id, session.session_id, session.vport),
        }
    }
}

async fn accept_pkg(mut read: P2pReadHalf) -> P2pResult<(P2pReadHalf, PackageCmdCode, Vec<u8>)> {
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
        if cmd_code == PackageCmdCode::SynSession || cmd_code == PackageCmdCode::AckSession
            || cmd_code == PackageCmdCode::SynReverseSession || cmd_code == PackageCmdCode::AckReverseSession
            || cmd_code == PackageCmdCode::SynClose {
            log::info!("accept first pkg cmd code {:?}", cmd_code);
            break Ok((read, cmd_code, cmd_body));
        } else {
            log::info!("accept error pkg cmd code {:?}, discard", cmd_code);
        }
    }
}

fn create_accept_handle(read: P2pReadHalf,
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

type ReadHolder = ObjectHolder<Option<P2pReadHalf>>;
type WriteHolder = ObjectHolder<Option<P2pWriteHalf>>;

pub struct NotifyHolder<T> {
    id: String,
    future: Mutex<Option<Notify<T>>>
}

impl<T> NotifyHolder<T> {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            id: format!("{}", rand::random::<u64>()),
            future: Mutex::new(None),
        })
    }

    pub fn get_id(&self) -> String {
        self.id.clone()
    }

    pub fn set_future(&self, future: Notify<T>) {
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
        future.notify(result);
        None
    }
}

pub type TunnelFutureHolder = NotifyHolder<P2pResult<(P2pReadHalf, PackageCmdCode, Vec<u8>)>>;
pub type TunnelFutureHolderRef = Arc<NotifyHolder<P2pResult<(P2pReadHalf, PackageCmdCode, Vec<u8>)>>>;

pub(crate) struct TunnelConnectionState {
    tunnel_id: TunnelId,
    tunnel_state: TunnelState,
    accept_handle: Option<SpawnHandle<()>>,
    read: Option<P2pReadHalf>,
    write: Option<P2pWriteHalf>,
}

impl TunnelConnectionState {
    pub(crate) fn new(
        tunnel_id: TunnelId,
        data_socket: P2pConnection,) -> Self {
        let (read, write) = data_socket.split();
        let mut obj = Self {
            tunnel_id,
            tunnel_state: TunnelState::Init,
            accept_handle: None,
            read: Some(read),
            write: Some(write),
        };
        obj
    }
}

#[derive(Clone, Copy, Eq, PartialEq, Debug)]
enum ReadState {
    Work,
    Closed,
    Error,
}
pub struct TunnelConnectionRead {
    conn: Arc<TunnelConnection>,
    local: Endpoint,
    remote: Endpoint,
    local_id: P2pId,
    remote_id: P2pId,
    read: ReadHolder,
    session_id: SessionId,
    vport: u16,
    remainder: u16,
    state: ReadState,
    read_future: Option<Pin<Box<dyn Send + Future<Output=P2pResult<usize>>>>>,
    recv_header_handle: Option<SpawnHandle<P2pResult<(PackageCmdCode, PackageHeader)>>>,
}

impl TunnelConnectionRead {
    pub fn new(conn: Arc<TunnelConnection>,
               read: P2pReadHalf,
               session_id: SessionId,
               vport: u16,) -> Self {
        conn.tunnel_stat.increase_work_instance();
        let mut this = Self {
            conn,
            local: read.local(),
            remote: read.remote(),
            local_id: read.local_id(),
            remote_id: read.remote_id(),
            read: ReadHolder::new(Some(read)),
            session_id,
            vport,
            remainder: 0,
            state: ReadState::Work,
            read_future: None,
            recv_header_handle: None,
        };
        this.enter_recv_header();
        this
    }

    pub fn remote(&self) -> Endpoint {
        self.remote.clone()
    }
    pub fn local(&self) -> Endpoint {
        self.local.clone()
    }
    pub fn remote_id(&self) -> P2pId {
        self.remote_id.clone()
    }
    pub fn local_id(&self) -> P2pId {
        self.local_id.clone()
    }

    fn enter_recv_header(&mut self) -> P2pResult<()> {
        if self.recv_header_handle.is_some() {
            return Ok(());
        }

        let read = self.read.clone();
        let conn = self.conn.clone();
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


    async fn recv_inner(&mut self, buf: &mut [u8]) -> P2pResult<usize> {
        if self.state != ReadState::Work {
            return Ok(0);
        }

        if self.recv_header_handle.is_some() {
            let (cmd_code, header) = self.recv_header_handle.take().unwrap().await.map_err(into_p2p_err!(P2pErrorCode::Failed, "get spawn result"))??;
            if (cmd_code == PackageCmdCode::SynClose) {
                self.state = ReadState::Closed;
                return Ok(0);
            }
            if (cmd_code != PackageCmdCode::PieceData) {
                log::error!("tunnel {:?} invalid cmd code {:?}", self.session_id, cmd_code);
                self.state = ReadState::Error;
                let tunnel_id = self.conn.get_tunnel_id();
                let mut state = self.conn.state.lock().unwrap();
                log::info!("tunnel {:?} state from {:?} to Error", tunnel_id, state.tunnel_state);
                state.tunnel_state = TunnelState::Error;
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
                let ret = self.read.get().await.as_mut().unwrap().read_exact(&mut buf[..self.remainder as usize]).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
                if ret != self.remainder as usize {
                    return Err(p2p_err!(P2pErrorCode::IoError, "read len not match"));
                }
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
                self.state = ReadState::Error;
                let tunnel_id = self.conn.get_tunnel_id();
                let mut state = self.conn.state.lock().unwrap();
                if state.tunnel_state != TunnelState::Idle {
                    log::info!("tunnel {:?} state from {:?} to Error", tunnel_id, state.tunnel_state);
                    state.tunnel_state = TunnelState::Error;
                }
                Err(err)
            }
        }
    }

    fn close(&mut self) {
        let read = self.read.clone();
        let read_state = self.state;
        let conn = self.conn.clone();
        let session_id = self.session_id.clone();
        {
            let mut state = self.conn.state.lock().unwrap();
            if state.tunnel_state != TunnelState::Worked && state.tunnel_state != TunnelState::WriteClosed {
                if state.tunnel_state != TunnelState::Error {
                    let tunnel_id = self.conn.get_tunnel_id();
                    log::error!("tunnel {:?} invalid state {:?}", tunnel_id, state.tunnel_state);
                    state.tunnel_state = TunnelState::Error;
                }
                return;
            }
        }
        let recv_header_handle = self.recv_header_handle.take();
        Executor::spawn_ok(async move {
            if read_state == ReadState::Closed {
                let read = read.get().await.take().unwrap();
                let read = {
                    let tunnel_id = conn.get_tunnel_id();
                    let mut state = conn.state.lock().unwrap();
                    if state.tunnel_state == TunnelState::Worked {
                        log::info!("tunnel {:?} state from {:?} to ReadClosed", tunnel_id, state.tunnel_state);
                        state.tunnel_state = TunnelState::ReadClosed;
                        state.read = Some(read);
                        None
                    } else if state.tunnel_state == TunnelState::WriteClosed {
                        Some(read)
                    } else {
                        None
                    }
                };
                if let Some(read) = read {
                    conn.enter_idle_mode(read);
                }
            } else if read_state == ReadState::Work { // 如果read正在接受数据，则继续接收数据，直到出现错误，或者收到关闭消息
                if recv_header_handle.is_none() {
                    let read = read.get().await.take().unwrap();
                    let tunnel_id = conn.get_tunnel_id();
                    let mut state = conn.state.lock().unwrap();
                    log::info!("tunnel {:?} state from {:?} to Error", tunnel_id, state.tunnel_state);
                    state.tunnel_state = TunnelState::Error;
                    state.read = Some(read);
                } else {
                    match recv_header_handle.unwrap().await {
                        Ok(ret) => {
                            match ret {
                                Ok((cmd_code, header)) => {
                                    if cmd_code == PackageCmdCode::SynClose {
                                        let read = read.get().await.take().unwrap();
                                        let read = {
                                            let tunnel_id = conn.get_tunnel_id();
                                            let mut state = conn.state.lock().unwrap();
                                            if state.tunnel_state == TunnelState::Worked {
                                                log::info!("tunnel {:?} state from {:?} to ReadClosed", tunnel_id, state.tunnel_state);
                                                state.tunnel_state = TunnelState::ReadClosed;
                                                state.read = Some(read);
                                                None
                                            } else if state.tunnel_state == TunnelState::WriteClosed {
                                                Some(read)
                                            } else {
                                                None
                                            }
                                        };
                                        if let Some(read) = read {
                                            conn.enter_idle_mode(read);
                                        }
                                    } if cmd_code == PackageCmdCode::PieceData {
                                        let mut read = read.get().await.take().unwrap();
                                        let out_conn = conn.clone();
                                        let ret: P2pResult<()> = async move {
                                            let mut buf = vec![0u8; header.pkg_len() as usize];
                                            let ret = read.read_exact(buf.as_mut_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
                                            if ret != buf.len() {
                                                return Err(p2p_err!(P2pErrorCode::IoError, "read len not match"));
                                            }

                                            loop {
                                                let mut buf_header = vec![0u8; PackageHeader::raw_bytes().unwrap()];
                                                let ret = read.read_exact(buf_header.as_mut_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
                                                if ret != buf_header.len() {
                                                    break Err(p2p_err!(P2pErrorCode::IoError, "read len not match"));
                                                }

                                                let header = PackageHeader::clone_from_slice(buf_header.as_slice()).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
                                                let cmd_code = match header.cmd_code() {
                                                    Ok(cmd_code) => cmd_code,
                                                    Err(err) => {
                                                        break Err(err);
                                                    }
                                                };

                                                if cmd_code == PackageCmdCode::SynClose {
                                                    let mut buf = vec![0u8; header.pkg_len() as usize];
                                                    let ret = read.read_exact(buf.as_mut_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
                                                    if ret != buf.len() {
                                                        break Err(p2p_err!(P2pErrorCode::IoError, "read len not match"));
                                                    }

                                                    let read = {
                                                        let tunnel_id = conn.get_tunnel_id();
                                                        let mut state = conn.state.lock().unwrap();
                                                        if state.tunnel_state == TunnelState::Worked {
                                                            log::info!("tunnel {:?} state from {:?} to ReadClosed", tunnel_id, state.tunnel_state);
                                                            state.tunnel_state = TunnelState::ReadClosed;
                                                            state.read = Some(read);
                                                            None
                                                        } else if state.tunnel_state == TunnelState::WriteClosed {
                                                            Some(read)
                                                        } else {
                                                            None
                                                        }
                                                    };
                                                    if let Some(read) = read {
                                                        conn.enter_idle_mode(read);
                                                    }
                                                    break Ok(());
                                                } else if cmd_code == PackageCmdCode::PieceData {
                                                    let mut buf = vec![0u8; header.pkg_len() as usize];
                                                    let ret = read.read_exact(buf.as_mut_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
                                                    if ret != buf.len() {
                                                        break Err(p2p_err!(P2pErrorCode::IoError, "read len not match"));
                                                    }
                                                } else {
                                                    break Err(p2p_err!(P2pErrorCode::ErrorState, "tunnel {:?} invalid cmd code {:?}", session_id, cmd_code));
                                                }
                                            }
                                        }.await;
                                        if ret.is_err() {
                                            let tunnel_id = out_conn.get_tunnel_id();
                                            let mut state = out_conn.state.lock().unwrap();
                                            log::info!("tunnel {:?} state from {:?} to Error.err {:?}", tunnel_id, state.tunnel_state, ret.err());
                                            state.tunnel_state = TunnelState::Error;
                                        }
                                    }
                                }
                                Err(e) => {
                                    let read = read.get().await.take().unwrap();
                                    let tunnel_id = conn.get_tunnel_id();
                                    let mut state = conn.state.lock().unwrap();
                                    log::info!("tunnel {:?} state from {:?} to Error", tunnel_id, state.tunnel_state);
                                    state.tunnel_state = TunnelState::Error;
                                    state.read = Some(read);
                                }
                            }
                        }
                        Err(e) => {
                            let read = read.get().await.take().unwrap();
                            let tunnel_id = conn.get_tunnel_id();
                            let mut state = conn.state.lock().unwrap();
                            log::info!("tunnel {:?} state from {:?} to Error", tunnel_id, state.tunnel_state);
                            state.tunnel_state = TunnelState::Error;
                            state.read = Some(read);
                        }
                    }
                }
            }
        });
    }
}

impl Drop for TunnelConnectionRead {
    fn drop(&mut self) {
        self.conn.tunnel_stat.decrease_work_instance();
        self.close();
    }
}

impl runtime::AsyncRead for TunnelConnectionRead {
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

pub struct TunnelConnectionWrite {
    conn: Arc<TunnelConnection>,
    write: Option<P2pWriteHalf>,
    session_id: SessionId,
    vport: u16,
    write_future: Option<Pin<Box<dyn Send + Future<Output=std::io::Result<()>>>>>,
    flush_future: Option<Pin<Box<dyn Send + Future<Output=std::io::Result<()>>>>>,
}

impl TunnelConnectionWrite {
    pub fn new(conn: Arc<TunnelConnection>,
               write: P2pWriteHalf,
               session_id: SessionId,
               vport: u16,) -> Self {
        conn.tunnel_stat.increase_work_instance();
        Self {
            conn,
            write: Some(write),
            session_id,
            vport,
            write_future: None,
            flush_future: None,
        }
    }

    pub fn remote(&self) -> Endpoint {
        self.write.as_ref().unwrap().remote()
    }
    pub fn local(&self) -> Endpoint {
        self.write.as_ref().unwrap().local()
    }
    pub fn remote_id(&self) -> P2pId {
        self.write.as_ref().unwrap().remote_id()
    }
    pub fn local_id(&self) -> P2pId {
        self.write.as_ref().unwrap().local_id()
    }

    async fn send_inner(&mut self, data: &[u8]) -> std::io::Result<()> {
        let mut remainder = data;
        while remainder.len() > 0 {
            let chunk = if remainder.len() > MTU_LARGE as usize {
                &remainder[..MTU_LARGE as usize]
            } else {
                remainder
            };
            let header = PackageHeader::new(self.conn.protocol_version(), PackageCmdCode::PieceData, chunk.len() as u16);
            self.write.as_mut().unwrap().write_all(header.to_vec().unwrap().as_slice()).await?;
            self.write.as_mut().unwrap().write_all(chunk).await?;
            self.write.as_mut().unwrap().flush().await?;
            remainder = &remainder[chunk.len()..];
        }
        Ok(())
    }

    pub(crate) async fn send_pkg<T: RawEncode>(&mut self, pkg: Package<T>) -> P2pResult<()> {
        let data = pkg.to_vec().map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
        self.write.as_mut().unwrap().write_all(data.as_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
        self.write.as_mut().unwrap().flush().await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
        Ok(())
    }

    async fn send(&mut self, data: &[u8]) -> std::io::Result<()> {
        match self.send_inner(data).await {
            Ok(()) => {
                Ok(())
            }
            Err(err) => {
                let tunnel_id = self.conn.get_tunnel_id();
                let mut tunnel = self.conn.state.lock().unwrap();
                if tunnel.tunnel_state != TunnelState::Idle {
                    log::info!("tunnel {:?} state from {:?} to Error", tunnel_id, tunnel.tunnel_state);
                    tunnel.tunnel_state = TunnelState::Error;
                }
                Err(err)
            }
        }
    }

    fn close(&mut self) {
        let mut write = self.write.take().unwrap();
        let tunnel = self.conn.clone();
        let tunnel_id = self.conn.get_tunnel_id();
        let protocol_version = self.conn.protocol_version();
        {
            let mut state = self.conn.state.lock().unwrap();
            if state.tunnel_state != TunnelState::Worked && state.tunnel_state != TunnelState::ReadClosed {
                if state.tunnel_state != TunnelState::Error {
                    log::error!("tunnel {:?} invalid state {:?}", tunnel_id, state.tunnel_state);
                    state.tunnel_state = TunnelState::Error;
                }
                return;
            }
        }
        Executor::spawn_ok(async move {
            let pkg = Package::new(protocol_version, PackageCmdCode::SynClose, SynClose {
                reason: 0,
                tunnel_id,
            });

            let local_id = tunnel.local_identity.get_id();
            let tunnel_tmp = tunnel.clone();
            let ret: P2pResult<()> = async move {
                let tunnel = tunnel_tmp;
                write.write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
                write.flush().await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
                let read = {
                    let mut state = tunnel.state.lock().unwrap();
                    state.write = Some(write);
                    if state.tunnel_state == TunnelState::Worked {
                        log::info!("tunnel {:?} state from {:?} to WriteClosed", tunnel_id, state.tunnel_state);
                        state.tunnel_state = TunnelState::WriteClosed;
                        None
                    } else if state.tunnel_state == TunnelState::ReadClosed {
                        state.read.take()
                    } else {
                        None
                    }
                };
                if let Some(read) = read {
                    tunnel.enter_idle_mode(read);
                }
                Ok(())
            }.await;
            if let Err(err) = ret {
                let mut state = tunnel.state.lock().unwrap();
                state.tunnel_state = TunnelState::Error;
                log::info!("close stream {:?} local_id {} remote_id {} err {}", tunnel_id, local_id.to_string(), tunnel.remote_id.to_string(), err);
            }
        });
    }
}

impl Drop for TunnelConnectionWrite {
    fn drop(&mut self) {
        self.conn.tunnel_stat.decrease_work_instance();
        self.close();
    }
}

impl runtime::AsyncWrite for TunnelConnectionWrite {
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
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        Poll::Ready(Ok(()))
    }
}

pub struct TunnelConnection {
    local_identity: P2pIdentityRef,
    remote_id: P2pId,
    remote_ep: Endpoint,
    conn_timeout: Duration,
    protocol_version: u8,
    state: Mutex<TunnelConnectionState>,
    tunnel_stat: TunnelStatRef,
    cert_factory: P2pIdentityCertFactoryRef,
    accept_future: TunnelFutureHolderRef,
    recv_future: TunnelFutureHolderRef,
}
pub type TunnelConnectionRef = Arc<TunnelConnection>;

impl TunnelConnection {
    pub fn new(
        tunnel_id: TunnelId,
        local_identity: P2pIdentityRef,
        remote_id: P2pId,
        remote_ep: Endpoint,
        conn_timeout: Duration,
        protocol_version: u8,
        tcp_socket: P2pConnection,
        cert_factory: P2pIdentityCertFactoryRef, ) -> Arc<Self> {
        log::info!("create tunnel connection {:?} local_id {} remote_id {} remote_ep {}",
            tunnel_id, local_identity.get_id().to_string(), remote_id.to_string(), remote_ep);
        let accept_future = TunnelFutureHolder::new();
        let recv_future = TunnelFutureHolder::new();
        Arc::new(Self {
            local_identity,
            remote_id,
            remote_ep,
            conn_timeout,
            protocol_version,
            state: Mutex::new(TunnelConnectionState::new(
                tunnel_id,
                tcp_socket,)),
            tunnel_stat: TunnelStat::new(),
            cert_factory,
            accept_future,
            recv_future,
        })
    }

    fn protocol_version(&self) -> u8 {
        self.protocol_version
    }

    pub(crate) fn set_tunnel_state(&self, new_state: TunnelState) {
        let mut state = self.state.lock().unwrap();
        log::info!("tunnel {:?} state from {:?} to {:?}", state.tunnel_id, state.tunnel_state, new_state);
        state.tunnel_state = new_state;
    }

    fn enter_idle_mode(&self, read: P2pReadHalf) -> P2pResult<()> {
        let mut state = self.state.lock().unwrap();
        assert_ne!(state.tunnel_state, TunnelState::Error);
        if state.tunnel_state == TunnelState::Idle {
            return Ok(());
        }
        let handle = create_accept_handle(read, self.accept_future.clone(), self.recv_future.clone())?;
        state.accept_handle = Some(handle);
        state.tunnel_state = TunnelState::Idle;
        log::info!("enter idle mode tunnel {:?} local_id {} remote_id {}",
            state.tunnel_id, self.local_identity.get_id().to_string(), self.remote_id.to_string());
        Ok(())
    }

    fn accept(&self) -> NotifyWaiter<P2pResult<(P2pReadHalf, PackageCmdCode, Vec<u8>)>> {
        let (notify, waiter) = Notify::new();
        self.accept_future.set_future(notify);
        let read = {
            let mut state = self.state.lock().unwrap();
            state.read.take()
        };
        if let Some(read) = read {
            self.enter_idle_mode(read);
        }
        waiter
    }

    fn recv(&self) -> NotifyWaiter<P2pResult<(P2pReadHalf, PackageCmdCode, Vec<u8>)>> {
        let (notify, waiter) = Notify::new();
        self.recv_future.set_future(notify);
        let read = {
            let mut state = self.state.lock().unwrap();
            state.read.take()
        };
        if let Some(read) = read {
            self.enter_idle_mode(read);
        }
        waiter
    }

    pub fn get_tunnel_id(&self) -> TunnelId {
        let inner = self.state.lock().unwrap();
        inner.tunnel_id
    }

    pub(crate) async fn open_reverse_session(self: &Arc<Self>, session_type: TunnelType, vport: u16, session_id: SessionId, result: u8) -> P2pResult<(AckReverseSession, TunnelConnectionRead, TunnelConnectionWrite)> {
        let (tunnel_id, mut write) = {
            let mut inner = self.state.lock().unwrap();
            if inner.tunnel_state != TunnelState::Idle && inner.tunnel_state != TunnelState::Init {
                return Err(p2p_err!(P2pErrorCode::ErrorState, "tunnel {:?} invalid state {:?}", inner.tunnel_id, inner.tunnel_state));
            }

            (inner.tunnel_id, inner.write.take().unwrap())
        };

        let future = self.recv();
        match runtime::timeout(self.conn_timeout, self.open_reverse_session_inner(tunnel_id, session_type, self.protocol_version, &mut write, future, vport, result, session_id.clone())).await {
            Ok(Ok((ack, read))) => {
                let mut inner = self.state.lock().unwrap();
                log::info!("tunnel {:?} state from {:?} to Worked", tunnel_id, inner.tunnel_state);
                inner.tunnel_state = TunnelState::Worked;
                Ok((ack, TunnelConnectionRead::new(self.clone(), read, session_id, vport), TunnelConnectionWrite::new(self.clone(), write, session_id, vport)))
            }
            Ok(Err(err)) => {
                let mut inner = self.state.lock().unwrap();
                log::info!("tunnel {:?} state from {:?} to Error", tunnel_id, inner.tunnel_state);
                inner.tunnel_state = TunnelState::Error;
                Err(err)
            }
            Err(err) => {
                let mut inner = self.state.lock().unwrap();
                log::info!("tunnel {:?} state from {:?} to Error", tunnel_id, inner.tunnel_state);
                inner.tunnel_state = TunnelState::Error;
                Err(p2p_err!(P2pErrorCode::Timeout, "{}", err))
            }
        }
    }

    async fn open_reverse_session_inner(&self,
                                        tunnel_id: TunnelId,
                                        session_type: TunnelType,
                                        protocol_version: u8,
                                        write: &mut P2pWriteHalf,
                                        recv_future: NotifyWaiter<P2pResult<(P2pReadHalf, PackageCmdCode, Vec<u8>)>>,
                                        vport: u16,
                                        result: u8,
                                        session_id: SessionId) -> P2pResult<(AckReverseSession, P2pReadHalf)> {
        let syn = SynReverseSession {
            tunnel_type: session_type,
            tunnel_id,
            session_id,
            vport,
            result,
            payload: Vec::new(),
        };
        let pkg = Package::new(protocol_version, PackageCmdCode::SynReverseSession, syn);

        write.write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
        write.flush().await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
        let (read, cmd_code, cmd_body) = recv_future.await?;

        if cmd_code != PackageCmdCode::AckReverseSession {
            return Err(p2p_err!(P2pErrorCode::InvalidData, "tunnel {:?} invalid ack stream", tunnel_id));
        }

        let ack = AckReverseSession::clone_from_slice(cmd_body.as_slice()).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
        Ok((ack, read))
    }

    pub(crate) async fn open_session(self: &Arc<Self>, session_type: TunnelType, vport: u16, session_id: SessionId) -> P2pResult<(AckSession, TunnelConnectionRead, TunnelConnectionWrite)> {
        let (tunnel_id, mut write) = {
            let mut inner = self.state.lock().unwrap();
            if inner.tunnel_state != TunnelState::Idle && inner.tunnel_state != TunnelState::Init {
                return Err(p2p_err!(P2pErrorCode::ErrorState, "tunnel {:?} invalid state {:?}", inner.tunnel_id, inner.tunnel_state));
            }
            (inner.tunnel_id, inner.write.take().unwrap())
        };

        let future = self.recv();
        match runtime::timeout(self.conn_timeout, self.open_session_inner(tunnel_id, session_type, self.protocol_version, &mut write, future, vport, session_id.clone())).await {
            Ok(Ok((ack, read))) => {
                {
                    let mut inner = self.state.lock().unwrap();
                    log::info!("tunnel {:?} state from {:?} to Worked", tunnel_id, inner.tunnel_state);
                    inner.tunnel_state = TunnelState::Worked;
                };
                Ok((ack, TunnelConnectionRead::new(self.clone(), read, session_id, vport), TunnelConnectionWrite::new(self.clone(), write, session_id, vport)))
            }
            Ok(Err(err)) => {
                let mut inner = self.state.lock().unwrap();
                log::info!("tunnel {:?} state from {:?} to Error", tunnel_id, inner.tunnel_state);
                inner.tunnel_state = TunnelState::Error;
                Err(err)
            }
            Err(err) => {
                let mut inner = self.state.lock().unwrap();
                log::info!("tunnel {:?} state from {:?} to Error", tunnel_id, inner.tunnel_state);
                inner.tunnel_state = TunnelState::Error;
                Err(p2p_err!(P2pErrorCode::Timeout, "{}", err))
            }
        }
    }


    async fn open_session_inner(&self,
                                tunnel_id: TunnelId,
                                session_type: TunnelType,
                                protocol_version: u8,
                                write: &mut P2pWriteHalf,
                                recv_future: NotifyWaiter<P2pResult<(P2pReadHalf, PackageCmdCode, Vec<u8>)>>,
                                vport: u16,
                                session_id: SessionId) -> P2pResult<(AckSession, P2pReadHalf)> {
        let syn = SynSession {
            tunnel_type: session_type,
            tunnel_id,
            to_vport: vport,
            session_id,
            payload: Vec::new(),
        };
        let pkg = Package::new(protocol_version, PackageCmdCode::SynSession, syn);

        write.write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
        write.flush().await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
        let (read, cmd_code, cmd_body) = recv_future.await?;

        if cmd_code != PackageCmdCode::AckSession {
            return Err(p2p_err!(P2pErrorCode::InvalidData, "tunnel {:?} invalid ack stream", tunnel_id));
        }

        let ack = AckSession::clone_from_slice(cmd_body.as_slice()).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
        Ok((ack, read))
    }

    pub(crate) async fn accept_session(self: &Arc<Self>) -> P2pResult<TunnelSession> {
        loop {
            let (read, cmd_code, cmd_body) = self.accept().await?;
            match cmd_code {
                PackageCmdCode::SynSession => {
                    let syn_stream = SynSession::clone_from_slice(cmd_body.as_slice()).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
                    let vport = syn_stream.to_vport;
                    let session_id = syn_stream.session_id;
                    let mut write = {
                        let mut inner = self.state.lock().unwrap();
                        if inner.tunnel_state != TunnelState::Idle {
                            continue;
                        }
                        log::info!("tunnel {:?} state from {:?} to Accepting", inner.tunnel_id, inner.tunnel_state);
                        inner.tunnel_state = TunnelState::Accepting;
                        inner.tunnel_id = syn_stream.tunnel_id;
                        let write = inner.write.take().unwrap();
                        write
                    };
                    return Ok(TunnelSession::Forward((syn_stream, TunnelConnectionRead::new(self.clone(), read, session_id, vport), TunnelConnectionWrite::new(self.clone(), write, session_id, vport))));
                }
                PackageCmdCode::SynReverseSession => {
                    let reserve_stream = SynReverseSession::clone_from_slice(cmd_body.as_slice()).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
                    let vport = reserve_stream.vport;
                    let session_id = reserve_stream.session_id;
                    let mut write = {
                        let mut inner = self.state.lock().unwrap();
                        if inner.tunnel_state != TunnelState::Idle {
                            continue;
                        }
                        log::info!("tunnel {:?} state from {:?} to Accepting", inner.tunnel_id, inner.tunnel_state);
                        inner.tunnel_state = TunnelState::Accepting;
                        inner.tunnel_id = reserve_stream.tunnel_id;
                        let write = inner.write.take().unwrap();
                        write
                    };
                    return Ok(TunnelSession::Reverse((reserve_stream, TunnelConnectionRead::new(self.clone(), read, session_id, vport), TunnelConnectionWrite::new(self.clone(), write, session_id, vport))));
                }
                _ => {
                    let inner = self.state.lock().unwrap();
                    return Err(p2p_err!(P2pErrorCode::InvalidData, "tunnel {:?} invalid cmd code {:?}", inner.tunnel_id, cmd_code));
                }
            }
        }
    }
    pub(crate) async fn shutdown(&self) -> P2pResult<()> {
        let (tunnel_id, accept_handle) = {
            let mut inner = self.state.lock().unwrap();
            log::info!("tunnel {:?} shutdown.local {}", inner.tunnel_id, self.local_identity.get_id());
            if inner.tunnel_state != TunnelState::Idle {
                return Ok(());
            }
            (inner.tunnel_id, inner.accept_handle.take())
        };

        if accept_handle.is_some() {
            accept_handle.unwrap().abort();
        }

        self.recv_future.try_complete(Err(p2p_err!(P2pErrorCode::Interrupted, "tunnel {:?} shutdown", tunnel_id)));
        self.accept_future.try_complete(Err(p2p_err!(P2pErrorCode::Interrupted, "tunnel {:?} shutdown", tunnel_id)));

        {
            log::info!("tunnel {:?} shutdowned.local {}", tunnel_id, self.local_identity.get_id());
        }
        // inner.data_socket.as_ref().unwrap().shutdown().await?;
        Ok(())
    }

    pub fn tunnel_stat(&self) -> TunnelStatRef {
        self.tunnel_stat.clone()
    }

    pub fn is_idle(&self) -> bool {
        let inner = self.state.lock().unwrap();
        log::trace!("tunnel {:?} state {:?}", inner.tunnel_id, inner.tunnel_state);
        inner.tunnel_state == TunnelState::Idle
    }

    pub fn is_error(&self) -> bool {
        let inner = self.state.lock().unwrap();
        inner.tunnel_state == TunnelState::Error
    }

    pub fn is_work(&self) -> bool {
        let inner = self.state.lock().unwrap();
        inner.tunnel_state == TunnelState::Worked
    }
}

impl Drop for TunnelConnection {
    fn drop(&mut self) {
        {
            log::info!("drop tunnel connection {:?}", self.state.lock().unwrap().tunnel_id);
        }
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
                log::trace!("select failed {:?}", e);
                futures = remaining;
            },
        }
    };
    Err(p2p_err!(P2pErrorCode::ConnectFailed, "connect failed"))
}
