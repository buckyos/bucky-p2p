use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};
use std::path::Path;
use std::thread;
use bucky_crypto::PrivateKey;
use bucky_objects::{Area, Device, DeviceCategory, DeviceId, Endpoint, NamedObject, ObjectDesc, Protocol, UniqueId};
use bucky_raw_codec::{FileDecoder, FileEncoder, RawDecode, RawFrom};
use flexi_logger::{Cleanup, Criterion, DeferredNow, Duplicate, FileSpec, Naming};
use log::Record;

use cyfs_p2p::{LocalDevice, sn::service::*};
use cyfs_p2p::protocol::{ReceiptWithSignature, SnServiceReceipt};

#[warn(unused_imports)]
pub(crate) use sfo_result::err as miner_err;
pub(crate) use sfo_result::into_err as into_miner_err;
use cyfs_p2p::error::{BdtError, BdtErrorCode};
use cyfs_p2p::executor::Executor;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum MinerErrorCode {
    #[default]
    Failed
}
pub type MinerResult<T> = sfo_result::Result<T, MinerErrorCode>;
pub type MinerError = sfo_result::Error<MinerErrorCode>;

const APP_NAME: &str = "sn-miner";

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

#[tokio::main]
async fn main() {
    flexi_logger::Logger::try_with_str("debug")
        .unwrap()
        .log_to_file(FileSpec::default().directory(std::env::current_dir().unwrap().join("logs")))
        .duplicate_to_stderr(Duplicate::All)
        .rotate(Criterion::Size(10 * 1024 * 1024), // 文件大小达到 10MB 时轮转
                Naming::Numbers, // 使用数字命名轮转文件
                Cleanup::KeepLogFiles(7), // 保留最近 7 个日志文件
        ).format(custom_format)
        .start().unwrap();

    Executor::init(None);
    let data_folder = std::env::current_dir().unwrap().join(APP_NAME);
    let default_desc_path = data_folder.join(APP_NAME);
    let matches = clap::App::new(APP_NAME)
        .arg(clap::Arg::with_name("desc").short("d").long("desc").takes_value(true)
            .default_value(default_desc_path.to_str().unwrap())
            .help("sn desc/sec files, exclude extension")).get_matches();

    match load_device_info(Path::new(matches.value_of("desc").unwrap())) {
        Ok((device, private_key)) => {
            let unique_id = String::from_utf8_lossy(device.desc().unique_id().as_slice());
            // cyfs_debug::CyfsLoggerBuilder::new_app(APP_NAME)
            //     .level("info")
            //     .console("warn")
            //     .build()
            //     .unwrap()
            //     .start();
            //
            // cyfs_debug::PanicBuilder::new(APP_NAME, unique_id.as_ref())
            //     .exit_on_panic(true)
            //     .build()
            //     .start();

            log::info!("sn-miner load device from {}, id {}", matches.value_of("desc").unwrap(), device.desc().object_id());

            let service = SnService::new(
                LocalDevice::new(device, private_key),
                Box::new(SnServiceContractServerImpl::new()),
            ).await;

            let _ = service.start().await;

            std::future::pending::<u8>().await;
        }
        Err(e) => {
            println!("ERROR: read desc/sec file err {}, path {}", e, matches.value_of("desc").unwrap());
            std::process::exit(1);
        }
    }

    println!("exit.");
}

fn load_device_info(folder_path: &Path) -> MinerResult<(Device, PrivateKey)> {
    if !folder_path.with_extension("desc").exists() {
        let private_key = PrivateKey::generate_rsa(1024).map_err(into_miner_err!(MinerErrorCode::Failed))?;
        let public_key = private_key.public();
        let device = Device::new(None, UniqueId::default(), vec![], vec![], vec![], public_key, Area::default(), DeviceCategory::Server).build();
        device.encode_to_file(folder_path.with_extension("desc").as_path(), true).map_err(into_miner_err!(MinerErrorCode::Failed))?;
        private_key.encode_to_file(folder_path.with_extension("sec").as_path(), true).map_err(into_miner_err!(MinerErrorCode::Failed))?;
    }
    let (mut device, _) = Device::decode_from_file(folder_path.with_extension("desc").as_path(), &mut vec![]).map_err(into_miner_err!(MinerErrorCode::Failed))?;
    let (private_key, _) = PrivateKey::decode_from_file(folder_path.with_extension("sec").as_path(), &mut vec![]).map_err(into_miner_err!(MinerErrorCode::Failed))?;

    let eps = device.mut_connect_info().mut_endpoints();
    if eps.len() == 0 {
        eps.push(Endpoint::from((Protocol::Udp, SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 3456)))));
        eps.push(Endpoint::from((Protocol::Udp, SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, 3456, 0, 0)))));
    } else {
        for endpoint in eps {
            match endpoint.mut_addr() {
                SocketAddr::V4(ref mut addr) => {
                    addr.set_ip(Ipv4Addr::UNSPECIFIED)
                }
                SocketAddr::V6(ref mut addr) => {
                    addr.set_ip(Ipv6Addr::UNSPECIFIED)
                }
            }
        }
    }


    Ok((device, private_key))
}
