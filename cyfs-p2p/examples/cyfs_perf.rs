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
use p2p_frame::networks::TunnelPurpose;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const PERF_VPORT: u16 = 5201;
const DEFAULT_SERVER_BIND: &str = "0.0.0.0:4433";
const DEFAULT_CLIENT_BIND: &str = "0.0.0.0:0";

fn tunnel_purpose(value: u16) -> TunnelPurpose {
    TunnelPurpose::from_value(&value).unwrap()
}

type AppResult<T> = Result<T, String>;

struct ReportSnapshot {
    start_secs: f64,
    end_secs: f64,
    bytes: u64,
}

#[derive(Clone, Copy)]
enum TransportProtocol {
    Quic,
    Tcp,
}

impl TransportProtocol {
    fn parse(value: &str) -> AppResult<Self> {
        match value.to_ascii_lowercase().as_str() {
            "quic" => Ok(Self::Quic),
            "tcp" => Ok(Self::Tcp),
            _ => Err(format!("invalid protocol: {value}, expected quic or tcp")),
        }
    }

    fn cyfs_protocol(self) -> CyfsProtocol {
        match self {
            Self::Quic => CyfsProtocol::Udp,
            Self::Tcp => CyfsProtocol::Tcp,
        }
    }

    fn p2p_protocol(self) -> P2pProtocol {
        match self {
            Self::Quic => P2pProtocol::Quic,
            Self::Tcp => P2pProtocol::Tcp,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Quic => "QUIC",
            Self::Tcp => "TCP",
        }
    }
}

#[derive(Clone)]
struct ServerOpts {
    bind: SocketAddr,
    interval_secs: u64,
    protocol: TransportProtocol,
}

#[derive(Clone)]
struct ClientOpts {
    bind: SocketAddr,
    server: SocketAddr,
    time_secs: u64,
    len: usize,
    parallel: usize,
    interval_secs: u64,
    protocol: TransportProtocol,
}

enum Command {
    Server(ServerOpts),
    Client(ClientOpts),
}

#[tokio::main]
async fn main() {
    // sfo_log::Logger::new("cyfs_iperf3").set_log_to_file(false).start().unwrap();
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
    let app = App::new("cyfs_perf")
        .about("QUIC direct mode throughput tool for cyfs-p2p")
        .subcommand(
            SubCommand::with_name("server")
                .about("Run throughput server")
                .arg(
                    Arg::with_name("bind")
                        .short("B")
                        .long("bind")
                        .takes_value(true)
                        .default_value(DEFAULT_SERVER_BIND)
                        .help("listen address"),
                )
                .arg(
                    Arg::with_name("protocol")
                        .short("X")
                        .long("protocol")
                        .takes_value(true)
                        .default_value("quic")
                        .possible_values(&["quic", "tcp"])
                        .case_insensitive(true)
                        .help("transport protocol"),
                )
                .arg(
                    Arg::with_name("interval")
                        .short("i")
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
                        .short("c")
                        .long("server")
                        .takes_value(true)
                        .required(true)
                        .help("server address ip:port"),
                )
                .arg(
                    Arg::with_name("bind")
                        .short("B")
                        .long("bind")
                        .takes_value(true)
                        .default_value(DEFAULT_CLIENT_BIND)
                        .help("local bind address"),
                )
                .arg(
                    Arg::with_name("protocol")
                        .short("X")
                        .long("protocol")
                        .takes_value(true)
                        .default_value("quic")
                        .possible_values(&["quic", "tcp"])
                        .case_insensitive(true)
                        .help("transport protocol"),
                )
                .arg(
                    Arg::with_name("time")
                        .short("t")
                        .long("time")
                        .takes_value(true)
                        .default_value("10")
                        .help("test duration in seconds"),
                )
                .arg(
                    Arg::with_name("len")
                        .short("l")
                        .long("len")
                        .takes_value(true)
                        .default_value("262144")
                        .help("write size per send in bytes"),
                )
                .arg(
                    Arg::with_name("parallel")
                        .short("P")
                        .long("parallel")
                        .takes_value(true)
                        .default_value("1")
                        .help("parallel stream count"),
                )
                .arg(
                    Arg::with_name("interval")
                        .short("i")
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
    let protocol = TransportProtocol::parse(value_of(matches, "protocol")?)?;

    if interval_secs == 0 {
        return Err("--interval must be > 0".to_string());
    }

    Ok(ServerOpts {
        bind,
        interval_secs,
        protocol,
    })
}

fn parse_client_args(matches: &clap::ArgMatches<'_>) -> AppResult<ClientOpts> {
    let bind = parse_socket(value_of(matches, "bind")?)?;
    let server = parse_socket(value_of(matches, "server")?)?;
    let time_secs = parse_u64("--time", value_of(matches, "time")?)?;
    let len = parse_usize("--len", value_of(matches, "len")?)?;
    let parallel = parse_usize("--parallel", value_of(matches, "parallel")?)?;
    let interval_secs = parse_u64("--interval", value_of(matches, "interval")?)?;
    let protocol = TransportProtocol::parse(value_of(matches, "protocol")?)?;

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
        protocol,
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
        "cyfs_perf (QUIC direct mode, no SN)",
        "",
        "Usage:",
        "  cyfs_perf server [-B <ip:port>] [-X <quic|tcp>] [-i <seconds>]",
        "  cyfs_perf client -c <ip:port> [-B <ip:port>] [-X <quic|tcp>] [-t <seconds>] [-l <bytes>] [-P <n>] [-i <seconds>]",
        "",
        "Examples:",
        "  cyfs_perf server -B 0.0.0.0:4433 -X quic",
        "  cyfs_perf client -c 127.0.0.1:4433 -t 10 -P 4 -X tcp",
    ]
    .join("\n")
}

fn format_bytes(bytes: u64) -> (f64, &'static str) {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;

    let bytes_f = bytes as f64;
    if bytes_f >= GB {
        (bytes_f / GB, "GBytes")
    } else if bytes_f >= MB {
        (bytes_f / MB, "MBytes")
    } else if bytes_f >= KB {
        (bytes_f / KB, "KBytes")
    } else {
        (bytes_f, "Bytes")
    }
}

fn format_bitrate(bytes: u64, secs: f64) -> (f64, &'static str) {
    const KBIT: f64 = 1_000.0;
    const MBIT: f64 = 1_000_000.0;
    const GBIT: f64 = 1_000_000_000.0;

    let bits_per_sec = if secs > 0.0 {
        bytes as f64 * 8.0 / secs
    } else {
        0.0
    };

    if bits_per_sec >= GBIT {
        (bits_per_sec / GBIT, "Gbits/sec")
    } else if bits_per_sec >= MBIT {
        (bits_per_sec / MBIT, "Mbits/sec")
    } else if bits_per_sec >= KBIT {
        (bits_per_sec / KBIT, "Kbits/sec")
    } else {
        (bits_per_sec, "bits/sec")
    }
}

fn print_separator() {
    println!("-----------------------------------------------------------");
}

fn print_report_header() {
    println!("[ ID] Interval           Transfer     Bitrate");
}

fn print_connection_line(id: &str, local: SocketAddr, remote: SocketAddr) {
    println!("[{id:>3}] local {local} connected to {remote}");
}

fn print_report_line(id: &str, snapshot: &ReportSnapshot) {
    let (transfer, transfer_unit) = format_bytes(snapshot.bytes);
    let (bitrate, bitrate_unit) =
        format_bitrate(snapshot.bytes, snapshot.end_secs - snapshot.start_secs);
    println!(
        "[{id:>3}] {start:>6.2}-{end:>6.2} sec  {transfer:>6.2} {transfer_unit:<7}  {bitrate:>6.2} {bitrate_unit}",
        start = snapshot.start_secs,
        end = snapshot.end_secs,
        transfer = transfer,
        transfer_unit = transfer_unit,
        bitrate = bitrate,
        bitrate_unit = bitrate_unit,
    );
}

fn new_endpoint(protocol: TransportProtocol, addr: SocketAddr) -> CyfsEndpoint {
    let mut ep = CyfsEndpoint::from((protocol.cyfs_protocol(), addr));
    ep.set_area(EndpointArea::Lan);
    ep
}

async fn create_stack(
    bind_addr: SocketAddr,
    protocol: TransportProtocol,
) -> AppResult<P2pStackRef> {
    let local_ep = new_endpoint(protocol, bind_addr);
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
    let stack = create_p2p_stack(stack_config).await.map_err(|e| {
        format!(
            "create p2p stack failed, code={:?}, msg={}",
            e.code(),
            e.msg()
        )
    })?;
    stack.set_as_default();
    Ok(stack)
}

async fn generate_runtime_identity(eps: Vec<CyfsEndpoint>) -> AppResult<(Device, PrivateKey)> {
    let private_key =
        PrivateKey::generate_rsa(1024).map_err(|e| format!("generate rsa key failed: {e}"))?;
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
    let stack = create_stack(opts.bind, opts.protocol).await?;
    let listener = stack
        .stream_manager()
        .listen(tunnel_purpose(PERF_VPORT))
        .await
        .map_err(|e| format!("listen failed, code={:?}, msg={}", e.code(), e.msg()))?;

    let interval = Duration::from_secs(opts.interval_secs);
    let next_conn_id = Arc::new(AtomicU64::new(1));

    loop {
        let (mut read, _write) = listener
            .accept()
            .await
            .map_err(|e| format!("accept failed, code={:?}, msg={}", e.code(), e.msg()))?;
        let conn_id = next_conn_id.fetch_add(1, Ordering::Relaxed);
        tokio::task::spawn(async move {
            let mut transfer_started: Option<Instant> = None;
            let mut interval_started: Option<Instant> = None;
            let mut bytes = 0u64;
            let mut interval_bytes = 0u64;
            let mut printed_intro = false;
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
                            if !printed_intro {
                                print_separator();
                                print_connection_line(
                                    &conn_id.to_string(),
                                    read.local().addr().clone(),
                                    read.remote().addr().clone(),
                                );
                                print_report_header();
                                printed_intro = true;
                            }
                        }
                        bytes += n;
                        interval_bytes += n;
                        if let Some(begin) = interval_started {
                            let elapsed = now.duration_since(begin);
                            if elapsed >= interval {
                                if interval_bytes > 0 {
                                    let base = transfer_started.unwrap();
                                    print_report_line(
                                        &conn_id.to_string(),
                                        &ReportSnapshot {
                                            start_secs: begin.duration_since(base).as_secs_f64(),
                                            end_secs: now.duration_since(base).as_secs_f64(),
                                            bytes: interval_bytes,
                                        },
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
                        print_report_line(
                            &conn_id.to_string(),
                            &ReportSnapshot {
                                start_secs: begin.duration_since(started).as_secs_f64(),
                                end_secs: Instant::now().duration_since(started).as_secs_f64(),
                                bytes: interval_bytes,
                            },
                        );
                    }
                }
                print_report_line(
                    &conn_id.to_string(),
                    &ReportSnapshot {
                        start_secs: 0.0,
                        end_secs: started.elapsed().as_secs_f64(),
                        bytes,
                    },
                );
            }
        });
    }
}

async fn run_client(opts: ClientOpts) -> AppResult<()> {
    let stack = create_stack(opts.bind, opts.protocol).await?;
    let remote = P2pEndpoint::from((opts.protocol.p2p_protocol(), opts.server));
    let deadline = Instant::now() + Duration::from_secs(opts.time_secs);

    print_separator();
    println!(
        "Client connecting to {}, {} port {}",
        opts.server.ip(),
        opts.protocol.name(),
        opts.server.port()
    );
    println!("[  5] local {} connected to {}", opts.bind, opts.server);
    print_report_header();

    let bytes_total = Arc::new(AtomicU64::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let report_handle = tokio::task::spawn(start_reporter(
        bytes_total.clone(),
        Duration::from_secs(opts.interval_secs),
        stop.clone(),
        Instant::now(),
        opts.parallel,
    ));

    let started = Instant::now();
    let mut handles = Vec::with_capacity(opts.parallel);
    for index in 0..opts.parallel {
        let stack = stack.clone();
        let remote = remote.clone();
        let bytes_total = bytes_total.clone();
        let deadline = deadline;
        let payload = vec![0u8; opts.len];
        let stream_id = (index + 5).to_string();
        let interval = Duration::from_secs(opts.interval_secs);

        handles.push(tokio::task::spawn(async move {
            let (read, mut write) = stack
                .stream_manager()
                .connect_direct(vec![remote], tunnel_purpose(PERF_VPORT), None)
                .await
                .map_err(|e| {
                    format!(
                        "connect direct failed, code={:?}, msg={}",
                        e.code(),
                        e.msg()
                    )
                })?;

            print_connection_line(
                &stream_id,
                read.local().addr().clone(),
                read.remote().addr().clone(),
            );

            let stream_started = Instant::now();
            let mut interval_started = stream_started;
            let mut interval_bytes = 0u64;
            let mut total_bytes = 0u64;
            while Instant::now() < deadline {
                write
                    .write_all(payload.as_slice())
                    .await
                    .map_err(|e| format!("client write failed: {e}"))?;
                bytes_total.fetch_add(payload.len() as u64, Ordering::Relaxed);
                total_bytes += payload.len() as u64;
                interval_bytes += payload.len() as u64;

                let now = Instant::now();
                if now.duration_since(interval_started) >= interval {
                    print_report_line(
                        &stream_id,
                        &ReportSnapshot {
                            start_secs: interval_started
                                .duration_since(stream_started)
                                .as_secs_f64(),
                            end_secs: now.duration_since(stream_started).as_secs_f64(),
                            bytes: interval_bytes,
                        },
                    );
                    interval_started = now;
                    interval_bytes = 0;
                }
            }

            write
                .flush()
                .await
                .map_err(|e| format!("client flush failed: {e}"))?;

            if interval_bytes > 0 {
                let now = Instant::now();
                print_report_line(
                    &stream_id,
                    &ReportSnapshot {
                        start_secs: interval_started
                            .duration_since(stream_started)
                            .as_secs_f64(),
                        end_secs: now.duration_since(stream_started).as_secs_f64(),
                        bytes: interval_bytes,
                    },
                );
            }
            print_report_line(
                &stream_id,
                &ReportSnapshot {
                    start_secs: 0.0,
                    end_secs: stream_started.elapsed().as_secs_f64(),
                    bytes: total_bytes,
                },
            );
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
    if opts.parallel > 1 {
        print_report_line(
            "SUM",
            &ReportSnapshot {
                start_secs: 0.0,
                end_secs: elapsed,
                bytes: total_bytes,
            },
        );
    }
    Ok(())
}

async fn start_reporter(
    total_bytes: Arc<AtomicU64>,
    interval: Duration,
    stop: Arc<AtomicBool>,
    started: Instant,
    parallel: usize,
) {
    let mut last_bytes = 0u64;
    while !stop.load(Ordering::Relaxed) {
        tokio::time::sleep(interval).await;
        let now_total = total_bytes.load(Ordering::Relaxed);
        let delta = now_total.saturating_sub(last_bytes);
        last_bytes = now_total;
        if delta == 0 {
            continue;
        }
        let now = Instant::now();
        if parallel > 1 {
            print_report_line(
                "SUM",
                &ReportSnapshot {
                    start_secs: now.duration_since(started).as_secs_f64() - interval.as_secs_f64(),
                    end_secs: now.duration_since(started).as_secs_f64(),
                    bytes: delta,
                },
            );
        }
    }
}
