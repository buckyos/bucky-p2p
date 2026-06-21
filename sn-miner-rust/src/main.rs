use bucky_crypto::PrivateKey;
use bucky_objects::{
    Area, Device, DeviceCategory, Endpoint, NamedObject, ObjectDesc, Protocol, UniqueId,
};
use bucky_raw_codec::{FileDecoder, FileEncoder};
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;

use cyfs_p2p::{CyfsIdentity, CyfsIdentityCertFactory, CyfsIdentityFactory};

use cyfs_p2p::p2p_identity::P2pId;
use cyfs_p2p::pn::PnServer;
use cyfs_p2p::sn::directory::{OwnerDirectoryServer, OwnerMember, OwnerMembership};
use cyfs_p2p::sn::service::{create_sn_service, SnServiceConfig};
pub(crate) use sfo_result::into_err as into_miner_err;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum MinerErrorCode {
    #[default]
    Failed,
}
pub type MinerResult<T> = sfo_result::Result<T, MinerErrorCode>;
pub type MinerError = sfo_result::Error<MinerErrorCode>;

const APP_NAME: &str = "sn-miner";

#[tokio::main]
async fn main() {
    sfo_log::Logger::new("sn-miner").start().unwrap();

    let default_desc_path = std::env::current_dir().unwrap().join("sn");
    let matches = clap::App::new(APP_NAME)
        .arg(
            clap::Arg::with_name("desc")
                .short("d")
                .long("desc")
                .takes_value(true)
                .default_value(default_desc_path.to_str().unwrap())
                .help("sn desc/sec files, exclude extension"),
        )
        .arg(
            clap::Arg::with_name("owner-members")
                .long("owner-members")
                .takes_value(true)
                .help("comma-separated owner SN members: peer_id or peer_id@endpoint"),
        )
        .arg(
            clap::Arg::with_name("owner-peer-endpoints")
                .long("owner-peer-endpoints")
                .takes_value(true)
                .help("comma-separated owner directory peer/control listen endpoints"),
        )
        .arg(
            clap::Arg::with_name("owner-serving-endpoints")
                .long("owner-serving-endpoints")
                .takes_value(true)
                .help("comma-separated owner directory serving-facing listen endpoints"),
        )
        .get_matches();

    match load_device_info(Path::new(matches.value_of("desc").unwrap())) {
        Ok((device, private_key)) => {
            // let unique_id = String::from_utf8_lossy(device.desc().unique_id().as_slice());
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

            log::info!(
                "sn-miner load device from {}, id {}",
                matches.value_of("desc").unwrap(),
                device.desc().object_id()
            );

            let local_identity = Arc::new(CyfsIdentity::new(device, private_key));
            let identity_factory = Arc::new(CyfsIdentityFactory);
            let cert_factory = Arc::new(CyfsIdentityCertFactory);
            let mut config = SnServiceConfig::new(
                local_identity.clone(),
                identity_factory.clone(),
                cert_factory.clone(),
            );
            let mut owner_membership = None;
            if let Some(owner_members) = matches.value_of("owner-members") {
                match parse_owner_membership(owner_members) {
                    Ok(membership) => {
                        config = config.set_owner_client_membership(membership.clone());
                        owner_membership = Some(membership);
                    }
                    Err(err) => {
                        println!("ERROR: invalid --owner-members: {}", err);
                        std::process::exit(1);
                    }
                }
            }
            let _owner_directory_server = match (
                matches.value_of("owner-peer-endpoints"),
                matches.value_of("owner-serving-endpoints"),
            ) {
                (Some(owner_peer_endpoints), Some(owner_serving_endpoints)) => {
                    let Some(membership) = owner_membership.clone() else {
                        println!(
                            "ERROR: --owner-members is required when owner directory endpoints are configured"
                        );
                        std::process::exit(1);
                    };
                    let owner_peer_endpoints = match parse_endpoints(owner_peer_endpoints) {
                        Ok(endpoints) => endpoints,
                        Err(err) => {
                            println!("ERROR: invalid --owner-peer-endpoints: {}", err);
                            std::process::exit(1);
                        }
                    };
                    let owner_serving_endpoints = match parse_endpoints(owner_serving_endpoints) {
                        Ok(endpoints) => endpoints,
                        Err(err) => {
                            println!("ERROR: invalid --owner-serving-endpoints: {}", err);
                            std::process::exit(1);
                        }
                    };
                    match OwnerDirectoryServer::new_with_default_runtime(
                        local_identity.clone(),
                        identity_factory.clone(),
                        cert_factory.clone(),
                        owner_peer_endpoints,
                        owner_serving_endpoints,
                        membership,
                        None,
                    ) {
                        Ok(server) => {
                            if let Err(err) = server.start().await {
                                println!("ERROR: start owner directory server failed: {:?}", err);
                                std::process::exit(1);
                            }
                            Some(server)
                        }
                        Err(err) => {
                            println!("ERROR: create owner directory server failed: {:?}", err);
                            std::process::exit(1);
                        }
                    }
                }
                (None, None) => None,
                _ => {
                    println!(
                        "ERROR: --owner-peer-endpoints and --owner-serving-endpoints must be configured together"
                    );
                    std::process::exit(1);
                }
            };
            let service = create_sn_service(config).await;
            let _pn_server = PnServer::new(service.ttp_server());
            _pn_server.start().await.unwrap();
            service.start().await.unwrap();
            std::future::pending::<u8>().await;
        }
        Err(e) => {
            println!(
                "ERROR: read desc/sec file err {}, path {}",
                e,
                matches.value_of("desc").unwrap()
            );
            std::process::exit(1);
        }
    }

    println!("exit.");
}

fn parse_owner_membership(value: &str) -> std::result::Result<OwnerMembership, String> {
    let mut members = Vec::new();
    for item in value.split(',') {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        if let Some((id, endpoint)) = item.split_once('@') {
            let id = P2pId::from_str(id.trim()).map_err(|err| format!("{:?}", err))?;
            let endpoint = cyfs_p2p::endpoint::Endpoint::from_str(endpoint.trim())
                .map_err(|err| format!("{:?}", err))?;
            members.push(OwnerMember::with_endpoint(id, endpoint));
        } else {
            members.push(OwnerMember::new(
                P2pId::from_str(item).map_err(|err| format!("{:?}", err))?,
            ));
        }
    }
    OwnerMembership::with_members(members, 1, std::time::Duration::from_secs(300))
        .map_err(|err| format!("{:?}", err))
}

fn parse_endpoints(value: &str) -> std::result::Result<Vec<cyfs_p2p::endpoint::Endpoint>, String> {
    let mut endpoints = Vec::new();
    for item in value.split(',') {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        endpoints.push(
            cyfs_p2p::endpoint::Endpoint::from_str(item).map_err(|err| format!("{:?}", err))?,
        );
    }
    if endpoints.is_empty() {
        return Err("at least one endpoint is required".to_owned());
    }
    Ok(endpoints)
}

fn load_device_info(folder_path: &Path) -> MinerResult<(Device, PrivateKey)> {
    if !folder_path.with_extension("desc").exists() {
        let private_key =
            PrivateKey::generate_rsa(1024).map_err(into_miner_err!(MinerErrorCode::Failed))?;
        let public_key = private_key.public();
        let device = Device::new(
            None,
            UniqueId::default(),
            vec![],
            vec![],
            vec![],
            public_key,
            Area::default(),
            DeviceCategory::Server,
        )
        .build();
        device
            .encode_to_file(folder_path.with_extension("desc").as_path(), true)
            .map_err(into_miner_err!(MinerErrorCode::Failed))?;
        private_key
            .encode_to_file(folder_path.with_extension("sec").as_path(), true)
            .map_err(into_miner_err!(MinerErrorCode::Failed))?;
    }
    let (mut device, _) =
        Device::decode_from_file(folder_path.with_extension("desc").as_path(), &mut vec![])
            .map_err(into_miner_err!(MinerErrorCode::Failed))?;
    let (private_key, _) =
        PrivateKey::decode_from_file(folder_path.with_extension("sec").as_path(), &mut vec![])
            .map_err(into_miner_err!(MinerErrorCode::Failed))?;

    let eps = device.mut_connect_info().mut_endpoints();
    if eps.len() == 0 {
        eps.push(Endpoint::from((
            Protocol::Udp,
            SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 3456)),
        )));
        eps.push(Endpoint::from((
            Protocol::Udp,
            SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, 3456, 0, 0)),
        )));
    } else {
        for endpoint in eps {
            match endpoint.mut_addr() {
                SocketAddr::V4(ref mut addr) => addr.set_ip(Ipv4Addr::UNSPECIFIED),
                SocketAddr::V6(ref mut addr) => addr.set_ip(Ipv6Addr::UNSPECIFIED),
            }
        }
    }

    Ok((device, private_key))
}
