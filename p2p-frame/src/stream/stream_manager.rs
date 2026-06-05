use crate::endpoint::Endpoint;
use crate::error::{P2pErrorCode, P2pResult, p2p_err};
use crate::executor::Executor;
use crate::networks::{
    ListenPurposeRegistry, Tunnel, TunnelManagerRef, TunnelPurpose, TunnelRef, TunnelStreamRead,
    TunnelStreamWrite,
};
use crate::p2p_identity::{P2pId, P2pIdentityCertRef, P2pIdentityRef};
use crate::types::SessionId;
use crate::types::SessionIdGenerator;
use std::ops::{Deref, DerefMut};
use std::sync::{
    Arc, Weak,
    atomic::{AtomicBool, Ordering},
};
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio::sync::{Notify, mpsc};

fn should_continue_accept_loop(err: &crate::error::P2pError) -> bool {
    matches!(err.code(), P2pErrorCode::PortNotListen)
}

const LISTENER_CHANNEL_CAPACITY: usize = 1024;

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

type AcceptedStream = (StreamRead, StreamWrite);

struct StreamListenerShared {
    is_stop: AtomicBool,
    notify_stop: Notify,
}

impl StreamListenerShared {
    fn new() -> Self {
        Self {
            is_stop: AtomicBool::new(false),
            notify_stop: Notify::new(),
        }
    }

    fn is_stop(&self) -> bool {
        self.is_stop.load(Ordering::SeqCst)
    }

    fn stop(&self) {
        if !self.is_stop.swap(true, Ordering::SeqCst) {
            self.notify_stop.notify_waiters();
        }
    }
}

pub struct StreamListener {
    listener_purpose: TunnelPurpose,
    accepted_rx: mpsc::Receiver<AcceptedStream>,
    shared: Arc<StreamListenerShared>,
}

pub struct StreamListenerSender {
    accepted_tx: mpsc::Sender<AcceptedStream>,
    shared: Arc<StreamListenerShared>,
}

impl StreamListener {
    fn channel(listener_purpose: TunnelPurpose) -> (Self, Arc<StreamListenerSender>) {
        let (accepted_tx, accepted_rx) = mpsc::channel(LISTENER_CHANNEL_CAPACITY);
        let shared = Arc::new(StreamListenerShared::new());
        (
            Self {
                listener_purpose,
                accepted_rx,
                shared: shared.clone(),
            },
            Arc::new(StreamListenerSender {
                accepted_tx,
                shared,
            }),
        )
    }

    pub async fn accept(&mut self) -> P2pResult<AcceptedStream> {
        if self.shared.is_stop() {
            return Err(p2p_err!(P2pErrorCode::UserCanceled, "user canceled"));
        }
        tokio::select! {
            _ = self.shared.notify_stop.notified() => {
                Err(p2p_err!(P2pErrorCode::UserCanceled, "user canceled"))
            }
            stream = self.accepted_rx.recv() => {
                if self.shared.is_stop() {
                    return Err(p2p_err!(P2pErrorCode::UserCanceled, "user canceled"));
                }
                stream.ok_or_else(|| {
                    p2p_err!(
                        P2pErrorCode::Interrupted,
                        "stream listener channel closed"
                    )
                })
            }
        }
    }

    pub fn stop(&mut self) {
        self.shared.stop();
        self.accepted_rx.close();
    }
}

impl StreamListenerSender {
    fn push_accepted(&self, stream: AcceptedStream) -> P2pResult<()> {
        if self.shared.is_stop() {
            return Err(p2p_err!(P2pErrorCode::UserCanceled, "user canceled"));
        }
        match self.accepted_tx.try_send(stream) {
            Ok(()) => Ok(()),
            Err(mpsc::error::TrySendError::Full(_)) => Err(p2p_err!(
                P2pErrorCode::OutOfLimit,
                "stream listener channel is full"
            )),
            Err(mpsc::error::TrySendError::Closed(_)) => Err(p2p_err!(
                P2pErrorCode::Interrupted,
                "stream listener channel closed"
            )),
        }
    }

    fn stop(&self) {
        self.shared.stop();
    }
}

pub type StreamListenerSenderRef = Arc<StreamListenerSender>;

pub struct StreamListenerGuard {
    stream_manager: StreamManagerRef,
    listener: StreamListener,
}

impl StreamListenerGuard {
    fn new(listener: StreamListener, stream_manager: StreamManagerRef) -> Self {
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
        &self.listener
    }
}

impl DerefMut for StreamListenerGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.listener
    }
}

pub struct ControlStreamListenerGuard {
    stream_manager: StreamManagerRef,
    listener: StreamListener,
}

impl ControlStreamListenerGuard {
    fn new(listener: StreamListener, stream_manager: StreamManagerRef) -> Self {
        Self {
            stream_manager,
            listener,
        }
    }
}

impl Drop for ControlStreamListenerGuard {
    fn drop(&mut self) {
        self.listener.stop();
        self.stream_manager
            .remove_control_listener(&self.listener.listener_purpose);
    }
}

impl Deref for ControlStreamListenerGuard {
    type Target = StreamListener;

    fn deref(&self) -> &Self::Target {
        &self.listener
    }
}

impl DerefMut for ControlStreamListenerGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.listener
    }
}

pub struct StreamManager {
    local_identity: P2pIdentityRef,
    tunnel_manager: TunnelManagerRef,
    session_gen: SessionIdGenerator,
    listeners: Arc<ListenPurposeRegistry<StreamListenerSender>>,
    control_listeners: Arc<ListenPurposeRegistry<StreamListenerSender>>,
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
            control_listeners: ListenPurposeRegistry::new(),
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

    pub async fn connect_control_stream(
        &self,
        remote: &P2pIdentityCertRef,
        purpose: TunnelPurpose,
    ) -> P2pResult<(StreamRead, StreamWrite)> {
        let tunnel = self.tunnel_manager.open_tunnel(remote).await?;
        let session_id = self.session_gen.generate();
        let (read, write) = tunnel.open_control_stream(purpose.clone()).await?;
        Ok(self.wrap_opened_stream(&tunnel, read, write, session_id, purpose))
    }

    pub async fn connect_control_stream_from_id(
        &self,
        remote_id: &P2pId,
        purpose: TunnelPurpose,
    ) -> P2pResult<(StreamRead, StreamWrite)> {
        let tunnel = self.tunnel_manager.open_tunnel_from_id(remote_id).await?;
        let session_id = self.session_gen.generate();
        let (read, write) = tunnel.open_control_stream(purpose.clone()).await?;
        Ok(self.wrap_opened_stream(&tunnel, read, write, session_id, purpose))
    }

    pub async fn connect_control_stream_direct(
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
        let (read, write) = tunnel.open_control_stream(purpose.clone()).await?;
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
        let (listener, sender) = StreamListener::channel(purpose.clone());
        self.listeners.insert(purpose, sender);
        Ok(StreamListenerGuard::new(listener, self.clone()))
    }

    pub async fn listen_control_stream(
        self: &StreamManagerRef,
        purpose: TunnelPurpose,
    ) -> P2pResult<ControlStreamListenerGuard> {
        if self.control_listeners.contains(&purpose) {
            return Err(p2p_err!(
                P2pErrorCode::StreamPortAlreadyListen,
                "control stream purpose {} already listen",
                purpose
            ));
        }
        log::debug!(
            "control stream listen register local_id={} purpose={}",
            self.local_identity.get_id(),
            purpose
        );
        let (listener, sender) = StreamListener::channel(purpose.clone());
        self.control_listeners.insert(purpose, sender);
        Ok(ControlStreamListenerGuard::new(listener, self.clone()))
    }

    fn remove_listener(&self, purpose: &TunnelPurpose) {
        if let Some(sender) = self.listeners.remove(purpose) {
            sender.stop();
        }
    }

    fn remove_control_listener(&self, purpose: &TunnelPurpose) {
        if let Some(sender) = self.control_listeners.remove(purpose) {
            sender.stop();
        }
    }

    fn start_subscription_loop(self: &Arc<Self>) {
        let weak = Arc::downgrade(self);
        let callback: crate::networks::IncomingTunnelCallback = Arc::new(move |result| {
            let weak = weak.clone();
            Box::pin(async move {
                let tunnel = match result {
                    Ok(tunnel) => tunnel,
                    Err(err) => {
                        log::warn!("stream tunnel subscription failed: {:?}", err);
                        return;
                    }
                };
                let Some(stream) = weak.upgrade() else {
                    return;
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
                                let sender = stream.listeners.get(&purpose);
                                if let Some(sender) = sender {
                                    let session_id = stream.session_gen.generate();
                                    let (stream_read, stream_write) = stream.wrap_opened_stream(
                                        &tunnel, read, write, session_id, purpose,
                                    );
                                    if let Err(err) =
                                        sender.push_accepted((stream_read, stream_write))
                                    {
                                        log::warn!(
                                            "stream listener deliver failed remote {} err {:?}",
                                            tunnel.remote_id(),
                                            err
                                        );
                                    }
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
                    return;
                }
                let callback_tunnel = tunnel.clone();
                let callback_weak = weak.clone();
                let callback: crate::networks::IncomingControlStreamCallback = Arc::new(
                    move |accepted| {
                        let tunnel = callback_tunnel.clone();
                        let weak = callback_weak.clone();
                        Box::pin(async move {
                            let Some(stream) = weak.upgrade() else {
                                return;
                            };
                            match accepted {
                                Ok((purpose, read, write)) => {
                                    let sender = stream.control_listeners.get(&purpose);
                                    if let Some(sender) = sender {
                                        let session_id = stream.session_gen.generate();
                                        let (stream_read, stream_write) = stream
                                            .wrap_opened_stream(
                                                &tunnel, read, write, session_id, purpose,
                                            );
                                        if let Err(err) =
                                            sender.push_accepted((stream_read, stream_write))
                                        {
                                            log::warn!(
                                                "control stream listener deliver failed remote {} err {:?}",
                                                tunnel.remote_id(),
                                                err
                                            );
                                        }
                                    }
                                }
                                Err(err) => {
                                    if should_continue_accept_loop(&err) {
                                        log::debug!(
                                            "control stream callback continue remote {} err {:?}",
                                            tunnel.remote_id(),
                                            err
                                        );
                                    } else {
                                        log::debug!(
                                            "control stream callback ended remote {} err {:?}",
                                            tunnel.remote_id(),
                                            err
                                        );
                                    }
                                }
                            }
                        })
                            as crate::networks::IncomingControlStreamCallbackFuture
                    },
                );
                if let Err(err) = tunnel
                    .listen_control_stream(
                        stream.control_listeners.as_listen_vports_ref(),
                        callback,
                    )
                    .await
                {
                    log::warn!(
                        "control stream inject listen purposes failed remote {} err {:?}",
                        tunnel.remote_id(),
                        err
                    );
                    return;
                }
                log::debug!(
                    "stream inject listen vports done local_id={} remote_id={} form={:?} protocol={:?}",
                    stream.local_identity.get_id(),
                    tunnel.remote_id(),
                    tunnel.form(),
                    tunnel.protocol()
                );
            })
        });
        self.tunnel_manager.subscribe(callback);
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

        fn close(&self) -> P2pResult<()> {
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

    #[tokio::test]
    async fn stream_wrappers_report_tunnel_closed_state() {
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
