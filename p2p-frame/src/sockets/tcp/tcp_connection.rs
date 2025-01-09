use std::any::Any;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use as_any::Downcast;
use bucky_raw_codec::{RawConvertTo};
use crate::runtime::{TcpStream, TlsConnector};
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
use rustls::version::TLS13;
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
pub struct TCPConnection {
    socket: Mutex<Option<runtime::TlsStream<TcpStream>>>,
    remote_identity_id: P2pId,
    local_identity_id: P2pId,
    local: Endpoint,
    remote: Endpoint,
    write_lock: Mutex<u8>,
}

impl std::fmt::Display for TCPConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "TcpSocket {{local:{},remote:{}}}",
            self.local, self.remote
        )
    }
}

impl TCPConnection {
    pub fn new(socket: runtime::TlsStream<TcpStream>,
               local_identity_id: P2pId,
               remote_identity_id: P2pId,
               local: Endpoint,
               remote: Endpoint) -> Self {
        Self {
            socket: Mutex::new(Some(socket)),
            remote_identity_id,
            local_identity_id,
            local,
            remote,
            write_lock: Mutex::new(0),
        }
    }


    pub async fn connect(
        cert_factory: P2pIdentityCertFactoryRef,
        local_identity_ref: P2pIdentityRef,
        remote_ep: Endpoint,
        remote_identity_id: P2pId,
        timeout: Duration,) -> P2pResult<TCPConnection> {
        let client_config =
            rustls::ClientConfig::builder_with_provider(crate::tls::provider().into())
                .with_protocol_versions(&[&TLS13])
                .unwrap()
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(crate::tls::TlsServerCertVerifier::new(cert_factory)))
                .with_client_auth_cert(vec![CertificateDer::from(local_identity_ref.get_name().to_vec().unwrap())],
                                       PrivatePkcs8KeyDer::from(local_identity_ref.get_encoded_identity()?).into()).unwrap();

        let socket = runtime::timeout(timeout, TcpStream::connect(remote_ep.addr()))
            .await
            .map_err(into_p2p_err!(P2pErrorCode::ConnectFailed, "tcp socket to {} connect failed", remote_ep))?
            .map_err(into_p2p_err!(P2pErrorCode::ConnectFailed, "tcp socket to {} connect failed", remote_ep))?;

        let local = socket.local_addr().map_err(into_p2p_err!(P2pErrorCode::Failed))?;
        let local = Endpoint::from((Protocol::Tcp, local));

        let tls_connector = TlsConnector::from(Arc::new(client_config));
        let tls_stream = tls_connector.connect(remote_identity_id.to_string().try_into().unwrap(), socket).await.map_err(into_p2p_err!(P2pErrorCode::ConnectFailed, "tls socket to {} connect failed", remote_ep))?;
        let socket = TCPConnection::new(runtime::TlsStream::from(tls_stream), local_identity_ref.get_id(), remote_identity_id, local, remote_ep);
        debug!("{} connected", socket);
        Ok(socket)
    }
}

struct TCPRead {
    read: Option<runtime::ReadHalf<runtime::TlsStream<TcpStream>>>,
}

impl TCPRead {
    pub fn new(read: runtime::ReadHalf<runtime::TlsStream<TcpStream>>) -> Self {
        Self {
            read: Some(read),
        }
    }

    pub fn take(&mut self) -> Option<runtime::ReadHalf<runtime::TlsStream<TcpStream>>> {
        self.read.take()
    }
}

impl P2pRead for TCPRead {
    fn get_any(&self) -> &dyn Any {
        self
    }

    fn get_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}


impl runtime::AsyncRead for TCPRead {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf,
    ) -> std::task::Poll<std::io::Result<()>> {
        let read = self.read.as_mut().unwrap();
        Pin::new(read).poll_read(cx, buf)
    }
}

struct TCPWrite {
    write: Option<runtime::WriteHalf<runtime::TlsStream<TcpStream>>>,
}

impl TCPWrite {
    pub fn new(write: runtime::WriteHalf<runtime::TlsStream<TcpStream>>) -> Self {
        Self {
            write: Some(write),
        }
    }

    pub fn take(&mut self) -> Option<runtime::WriteHalf<runtime::TlsStream<TcpStream>>> {
        self.write.take()
    }
}

impl P2pWrite for TCPWrite {
    fn get_any(&self) -> &dyn Any {
        self
    }

    fn get_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl runtime::AsyncWrite for TCPWrite {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let write = self.write.as_mut().unwrap();
        Pin::new(write).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let write = self.write.as_mut().unwrap();
        Pin::new(write).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let write = self.write.as_mut().unwrap();
        Pin::new(write).poll_shutdown(cx)
    }
}

impl P2pConnection for TCPConnection {
    fn is_stream(&self) -> bool {
        true
    }

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

    fn split(&self) -> P2pResult<(Box<dyn P2pRead>, Box<dyn P2pWrite>)> {
        let mut socket = self.socket.lock().unwrap();
        if socket.is_none() {
            return Err(p2p_err!(P2pErrorCode::IoError, "socket is none"));
        }
        let (read, write) = runtime::split(socket.take().unwrap());
        Ok((Box::new(TCPRead::new(read)), Box::new(TCPWrite::new(write))))
    }

    fn unsplit(&self, mut read: Box<dyn P2pRead>, mut write: Box<dyn P2pWrite>) {
        let mut socket = self.socket.lock().unwrap();
        let mut read = unsafe { std::mem::transmute::<_, &mut dyn Any>(read.get_any_mut())};
        if let Some(read) = read.downcast_mut::<TCPRead>() {
            if let Some(read) = read.take() {
                let mut write = unsafe { std::mem::transmute::<_, &mut dyn Any>(write.get_any_mut())};
                if let Some(write) = write.downcast_mut::<TCPWrite>() {
                    if let Some(write) = write.take() {
                        *socket = Some(read.unsplit(write));
                    }
                }
            }
        }
    }
}
