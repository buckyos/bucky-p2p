use std::collections::HashSet;
use std::future::Future;
use std::io::{Error, ErrorKind};
use std::net::Shutdown;
use std::ops::{Deref, DerefMut};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;
use bucky_objects::{DeviceDesc, DeviceId, Endpoint};
use bucky_raw_codec::{CodecResult, RawConvertTo, RawEncode, RawFixedBytes, RawFrom, TailedOwnedData};
use bucky_time::bucky_time_now;
use notify_future::NotifyFuture;
use num_traits::FromPrimitive;
use rustls::internal::msgs::handshake::SessionId;
use runtime::{AsyncRead, AsyncWrite, AsyncWriteExt, AsyncReadExt, ReadBuf};
use crate::sockets::{NetManagerRef};
use crate::{IncreaseId, LocalDeviceRef, MixAesKey, runtime, TempSeq};
use crate::error::{bdt_err, BdtErrorCode, BdtResult, into_bdt_err};
use crate::executor::{Executor, SpawnHandle};
use crate::history::keystore::{EncryptedKey, Keystore};
use crate::protocol::{AckTunnel, Package, MTU, MTU_LARGE, PackageCmdCode, SynTunnel, PackageHeader};
use crate::protocol::v0::{AckStream, SynStream, SynDatagram, AckDatagram, SynClose, AckClose, SynReverseStream, AckReverseStream, AckReverseDatagram, SynReverseDatagram};
use crate::receive_processor::{ReceiveProcessorRef};
use crate::sockets::tcp::{TCPRead, TCPSocket, TCPWrite};
use crate::tunnel::{SocketType, TunnelConnection, TunnelDatagramRecv, TunnelDatagramSend, TunnelInstance, TunnelListenPortsRef, TunnelStat, TunnelStatRef, TunnelStream, TunnelType};
use crate::tunnel::TunnelInstance::ReverseDatagram;

#[derive(Debug, Clone, Eq, PartialEq)]
enum TunnelState {
    Init,
    Idle,
    Opening,
    Worked,
    Accepting,
    Error,
}

pub struct TunnelStateHold {
    state: Mutex<TunnelState>,
}
pub type TunnelStateHoldRef = Arc<TunnelStateHold>;

impl TunnelStateHold {
    pub fn new(state: TunnelState) -> Self {
        Self {
            state: Mutex::new(state),
        }
    }

    pub fn set(&self, state: TunnelState) {
        *self.state.lock().unwrap() = state;
    }

    pub fn get(&self) -> TunnelState {
        self.state.lock().unwrap().clone()
    }
}

pub struct AcceptHandleHold {
    handle: Mutex<Option<Arc<SpawnHandle<BdtResult<(TCPRead, PackageCmdCode, Vec<u8>)>>>>>,
}
pub type AcceptHandleHoldRef = Arc<AcceptHandleHold>;

impl AcceptHandleHold {
    pub fn new() -> Self {
        Self {
            handle: Mutex::new(None),
        }
    }

    pub fn set(&self, handle: SpawnHandle<BdtResult<(TCPRead, PackageCmdCode, Vec<u8>)>>) {
        *self.handle.lock().unwrap() = Some(Arc::new(handle));
    }

    pub fn take(&self) -> Option<Arc<SpawnHandle<BdtResult<(TCPRead, PackageCmdCode, Vec<u8>)>>>> {
        self.handle.lock().unwrap().take()
    }

    pub fn get(&self) -> Option<Arc<SpawnHandle<BdtResult<(TCPRead, PackageCmdCode, Vec<u8>)>>>> {
        self.handle.lock().unwrap().clone()
    }
}
async fn accept_first_pkg(mut read: TCPRead) -> BdtResult<(TCPRead, PackageCmdCode, Vec<u8>)> {
    let mut buf_header = [0u8; 16];
    read.read_exact(&mut buf_header[0..PackageHeader::raw_bytes().unwrap()]).await.map_err(into_bdt_err!(BdtErrorCode::IoError))?;
    let header = PackageHeader::clone_from_slice(buf_header.as_slice()).map_err(into_bdt_err!(BdtErrorCode::RawCodecError))?;
    let cmd_code = match header.cmd_code() {
        Ok(cmd_code) => cmd_code,
        Err(err) => {
            return Err(err);
        }
    };
    let mut cmd_body = vec![0u8; header.pkg_len() as usize];
    read.read_exact(cmd_body.as_mut_slice()).await.map_err(into_bdt_err!(BdtErrorCode::IoError))?;
    log::info!("accept first pkg cmd code {:?}", cmd_code);
    Ok((read, cmd_code, cmd_body))
}

fn create_accept_handle(read: TCPRead,
                        accept_future: Arc<Mutex<Option<NotifyFuture<BdtResult<(TCPRead, PackageCmdCode, Vec<u8>)>>>>>,
                        recv_future: Arc<Mutex<Option<NotifyFuture<BdtResult<(TCPRead, PackageCmdCode, Vec<u8>)>>>>>
) -> BdtResult<SpawnHandle<()>> {
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
                    accept_future.take().unwrap().set_complete(Err(bdt_err!(BdtErrorCode::Interrupted, "accept handle abort")));
                }
            }
        }
    })
}

pub struct TcpTunnelConnectionImpl {
    sequence: TempSeq,
    local_device: LocalDeviceRef,
    remote_id: DeviceId,
    remote_ep: Endpoint,
    conn_timeout: Duration,
    data_socket: Option<Arc<TCPSocket>>,
    protocol_version: u8,
    stack_version: u32,
    tunnel_state: TunnelState,
    lister_ports: TunnelListenPortsRef,
    accept_handle: Option<SpawnHandle<()>>,
    write: Option<TCPWrite>,
    accept_future: Arc<Mutex<Option<NotifyFuture<BdtResult<(TCPRead, PackageCmdCode, Vec<u8>)>>>>>,
    recv_future: Arc<Mutex<Option<NotifyFuture<BdtResult<(TCPRead, PackageCmdCode, Vec<u8>)>>>>>,
    tunnel_stat: TunnelStatRef,
}

impl TcpTunnelConnectionImpl {
    pub(crate) fn new(
        sequence: TempSeq,
        local_device: LocalDeviceRef,
        remote_id: DeviceId,
        remote_ep: Endpoint,
        conn_timeout: Duration,
        protocol_version: u8,
        stack_version: u32,
        mut tcp_socket: Option<TCPSocket>,
        lister_ports: TunnelListenPortsRef, ) -> BdtResult<Self> {
        let mut obj = Self {
            sequence,
            local_device,
            remote_id,
            remote_ep,
            conn_timeout,
            data_socket: tcp_socket.map(|socket| Arc::new(socket)),
            protocol_version,
            stack_version,
            tunnel_state: TunnelState::Init,
            lister_ports,
            accept_handle: None,
            write: None,
            accept_future: Arc::new(Mutex::new(None)),
            recv_future: Arc::new(Mutex::new(None)),
            tunnel_stat: TunnelStat::new(),
        };
        obj.enter_idle_mode()?;
        Ok(obj)
    }

    fn enter_idle_mode(&mut self) -> BdtResult<()> {
        if self.data_socket.is_some() && !self.data_socket.as_ref().unwrap().is_split() {
            let (read, write) = self.data_socket.as_ref().unwrap().split()?;
            let handle = create_accept_handle(read, self.recv_future.clone(), self.accept_future.clone())?;
            self.write = Some(write);
            self.accept_handle = Some(handle);
            self.tunnel_state = TunnelState::Idle;
        }
        Ok(())
    }



    async fn open_stream_inner(sequence: TempSeq,
                               write: &mut TCPWrite,
                               recv_future: NotifyFuture<BdtResult<(TCPRead, PackageCmdCode, Vec<u8>)>>,
                               vport: u16,
                               session_id: IncreaseId) -> BdtResult<TCPRead> {
        let syn = SynStream {
            sequence,
            to_vport: vport,
            session_id,
            payload: Vec::new(),
        };
        let pkg = Package::new(PackageCmdCode::SynStream, syn);

        write.write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_bdt_err!(BdtErrorCode::IoError))?;
        let (read, cmd_code, cmd_body) = recv_future.await?;

        if cmd_code != PackageCmdCode::AckStream {
            return Err(bdt_err!(BdtErrorCode::InvalidData, "tunnel {:?} invalid ack stream", sequence));
        }

        let ack = AckStream::clone_from_slice(cmd_body.as_slice()).map_err(into_bdt_err!(BdtErrorCode::RawCodecError))?;
        if ack.result != 0 {
            return Err(bdt_err!(BdtErrorCode::ConnectionRefused, "tunnel {:?} open stream failed. return {}", sequence, ack.result));
        }

        Ok(read)
    }

    async fn open_reverse_stream_inner(sequence: TempSeq,
                                       write: &mut TCPWrite,
                                       recv_future: NotifyFuture<BdtResult<(TCPRead, PackageCmdCode, Vec<u8>)>>,
                                       vport: u16,
                                       session_id: IncreaseId) -> BdtResult<TCPRead> {
        let syn = SynReverseStream {
            sequence,
            session_id,
            vport,
            payload: Vec::new(),
        };
        let pkg = Package::new(PackageCmdCode::SynReverseStream, syn);

        write.write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_bdt_err!(BdtErrorCode::IoError))?;
        let (read, cmd_code, cmd_body) = recv_future.await?;

        if cmd_code != PackageCmdCode::AckReverseStream {
            return Err(bdt_err!(BdtErrorCode::InvalidData, "tunnel {:?} invalid ack stream", sequence));
        }

        let ack = AckReverseStream::clone_from_slice(cmd_body.as_slice()).map_err(into_bdt_err!(BdtErrorCode::RawCodecError))?;
        if ack.result != 0 {
            return Err(bdt_err!(BdtErrorCode::ConnectionRefused, "tunnel {:?} open stream failed. return {}", sequence, ack.result));
        }

        Ok(read)
    }

    async fn open_datagram_inner(sequence: TempSeq,
                                 write: &mut TCPWrite,
                                 recv_future: NotifyFuture<BdtResult<(TCPRead, PackageCmdCode, Vec<u8>)>>,) -> BdtResult<TCPRead> {
        let syn = SynDatagram {
            sequence: sequence,
            payload: Vec::new(),
        };
        let pkg = Package::new(PackageCmdCode::SynDatagram, syn);

        write.write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_bdt_err!(BdtErrorCode::IoError))?;
        let (read, cmd_code, cmd_body) = recv_future.await?;

        if cmd_code != PackageCmdCode::AckDatagram {
            return Err(bdt_err!(BdtErrorCode::InvalidData, "tunnel {:?} invalid ack stream", sequence));
        }

        let ack = AckDatagram::clone_from_slice(cmd_body.as_slice()).map_err(into_bdt_err!(BdtErrorCode::RawCodecError))?;
        if ack.result != 0 {
            return Err(bdt_err!(BdtErrorCode::ConnectionRefused, "tunnel {:?} open stream failed. return {}", sequence, ack.result));
        }

        Ok(read)
    }

    async fn open_reverse_datagram_inner(sequence: TempSeq,
                                 write: &mut TCPWrite,
                                 recv_future: NotifyFuture<BdtResult<(TCPRead, PackageCmdCode, Vec<u8>)>>,) -> BdtResult<TCPRead> {
        let syn = SynReverseDatagram {
            sequence,
            payload: Vec::new(),
        };
        let pkg = Package::new(PackageCmdCode::SynReverseDatagram, syn);

        write.write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_bdt_err!(BdtErrorCode::IoError))?;
        let (read, cmd_code, cmd_body) = recv_future.await?;

        if cmd_code != PackageCmdCode::AckReverseDatagram {
            return Err(bdt_err!(BdtErrorCode::InvalidData, "tunnel {:?} invalid ack stream", sequence));
        }

        let ack = AckReverseDatagram::clone_from_slice(cmd_body.as_slice()).map_err(into_bdt_err!(BdtErrorCode::RawCodecError))?;
        if ack.result != 0 {
            return Err(bdt_err!(BdtErrorCode::ConnectionRefused, "tunnel {:?} open stream failed. return {}", sequence, ack.result));
        }

        Ok(read)
    }
}

pub struct TcpTunnelConnection {
    inner: Arc<Mutex<TcpTunnelConnectionImpl>>
}

impl TcpTunnelConnection {
    pub(crate) fn new(
        sequence: TempSeq,
        local_device: LocalDeviceRef,
        remote_id: DeviceId,
        remote_ep: Endpoint,
        conn_timeout: Duration,
        protocol_version: u8,
        stack_version: u32,
        mut tcp_socket: Option<TCPSocket>,
        lister_ports: TunnelListenPortsRef, ) -> BdtResult<Self> {
        Ok(Self {
            inner: Arc::new(Mutex::new(TcpTunnelConnectionImpl::new(sequence,
                                                                    local_device,
                                                                    remote_id,
                                                                    remote_ep,
                                                                    conn_timeout,
                                                                    protocol_version,
                                                                    stack_version,
                                                                    tcp_socket,
                                                                    lister_ports)?))
        })
    }

    async fn open_reverse_stream(&self, vport: u16, session_id: IncreaseId) -> BdtResult<Box<dyn TunnelStream>> {
        let (conn_timeout, sequence, mut write, recv_future) = {
            let mut inner = self.inner.lock().unwrap();
            if inner.tunnel_state != TunnelState::Idle {
                return Err(bdt_err!(BdtErrorCode::ErrorState, "tunnel {:?} invalid state {:?}", inner.sequence, inner.tunnel_state));
            }
            inner.tunnel_state = TunnelState::Opening;

            let future = NotifyFuture::<BdtResult<(TCPRead, PackageCmdCode, Vec<u8>)>>::new();
            {
                let mut recv_future = inner.recv_future.lock().unwrap();
                *recv_future = Some(future.clone());
            }

            (inner.conn_timeout, inner.sequence, inner.write.take().unwrap(), future)
        };
        match runtime::timeout(conn_timeout, TcpTunnelConnectionImpl::open_reverse_stream_inner(sequence, &mut write, recv_future, vport, session_id.clone())).await {
            Ok(Ok(read)) => {
                let mut inner = self.inner.lock().unwrap();
                inner.tunnel_state = TunnelState::Worked;
                Ok(Box::new(TcpTunnelStream::new(self.inner.clone(), read, write, session_id, vport)))
            }
            Ok(Err(err)) => {
                let mut inner = self.inner.lock().unwrap();
                inner.tunnel_state = TunnelState::Error;
                Err(err)
            }
            Err(err) => {
                let mut inner = self.inner.lock().unwrap();
                inner.tunnel_state = TunnelState::Error;
                Err(bdt_err!(BdtErrorCode::Timeout, "{}", err))
            }
        }
    }

    async fn open_reverse_datagram(&self) -> BdtResult<Box<dyn TunnelDatagramRecv>> {
        let (conn_timeout, sequence, mut write, recv_future) = {
            let mut inner = self.inner.lock().unwrap();
            if inner.tunnel_state != TunnelState::Idle {
                return Err(bdt_err!(BdtErrorCode::ErrorState, "tunnel {:?} invalid state {:?}", inner.sequence, inner.tunnel_state));
            }
            inner.tunnel_state = TunnelState::Opening;
            let future = NotifyFuture::<BdtResult<(TCPRead, PackageCmdCode, Vec<u8>)>>::new();
            {
                let mut recv_future = inner.recv_future.lock().unwrap();
                *recv_future = Some(future.clone());
            }
            (inner.conn_timeout, inner.sequence, inner.write.take().unwrap(), future)
        };
        match runtime::timeout(conn_timeout, TcpTunnelConnectionImpl::open_reverse_datagram_inner(sequence, &mut write, recv_future)).await {
            Ok(Ok(read)) => {
                let mut inner = self.inner.lock().unwrap();
                inner.tunnel_state = TunnelState::Worked;
                Ok(Box::new(TcpTunnelDatagramRecv::new(self.inner.clone(), read, write)))
            }
            Ok(Err(err)) => {
                let mut inner = self.inner.lock().unwrap();
                inner.tunnel_state = TunnelState::Error;
                Err(err)
            }
            Err(err) => {
                let mut inner = self.inner.lock().unwrap();
                inner.tunnel_state = TunnelState::Error;
                Err(bdt_err!(BdtErrorCode::Timeout, "{}", err))
            }
        }
    }

}

#[async_trait::async_trait]
impl TunnelConnection for TcpTunnelConnection {
    fn get_sequence(&self) -> TempSeq {
        let inner = self.inner.lock().unwrap();
        inner.sequence
    }

    fn socket_type(&self) -> SocketType {
        SocketType::TCP
    }

    fn is_idle(&self) -> bool {
        let inner = self.inner.lock().unwrap();
        inner.tunnel_state == TunnelState::Idle
    }

    fn tunnel_stat(&self) -> TunnelStatRef {
        let inner = self.inner.lock().unwrap();
        inner.tunnel_stat.clone()
    }

    async fn connect_stream(&self, vport: u16, session_id: IncreaseId) -> BdtResult<Box<dyn TunnelStream>> {
        let (has_socket, local_device, remote_ep, remote_id, conn_timeout) = {
            let mut inner = self.inner.lock().unwrap();
            (inner.data_socket.is_some(), inner.local_device.clone(), inner.remote_ep.clone(), inner.remote_id.clone(), inner.conn_timeout)
        };
        if has_socket {
            return Ok(self.open_stream(vport, session_id).await?);
        }
        let mut tcp_socket = TCPSocket::connect(local_device, remote_ep, remote_id, conn_timeout).await?;

        {
            let mut inner = self.inner.lock().unwrap();
            inner.data_socket = Some(Arc::new(tcp_socket));
            inner.enter_idle_mode()?;
        }

        Ok(self.open_stream(vport, session_id).await?)
    }

    async fn connect_datagram(&self) -> BdtResult<Box<dyn TunnelDatagramSend>> {
        let (has_socket, local_device, remote_ep, remote_id, conn_timeout) = {
            let inner = self.inner.lock().unwrap();
            (inner.data_socket.is_some(), inner.local_device.clone(), inner.remote_ep.clone(), inner.remote_id.clone(), inner.conn_timeout)
        };

        if has_socket {
            return Ok(self.open_datagram().await?);
        }

        let mut tcp_socket = TCPSocket::connect(local_device, remote_ep, remote_id, conn_timeout).await?;

        {
            let mut inner = self.inner.lock().unwrap();
            inner.data_socket = Some(Arc::new(tcp_socket));
            inner.enter_idle_mode()?;
        }

        Ok(self.open_datagram().await?)
    }

    async fn connect_reverse_stream(&self, vport: u16, session_id: IncreaseId) -> BdtResult<Box<dyn TunnelStream>> {
        let (has_socket, local_device, remote_ep, remote_id, conn_timeout) = {
            let mut inner = self.inner.lock().unwrap();
            (inner.data_socket.is_some(), inner.local_device.clone(), inner.remote_ep.clone(), inner.remote_id.clone(), inner.conn_timeout)
        };

        if has_socket {
            return Ok(self.open_reverse_stream(vport, session_id).await?);
        }

        let mut tcp_socket = TCPSocket::connect(local_device, remote_ep, remote_id, conn_timeout).await?;

        {
            let mut inner = self.inner.lock().unwrap();
            inner.data_socket = Some(Arc::new(tcp_socket));
            inner.enter_idle_mode()?;
        }

        Ok(self.open_reverse_stream(vport, session_id).await?)
    }

    async fn connect_reverse_datagram(&self) -> BdtResult<Box<dyn TunnelDatagramRecv>> {
        let (has_socket, local_device, remote_ep, remote_id, conn_timeout) = {
            let mut inner = self.inner.lock().unwrap();
            (inner.data_socket.is_some(), inner.local_device.clone(), inner.remote_ep.clone(), inner.remote_id.clone(), inner.conn_timeout)
        };

        if has_socket {
            return Ok(self.open_reverse_datagram().await?);
        }

        let mut tcp_socket = TCPSocket::connect(local_device, remote_ep, remote_id, conn_timeout).await?;

        {
            let mut inner = self.inner.lock().unwrap();
            inner.data_socket = Some(Arc::new(tcp_socket));
            inner.enter_idle_mode()?;
        }

        Ok(self.open_reverse_datagram().await?)
    }

    async fn open_stream(&self, vport: u16, session_id: IncreaseId) -> BdtResult<Box<dyn TunnelStream>> {
        let (conn_timeout, sequence, mut write, recv_future) = {
            let mut inner = self.inner.lock().unwrap();
            if inner.tunnel_state != TunnelState::Idle {
                return Err(bdt_err!(BdtErrorCode::ErrorState, "tunnel {:?} invalid state {:?}", inner.sequence, inner.tunnel_state));
            }
            inner.tunnel_state = TunnelState::Opening;

            let future = NotifyFuture::<BdtResult<(TCPRead, PackageCmdCode, Vec<u8>)>>::new();
            {
                let mut recv_future = inner.recv_future.lock().unwrap();
                *recv_future = Some(future.clone());
            }

            (inner.conn_timeout, inner.sequence, inner.write.take().unwrap(), future)
        };
        match runtime::timeout(conn_timeout, TcpTunnelConnectionImpl::open_stream_inner(sequence, &mut write, recv_future, vport, session_id.clone())).await {
            Ok(Ok(read)) => {
                let mut inner = self.inner.lock().unwrap();
                inner.tunnel_state = TunnelState::Worked;
                Ok(Box::new(TcpTunnelStream::new(self.inner.clone(), read, write, session_id, vport)))
            }
            Ok(Err(err)) => {
                let mut inner = self.inner.lock().unwrap();
                inner.tunnel_state = TunnelState::Error;
                Err(err)
            }
            Err(err) => {
                let mut inner = self.inner.lock().unwrap();
                inner.tunnel_state = TunnelState::Error;
                Err(bdt_err!(BdtErrorCode::Timeout, "{}", err))
            }
        }
    }

    async fn open_datagram(&self) -> BdtResult<Box<dyn TunnelDatagramSend>> {
        let (conn_timeout, sequence, mut write, recv_future) = {
            let mut inner = self.inner.lock().unwrap();
            if inner.tunnel_state != TunnelState::Idle {
                return Err(bdt_err!(BdtErrorCode::ErrorState, "tunnel {:?} invalid state {:?}", inner.sequence, inner.tunnel_state));
            }
            inner.tunnel_state = TunnelState::Opening;
            let future = NotifyFuture::<BdtResult<(TCPRead, PackageCmdCode, Vec<u8>)>>::new();
            {
                let mut recv_future = inner.recv_future.lock().unwrap();
                *recv_future = Some(future.clone());
            }
            (inner.conn_timeout, inner.sequence, inner.write.take().unwrap(), future)
        };
        match runtime::timeout(conn_timeout, TcpTunnelConnectionImpl::open_datagram_inner(sequence, &mut write, recv_future)).await {
            Ok(Ok(read)) => {
                let mut inner = self.inner.lock().unwrap();
                inner.tunnel_state = TunnelState::Worked;
                Ok(Box::new(TcpTunnelDatagramSend::new(self.inner.clone(), read, write)))
            }
            Ok(Err(err)) => {
                let mut inner = self.inner.lock().unwrap();
                inner.tunnel_state = TunnelState::Error;
                Err(err)
            }
            Err(err) => {
                let mut inner = self.inner.lock().unwrap();
                inner.tunnel_state = TunnelState::Error;
                Err(bdt_err!(BdtErrorCode::Timeout, "{}", err))
            }
        }
    }

    async fn accept_instance(&self) -> BdtResult<TunnelInstance> {
        loop {
            let future = NotifyFuture::new();
            {
                let mut inner = self.inner.lock().unwrap();
                let mut accept_future = inner.accept_future.lock().unwrap();
                *accept_future = Some(future.clone());
            };

            let (read, cmd_code, cmd_body) = future.await?;
            match cmd_code {
                PackageCmdCode::SynStream => {
                    let syn_stream = SynStream::clone_from_slice(cmd_body.as_slice()).map_err(into_bdt_err!(BdtErrorCode::RawCodecError))?;
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
                    write.write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_bdt_err!(BdtErrorCode::IoError))?;
                    {
                        let mut inner = self.inner.lock().unwrap();
                        inner.tunnel_state = TunnelState::Worked;
                    }
                    return Ok(TunnelInstance::Stream(Box::new(TcpTunnelStream::new(self.inner.clone(), read, write, session_id, vport))))
                }
                PackageCmdCode::SynReverseStream => {
                    let reserve_stream = SynReverseStream::clone_from_slice(cmd_body.as_slice()).map_err(into_bdt_err!(BdtErrorCode::RawCodecError))?;
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
                    write.write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_bdt_err!(BdtErrorCode::IoError))?;
                    {
                        let mut inner = self.inner.lock().unwrap();
                        inner.tunnel_state = TunnelState::Worked;
                    }
                    return Ok(TunnelInstance::ReverseStream(Box::new(TcpTunnelStream::new(self.inner.clone(), read, write, reserve_stream.session_id, 0))));
                }
                PackageCmdCode::SynDatagram => {
                    let syn_datagram = SynDatagram::clone_from_slice(cmd_body.as_slice()).map_err(into_bdt_err!(BdtErrorCode::RawCodecError))?;
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
                    write.write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_bdt_err!(BdtErrorCode::IoError))?;
                    {
                        let mut inner = self.inner.lock().unwrap();
                        inner.tunnel_state = TunnelState::Worked;
                    }
                    return Ok(TunnelInstance::Datagram(Box::new(TcpTunnelDatagramRecv::new(self.inner.clone(), read, write))))
                }
                PackageCmdCode::SynReverseDatagram => {
                    let reverse_datagram = SynReverseDatagram::clone_from_slice(cmd_body.as_slice()).map_err(into_bdt_err!(BdtErrorCode::RawCodecError))?;
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
                    write.write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_bdt_err!(BdtErrorCode::IoError))?;
                    {
                        let mut inner = self.inner.lock().unwrap();
                        inner.tunnel_state = TunnelState::Worked;
                    }
                    return Ok(TunnelInstance::ReverseDatagram(Box::new(TcpTunnelDatagramSend::new(self.inner.clone(), read, write))));
                }
                _ => {
                    let inner = self.inner.lock().unwrap();
                    return Err(bdt_err!(BdtErrorCode::InvalidData, "tunnel {:?} invalid cmd code {:?}", inner.sequence, cmd_code));
                }
            }
        }
    }

    async fn shutdown(&self) -> BdtResult<()> {
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
                future.take().unwrap().set_complete(Err(bdt_err!(BdtErrorCode::Interrupted, "tunnel {:?} shutdown", self.inner.lock().unwrap().sequence)));
            }
        }
        {
            let mut future = accept_future.lock().unwrap();
            if future.is_some() {
                future.take().unwrap().set_complete(Err(bdt_err!(BdtErrorCode::Interrupted, "tunnel {:?} shutdown", self.inner.lock().unwrap().sequence)));

            }
        }
        // inner.data_socket.as_ref().unwrap().shutdown().await?;
        Ok(())
    }

}

impl Drop for TcpTunnelConnection {
    fn drop(&mut self) {
        log::info!("drop tunnel {:?}", self.inner.lock().unwrap().sequence);
        Executor::block_on(self.shutdown());
    }

}

impl runtime::AsyncRead for TcpTunnelStream {
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

impl runtime::AsyncWrite for TcpTunnelStream {
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

pub struct TcpTunnelStream {
    tunnel: Arc<Mutex<TcpTunnelConnectionImpl>>,
    session_id: IncreaseId,
    port: u16,
    read: Option<TCPRead>,
    write: Option<TCPWrite>,
    remainder: u16,
    read_future: Option<Pin<Box<dyn Send + Future<Output=BdtResult<usize>>>>>,
    write_future: Option<Pin<Box<dyn Send + Future<Output=std::io::Result<()>>>>>,
}

impl TcpTunnelStream {
    pub(crate) fn new(
        tunnel: Arc<Mutex<TcpTunnelConnectionImpl>>,
        read: TCPRead,
        write: TCPWrite,
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

    async fn handle_cmd(&mut self, cmd_code: PackageCmdCode, cmd_body: &[u8]) -> BdtResult<()> {
        if cmd_code == PackageCmdCode::SynClose {
            let syn_close = SynClose::clone_from_slice(cmd_body).map_err(into_bdt_err!(BdtErrorCode::RawCodecError))?;
            let ack = AckClose {
                sequence: syn_close.sequence,
            };
            let pkg = Package::new(PackageCmdCode::AckClose, ack);
            self.write.as_mut().unwrap().write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_bdt_err!(BdtErrorCode::IoError))?;
            let mut tunnel = self.tunnel.lock().unwrap();
            tunnel.data_socket.as_mut().unwrap().unsplit(self.read.take().unwrap(), self.write.take().unwrap());
            tunnel.enter_idle_mode();
            Ok(())
        } else {
            let mut tunnel = self.tunnel.lock().unwrap();
            Err(bdt_err!(BdtErrorCode::ErrorState, "tunnel {:?} invalid cmd code {:?}", tunnel.sequence, cmd_code))
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

    async fn recv_inner(&mut self, buf: &mut [u8]) -> BdtResult<usize> {
        {
            let tunnel = self.tunnel.lock().unwrap();
            if tunnel.tunnel_state != TunnelState::Worked {
                return Ok(0);
            }
        }
        if self.remainder == 0 {
            let mut buf_header = [0u8; 16];
            let header = self.read.as_mut().unwrap().read_exact(&mut buf_header[0..PackageHeader::raw_bytes().unwrap()]).await.map_err(into_bdt_err!(BdtErrorCode::IoError))?;
            let header = PackageHeader::clone_from_slice(buf_header.as_slice()).map_err(into_bdt_err!(BdtErrorCode::RawCodecError))?;
            let cmd_code = match header.cmd_code() {
                Ok(cmd_code) => cmd_code,
                Err(err) => {
                    return Err(err);
                }
            };
            if cmd_code != PackageCmdCode::PieceData {
                let mut cmd_body = vec![0u8; header.pkg_len() as usize];
                self.read.as_mut().unwrap().read_exact(cmd_body.as_mut_slice()).await.map_err(into_bdt_err!(BdtErrorCode::IoError))?;
                self.handle_cmd(cmd_code, cmd_body.as_slice()).await?;
                return Ok(0);
            } else {
                self.remainder = header.pkg_len();
            }
        }
        if self.remainder as usize > buf.len() {
            let recv_len = self.read.as_mut().unwrap().read(buf).await.map_err(into_bdt_err!(BdtErrorCode::IoError))?;
            self.remainder -= recv_len as u16;
            Ok(recv_len)
        } else {
            let recv_len = self.remainder;
            self.read.as_mut().unwrap().read_exact(&mut buf[..self.remainder as usize]).await.map_err(into_bdt_err!(BdtErrorCode::IoError))?;
            self.remainder = 0;
            Ok(recv_len as usize)
        }
    }

    async fn recv(&mut self, buf: &mut [u8]) -> BdtResult<usize> {
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

    async fn read_pkg(&mut self) -> BdtResult<(PackageCmdCode, Vec<u8>)> {
        let mut buf_header = [0u8; 16];
        self.read.as_mut().unwrap().read_exact(&mut buf_header[0..PackageHeader::raw_bytes().unwrap()]).await.map_err(into_bdt_err!(BdtErrorCode::IoError))?;
        let header = PackageHeader::clone_from_slice(buf_header.as_slice()).map_err(into_bdt_err!(BdtErrorCode::RawCodecError))?;
        let cmd_code = match header.cmd_code() {
            Ok(cmd_code) => cmd_code,
            Err(err) => {
                return Err(err);
            }
        };
        let mut cmd_body = vec![0u8; header.pkg_len() as usize];
        self.read.as_mut().unwrap().read_exact(cmd_body.as_mut_slice()).await.map_err(into_bdt_err!(BdtErrorCode::IoError))?;
        Ok((cmd_code, cmd_body))
    }

    async fn close_inner(&mut self) -> BdtResult<()> {
        let sequence = {
            self.tunnel.lock().unwrap().sequence
        };
        let pkg = Package::new(PackageCmdCode::SynClose, SynClose {
            sequence: sequence,
        });

        self.write.as_mut().unwrap().write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_bdt_err!(BdtErrorCode::IoError))?;

        loop {
            let (cmd_code, cmd_body) = self.read_pkg().await?;
            if cmd_code == PackageCmdCode::AckClose {
                let close = AckClose::clone_from_slice(cmd_body.as_slice()).map_err(into_bdt_err!(BdtErrorCode::RawCodecError))?;
                if close.sequence == sequence {
                    break;
                }
            } else if cmd_code == PackageCmdCode::SynClose {
                let close = SynClose::clone_from_slice(cmd_body.as_slice()).map_err(into_bdt_err!(BdtErrorCode::RawCodecError))?;
                if close.sequence != sequence {
                    continue;
                }
                let ack = AckClose {
                    sequence: close.sequence,
                };
                let pkg = Package::new(PackageCmdCode::AckClose, ack);
                self.write.as_mut().unwrap().write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_bdt_err!(BdtErrorCode::IoError))?;
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
impl TunnelStream for TcpTunnelStream {
    fn port(&self) -> u16 {
        self.port
    }

    fn session_id(&self) -> IncreaseId {
        self.session_id
    }

    fn sequence(&self) -> TempSeq {
        let tunnel = self.tunnel.lock().unwrap();
        tunnel.sequence
    }

    fn remote_device_id(&self) -> DeviceId {
        let tunnel = self.tunnel.lock().unwrap();
        tunnel.remote_id.clone()
    }

    fn local_device_id(&self) -> DeviceId {
        let tunnel = self.tunnel.lock().unwrap();
        tunnel.local_device.device_id().clone()
    }

    fn remote_endpoint(&self) -> Endpoint {
        let tunnel = self.tunnel.lock().unwrap();
        tunnel.remote_ep.clone()
    }

    fn local_endpoint(&self) -> Endpoint {
        let tunnel = self.tunnel.lock().unwrap();
        tunnel.data_socket.as_ref().unwrap().local().clone()
    }

    async fn close(&mut self) -> BdtResult<()> {
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
                Err(bdt_err!(BdtErrorCode::Timeout, "{}", err))
            }
        }
    }
}

impl Drop for TcpTunnelStream {
    fn drop(&mut self) {
        {
            let tunnel = self.tunnel.lock().unwrap();
            log::info!("drop tcp tunnel stream {:?} local_id {} remote_id {}",
                tunnel.sequence, tunnel.local_device.device_id().to_string(), tunnel.remote_id.to_string());
        }
        {
            let tunnel = self.tunnel.lock().unwrap();
            tunnel.tunnel_stat.decrease_work_instance();
        }
        Executor::block_on(self.close());
    }
}


pub struct TcpTunnelDatagramSend {
    tunnel: Arc<Mutex<TcpTunnelConnectionImpl>>,
    read: Option<TCPRead>,
    write: Option<TCPWrite>,
    write_future: Option<Pin<Box<dyn Send + Future<Output=std::io::Result<usize>>>>>,
}

impl TcpTunnelDatagramSend {
    pub fn new(tunnel: Arc<Mutex<TcpTunnelConnectionImpl>>, read: TCPRead, write: TCPWrite) -> Self {
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

    async fn handle_cmd(&mut self, cmd_code: PackageCmdCode, cmd_body: &[u8]) -> BdtResult<()> {
        if cmd_code == PackageCmdCode::SynClose {
            let syn_close = SynClose::clone_from_slice(cmd_body).map_err(into_bdt_err!(BdtErrorCode::RawCodecError))?;
            let ack = AckClose {
                sequence: syn_close.sequence,
            };
            let pkg = Package::new(PackageCmdCode::AckClose, ack);
            self.write.as_mut().unwrap().write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_bdt_err!(BdtErrorCode::IoError))?;
            let mut tunnel = self.tunnel.lock().unwrap();
            tunnel.data_socket.as_mut().unwrap().unsplit(self.read.take().unwrap(), self.write.take().unwrap());
            tunnel.enter_idle_mode()?;
            Ok(())
        } else {
            let tunnel = self.tunnel.lock().unwrap();
            Err(bdt_err!(BdtErrorCode::ErrorState, "tunnel {:?} invalid cmd code {:?}", tunnel.sequence, cmd_code))
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

    async fn read_pkg(&mut self) -> BdtResult<(PackageCmdCode, Vec<u8>)> {
        let mut buf_header = [0u8; 16];
        self.read.as_mut().unwrap().read_exact(&mut buf_header[0..PackageHeader::raw_bytes().unwrap()]).await.map_err(into_bdt_err!(BdtErrorCode::IoError))?;
        let header = PackageHeader::clone_from_slice(buf_header.as_slice()).map_err(into_bdt_err!(BdtErrorCode::RawCodecError))?;
        let cmd_code = match header.cmd_code() {
            Ok(cmd_code) => cmd_code,
            Err(err) => {
                return Err(err);
            }
        };
        let mut cmd_body = vec![0u8; header.pkg_len() as usize];
        self.read.as_mut().unwrap().read_exact(cmd_body.as_mut_slice()).await.map_err(into_bdt_err!(BdtErrorCode::IoError))?;
        Ok((cmd_code, cmd_body))
    }

    async fn close_inner(&mut self) -> BdtResult<()> {
        let sequence = {
            self.tunnel.lock().unwrap().sequence
        };
        let pkg = Package::new(PackageCmdCode::SynClose, SynClose {
            sequence: sequence,
        });

        self.write.as_mut().unwrap().write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_bdt_err!(BdtErrorCode::IoError))?;

        loop {
            let (cmd_code, cmd_body) = self.read_pkg().await?;
            if cmd_code == PackageCmdCode::AckClose {
                let close = AckClose::clone_from_slice(cmd_body.as_slice()).map_err(into_bdt_err!(BdtErrorCode::RawCodecError))?;
                if close.sequence == sequence {
                    break;
                }
            } else if cmd_code == PackageCmdCode::SynClose {
                let close = SynClose::clone_from_slice(cmd_body.as_slice()).map_err(into_bdt_err!(BdtErrorCode::RawCodecError))?;
                if close.sequence != sequence {
                    continue;
                }
                let ack = AckClose {
                    sequence: close.sequence,
                };
                let pkg = Package::new(PackageCmdCode::AckClose, ack);
                self.write.as_mut().unwrap().write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_bdt_err!(BdtErrorCode::IoError))?;
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
impl AsyncWrite for TcpTunnelDatagramSend {
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

#[async_trait::async_trait]
impl TunnelDatagramSend for TcpTunnelDatagramSend {
    fn sequence(&self) -> TempSeq {
        let tunnel = self.tunnel.lock().unwrap();
        tunnel.sequence
    }

    fn remote_device_id(&self) -> DeviceId {
        let tunnel = self.tunnel.lock().unwrap();
        tunnel.remote_id.clone()
    }

    fn local_device_id(&self) -> DeviceId {
        let tunnel = self.tunnel.lock().unwrap();
        tunnel.local_device.device_id().clone()
    }

    fn remote_endpoint(&self) -> Endpoint {
        let tunnel = self.tunnel.lock().unwrap();
        tunnel.remote_ep.clone()
    }

    fn local_endpoint(&self) -> Endpoint {
        let tunnel = self.tunnel.lock().unwrap();
        tunnel.data_socket.as_ref().unwrap().local().clone()
    }

    async fn close(&mut self) -> BdtResult<()> {
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
                Err(bdt_err!(BdtErrorCode::Timeout, "{}", err))
            }
        }
    }
}

impl Drop for TcpTunnelDatagramSend {
    fn drop(&mut self) {
        {
            let tunnel = self.tunnel.lock().unwrap();
            log::info!("drop tcp tunnel datagram {:?} local_id {} remote_id {}",
                tunnel.sequence, tunnel.local_device.device_id().to_string(), tunnel.remote_id.to_string());
        }
        {
            let tunnel = self.tunnel.lock().unwrap();
            tunnel.tunnel_stat.decrease_work_instance();
        }
        Executor::block_on(self.close());
    }
}

pub struct TcpTunnelDatagramRecv {
    tunnel: Arc<Mutex<TcpTunnelConnectionImpl>>,
    read: Option<TCPRead>,
    write: Option<TCPWrite>,
    remainder: u16,
    read_future: Option<Pin<Box<dyn Send + Future<Output=std::io::Result<usize>>>>>,
}

impl TcpTunnelDatagramRecv {
    pub fn new(tunnel: Arc<Mutex<TcpTunnelConnectionImpl>>, read: TCPRead, write: TCPWrite) -> Self {
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

    async fn handle_cmd(&mut self, cmd_code: PackageCmdCode, cmd_body: &[u8]) -> BdtResult<()> {
        if cmd_code == PackageCmdCode::SynClose {
            let syn_close = SynClose::clone_from_slice(cmd_body).map_err(into_bdt_err!(BdtErrorCode::RawCodecError))?;
            let ack = AckClose {
                sequence: syn_close.sequence,
            };
            let pkg = Package::new(PackageCmdCode::AckClose, ack);
            self.write.as_mut().unwrap().write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_bdt_err!(BdtErrorCode::IoError))?;
            let mut tunnel = self.tunnel.lock().unwrap();
            tunnel.data_socket.as_mut().unwrap().unsplit(self.read.take().unwrap(), self.write.take().unwrap());
            tunnel.enter_idle_mode()?;
            Ok(())
        } else {
            let tunnel = self.tunnel.lock().unwrap();
            Err(bdt_err!(BdtErrorCode::ErrorState, "tunnel {:?} invalid cmd code {:?}", tunnel.sequence, cmd_code))
        }
    }

    async fn recv_inner(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.remainder == 0 {
            let mut buf_header = [0u8; 16];
            let header = self.read.as_mut().unwrap().read_exact(&mut buf_header[0..PackageHeader::raw_bytes().unwrap()]).await?;
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
    async fn read_pkg(&mut self) -> BdtResult<(PackageCmdCode, Vec<u8>)> {
        let mut buf_header = [0u8; 16];
        self.read.as_mut().unwrap().read_exact(&mut buf_header[0..PackageHeader::raw_bytes().unwrap()]).await.map_err(into_bdt_err!(BdtErrorCode::IoError))?;
        let header = PackageHeader::clone_from_slice(buf_header.as_slice()).map_err(into_bdt_err!(BdtErrorCode::RawCodecError))?;
        let cmd_code = match header.cmd_code() {
            Ok(cmd_code) => cmd_code,
            Err(err) => {
                return Err(err);
            }
        };
        let mut cmd_body = vec![0u8; header.pkg_len() as usize];
        self.read.as_mut().unwrap().read_exact(cmd_body.as_mut_slice()).await.map_err(into_bdt_err!(BdtErrorCode::IoError))?;
        Ok((cmd_code, cmd_body))
    }

    async fn close_inner(&mut self) -> BdtResult<()> {
        let sequence = {
            self.tunnel.lock().unwrap().sequence
        };
        let pkg = Package::new(PackageCmdCode::SynClose, SynClose {
            sequence: sequence,
        });

        self.write.as_mut().unwrap().write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_bdt_err!(BdtErrorCode::IoError))?;

        loop {
            let (cmd_code, cmd_body) = self.read_pkg().await?;
            if cmd_code == PackageCmdCode::AckClose {
                let close = AckClose::clone_from_slice(cmd_body.as_slice()).map_err(into_bdt_err!(BdtErrorCode::RawCodecError))?;
                if close.sequence == sequence {
                    break;
                }
            } else if cmd_code == PackageCmdCode::SynClose {
                let close = SynClose::clone_from_slice(cmd_body.as_slice()).map_err(into_bdt_err!(BdtErrorCode::RawCodecError))?;
                if close.sequence != sequence {
                    continue;
                }
                let ack = AckClose {
                    sequence: close.sequence,
                };
                let pkg = Package::new(PackageCmdCode::AckClose, ack);
                self.write.as_mut().unwrap().write_all(pkg.to_vec().unwrap().as_slice()).await.map_err(into_bdt_err!(BdtErrorCode::IoError))?;
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

impl AsyncRead for TcpTunnelDatagramRecv {
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

#[async_trait::async_trait]
impl TunnelDatagramRecv for TcpTunnelDatagramRecv {
    fn sequence(&self) -> TempSeq {
        let tunnel = self.tunnel.lock().unwrap();
        tunnel.sequence
    }

    fn remote_device_id(&self) -> DeviceId {
        let tunnel = self.tunnel.lock().unwrap();
        tunnel.remote_id.clone()
    }

    fn local_device_id(&self) -> DeviceId {
        let tunnel = self.tunnel.lock().unwrap();
        tunnel.local_device.device_id().clone()
    }

    fn remote_endpoint(&self) -> Endpoint {
        let tunnel = self.tunnel.lock().unwrap();
        tunnel.remote_ep.clone()
    }

    fn local_endpoint(&self) -> Endpoint {
        let tunnel = self.tunnel.lock().unwrap();
        tunnel.data_socket.as_ref().unwrap().local().clone()
    }

    async fn close(&mut self) -> BdtResult<()> {
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
                Err(bdt_err!(BdtErrorCode::Timeout, "{}", err))
            }
        }
    }
}

impl Drop for TcpTunnelDatagramRecv {
    fn drop(&mut self) {
        {
            let tunnel = self.tunnel.lock().unwrap();
            log::info!("drop tcp tunnel datagram {:?} local_id {} remote_id {}",
                tunnel.sequence, tunnel.local_device.device_id().to_string(), tunnel.remote_id.to_string());
        }
        {
            let tunnel = self.tunnel.lock().unwrap();
            tunnel.tunnel_stat.decrease_work_instance();
        }
        Executor::block_on(self.close());
    }
}
