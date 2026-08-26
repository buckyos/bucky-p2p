use bucky_crypto::PrivateKey;
use bucky_objects::{
    Area, Device, DeviceCategory, Endpoint as DeviceEndpoint, Protocol as DeviceProtocol, UniqueId,
};
use bucky_raw_codec::FileEncoder;
use cyfs_p2p::endpoint::{Endpoint, Protocol};
use cyfs_p2p::p2p_identity::P2pId;
use std::fs::{self, File};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const READY_TIMEOUT: Duration = Duration::from_secs(15);
const POLL_INTERVAL: Duration = Duration::from_millis(50);

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_sn-miner")
}

fn temp_dir(name: &str) -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path =
        std::env::temp_dir().join(format!("sn-miner-{}-{}-{}", name, std::process::id(), now));
    fs::create_dir_all(&path).unwrap();
    path
}

fn endpoint(port: u16) -> String {
    Endpoint::from((
        Protocol::Tcp,
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)),
    ))
    .to_string()
}

fn p2p_id(seed: u8) -> String {
    P2pId::from(vec![seed; 32]).to_string()
}

fn reserve_tcp_port() -> u16 {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    listener.local_addr().unwrap().port()
}

fn loopback(port: u16) -> SocketAddr {
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
}

fn write_identity(base: &Path, tcp_port: u16) {
    let private_key = PrivateKey::generate_rsa(1024).unwrap();
    let device = Device::new(
        None,
        UniqueId::default(),
        vec![DeviceEndpoint::from((DeviceProtocol::Tcp, loopback(tcp_port)))],
        vec![],
        vec![],
        private_key.public(),
        Area::default(),
        DeviceCategory::Server,
    )
    .build();
    device
        .encode_to_file(base.with_extension("desc").as_path(), true)
        .unwrap();
    private_key
        .encode_to_file(base.with_extension("sec").as_path(), true)
        .unwrap();
}

fn write_config(dir: &Path, name: &str, content: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, content).unwrap();
    path
}

struct ManagedChild {
    child: Option<Child>,
    stdout_path: PathBuf,
    stderr_path: PathBuf,
}

impl ManagedChild {
    fn spawn(config: &Path) -> Self {
        let dir = config.parent().unwrap();
        let stdout_path = dir.join("sn-miner.stdout.log");
        let stderr_path = dir.join("sn-miner.stderr.log");
        let stdout = File::create(&stdout_path).unwrap();
        let stderr = File::create(&stderr_path).unwrap();
        let child = Command::new(bin())
            .arg("--config")
            .arg(config)
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr))
            .spawn()
            .unwrap();
        Self {
            child: Some(child),
            stdout_path,
            stderr_path,
        }
    }

    fn output(&self) -> String {
        let stdout = fs::read_to_string(&self.stdout_path).unwrap_or_default();
        let stderr = fs::read_to_string(&self.stderr_path).unwrap_or_default();
        format!("stdout:\n{}\nstderr:\n{}", stdout, stderr)
    }

    fn try_wait(&mut self) -> Result<Option<ExitStatus>, String> {
        self.child
            .as_mut()
            .ok_or_else(|| "child already reaped".to_owned())?
            .try_wait()
            .map_err(|err| format!("query child status failed: {}", err))
    }

    fn wait_ready(
        &mut self,
        role: &str,
        listen_addrs: &[SocketAddr],
        timeout: Duration,
    ) -> Result<(), String> {
        let marker = format!("SN_MINER_READY role={}", role);
        let deadline = Instant::now() + timeout;
        let mut marker_seen = false;

        loop {
            if let Some(status) = self.try_wait()? {
                return Err(format!(
                    "sn-miner exited before {} readiness with status {}\n{}",
                    role,
                    status,
                    self.output()
                ));
            }

            let output = self.output();
            marker_seen |= output.contains(&marker);
            if marker_seen
                && listen_addrs.iter().all(|addr| {
                    TcpStream::connect_timeout(addr, Duration::from_millis(200)).is_ok()
                })
            {
                return Ok(());
            }

            if Instant::now() >= deadline {
                return Err(format!(
                    "timed out waiting for {} readiness (marker_seen={}, listen_addrs={:?})\n{}",
                    role, marker_seen, listen_addrs, output
                ));
            }
            thread::sleep(POLL_INTERVAL);
        }
    }

    fn stop_and_wait(&mut self) -> Result<ExitStatus, String> {
        let mut child = self
            .child
            .take()
            .ok_or_else(|| "child already reaped".to_owned())?;
        if child
            .try_wait()
            .map_err(|err| format!("query child before stop failed: {}", err))?
            .is_none()
        {
            child
                .kill()
                .map_err(|err| format!("kill child failed: {}", err))?;
        }
        child
            .wait()
            .map_err(|err| format!("wait for child failed: {}", err))
    }
}

impl Drop for ManagedChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn assert_role_ready(config: &Path, role: &str, listen_addrs: &[SocketAddr]) {
    let mut child = ManagedChild::spawn(config);
    if let Err(err) = child.wait_ready(role, listen_addrs, READY_TIMEOUT) {
        let _ = child.stop_and_wait();
        panic!("{}", err);
    }
    child.stop_and_wait().unwrap();
}

#[test]
fn sn_miner_rejects_mixed_owner_and_serving_config_before_ready() {
    let dir = temp_dir("invalid");
    let config = write_config(
        &dir,
        "invalid.conf",
        &format!(
            "\
role=serving
desc={}
owner_members={}
serving_owner_members={}
owner_serving_endpoints={}
",
            dir.join("sn").display(),
            p2p_id(1),
            p2p_id(2),
            endpoint(0)
        ),
    );

    let mut child = ManagedChild::spawn(&config);
    let err = child
        .wait_ready("serving", &[], Duration::from_secs(5))
        .unwrap_err();
    assert!(err.contains("exited before serving readiness"), "{}", err);
    assert!(err.contains("ERROR:"), "{}", err);
    child.stop_and_wait().unwrap();
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn sn_miner_owner_role_reaches_both_listener_surfaces() {
    let dir = temp_dir("owner-ready");
    let owner_peer_port = reserve_tcp_port();
    let owner_serving_port = reserve_tcp_port();
    let config = write_config(
        &dir,
        "owner.conf",
        &format!(
            "\
role=owner
desc={}
owner_members={}
owner_peer_endpoints={}
owner_serving_endpoints={}
",
            dir.join("owner-sn").display(),
            p2p_id(3),
            endpoint(owner_peer_port),
            endpoint(owner_serving_port)
        ),
    );

    assert_role_ready(
        &config,
        "owner",
        &[loopback(owner_peer_port), loopback(owner_serving_port)],
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn sn_miner_serving_role_reaches_peer_listener() {
    let dir = temp_dir("serving-ready");
    let serving_port = reserve_tcp_port();
    let serving_desc = dir.join("serving-sn");
    write_identity(&serving_desc, serving_port);
    let config = write_config(
        &dir,
        "serving.conf",
        &format!(
            "\
role=serving
desc={}
owner_members={}
owner_serving_endpoints={}
online_heartbeat_interval_secs=1
route_publish_interval_secs=2
",
            serving_desc.display(),
            p2p_id(4),
            endpoint(reserve_tcp_port())
        ),
    );

    assert_role_ready(&config, "serving", &[loopback(serving_port)]);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn readiness_wait_rejects_a_running_process_with_the_wrong_role_marker() {
    let dir = temp_dir("marker-mismatch");
    let owner_peer_port = reserve_tcp_port();
    let owner_serving_port = reserve_tcp_port();
    let config = write_config(
        &dir,
        "owner.conf",
        &format!(
            "\
role=owner
desc={}
owner_members={}
owner_peer_endpoints={}
owner_serving_endpoints={}
",
            dir.join("owner-sn").display(),
            p2p_id(5),
            endpoint(owner_peer_port),
            endpoint(owner_serving_port)
        ),
    );

    let mut child = ManagedChild::spawn(&config);
    let err = child
        .wait_ready("serving", &[loopback(owner_peer_port)], Duration::from_secs(2))
        .unwrap_err();
    assert!(err.contains("timed out waiting for serving readiness"), "{}", err);
    assert!(err.contains("SN_MINER_READY role=owner"), "{}", err);
    child.stop_and_wait().unwrap();
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn readiness_wait_rejects_an_unreachable_listener_after_the_ready_marker() {
    let dir = temp_dir("probe-failure");
    let owner_peer_port = reserve_tcp_port();
    let owner_serving_port = reserve_tcp_port();
    let unreachable_port = reserve_tcp_port();
    let config = write_config(
        &dir,
        "owner.conf",
        &format!(
            "\
role=owner
desc={}
owner_members={}
owner_peer_endpoints={}
owner_serving_endpoints={}
",
            dir.join("owner-sn").display(),
            p2p_id(6),
            endpoint(owner_peer_port),
            endpoint(owner_serving_port)
        ),
    );

    let mut child = ManagedChild::spawn(&config);
    let err = child
        .wait_ready("owner", &[loopback(unreachable_port)], Duration::from_secs(2))
        .unwrap_err();
    assert!(err.contains("marker_seen=true"), "{}", err);
    assert!(err.contains("SN_MINER_READY role=owner"), "{}", err);
    child.stop_and_wait().unwrap();
    fs::remove_dir_all(dir).unwrap();
}
