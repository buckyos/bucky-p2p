use super::protocol::{TcpConnectionHello, read_raw_frame, write_raw_frame};
use crate::endpoint::{Endpoint, Protocol};
use crate::error::{P2pErrorCode, P2pResult, into_p2p_err, p2p_err};
use crate::networks::{parse_server_name, validate_server_name};
use crate::p2p_identity::{P2pId, P2pIdentityCertFactoryRef, P2pIdentityRef};
use crate::runtime;
use crate::runtime::{TcpListener, TcpStream, TlsAcceptor, TlsConnector};
use crate::tls::{ServerCertResolverRef, TlsServerCertResolver};
use rustls::ServerConfig;
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
use rustls::version::TLS13;
use socket2::{Domain, Protocol as SocketProtocol, Socket, Type};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpSocket;

pub(crate) struct TcpTlsConnection {
    pub stream: runtime::TlsStream<TcpStream>,
    pub local_ep: Endpoint,
    pub remote_ep: Endpoint,
    pub local_id: P2pId,
    pub local_name: String,
    pub remote_id: P2pId,
    pub remote_name: String,
}

pub(crate) fn build_acceptor(
    cert_resolver: ServerCertResolverRef,
    cert_factory: P2pIdentityCertFactoryRef,
) -> TlsAcceptor {
    let mut server_config = ServerConfig::builder_with_provider(crate::tls::provider().into())
        .with_protocol_versions(&[&TLS13])
        .unwrap()
        .with_client_cert_verifier(Arc::new(crate::tls::TlsClientCertVerifier::new(
            cert_factory,
        )))
        .with_cert_resolver(cert_resolver.clone().get_resolves_server_cert());
    server_config.key_log = Arc::new(rustls::KeyLogFile::new());
    TlsAcceptor::from(Arc::new(server_config))
}

fn new_tcp_socket(addr: &std::net::SocketAddr, reuse_address: bool) -> P2pResult<TcpSocket> {
    let domain = match addr {
        std::net::SocketAddr::V4(_) => Domain::IPV4,
        std::net::SocketAddr::V6(_) => Domain::IPV6,
    };
    let socket = Socket::new(domain, Type::STREAM, Some(SocketProtocol::TCP)).map_err(
        into_p2p_err!(P2pErrorCode::Failed, "create tcp socket failed"),
    )?;
    socket.set_nonblocking(true).map_err(into_p2p_err!(
        P2pErrorCode::Failed,
        "set tcp socket nonblocking failed"
    ))?;
    #[cfg(target_os = "linux")]
    if reuse_address {
        socket.set_reuse_address(true).map_err(into_p2p_err!(
            P2pErrorCode::Failed,
            "set reuse address failed"
        ))?;
    }

    Ok(TcpSocket::from_std_stream(socket.into()))
}

pub(crate) async fn bind_listener(local: Endpoint, reuse_address: bool) -> P2pResult<TcpListener> {
    let bind_local = if local.addr().is_ipv4() {
        local
    } else {
        Endpoint::default_tcp(&local)
    };
    let socket = new_tcp_socket(bind_local.addr(), reuse_address).map_err(into_p2p_err!(
        P2pErrorCode::AlreadyExists,
        "create tcp socket failed"
    ))?;
    socket.bind(*bind_local.addr()).map_err(into_p2p_err!(
        P2pErrorCode::AlreadyExists,
        "bind port failed"
    ))?;
    socket.listen(1024).map_err(into_p2p_err!(
        P2pErrorCode::AlreadyExists,
        "listen port failed"
    ))
}

fn build_client_config(
    cert_factory: P2pIdentityCertFactoryRef,
    local_identity_ref: &P2pIdentityRef,
    remote_identity_id: &P2pId,
) -> P2pResult<rustls::ClientConfig> {
    rustls::ClientConfig::builder_with_provider(crate::tls::provider().into())
        .with_protocol_versions(&[&TLS13])
        .unwrap()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(crate::tls::TlsServerCertVerifier::new(
            cert_factory,
            remote_identity_id.clone(),
        )))
        .with_client_auth_cert(
            vec![CertificateDer::from(
                local_identity_ref.get_identity_cert()?.get_encoded_cert()?,
            )],
            PrivatePkcs8KeyDer::from(local_identity_ref.get_encoded_identity()?).into(),
        )
        .map_err(into_p2p_err!(P2pErrorCode::TlsError))
}

pub(crate) async fn connect_with_optional_local(
    cert_factory: P2pIdentityCertFactoryRef,
    local_identity_ref: &P2pIdentityRef,
    local_ep: Option<&Endpoint>,
    remote_ep: &Endpoint,
    remote_identity_id: &P2pId,
    remote_name: Option<String>,
    timeout: Duration,
    reuse_address: bool,
    hello: &TcpConnectionHello,
) -> P2pResult<TcpTlsConnection> {
    let client_config =
        build_client_config(cert_factory.clone(), local_identity_ref, remote_identity_id)?;
    let socket = if let Some(local_ep) = local_ep {
        let socket = new_tcp_socket(local_ep.addr(), reuse_address).map_err(into_p2p_err!(
            P2pErrorCode::ConnectFailed,
            "create tcp socket failed"
        ))?;
        socket.bind(*local_ep.addr()).map_err(into_p2p_err!(
            P2pErrorCode::ConnectFailed,
            "bind to {} failed",
            local_ep
        ))?;
        runtime::timeout(timeout, socket.connect(*remote_ep.addr()))
            .await
            .map_err(into_p2p_err!(
                P2pErrorCode::ConnectFailed,
                "tcp socket to {} connect failed",
                remote_ep
            ))?
            .map_err(into_p2p_err!(
                P2pErrorCode::ConnectFailed,
                "tcp socket to {} connect failed",
                remote_ep
            ))?
    } else {
        runtime::timeout(timeout, TcpStream::connect(remote_ep.addr()))
            .await
            .map_err(into_p2p_err!(
                P2pErrorCode::ConnectFailed,
                "tcp socket to {} connect failed",
                remote_ep
            ))?
            .map_err(into_p2p_err!(
                P2pErrorCode::ConnectFailed,
                "tcp socket to {} connect failed",
                remote_ep
            ))?
    };

    let local_addr = socket
        .local_addr()
        .map_err(into_p2p_err!(P2pErrorCode::Failed))?;
    let local = Endpoint::from((Protocol::Tcp, local_addr));
    let remote_name = remote_name.unwrap_or(remote_identity_id.to_string());

    let tls_connector = TlsConnector::from(Arc::new(client_config));
    let mut tls_stream = tls_connector
        .connect(
            validate_server_name(remote_name.clone())
                .try_into()
                .unwrap(),
            socket,
        )
        .await
        .map_err(into_p2p_err!(
            P2pErrorCode::ConnectFailed,
            "tls socket to {} connect failed",
            remote_ep
        ))?;

    write_raw_frame(&mut tls_stream, hello).await?;

    let (remote_identity_id, remote_name) = if remote_identity_id.is_default() {
        let (_, tls_conn) = tls_stream.get_ref();
        let cert = tls_conn
            .peer_certificates()
            .ok_or_else(|| p2p_err!(P2pErrorCode::CertError, "no cert"))?;
        if cert.is_empty() {
            return Err(p2p_err!(P2pErrorCode::CertError, "no cert"));
        }
        let remote_device = cert_factory.create(&cert[0].as_ref().to_vec())?;
        (remote_device.get_id(), remote_device.get_name())
    } else {
        (remote_identity_id.clone(), remote_name)
    };

    Ok(TcpTlsConnection {
        stream: runtime::TlsStream::from(tls_stream),
        local_ep: local,
        remote_ep: *remote_ep,
        local_id: local_identity_ref.get_id(),
        local_name: local_identity_ref.get_name(),
        remote_id: remote_identity_id,
        remote_name,
    })
}

pub(crate) async fn accept_connection(
    acceptor: &TlsAcceptor,
    cert_factory: &P2pIdentityCertFactoryRef,
    cert_resolver: &ServerCertResolverRef,
    socket: TcpStream,
) -> P2pResult<(TcpTlsConnection, TcpConnectionHello)> {
    let remote = socket
        .peer_addr()
        .map_err(into_p2p_err!(P2pErrorCode::Failed))?;
    let local = socket
        .local_addr()
        .map_err(into_p2p_err!(P2pErrorCode::Failed))?;
    let remote = Endpoint::from((Protocol::Tcp, remote));
    let local = Endpoint::from((Protocol::Tcp, local));

    let mut tls_stream = acceptor
        .accept(socket)
        .await
        .map_err(into_p2p_err!(P2pErrorCode::ConnectFailed))?;
    let hello = read_raw_frame::<_, TcpConnectionHello>(&mut tls_stream).await?;

    let (_, tls_conn) = tls_stream.get_ref();
    let cert = tls_conn
        .peer_certificates()
        .ok_or_else(|| p2p_err!(P2pErrorCode::CertError, "no cert"))?;
    if cert.is_empty() {
        return Err(p2p_err!(P2pErrorCode::CertError, "no cert"));
    }

    let server_name = tls_conn
        .server_name()
        .ok_or_else(|| p2p_err!(P2pErrorCode::CertError, "no server name"))?;
    let server_name = parse_server_name(server_name).to_owned();
    let local_cert = cert_resolver
        .get_server_identity(server_name.as_str())
        .await
        .ok_or_else(|| p2p_err!(P2pErrorCode::CertError, "no server cert"))?;
    let local_id = local_cert.get_id();
    let remote_device = cert_factory.create(&cert[0].as_ref().to_vec())?;
    let remote_id = remote_device.get_id();
    let remote_name = remote_device.get_name();

    Ok((
        TcpTlsConnection {
            stream: runtime::TlsStream::from(tls_stream),
            local_ep: local,
            remote_ep: remote,
            local_id,
            local_name: server_name,
            remote_id,
            remote_name,
        },
        hello,
    ))
}
