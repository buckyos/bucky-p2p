use std::collections::HashMap;
use std::ops::Deref;
use std::sync::{Arc, Mutex};
use bucky_objects::Device;
use callback_result::SingleCallbackWaiter;
use futures::future::{abortable, AbortHandle};
use crate::error::{bdt_err, BdtErrorCode, BdtResult};
use crate::stream::StreamGuard;
use crate::stream::udp_stream::UdpStream;
use super::tcp_stream::TcpStream;
use crate::tunnel::{SocketType, TunnelGuard, TunnelManagerRef, TunnelType};

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
    tunnel_manager: TunnelManagerRef,
    listeners: Mutex<HashMap<u16, StreamListenerRef>>,
    send_buffer: usize,
    min_box: u16,
    max_box: u16
}

pub type StreamManagerRef = Arc<StreamManager>;

impl StreamManager {
    pub fn new(tunnel_manager: TunnelManagerRef,
               send_buffer: usize,
               min_box: u16,
               max_box: u16) -> Arc<Self> {
        let stream = Arc::new(Self {
            tunnel_manager: tunnel_manager.clone(),
            listeners: Mutex::new(HashMap::new()),
            send_buffer,
            min_box,
            max_box,
        });

        let weak = Arc::downgrade(&stream);
        tunnel_manager.set_stream_listener(move |tunnel: TunnelGuard| {
            let weak = weak.clone();
            async move {
                if let TunnelType::STREAM(vport) = tunnel.tunnel_type() {
                    if let Some(stream_manager) = weak.upgrade() {
                        let mut listeners = stream_manager.listeners.lock().unwrap();
                        if let Some(listener) = listeners.get(&vport) {
                            listener.waiter.set_result_with_cache(StreamGuard::new(Arc::new(TcpStream::new(tunnel, stream_manager.send_buffer, stream_manager.min_box, stream_manager.max_box))));
                        }
                    }
                }
            }
        });
        stream
    }

    pub async fn connect(&self, remote: &Device, port: u16, ) -> BdtResult<StreamGuard> {
        let tunnel = self.tunnel_manager.create_stream_tunnel(remote, port).await?;
        let stream = match tunnel.socket_type() {
            SocketType::TCP => {
                StreamGuard::new(Arc::new(TcpStream::new(tunnel, self.send_buffer, self.min_box, self.max_box)))
            },
            SocketType::UDP => {
                StreamGuard::new(Arc::new(UdpStream::new(tunnel)))
            }
        };
        Ok(stream)
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
