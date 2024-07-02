use std::cell::RefCell;
use std::net::Shutdown;
use std::ops::Deref;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use bucky_objects::{DeviceDesc, DeviceId, Endpoint, Protocol};
use bucky_raw_codec::{RawConvertTo, RawDecode, RawDecodeWithContext, RawFixedBytes};
use crate::runtime::{TcpStream, TlsConnector, AsyncWriteExt, AsyncReadExt};
use rustls::ClientConfig;
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
use rustls::version::TLS13;
use crate::error::{bdt_err, BdtError, BdtErrorCode, BdtResult, into_bdt_err};
use crate::{LocalDeviceRef, runtime};
use crate::types::MixAesKey;

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
    remote_device_id: DeviceId,
    local_device_id: DeviceId,
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
    pub fn new(socket: runtime::TlsStream<TcpStream>, local_device_id: DeviceId, remote_device_id: DeviceId, local: Endpoint, remote: Endpoint) -> Self {
        Self {
            socket: Mutex::new(Some(socket)),
            remote_device_id,
            local_device_id,
            local,
            remote,
            write_lock: Mutex::new(0),
        }
    }


    pub async fn connect(
        local_device_ref: LocalDeviceRef,
        remote_ep: Endpoint,
        remote_device_id: DeviceId,
        timeout: Duration,) -> BdtResult<TCPSocket> {
        let client_config =
            rustls::ClientConfig::builder_with_provider(bucky_rustls::provider().into())
                .with_protocol_versions(&[&TLS13])
                .unwrap()
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(bucky_rustls::BuckyServerCertVerifier{}))
                .with_client_auth_cert(vec![CertificateDer::from(local_device_ref.device().to_vec().unwrap())],
                                       PrivatePkcs8KeyDer::from(local_device_ref.key().to_vec().unwrap()).into()).unwrap();

        let socket = runtime::timeout(timeout, TcpStream::connect(remote_ep.addr()))
            .await
            .map_err(into_bdt_err!(BdtErrorCode::ConnectFailed, "tcp socket to {} connect failed", remote_ep))?
            .map_err(into_bdt_err!(BdtErrorCode::ConnectFailed, "tcp socket to {} connect failed", remote_ep))?;

        let local = socket.local_addr().map_err(into_bdt_err!(BdtErrorCode::Failed))?;
        let local = Endpoint::from((Protocol::Tcp, local));

        let tls_connector = TlsConnector::from(Arc::new(client_config));
        let tls_stream = tls_connector.connect(remote_device_id.object_id().to_base36().try_into().unwrap(), socket).await.map_err(into_bdt_err!(BdtErrorCode::ConnectFailed, "tls socket to {} connect failed", remote_ep))?;
        let socket = TCPSocket::new(runtime::TlsStream::from(tls_stream), local_device_ref.device_id().clone(), remote_device_id, local, remote_ep);
        debug!("{} connected", socket);
        Ok(socket)
    }

    pub fn remote_device_id(&self) -> &DeviceId {
        &self.remote_device_id
    }

    pub fn local_device_id(&self) -> &DeviceId {
        &self.local_device_id
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
