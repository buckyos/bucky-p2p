use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use crate::error::BdtResult;
use crate::p2p_identity::DeviceId;
use crate::sockets::{QuicListenerEventListener, QuicSocket};
use crate::sockets::tcp::{TcpListenerEventListener, TCPSocket};

// #[callback_trait::unsafe_callback_trait]
// pub trait PackageBoxProcessor: 'static + Send + Sync {
//     async fn on_package(&self,
//                         resp_sender: &mut RespSender,
//                         pkg: Package, ) -> BdtResult<()>;
// }

pub struct ReceiveProcessor {
    // package_box_processors: HashMap<u16, Arc<dyn PackageBoxProcessor>>,
    tcp_processor: Option<Arc<dyn TcpListenerEventListener>>,
    quic_processor: Option<Arc<dyn QuicListenerEventListener>>,
}

pub type ReceiveProcessorRef = Arc<ReceiveProcessor>;

impl ReceiveProcessor {
    pub fn new() -> Self {
        Self {
            // package_box_processors: HashMap::new(),
            tcp_processor: None,
            quic_processor: None,
        }
    }

    pub fn add_tcp_processor(&mut self, processor: impl TcpListenerEventListener) {
        self.tcp_processor = Some(Arc::new(processor));
    }

    pub fn get_tcp_processor(&self) -> &Option<Arc<dyn TcpListenerEventListener>> {
        &self.tcp_processor
    }

    pub fn add_quic_processor(&mut self, processor: impl QuicListenerEventListener) {
        self.quic_processor = Some(Arc::new(processor));
    }

    pub fn get_quic_processor(&self) -> &Option<Arc<dyn QuicListenerEventListener>> {
        &self.quic_processor
    }
}

pub struct ReceiveDispatcher {
    processors: RwLock<HashMap<DeviceId, ReceiveProcessorRef>>,
}
pub type ReceiveDispatcherRef = Arc<ReceiveDispatcher>;

impl ReceiveDispatcher {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            processors: RwLock::new(HashMap::new()),
        })
    }

    pub fn add_processor(&self, device_id: DeviceId, processor: ReceiveProcessorRef) {
        log::info!("ReceiveDispatcher add_processor device_id = {}", device_id);
        self.processors.write().unwrap().insert(device_id, processor);
    }

    pub fn get_processor(&self, device_id: &DeviceId) -> Option<ReceiveProcessorRef> {
        self.processors.read().unwrap().get(device_id).map(|p| p.clone())
    }

    pub fn remove_processor(&self, device_id: &DeviceId) {
        log::info!("ReceiveDispatcher remove_processor device_id = {}", device_id);
        self.processors.write().unwrap().remove(device_id);
    }
}

#[async_trait::async_trait]
impl TcpListenerEventListener for ReceiveDispatcher {
    async fn on_new_connection(&self,
                               socket: TCPSocket,) -> BdtResult<()> {
        let processor = self.get_processor(socket.local_device_id());
        if processor.is_none() {
            return Ok(());
        }
        let processor = processor.unwrap();

        if let Some(tcp_processor) = processor.get_tcp_processor() {
            tcp_processor.on_new_connection(socket).await?;
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl QuicListenerEventListener for ReceiveDispatcher {
    async fn on_new_connection(&self,
                               socket: QuicSocket, ) -> BdtResult<()> {
        let processor = self.get_processor(socket.local_device_id());
        if processor.is_none() {
            return Ok(());
        }
        let processor = processor.unwrap();

        if let Some(tcp_processor) = processor.get_quic_processor() {
            tcp_processor.on_new_connection(socket).await?;
        }
        Ok(())
    }
}
