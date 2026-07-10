use bucky_crypto::PrivateKey;
use bucky_objects::{
    Area, Device, DeviceCategory, Endpoint, NamedObject, ObjectDesc, Protocol, UniqueId,
};
use bucky_raw_codec::{FileDecoder, FileEncoder};
use std::collections::HashMap;
use std::io::Write as _;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use cyfs_p2p::{CyfsIdentity, CyfsIdentityCertFactory, CyfsIdentityFactory};

use cyfs_p2p::p2p_identity::P2pId;
use cyfs_p2p::pn::PnServer;
use cyfs_p2p::sn::directory::{OwnerDirectoryServer, OwnerMember, OwnerMembership};
use cyfs_p2p::sn::service::{create_sn_service, SnServiceConfig};
pub(crate) use sfo_result::into_err as into_miner_err;
use sfo_reuseport::{ServerRuntime, ServerRuntimeConfig};

#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum MinerErrorCode {
    #[default]
    Failed,
}
pub type MinerResult<T> = sfo_result::Result<T, MinerErrorCode>;
pub type MinerError = sfo_result::Error<MinerErrorCode>;

const APP_NAME: &str = "sn-miner";

fn announce_ready(role: &str) -> std::result::Result<(), String> {
    println!("SN_MINER_READY role={}", role);
    std::io::stdout()
        .flush()
        .map_err(|err| format!("flush {} readiness marker failed: {}", role, err))
}

#[tokio::main]
async fn main() {
    sfo_log::Logger::new("sn-miner").start().unwrap();

    if let Err(err) = run().await {
        println!("ERROR: {}", err);
        std::process::exit(1);
    }

    println!("exit.");
}

async fn run() -> std::result::Result<(), String> {
    let default_desc_path = std::env::current_dir().unwrap().join("sn");
    let matches = clap::App::new(APP_NAME)
        .arg(
            clap::Arg::with_name("config")
                .short("c")
                .long("config")
                .takes_value(true)
                .help("sn-miner config file"),
        )
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

    if let Some(config_path) = matches.value_of("config") {
        let config = SnMinerConfig::load(Path::new(config_path))?;
        return run_configured(config).await;
    }

    run_legacy_serving(
        Path::new(matches.value_of("desc").unwrap()),
        matches.value_of("owner-members"),
        matches.value_of("owner-peer-endpoints"),
        matches.value_of("owner-serving-endpoints"),
    )
    .await
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SnMinerRole {
    Owner,
    Serving,
}

impl SnMinerRole {
    fn parse(value: &str) -> std::result::Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "owner" | "ownersn" | "owner_sn" => Ok(Self::Owner),
            "serving" | "servingsn" | "serving_sn" => Ok(Self::Serving),
            other => Err(format!("unsupported role '{}'", other)),
        }
    }
}

#[derive(Clone, Debug)]
struct SnMinerConfig {
    role: SnMinerRole,
    desc_path: PathBuf,
    owner_members: String,
    owner_peer_endpoints: Option<String>,
    owner_serving_endpoints: String,
    online_heartbeat_interval: Duration,
    route_publish_interval: Duration,
}

impl SnMinerConfig {
    fn load(path: &Path) -> std::result::Result<Self, String> {
        let values = parse_config_file(path)?;
        let role = SnMinerRole::parse(required_value(&values, "role")?)?;
        let desc_path = PathBuf::from(
            values
                .get("desc")
                .cloned()
                .unwrap_or_else(|| "sn".to_owned()),
        );

        let serving_member_alias = values.get("serving_owner_members");
        let serving_endpoint_alias = values.get("serving_owner_serving_endpoints");
        if serving_member_alias.is_some() && values.contains_key("owner_members") {
            return Err("configure only one of owner_members or serving_owner_members".to_owned());
        }
        if serving_endpoint_alias.is_some() && values.contains_key("owner_serving_endpoints") {
            return Err(
                "configure only one of owner_serving_endpoints or serving_owner_serving_endpoints"
                    .to_owned(),
            );
        }

        let owner_members = values
            .get("owner_members")
            .or(serving_member_alias)
            .cloned()
            .ok_or_else(|| "missing owner_members".to_owned())?;
        let owner_serving_endpoints = values
            .get("owner_serving_endpoints")
            .or(serving_endpoint_alias)
            .cloned()
            .ok_or_else(|| "missing owner_serving_endpoints".to_owned())?;
        let owner_peer_endpoints = values.get("owner_peer_endpoints").cloned();

        match role {
            SnMinerRole::Owner => {
                if serving_member_alias.is_some() || serving_endpoint_alias.is_some() {
                    return Err("owner role cannot use serving_* config keys".to_owned());
                }
                if owner_peer_endpoints.is_none() {
                    return Err("owner role requires owner_peer_endpoints".to_owned());
                }
            }
            SnMinerRole::Serving => {
                if owner_peer_endpoints.is_some() {
                    return Err("serving role cannot configure owner_peer_endpoints".to_owned());
                }
            }
        }

        Ok(Self {
            role,
            desc_path,
            owner_members,
            owner_peer_endpoints,
            owner_serving_endpoints,
            online_heartbeat_interval: parse_duration_secs(
                values.get("online_heartbeat_interval_secs"),
                30,
                "online_heartbeat_interval_secs",
            )?,
            route_publish_interval: parse_duration_secs(
                values.get("route_publish_interval_secs"),
                300,
                "route_publish_interval_secs",
            )?,
        })
    }
}

async fn run_configured(config: SnMinerConfig) -> std::result::Result<(), String> {
    match config.role {
        SnMinerRole::Owner => run_owner_role(config).await,
        SnMinerRole::Serving => run_serving_role(config).await,
    }
}

async fn run_owner_role(config: SnMinerConfig) -> std::result::Result<(), String> {
    let (local_identity, identity_factory, cert_factory) = load_identity(&config.desc_path)?;
    let membership = parse_owner_membership(&config.owner_members)
        .map_err(|err| format!("invalid owner_members: {}", err))?;
    let owner_peer_endpoints = parse_endpoints(
        config
            .owner_peer_endpoints
            .as_deref()
            .ok_or_else(|| "owner role requires owner_peer_endpoints".to_owned())?,
    )
    .map_err(|err| format!("invalid owner_peer_endpoints: {}", err))?;
    let owner_serving_endpoints = parse_endpoints(&config.owner_serving_endpoints)
        .map_err(|err| format!("invalid owner_serving_endpoints: {}", err))?;
    let server = OwnerDirectoryServer::new_with_default_runtime(
        local_identity,
        identity_factory,
        cert_factory,
        owner_peer_endpoints,
        owner_serving_endpoints,
        membership,
        None,
    )
    .map_err(|err| format!("create owner directory server failed: {:?}", err))?;
    server
        .start()
        .await
        .map_err(|err| format!("start owner directory server failed: {:?}", err))?;
    announce_ready("owner")?;
    std::future::pending::<()>().await;
    Ok(())
}

async fn run_serving_role(config: SnMinerConfig) -> std::result::Result<(), String> {
    let (local_identity, identity_factory, cert_factory) = load_identity(&config.desc_path)?;
    let membership = parse_owner_membership_with_endpoints(
        &config.owner_members,
        Some(config.owner_serving_endpoints.as_str()),
        true,
    )
    .map_err(|err| format!("invalid serving owner membership: {}", err))?;
    log::info!(
        "serving role configured online_heartbeat_interval={:?} route_publish_interval={:?}",
        config.online_heartbeat_interval,
        config.route_publish_interval
    );
    let server_runtime =
        ServerRuntime::start(ServerRuntimeConfig::default()).map_err(|err| format!("{:?}", err))?;
    let service_config = SnServiceConfig::new(
        local_identity,
        identity_factory,
        cert_factory,
        server_runtime,
    )
    .set_owner_client_membership(membership);
    start_serving_service(service_config).await
}

async fn run_legacy_serving(
    desc_path: &Path,
    owner_members: Option<&str>,
    owner_peer_endpoints: Option<&str>,
    owner_serving_endpoints: Option<&str>,
) -> std::result::Result<(), String> {
    if owner_peer_endpoints.is_some() || owner_serving_endpoints.is_some() {
        return Err(
            "legacy owner directory flags are not role-safe; use --config with role=owner or role=serving"
                .to_owned(),
        );
    }
    let (local_identity, identity_factory, cert_factory) = load_identity(desc_path)?;
    let server_runtime =
        ServerRuntime::start(ServerRuntimeConfig::default()).map_err(|err| format!("{:?}", err))?;
    let mut service_config = SnServiceConfig::new(
        local_identity,
        identity_factory,
        cert_factory,
        server_runtime,
    );
    if let Some(owner_members) = owner_members {
        let membership = parse_owner_membership(owner_members)
            .map_err(|err| format!("invalid --owner-members: {}", err))?;
        service_config = service_config.set_owner_client_membership(membership);
    }
    start_serving_service(service_config).await
}

async fn start_serving_service(config: SnServiceConfig) -> std::result::Result<(), String> {
    let service = create_sn_service(config)
        .await
        .map_err(|err| format!("{:?}", err))?;
    let pn_server = PnServer::new(service.ttp_server());
    pn_server
        .start()
        .await
        .map_err(|err| format!("{:?}", err))?;
    service.start().await.map_err(|err| format!("{:?}", err))?;
    announce_ready("serving")?;
    std::future::pending::<()>().await;
    Ok(())
}

fn load_identity(
    desc_path: &Path,
) -> std::result::Result<
    (
        Arc<CyfsIdentity>,
        Arc<CyfsIdentityFactory>,
        Arc<CyfsIdentityCertFactory>,
    ),
    String,
> {
    let (device, private_key) = load_device_info(desc_path).map_err(|err| {
        format!(
            "read desc/sec file err {}, path {}",
            err,
            desc_path.display()
        )
    })?;
    log::info!(
        "sn-miner load device from {}, id {}",
        desc_path.display(),
        device.desc().object_id()
    );
    Ok((
        Arc::new(CyfsIdentity::new(device, private_key)),
        Arc::new(CyfsIdentityFactory),
        Arc::new(CyfsIdentityCertFactory),
    ))
}

fn parse_config_file(path: &Path) -> std::result::Result<HashMap<String, String>, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|err| format!("read config {} failed: {}", path.display(), err))?;
    let mut values = HashMap::new();
    for (line_no, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| format!("invalid config line {}: expected key=value", line_no + 1))?;
        let key = key.trim();
        if key.is_empty() {
            return Err(format!("invalid config line {}: empty key", line_no + 1));
        }
        values.insert(key.to_owned(), strip_quotes(value.trim()).to_owned());
    }
    Ok(values)
}

fn strip_quotes(value: &str) -> &str {
    if value.len() >= 2 {
        let bytes = value.as_bytes();
        if (bytes[0] == b'"' && bytes[value.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[value.len() - 1] == b'\'')
        {
            return &value[1..value.len() - 1];
        }
    }
    value
}

fn required_value<'a>(
    values: &'a HashMap<String, String>,
    key: &str,
) -> std::result::Result<&'a str, String> {
    values
        .get(key)
        .map(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("missing {}", key))
}

fn parse_duration_secs(
    value: Option<&String>,
    default_secs: u64,
    key: &str,
) -> std::result::Result<Duration, String> {
    let secs = value
        .map(|value| value.parse::<u64>())
        .transpose()
        .map_err(|err| format!("invalid {}: {}", key, err))?
        .unwrap_or(default_secs);
    if secs == 0 {
        return Err(format!("{} must be greater than zero", key));
    }
    Ok(Duration::from_secs(secs))
}

fn parse_owner_membership(value: &str) -> std::result::Result<OwnerMembership, String> {
    parse_owner_membership_with_endpoints(value, None, false)
}

fn parse_owner_membership_with_endpoints(
    value: &str,
    endpoints: Option<&str>,
    require_endpoints: bool,
) -> std::result::Result<OwnerMembership, String> {
    let endpoint_values = endpoints
        .map(parse_endpoints)
        .transpose()?
        .unwrap_or_default();
    let mut members = Vec::new();
    for (index, item) in value.split(',').enumerate() {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        if let Some((id, endpoint)) = item.split_once('@') {
            let id = P2pId::from_str(id.trim()).map_err(|err| format!("{:?}", err))?;
            let endpoint = cyfs_p2p::endpoint::Endpoint::from_str(endpoint.trim())
                .map_err(|err| format!("{:?}", err))?;
            members.push(OwnerMember::with_endpoint(id, endpoint));
        } else if let Some(endpoint) = endpoint_values.get(index).copied() {
            members.push(OwnerMember::with_endpoint(
                P2pId::from_str(item).map_err(|err| format!("{:?}", err))?,
                endpoint,
            ));
        } else {
            members.push(OwnerMember::new(
                P2pId::from_str(item).map_err(|err| format!("{:?}", err))?,
            ));
        }
    }
    if require_endpoints && members.iter().any(|member| member.endpoints.is_empty()) {
        return Err("serving role requires endpoint for every owner member".to_owned());
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
