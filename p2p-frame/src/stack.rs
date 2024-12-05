use std::sync::Arc;
use std::time::Duration;
use once_cell::sync::OnceCell;
use crate::endpoint::Endpoint;
use crate::error::P2pResult;
use crate::executor::Executor;
use crate::finder::{DeviceCache, DeviceCacheConfig};
use crate::p2p_identity::{P2pId, P2pIdentityRef, P2pIdentityCertFactoryRef, P2pIdentityCertRef, P2pIdentityFactoryRef};
use crate::protocol::v0::SnCalled;
use crate::receive_processor::{ReceiveDispatcher, ReceiveDispatcherRef, ReceiveProcessor, ReceiveProcessorRef};
use crate::sn::client::{SNClientService, SNClientServiceRef, SNEvent};
use crate::sockets::{NetManager, NetManagerRef};
use crate::stream::{StreamManager, StreamManagerRef};
use crate::tls::init_tls;
use crate::tunnel::{DefaultDeviceFinder, DeviceFinder, DeviceFinderRef, TunnelManager, TunnelManagerEvent, TunnelManagerRef};
use crate::types::TempSeqGenerator;

static NET_MANAGER: OnceCell<NetManagerRef> = OnceCell::new();
static RECEIVE_DISPATCHER: OnceCell<ReceiveDispatcherRef> = OnceCell::new();
static IDENTITY_FACTORY: OnceCell<P2pIdentityFactoryRef> = OnceCell::new();
static CERT_FACTORY: OnceCell<P2pIdentityCertFactoryRef> = OnceCell::new();

pub async fn init_p2p(
    identity_factory: P2pIdentityFactoryRef,
    cert_factory: P2pIdentityCertFactoryRef,
    endpoints: &[Endpoint],
    port_mapping: Option<Vec<(Endpoint, u16)>>,
    tcp_accept_timout: Duration,) -> P2pResult<()> {
    Executor::init(None);
    init_tls(identity_factory.clone());
    let device_cache =  Arc::new(DeviceCache::new(&DeviceCacheConfig {
        expire: Duration::from_secs(600),
        capacity: 1024,
    }, None));
    let net_manager = Arc::new(NetManager::open(device_cache, cert_factory.clone(), endpoints, port_mapping, tcp_accept_timout).await?);
    let net_manager = NET_MANAGER.get_or_init(move || {
        net_manager.clone()
    });
    let dispatcher = RECEIVE_DISPATCHER.get_or_init(||ReceiveDispatcher::new());
    net_manager.set_quic_listener_event_listener(dispatcher.clone());
    net_manager.set_tcp_listener_event_listener(dispatcher.clone());
    net_manager.listen();

    IDENTITY_FACTORY.get_or_init(||identity_factory.clone());
    CERT_FACTORY.get_or_init(||cert_factory.clone());

    Ok(())
}

pub struct P2pStack {
    local_identity: P2pIdentityRef,
    sn_service: SNClientServiceRef,
    tunnel_manager: TunnelManagerRef,
    net_manager: NetManagerRef,
    stream_manager: StreamManagerRef,
    processor_holder: ReceiveProcessorHolder,
}
pub type P2pStackRef = Arc<P2pStack>;

struct ReceiveProcessorHolder {
    device_id: P2pId,
}

impl Drop for ReceiveProcessorHolder {
    fn drop(&mut self) {
        RECEIVE_DISPATCHER.get().unwrap().remove_processor(&self.device_id);
    }
}

impl P2pStack {
    pub fn new(
        local_identity: P2pIdentityRef,
        sn_service: SNClientServiceRef,
        tunnel_manager: TunnelManagerRef,
        stream_manager: StreamManagerRef,
        net_manager: NetManagerRef,) -> Self {
        net_manager.add_listen_device(local_identity.clone());
        let device_id = local_identity.get_id();
        Self {
            local_identity,
            sn_service,
            tunnel_manager,
            net_manager,
            stream_manager,
            processor_holder: ReceiveProcessorHolder {
                device_id,
            },
        }
    }

    pub async fn wait_online(&self, timeout: Option<Duration>) -> P2pResult<()> {
        self.sn_service.wait_online(timeout).await?;
        Ok(())
    }

    pub fn tunnel_manager(&self) -> &TunnelManagerRef {
        &self.tunnel_manager
    }

    pub fn local_identity(&self) -> &P2pIdentityRef {
        &self.local_identity
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
        log::info!("P2pStack drop.device = {}", self.local_identity.get_id());
        self.net_manager.remove_listen_device(&self.local_identity.get_id());
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

pub async fn create_p2p_stack(local_identity: P2pIdentityRef,
                          sn_list: Vec<P2pIdentityCertRef>,
                          device_finder: Option<DeviceFinderRef>,
                          conn_timeout: Duration,
                          idle_timeout: Duration,
                          sn_ping_interval: Duration,
                          sn_call_timeout: Duration,) -> P2pResult<P2pStackRef> {
    let gen_seq = Arc::new(TempSeqGenerator::new());
    let mut processor = ReceiveProcessor::new();
    let net_manager = NET_MANAGER.get().unwrap().clone();
    let device_id = local_identity.get_id();

    let cert_factory = CERT_FACTORY.get().unwrap().clone();
    let sn_service = SNClientService::new(
        net_manager.clone(),
        sn_list,
        local_identity.clone(),
        gen_seq.clone(),
        cert_factory.clone(),
        sn_ping_interval,
        sn_call_timeout,
        conn_timeout,
    );

    let device_finder = if device_finder.is_some() {
        device_finder.unwrap()
    } else {
        DefaultDeviceFinder::new(sn_service.clone(), cert_factory.clone())
    };
    let tunnel_manager = TunnelManager::new(
        net_manager.clone(),
        RECEIVE_DISPATCHER.get().unwrap().clone(),
        sn_service.clone(),
        local_identity.clone(),
        device_finder,
        cert_factory.clone(),
        0,
        0,
        conn_timeout,
        idle_timeout,
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

    let stream_manager = StreamManager::new(local_identity.clone(), tunnel_manager.clone());

    Ok(Arc::new(P2pStack::new(
        local_identity.clone(),
        sn_service,
        tunnel_manager,
        stream_manager,
        net_manager.clone(),)))
}
