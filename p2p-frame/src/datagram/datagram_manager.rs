use crate::endpoint::Endpoint;
use crate::error::{P2pErrorCode, P2pResult, into_p2p_err, p2p_err};
use crate::executor::Executor;
use crate::networks::{
    ListenPurposeRegistry, Tunnel, TunnelDatagramRead, TunnelDatagramWrite, TunnelManagerRef,
    TunnelPurpose, TunnelRef,
};
use crate::p2p_identity::{P2pId, P2pIdentityCertRef, P2pIdentityRef};
use crate::types::{SessionId, SessionIdGenerator};
use callback_result::SingleCallbackWaiter;
use futures::future::{AbortHandle, abortable};
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Mutex, Weak};
use tokio::io::{AsyncWriteExt, BufWriter};

fn should_continue_accept_loop(err: &crate::error::P2pError) -> bool {
    matches!(err.code(), P2pErrorCode::PortNotListen)
}

pub struct DatagramRead {
    read: TunnelDatagramRead,
    tunnel: Weak<dyn Tunnel>,
    session_id: SessionId,
    purpose: TunnelPurpose,
}

impl DatagramRead {
    pub fn new(
        read: TunnelDatagramRead,
        tunnel: Weak<dyn Tunnel>,
        session_id: SessionId,
        purpose: TunnelPurpose,
    ) -> Self {
        Self {
            read,
            tunnel,
            session_id,
            purpose,
        }
    }

    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    pub fn purpose(&self) -> &TunnelPurpose {
        &self.purpose
    }

    pub fn is_closed(&self) -> bool {
        self.tunnel
            .upgrade()
            .map(|tunnel| tunnel.is_closed())
            .unwrap_or(true)
    }
}

impl Deref for DatagramRead {
    type Target = TunnelDatagramRead;

    fn deref(&self) -> &Self::Target {
        &self.read
    }
}

impl DerefMut for DatagramRead {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.read
    }
}

pub struct DatagramWrite {
    write: Option<BufWriter<TunnelDatagramWrite>>,
    tunnel: Weak<dyn Tunnel>,
    session_id: SessionId,
    purpose: TunnelPurpose,
}

impl DatagramWrite {
    pub fn new(
        write: TunnelDatagramWrite,
        tunnel: Weak<dyn Tunnel>,
        session_id: SessionId,
        purpose: TunnelPurpose,
    ) -> Self {
        Self {
            write: Some(BufWriter::new(write)),
            tunnel,
            session_id,
            purpose,
        }
    }

    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    pub fn purpose(&self) -> &TunnelPurpose {
        &self.purpose
    }

    pub fn is_closed(&self) -> bool {
        self.tunnel
            .upgrade()
            .map(|tunnel| tunnel.is_closed())
            .unwrap_or(true)
    }
}

impl Deref for DatagramWrite {
    type Target = BufWriter<TunnelDatagramWrite>;

    fn deref(&self) -> &Self::Target {
        self.write.as_ref().unwrap()
    }
}

impl DerefMut for DatagramWrite {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.write.as_mut().unwrap()
    }
}

impl Drop for DatagramWrite {
    fn drop(&mut self) {
        if let Some(mut write) = self.write.take() {
            Executor::spawn_ok(async move {
                let _ = write.flush().await;
            })
        }
    }
}

struct DatagramListenerState {
    abort_handle: Option<AbortHandle>,
    is_stop: bool,
}

pub struct DatagramListener {
    listener_purpose: TunnelPurpose,
    waiter: SingleCallbackWaiter<DatagramRead>,
    state: Mutex<DatagramListenerState>,
}

impl Drop for DatagramListener {
    fn drop(&mut self) {
        log::info!("DatagramListener drop.purpose = {}", self.listener_purpose);
    }
}

impl DatagramListener {
    pub fn new(listener_purpose: TunnelPurpose) -> Self {
        Self {
            listener_purpose,
            waiter: SingleCallbackWaiter::new(),
            state: Mutex::new(DatagramListenerState {
                abort_handle: None,
                is_stop: false,
            }),
        }
    }

    pub async fn accept(&self) -> P2pResult<DatagramRead> {
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

pub type DatagramListenerRef = Arc<DatagramListener>;

pub struct DatagramListenerGuard {
    datagram_manager: DatagramManagerRef,
    listener: DatagramListenerRef,
}

impl Drop for DatagramListenerGuard {
    fn drop(&mut self) {
        self.listener.stop();
        self.datagram_manager
            .remove_listener(&self.listener.listener_purpose);
    }
}

impl Deref for DatagramListenerGuard {
    type Target = DatagramListenerRef;

    fn deref(&self) -> &Self::Target {
        &self.listener
    }
}

impl DerefMut for DatagramListenerGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.listener
    }
}

pub struct DatagramManager {
    local_identity: P2pIdentityRef,
    tunnel_manager: TunnelManagerRef,
    session_gen: SessionIdGenerator,
    listeners: Arc<ListenPurposeRegistry<DatagramListener>>,
}

pub type DatagramManagerRef = Arc<DatagramManager>;

impl Drop for DatagramManager {
    fn drop(&mut self) {
        log::info!(
            "DatagramManager drop.device = {}",
            self.local_identity.get_id()
        );
    }
}

impl DatagramManager {
    pub fn new(local_identity: P2pIdentityRef, tunnel_manager: TunnelManagerRef) -> Arc<Self> {
        let datagram = Arc::new(Self {
            local_identity,
            tunnel_manager,
            session_gen: SessionIdGenerator::new(),
            listeners: ListenPurposeRegistry::new(),
        });
        datagram.start_subscription_loop();
        datagram
    }

    pub async fn connect(
        &self,
        remote: &P2pIdentityCertRef,
        purpose: TunnelPurpose,
    ) -> P2pResult<DatagramWrite> {
        let tunnel = self.tunnel_manager.open_tunnel(remote).await?;
        let session_id = self.session_gen.generate();
        let write = tunnel.open_datagram(purpose.clone()).await?;
        Ok(DatagramWrite::new(
            write,
            Arc::downgrade(&tunnel),
            session_id,
            purpose,
        ))
    }

    pub async fn connect_from_id(
        &self,
        remote_id: &P2pId,
        purpose: TunnelPurpose,
    ) -> P2pResult<DatagramWrite> {
        let tunnel = self.tunnel_manager.open_tunnel_from_id(remote_id).await?;
        let session_id = self.session_gen.generate();
        let write = tunnel.open_datagram(purpose.clone()).await?;
        Ok(DatagramWrite::new(
            write,
            Arc::downgrade(&tunnel),
            session_id,
            purpose,
        ))
    }

    pub async fn connect_direct(
        &self,
        remote_pes: Vec<Endpoint>,
        purpose: TunnelPurpose,
        remote_id: &P2pId,
    ) -> P2pResult<DatagramWrite> {
        let tunnel = self
            .tunnel_manager
            .open_direct_tunnel(remote_pes, remote_id)
            .await?;
        let session_id = self.session_gen.generate();
        let write = tunnel.open_datagram(purpose.clone()).await?;
        Ok(DatagramWrite::new(
            write,
            Arc::downgrade(&tunnel),
            session_id,
            purpose,
        ))
    }

    pub async fn listen(
        self: &DatagramManagerRef,
        purpose: TunnelPurpose,
    ) -> P2pResult<DatagramListenerGuard> {
        if self.listeners.contains(&purpose) {
            return Err(p2p_err!(
                P2pErrorCode::DatagramPortAlreadyListen,
                "datagram purpose {} already listen",
                purpose
            ));
        }

        log::debug!(
            "datagram listen register local_id={} purpose={}",
            self.local_identity.get_id(),
            purpose
        );
        let listener = Arc::new(DatagramListener::new(purpose.clone()));
        self.listeners.insert(purpose, listener.clone());
        Ok(DatagramListenerGuard {
            datagram_manager: self.clone(),
            listener,
        })
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
                        log::warn!("datagram tunnel subscription stopped: {:?}", err);
                        break;
                    }
                };
                let Some(datagram) = weak.upgrade() else {
                    break;
                };
                log::debug!(
                    "datagram inject listen vports local_id={} remote_id={} form={:?} protocol={:?}",
                    datagram.local_identity.get_id(),
                    tunnel.remote_id(),
                    tunnel.form(),
                    tunnel.protocol()
                );
                let callback_tunnel = tunnel.clone();
                let callback_weak = weak.clone();
                let callback: crate::networks::IncomingDatagramCallback =
                    Arc::new(move |accepted| {
                        let tunnel = callback_tunnel.clone();
                        let weak = callback_weak.clone();
                        Box::pin(async move {
                            let Some(datagram) = weak.upgrade() else {
                                return;
                            };
                            match accepted {
                                Ok((purpose, read)) => {
                                    let listener = datagram.listeners.get(&purpose);
                                    if let Some(listener) = listener {
                                        listener.waiter.set_result_with_cache(DatagramRead::new(
                                            read,
                                            Arc::downgrade(&tunnel),
                                            datagram.session_gen.generate(),
                                            purpose,
                                        ));
                                    }
                                }
                                Err(err) => {
                                    if should_continue_accept_loop(&err) {
                                        log::debug!(
                                            "datagram callback continue remote {} err {:?}",
                                            tunnel.remote_id(),
                                            err
                                        );
                                    } else {
                                        log::debug!(
                                            "datagram callback ended remote {} err {:?}",
                                            tunnel.remote_id(),
                                            err
                                        );
                                    }
                                }
                            }
                        })
                            as crate::networks::IncomingDatagramCallbackFuture
                    });
                if let Err(err) = tunnel
                    .listen_datagram(datagram.listeners.as_listen_vports_ref(), callback)
                    .await
                {
                    log::warn!(
                        "datagram inject listen vports failed remote {} err {:?}",
                        tunnel.remote_id(),
                        err
                    );
                    continue;
                }
                log::debug!(
                    "datagram inject listen vports done local_id={} remote_id={} form={:?} protocol={:?}",
                    datagram.local_identity.get_id(),
                    tunnel.remote_id(),
                    tunnel.form(),
                    tunnel.protocol()
                );
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::endpoint::Protocol;
    use crate::networks::{
        TunnelDatagramRead, TunnelDatagramWrite, TunnelForm, TunnelState, TunnelStreamRead,
        TunnelStreamWrite,
    };
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

    fn datagram_pair() -> (TunnelDatagramRead, TunnelDatagramWrite) {
        let (datagram, _peer) = tokio::io::duplex(16);
        let (read, write) = tokio::io::split(datagram);
        (Box::pin(read), Box::pin(write))
    }

    #[test]
    fn datagram_wrappers_report_tunnel_closed_state() {
        Executor::init();
        let tunnel = Arc::new(TestTunnel {
            closed: AtomicBool::new(false),
        });
        let tunnel_ref: TunnelRef = tunnel.clone();
        let tunnel_weak = Arc::downgrade(&tunnel_ref);
        let purpose = TunnelPurpose::from_bytes(vec![1]);
        let (read, write) = datagram_pair();
        let datagram_read = DatagramRead::new(
            read,
            tunnel_weak.clone(),
            SessionId::default(),
            purpose.clone(),
        );
        let datagram_write = DatagramWrite::new(write, tunnel_weak, SessionId::default(), purpose);

        assert!(!datagram_read.is_closed());
        assert!(!datagram_write.is_closed());

        tunnel.closed.store(true, Ordering::SeqCst);

        assert!(datagram_read.is_closed());
        assert!(datagram_write.is_closed());
    }
}
