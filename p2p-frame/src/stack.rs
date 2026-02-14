use std::sync::Arc;
use std::time::Duration;
use once_cell::sync::OnceCell;
use rustls::server::ResolvesServerCert;
use crate::datagram::{DatagramManager, DatagramManagerRef};
use crate::endpoint::{Endpoint, Protocol};
use crate::error::P2pResult;
use crate::executor::Executor;
use crate::finder::{DeviceCache, DeviceCacheConfig};
use crate::p2p_connection::{DefaultP2pConnectionInfoCache, P2pConnectionInfoCacheRef};
use crate::p2p_identity::{P2pIdentityRef, P2pIdentityCertFactoryRef, P2pIdentityCertRef, P2pIdentityFactoryRef, P2pIdentityCertCacheRef, P2pSn};
use crate::p2p_network::P2pNetworkRef;
use crate::pn::{DefaultPnClient, PnClientRef};
use crate::protocol::v0::SnCalled;
use crate::sn::client::{SNClientService, SNClientServiceRef};
use crate::sockets::{NetManager, NetManagerRef, QuicCongestionAlgorithm, QuicNetwork};
use crate::sockets::tcp::TcpNetwork;
use crate::stream::{StreamManager, StreamManagerRef};
use crate::tls::{init_tls, DefaultTlsServerCertResolver, ServerCertResolverRef, TlsServerCertResolver};
pub use crate::tunnel::{DeviceFinder, DeviceFinderRef};
use crate::tunnel::{DefaultDeviceFinder, TunnelListener, TunnelListenerRef, TunnelManager, TunnelManagerRef};
use crate::types::{SequenceGenerator, TunnelIdGenerator};

pub struct P2pEnv {
    net_manager: NetManagerRef,
    identity_factory: P2pIdentityFactoryRef,
    cert_factory: P2pIdentityCertFactoryRef,
    identity_cert_cache: P2pIdentityCertCacheRef,
    sever_cert_resolver: ServerCertResolverRef,
}

impl P2pEnv {
    pub fn new(
        net_manager: NetManagerRef,
        identity_factory: P2pIdentityFactoryRef,
        cert_factory: P2pIdentityCertFactoryRef,
        identity_cert_cache: P2pIdentityCertCacheRef,
        sever_cert_resolver: ServerCertResolverRef,) -> P2pEnvRef {
        Arc::new(P2pEnv {
            net_manager,
            identity_factory,
            cert_factory,
            identity_cert_cache,
            sever_cert_resolver,
        })
    }
}
pub type P2pEnvRef = Arc<P2pEnv>;

pub struct P2pConfig {
    identity_factory: P2pIdentityFactoryRef,
    cert_factory: P2pIdentityCertFactoryRef,
    identity_cert_cache: P2pIdentityCertCacheRef,
    sever_cert_resolver: ServerCertResolverRef,
    connection_info_cache: P2pConnectionInfoCacheRef,
    extra_networks: Vec<P2pNetworkRef>,
    endpoints: Vec<Endpoint>,
    port_mapping: Option<Vec<(Endpoint, u16)>>,
    tcp_accept_timout: Duration,
    tcp_connect_timout: Duration,
    quic_connect_timeout: Duration,
    quic_idle_time: Duration,
    quic_congestion_algorithm: QuicCongestionAlgorithm,
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
            connection_info_cache: DefaultP2pConnectionInfoCache::new(),
            extra_networks: Vec::new(),
            endpoints,
            port_mapping: None,
            tcp_accept_timout: Duration::from_secs(10),
            tcp_connect_timout: Duration::from_secs(10),
            quic_connect_timeout: Duration::from_secs(10),
            quic_idle_time: Duration::from_secs(60),
            quic_congestion_algorithm: QuicCongestionAlgorithm::Bbr,
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

    pub fn set_connection_info_cache(mut self, connection_info_cache: P2pConnectionInfoCacheRef) -> Self {
        self.connection_info_cache = connection_info_cache;
        self
    }

    pub fn connection_info_cache(&self) -> &P2pConnectionInfoCacheRef {
        &self.connection_info_cache
    }

    pub fn quic_congestion_algorithm(&self) -> QuicCongestionAlgorithm {
        self.quic_congestion_algorithm
    }

    pub fn set_quic_congestion_algorithm(mut self, quic_congestion_algorithm: QuicCongestionAlgorithm) -> Self {
        self.quic_congestion_algorithm = quic_congestion_algorithm;
        self
    }
}

pub async fn create_p2p_env(
    config: P2pConfig,
) -> P2pResult<P2pEnvRef> {
    Executor::init_new_multi_thread(None);
    let identity_factory = config.identity_factory();
    let cert_factory = config.cert_factory();
    init_tls(config.identity_factory().clone());
    let device_cache =  config.identity_cert_cache().clone();

    let tsl_server_cert_resolver = config.sever_cert_resolver();
    let tcp_network = Arc::new(TcpNetwork::new(device_cache.clone(),
                                               tsl_server_cert_resolver.clone(),
                                               cert_factory.clone(),
                                               config.tcp_accept_timout));
    let quic_network = Arc::new(QuicNetwork::new(device_cache.clone(),
                                                 tsl_server_cert_resolver.clone(),
                                                 cert_factory.clone(),
                                                 config.quic_congestion_algorithm,
                                                 config.quic_connect_timeout,
                                                 config.quic_idle_time));

    let mut networks = config.extra_networks().clone();
    networks.push(tcp_network as P2pNetworkRef);
    networks.push(quic_network as P2pNetworkRef);

    let net_manager = Arc::new(NetManager::new(networks, tsl_server_cert_resolver.clone(), config.connection_info_cache.clone())?);
    net_manager.listen(config.endpoints(), config.port_mapping().clone()).await?;

    Ok(P2pEnv::new(net_manager, identity_factory.clone(), cert_factory.clone(), device_cache, tsl_server_cert_resolver.clone()))
}

pub struct P2pStack {
    env: P2pEnvRef,
    local_identity: P2pIdentityRef,
    sn_service: SNClientServiceRef,
    net_manager: NetManagerRef,
    stream_manager: StreamManagerRef,
    datagram_manager: DatagramManagerRef,
}
pub type P2pStackRef = Arc<P2pStack>;

impl P2pStack {
    pub(crate) async fn new(
        env: P2pEnvRef,
        local_identity: P2pIdentityRef,
        sn_service: SNClientServiceRef,
        stream_manager: StreamManagerRef,
        datagram_manager: DatagramManagerRef,
        net_manager: NetManagerRef,) -> P2pResult<Self> {
        net_manager.add_listen_device(local_identity.clone()).await?;
        Ok(Self {
            env,
            local_identity,
            sn_service,
            net_manager,
            stream_manager,
            datagram_manager,
        })
    }

    pub async fn wait_online(&self, timeout: Option<Duration>) -> P2pResult<()> {
        self.sn_service.wait_online(timeout).await?;
        Ok(())
    }

    pub fn local_identity(&self) -> &P2pIdentityRef {
        &self.local_identity
    }

    pub fn stream_manager(&self) -> &StreamManagerRef {
        &self.stream_manager
    }

    pub fn datagram_manager(&self) -> &DatagramManagerRef {
        &self.datagram_manager
    }

    pub fn sn_client(&self) -> &SNClientServiceRef {
        &self.sn_service
    }

    pub fn cert_cache(&self) -> &P2pIdentityCertCacheRef {
        &self.env.identity_cert_cache
    }

    pub fn set_as_default(&self) {
        self.env.sever_cert_resolver.set_default_server_identity(&self.local_identity.get_id());
    }

    pub fn get_listen_eps(&self, protocol: Protocol) -> Option<Vec<(Endpoint, Option<u16>)>> {
        if let Ok(network) = self.net_manager.get_network(protocol) {
            Some(network.listeners().iter().map(|l| (l.local(), l.mapping_port())).collect())
        } else {
            None
        }
    }

    pub fn p2p_env(&self) -> &P2pEnvRef {
        &self.env
    }
}

impl Drop for P2pStack {
    fn drop(&mut self) {
        log::info!("P2pStack drop.device = {}", self.local_identity.get_id());
        Executor::block_on(self.net_manager.remove_listen_device(&self.local_identity.get_name()));
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
    env: P2pEnvRef,
    local_identity: P2pIdentityRef,
    sn_list: Vec<P2pSn>,
    conn_timeout: Duration,
    idle_timeout: Duration,
    sn_ping_interval: Duration,
    sn_call_timeout: Duration,
    sn_query_interval: Duration,
    device_finder: Option<DeviceFinderRef>,
    sn_tunnel_count: u16,
    support_proxy: bool,
    proxy_client: Option<PnClientRef>,
}

impl P2pStackConfig {
    pub fn new(
        env: P2pEnvRef,
        local_identity: P2pIdentityRef,) -> Self {
        Self {
            env,
            local_identity,
            sn_list: Vec::new(),
            conn_timeout: Duration::from_secs(30),
            idle_timeout: Duration::from_secs(30),
            sn_ping_interval: Duration::from_secs(30),
            sn_call_timeout: Duration::from_secs(30),
            sn_query_interval: Duration::from_secs(300),
            device_finder: None,
            sn_tunnel_count: 5,
            support_proxy: false,
            proxy_client: None,
        }
    }

    pub fn local_identity(&self) -> &P2pIdentityRef {
        &self.local_identity
    }

    pub fn sn_list(&self) -> &[P2pSn] {
        self.sn_list.as_slice()
    }

    pub fn add_sn_list(mut self, mut sn_list: Vec<P2pSn>) -> Self {
        self.sn_list.append(&mut sn_list);
        self
    }

    pub fn add_sn(mut self, sn: P2pSn) -> Self {
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

    pub fn sn_tunnel_count(&self) -> u16 {
        self.sn_tunnel_count
    }

    pub fn set_sn_tunnel_count(mut self, tunnel_count: u16) -> Self {
        if tunnel_count > 0 {
            self.sn_tunnel_count = tunnel_count;
        }
        self
    }

    pub fn support_proxy(&self) -> bool {
        self.support_proxy
    }

    pub fn set_support_proxy(mut self, support_proxy: bool) -> Self {
        self.support_proxy = support_proxy;
        self
    }

    pub fn proxy_client(&self) -> &Option<PnClientRef> {
        &self.proxy_client
    }

    pub fn set_proxy_client(mut self, proxy_client: PnClientRef) -> Self {
        self.proxy_client = Some(proxy_client);
        self
    }

    pub fn sn_query_interval(&self) -> Duration {
        self.sn_query_interval
    }

    pub fn set_sn_query_interval(mut self, sn_query_interval: Duration) -> Self {
        self.sn_query_interval = sn_query_interval;
        self
    }
}

pub async fn create_p2p_stack(config: P2pStackConfig) -> P2pResult<P2pStackRef> {
    let gen_id = Arc::new(TunnelIdGenerator::new());
    let gen_seq = Arc::new(SequenceGenerator::new());
    let net_manager = config.env.net_manager.clone();
    let cert_factory = config.env.cert_factory.clone();
    let cert_resolver = config.env.sever_cert_resolver.clone();
    let local_identity = config.local_identity();
    let sn_list = config.sn_list().to_vec();
    let conn_timeout = config.conn_timeout();
    let sn_ping_interval = config.sn_ping_interval();
    let sn_call_timeout = config.sn_call_timeout();
    let device_finder = config.device_finder().clone();
    let idle_timeout = config.idle_timeout();
    let sn_tunnel_count = config.sn_tunnel_count();
    let sn_query_interval = config.sn_query_interval();
    let support_proxy = config.support_proxy();
    let proxy_client = config.proxy_client().clone();

    let sn_service = SNClientService::new(
        net_manager.clone(),
        sn_list,
        local_identity.clone(),
        gen_seq.clone(),
        gen_id.clone(),
        cert_factory.clone(),
        sn_tunnel_count,
        sn_ping_interval,
        sn_call_timeout,
        conn_timeout,
    );

    let proxy_client = if support_proxy {
        let proxy_client = if let Some(proxy_client) = proxy_client {
            proxy_client
        } else {
            let default = DefaultPnClient::new(sn_service.get_cmd_client().clone(),
                                               local_identity.clone(),
                                               cert_factory.clone(),
                                               cert_resolver.clone());
            default
        };
        Some(proxy_client)
    } else {
        None
    };

    let device_finder = if device_finder.is_some() {
        device_finder.unwrap()
    } else {
        DefaultDeviceFinder::new(sn_service.clone(), cert_factory.clone(), config.env.identity_cert_cache.clone(), sn_query_interval)
    };
    let tunnel_listener = TunnelListener::new(
        local_identity.clone(),
        net_manager.clone(),
        sn_service.clone(),
        proxy_client.clone(),
        0,
        cert_factory.clone(),
        conn_timeout,
    );
    tunnel_listener.listen();


    let _ = sn_service.start().await;

    let tunnel_id_gen = Arc::new(TunnelIdGenerator::new());
    let stream_manager = StreamManager::new(local_identity.clone(),
                                            net_manager.clone(),
                                            sn_service.clone(),
                                            device_finder.clone(),
                                            cert_factory.clone(),
                                            tunnel_id_gen.clone(),
                                            proxy_client.clone(),
                                            tunnel_listener.clone(),
                                            net_manager.get_connection_info_cache().clone(),
                                            0,
                                            conn_timeout,
                                            idle_timeout);
    let datagram_manager = DatagramManager::new(local_identity.clone(),
                                                net_manager.clone(),
                                                sn_service.clone(),
                                                device_finder.clone(),
                                                cert_factory.clone(),
                                                tunnel_id_gen.clone(),
                                                proxy_client.clone(),
                                                tunnel_listener.clone(),
                                                net_manager.get_connection_info_cache().clone(),
                                                0,
                                                conn_timeout,
                                                idle_timeout);

    Ok(Arc::new(P2pStack::new(
        config.env.clone(),
        local_identity.clone(),
        sn_service,
        stream_manager,
        datagram_manager,
        net_manager.clone(),).await?))
}
