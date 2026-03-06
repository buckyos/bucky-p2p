use clap::{App, Arg, SubCommand};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, Error as RustlsError, ServerConfig, SignatureScheme};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::{TlsAcceptor, TlsConnector, TlsStream};

const DEFAULT_SERVER_BIND: &str = "0.0.0.0:5201";
const DEFAULT_CLIENT_BIND: &str = "0.0.0.0:0";
const DEFAULT_TLS_SERVER_NAME: &str = "tcp-iperf3.local";

type AppResult<T> = Result<T, String>;

struct ReportSnapshot {
    start_secs: f64,
    end_secs: f64,
    bytes: u64,
}

#[derive(Clone)]
struct ServerOpts {
    bind: SocketAddr,
    interval_secs: u64,
    tls: bool,
}

#[derive(Clone)]
struct ClientOpts {
    bind: SocketAddr,
    server: SocketAddr,
    time_secs: u64,
    len: usize,
    parallel: usize,
    interval_secs: u64,
    tls: bool,
}

trait AsyncStream: AsyncRead + AsyncWrite + Unpin + Send {}

impl<T> AsyncStream for T where T: AsyncRead + AsyncWrite + Unpin + Send {}

type BoxedStream = Box<dyn AsyncStream>;

struct ConnectedStream {
    stream: BoxedStream,
    local: SocketAddr,
    remote: SocketAddr,
}

#[derive(Debug)]
struct NoCertificateVerification;

enum Command {
    Server(ServerOpts),
    Client(ClientOpts),
}

impl ServerCertVerifier for NoCertificateVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, RustlsError> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::RSA_PKCS1_SHA256,
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::ED25519,
        ]
    }
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
    let app = App::new("tcp_iperf3")
        .about("TCP throughput tool")
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
                    Arg::with_name("interval")
                        .short("i")
                        .long("interval")
                        .takes_value(true)
                        .default_value("1")
                        .help("report interval in seconds"),
                )
                .arg(
                    Arg::with_name("tls")
                        .long("tls")
                        .help("enable TLS over TCP"),
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
                )
                .arg(
                    Arg::with_name("tls")
                        .long("tls")
                        .help("enable TLS over TCP"),
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
        tls: matches.is_present("tls"),
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
        tls: matches.is_present("tls"),
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
        "tcp_iperf3",
        "",
        "Usage:",
        "  tcp_iperf3 server [-B <ip:port>] [-i <seconds>] [--tls]",
        "  tcp_iperf3 client -c <ip:port> [-B <ip:port>] [-t <seconds>] [-l <bytes>] [-P <n>] [-i <seconds>] [--tls]",
        "",
        "Examples:",
        "  tcp_iperf3 server -B 0.0.0.0:5201",
        "  tcp_iperf3 client -c 127.0.0.1:5201 -t 10 -P 4",
        "  tcp_iperf3 server -B 0.0.0.0:5201 --tls",
        "  tcp_iperf3 client -c 127.0.0.1:5201 -t 10 -P 4 --tls",
    ]
    .join("\n")
}

fn provider() -> rustls::crypto::CryptoProvider {
    rustls::crypto::ring::default_provider()
}

fn build_server_tls_acceptor() -> AppResult<TlsAcceptor> {
    let certified = rcgen::generate_simple_self_signed(vec![DEFAULT_TLS_SERVER_NAME.to_string()])
        .map_err(|e| format!("generate self-signed certificate failed: {e}"))?;
    let cert_chain = vec![certified.cert.der().clone()];
    let key = PrivatePkcs8KeyDer::from(certified.key_pair.serialize_der());
    let config = ServerConfig::builder_with_provider(provider().into())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|e| format!("build tls server config failed: {e}"))?
        .with_no_client_auth()
        .with_single_cert(cert_chain, key.into())
        .map_err(|e| format!("install tls server certificate failed: {e}"))?;
    Ok(TlsAcceptor::from(Arc::new(config)))
}

fn build_client_tls_connector() -> AppResult<TlsConnector> {
    let config = ClientConfig::builder_with_provider(provider().into())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|e| format!("build tls client config failed: {e}"))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(NoCertificateVerification))
        .with_no_client_auth();
    Ok(TlsConnector::from(Arc::new(config)))
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

async fn run_server(opts: ServerOpts) -> AppResult<()> {
    let tls_acceptor = if opts.tls {
        Some(build_server_tls_acceptor()?)
    } else {
        None
    };
    let listener = TcpListener::bind(opts.bind)
        .await
        .map_err(|e| format!("tcp listen failed on {}: {e}", opts.bind))?;
    let local = listener
        .local_addr()
        .map_err(|e| format!("get local listen address failed: {e}"))?;

    println!(
        "Server listening on {local} ({})",
        if opts.tls { "TLS over TCP" } else { "plain TCP" }
    );

    let interval = Duration::from_secs(opts.interval_secs);
    let next_conn_id = Arc::new(AtomicU64::new(1));

    loop {
        let (stream, remote) = listener
            .accept()
            .await
            .map_err(|e| format!("accept failed: {e}"))?;
        let local = stream
            .local_addr()
            .map_err(|e| format!("get accepted local address failed: {e}"))?;
        let conn_id = next_conn_id.fetch_add(1, Ordering::Relaxed);
        let tls_acceptor = tls_acceptor.clone();

        tokio::task::spawn(async move {
            let mut stream: BoxedStream = if let Some(tls_acceptor) = tls_acceptor {
                match tls_acceptor.accept(stream).await {
                    Ok(stream) => Box::new(TlsStream::from(stream)),
                    Err(e) => {
                        eprintln!("server tls accept failed, conn {conn_id}: {e}");
                        return;
                    }
                }
            } else {
                Box::new(stream)
            };

            let mut transfer_started: Option<Instant> = None;
            let mut interval_started: Option<Instant> = None;
            let mut bytes = 0u64;
            let mut interval_bytes = 0u64;
            let mut printed_intro = false;
            let mut buf = vec![0u8; 64 * 1024];

            loop {
                match stream.read(buf.as_mut_slice()).await {
                    Ok(0) => break,
                    Ok(n) => {
                        let n = n as u64;
                        let now = Instant::now();
                        if transfer_started.is_none() {
                            transfer_started = Some(now);
                            interval_started = Some(now);
                            if !printed_intro {
                                print_separator();
                                print_connection_line(&conn_id.to_string(), local, remote);
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
                let finished = Instant::now();
                if let Some(begin) = interval_started {
                    let elapsed = finished.duration_since(begin);
                    if interval_bytes > 0 && elapsed.as_secs_f64() > 0.0 {
                        print_report_line(
                            &conn_id.to_string(),
                            &ReportSnapshot {
                                start_secs: begin.duration_since(started).as_secs_f64(),
                                end_secs: finished.duration_since(started).as_secs_f64(),
                                bytes: interval_bytes,
                            },
                        );
                    }
                }
                print_report_line(
                    &conn_id.to_string(),
                    &ReportSnapshot {
                        start_secs: 0.0,
                        end_secs: finished.duration_since(started).as_secs_f64(),
                        bytes,
                    },
                );
            }
        });
    }
}

async fn run_client(opts: ClientOpts) -> AppResult<()> {
    let deadline = Instant::now() + Duration::from_secs(opts.time_secs);
    let tls_connector = if opts.tls {
        Some(build_client_tls_connector()?)
    } else {
        None
    };

    print_separator();
    println!(
        "Client connecting to {}, {} port {}",
        opts.server.ip(),
        if opts.tls { "TLS/TCP" } else { "TCP" },
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
        let bind = opts.bind;
        let server = opts.server;
        let bytes_total = bytes_total.clone();
        let deadline = deadline;
        let payload = vec![0u8; opts.len];
        let stream_id = (index + 5).to_string();
        let interval = Duration::from_secs(opts.interval_secs);
        let tls_connector = tls_connector.clone();

        handles.push(tokio::task::spawn(async move {
            let ConnectedStream {
                mut stream,
                local,
                remote,
            } = connect_from(bind, server, tls_connector).await?;

            print_connection_line(&stream_id, local, remote);

            let stream_started = Instant::now();
            let mut interval_started = stream_started;
            let mut interval_bytes = 0u64;
            let mut total_bytes = 0u64;
            while Instant::now() < deadline {
                stream
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
                            start_secs: interval_started.duration_since(stream_started).as_secs_f64(),
                            end_secs: now.duration_since(stream_started).as_secs_f64(),
                            bytes: interval_bytes,
                        },
                    );
                    interval_started = now;
                    interval_bytes = 0;
                }
            }

            stream
                .flush()
                .await
                .map_err(|e| format!("client flush failed: {e}"))?;

            if interval_bytes > 0 {
                let now = Instant::now();
                print_report_line(
                    &stream_id,
                    &ReportSnapshot {
                        start_secs: interval_started.duration_since(stream_started).as_secs_f64(),
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

async fn connect_from(
    bind: SocketAddr,
    server: SocketAddr,
    tls_connector: Option<TlsConnector>,
) -> AppResult<ConnectedStream> {
    let socket = if server.is_ipv4() {
        tokio::net::TcpSocket::new_v4().map_err(|e| format!("create tcp v4 socket failed: {e}"))?
    } else {
        tokio::net::TcpSocket::new_v6().map_err(|e| format!("create tcp v6 socket failed: {e}"))?
    };
    socket
        .bind(bind)
        .map_err(|e| format!("bind local tcp address {bind} failed: {e}"))?;
    let socket = socket
        .connect(server)
        .await
        .map_err(|e| format!("connect tcp server {server} failed: {e}"))?;
    let local = socket
        .local_addr()
        .map_err(|e| format!("get local tcp address failed: {e}"))?;
    let remote = socket
        .peer_addr()
        .map_err(|e| format!("get remote tcp address failed: {e}"))?;

    let stream: BoxedStream = if let Some(tls_connector) = tls_connector {
        let server_name = ServerName::try_from(DEFAULT_TLS_SERVER_NAME)
            .map_err(|e| format!("invalid tls server name: {e}"))?;
        let stream = tls_connector
            .connect(server_name, socket)
            .await
            .map_err(|e| format!("tls connect to {server} failed: {e}"))?;
        Box::new(TlsStream::from(stream))
    } else {
        Box::new(socket)
    };

    Ok(ConnectedStream {
        stream,
        local,
        remote,
    })
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
