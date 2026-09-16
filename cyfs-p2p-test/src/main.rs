extern crate core;

use p2p_frame::endpoint::{Endpoint, EndpointArea, Protocol};
use p2p_frame::error::{P2pError, P2pErrorCode, P2pResult};
use p2p_frame::networks::TunnelPurpose;
use p2p_frame::p2p_identity::{
    P2pId, P2pIdentityCertFactory, P2pIdentityCertRef, P2pIdentityRef, P2pSn,
};
use p2p_frame::pn::PnServer;
use p2p_frame::sn::client::{SNClientServiceRef, SnLocalIpProvider, SnLocalIpProviderRef};
use p2p_frame::sn::service::{create_sn_service, SnServiceConfig};
use p2p_frame::stack::{
    create_p2p_env, create_p2p_stack, DeviceFinder, DeviceFinderRef, P2pConfig as P2pFrameConfig,
    P2pEnvRef, P2pStackConfig, P2pStackRef, PnServerAddress,
};
use p2p_frame::x509::{X509IdentityCertFactory, X509IdentityFactory, generate_rsa_x509_identity};
use serde::{Deserialize, Serialize};
use sfo_reuseport::{ServerRuntime, ServerRuntimeConfig};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const APP_NAME: &str = "cyfs-p2p-test";
fn tunnel_purpose(value: u16) -> TunnelPurpose {
    TunnelPurpose::from_value(&value).unwrap()
}

fn build_identity(name: &str, endpoints: Vec<Endpoint>) -> P2pIdentityRef {
    let identity = generate_rsa_x509_identity(Some(name.to_owned()))
        .unwrap_or_else(|e| panic!("generate x509 identity {name} failed: {e:?}"));
    let identity: P2pIdentityRef = Arc::new(identity);
    identity.update_endpoints(endpoints)
}

fn build_sn_entry(identity: &P2pIdentityRef) -> P2pSn {
    let cert = identity
        .get_identity_cert()
        .expect("x509 identity cert must be available");
    P2pSn::new(cert.get_id(), cert.get_name(), cert.endpoints())
}

#[derive(Deserialize)]
pub struct TcpConfig {
    ep_list: Vec<EP>,
    port: Option<u16>,
}

#[derive(Deserialize)]
pub struct UdpConfig {
    ep_list: Vec<EP>,
    port: Option<u16>,
}

#[derive(Deserialize)]
pub struct TestP2pConfig {
    ep_list: Vec<EP>,
    port_map: Option<u16>,
    tcp: Option<TcpConfig>,
    udp: Option<UdpConfig>,
}

impl TestP2pConfig {
    pub fn get_ep_list(&self) -> Vec<Endpoint> {
        let mut eps = Vec::new();
        for ep in self.ep_list.iter() {
            eps.push(Endpoint::from((
                Protocol::Quic,
                SocketAddr::V4(SocketAddrV4::new(ep.ip.parse().unwrap(), ep.port)),
            )));
            eps.push(Endpoint::from((
                Protocol::Tcp,
                SocketAddr::V4(SocketAddrV4::new(ep.ip.parse().unwrap(), ep.port)),
            )));
        }
        if let Some(tcp) = self.tcp.as_ref() {
            for ep in tcp.ep_list.iter() {
                eps.push(Endpoint::from((
                    Protocol::Tcp,
                    SocketAddr::V4(SocketAddrV4::new(ep.ip.parse().unwrap(), ep.port)),
                )));
            }
        }
        if let Some(udp) = self.udp.as_ref() {
            for ep in udp.ep_list.iter() {
                eps.push(Endpoint::from((
                    Protocol::Quic,
                    SocketAddr::V4(SocketAddrV4::new(ep.ip.parse().unwrap(), ep.port)),
                )));
            }
        }
        eps
    }

    pub fn get_port_mapping(&self) -> Vec<(Endpoint, u16)> {
        let mut udp_map_port = self.port_map.clone();
        if self.udp.is_some() && self.udp.as_ref().unwrap().port.is_some() {
            udp_map_port = self.udp.as_ref().unwrap().port.clone();
        }

        let mut tcp_map_port = self.port_map.clone();
        if self.tcp.is_some() && self.tcp.as_ref().unwrap().port.is_some() {
            tcp_map_port = self.tcp.as_ref().unwrap().port.clone();
        }

        let ep_list = self.get_ep_list();
        let mut map_port_list = Vec::new();
        for ep in ep_list.iter() {
            if ep.protocol() == Protocol::Tcp {
                if let Some(port) = tcp_map_port {
                    map_port_list.push((ep.clone(), port));
                }
            } else {
                if let Some(port) = udp_map_port {
                    map_port_list.push((ep.clone(), port));
                }
            }
        }
        map_port_list
    }
}

#[derive(Serialize, Deserialize)]
pub struct EP {
    ip: String,
    port: u16,
}

#[derive(Serialize, Deserialize)]
pub struct Config {
    tcp_list: Vec<EP>,
    udp_list: Vec<EP>,
    sn: EP,
}

#[tokio::main]
async fn main() {
    sfo_log::Logger::new("cyfs-p2p-test")
        .set_log_to_file(true)
        .add_filter("quinn")
        .set_log_file_count(5)
        .set_log_level("debug")
        .start()
        .unwrap();

    let data_folder = std::env::current_dir().unwrap();
    let matches = clap::App::new(APP_NAME)
        .subcommand(clap::SubCommand::with_name("all-in-one").about("all in one"))
        .subcommand(
            clap::SubCommand::with_name("client")
                .arg(
                    clap::Arg::with_name("config")
                        .short("c")
                        .long("config")
                        .takes_value(true)
                        .default_value(data_folder.to_str().unwrap())
                        .help("config path"),
                )
                .arg(
                    clap::Arg::with_name("target")
                        .short("t")
                        .long("target")
                        .takes_value(true)
                        .help("target device id"),
                ),
        )
        .subcommand(
            clap::SubCommand::with_name("server").arg(
                clap::Arg::with_name("config")
                    .short("c")
                    .long("config")
                    .takes_value(true)
                    .default_value(data_folder.to_str().unwrap())
                    .help("config path"),
            ),
        )
        .get_matches();

    match matches.subcommand() {
        ("all-in-one", _) => {
            all_in_one().await;
            return;
        }
        ("client", Some(matches)) => {
            let data_folder = matches.value_of("config").unwrap();
            let target = matches.value_of("target").map_or(None, |v| {
                if let Ok(p2p_id) = P2pId::from_str(v) {
                    Some(p2p_id)
                } else {
                    None
                }
            });
            let data_folder = std::path::Path::new(data_folder);
            client_instance(data_folder, target).await;
        }
        ("server", Some(matches)) => {
            let data_folder = matches.value_of("config").unwrap();
            let data_folder = std::path::Path::new(data_folder);
            server_instance(data_folder).await;
        }
        _ => {
            println!("Please specify a subcommand");
            return;
        }
    }
    // let device_desc_path = data_folder.join("device.desc");
    // let device_key_path = data_folder.join("device.sec");
    // let config_path = data_folder.join("config.json");
    //
    // let config = std::fs::read_to_string(config_path.as_path()).unwrap();
    // let config: Config = serde_json::from_str(config.as_str()).unwrap();
    // let (mut sn_desc, _) = Device::decode_from_file(sn_desc_path.as_path(), &mut Vec::new()).unwrap();

    std::future::pending::<u8>().await;
}

async fn client_instance(data_folder: &Path, target: Option<P2pId>) {
    let sn_identity = build_identity(
        "sn",
        vec![Endpoint::from((
            Protocol::Quic,
            SocketAddr::V4(SocketAddrV4::new(
                Ipv4Addr::from_str("127.0.0.1").unwrap(),
                3456,
            )),
        ))],
    );
    let sn_list = vec![build_sn_entry(&sn_identity)];
    let config_path = data_folder.join("config.toml");
    let (local_eps, map_port_list) = if config_path.exists() {
        let config = std::fs::read_to_string(config_path.as_path()).unwrap();
        let config: TestP2pConfig = toml::from_str(config.as_str()).unwrap();
        let local_eps = config.get_ep_list();
        let map_port_list = config.get_port_mapping();
        (local_eps, Some(map_port_list))
    } else {
        let mut local_eps = Vec::new();
        let mut ep = Endpoint::from((
            Protocol::Quic,
            SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 4433)),
        ));
        ep.set_area(EndpointArea::Lan);
        local_eps.push(ep);
        (local_eps, None)
    };
    let server_runtime = ServerRuntime::start(ServerRuntimeConfig::default()).unwrap();
    let mut p2p_config = P2pFrameConfig::new(
        Arc::new(X509IdentityFactory),
        Arc::new(X509IdentityCertFactory),
        local_eps.clone(),
        server_runtime,
    );
    if let Some(map_port_list) = map_port_list {
        for (ep, port) in map_port_list.iter() {
            p2p_config = p2p_config.add_port_mapping((ep.clone(), *port));
        }
    }

    let env = create_p2p_env(p2p_config).await.unwrap();

    let stack = create_stack(env, "client", local_eps.clone(), sn_list.clone())
        .await
        .unwrap();
    stack.wait_online(None).await.unwrap();

    let remote_id = target.unwrap_or_else(|| {
        P2pId::from_str("5aSixgLnAyXzWaqpyKTz7hFkvzXMzJgGnxnuCg67JYJP")
            .expect("default p2p id must parse")
    });

    loop {
        {
            let (mut read, mut write) = stack
                .stream_manager()
                .connect_from_id(&remote_id, tunnel_purpose(80))
                .await
                .unwrap();
            write.write_all("test".as_bytes()).await.unwrap();
            let mut buf = [0u8; 1024];
            let len = read.read(buf.as_mut_slice()).await.unwrap();
            log::info!("recv {}", String::from_utf8_lossy(&buf[..len]));
        }
        {
            let (mut read, mut write) = stack
                .stream_manager()
                .connect_from_id(&remote_id, tunnel_purpose(80))
                .await
                .unwrap();
            write.write_all("test".as_bytes()).await.unwrap();
            let mut buf = [0u8; 1024];
            let len = read.read(buf.as_mut_slice()).await.unwrap();
            log::info!("recv {}", String::from_utf8_lossy(&buf[..len]));
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

async fn server_instance(data_folder: &Path) {
    let sn_identity = build_identity(
        "sn",
        vec![Endpoint::from((
            Protocol::Quic,
            SocketAddr::V4(SocketAddrV4::new(
                Ipv4Addr::from_str("127.0.0.1").unwrap(),
                3456,
            )),
        ))],
    );
    let sn_list = vec![build_sn_entry(&sn_identity)];

    let config_path = data_folder.join("config.toml");
    let (local_eps, map_port_lsit) = if config_path.exists() {
        let config = std::fs::read_to_string(config_path.as_path()).unwrap();
        let config: TestP2pConfig = toml::from_str(config.as_str()).unwrap();
        let local_eps = config.get_ep_list();
        let map_port_list = config.get_port_mapping();
        (local_eps, Some(map_port_list))
    } else {
        let mut local_eps = Vec::new();
        let mut ep = Endpoint::from((
            Protocol::Quic,
            SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 4433)),
        ));
        ep.set_area(EndpointArea::Lan);
        local_eps.push(ep);
        (local_eps, None)
    };
    let server_runtime = ServerRuntime::start(ServerRuntimeConfig::default()).unwrap();
    let mut p2p_config = P2pFrameConfig::new(
        Arc::new(X509IdentityFactory),
        Arc::new(X509IdentityCertFactory),
        local_eps.clone(),
        server_runtime,
    );
    if let Some(map_port_list) = map_port_lsit {
        for (ep, port) in map_port_list.iter() {
            p2p_config = p2p_config.add_port_mapping((ep.clone(), *port));
        }
    }

    let env = create_p2p_env(p2p_config).await.unwrap();

    let stack = create_stack(env, "server", local_eps.clone(), sn_list.clone())
        .await
        .unwrap();
    stack.wait_online(None).await.unwrap();

    let mut listener = stack
        .stream_manager()
        .listen(tunnel_purpose(80))
        .await
        .unwrap();
    loop {
        let (mut read, mut write) = listener.accept().await.unwrap();
        tokio::task::spawn(async move {
            let mut buf = [0u8; 1024];
            let len = read.read(buf.as_mut_slice()).await.unwrap();
            println!("read {}", String::from_utf8_lossy(&buf[..len]));
            write.write("hello".as_bytes()).await.unwrap();
        });
    }
}

#[derive(Clone, Copy)]
enum CaseExpectation {
    Success,
    Fail,
}

struct CaseMetric {
    total: u64,
    passed: u64,
    failed: u64,
    total_latency_ms: u128,
    max_latency_ms: u128,
}

struct OverrideDeviceFinder {
    sn_client: SNClientServiceRef,
    cert_factory: Arc<X509IdentityCertFactory>,
    override_eps: HashMap<P2pId, Vec<Endpoint>>,
    blocked_ids: HashSet<P2pId>,
    blocked_all: bool,
}

struct EmptySnLocalIpProvider;

impl SnLocalIpProvider for EmptySnLocalIpProvider {
    fn get_local_ips(&self) -> Vec<IpAddr> {
        Vec::new()
    }
}

impl OverrideDeviceFinder {
    fn new(
        sn_client: SNClientServiceRef,
        cert_factory: Arc<X509IdentityCertFactory>,
        override_eps: HashMap<P2pId, Vec<Endpoint>>,
    ) -> DeviceFinderRef {
        Self::new_with_block(sn_client, cert_factory, override_eps, HashSet::new(), false)
    }

    fn new_with_block(
        sn_client: SNClientServiceRef,
        cert_factory: Arc<X509IdentityCertFactory>,
        override_eps: HashMap<P2pId, Vec<Endpoint>>,
        blocked_ids: HashSet<P2pId>,
        blocked_all: bool,
    ) -> DeviceFinderRef {
        Arc::new(Self {
            sn_client,
            cert_factory,
            override_eps,
            blocked_ids,
            blocked_all,
        })
    }
}

#[async_trait::async_trait]
impl DeviceFinder for OverrideDeviceFinder {
    async fn get_identity_cert(&self, device_id: &P2pId) -> P2pResult<P2pIdentityCertRef> {
        if self.blocked_all || self.blocked_ids.contains(device_id) {
            return Err(P2pError::from((
                P2pErrorCode::NotFound,
                format!("device {} blocked by test finder", device_id),
                std::io::Error::new(std::io::ErrorKind::NotFound, "device blocked"),
            )));
        }

        let resp = self.sn_client.query(device_id).await?;
        let peer_info = match resp.peer_info {
            Some(peer_info) => peer_info,
            None => {
                return Err(P2pError::from((
                    P2pErrorCode::NotFound,
                    "device not found".to_string(),
                    std::io::Error::new(std::io::ErrorKind::NotFound, "device not found"),
                )));
            }
        };
        let cert = self.cert_factory.create(&peer_info)?;
        let eps = self
            .override_eps
            .get(device_id)
            .cloned()
            .unwrap_or(resp.end_point_array);
        Ok(cert.update_endpoints(eps))
    }
}

impl CaseMetric {
    fn new() -> Self {
        Self {
            total: 0,
            passed: 0,
            failed: 0,
            total_latency_ms: 0,
            max_latency_ms: 0,
        }
    }

    fn add(&mut self, passed: bool, latency_ms: u128) {
        self.total += 1;
        if passed {
            self.passed += 1;
        } else {
            self.failed += 1;
        }
        self.total_latency_ms += latency_ms;
        if latency_ms > self.max_latency_ms {
            self.max_latency_ms = latency_ms;
        }
    }

    fn avg_latency_ms(&self) -> u128 {
        if self.total == 0 {
            0
        } else {
            self.total_latency_ms / self.total as u128
        }
    }
}

async fn run_case<F, Fut>(
    stats: &mut HashMap<&'static str, CaseMetric>,
    name: &'static str,
    expect: CaseExpectation,
    f: F,
) -> bool
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = P2pResult<()>>,
{
    let now = Instant::now();
    let ret = f().await;
    let latency_ms = now.elapsed().as_millis();
    let passed = match expect {
        CaseExpectation::Success => ret.is_ok(),
        CaseExpectation::Fail => ret.is_err(),
    };

    let metric = stats.entry(name).or_insert_with(CaseMetric::new);
    metric.add(passed, latency_ms);

    match ret {
        Ok(_) => {
            if passed {
                log::info!("[case:{}] pass latency={}ms", name, latency_ms);
            } else {
                log::warn!(
                    "[case:{}] unexpected success latency={}ms",
                    name,
                    latency_ms
                );
            }
        }
        Err(e) => {
            if passed {
                log::info!(
                    "[case:{}] pass(expected fail) err={:?}latency={}ms",
                    name,
                    e,
                    latency_ms
                );
            } else {
                log::error!("[case:{}] fail err={:?} latency={}ms", name, e, latency_ms);
            }
        }
    }

    passed
}

fn print_case_stats(round: u64, stats: &HashMap<&'static str, CaseMetric>) {
    let mut total = 0u64;
    let mut passed = 0u64;
    for metric in stats.values() {
        total += metric.total;
        passed += metric.passed;
    }
    let rate = if total == 0 {
        0.0
    } else {
        (passed as f64) * 100.0 / (total as f64)
    };
    log::info!(
        "[all-in-one] round={} total={} passed={} pass_rate={:.2}%",
        round,
        total,
        passed,
        rate
    );

    for (name, metric) in stats.iter() {
        log::info!(
            "[all-in-one] case={} total={} pass={} fail={} avg={}ms max={}ms",
            name,
            metric.total,
            metric.passed,
            metric.failed,
            metric.avg_latency_ms(),
            metric.max_latency_ms
        );
    }
}

async fn start_stream_listener(stack: P2pStackRef, port: u16, label: &'static str) {
    let mut listener = stack
        .stream_manager()
        .listen(tunnel_purpose(port))
        .await
        .unwrap();
    loop {
        let (mut read, mut write) = listener.accept().await.unwrap();
        tokio::task::spawn(async move {
            let ret: std::io::Result<()> = async {
                write
                    .write_all(format!("{}-hello", label).as_bytes())
                    .await?;
                write.flush().await?;
                let mut buf = [0u8; 128];
                let len = read.read(buf.as_mut_slice()).await?;
                log::info!(
                    "[listener:{}] recv {}",
                    label,
                    String::from_utf8_lossy(&buf[..len])
                );
                Ok(())
            }
            .await;

            if let Err(e) = ret {
                log::warn!("[listener:{}] io err {:?}", label, e);
            }
        });
    }
}

async fn start_datagram_listener(stack: P2pStackRef, port: u16, label: &'static str) {
    let mut listener = stack
        .datagram_manager()
        .listen(tunnel_purpose(port))
        .await
        .unwrap();
    loop {
        let mut recv = listener.accept().await.unwrap();
        tokio::task::spawn(async move {
            let mut buf = [0u8; 128];
            match recv.read(buf.as_mut_slice()).await {
                Ok(len) => {
                    log::info!(
                        "[datagram:{}] recv {}",
                        label,
                        String::from_utf8_lossy(&buf[..len])
                    );
                }
                Err(e) => {
                    log::warn!("[datagram:{}] read err {:?}", label, e);
                }
            }
        });
    }
}

async fn all_in_one() {
    let server_runtime = ServerRuntime::start(ServerRuntimeConfig::default()).unwrap();
    let identity_factory = Arc::new(X509IdentityFactory);
    let cert_factory = Arc::new(X509IdentityCertFactory);
    let sn_identity = build_identity(
        "sn",
        vec![Endpoint::from((
            Protocol::Quic,
            SocketAddr::V4(SocketAddrV4::new(
                Ipv4Addr::from_str("127.0.0.1").unwrap(),
                3456,
            )),
        ))],
    );
    let sn_list = vec![build_sn_entry(&sn_identity)];

    let sn_service = SnServiceConfig::new(
        sn_identity,
        identity_factory.clone(),
        cert_factory.clone(),
        server_runtime.clone(),
    );
    let sn_service = create_sn_service(sn_service).await.unwrap();
    let _pn_server = PnServer::new(sn_service.ttp_server());
    _pn_server.start().await.unwrap();
    sn_service.start().await.unwrap();

    let mut env_eps = Vec::new();
    let mut env_ep = Endpoint::from((
        Protocol::Quic,
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 4433)),
    ));
    env_ep.set_area(EndpointArea::Lan);
    env_eps.push(env_ep);

    let mut p2p_config = P2pFrameConfig::new(
        identity_factory,
        cert_factory.clone(),
        env_eps.clone(),
        server_runtime,
    );
    p2p_config = p2p_config
        .set_tcp_accept_timout(std::time::Duration::from_secs(5))
        .set_quic_connect_timeout(Duration::from_secs(5))
        .set_quic_idle_time(Duration::from_secs(60));
    let env = create_p2p_env(p2p_config).await.unwrap();

    let mut direct_eps = Vec::new();
    let mut direct_ep = Endpoint::from((
        Protocol::Quic,
        SocketAddr::V4(SocketAddrV4::new(
            Ipv4Addr::from_str("127.0.0.1").unwrap(),
            4433,
        )),
    ));
    direct_ep.set_area(EndpointArea::Lan);
    direct_eps.push(direct_ep);

    let mut reverse_eps = Vec::new();
    let mut reverse_ep = Endpoint::from((
        Protocol::Quic,
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 4433)),
    ));
    reverse_ep.set_area(EndpointArea::Lan);
    reverse_eps.push(reverse_ep);

    let stack_direct_target = create_stack(
        env.clone(),
        "direct-target",
        direct_eps.clone(),
        sn_list.clone(),
    )
    .await
    .unwrap();
    stack_direct_target.wait_online(None).await.unwrap();

    let stack_reverse_target = create_stack(
        env.clone(),
        "reverse-target",
        reverse_eps.clone(),
        sn_list.clone(),
    )
    .await
    .unwrap();
    stack_reverse_target.wait_online(None).await.unwrap();

    let stack_client = create_stack(
        env.clone(),
        "client",
        direct_eps.clone(),
        sn_list.clone(),
    )
    .await
    .unwrap();
    stack_client.wait_online(None).await.unwrap();

    let cert_factory = Arc::new(X509IdentityCertFactory);
    let proxy_target_finder = OverrideDeviceFinder::new_with_block(
        stack_client.sn_client().clone(),
        cert_factory.clone(),
        HashMap::new(),
        HashSet::new(),
        true,
    );

    let stack_proxy_target = create_stack_with_device_finder(
        env.clone(),
        "proxy-target",
        reverse_eps.clone(),
        sn_list.clone(),
        Some(proxy_target_finder),
        None,
    )
    .await
    .unwrap();
    stack_proxy_target.wait_online(None).await.unwrap();

    let direct_target_id = stack_direct_target
        .local_identity()
        .get_identity_cert()
        .unwrap()
        .get_id();
    let client_id = stack_client
        .local_identity()
        .get_identity_cert()
        .unwrap()
        .get_id();
    let reverse_target_id = stack_reverse_target
        .local_identity()
        .get_identity_cert()
        .unwrap()
        .get_id();
    let proxy_target_id = stack_proxy_target
        .local_identity()
        .get_identity_cert()
        .unwrap()
        .get_id();

    let unreachable_eps = vec![p2p_frame::endpoint::Endpoint::from((
        p2p_frame::endpoint::Protocol::Quic,
        SocketAddr::V6(SocketAddrV6::new(
            Ipv6Addr::from_str("::1").unwrap(),
            6553,
            0,
            0,
        )),
    ))];

    let mut reverse_override = HashMap::new();
    reverse_override.insert(reverse_target_id.clone(), unreachable_eps.clone());
    let reverse_finder = OverrideDeviceFinder::new(
        stack_client.sn_client().clone(),
        cert_factory.clone(),
        reverse_override,
    );

    let mut proxy_override = HashMap::new();
    proxy_override.insert(proxy_target_id.clone(), unreachable_eps.clone());
    let proxy_finder = OverrideDeviceFinder::new(
        stack_client.sn_client().clone(),
        cert_factory,
        proxy_override,
    );

    let stack_reverse_caller = create_stack_with_device_finder(
        env.clone(),
        "reverse-caller",
        direct_eps.clone(),
        sn_list.clone(),
        Some(reverse_finder),
        None,
    )
    .await
    .unwrap();
    stack_reverse_caller.wait_online(None).await.unwrap();

    let stack_proxy_caller = create_stack_with_device_finder(
        env.clone(),
        "proxy-caller",
        reverse_eps.clone(),
        sn_list.clone(),
        Some(proxy_finder),
        Some(Arc::new(EmptySnLocalIpProvider)),
    )
    .await
    .unwrap();
    stack_proxy_caller.wait_online(None).await.unwrap();

    tokio::task::spawn(start_stream_listener(
        stack_direct_target.clone(),
        1234,
        "direct",
    ));
    tokio::task::spawn(start_stream_listener(stack_client.clone(), 1234, "client"));
    tokio::task::spawn(start_datagram_listener(
        stack_direct_target.clone(),
        1234,
        "direct",
    ));
    tokio::task::spawn(start_stream_listener(
        stack_reverse_target.clone(),
        2234,
        "reverse",
    ));
    tokio::task::spawn(start_stream_listener(
        stack_proxy_target.clone(),
        3234,
        "proxy",
    ));
    tokio::task::spawn(start_datagram_listener(
        stack_proxy_target.clone(),
        3236,
        "proxy",
    ));

    let mut stats: HashMap<&'static str, CaseMetric> = HashMap::new();
    let mut round: u64 = 0;
    let mut stat_tick = tokio::time::interval(Duration::from_secs(10));

    loop {
        round += 1;

        if !run_case(
            &mut stats,
            "stream_direct_success",
            CaseExpectation::Success,
            || async {
                let (mut read, mut write) = stack_client
                    .stream_manager()
                    .connect_from_id(&direct_target_id, tunnel_purpose(1234))
                    .await?;
                let mut buf = [0u8; 64];
                let _ = read.read(buf.as_mut_slice()).await.map_err(|e| {
                    P2pError::from((P2pErrorCode::Failed, "stream read failed".to_string(), e))
                })?;
                write
                    .write_all("stream direct".as_bytes())
                    .await
                    .map_err(|e| {
                        P2pError::from((P2pErrorCode::Failed, "stream write failed".to_string(), e))
                    })?;
                write.flush().await.map_err(|e| {
                    P2pError::from((P2pErrorCode::Failed, "stream flush failed".to_string(), e))
                })?;
                Ok(())
            },
        )
        .await
        {
            log::error!("all-in-one stop: case stream_direct_success failed");
            return;
        }

        if !run_case(
            &mut stats,
            "stream_direct_return_success",
            CaseExpectation::Success,
            || async {
                let (mut read, mut write) = stack_direct_target
                    .stream_manager()
                    .connect_from_id(&client_id, tunnel_purpose(1234))
                    .await?;
                let mut buf = [0u8; 64];
                let _ = read.read(buf.as_mut_slice()).await.map_err(|e| {
                    P2pError::from((
                        P2pErrorCode::Failed,
                        "stream return read failed".to_string(),
                        e,
                    ))
                })?;
                write
                    .write_all("stream direct return".as_bytes())
                    .await
                    .map_err(|e| {
                        P2pError::from((
                            P2pErrorCode::Failed,
                            "stream return write failed".to_string(),
                            e,
                        ))
                    })?;
                write.flush().await.map_err(|e| {
                    P2pError::from((
                        P2pErrorCode::Failed,
                        "stream return flush failed".to_string(),
                        e,
                    ))
                })?;
                Ok(())
            },
        )
        .await
        {
            log::error!("all-in-one stop: case stream_direct_return_success failed");
            return;
        }

        if !run_case(
            &mut stats,
            "stream_wrong_port_fail_expected",
            CaseExpectation::Fail,
            || async {
                let _ = stack_client
                    .stream_manager()
                    .connect_from_id(&direct_target_id, tunnel_purpose(19999))
                    .await?;
                Ok(())
            },
        )
        .await
        {
            log::error!("all-in-one stop: case stream_wrong_port_fail_expected failed");
            return;
        }

        if !run_case(
            &mut stats,
            "datagram_direct_success",
            CaseExpectation::Success,
            || async {
                let mut send = stack_client
                    .datagram_manager()
                    .connect_from_id(&direct_target_id, tunnel_purpose(1234))
                    .await?;
                send.write_all("datagram direct".as_bytes())
                    .await
                    .map_err(|e| {
                        P2pError::from((
                            P2pErrorCode::Failed,
                            "datagram write failed".to_string(),
                            e,
                        ))
                    })?;
                Ok(())
            },
        )
        .await
        {
            log::error!("all-in-one stop: case datagram_direct_success failed");
            return;
        }

        if !run_case(
            &mut stats,
            "direct_bad_endpoint_fail_expected",
            CaseExpectation::Fail,
            || async {
                let (_read, _write) = stack_client
                    .stream_manager()
                    .connect_direct(
                        vec![p2p_frame::endpoint::Endpoint::from((
                            p2p_frame::endpoint::Protocol::Quic,
                            SocketAddr::V6(SocketAddrV6::new(
                                Ipv6Addr::from_str("::1").unwrap(),
                                55555,
                                0,
                                0,
                            )),
                        ))],
                        tunnel_purpose(1234),
                        &P2pId::default(),
                    )
                    .await?;
                Ok(())
            },
        )
        .await
        {
            log::error!("all-in-one stop: case direct_bad_endpoint_fail_expected failed");
            return;
        }

        if !run_case(
            &mut stats,
            "reverse_connect_case",
            CaseExpectation::Success,
            || async {
                let (mut read, mut write) = stack_reverse_caller
                    .stream_manager()
                    .connect_from_id(&reverse_target_id, tunnel_purpose(2234))
                    .await?;
                let mut buf = [0u8; 64];
                let _ = read.read(buf.as_mut_slice()).await.map_err(|e| {
                    P2pError::from((P2pErrorCode::Failed, "reverse read failed".to_string(), e))
                })?;
                write
                    .write_all("reverse hello".as_bytes())
                    .await
                    .map_err(|e| {
                        P2pError::from((
                            P2pErrorCode::Failed,
                            "reverse write failed".to_string(),
                            e,
                        ))
                    })?;
                write.flush().await.map_err(|e| {
                    P2pError::from((P2pErrorCode::Failed, "reverse flush failed".to_string(), e))
                })?;
                Ok(())
            },
        )
        .await
        {
            log::error!("all-in-one stop: case reverse_connect_case failed");
            return;
        }

        if !run_case(
            &mut stats,
            "reverse_fail_case",
            CaseExpectation::Fail,
            || async {
                let _ = stack_reverse_caller
                    .stream_manager()
                    .connect_from_id(&reverse_target_id, tunnel_purpose(2235))
                    .await?;
                Ok(())
            },
        )
        .await
        {
            log::error!("all-in-one stop: case reverse_fail_case failed");
            return;
        }

        if !run_case(
            &mut stats,
            "proxy_connect_case",
            CaseExpectation::Success,
            || async {
                let (mut read, mut write) = stack_proxy_caller
                    .stream_manager()
                    .connect_from_id(&proxy_target_id, tunnel_purpose(3234))
                    .await?;
                let mut buf = [0u8; 64];
                let _ = read.read(buf.as_mut_slice()).await.map_err(|e| {
                    P2pError::from((P2pErrorCode::Failed, "proxy read failed".to_string(), e))
                })?;
                write
                    .write_all("proxy hello".as_bytes())
                    .await
                    .map_err(|e| {
                        P2pError::from((P2pErrorCode::Failed, "proxy write failed".to_string(), e))
                    })?;
                write.flush().await.map_err(|e| {
                    P2pError::from((P2pErrorCode::Failed, "proxy flush failed".to_string(), e))
                })?;
                Ok(())
            },
        )
        .await
        {
            log::error!("all-in-one stop: case proxy_connect_case failed");
            return;
        }

        if !run_case(
            &mut stats,
            "proxy_datagram_case",
            CaseExpectation::Success,
            || async {
                let mut send = stack_proxy_caller
                    .datagram_manager()
                    .connect_from_id(&proxy_target_id, tunnel_purpose(3236))
                    .await?;
                send.write_all("proxy datagram".as_bytes())
                    .await
                    .map_err(|e| {
                        P2pError::from((
                            P2pErrorCode::Failed,
                            "proxy datagram write failed".to_string(),
                            e,
                        ))
                    })?;
                Ok(())
            },
        )
        .await
        {
            log::error!("all-in-one stop: case proxy_datagram_case failed");
            return;
        }

        if !run_case(
            &mut stats,
            "proxy_fail_case",
            CaseExpectation::Fail,
            || async {
                let _ = stack_proxy_caller
                    .stream_manager()
                    .connect_from_id(&proxy_target_id, tunnel_purpose(3235))
                    .await?;
                Ok(())
            },
        )
        .await
        {
            log::error!("all-in-one stop: case proxy_fail_case failed");
            return;
        }

        if !run_case(
            &mut stats,
            "reconnect_stability",
            CaseExpectation::Success,
            || async {
                for _ in 0..3 {
                    let (_read, mut write) = stack_client
                        .stream_manager()
                        .connect_from_id(&direct_target_id, tunnel_purpose(1234))
                        .await?;
                    write.write_all("reconnect".as_bytes()).await.map_err(|e| {
                        P2pError::from((
                            P2pErrorCode::Failed,
                            "reconnect write failed".to_string(),
                            e,
                        ))
                    })?;
                    write.flush().await.map_err(|e| {
                        P2pError::from((
                            P2pErrorCode::Failed,
                            "reconnect flush failed".to_string(),
                            e,
                        ))
                    })?;
                }
                Ok(())
            },
        )
        .await
        {
            log::error!("all-in-one stop: case reconnect_stability failed");
            return;
        }

        if !run_case(
            &mut stats,
            "concurrent_stream_burst",
            CaseExpectation::Success,
            || async {
                let mut tasks = Vec::new();
                for _ in 0..10 {
                    let stack = stack_client.clone();
                    let remote = direct_target_id.clone();
                    let task = tokio::task::spawn(async move {
                        let (_read, mut write) = stack
                            .stream_manager()
                            .connect_from_id(&remote, tunnel_purpose(1234))
                            .await?;
                        write.write_all("burst".as_bytes()).await.map_err(|e| {
                            P2pError::from((
                                P2pErrorCode::Failed,
                                "burst write failed".to_string(),
                                e,
                            ))
                        })?;
                        write.flush().await.map_err(|e| {
                            P2pError::from((
                                P2pErrorCode::Failed,
                                "burst flush failed".to_string(),
                                e,
                            ))
                        })?;
                        Ok::<(), P2pError>(())
                    });
                    tasks.push(task);
                }

                for task in tasks {
                    let ret = task.await.map_err(|e| {
                        P2pError::from((P2pErrorCode::Failed, "join task failed".to_string(), e))
                    })?;
                    ret?;
                }

                Ok(())
            },
        )
        .await
        {
            log::error!("all-in-one stop: case concurrent_stream_burst failed");
            return;
        }

        tokio::select! {
            _ = stat_tick.tick() => {
                print_case_stats(round, &stats);
            }
            _ = tokio::time::sleep(Duration::from_millis(1000)) => {}
        }
    }
}
async fn create_stack_with_device_finder(
    env: P2pEnvRef,
    identity_name: &str,
    eps: Vec<Endpoint>,
    sn_list: Vec<P2pSn>,
    device_finder: Option<DeviceFinderRef>,
    local_ip_provider: Option<SnLocalIpProviderRef>,
) -> P2pResult<P2pStackRef> {
    let local_identity = build_identity(identity_name, eps);
    let mut config = P2pStackConfig::new(env, local_identity)
        .add_sn_list(sn_list)
        .set_support_proxy(true)
        .set_proxy_server(PnServerAddress::Sn)
        .set_proxy_stream_encrypted(true);
    if let Some(device_finder) = device_finder {
        config = config.set_device_finder(device_finder);
    }
    if let Some(local_ip_provider) = local_ip_provider {
        config = config.set_local_ip_provider(local_ip_provider);
    }

    create_p2p_stack(config).await
}

async fn create_stack(
    env: P2pEnvRef,
    identity_name: &str,
    eps: Vec<Endpoint>,
    sn_list: Vec<P2pSn>,
) -> P2pResult<P2pStackRef> {
    create_stack_with_device_finder(env, identity_name, eps, sn_list, None, None).await
}
