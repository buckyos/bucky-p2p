use crate::endpoint::Endpoint;
use crate::error::{P2pErrorCode, P2pResult, into_p2p_err, p2p_err};
use crate::executor::Executor;
use crate::networks::{
    ListenVPortRegistry, TunnelDatagramRead, TunnelDatagramWrite, TunnelManagerRef, TunnelRef,
};
use crate::p2p_identity::{P2pId, P2pIdentityCertRef, P2pIdentityRef};
use crate::types::{SessionId, SessionIdGenerator};
use callback_result::SingleCallbackWaiter;
use futures::future::{AbortHandle, abortable};
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncWriteExt, BufWriter};

pub struct DatagramRead {
    read: TunnelDatagramRead,
    session_id: SessionId,
    vport: u16,
}

impl DatagramRead {
    pub fn new(read: TunnelDatagramRead, session_id: SessionId, vport: u16) -> Self {
        Self {
            read,
            session_id,
            vport,
        }
    }

    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    pub fn vport(&self) -> u16 {
        self.vport
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
    session_id: SessionId,
    vport: u16,
}

impl DatagramWrite {
    pub fn new(write: TunnelDatagramWrite, session_id: SessionId, vport: u16) -> Self {
        Self {
            write: Some(BufWriter::new(write)),
            session_id,
            vport,
        }
    }

    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    pub fn vport(&self) -> u16 {
        self.vport
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
    listener_port: u16,
    waiter: SingleCallbackWaiter<DatagramRead>,
    state: Mutex<DatagramListenerState>,
}

impl Drop for DatagramListener {
    fn drop(&mut self) {
        log::info!("DatagramListener drop.port = {}", self.listener_port);
    }
}

impl DatagramListener {
    pub fn new(listener_port: u16) -> Self {
        Self {
            listener_port,
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
            .remove_listener(self.listener.listener_port);
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
    listeners: Arc<ListenVPortRegistry<DatagramListener>>,
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
            listeners: ListenVPortRegistry::new(),
        });
        datagram.start_subscription_loop();
        datagram
    }

    pub async fn connect(
        &self,
        remote: &P2pIdentityCertRef,
        port: u16,
    ) -> P2pResult<DatagramWrite> {
        let tunnel = self.tunnel_manager.open_tunnel(remote).await?;
        let session_id = self.session_gen.generate();
        let write = tunnel.open_datagram(port).await?;
        Ok(DatagramWrite::new(write, session_id, port))
    }

    pub async fn connect_from_id(&self, remote_id: &P2pId, port: u16) -> P2pResult<DatagramWrite> {
        let tunnel = self.tunnel_manager.open_tunnel_from_id(remote_id).await?;
        let session_id = self.session_gen.generate();
        let write = tunnel.open_datagram(port).await?;
        Ok(DatagramWrite::new(write, session_id, port))
    }

    pub async fn connect_direct(
        &self,
        remote_pes: Vec<Endpoint>,
        port: u16,
        remote_id: Option<P2pId>,
    ) -> P2pResult<DatagramWrite> {
        let tunnel = self
            .tunnel_manager
            .open_direct_tunnel(remote_pes, remote_id)
            .await?;
        let session_id = self.session_gen.generate();
        let write = tunnel.open_datagram(port).await?;
        Ok(DatagramWrite::new(write, session_id, port))
    }

    pub async fn listen(self: &DatagramManagerRef, port: u16) -> P2pResult<DatagramListenerGuard> {
        if self.listeners.contains(port) {
            return Err(p2p_err!(
                P2pErrorCode::DatagramPortAlreadyListen,
                "stream port {} already listen",
                port
            ));
        }

        log::debug!(
            "datagram listen register local_id={} vport={}",
            self.local_identity.get_id(),
            port
        );
        let listener = Arc::new(DatagramListener::new(port));
        self.listeners.insert(port, listener.clone());
        Ok(DatagramListenerGuard {
            datagram_manager: self.clone(),
            listener,
        })
    }

    fn remove_listener(&self, port: u16) {
        self.listeners.remove(port);
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
                if let Err(err) = tunnel
                    .listen_datagram(datagram.listeners.as_listen_vports_ref())
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
                datagram.start_tunnel_accept_loop(tunnel);
            }
        });
    }

    fn start_tunnel_accept_loop(self: &Arc<Self>, tunnel: TunnelRef) {
        let weak = Arc::downgrade(self);
        Executor::spawn_ok(async move {
            loop {
                let accepted = tunnel.accept_datagram().await;
                let Some(datagram) = weak.upgrade() else {
                    break;
                };
                match accepted {
                    Ok((vport, read)) => {
                        let listener = datagram.listeners.get(vport);
                        if let Some(listener) = listener {
                            listener.waiter.set_result_with_cache(DatagramRead::new(
                                read,
                                datagram.session_gen.generate(),
                                vport,
                            ));
                        }
                    }
                    Err(err) => {
                        log::debug!(
                            "datagram accept loop ended remote {} err {:?}",
                            tunnel.remote_id(),
                            err
                        );
                        break;
                    }
                }
            }
        });
    }
}
