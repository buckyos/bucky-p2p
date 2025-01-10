use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use callback_result::SingleCallbackWaiter;
use futures::future::{abortable, AbortHandle};
use crate::datagram::datagram::DatagramRecvGuard;
use crate::error::{p2p_err, P2pErrorCode, P2pResult};
use crate::p2p_identity::P2pIdentityRef;
use crate::tunnel::{TunnelDatagramRecv, TunnelManagerRef};
use crate::types::IncreaseIdGenerator;

struct DatagramListenerState {
    abort_handle: Option<AbortHandle>,
    is_stop: bool,
}

pub struct DatagramListener {
    listener_port: u16,
    waiter: SingleCallbackWaiter<DatagramRecvGuard>,
    state: Mutex<DatagramListenerState>,
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

    pub async fn accept(&self) -> P2pResult<DatagramRecvGuard> {
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
                        listener.waiter.set_result(DatagramRecvGuard::new(tunnel));
                    }
                }
            }
        });

        datagram
    }
}
