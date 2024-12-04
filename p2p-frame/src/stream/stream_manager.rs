use std::collections::HashMap;
use std::ops::Deref;
use std::sync::{Arc, Mutex};
use callback_result::SingleCallbackWaiter;
use futures::future::{abortable, AbortHandle};
use crate::error::{bdt_err, BdtErrorCode, BdtResult};
use crate::p2p_identity::{P2pId, LocalDeviceRef, P2pIdentityCertRef};
use crate::stream::StreamGuard;
use crate::tunnel::{TunnelManagerRef, TunnelStream};
use crate::types::IncreaseIdGenerator;

struct StreamListenerState {
    abort_handle: Option<AbortHandle>,
    is_stop: bool,
}
pub struct StreamListener {
    listener_port: u16,
    waiter: SingleCallbackWaiter<StreamGuard>,
    state: Mutex<StreamListenerState>
}

impl StreamListener {
    fn new(listener_port: u16) -> Self {
        Self {
            listener_port,
            waiter: SingleCallbackWaiter::new(),
            state: Mutex::new(StreamListenerState {
                abort_handle: None,
                is_stop: false,
            }),
        }
    }

    pub async fn accept(&self) -> BdtResult<StreamGuard> {
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
            Err(bdt_err!(BdtErrorCode::UserCanceled, "user canceled"))
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
    listener: StreamListenerRef
}

impl StreamListenerGuard {
    fn new(listener: StreamListenerRef, stream_manager: StreamManagerRef) -> Self {
        Self {
            stream_manager,
            listener
        }
    }
}

impl Drop for StreamListenerGuard {
    fn drop(&mut self) {
        self.listener.stop();
        self.stream_manager.remove_listener(self.listener_port);
    }
}

impl Deref for StreamListenerGuard {
    type Target = StreamListener;

    fn deref(&self) -> &Self::Target {
        self.listener.as_ref()
    }
}

pub struct StreamManager {
    local_device: LocalDeviceRef,
    tunnel_manager: TunnelManagerRef,
    session_gen: IncreaseIdGenerator,
    listeners: Mutex<HashMap<u16, StreamListenerRef>>,
}

pub type StreamManagerRef = Arc<StreamManager>;

impl Drop for StreamManager {
    fn drop(&mut self) {
        log::info!("StreamManager drop.device = {}", self.local_device.get_id());
    }
}

impl StreamManager {
    pub fn new(local_device: LocalDeviceRef, tunnel_manager: TunnelManagerRef,) -> Arc<Self> {
        let stream = Arc::new(Self {
            local_device,
            tunnel_manager: tunnel_manager.clone(),
            session_gen: IncreaseIdGenerator::new(),
            listeners: Mutex::new(HashMap::new()),
        });

        let weak = Arc::downgrade(&stream);
        tunnel_manager.set_stream_listener(move |tunnel: Box<dyn TunnelStream>| {
            let weak = weak.clone();
            async move {
                if let Some(stream_manager) = weak.upgrade() {
                    let listeners = stream_manager.listeners.lock().unwrap();
                    if let Some(listener) = listeners.get(&tunnel.port()) {
                        listener.waiter.set_result_with_cache(StreamGuard::new(tunnel));
                    }
                }
            }
        });
        stream
    }

    pub async fn connect(&self, remote: &P2pIdentityCertRef, port: u16, ) -> BdtResult<StreamGuard> {
        let session_id = self.session_gen.generate();
        let tunnel = self.tunnel_manager.create_stream_tunnel(remote, session_id, port).await?;
        Ok(StreamGuard::new(tunnel))
    }

    pub async fn connect_from_id(&self, remote_id: &P2pId, port: u16) -> BdtResult<StreamGuard> {
        let session_id = self.session_gen.generate();
        let tunnel = self.tunnel_manager.create_stream_tunnel_from_id(remote_id, session_id, port).await?;
        Ok(StreamGuard::new(tunnel))
    }

    pub async fn listen(self: &StreamManagerRef, port: u16) -> BdtResult<StreamListenerGuard> {
        let mut listeners = self.listeners.lock().unwrap();
        if listeners.contains_key(&port) {
            return Err(bdt_err!(BdtErrorCode::StreamPortAlreadyListen, "stream port already listen"));
        }
        let listener = Arc::new(StreamListener::new(port));
        listeners.insert(port, listener.clone());
        self.tunnel_manager.add_listen_port(port);
        Ok(StreamListenerGuard::new(listener, self.clone()))
    }

    fn remove_listener(&self, port: u16) {
        let mut listeners = self.listeners.lock().unwrap();
        listeners.remove(&port);
        self.tunnel_manager.remove_listen_port(port);
    }
}
