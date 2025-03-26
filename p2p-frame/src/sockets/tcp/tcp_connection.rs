use std::any::Any;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use as_any::{AsAny, Downcast};
use bucky_raw_codec::{RawConvertTo};
use crate::runtime::{TcpStream, TlsConnector};
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
use rustls::version::TLS13;
use tokio::net::TcpSocket;
use crate::error::{p2p_err, P2pErrorCode, P2pResult, into_p2p_err};
use crate::p2p_identity::{P2pId, P2pIdentityRef, P2pIdentityCertFactoryRef};
use crate::endpoint::{Endpoint, Protocol};
use crate::p2p_connection::{P2pConnection, P2pRead, P2pWrite};
use crate::runtime;

#[derive(Eq, PartialEq)]
enum BoxType {
    Package,
    RawData,
}
pub struct TCPSend {
    socket: runtime::TlsStream<TcpStream>
}
pub struct TCPConnection;

impl TCPConnection {
    pub async fn connect(
        cert_factory: P2pIdentityCertFactoryRef,
        local_identity_ref: P2pIdentityRef,
        remote_ep: Endpoint,
        remote_identity_id: P2pId,
        remote_name: Option<String>,
        timeout: Duration,) -> P2pResult<P2pConnection> {
        let client_config =
            rustls::ClientConfig::builder_with_provider(crate::tls::provider().into())
                .with_protocol_versions(&[&TLS13])
                .unwrap()
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(crate::tls::TlsServerCertVerifier::new(cert_factory, remote_identity_id.clone())))
                .with_client_auth_cert(vec![CertificateDer::from(local_identity_ref.get_identity_cert()?.get_encoded_cert()?)],
                                       PrivatePkcs8KeyDer::from(local_identity_ref.get_encoded_identity()?).into()).unwrap();

        let socket = runtime::timeout(timeout, TcpStream::connect(remote_ep.addr()))
            .await
            .map_err(into_p2p_err!(P2pErrorCode::ConnectFailed, "tcp socket to {} connect failed", remote_ep))?
            .map_err(into_p2p_err!(P2pErrorCode::ConnectFailed, "tcp socket to {} connect failed", remote_ep))?;

        let local = socket.local_addr().map_err(into_p2p_err!(P2pErrorCode::Failed))?;
        let local = Endpoint::from((Protocol::Tcp, local));
        let remote_name = remote_name.unwrap_or(remote_identity_id.to_string());

        let tls_connector = TlsConnector::from(Arc::new(client_config));
        let tls_stream = tls_connector.connect(remote_name.clone().try_into().unwrap(), socket).await.map_err(into_p2p_err!(P2pErrorCode::ConnectFailed, "tls socket to {} connect failed", remote_ep))?;
        let (read, write) = runtime::split(runtime::TlsStream::from(tls_stream));
        let read = TCPRead::new(read, local_identity_ref.get_id(), remote_identity_id.clone(), local, remote_ep, remote_name.clone());
        let write = TCPWrite::new(write, local_identity_ref.get_id(), remote_identity_id, local, remote_ep, remote_name.clone());
        let socket = P2pConnection::new(Box::new(read), Box::new(write));
        Ok(socket)
    }

    pub async fn connect_with_ep(
        cert_factory: P2pIdentityCertFactoryRef,
        local_identity_ref: P2pIdentityRef,
        local_ep: Endpoint,
        remote_ep: Endpoint,
        remote_identity_id: P2pId,
        remote_name: Option<String>,
        timeout: Duration,) -> P2pResult<P2pConnection> {
        let client_config =
            rustls::ClientConfig::builder_with_provider(crate::tls::provider().into())
                .with_protocol_versions(&[&TLS13])
                .unwrap()
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(crate::tls::TlsServerCertVerifier::new(cert_factory, remote_identity_id.clone())))
                .with_client_auth_cert(vec![CertificateDer::from(local_identity_ref.get_name().to_vec().unwrap())],
                                       PrivatePkcs8KeyDer::from(local_identity_ref.get_encoded_identity()?).into()).unwrap();

        let socket = if local_ep.addr().is_ipv4() {
            TcpSocket::new_v4().map_err(into_p2p_err!(P2pErrorCode::ConnectFailed, "create socket failed"))?
        } else {
            TcpSocket::new_v6().map_err(into_p2p_err!(P2pErrorCode::ConnectFailed, "create socket failed"))?
        };
        socket.bind(local_ep.addr().clone()).map_err(into_p2p_err!(P2pErrorCode::ConnectFailed, "bind to {} failed", local_ep))?;
        let socket = runtime::timeout(timeout, socket.connect(remote_ep.addr().clone()))
            .await
            .map_err(into_p2p_err!(P2pErrorCode::ConnectFailed, "tcp socket to {} connect failed", remote_ep))?
            .map_err(into_p2p_err!(P2pErrorCode::ConnectFailed, "tcp socket to {} connect failed", remote_ep))?;

        let local = socket.local_addr().map_err(into_p2p_err!(P2pErrorCode::Failed))?;
        let local = Endpoint::from((Protocol::Tcp, local));
        let remote_name = remote_name.unwrap_or(remote_identity_id.to_string());

        let tls_connector = TlsConnector::from(Arc::new(client_config));
        let tls_stream = tls_connector.connect(remote_name.clone().try_into().unwrap(), socket).await.map_err(into_p2p_err!(P2pErrorCode::ConnectFailed, "tls socket to {} connect failed", remote_ep))?;
        let (read, write) = runtime::split(runtime::TlsStream::from(tls_stream));
        let read = TCPRead::new(read, local_identity_ref.get_id(), remote_identity_id.clone(), local, remote_ep, remote_name.clone());
        let write = TCPWrite::new(write, local_identity_ref.get_id(), remote_identity_id, local, remote_ep,remote_name);
        let socket = P2pConnection::new(Box::new(read), Box::new(write));
        Ok(socket)
    }
}

pub(crate) struct TCPRead {
    read: runtime::ReadHalf<runtime::TlsStream<TcpStream>>,
    remote_identity_id: P2pId,
    local_identity_id: P2pId,
    local: Endpoint,
    remote: Endpoint,
    remote_name: String,
}

impl TCPRead {
    pub fn new(read: runtime::ReadHalf<runtime::TlsStream<TcpStream>>,
               remote_identity_id: P2pId,
               local_identity_id: P2pId,
               local: Endpoint,
               remote: Endpoint,
               remote_name: String,) -> Self {
        Self {
            read,
            remote_identity_id,
            local_identity_id,
            local,
            remote,
            remote_name,
        }
    }
}

impl P2pRead for TCPRead {
    fn remote(&self) -> Endpoint {
        self.remote.clone()
    }

    fn local(&self) -> Endpoint {
        self.local.clone()
    }

    fn remote_id(&self) -> P2pId {
        self.remote_identity_id.clone()
    }

    fn local_id(&self) -> P2pId {
        self.local_identity_id.clone()
    }

    fn remote_name(&self) -> String {
        self.remote_name.clone()
    }
}


impl runtime::AsyncRead for TCPRead {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf,
    ) -> std::task::Poll<std::io::Result<()>> {
        Pin::new(&mut self.read).poll_read(cx, buf)
    }
}

pub(crate) struct TCPWrite {
    write: runtime::WriteHalf<runtime::TlsStream<TcpStream>>,
    remote_identity_id: P2pId,
    local_identity_id: P2pId,
    local: Endpoint,
    remote: Endpoint,
    remote_name: String,
}

impl TCPWrite {
    pub fn new(write: runtime::WriteHalf<runtime::TlsStream<TcpStream>>,
               remote_identity_id: P2pId,
               local_identity_id: P2pId,
               local: Endpoint,
               remote: Endpoint,
               remote_name: String,) -> Self {
        Self {
            write,
            remote_identity_id,
            local_identity_id,
            local,
            remote,
            remote_name,
        }
    }
}

impl P2pWrite for TCPWrite {
    fn remote(&self) -> Endpoint {
        self.remote.clone()
    }

    fn local(&self) -> Endpoint {
        self.local.clone()
    }

    fn remote_id(&self) -> P2pId {
        self.remote_identity_id.clone()
    }

    fn local_id(&self) -> P2pId {
        self.local_identity_id.clone()
    }

    fn remote_name(&self) -> String {
        self.remote_name.clone()
    }
}

impl runtime::AsyncWrite for TCPWrite {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        Pin::new(&mut self.write).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        Pin::new(&mut self.write).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        Pin::new(&mut self.write).poll_shutdown(cx)
    }
}
