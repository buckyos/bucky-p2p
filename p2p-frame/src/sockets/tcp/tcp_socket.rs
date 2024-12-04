use std::sync::{Arc, Mutex};
use std::time::Duration;
use bucky_raw_codec::{RawConvertTo};
use crate::runtime::{TcpStream, TlsConnector};
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
use rustls::version::TLS13;
use crate::error::{bdt_err, BdtErrorCode, BdtResult, into_bdt_err};
use crate::p2p_identity::{P2pId, P2pIdentityRef, P2pIdentityCertFactoryRef};
use crate::endpoint::{Endpoint, Protocol};
use crate::runtime;

#[derive(Eq, PartialEq)]
enum BoxType {
    Package,
    RawData,
}
pub struct TCPSend {
    socket: runtime::TlsStream<TcpStream>
}
pub struct TCPSocket {
    socket: Mutex<Option<runtime::TlsStream<TcpStream>>>,
    remote_identity_id: P2pId,
    local_identity_id: P2pId,
    local: Endpoint,
    remote: Endpoint,
    write_lock: Mutex<u8>,
}

impl std::fmt::Display for TCPSocket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "TcpSocket {{local:{},remote:{}}}",
            self.local, self.remote
        )
    }
}

impl TCPSocket {
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
        timeout: Duration,) -> BdtResult<TCPSocket> {
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
            .map_err(into_bdt_err!(BdtErrorCode::ConnectFailed, "tcp socket to {} connect failed", remote_ep))?
            .map_err(into_bdt_err!(BdtErrorCode::ConnectFailed, "tcp socket to {} connect failed", remote_ep))?;

        let local = socket.local_addr().map_err(into_bdt_err!(BdtErrorCode::Failed))?;
        let local = Endpoint::from((Protocol::Tcp, local));

        let tls_connector = TlsConnector::from(Arc::new(client_config));
        let tls_stream = tls_connector.connect(remote_identity_id.to_string().try_into().unwrap(), socket).await.map_err(into_bdt_err!(BdtErrorCode::ConnectFailed, "tls socket to {} connect failed", remote_ep))?;
        let socket = TCPSocket::new(runtime::TlsStream::from(tls_stream), local_identity_ref.get_id(), remote_identity_id, local, remote_ep);
        debug!("{} connected", socket);
        Ok(socket)
    }

    pub fn remote_identity_id(&self) -> &P2pId {
        &self.remote_identity_id
    }

    pub fn local_identity_id(&self) -> &P2pId {
        &self.local_identity_id
    }

    pub fn remote(&self) -> &Endpoint {
        &self.remote
    }

    pub fn local(&self) -> &Endpoint {
        &self.local
    }

    pub fn split(&self) -> BdtResult<(runtime::ReadHalf<runtime::TlsStream<TcpStream>>, runtime::WriteHalf<runtime::TlsStream<TcpStream>>)> {
        let mut socket = self.socket.lock().unwrap();
        if socket.is_none() {
            return Err(bdt_err!(BdtErrorCode::IoError, "socket is none"));
        }
        Ok(runtime::split(socket.take().unwrap()))
    }

    pub fn is_split(&self) -> bool {
        let socket = self.socket.lock().unwrap();
        socket.is_none()
    }

    pub fn unsplit(&self, read: runtime::ReadHalf<runtime::TlsStream<TcpStream>>, write: runtime::WriteHalf<runtime::TlsStream<TcpStream>>) {
        let mut socket = self.socket.lock().unwrap();
        *socket = Some(read.unsplit(write));
    }
}

pub type TCPRead = runtime::ReadHalf<runtime::TlsStream<TcpStream>>;
pub type TCPWrite = runtime::WriteHalf<runtime::TlsStream<TcpStream>>;
