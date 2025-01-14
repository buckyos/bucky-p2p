use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Mutex};
use callback_result::SingleCallbackWaiter;
use futures::future::{abortable, AbortHandle};
use crate::error::{p2p_err, P2pErrorCode, P2pResult};
use crate::p2p_identity::{P2pId, P2pIdentityCertRef, P2pIdentityRef};
use crate::tunnel::{TunnelDatagramRecv, TunnelDatagramSend, TunnelManagerRef};
use crate::types::IncreaseIdGenerator;

struct DatagramListenerState {
    abort_handle: Option<AbortHandle>,
    is_stop: bool,
}

pub struct DatagramListener {
    listener_port: u16,
    waiter: SingleCallbackWaiter<TunnelDatagramRecv>,
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

    pub async fn accept(&self) -> P2pResult<TunnelDatagramRecv> {
        let future = self.waiter.create_result_future();
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

pub struct DatagramManager {
    local_identity: P2pIdentityRef,
    tunnel_manager: TunnelManagerRef,
    session_gen: IncreaseIdGenerator,
    listeners: Mutex<HashMap<u16, DatagramListenerRef>>,
}
pub type DatagramManagerRef = std::sync::Arc<DatagramManager>;

impl Drop for DatagramManager {
    fn drop(&mut self) {
        log::info!("DatagramManager drop.device = {}", self.local_identity.get_id());
    }
}

impl DatagramManager {
    pub fn new(local_identity: P2pIdentityRef, tunnel_manager: TunnelManagerRef) -> Arc<Self> {
        let datagram = Arc::new(DatagramManager {
            local_identity,
            tunnel_manager: tunnel_manager.clone(),
            session_gen: IncreaseIdGenerator::new(),
            listeners: Mutex::new(HashMap::new()),
        });

        let weak = Arc::downgrade(&datagram);
        tunnel_manager.set_datagram_listener(move |tunnel: TunnelDatagramRecv| {
            let weak = weak.clone();
            async move {
                if let Some(datagram) = weak.upgrade() {
                    let listeners = datagram.listeners.lock().unwrap();
                    if let Some(listener) = listeners.get(&tunnel.port()) {
                        listener.waiter.set_result(tunnel);
                    }
                }
            }
        });

        datagram
    }

    pub async fn connect(&self, remote: &P2pIdentityCertRef, port: u16) -> P2pResult<TunnelDatagramSend> {
        let session_id = self.session_gen.generate();
        let tunnel = self.tunnel_manager.create_datagram_tunnel(remote, port, session_id).await?;
        Ok(tunnel)
    }

    pub async fn connect_from_id(&self, remote_id: &P2pId, port: u16) -> P2pResult<TunnelDatagramSend> {
        let session_id = self.session_gen.generate();
        let tunnel = self.tunnel_manager.create_datagram_tunnel_from_id(remote_id, port, session_id).await?;
        Ok(tunnel)
    }

    pub async fn listen(self: &DatagramManagerRef, port: u16) -> P2pResult<DatagramListenerGuard> {
        let mut listeners = self.listeners.lock().unwrap();
        if listeners.contains_key(&port) {
            return Err(p2p_err!(P2pErrorCode::DatagramPortAlreadyListen, "stream port {} already listen", port));
        }

        let listener = Arc::new(DatagramListener::new(port));
        listeners.insert(port, listener.clone());
        self.tunnel_manager.add_datagram_listen_port(port);
        Ok(DatagramListenerGuard {
            datagram_manager: self.clone(),
            listener,
        })
    }

    fn remove_listener(&self, port: u16) {
        let mut listeners = self.listeners.lock().unwrap();
        listeners.remove(&port);
        self.tunnel_manager.remove_datagram_listen_port(port);
    }
}
