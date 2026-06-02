use crate::endpoint::Endpoint;
use crate::error::{P2pErrorCode, P2pResult, into_p2p_err, p2p_err};
use crate::executor::Executor;
use crate::networks::{
    ListenPurposeRegistry, Tunnel, TunnelManagerRef, TunnelPurpose, TunnelRef, TunnelStreamRead,
    TunnelStreamWrite,
};
use crate::p2p_identity::{P2pId, P2pIdentityCertRef, P2pIdentityRef};
use crate::types::SessionId;
use crate::types::SessionIdGenerator;
use callback_result::SingleCallbackWaiter;
use futures::TryFutureExt;
use futures::future::{AbortHandle, abortable};
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Mutex, Weak};
use tokio::io::{AsyncWriteExt, BufWriter};

fn should_continue_accept_loop(err: &crate::error::P2pError) -> bool {
    matches!(err.code(), P2pErrorCode::PortNotListen)
}

pub struct StreamRead {
    read: TunnelStreamRead,
    tunnel: Weak<dyn Tunnel>,
    session_id: SessionId,
    purpose: TunnelPurpose,
    remote: Endpoint,
    local: Endpoint,
    remote_id: P2pId,
    local_id: P2pId,
}

impl StreamRead {
    pub fn new(
        read: TunnelStreamRead,
        tunnel: Weak<dyn Tunnel>,
        session_id: SessionId,
        purpose: TunnelPurpose,
        local_id: P2pId,
        remote_id: P2pId,
        local: Endpoint,
        remote: Endpoint,
    ) -> Self {
        Self {
            read,
            tunnel,
            session_id,
            purpose,
            remote,
            local,
            remote_id,
            local_id,
        }
    }

    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    pub fn purpose(&self) -> &TunnelPurpose {
        &self.purpose
    }

    pub fn remote(&self) -> Endpoint {
        self.remote
    }

    pub fn local(&self) -> Endpoint {
        self.local
    }

    pub fn remote_id(&self) -> P2pId {
        self.remote_id.clone()
    }

    pub fn local_id(&self) -> P2pId {
        self.local_id.clone()
    }

    pub fn is_closed(&self) -> bool {
        self.tunnel
            .upgrade()
            .map(|tunnel| tunnel.is_closed())
            .unwrap_or(true)
    }
}

impl Deref for StreamRead {
    type Target = TunnelStreamRead;

    fn deref(&self) -> &Self::Target {
        &self.read
    }
}

impl DerefMut for StreamRead {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.read
    }
}

pub struct StreamWrite {
    write: Option<BufWriter<TunnelStreamWrite>>,
    tunnel: Weak<dyn Tunnel>,
    session_id: SessionId,
    purpose: TunnelPurpose,
    remote: Endpoint,
    local: Endpoint,
    remote_id: P2pId,
    local_id: P2pId,
}

impl StreamWrite {
    pub fn new(
        write: TunnelStreamWrite,
        tunnel: Weak<dyn Tunnel>,
        session_id: SessionId,
        purpose: TunnelPurpose,
        local_id: P2pId,
        remote_id: P2pId,
        local: Endpoint,
        remote: Endpoint,
    ) -> Self {
        Self {
            write: Some(BufWriter::new(write)),
            tunnel,
            session_id,
            purpose,
            remote,
            local,
            remote_id,
            local_id,
        }
    }

    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    pub fn purpose(&self) -> &TunnelPurpose {
        &self.purpose
    }

    pub fn remote(&self) -> Endpoint {
        self.remote
    }

    pub fn local(&self) -> Endpoint {
        self.local
    }

    pub fn remote_id(&self) -> P2pId {
        self.remote_id.clone()
    }

    pub fn local_id(&self) -> P2pId {
        self.local_id.clone()
    }

    pub fn is_closed(&self) -> bool {
        self.tunnel
            .upgrade()
            .map(|tunnel| tunnel.is_closed())
            .unwrap_or(true)
    }
}

impl Deref for StreamWrite {
    type Target = BufWriter<TunnelStreamWrite>;

    fn deref(&self) -> &Self::Target {
        self.write.as_ref().unwrap()
    }
}

impl DerefMut for StreamWrite {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.write.as_mut().unwrap()
    }
}

impl Drop for StreamWrite {
    fn drop(&mut self) {
        if let Some(mut write) = self.write.take() {
            Executor::spawn_ok(async move {
                let _ = write.flush().await;
            })
        }
    }
}

struct StreamListenerState {
    abort_handle: Option<AbortHandle>,
    is_stop: bool,
}

pub struct StreamListener {
    listener_purpose: TunnelPurpose,
    waiter: SingleCallbackWaiter<(StreamRead, StreamWrite)>,
    state: Mutex<StreamListenerState>,
}

impl StreamListener {
    fn new(listener_purpose: TunnelPurpose) -> Self {
        Self {
            listener_purpose,
            waiter: SingleCallbackWaiter::new(),
            state: Mutex::new(StreamListenerState {
                abort_handle: None,
                is_stop: false,
            }),
        }
    }

    pub async fn accept(&self) -> P2pResult<(StreamRead, StreamWrite)> {
        let future = self
            .waiter
            .create_result_future()
            .map_err(into_p2p_err!(P2pErrorCode::Failed))?;
        let (abort_future, handle) = abortable(async move { future.await });
        {
            let mut state = self.state.lock().unwrap();
            if !state.is_stop {
                state.abort_handle = Some(handle);
            }
        }
        let ret = abort_future.await;
        {
            let mut state = self.state.lock().unwrap();
            state.abort_handle = None;
        }
        if ret.is_err() {
            Err(p2p_err!(P2pErrorCode::UserCanceled, "user canceled"))
        } else {
            Ok(ret.unwrap().unwrap())
        }
    }

    pub fn stop(&self) {
        let mut state = self.state.lock().unwrap();
        state.is_stop = true;
        if let Some(handle) = state.abort_handle.take() {
            handle.abort();
        }
    }
}

pub type StreamListenerRef = Arc<StreamListener>;

pub struct StreamListenerGuard {
    stream_manager: StreamManagerRef,
    listener: StreamListenerRef,
}

impl StreamListenerGuard {
    fn new(listener: StreamListenerRef, stream_manager: StreamManagerRef) -> Self {
        Self {
            stream_manager,
            listener,
        }
    }
}

impl Drop for StreamListenerGuard {
    fn drop(&mut self) {
        self.listener.stop();
        self.stream_manager
            .remove_listener(&self.listener.listener_purpose);
    }
}

impl Deref for StreamListenerGuard {
    type Target = StreamListener;

    fn deref(&self) -> &Self::Target {
        self.listener.as_ref()
    }
}

pub struct StreamManager {
    local_identity: P2pIdentityRef,
    tunnel_manager: TunnelManagerRef,
    session_gen: SessionIdGenerator,
    listeners: Arc<ListenPurposeRegistry<StreamListener>>,
}

pub type StreamManagerRef = Arc<StreamManager>;

impl Drop for StreamManager {
    fn drop(&mut self) {
        log::info!(
            "StreamManager drop.device = {}",
            self.local_identity.get_id()
        );
    }
}

impl StreamManager {
    pub fn new(local_identity: P2pIdentityRef, tunnel_manager: TunnelManagerRef) -> Arc<Self> {
        let stream = Arc::new(Self {
            local_identity,
            tunnel_manager,
            session_gen: SessionIdGenerator::new(),
            listeners: ListenPurposeRegistry::new(),
        });
        stream.start_subscription_loop();
        stream
    }

    pub async fn connect(
        &self,
        remote: &P2pIdentityCertRef,
        purpose: TunnelPurpose,
    ) -> P2pResult<(StreamRead, StreamWrite)> {
        let tunnel = self.tunnel_manager.open_tunnel(remote).await?;
        let session_id = self.session_gen.generate();
        let (read, write) = tunnel.open_stream(purpose.clone()).await?;
        Ok(self.wrap_opened_stream(&tunnel, read, write, session_id, purpose))
    }

    pub async fn connect_from_id(
        &self,
        remote_id: &P2pId,
        purpose: TunnelPurpose,
    ) -> P2pResult<(StreamRead, StreamWrite)> {
        let tunnel = self.tunnel_manager.open_tunnel_from_id(remote_id).await?;
        let session_id = self.session_gen.generate();
        let (read, write) = tunnel.open_stream(purpose.clone()).await?;
        Ok(self.wrap_opened_stream(&tunnel, read, write, session_id, purpose))
    }

    pub async fn connect_direct(
        &self,
        remote_eps: Vec<Endpoint>,
        purpose: TunnelPurpose,
        remote_id: &P2pId,
    ) -> P2pResult<(StreamRead, StreamWrite)> {
        let tunnel = self
            .tunnel_manager
            .open_direct_tunnel(remote_eps, remote_id)
            .await?;
        let session_id = self.session_gen.generate();
        let (read, write) = tunnel.open_stream(purpose.clone()).await?;
        Ok(self.wrap_opened_stream(&tunnel, read, write, session_id, purpose))
    }

    pub async fn listen(
        self: &StreamManagerRef,
        purpose: TunnelPurpose,
    ) -> P2pResult<StreamListenerGuard> {
        if self.listeners.contains(&purpose) {
            return Err(p2p_err!(
                P2pErrorCode::StreamPortAlreadyListen,
                "stream purpose {} already listen",
                purpose
            ));
        }
        log::debug!(
            "stream listen register local_id={} purpose={}",
            self.local_identity.get_id(),
            purpose
        );
        let listener = Arc::new(StreamListener::new(purpose.clone()));
        self.listeners.insert(purpose, listener.clone());
        Ok(StreamListenerGuard::new(listener, self.clone()))
    }

    fn remove_listener(&self, purpose: &TunnelPurpose) {
        self.listeners.remove(purpose);
    }

    fn start_subscription_loop(self: &Arc<Self>) {
        let mut subscription = self.tunnel_manager.subscribe();
        let weak = Arc::downgrade(self);
        Executor::spawn_ok(async move {
            loop {
                let tunnel = match subscription.accept_tunnel().await {
                    Ok(tunnel) => tunnel,
                    Err(err) => {
                        log::warn!("stream tunnel subscription stopped: {:?}", err);
                        break;
                    }
                };
                let Some(stream) = weak.upgrade() else {
                    break;
                };
                log::debug!(
                    "stream inject listen vports local_id={} remote_id={} form={:?} protocol={:?}",
                    stream.local_identity.get_id(),
                    tunnel.remote_id(),
                    tunnel.form(),
                    tunnel.protocol()
                );
                let callback_tunnel = tunnel.clone();
                let callback_weak = weak.clone();
                let callback: crate::networks::IncomingStreamCallback = Arc::new(move |accepted| {
                    let tunnel = callback_tunnel.clone();
                    let weak = callback_weak.clone();
                    Box::pin(async move {
                        let Some(stream) = weak.upgrade() else {
                            return;
                        };
                        match accepted {
                            Ok((purpose, read, write)) => {
                                let listener = stream.listeners.get(&purpose);
                                if let Some(listener) = listener {
                                    let session_id = stream.session_gen.generate();
                                    let (stream_read, stream_write) = stream.wrap_opened_stream(
                                        &tunnel, read, write, session_id, purpose,
                                    );
                                    listener
                                        .waiter
                                        .set_result_with_cache((stream_read, stream_write));
                                }
                            }
                            Err(err) => {
                                if should_continue_accept_loop(&err) {
                                    log::debug!(
                                        "stream callback continue remote {} err {:?}",
                                        tunnel.remote_id(),
                                        err
                                    );
                                } else {
                                    log::debug!(
                                        "stream callback ended remote {} err {:?}",
                                        tunnel.remote_id(),
                                        err
                                    );
                                }
                            }
                        }
                    }) as crate::networks::IncomingStreamCallbackFuture
                });
                if let Err(err) = tunnel
                    .listen_stream(stream.listeners.as_listen_vports_ref(), callback)
                    .await
                {
                    log::warn!(
                        "stream inject listen vports failed remote {} err {:?}",
                        tunnel.remote_id(),
                        err
                    );
                    continue;
                }
                log::debug!(
                    "stream inject listen vports done local_id={} remote_id={} form={:?} protocol={:?}",
                    stream.local_identity.get_id(),
                    tunnel.remote_id(),
                    tunnel.form(),
                    tunnel.protocol()
                );
            }
        });
    }

    fn wrap_opened_stream(
        &self,
        tunnel: &TunnelRef,
        read: TunnelStreamRead,
        write: TunnelStreamWrite,
        session_id: SessionId,
        purpose: TunnelPurpose,
    ) -> (StreamRead, StreamWrite) {
        let local = tunnel.local_ep().unwrap_or_default();
        let remote = tunnel.remote_ep().unwrap_or_default();
        let local_id = tunnel.local_id();
        let remote_id = tunnel.remote_id();
        let tunnel = Arc::downgrade(tunnel);
        (
            StreamRead::new(
                read,
                tunnel.clone(),
                session_id,
                purpose.clone(),
                local_id.clone(),
                remote_id.clone(),
                local,
                remote,
            ),
            StreamWrite::new(
                write, tunnel, session_id, purpose, local_id, remote_id, local, remote,
            ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::endpoint::Protocol;
    use crate::networks::{TunnelDatagramRead, TunnelDatagramWrite, TunnelForm, TunnelState};
    use crate::types::{TunnelCandidateId, TunnelId};
    use std::sync::atomic::{AtomicBool, Ordering};

    struct TestTunnel {
        closed: AtomicBool,
    }

    #[async_trait::async_trait]
    impl Tunnel for TestTunnel {
        fn tunnel_id(&self) -> TunnelId {
            TunnelId::from(1)
        }

        fn candidate_id(&self) -> TunnelCandidateId {
            TunnelCandidateId::from(1)
        }

        fn form(&self) -> TunnelForm {
            TunnelForm::Active
        }

        fn is_reverse(&self) -> bool {
            false
        }

        fn protocol(&self) -> Protocol {
            Protocol::Tcp
        }

        fn local_id(&self) -> P2pId {
            P2pId::default()
        }

        fn remote_id(&self) -> P2pId {
            P2pId::default()
        }

        fn local_ep(&self) -> Option<Endpoint> {
            None
        }

        fn remote_ep(&self) -> Option<Endpoint> {
            None
        }

        fn state(&self) -> TunnelState {
            if self.is_closed() {
                TunnelState::Closed
            } else {
                TunnelState::Connected
            }
        }

        fn is_closed(&self) -> bool {
            self.closed.load(Ordering::SeqCst)
        }

        async fn close(&self) -> P2pResult<()> {
            self.closed.store(true, Ordering::SeqCst);
            Ok(())
        }

        async fn listen_stream(
            &self,
            _vports: crate::networks::ListenVPortsRef,
            _callback: crate::networks::IncomingStreamCallback,
        ) -> P2pResult<()> {
            Ok(())
        }

        async fn listen_datagram(
            &self,
            _vports: crate::networks::ListenVPortsRef,
            _callback: crate::networks::IncomingDatagramCallback,
        ) -> P2pResult<()> {
            Ok(())
        }

        async fn open_stream(
            &self,
            _purpose: TunnelPurpose,
        ) -> P2pResult<(TunnelStreamRead, TunnelStreamWrite)> {
            Err(p2p_err!(P2pErrorCode::NotSupport, "not supported"))
        }

        async fn open_datagram(&self, _purpose: TunnelPurpose) -> P2pResult<TunnelDatagramWrite> {
            Err(p2p_err!(P2pErrorCode::NotSupport, "not supported"))
        }
    }

    fn stream_pair() -> (TunnelStreamRead, TunnelStreamWrite) {
        let (stream, _peer) = tokio::io::duplex(16);
        let (read, write) = tokio::io::split(stream);
        (Box::pin(read), Box::pin(write))
    }

    #[test]
    fn stream_wrappers_report_tunnel_closed_state() {
        Executor::init();
        let tunnel = Arc::new(TestTunnel {
            closed: AtomicBool::new(false),
        });
        let tunnel_ref: TunnelRef = tunnel.clone();
        let tunnel_weak = Arc::downgrade(&tunnel_ref);
        let purpose = TunnelPurpose::from_bytes(vec![1]);
        let (read, write) = stream_pair();
        let stream_read = StreamRead::new(
            read,
            tunnel_weak.clone(),
            SessionId::default(),
            purpose.clone(),
            P2pId::default(),
            P2pId::default(),
            Endpoint::default(),
            Endpoint::default(),
        );
        let stream_write = StreamWrite::new(
            write,
            tunnel_weak,
            SessionId::default(),
            purpose,
            P2pId::default(),
            P2pId::default(),
            Endpoint::default(),
            Endpoint::default(),
        );

        assert!(!stream_read.is_closed());
        assert!(!stream_write.is_closed());

        tunnel.closed.store(true, Ordering::SeqCst);

        assert!(stream_read.is_closed());
        assert!(stream_write.is_closed());
    }
}
