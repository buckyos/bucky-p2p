use std::sync::Arc;
use std::time::Duration;
use once_cell::sync::OnceCell;
use rustls::server::ResolvesServerCert;
use crate::endpoint::Endpoint;
use crate::error::P2pResult;
use crate::executor::Executor;
use crate::finder::{DeviceCache, DeviceCacheConfig};
use crate::p2p_identity::{P2pIdentityRef, P2pIdentityCertFactoryRef, P2pIdentityCertRef, P2pIdentityFactoryRef, P2pIdentityCertCacheRef};
use crate::p2p_network::P2pNetworkRef;
use crate::protocol::v0::SnCalled;
use crate::sn::client::{SNClientService, SNClientServiceRef};
use crate::sockets::{NetManager, NetManagerRef, QuicNetwork};
use crate::sockets::tcp::TcpNetwork;
use crate::stream::{StreamManager, StreamManagerRef};
use crate::tls::{init_tls, DefaultTlsServerCertResolver, ServerCertResolverRef, TlsServerCertResolver};
use crate::tunnel::{DefaultDeviceFinder, DeviceFinderRef, TunnelManager, TunnelManagerRef};
use crate::types::TempSeqGenerator;

static NET_MANAGER: OnceCell<NetManagerRef> = OnceCell::new();
static IDENTITY_FACTORY: OnceCell<P2pIdentityFactoryRef> = OnceCell::new();
static CERT_FACTORY: OnceCell<P2pIdentityCertFactoryRef> = OnceCell::new();

pub struct P2pConfig {
    identity_factory: P2pIdentityFactoryRef,
    cert_factory: P2pIdentityCertFactoryRef,
    identity_cert_cache: P2pIdentityCertCacheRef,
    sever_cert_resolver: ServerCertResolverRef,
    extra_networks: Vec<P2pNetworkRef>,
    endpoints: Vec<Endpoint>,
    port_mapping: Option<Vec<(Endpoint, u16)>>,
    tcp_accept_timout: Duration,
    tcp_connect_timout: Duration,
    quic_connect_timeout: Duration,
    quic_idle_time: Duration,
}

impl P2pConfig {
    pub fn new(
        identity_factory: P2pIdentityFactoryRef,
        cert_factory: P2pIdentityCertFactoryRef,
        endpoints: Vec<Endpoint>,) -> Self {
        Self {
            identity_factory,
            cert_factory,
            identity_cert_cache: Arc::new(DeviceCache::new(&DeviceCacheConfig {
                expire: Duration::from_secs(600),
                capacity: 1024,
            }, None)),
            sever_cert_resolver: DefaultTlsServerCertResolver::new(),
            extra_networks: Vec::new(),
            endpoints,
            port_mapping: None,
            tcp_accept_timout: Duration::from_secs(30),
            tcp_connect_timout: Duration::from_secs(30),
            quic_connect_timeout: Duration::from_secs(30),
            quic_idle_time: Duration::from_secs(60),
        }
    }
    pub fn identity_factory(&self) -> &P2pIdentityFactoryRef {
        &self.identity_factory
    }

    pub fn cert_factory(&self) -> &P2pIdentityCertFactoryRef {
        &self.cert_factory
    }

    pub fn identity_cert_cache(&self) -> &P2pIdentityCertCacheRef {
        &self.identity_cert_cache
    }

    pub fn set_identity_cert_cache(mut self, identity_cert_cache: P2pIdentityCertCacheRef) -> Self {
        self.identity_cert_cache = identity_cert_cache;
        self
    }

    pub fn sever_cert_resolver(&self) -> &ServerCertResolverRef {
        &self.sever_cert_resolver
    }

    pub fn set_sever_cert_resolver(mut self, sever_cert_resolver: ServerCertResolverRef) -> Self {
        self.sever_cert_resolver = sever_cert_resolver;
        self
    }

    pub fn extra_networks(&self) -> &Vec<P2pNetworkRef> {
        &self.extra_networks
    }

    pub fn add_extra_network(mut self, extra_network: P2pNetworkRef) -> Self {
        self.extra_networks.push(extra_network);
        self
    }

    pub fn endpoints(&self) -> &[Endpoint] {
        self.endpoints.as_slice()
    }

    pub fn add_endpoint(mut self, endpoint: Endpoint) -> Self {
        self.endpoints.push(endpoint);
        self
    }

    pub fn port_mapping(&self) -> &Option<Vec<(Endpoint, u16)>> {
        &self.port_mapping
    }

    pub fn add_port_mapping(mut self, port_mapping: (Endpoint, u16)) -> Self {
        if self.port_mapping.is_none() {
            self.port_mapping = Some(vec![]);
        } else {
            self.port_mapping.as_mut().unwrap().push(port_mapping);
        }
        self
    }

    pub fn add_port_mapping_list(mut self, port_mapping: Vec<(Endpoint, u16)>) -> Self {
        if self.port_mapping.is_none() {
            self.port_mapping = Some(port_mapping);
        } else {
            self.port_mapping.as_mut().unwrap().extend(port_mapping);
        }
        self
    }

    pub fn tcp_accept_timout(&self) -> Duration {
        self.tcp_accept_timout
    }

    pub fn set_tcp_accept_timout(mut self, tcp_accept_timout: Duration) -> Self {
        self.tcp_accept_timout = tcp_accept_timout;
        self
    }

    pub fn tcp_connect_timout(&self) -> Duration {
        self.tcp_connect_timout
    }

    pub fn set_tcp_connect_timout(mut self, tcp_connect_timout: Duration) -> Self {
        self.tcp_connect_timout = tcp_connect_timout;
        self
    }

    pub fn quic_connect_timeout(&self) -> Duration {
        self.quic_connect_timeout
    }

    pub fn set_quic_connect_timeout(mut self, quic_connect_timeout: Duration) -> Self {
        self.quic_connect_timeout = quic_connect_timeout;
        self
    }

    pub fn quic_idle_time(&self) -> Duration {
        self.quic_idle_time
    }

    pub fn set_quic_idle_time(mut self, quic_idle_time: Duration) -> Self {
        self.quic_idle_time = quic_idle_time;
        self
    }
}

pub async fn init_p2p(
    config: P2pConfig,
) -> P2pResult<()> {
    Executor::init(None);
    let identity_factory = config.identity_factory();
    let cert_factory = config.cert_factory();
    init_tls(config.identity_factory().clone());
    let device_cache =  config.identity_cert_cache().clone();

    let tsl_server_cert_resolver = config.sever_cert_resolver();
    let tcp_network = Arc::new(TcpNetwork::new(device_cache.clone(), tsl_server_cert_resolver.clone(), cert_factory.clone(), config.tcp_accept_timout));
    let quic_network = Arc::new(QuicNetwork::new(device_cache.clone(), tsl_server_cert_resolver.clone(), cert_factory.clone(), config.quic_connect_timeout, config.quic_idle_time));

    let mut networks = config.extra_networks().clone();
    networks.push(tcp_network as P2pNetworkRef);
    networks.push(quic_network as P2pNetworkRef);

    let net_manager = Arc::new(NetManager::new(networks, tsl_server_cert_resolver.clone())?);
    net_manager.listen(config.endpoints(), config.port_mapping().clone()).await?;
    NET_MANAGER.get_or_init(move || {
        net_manager.clone()
    });

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
}
pub type P2pStackRef = Arc<P2pStack>;

impl P2pStack {
    pub async fn new(
        local_identity: P2pIdentityRef,
        sn_service: SNClientServiceRef,
        tunnel_manager: TunnelManagerRef,
        stream_manager: StreamManagerRef,
        net_manager: NetManagerRef,) -> P2pResult<Self> {
        net_manager.add_listen_device(local_identity.clone()).await?;
        Ok(Self {
            local_identity,
            sn_service,
            tunnel_manager,
            net_manager,
            stream_manager,
        })
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
        Executor::block_on(self.net_manager.remove_listen_device(&self.local_identity.get_id()));
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

pub struct P2pStackConfig {
    local_identity: P2pIdentityRef,
    sn_list: Vec<P2pIdentityCertRef>,
    conn_timeout: Duration,
    idle_timeout: Duration,
    sn_ping_interval: Duration,
    sn_call_timeout: Duration,
    device_finder: Option<DeviceFinderRef>,
}

impl P2pStackConfig {
    pub fn new(
        local_identity: P2pIdentityRef,) -> Self {
        Self {
            local_identity,
            sn_list: Vec::new(),
            conn_timeout: Duration::from_secs(30),
            idle_timeout: Duration::from_secs(30),
            sn_ping_interval: Duration::from_secs(30),
            sn_call_timeout: Duration::from_secs(30),
            device_finder: None,
        }
    }

    pub fn local_identity(&self) -> &P2pIdentityRef {
        &self.local_identity
    }

    pub fn sn_list(&self) -> &[P2pIdentityCertRef] {
        self.sn_list.as_slice()
    }

    pub fn add_sn_list(mut self, mut sn_list: Vec<P2pIdentityCertRef>) -> Self {
        self.sn_list.append(&mut sn_list);
        self
    }

    pub fn add_sn(mut self, sn: P2pIdentityCertRef) -> Self {
        self.sn_list.push(sn);
        self
    }

    pub fn conn_timeout(&self) -> Duration {
        self.conn_timeout
    }

    pub fn set_conn_timeout(mut self, conn_timeout: Duration) -> Self {
        self.conn_timeout = conn_timeout;
        self
    }

    pub fn idle_timeout(&self) -> Duration {
        self.idle_timeout
    }

    pub fn set_idle_timeout(mut self, idle_timeout: Duration) -> Self {
        self.idle_timeout = idle_timeout;
        self
    }

    pub fn sn_ping_interval(&self) -> Duration {
        self.sn_ping_interval
    }

    pub fn set_sn_ping_interval(mut self, sn_ping_interval: Duration) -> Self {
        self.sn_ping_interval = sn_ping_interval;
        self
    }

    pub fn sn_call_timeout(&self) -> Duration {
        self.sn_call_timeout
    }

    pub fn set_sn_call_timeout(mut self, sn_call_timeout: Duration) -> Self {
        self.sn_call_timeout = sn_call_timeout;
        self
    }

    pub fn device_finder(&self) -> &Option<DeviceFinderRef> {
        &self.device_finder
    }

    pub fn set_device_finder(mut self, device_finder: DeviceFinderRef) -> Self {
        self.device_finder = Some(device_finder);
        self
    }
}

pub async fn create_p2p_stack(config: P2pStackConfig) -> P2pResult<P2pStackRef> {
    let gen_seq = Arc::new(TempSeqGenerator::new());
    let net_manager = NET_MANAGER.get().unwrap().clone();

    let cert_factory = CERT_FACTORY.get().unwrap().clone();
    let local_identity = config.local_identity();
    let sn_list = config.sn_list().to_vec();
    let conn_timeout = config.conn_timeout();
    let sn_ping_interval = config.sn_ping_interval();
    let sn_call_timeout = config.sn_call_timeout();
    let device_finder = config.device_finder().clone();
    let idle_timeout = config.idle_timeout();
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

    let _ = sn_service.start().await;

    let stream_manager = StreamManager::new(local_identity.clone(), tunnel_manager.clone());

    Ok(Arc::new(P2pStack::new(
        local_identity.clone(),
        sn_service,
        tunnel_manager,
        stream_manager,
        net_manager.clone(),).await?))
}
