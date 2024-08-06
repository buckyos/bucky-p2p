extern crate core;

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::path::Path;
use std::str::FromStr;
use std::thread;
use std::time::Duration;
use bucky_crypto::PrivateKey;
use bucky_objects::{Area, Device, DeviceCategory, DeviceId, Endpoint, EndpointArea, NamedObject, Protocol, UniqueId};
use bucky_raw_codec::FileDecoder;
use flexi_logger::{Cleanup, Criterion, DeferredNow, Duplicate, FileSpec, Naming, Record};
use cyfs_p2p::{create_p2p_stack, IncreaseIdGenerator, init_p2p, LocalDevice, P2pStackRef};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use cyfs_p2p::error::{BdtError, BdtErrorCode, BdtResult};
use cyfs_p2p::protocol::{ReceiptWithSignature, SnServiceReceipt};
use cyfs_p2p::sn::service::{IsAcceptClient, ReceiptRequestTime, SnService, SnServiceContractServer};

const APP_NAME: &str = "cyfs-p2p-test";

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
pub struct P2pConfig {
    ep_list: Vec<EP>,
    port_map: Option<u16>,
    tcp: Option<TcpConfig>,
    udp: Option<UdpConfig>,
}

impl P2pConfig {
    pub fn get_ep_list(&self) -> Vec<Endpoint> {
        let mut eps = Vec::new();
        for ep in self.ep_list.iter() {
            eps.push(Endpoint::from((Protocol::Udp, SocketAddr::V4(SocketAddrV4::new(ep.ip.parse().unwrap(), ep.port)))));
            eps.push(Endpoint::from((Protocol::Tcp, SocketAddr::V4(SocketAddrV4::new(ep.ip.parse().unwrap(), ep.port)))));
        }
        if let Some(tcp) = self.tcp.as_ref() {
            for ep in tcp.ep_list.iter() {
                eps.push(Endpoint::from((Protocol::Tcp, SocketAddr::V4(SocketAddrV4::new(ep.ip.parse().unwrap(), ep.port)))));
            }
        }
        if let Some(udp) = self.udp.as_ref() {
            for ep in udp.ep_list.iter() {
                eps.push(Endpoint::from((Protocol::Udp, SocketAddr::V4(SocketAddrV4::new(ep.ip.parse().unwrap(), ep.port)))));
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

fn custom_format(writer: &mut dyn std::io::Write, now: &mut DeferredNow, record: &Record) -> std::io::Result<()> {
    let file = match record.file() {
        None => {
            "<unknown>".to_string()
        }
        Some(path) => {
            Path::new(path).file_name().map(|v| v.to_string_lossy().to_string()).unwrap_or("<unknown>".to_string())
        }
    };
    write!(
        writer,
        "{} [{}] [{}:{}] [{}] - {}",
        now.format("%Y-%m-%d %H:%M:%S"),
        record.level(),
        file,
        record.line().unwrap_or(0),
        thread::current().name().unwrap_or("<unnamed>"),
        &record.args()
    )
}

struct SnServiceContractServerImpl {}

impl SnServiceContractServerImpl {
    fn new() -> SnServiceContractServerImpl {
        SnServiceContractServerImpl {}
    }
}

impl SnServiceContractServer for SnServiceContractServerImpl {
    fn check_receipt(
        &self,
        _client_device: &Device,
        _local_receipt: &SnServiceReceipt,
        _client_receipt: &Option<ReceiptWithSignature>,
        _last_request_time: &ReceiptRequestTime,
    ) -> IsAcceptClient {
        IsAcceptClient::Accept(false)
    }

    fn verify_auth(&self, _client_device_id: &DeviceId) -> IsAcceptClient {
        IsAcceptClient::Accept(false)
    }
}

#[tokio::main]
async fn main() {
    flexi_logger::Logger::try_with_str("info")
        .unwrap()
        .log_to_file(FileSpec::default().directory(std::env::current_dir().unwrap().join("logs")))
        .duplicate_to_stderr(Duplicate::All)
        .rotate(Criterion::Size(10 * 1024 * 1024), // 文件大小达到 10MB 时轮转
                Naming::Numbers, // 使用数字命名轮转文件
                Cleanup::KeepLogFiles(7), // 保留最近 7 个日志文件
        ).format(custom_format)
        .start().unwrap();

    let data_folder = std::env::current_dir().unwrap();
    let matches = clap::App::new(APP_NAME)
        .subcommand(clap::SubCommand::with_name("all-in-one").about("all in one"))
        .subcommand(clap::SubCommand::with_name("client").arg(
            clap::Arg::with_name("config").short("c").long("config").takes_value(true)
                .default_value(data_folder.to_str().unwrap())
                .help("config path")
        ).arg(
            clap::Arg::with_name("target").short("t").long("target").takes_value(true)
                .help("target device id")
        ))
        .subcommand(clap::SubCommand::with_name("server").arg(
            clap::Arg::with_name("config").short("c").long("config").takes_value(true)
                .default_value(data_folder.to_str().unwrap())
                .help("config path")
        )).get_matches();

    match matches.subcommand() {
        ("all-in-on", _) => {
            all_in_one().await;
        }
        ("client", Some(matches)) => {
            let data_folder = matches.value_of("config").unwrap();
            let target = matches.value_of("target").map_or(None, |v| {
                if let Ok(device_id) = DeviceId::from_str(v) {
                    Some(device_id)
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

async fn client_instance(data_folder: &Path, target: Option<DeviceId>) {
    let sn_desc_path = data_folder.join("sn.desc");
    let sn_desc = Device::decode_from_file(sn_desc_path.as_path(), &mut Vec::new()).unwrap().0;
    let config_path = data_folder.join("config.toml");
    let (local_eps, map_port_lsit) = if config_path.exists() {
        let config = std::fs::read_to_string(config_path.as_path()).unwrap();
        let config: P2pConfig = toml::from_str(config.as_str()).unwrap();
        let local_eps = config.get_ep_list();
        let map_port_list = config.get_port_mapping();
        (local_eps, Some(map_port_list))
    } else {
        let mut local_eps = Vec::new();
        let mut ep = Endpoint::from((Protocol::Udp, SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 4433))));
        ep.set_area(EndpointArea::Lan);
        local_eps.push(ep);
        (local_eps, None)
    };
    init_p2p(local_eps.as_slice(), map_port_lsit, std::time::Duration::from_secs(300)).await.unwrap();

    let stack = create_stack(data_folder, local_eps.clone(), vec![sn_desc.clone()]).await.unwrap();
    stack.wait_online(None).await.unwrap();

    // let resp = stack.sn_client().query(&DeviceId::from_str("5aSixgM5JhQHzm2DDaWRsAS24QdR3DhvDr2ZDn5aJj6w").unwrap()).await.unwrap();
    let resp = if target.is_some() {
        stack.sn_client().query(target.as_ref().unwrap()).await.unwrap()
    } else {
        stack.sn_client().query(&DeviceId::from_str("5aSixgLnAyXzWaqpyKTz7hFkvzXMzJgGnxnuCg67JYJP").unwrap()).await.unwrap()
    };
    log::info!("query resp {:?}", resp);

    if let Some(mut remote) = resp.peer_info {
        remote.mut_connect_info().mut_endpoints().extend_from_slice(resp.end_point_array.as_slice());
        {
            let mut stream = stack.stream_manager().connect(&remote, 80).await.unwrap();
            stream.write_all("test".as_bytes()).await.unwrap();
            let mut buf = [0u8; 1024];
            let len = stream.read(buf.as_mut_slice()).await.unwrap();
            log::info!("recv {}", String::from_utf8_lossy(&buf[..len]));
        }
        {
            let mut stream = stack.stream_manager().connect(&remote, 80).await.unwrap();
            stream.write_all("test".as_bytes()).await.unwrap();
            let mut buf = [0u8; 1024];
            let len = stream.read(buf.as_mut_slice()).await.unwrap();
            log::info!("recv {}", String::from_utf8_lossy(&buf[..len]));
        }
    }
}

async fn server_instance(data_folder: &Path) {
    let sn_desc_path = data_folder.join("sn.desc");
    let sn_desc = Device::decode_from_file(sn_desc_path.as_path(), &mut Vec::new()).unwrap().0;

    let config_path = data_folder.join("config.toml");
    let (local_eps, map_port_lsit) = if config_path.exists() {
        let config = std::fs::read_to_string(config_path.as_path()).unwrap();
        let config: P2pConfig = toml::from_str(config.as_str()).unwrap();
        let local_eps = config.get_ep_list();
        let map_port_list = config.get_port_mapping();
        (local_eps, Some(map_port_list))
    } else {
        let mut local_eps = Vec::new();
        let mut ep = Endpoint::from((Protocol::Udp, SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 4433))));
        ep.set_area(EndpointArea::Lan);
        local_eps.push(ep);
        (local_eps, None)
    };
    init_p2p(local_eps.as_slice(), map_port_lsit, std::time::Duration::from_secs(300)).await.unwrap();

    let stack = create_stack(data_folder, local_eps.clone(), vec![sn_desc.clone()]).await.unwrap();
    stack.wait_online(None).await.unwrap();

    let listener = stack.stream_manager().listen(80).await.unwrap();
    loop {
        let mut stream = listener.accept().await.unwrap();
        tokio::task::spawn(async move {
            let mut buf = [0u8; 1024];
            let len = stream.read(buf.as_mut_slice()).await.unwrap();
            println!("read {}", String::from_utf8_lossy(&buf[..len]));
            stream.write("hello".as_bytes()).await.unwrap();
        });
    }
}

async fn all_in_one() {
    let sn_key = PrivateKey::generate_rsa(1024).map_err(|e| BdtError::from((BdtErrorCode::Failed, "", e))).unwrap();
    let sn_public_key = sn_key.public();
    let mut sn_desc = Device::new(None, UniqueId::default(), vec![], vec![], vec![], sn_public_key, Area::default(), DeviceCategory::OOD).build();

    let eps = sn_desc.mut_connect_info().mut_endpoints();
    if eps.len() == 0 {
        eps.push(Endpoint::from((Protocol::Udp, SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0,1), 3456)))));
        // eps.push(Endpoint::from((Protocol::Udp, SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, 3456, 0, 0)))));
    }

    let sn_service = SnService::new(
        LocalDevice::new(sn_desc.clone(), sn_key),
        Box::new(SnServiceContractServerImpl::new()),
    ).await;
    sn_service.start().await.unwrap();

    //
    // let (device_desc, _) = Device::decode_from_file(device_desc_path.as_path(), &mut Vec::new()).unwrap();
    // let (device_key, _) = PrivateKey::decode_from_file(device_key_path.as_path(), &mut Vec::new()).unwrap();

    // let unique_id = String::from_utf8_lossy(device_desc.desc().unique_id().as_slice());
    // cyfs_debug::CyfsLoggerBuilder::new_app(APP_NAME)
    //     .level("debug")
    //     .console("debug")
    //     .build()
    //     .unwrap()
    //     .start();

    // cyfs_debug::PanicBuilder::new(APP_NAME, "test")
    //     .exit_on_panic(true)
    //     .build()
    //     .start();

    let mut local_eps = Vec::new();
    // for ep in config.tcp_list.iter() {
    //     local_eps.push(Endpoint::from((Protocol::Tcp, SocketAddr::V4(SocketAddrV4::new(ep.ip.parse().unwrap(), ep.port))));
    // }
    // for ep in config.udp_list.iter() {
    //     local_eps.push(Endpoint::from((Protocol::Udp, SocketAddr::V4(SocketAddrV4::new(ep.ip.parse().unwrap(), ep.port))));
    // }
    // let mut ep = Endpoint::from((Protocol::Tcp, SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 4433))));
    // ep.set_area(EndpointArea::Wan);
    // local_eps.push(ep);

    let mut ep = Endpoint::from((Protocol::Udp, SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 4433))));
    ep.set_area(EndpointArea::Lan);
    local_eps.push(ep);
    init_p2p(local_eps.as_slice(), None, std::time::Duration::from_secs(300)).await.unwrap();

    let stack1 = create_stack(Path::new("./"), local_eps.clone(), vec![sn_desc.clone()]).await.unwrap();
    stack1.wait_online(None).await.unwrap();
    let stack2 = create_stack(Path::new("./"), local_eps.clone(), vec![sn_desc.clone()]).await.unwrap();
    stack2.wait_online(None).await.unwrap();

    let generator = IncreaseIdGenerator::new();
    tokio::task::spawn(async move {
        let mut tunnel = stack1.tunnel_manager().create_stream_tunnel(stack2.local_device().device(), generator.generate(), 0).await.unwrap();
        let mut buf = [0u8; 32];
        tunnel.read(buf.as_mut_slice()).await.unwrap();
        tokio::time::sleep(Duration::from_secs(30)).await;
    });

}
async fn create_stack(config_path: &Path, eps: Vec<Endpoint>, sn_list: Vec<Device>) -> BdtResult<P2pStackRef> {
    let (private_key, device) = if config_path.join("device.desc").exists() && config_path.join("device.sec").exists() {
        let (device_desc, _) = Device::decode_from_file(config_path.join("device.desc").as_path(), &mut Vec::new()).map_err(|e| BdtError::from((BdtErrorCode::Failed, "", e)))?;
        let (private_key, _) = PrivateKey::decode_from_file(config_path.join("device.sec").as_path(), &mut Vec::new()).map_err(|e| BdtError::from((BdtErrorCode::Failed, "", e)))?;
        (private_key, device_desc)
    } else {
        let private_key = PrivateKey::generate_rsa(1024).map_err(|e| BdtError::from((BdtErrorCode::Failed, "", e)))?;
        let public_key = private_key.public();
        let device = Device::new(None, UniqueId::default(), eps, vec![], vec![], public_key, Area::default(), DeviceCategory::OOD).build();
        (private_key, device)
    };

    create_p2p_stack(device, private_key, sn_list).await
}

