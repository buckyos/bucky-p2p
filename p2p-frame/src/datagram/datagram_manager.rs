use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use callback_result::SingleCallbackWaiter;
use futures::future::{abortable, AbortHandle};
use tokio::io::{AsyncWriteExt, BufWriter};
use crate::endpoint::Endpoint;
use crate::error::{into_p2p_err, p2p_err, P2pErrorCode, P2pResult};
use crate::executor::Executor;
use crate::p2p_connection::{P2pConnection, P2pConnectionInfoCacheRef};
use crate::p2p_identity::{P2pId, P2pIdentityCertFactoryRef, P2pIdentityCertRef, P2pIdentityRef};
use crate::pn::PnClientRef;
use crate::protocol::v0::TunnelType;
use crate::sn::client::SNClientServiceRef;
use crate::sockets::NetManagerRef;
use crate::tunnel::{DeviceFinderRef, P2pConnectionFactory, TunnelConnectionRead, TunnelConnectionWrite, TunnelListenerRef, TunnelManager, TunnelManagerRef};
use crate::types::{SessionId, SessionIdGenerator, TunnelIdGenerator};

pub struct DatagramRead {
    read: TunnelConnectionRead,
    session_id: SessionId,
    vport: u16,
}

impl DatagramRead {
    pub fn new(read: TunnelConnectionRead, session_id: SessionId, vport: u16) -> Self {
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
    type Target = TunnelConnectionRead;

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
    write: Option<BufWriter<TunnelConnectionWrite>>,
    session_id: SessionId,
    vport: u16,
}

impl DatagramWrite {
    pub fn new(write: TunnelConnectionWrite, session_id: SessionId, vport: u16) -> Self {
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
    type Target = BufWriter<TunnelConnectionWrite>;

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
                write.flush().await;
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
        DatagramListener {
            listener_port,
            waiter: SingleCallbackWaiter::new(),
            state: Mutex::new(DatagramListenerState {
                abort_handle: None,
                is_stop: false,
            }),
        }
    }

    pub async fn accept(&self) -> P2pResult<DatagramRead> {
        let future = self.waiter.create_result_future().map_err(into_p2p_err!(P2pErrorCode::Failed))?;
        let (abort_future, handle) = abortable(async move {
            future.await
        });
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
        if let Err(_) = ret {
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
pub type DatagramListenerRef = std::sync::Arc<DatagramListener>;

pub struct DatagramListenerGuard {
    datagram_manager: DatagramManagerRef,
    listener: DatagramListenerRef,
}

impl Drop for DatagramListenerGuard {
    fn drop(&mut self) {
        self.listener.stop();
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

struct P2pDatagramConnectionFactory {
    net_manager: NetManagerRef,
}

impl P2pDatagramConnectionFactory {
    fn new(net_manager: NetManagerRef) -> Self {
        Self {
            net_manager
        }
    }
}

#[async_trait::async_trait]
impl P2pConnectionFactory for  P2pDatagramConnectionFactory {
    fn tunnel_type(&self) -> TunnelType {
        TunnelType::Datagram
    }

    async fn create_connect(&self, local_identity: &P2pIdentityRef, remote: &Endpoint, remote_id: &P2pId, remote_name: Option<String>) -> P2pResult<Vec<P2pConnection>> {
        self.net_manager.get_network(remote.protocol())?.create_stream_connect(local_identity, remote, remote_id, remote_name).await
    }

    async fn create_connect_with_local_ep(&self, local_identity: &P2pIdentityRef, local_ep: &Endpoint, remote: &Endpoint, remote_id: &P2pId, remote_name: Option<String>) -> P2pResult<P2pConnection> {
        self.net_manager.get_network(remote.protocol())?.create_stream_connect_with_local_ep(local_identity, local_ep, remote, remote_id, remote_name).await
    }
}

pub struct DatagramManager {
    local_identity: P2pIdentityRef,
    tunnel_manager: TunnelManagerRef<P2pDatagramConnectionFactory>,
    session_gen: SessionIdGenerator,
    listeners: Mutex<HashMap<u16, DatagramListenerRef>>,
}
pub type DatagramManagerRef = std::sync::Arc<DatagramManager>;

impl Drop for DatagramManager {
    fn drop(&mut self) {
        log::info!("DatagramManager drop.device = {}", self.local_identity.get_id());
    }
}

impl DatagramManager {
    pub fn new(local_identity: P2pIdentityRef,
               net_manager: NetManagerRef,
               sn_service: SNClientServiceRef,
               device_finder: DeviceFinderRef,
               cert_factory: P2pIdentityCertFactoryRef,
               gen_id: Arc<TunnelIdGenerator>,
               pn_client: Option<PnClientRef>,
               tunnel_listener: TunnelListenerRef,
               conn_info_cache: P2pConnectionInfoCacheRef,
               protocol_version: u8,
               conn_timeout: Duration,
               idle_timeout: Duration,) -> Arc<Self> {

        let tunnel_manager = TunnelManager::new(
            sn_service,
            local_identity.clone(),
            device_finder,
            cert_factory,
            pn_client,
            gen_id,
            P2pDatagramConnectionFactory::new(net_manager),
            tunnel_listener,
            conn_info_cache,
            protocol_version,
            conn_timeout,
            idle_timeout);

        let datagram = Arc::new(DatagramManager {
            local_identity,
            tunnel_manager: tunnel_manager.clone(),
            session_gen: SessionIdGenerator::new(),
            listeners: Mutex::new(HashMap::new()),
        });

        let weak = Arc::downgrade(&datagram);
        tunnel_manager.set_listener(move |session_id: SessionId, vport: u16, read: TunnelConnectionRead, write: TunnelConnectionWrite| {
            let weak = weak.clone();
            async move {
                if let Some(datagram) = weak.upgrade() {
                    let listeners = datagram.listeners.lock().unwrap();
                    if let Some(listener) = listeners.get(&vport) {
                        listener.waiter.set_result_with_cache(DatagramRead::new(read, session_id, vport));
                    }
                }
                Ok(())
            }
        });

        datagram
    }

    pub async fn connect(&self, remote: &P2pIdentityCertRef, port: u16) -> P2pResult<DatagramWrite> {
        let session_id = self.session_gen.generate();
        let (_, write) = self.tunnel_manager.create_session(remote, session_id, port).await?;
        Ok(DatagramWrite::new(write, session_id, port))
    }

    pub async fn connect_from_id(&self, remote_id: &P2pId, port: u16) -> P2pResult<DatagramWrite> {
        let session_id = self.session_gen.generate();
        let (_, write) = self.tunnel_manager.create_session_from_id(remote_id, session_id, port).await?;
        Ok(DatagramWrite::new(write, session_id, port))
    }

    pub async fn listen(self: &DatagramManagerRef, port: u16) -> P2pResult<DatagramListenerGuard> {
        let mut listeners = self.listeners.lock().unwrap();
        if listeners.contains_key(&port) {
            return Err(p2p_err!(P2pErrorCode::DatagramPortAlreadyListen, "stream port {} already listen", port));
        }

        let listener = Arc::new(DatagramListener::new(port));
        listeners.insert(port, listener.clone());
        self.tunnel_manager.add_listen_port(port);
        Ok(DatagramListenerGuard {
            datagram_manager: self.clone(),
            listener,
        })
    }

    fn remove_listener(&self, port: u16) {
        let mut listeners = self.listeners.lock().unwrap();
        listeners.remove(&port);
        self.tunnel_manager.remove_listen_port(port);
    }
}
