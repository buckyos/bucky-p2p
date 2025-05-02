use std::cell::UnsafeCell;
use std::collections::HashMap;
use std::future::Future;
use std::io::Error;
use std::marker::PhantomData;
use std::ops::DerefMut;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, SystemTime};
use bucky_raw_codec::{RawConvertTo, RawDecode, RawFrom};
use callback_result::SingleCallbackWaiter;
use notify_future::{Notify, NotifyWaiter};
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
use rustls::ServerConfig;
use rustls::version::TLS13;
use sfo_cmd_server::client::{CmdClient, SendGuard};
use sfo_cmd_server::errors::{into_cmd_err, CmdErrorCode};
use sfo_cmd_server::{CmdBodyRead, PeerId};
use sfo_split::Splittable;
use tokio::io::{split, AsyncRead, AsyncWrite, BufWriter, ReadBuf};
use tokio_rustls::{TlsAcceptor, TlsConnector};
use crate::error::{into_p2p_err, p2p_err, P2pError, P2pErrorCode, P2pResult};
use crate::executor::{Executor, SpawnHandle};
use crate::p2p_identity::{P2pId, P2pIdentityCertFactoryRef, P2pIdentityRef};
use crate::pn::{FromProxy, FromProxyResp, PnClient, PnCmdHeader, PnTunnelRead, PnTunnelWrite, ProxyClosed, ProxyHeart, ProxyHeartResp, ToProxy, ToProxyResp};
use crate::protocol::PackageCmdCode;
use crate::runtime;
use crate::sn::types::{CmdTunnelId};
use crate::tls::ServerCertResolverRef;
use crate::types::{TunnelId};

type PnTunnelReadWaiterRef = Arc<SingleCallbackWaiter<P2pResult<(usize, Vec<u8>)>>>;

struct RecvCacheState {
    pub cache: HashMap<u32, P2pResult<(usize, Vec<u8>)>>,
    pub expect_seq: u32,
    pub notify: Option<Notify<P2pResult<(usize, Vec<u8>)>>>,
    pub latest_heart: SystemTime,
    pub poll_waiter: Option<NotifyWaiter<P2pResult<(usize, Vec<u8>)>>>,
    pub is_error: bool,
}

pub trait ConnectionState: 'static + Sync + Send {
    fn update_latest_heart_time(&self);
    fn has_timout(&self) -> bool;
    fn is_error(&self) -> bool;
}

#[derive(Clone)]
struct RecvCache {
    state: Arc<Mutex<RecvCacheState>>,
}

impl RecvCache {
    fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(RecvCacheState {
                cache: Default::default(),
                expect_seq: 1,
                notify: None,
                latest_heart: SystemTime::now(),
                poll_waiter: None,
                is_error: false,
            })),
        }
    }

    // async fn get_data(&self) -> P2pResult<(usize, Vec<u8>)> {
    //     let waiter = {
    //         let mut state = self.state.lock().unwrap();
    //         if state.cache.contains_key(&state.expect_seq) {
    //             let data = state.cache.remove(&state.expect_seq).unwrap();
    //             state.expect_seq += 1;
    //             return data;
    //         }
    //         let waiter = NotifyFuture::new();
    //         state.waiter = Some(waiter.clone());
    //         waiter
    //     };
    //     waiter.await
    // }

    fn insert(&self, seq: u32, data: (usize, Vec<u8>)) -> bool {
        let mut state = self.state.lock().unwrap();
        if seq == state.expect_seq && state.notify.is_some() {
            let waiter = state.notify.take().unwrap();
            waiter.notify(Ok(data));
            true
        } else {
            if seq < state.expect_seq {
                return false;
            }

            if state.cache.len() >= 1024 {
                false
            } else {
                state.cache.insert(seq, Ok(data));
                true
            }
        }
    }

    fn insert_err(&self, err: P2pError) {
        let mut state = self.state.lock().unwrap();
        let expect_seq = state.expect_seq;
        state.is_error = true;
        if state.notify.is_some() {
            let waiter = state.notify.take().unwrap();
            waiter.notify(Err(err));
        } else {
            state.cache.insert(expect_seq, Err(err));
        }
    }

}

impl ConnectionState for RecvCache {

    fn update_latest_heart_time(&self) {
        let mut state = self.state.lock().unwrap();
        state.latest_heart = SystemTime::now();
    }

    fn has_timout(&self) -> bool {
        let state = self.state.lock().unwrap();
        SystemTime::now().duration_since(state.latest_heart).unwrap().as_secs() > 60
    }

    fn is_error(&self) -> bool {
        let state = self.state.lock().unwrap();
        state.is_error
    }
}

impl Future for RecvCache {
    type Output = P2pResult<(usize, Vec<u8>)>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let state = &mut self.state.lock().unwrap();
        let expect_seq = state.expect_seq;
        if state.poll_waiter.is_none() {
            if state.cache.contains_key(&expect_seq) {
                let data = state.cache.remove(&expect_seq).unwrap();
                state.expect_seq += 1;
                Poll::Ready(data)
            } else {
                let (notify, mut waiter) = Notify::new();
                state.notify = Some(notify);
                state.poll_waiter = Some(waiter);
                let mut waiter = state.poll_waiter.as_mut().unwrap();
                match Pin::new(waiter).poll(cx) {
                    Poll::Ready(ret) => {
                        state.poll_waiter = None;
                        state.expect_seq += 1;
                        Poll::Ready(ret)
                    }
                    Poll::Pending => {
                        Poll::Pending
                    }
                }
            }
        } else {
            let mut waiter = state.poll_waiter.as_mut().unwrap();
            match Pin::new(waiter).poll(cx) {
                Poll::Ready(ret) => {
                    state.poll_waiter = None;
                    state.expect_seq += 1;
                    Poll::Ready(ret)
                }
                Poll::Pending => {
                    Poll::Pending
                }
            }
        }
    }
}

struct PnTunnelReadImpl<W: Send + 'static + Unpin, S: SendGuard<W> + Unpin, T: CmdClient<u16, u8, W, S>> {
    cmd_client: Arc<DefaultPnClient<W, S, T>>,
    read_cache: Option<(usize, Vec<u8>)>,
    recv_cache: RecvCache,
    timeout_check_handle: SpawnHandle<()>,
    p2p_id: P2pId,
    remote_name: String,
    tunnel_id: TunnelId,
}

impl<W: Send + 'static + Unpin, S: SendGuard<W> + Unpin, T: CmdClient<u16, u8, W, S>> PnTunnelReadImpl<W, S, T> {
    pub(crate) fn new(cmd_client: Arc<DefaultPnClient<W, S, T>>, p2p_id: P2pId, tunnel_id: TunnelId, remote_name: String) -> Self {
        let recv_cache = RecvCache::new();
        cmd_client.add_tunnel_recv_cache(p2p_id.clone(), tunnel_id, recv_cache.clone());
        let heart_state = recv_cache.clone();
        let timeout_check_handle = Executor::spawn_with_handle(async move {
            loop {
                runtime::sleep(Duration::from_secs(20)).await;
                if heart_state.has_timout() {
                    heart_state.insert_err(p2p_err!(P2pErrorCode::ConnectionAborted, "tunnel {:?} heart timeout", tunnel_id));
                    break;
                }
            }
        }).unwrap();
        Self {
            cmd_client,
            p2p_id,
            remote_name,
            tunnel_id,
            read_cache: None,
            recv_cache,
            timeout_check_handle,
        }
    }

    fn get_recv_cache(&self) -> &RecvCache {
        &self.recv_cache
    }
}

impl<W: Send + 'static + Unpin, S: SendGuard<W> + Unpin, T: CmdClient<u16, u8, W, S>> Drop for PnTunnelReadImpl<W, S, T> {
    fn drop(&mut self) {
        self.cmd_client.remove_tunnel_recv_cache(&self.p2p_id, &self.tunnel_id);
        self.timeout_check_handle.abort();
    }
}

impl<W: Send + 'static + Unpin, S: SendGuard<W> + Unpin, T: CmdClient<u16, u8, W, S>> runtime::AsyncRead for PnTunnelReadImpl<W, S, T> {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        if self.read_cache.is_none() {
            match Pin::new(&mut self.recv_cache).poll(cx) {
                Poll::Ready(ret) => {
                    match ret {
                        Ok(data) => {
                            log::trace!("cache remote {} data offset {} len {}", self.remote_name, data.0, data.1.len());
                            self.read_cache = Some(data);
                        },
                        Err(e) => {
                            return Poll::Ready(Err(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())));
                        }
                    }
                }
                Poll::Pending => {
                    return Poll::Pending;
                }
            }
        }

        let len = {
            let buf_ref = buf.initialize_unfilled();
            let (size, data) = self.read_cache.as_mut().unwrap();
            let data_slice = &data[*size..];
            if buf_ref.len() > data_slice.len() {
                buf_ref[..data_slice.len()].copy_from_slice(data_slice);
                let len = data_slice.len();
                self.read_cache = None;
                len
            } else {
                buf_ref.copy_from_slice(&data_slice[..buf_ref.len()]);
                *size += buf_ref.len();
                if *size == data.len() {
                    self.read_cache = None;
                }
                buf_ref.len()
            }
        };
        buf.advance(len);
        Poll::Ready(Ok(()))
    }
}

struct DefaultPnTunnelRead<R: AsyncRead + Unpin + Send + 'static> {
    p2p_id: P2pId,
    remote_name: String,
    tunnel_id: TunnelId,
    read: R,
}

impl<R: AsyncRead + Unpin + Send + 'static> DefaultPnTunnelRead<R> {
    pub(crate) fn new(p2p_id: P2pId, remote_name: String, tunnel_id: TunnelId, read: R) -> Self {
        Self {
            p2p_id,
            remote_name,
            tunnel_id,
            read,
        }
    }
}

impl<R: AsyncRead + Unpin + Send + 'static> PnTunnelRead for DefaultPnTunnelRead<R> {
    fn tunnel_id(&self) -> TunnelId {
        self.tunnel_id
    }

    fn remote_id(&self) -> P2pId {
        self.p2p_id.clone()
    }

    fn remote_name(&self) -> String {
        self.remote_name.clone()
    }
}

impl<R: AsyncRead + Unpin + Send + 'static> AsyncRead for DefaultPnTunnelRead<R> {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.read).poll_read(cx, buf)
    }
}

pub struct PnTunnelWriteImpl<W: Send + 'static + Unpin, S: SendGuard<W> + Unpin, T: CmdClient<u16, u8, W, S>> {
    cmd_client: Arc<T>,
    tunnel_id: TunnelId,
    seq: u32,
    from: P2pId,
    to: P2pId,
    remote_name: String,
    writing_future: Option<Pin<Box<dyn Send + Future<Output=P2pResult<()>>>>>,
    heart_handle: Option<SpawnHandle<()>>,
    connection_state: Box<dyn ConnectionState>,
    version: u8,
    _phantom: PhantomData<(W, S)>,
}

impl<W: Send + 'static + Unpin, S: SendGuard<W> + Unpin, T: CmdClient<u16, u8, W, S>> Drop for PnTunnelWriteImpl<W, S, T> {
    fn drop(&mut self) {
        if self.heart_handle.is_some() {
            self.heart_handle.as_ref().unwrap().abort();
        }
        self.close();
    }
}

impl<W: Send + 'static + Unpin, S: SendGuard<W> + Unpin, T: CmdClient<u16, u8, W, S>> PnTunnelWriteImpl<W, S, T> {
    pub(crate) fn new(cmd_client: Arc<T>,
                      tunnel_id: TunnelId,
                      from: P2pId,
                      to: P2pId,
                      remote_name: String,
                      is_send_heart: bool,
                      connection_state: Box<dyn ConnectionState>) -> Self {
        let heart_handle = if is_send_heart {
            let client = cmd_client.clone();
            let to_id = to.clone();
            let from = from.clone();
            let handle = Executor::spawn_with_handle(async move {
                loop {
                    runtime::sleep(Duration::from_secs(20)).await;
                    let heart = ProxyHeart {
                        tunnel_id,
                        from: from.clone(),
                        to: to_id.clone(),
                    };
                    let cmd_code = PackageCmdCode::ProxyHeart as u8;
                    let body = match heart.to_vec() {
                        Ok(body) => {
                            body
                        },
                        Err(e) => {
                            log::error!("heart to_vec error: {}", e);
                            break;
                        }
                    };
                    match client.send(cmd_code, 0, body.as_slice()).await.map_err(into_cmd_err!(CmdErrorCode::IoError)) {
                        Ok(_) => {
                        },
                        Err(e) => {
                            log::error!("send heart error: {}", e);
                            break;
                        }
                    }
                }
            }).unwrap();
            Some(handle)
        } else {
            None
        };
        Self {
            cmd_client,
            tunnel_id,
            seq: 0,
            from,
            to,
            remote_name,
            writing_future: None,
            heart_handle,
            connection_state,
            version: 0,
            _phantom: Default::default(),
        }
    }

    async fn send(&mut self, data: &[u8]) -> P2pResult<()> {
        let cmd_code = PackageCmdCode::ToProxy as u8;
        self.seq += 1;
        log::trace!("send data to: {}, tunnel_id: {:?}, seq: {} len: {}", self.to, self.tunnel_id, self.seq, data.len());
        let from_proxy = ToProxy {
            tunnel_id: self.tunnel_id,
            seq: self.seq,
            to: self.to.clone(),
        };
        let mut body = from_proxy.to_vec().map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
        self.cmd_client.send2(cmd_code, self.version, vec![body.as_slice(), data].as_slice()).await.map_err(into_p2p_err!(P2pErrorCode::IoError))?;
        Ok(())
    }

    fn close(&self) {
        if self.connection_state.is_error() {
            let cmd_client = self.cmd_client.clone();
            let tunnel_id = self.tunnel_id;
            let from = self.from.clone();
            let to = self.to.clone();
            let version = self.version;
            Executor::spawn(async move {
                log::info!("send proxy closed tunnel_id: {:?} from: {} to: {}", tunnel_id, from, to);
                let cmd_code = PackageCmdCode::ProxyClosed as u8;
                let proxy_closed = ProxyClosed {
                    tunnel_id: Default::default(),
                    from,
                    to,
                };
                if let Ok(body) = proxy_closed.to_vec() {
                    if let Err(e) = cmd_client.send(cmd_code, version, body.as_slice()).await {
                        log::error!("send proxy closed error: {}", e);
                    }
                }
            });
        }
    }
}

impl<W: Send + 'static + Unpin, S: SendGuard<W> + Unpin, T: CmdClient<u16, u8, W, S>> AsyncWrite for PnTunnelWriteImpl<W, S, T> {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<Result<usize, Error>> {
        unsafe {
            if self.writing_future.is_none() {
                let this: &'static mut Self = std::mem::transmute(self.as_mut().deref_mut());
                let buf: &'static [u8] = std::mem::transmute(buf);
                let future = this.send(buf);
                self.as_mut().writing_future = Some(Box::pin(future));
            }

            match Pin::new(self.as_mut().writing_future.as_mut().unwrap()).poll(cx) {
                Poll::Ready(ret) => {
                    self.as_mut().writing_future = None;
                    match ret {
                        Ok(_) => {
                            Poll::Ready(Ok(buf.len()))
                        },
                        Err(e) => {
                            Poll::Ready(Err(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))
                        }
                    }
                }
                Poll::Pending => {
                    Poll::Pending
                }
            }
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        Poll::Ready(Ok(()))
    }
}


struct  DefaultPnTunnelWrite<W: AsyncWrite + Unpin + Send + 'static> {
    tunnel_id: TunnelId,
    remote_id: P2pId,
    remote_name: String,
    write: W,
}

impl<W: AsyncWrite + Unpin + Send + 'static> DefaultPnTunnelWrite<W> {
    pub(crate) fn new(tunnel_id: TunnelId, remote_id: P2pId, remote_name: String, write: W) -> Self {
        Self {
            tunnel_id,
            remote_id,
            remote_name,
            write,
        }
    }
}

impl<W: AsyncWrite + Unpin + Send + 'static> PnTunnelWrite for DefaultPnTunnelWrite<W> {
    fn tunnel_id(&self) -> TunnelId {
        self.tunnel_id
    }

    fn remote_id(&self) -> P2pId {
        self.remote_id.clone()
    }

    fn remote_name(&self) -> String {
        self.remote_name.clone()
    }
}

impl<W: AsyncWrite + Unpin + Send + 'static> AsyncWrite for DefaultPnTunnelWrite<W> {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<Result<usize, Error>> {
        Pin::new(&mut self.get_mut().write).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        Pin::new(&mut self.get_mut().write).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        Pin::new(&mut self.get_mut().write).poll_shutdown(cx)
    }
}

struct DefaultPnClientImpl<W: Send + 'static + Unpin, S: SendGuard<W> + Unpin, T: CmdClient<u16, u8, W, S>> {
    cmd_client: Arc<T>,
    tunnel_recv_cache: Arc<Mutex<HashMap<P2pId, HashMap<TunnelId, RecvCache>>>>,
    accept_waiter: Arc<SingleCallbackWaiter<P2pResult<(Box<dyn crate::pn::PnTunnelRead>, Box<dyn crate::pn::PnTunnelWrite>)>>>,
    local_identity: P2pIdentityRef,
    cert_factory: P2pIdentityCertFactoryRef,
    tls_acceptor: TlsAcceptor,
    version: u8,
    _p: PhantomData<Arc<tokio::sync::Mutex<(W, S)>>>
}

pub struct DefaultPnClient<W: Send + 'static + Unpin, S: SendGuard<W> + Unpin, T: CmdClient<u16, u8, W, S>> {
    inner: Arc<DefaultPnClientImpl<W, S, T>>
}

impl<W: Send + 'static + Unpin, S: SendGuard<W> + Unpin, T: CmdClient<u16, u8, W, S>> Clone for DefaultPnClient<W, S, T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone()
        }
    }
}

impl<W: Send + 'static + Unpin, S: SendGuard<W> + Unpin, T: CmdClient<u16, u8, W, S>> DefaultPnClient<W, S, T> {
    pub fn new(cmd_client: Arc<T>,
               local_identity: P2pIdentityRef,
               cert_factory: P2pIdentityCertFactoryRef,
               cert_resolver: ServerCertResolverRef,) -> Arc<Self> {
        let mut server_config =
            ServerConfig::builder_with_provider(crate::tls::provider().into())
                .with_protocol_versions(&[&TLS13])
                .unwrap()
                .with_client_cert_verifier(Arc::new(crate::tls::TlsClientCertVerifier::new(cert_factory.clone())))
                .with_cert_resolver(cert_resolver.clone().get_resolves_server_cert());

        server_config.key_log = Arc::new(rustls::KeyLogFile::new());


        let this = Arc::new(Self {
            inner: Arc::new(DefaultPnClientImpl {
                cmd_client,
                tunnel_recv_cache: Arc::new(Mutex::new(Default::default())),
                accept_waiter: Arc::new(SingleCallbackWaiter::new()),
                local_identity,
                cert_factory,
                tls_acceptor: TlsAcceptor::from(Arc::new(server_config)),
                version: 0,
                _p: Default::default(),
            })
        });
        this.register_cmd_handler();
        this
    }

    fn register_cmd_handler(self: &Arc<Self>) {
        let this = self.clone();
        self.inner.cmd_client.register_cmd_handler(PackageCmdCode::ToProxyResp as u8, move |_peer_id: PeerId, _tunnel_id: CmdTunnelId, _header: PnCmdHeader, mut body: CmdBodyRead| {
            let this = this.clone();
            async move {
                let data = body.read_all().await?;
                let (resp, buf) = ToProxyResp::raw_decode(data.as_slice()).map_err(into_cmd_err!(CmdErrorCode::RawCodecError))?;
                let tunnel_recv_cache = this.inner.tunnel_recv_cache.lock().unwrap();
                if let Some(recv_cache_map) = tunnel_recv_cache.get(&resp.to) {
                    if let Some(recv_cache) = recv_cache_map.get(&resp.tunnel_id) {
                        recv_cache.insert_err(p2p_err!(P2pErrorCode::Failed, "err {}", resp.result));
                    }
                }
                Ok(())
            }
        });

        let this = self.clone();
        self.inner.cmd_client.register_cmd_handler(PackageCmdCode::FromProxy as u8, move |_peer_id: PeerId, _tunnel_id: CmdTunnelId, _header: PnCmdHeader, mut body: CmdBodyRead| {
            let this = this.clone();
            async move {
                let data = body.read_all().await?;
                log::trace!("recv proxy data tunnel: {:?} len: {} local: {} data: {}", _tunnel_id, data.len(), this.inner.local_identity.get_id(), hex::encode(&data));
                let (from, buf) = FromProxy::raw_decode(data.as_slice()).map_err(into_cmd_err!(CmdErrorCode::RawCodecError))?;
                log::trace!("recv proxy data from: {}, to: {:?}, tunnel_id: {:?}, seq: {}, len: {}", from.from, this.inner.local_identity.get_id(), from.tunnel_id, from.seq, buf.len());
                let recv_cache = {
                    let tunnel_recv_cache = this.inner.tunnel_recv_cache.lock().unwrap();
                    if let Some(recv_cache_map) = tunnel_recv_cache.get(&from.from) {
                        if let Some(recv_cache) = recv_cache_map.get(&from.tunnel_id) {
                            Some(recv_cache.clone())
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                };
                if recv_cache.is_some() {
                    let recv_cache = recv_cache.unwrap();
                    if recv_cache.insert(from.seq, (data.len() - buf.len(), data)) {
                        return Ok(())
                    } else {
                        log::info!("recv cache is full");
                        recv_cache.insert_err(p2p_err!(P2pErrorCode::Failed, "recv cache is full"));
                        let resp = FromProxyResp {
                            from: from.from.clone(),
                            tunnel_id: from.tunnel_id,
                            result: 1,
                        };
                        this.inner.cmd_client.send(PackageCmdCode::FromProxyResp as u8, this.inner.version, resp.to_vec().map_err(into_cmd_err!(CmdErrorCode::RawCodecError))?.as_slice()).await.map_err(into_cmd_err!(CmdErrorCode::IoError))?;
                    }
                } else {
                    let tunnel_read = PnTunnelReadImpl::new(this.clone(), from.from.clone(), from.tunnel_id, from.from.to_string());
                    tunnel_read.get_recv_cache().insert(from.seq, (data.len() - buf.len(), data));
                    let tunnel_write = PnTunnelWriteImpl::new(this.inner.cmd_client.clone(),
                                                                                        from.tunnel_id,
                                                                                        this.inner.local_identity.get_id(),
                                                                                        from.from.clone(),
                                                                                        from.from.to_string(),
                                                                                        false,
                                                                                        Box::new(tunnel_read.get_recv_cache().clone()));
                    let accept_waiter = this.inner.accept_waiter.clone();
                    let tls_acceptor = this.inner.tls_acceptor.clone();
                    Executor::spawn(async move {
                        let conn = Splittable::new(tunnel_read, tunnel_write);
                        let tls_stream = match tls_acceptor.accept(conn).await {
                            Ok(s) => s,
                            Err(e) => {
                                log::error!("accept tls error: {}", e);
                                return;
                            }
                        };
                        let (tunnel_read, tunnel_write) = split(tls_stream);
                        accept_waiter.set_result_with_cache(Ok((Box::new(DefaultPnTunnelRead::new(from.from.clone(), from.from.to_string(), from.tunnel_id, tunnel_read)),
                                                                           Box::new(DefaultPnTunnelWrite::new(from.tunnel_id, from.from.clone(), from.from.to_string(), tunnel_write)))));
                    });
                }
                Ok(())
            }
        });

        let this = self.clone();
        let weak_cmd_client = Arc::downgrade(&this.inner.cmd_client);
        self.inner.cmd_client.register_cmd_handler(PackageCmdCode::ProxyHeart as u8, move |_peer_id: PeerId, _tunnel_id: CmdTunnelId, _header: PnCmdHeader, mut body: CmdBodyRead| {
            let this = this.clone();
            let weak_cmd_client = weak_cmd_client.clone();
            async move {
                let data = body.read_all().await?;
                let heart = ProxyHeart::clone_from_slice(data.as_slice()).map_err(into_cmd_err!(CmdErrorCode::RawCodecError))?;
                let mut has_tunnel = false;
                {
                    let tunnel_recv_cache = this.inner.tunnel_recv_cache.lock().unwrap();
                    if let Some(recv_cache_map) = tunnel_recv_cache.get(&heart.from) {
                        if let Some(recv_cache) = recv_cache_map.get(&heart.tunnel_id) {
                            recv_cache.update_latest_heart_time();
                            has_tunnel = true;
                        }
                    }
                }
                log::debug!("recv heart from: {}, to: {}, tunnel_id: {:?}, has_tunnel: {}", heart.from, heart.to, heart.tunnel_id, has_tunnel);
                if has_tunnel {
                    if let Some(cmd_client) = weak_cmd_client.upgrade() {
                        let cmd_code = PackageCmdCode::ProxyHeartResp as u8;
                        let resp = ProxyHeartResp {
                            tunnel_id: heart.tunnel_id,
                            from: heart.to.clone(),
                            to: heart.from.clone(),
                        };
                        let mut body = resp.to_vec().map_err(into_cmd_err!(CmdErrorCode::RawCodecError))?;
                        cmd_client.send(cmd_code, this.inner.version, body.as_slice()).await.map_err(into_cmd_err!(CmdErrorCode::IoError))?;
                    }
                }
                Ok(())
            }
        });

        let this = self.clone();
        self.inner.cmd_client.register_cmd_handler(PackageCmdCode::ProxyHeartResp as u8, move |_peer_id: PeerId, _tunnel_id: CmdTunnelId, _header: PnCmdHeader, mut body: CmdBodyRead| {
            let this = this.clone();
            async move {
                let data = body.read_all().await?;
                let heart = ProxyHeartResp::clone_from_slice(data.as_slice()).map_err(into_cmd_err!(CmdErrorCode::RawCodecError))?;
                let tunnel_recv_cache = this.inner.tunnel_recv_cache.lock().unwrap();
                let mut has_tunnel = false;
                if let Some(recv_cache_map) = tunnel_recv_cache.get(&heart.from) {
                    if let Some(recv_cache) = recv_cache_map.get(&heart.tunnel_id) {
                        recv_cache.update_latest_heart_time();
                        has_tunnel = true;
                    }
                }
                log::debug!("recv heart resp from: {}, to: {}, tunnel_id: {:?}, has_tunnel: {}", heart.from, heart.to, heart.tunnel_id, has_tunnel);
                Ok(())
            }
        });

        let this = self.clone();
        self.inner.cmd_client.register_cmd_handler(PackageCmdCode::ProxyClosed as u8, move |_peer_id: PeerId, _tunnel_id: CmdTunnelId, _header: PnCmdHeader, mut body: CmdBodyRead| {
            let this = this.clone();
            async move {
                let data = body.read_all().await?;
                let heart = ProxyClosed::clone_from_slice(data.as_slice()).map_err(into_cmd_err!(CmdErrorCode::RawCodecError))?;
                let tunnel_recv_cache = this.inner.tunnel_recv_cache.lock().unwrap();
                let mut has_tunnel = false;
                if let Some(recv_cache_map) = tunnel_recv_cache.get(&heart.from) {
                    if let Some(recv_cache) = recv_cache_map.get(&heart.tunnel_id) {
                        recv_cache.insert_err(p2p_err!(P2pErrorCode::ConnectionAborted, "proxy closed"));
                        has_tunnel = true;
                    }
                }
                Ok(())
            }
        });
    }

    fn remove_tunnel_recv_cache(&self, p2p_id: &P2pId, tunnel_id: &TunnelId) {
        let mut tunnel_recv_cache = self.inner.tunnel_recv_cache.lock().unwrap();
        if let Some(recv_cache_map) = tunnel_recv_cache.get_mut(p2p_id) {
            recv_cache_map.remove(&tunnel_id);
            if recv_cache_map.is_empty() {
                tunnel_recv_cache.remove(&p2p_id);
            }
        }
    }

    fn add_tunnel_recv_cache(&self, p2p_id: P2pId, tunnel_id: TunnelId, recv_cache: RecvCache) {
        let mut tunnel_recv_cache = self.inner.tunnel_recv_cache.lock().unwrap();
        let mut recv_cache_map = tunnel_recv_cache.entry(p2p_id).or_insert_with(Default::default);
        recv_cache_map.insert(tunnel_id, recv_cache);
    }

    fn close_all_tunnel(&self) {
        let mut tunnel_recv_cache = self.inner.tunnel_recv_cache.lock().unwrap();
        for (p2p_id, recv_cache_map) in tunnel_recv_cache.iter() {
            for (tunnel_id, recv_cache) in recv_cache_map.iter() {
                recv_cache.insert_err(p2p_err!(P2pErrorCode::ConnectionAborted, "close all tunnel"));
            }
        }
        tunnel_recv_cache.clear();
    }
}

#[async_trait::async_trait]
impl<W: Send + 'static + Unpin, S: SendGuard<W> + Unpin, T: CmdClient<u16, u8, W, S>> PnClient for DefaultPnClient<W, S, T> {
    async fn accept(&self) -> P2pResult<(Box<dyn PnTunnelRead>, Box<dyn PnTunnelWrite>)> {
        self.inner.accept_waiter.create_result_future().map_err(into_p2p_err!(P2pErrorCode::Failed))?.await.map_err(into_p2p_err!(P2pErrorCode::IoError))?
    }

    async fn connect(&self, tunnel_id: TunnelId, to: P2pId, to_name: Option<String>) -> P2pResult<(Box<dyn PnTunnelRead>, Box<dyn PnTunnelWrite>)> {
        let client_config =
            rustls::ClientConfig::builder_with_provider(crate::tls::provider().into())
                .with_protocol_versions(&[&TLS13])
                .unwrap()
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(crate::tls::TlsServerCertVerifier::new(self.inner.cert_factory.clone(), to.clone())))
                .with_client_auth_cert(vec![CertificateDer::from(self.inner.local_identity.get_identity_cert()?.get_encoded_cert()?)],
                                       PrivatePkcs8KeyDer::from(self.inner.local_identity.get_encoded_identity()?).into()).unwrap();

        let tls_connector = TlsConnector::from(Arc::new(client_config));
        let tunnel_read = PnTunnelReadImpl::new(Arc::new(self.clone()), to.clone(), tunnel_id, to.to_string());
        let tunnel_write =  PnTunnelWriteImpl::new(self.inner.cmd_client.clone(),
                                                                            tunnel_id,
                                                                            self.inner.local_identity.get_id(),
                                                                            to.clone(),
                                                                            to.to_string(),
                                                                            true,
                                                                            Box::new(tunnel_read.get_recv_cache().clone()));
        let mut conn = Splittable::new(tunnel_read, tunnel_write);
        let remote_name = to_name.unwrap_or(to.to_string());
        let mut tls_stream = tls_connector.connect(remote_name.clone().try_into().unwrap(), conn).await.map_err(into_p2p_err!(P2pErrorCode::ConnectFailed, "tls socket to {} connect failed", remote_name))?;
        let (tunnel_read, tunnel_write) = split(tls_stream);
        Ok((Box::new(DefaultPnTunnelRead::new(to.clone(), remote_name.clone(), tunnel_id, tunnel_read)),
            Box::new(DefaultPnTunnelWrite::new(tunnel_id, to.clone(), remote_name, tunnel_write))))
    }
}

impl<W: Send + 'static + Unpin, S: SendGuard<W> + Unpin, T: CmdClient<u16, u8, W, S>> Drop for DefaultPnClientImpl<W, S, T> {
    fn drop(&mut self) {
        log::info!("drop DefaultPnClient");
        let mut tunnel_recv_cache = self.tunnel_recv_cache.lock().unwrap();
        for (p2p_id, recv_cache_map) in tunnel_recv_cache.iter() {
            for (tunnel_id, recv_cache) in recv_cache_map.iter() {
                recv_cache.insert_err(p2p_err!(P2pErrorCode::ConnectionAborted, "close all tunnel"));
            }
        }
        tunnel_recv_cache.clear();
    }
}
