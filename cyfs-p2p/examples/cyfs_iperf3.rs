use bucky_crypto::PrivateKey;
use bucky_objects::{
    sign_and_push_named_object, Area, Device, DeviceCategory, Endpoint as CyfsEndpoint,
    EndpointArea, Protocol as CyfsProtocol, RsaCPUObjectSigner, SignatureSource, UniqueId,
    SIGNATURE_SOURCE_REFINDEX_SELF,
};
use clap::{App, Arg, SubCommand};
use cyfs_p2p::stack::{create_p2p_env, create_p2p_stack, P2pStackRef};
use cyfs_p2p::{create_cyfs_p2p_config, create_cyfs_p2p_stack_config, cyfs_to_p2p_endpoint};
use p2p_frame::endpoint::{Endpoint as P2pEndpoint, Protocol as P2pProtocol};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const PERF_VPORT: u16 = 5201;
const DEFAULT_SERVER_BIND: &str = "0.0.0.0:4433";
const DEFAULT_CLIENT_BIND: &str = "0.0.0.0:0";

type AppResult<T> = Result<T, String>;

#[derive(Clone)]
struct ServerOpts {
    bind: SocketAddr,
    interval_secs: u64,
}

#[derive(Clone)]
struct ClientOpts {
    bind: SocketAddr,
    server: SocketAddr,
    time_secs: u64,
    len: usize,
    parallel: usize,
    interval_secs: u64,
}

enum Command {
    Server(ServerOpts),
    Client(ClientOpts),
}

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

async fn run() -> AppResult<()> {
    match parse_command()? {
        Command::Server(opts) => run_server(opts).await,
        Command::Client(opts) => run_client(opts).await,
    }
}

fn parse_command() -> AppResult<Command> {
    let app = App::new("cyfs_iperf3")
        .about("QUIC direct mode throughput tool for cyfs-p2p")
        .subcommand(
            SubCommand::with_name("server")
                .about("Run throughput server")
                .arg(
                    Arg::with_name("bind")
                        .long("bind")
                        .takes_value(true)
                        .default_value(DEFAULT_SERVER_BIND)
                        .help("QUIC listen address"),
                )
                .arg(
                    Arg::with_name("interval")
                        .long("interval")
                        .takes_value(true)
                        .default_value("1")
                        .help("report interval in seconds"),
                ),
        )
        .subcommand(
            SubCommand::with_name("client")
                .about("Run throughput client")
                .arg(
                    Arg::with_name("server")
                        .long("server")
                        .takes_value(true)
                        .required(true)
                        .help("server address ip:port"),
                )
                .arg(
                    Arg::with_name("bind")
                        .long("bind")
                        .takes_value(true)
                        .default_value(DEFAULT_CLIENT_BIND)
                        .help("local bind address"),
                )
                .arg(
                    Arg::with_name("time")
                        .long("time")
                        .takes_value(true)
                        .default_value("10")
                        .help("test duration in seconds"),
                )
                .arg(
                    Arg::with_name("len")
                        .long("len")
                        .takes_value(true)
                        .default_value("16384")
                        .help("write size per send in bytes"),
                )
                .arg(
                    Arg::with_name("parallel")
                        .long("parallel")
                        .takes_value(true)
                        .default_value("1")
                        .help("parallel stream count"),
                )
                .arg(
                    Arg::with_name("interval")
                        .long("interval")
                        .takes_value(true)
                        .default_value("1")
                        .help("report interval in seconds"),
                ),
        );

    let matches = app.get_matches();
    match matches.subcommand() {
        ("server", Some(matches)) => parse_server_args(matches).map(Command::Server),
        ("client", Some(matches)) => parse_client_args(matches).map(Command::Client),
        _ => Err(usage()),
    }
}

fn parse_server_args(matches: &clap::ArgMatches<'_>) -> AppResult<ServerOpts> {
    let bind = parse_socket(value_of(matches, "bind")?)?;
    let interval_secs = parse_u64("--interval", value_of(matches, "interval")?)?;

    if interval_secs == 0 {
        return Err("--interval must be > 0".to_string());
    }

    Ok(ServerOpts {
        bind,
        interval_secs,
    })
}

fn parse_client_args(matches: &clap::ArgMatches<'_>) -> AppResult<ClientOpts> {
    let bind = parse_socket(value_of(matches, "bind")?)?;
    let server = parse_socket(value_of(matches, "server")?)?;
    let time_secs = parse_u64("--time", value_of(matches, "time")?)?;
    let len = parse_usize("--len", value_of(matches, "len")?)?;
    let parallel = parse_usize("--parallel", value_of(matches, "parallel")?)?;
    let interval_secs = parse_u64("--interval", value_of(matches, "interval")?)?;

    if time_secs == 0 {
        return Err("--time must be > 0".to_string());
    }
    if len == 0 {
        return Err("--len must be > 0".to_string());
    }
    if parallel == 0 {
        return Err("--parallel must be > 0".to_string());
    }
    if interval_secs == 0 {
        return Err("--interval must be > 0".to_string());
    }

    Ok(ClientOpts {
        bind,
        server,
        time_secs,
        len,
        parallel,
        interval_secs,
    })
}

fn value_of<'a>(matches: &'a clap::ArgMatches<'_>, name: &str) -> AppResult<&'a str> {
    matches
        .value_of(name)
        .ok_or_else(|| format!("missing value for {name}"))
}

fn parse_socket(value: &str) -> AppResult<SocketAddr> {
    value
        .parse::<SocketAddr>()
        .map_err(|e| format!("invalid socket address {value}: {e}"))
}

fn parse_u64(name: &str, value: &str) -> AppResult<u64> {
    value
        .parse::<u64>()
        .map_err(|e| format!("invalid value for {name}: {value}, err: {e}"))
}

fn parse_usize(name: &str, value: &str) -> AppResult<usize> {
    value
        .parse::<usize>()
        .map_err(|e| format!("invalid value for {name}: {value}, err: {e}"))
}

fn usage() -> String {
    [
        "cyfs_iperf3 (QUIC direct mode, no SN)",
        "",
        "Usage:",
        "  cyfs_iperf3 server [--bind <ip:port>] [--interval <seconds>]",
        "  cyfs_iperf3 client --server <ip:port> [--bind <ip:port>] [--time <seconds>] [--len <bytes>] [--parallel <n>] [--interval <seconds>]",
        "",
        "Examples:",
        "  cyfs_iperf3 server --bind 0.0.0.0:4433",
        "  cyfs_iperf3 client --server 127.0.0.1:4433 --time 10 --parallel 4",
    ]
    .join("\n")
}

fn format_mbps(bytes: u64, secs: f64) -> f64 {
    if secs <= 0.0 {
        return 0.0;
    }
    (bytes as f64) * 8.0 / secs / 1_000_000.0
}

fn new_quic_endpoint(addr: SocketAddr) -> CyfsEndpoint {
    let mut ep = CyfsEndpoint::from((CyfsProtocol::Udp, addr));
    ep.set_area(EndpointArea::Lan);
    ep
}

async fn create_stack(bind_addr: SocketAddr) -> AppResult<P2pStackRef> {
    let local_ep = new_quic_endpoint(bind_addr);
    let config = create_cyfs_p2p_config(vec![cyfs_to_p2p_endpoint(&local_ep)]);
    let env = create_p2p_env(config).await.map_err(|e| {
        format!(
            "create p2p env failed, code={:?}, msg={}",
            e.code(),
            e.msg()
        )
    })?;

    let (device, key) = generate_runtime_identity(vec![local_ep]).await?;
    let stack_config = create_cyfs_p2p_stack_config(env, device, key, vec![]);
    create_p2p_stack(stack_config).await.map_err(|e| {
        format!(
            "create p2p stack failed, code={:?}, msg={}",
            e.code(),
            e.msg()
        )
    })
}

async fn generate_runtime_identity(eps: Vec<CyfsEndpoint>) -> AppResult<(Device, PrivateKey)> {
    let private_key = PrivateKey::generate_rsa(1024)
        .map_err(|e| format!("generate rsa key failed: {e}"))?;
    let public_key = private_key.public();
    let mut device = Device::new(
        None,
        UniqueId::default(),
        eps,
        vec![],
        vec![],
        public_key.clone(),
        Area::default(),
        DeviceCategory::OOD,
    )
    .build();

    let signer = RsaCPUObjectSigner::new(public_key, private_key.clone());
    sign_and_push_named_object(
        &signer,
        &mut device,
        &SignatureSource::RefIndex(SIGNATURE_SOURCE_REFINDEX_SELF),
    )
    .await
    .map_err(|e| format!("sign runtime device failed: {e}"))?;

    Ok((device, private_key))
}

async fn run_server(opts: ServerOpts) -> AppResult<()> {
    let stack = create_stack(opts.bind).await?;
    let listener = stack
        .stream_manager()
        .listen(PERF_VPORT)
        .await
        .map_err(|e| {
            format!(
                "listen failed, code={:?}, msg={}",
                e.code(),
                e.msg()
            )
        })?;

    let interval = Duration::from_secs(opts.interval_secs);
    let next_conn_id = Arc::new(AtomicU64::new(1));

    loop {
        let (mut read, _write) = listener.accept().await.map_err(|e| {
            format!(
                "accept failed, code={:?}, msg={}",
                e.code(),
                e.msg()
            )
        })?;
        let conn_id = next_conn_id.fetch_add(1, Ordering::Relaxed);
        tokio::task::spawn(async move {
            let mut transfer_started: Option<Instant> = None;
            let mut interval_started: Option<Instant> = None;
            let mut bytes = 0u64;
            let mut interval_bytes = 0u64;
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                match read.read(buf.as_mut_slice()).await {
                    Ok(0) => {
                        break;
                    }
                    Ok(n) => {
                        let n = n as u64;
                        let now = Instant::now();
                        if transfer_started.is_none() {
                            transfer_started = Some(now);
                            interval_started = Some(now);
                        }
                        bytes += n;
                        interval_bytes += n;
                        if let Some(begin) = interval_started {
                            let elapsed = now.duration_since(begin);
                            if elapsed >= interval {
                                if interval_bytes > 0 {
                                    let mbps = format_mbps(interval_bytes, elapsed.as_secs_f64());
                                    println!(
                                        "[server][conn {conn_id}] interval={:.2}s bytes={} rate={:.3} Mbps total={}",
                                        elapsed.as_secs_f64(),
                                        interval_bytes,
                                        mbps,
                                        bytes
                                    );
                                }
                                interval_started = Some(now);
                                interval_bytes = 0;
                            }
                        }
                    }
                    Err(e) => {
                        if transfer_started.is_some() {
                            eprintln!("server connection read error, conn {conn_id}: {e}");
                        }
                        break;
                    }
                }
            }

            if let Some(started) = transfer_started {
                if let Some(begin) = interval_started {
                    let elapsed = Instant::now().duration_since(begin);
                    if interval_bytes > 0 && elapsed.as_secs_f64() > 0.0 {
                        let mbps = format_mbps(interval_bytes, elapsed.as_secs_f64());
                        println!(
                            "[server][conn {conn_id}] interval={:.2}s bytes={} rate={:.3} Mbps total={}",
                            elapsed.as_secs_f64(),
                            interval_bytes,
                            mbps,
                            bytes
                        );
                    }
                }
                let secs = started.elapsed().as_secs_f64();
                let mbps = format_mbps(bytes, secs);
                println!(
                    "[server][conn {conn_id}] done: bytes={} elapsed={:.2}s avg={:.3} Mbps",
                    bytes, secs, mbps
                );
            }
        });
    }

}

async fn run_client(opts: ClientOpts) -> AppResult<()> {
    let stack = create_stack(opts.bind).await?;
    let remote = P2pEndpoint::from((P2pProtocol::Quic, opts.server));
    let deadline = Instant::now() + Duration::from_secs(opts.time_secs);

    println!(
        "client started: bind={} server={} protocol=quic streams={} duration={}s len={}B internal_vport={}",
        opts.bind, opts.server, opts.parallel, opts.time_secs, opts.len, PERF_VPORT
    );

    let bytes_total = Arc::new(AtomicU64::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let report_handle = tokio::task::spawn(start_reporter(
        "client",
        bytes_total.clone(),
        Duration::from_secs(opts.interval_secs),
        stop.clone(),
    ));

    let started = Instant::now();
    let mut handles = Vec::with_capacity(opts.parallel);
    for _ in 0..opts.parallel {
        let stack = stack.clone();
        let remote = remote.clone();
        let bytes_total = bytes_total.clone();
        let deadline = deadline;
        let payload = vec![0u8; opts.len];

        handles.push(tokio::task::spawn(async move {
            let (_read, mut write) = stack
                .stream_manager()
                .connect_direct(vec![remote], PERF_VPORT, None)
                .await
                .map_err(|e| {
                    format!(
                        "connect direct failed, code={:?}, msg={}",
                        e.code(),
                        e.msg()
                    )
                })?;

            while Instant::now() < deadline {
                write
                    .write_all(payload.as_slice())
                    .await
                    .map_err(|e| format!("client write failed: {e}"))?;
                bytes_total.fetch_add(payload.len() as u64, Ordering::Relaxed);
            }

            write
                .flush()
                .await
                .map_err(|e| format!("client flush failed: {e}"))?;
            Ok::<(), String>(())
        }));
    }

    for handle in handles {
        handle
            .await
            .map_err(|e| format!("writer task join failed: {e}"))??;
    }

    stop.store(true, Ordering::Relaxed);
    let _ = report_handle.await;

    let elapsed = started.elapsed().as_secs_f64();
    let total_bytes = bytes_total.load(Ordering::Relaxed);
    let avg_mbps = format_mbps(total_bytes, elapsed);
    println!(
        "client done: bytes={} elapsed={:.2}s avg={:.3} Mbps",
        total_bytes, elapsed, avg_mbps
    );
    Ok(())
}

async fn start_reporter(
    role: &'static str,
    total_bytes: Arc<AtomicU64>,
    interval: Duration,
    stop: Arc<AtomicBool>,
) {
    let started = Instant::now();
    let mut last_bytes = 0u64;
    while !stop.load(Ordering::Relaxed) {
        tokio::time::sleep(interval).await;
        let now_total = total_bytes.load(Ordering::Relaxed);
        let delta = now_total.saturating_sub(last_bytes);
        last_bytes = now_total;
        let secs = interval.as_secs_f64();
        let instant_mbps = format_mbps(delta, secs);
        let elapsed = started.elapsed().as_secs_f64();
        let avg_mbps = format_mbps(now_total, elapsed);
        println!(
            "[{role}] interval={:.2}s bytes={} rate={:.3} Mbps total={} avg={:.3} Mbps",
            secs, delta, instant_mbps, now_total, avg_mbps
        );
    }
}
