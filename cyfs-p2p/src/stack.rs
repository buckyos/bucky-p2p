use std::sync::Arc;
use std::time::Duration;
use bucky_crypto::PrivateKey;
use bucky_objects::{Device, DeviceId, Endpoint, NamedObject};
use once_cell::sync::OnceCell;
use crate::executor::Executor;
use crate::history::keystore::{Keystore};
use crate::{LocalDevice, LocalDeviceRef, TempSeqGenerator};
use crate::error::BdtResult;
use crate::finder::{DeviceCache, DeviceCacheConfig};
use crate::protocol::v0::SnCalled;
use crate::receive_processor::{ReceiveDispatcher, ReceiveDispatcherRef, ReceiveProcessor, ReceiveProcessorRef};
use crate::sn::client::{SNClientService, SNClientServiceRef, SNEvent};
use crate::sockets::{NetManager, NetManagerRef};
use crate::stream::{StreamManager, StreamManagerRef};
use crate::tunnel::{TunnelManager, TunnelManagerEvent, TunnelManagerRef};

static NET_MANAGER: OnceCell<NetManagerRef> = OnceCell::new();
static RECEIVE_DISPATCHER: OnceCell<ReceiveDispatcherRef> = OnceCell::new();

pub async fn init_p2p(
    endpoints: &[Endpoint],
    port_mapping: Option<Vec<(Endpoint, u16)>>,
    tcp_accept_timout: Duration,) -> BdtResult<()> {
    Executor::init(None);
    let key_store = Arc::new(Keystore::new(crate::history::keystore::Config {
        key_expire: Duration::from_secs(120),
            active_time: Duration::from_secs(600),
            capacity: 256,
        }));
    let device_cache =  Arc::new(DeviceCache::new(&DeviceCacheConfig {
        expire: Duration::from_secs(600),
        capacity: 1024,
    }, None));
    let net_manager = Arc::new(NetManager::open(device_cache, endpoints, port_mapping, tcp_accept_timout).await?);
    let net_manager = NET_MANAGER.get_or_init(move || {
        net_manager.clone()
    });
    let dispatcher = RECEIVE_DISPATCHER.get_or_init(||ReceiveDispatcher::new(key_store.clone()));
    net_manager.set_quic_listener_event_listener(dispatcher.clone());
    net_manager.set_tcp_listener_event_listener(dispatcher.clone());
    net_manager.listen();

    Ok(())
}

pub struct P2pStack {
    local_device: LocalDeviceRef,
    sn_service: SNClientServiceRef,
    tunnel_manager: TunnelManagerRef,
    net_manager: NetManagerRef,
    stream_manager: StreamManagerRef,
    processor_holder: ReceiveProcessorHolder,
}
pub type P2pStackRef = Arc<P2pStack>;

struct ReceiveProcessorHolder {
    device_id: DeviceId,
}

impl Drop for ReceiveProcessorHolder {
    fn drop(&mut self) {
        RECEIVE_DISPATCHER.get().unwrap().remove_processor(&self.device_id);
    }
}

impl P2pStack {
    pub(crate) fn new(
        local_device: LocalDeviceRef,
        sn_service: SNClientServiceRef,
        tunnel_manager: TunnelManagerRef,
        stream_manager: StreamManagerRef,
        net_manager: NetManagerRef,) -> Self {
        net_manager.add_listen_device(local_device.device().clone(), local_device.key().clone());
        let device_id = local_device.device_id().clone();
        Self {
            local_device,
            sn_service,
            tunnel_manager,
            net_manager,
            stream_manager,
            processor_holder: ReceiveProcessorHolder {
                device_id,
            },
        }
    }

    pub async fn wait_online(&self, timeout: Option<Duration>) -> BdtResult<()> {
        self.sn_service.wait_online(timeout).await?;
        Ok(())
    }

    pub fn tunnel_manager(&self) -> &TunnelManagerRef {
        &self.tunnel_manager
    }

    pub fn local_device(&self) -> &LocalDeviceRef {
        &self.local_device
    }

    pub fn stream_manager(&self) -> &StreamManagerRef {
        &self.stream_manager
    }

    pub fn sn_client(&self) -> &SNClientServiceRef {
        &self.sn_service
    }
}

impl Drop for P2pStack {
    fn drop(&mut self) {
        log::info!("P2pStack drop.device = {}", self.local_device.device_id());
        self.net_manager.remove_listen_device(&self.local_device.device().desc().device_id());
        Executor::block_on(self.sn_service.stop());
    }
}

pub struct TunnelManagerEventListener {

}

impl TunnelManagerEventListener {
    pub fn new() -> Self {
        Self {}
    }

}

pub async fn create_p2p_stack(local_device: Device, local_key: PrivateKey, sn_list: Vec<Device>) -> BdtResult<P2pStackRef> {
    let gen_seq = Arc::new(TempSeqGenerator::new());
    let mut processor = ReceiveProcessor::new();
    let net_manager = NET_MANAGER.get().unwrap().clone();
    let device_id = local_device.desc().device_id();
    // net_manager.key_store().add_local_key(device_id.clone(), local_key, local_device.desc().clone());

    let local_device = LocalDevice::new(local_device, local_key.clone());
    let sn_service = SNClientService::new(
        net_manager.clone(),
        sn_list,
        local_device.clone(),
        gen_seq.clone(),
        Duration::from_secs(300),
        Duration::from_secs(300),
        Duration::from_secs(300),
    );

    let tunnel_manager = TunnelManager::new(
        net_manager.clone(),
        RECEIVE_DISPATCHER.get().unwrap().clone(),
        sn_service.clone(),
        local_device.clone(),
        0,
        0,
        Duration::from_secs(30),
    );

    let manager = Arc::downgrade(&tunnel_manager);
    sn_service.set_listener(move |called: SnCalled| {
        let manager = manager.clone();
        async move {
            if let Some(manager) = manager.upgrade() {
                manager.on_sn_called(called).await?;
            }
            Ok(())
        }
    });

    // sn_service.register_pkg_processor(&mut processor);
    tunnel_manager.register_pkg_processor(&mut processor);
    let processor = Arc::new(processor);
    RECEIVE_DISPATCHER.get().unwrap().add_processor(device_id, processor.clone());
    sn_service.start().await;

    let stream_manager = StreamManager::new(local_device.clone(), tunnel_manager.clone());

    Ok(Arc::new(P2pStack::new(
        local_device.clone(),
        sn_service,
        tunnel_manager,
        stream_manager,
        net_manager.clone(),)))
}
