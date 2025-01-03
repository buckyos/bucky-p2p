use std::io;
use std::io::Error;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;
use as_any::Downcast;
use quinn::crypto::rustls::QuicClientConfig;
use quinn::{VarInt};
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
use rustls::version::TLS13;
use crate::endpoint::{Endpoint, Protocol};
use crate::error::{P2pErrorCode, P2pResult, into_p2p_err};
use crate::executor::Executor;
use crate::p2p_connection::{P2pConnection};
use crate::p2p_identity::{P2pId, P2pIdentityRef, P2pIdentityCertFactoryRef};
use crate::runtime;

pub struct QuicConnection {
    socket: quinn::Connection,
    remote_identity_id: P2pId,
    local_identity_id: P2pId,
    local: Endpoint,
    remote: Endpoint,
    send: Mutex<Option<quinn::SendStream>>,
    recv: Mutex<Option<quinn::RecvStream>>,
    is_stream: bool,
}

impl QuicConnection {
    pub fn new(
        socket: quinn::Connection,
        local_identity_id: P2pId,
        remote_identity_id: P2pId,
        local: Endpoint,
        remote: Endpoint,
    ) -> Self {
        Self {
            socket,
            local_identity_id,
            remote_identity_id,
            local,
            remote,
            send: Mutex::new(None),
            recv: Mutex::new(None),
            is_stream: false,
        }
    }

    pub async fn connect(local_identity_ref: P2pIdentityRef,
                         cert_factory: P2pIdentityCertFactoryRef,
                         remote_identity_id: P2pId,
                         remote: Endpoint,
                         timeout: Duration,
                         idle_timeout: Duration) -> P2pResult<Self> {
        log::info!("quic to {} connect begin", remote);
        let client_key = local_identity_ref.get_encoded_identity()?;
        let client_cert = local_identity_ref.get_identity_cert()?.get_encoded_cert()?;
        let mut config =
            rustls::ClientConfig::builder_with_provider(crate::tls::provider().into())
                .with_protocol_versions(&[&TLS13])
                .unwrap()
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(crate::tls::TlsServerCertVerifier::new(cert_factory)))
                .with_client_auth_cert(vec![CertificateDer::from(client_cert)], PrivatePkcs8KeyDer::from(client_key).into())
                .map_err(into_p2p_err!(P2pErrorCode::TlsError))?;
        config.enable_early_data = true;

        let mut client_config =
            quinn::ClientConfig::new(Arc::new(QuicClientConfig::try_from(config).unwrap()));
        let mut transport_config = quinn::TransportConfig::default();
        transport_config.max_idle_timeout(Some(idle_timeout.try_into().unwrap()));
        if idle_timeout > Duration::from_secs(30) {
            transport_config.keep_alive_interval(Some(Duration::from_secs(30)));
        }
        // transport_config.max_concurrent_bidi_streams(1_u8.into());
        // transport_config.max_concurrent_uni_streams(1_u8.into());
        client_config.transport_config(Arc::new(transport_config));
        let mut endpoint = quinn::Endpoint::client("0.0.0.0:0".parse().unwrap()).unwrap();
        endpoint.set_default_client_config(client_config);

        let conn = runtime::timeout(timeout, endpoint.connect(remote.addr().clone(),
                                    remote_identity_id.to_string().as_str()).unwrap()).await
            .map_err(into_p2p_err!(P2pErrorCode::ConnectFailed, "quic to {} connect failed", remote))?
            .map_err(into_p2p_err!(P2pErrorCode::ConnectFailed, "quic to {} connect failed", remote))?;
        Ok(Self::new(conn,
                     local_identity_ref.get_id(),
                     remote_identity_id,
                     Endpoint::from((Protocol::Quic, endpoint.local_addr().map_err(into_p2p_err!(P2pErrorCode::TlsError))?)),
                     remote))
    }

    pub async fn connect_with_ep(ep: quinn::Endpoint,
                                 local_identity_ref: P2pIdentityRef,
                                 cert_factory: P2pIdentityCertFactoryRef,
                                 remote_identity_id: P2pId,
                                 remote: Endpoint,
                                 timeout: Duration,
                                 idle_timeout: Duration) -> P2pResult<Self> {
        log::info!("connect with ep remote = {}", remote);
        let client_key = local_identity_ref.get_encoded_identity()?;
        let client_cert = local_identity_ref.get_identity_cert()?.get_encoded_cert()?;
        let mut config =
            rustls::ClientConfig::builder_with_provider(crate::tls::provider().into())
                .with_protocol_versions(&[&TLS13])
                .unwrap()
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(crate::tls::TlsServerCertVerifier::new(cert_factory)))
                .with_client_auth_cert(vec![CertificateDer::from(client_cert)], PrivatePkcs8KeyDer::from(client_key).into())
                .map_err(into_p2p_err!(P2pErrorCode::TlsError))?;
        config.enable_early_data = true;

        let mut client_config =
            quinn::ClientConfig::new(Arc::new(QuicClientConfig::try_from(config).unwrap()));
        let mut transport_config = quinn::TransportConfig::default();
        transport_config.max_idle_timeout(Some(idle_timeout.try_into().unwrap()));
        if idle_timeout > Duration::from_secs(30) {
            transport_config.keep_alive_interval(Some(Duration::from_secs(30)));
        }
        client_config.transport_config(Arc::new(transport_config));

        let conn = runtime::timeout(timeout, ep.connect_with(client_config,
                                   remote.addr().clone(),
                                   remote_identity_id.to_string().as_str()).unwrap()).await
            .map_err(into_p2p_err!(P2pErrorCode::ConnectFailed, "quic to {} connect failed", remote))?
            .map_err(into_p2p_err!(P2pErrorCode::ConnectFailed, "quic to {} connect failed", remote))?;
        Ok(Self::new(conn,
                     local_identity_ref.get_id(),
                     remote_identity_id,
                     Endpoint::from((Protocol::Quic, ep.local_addr().map_err(into_p2p_err!(P2pErrorCode::TlsError))?)),
                     remote))
    }

    pub fn socket(&self) -> &quinn::Connection {
        &self.socket
    }

    pub async fn shutdown(&self) -> P2pResult<()> {
        let mut send = self.send.lock().unwrap();
        if let Some(send) = send.as_mut() {
            send.finish()
                .map_err(into_p2p_err!(P2pErrorCode::IoError, "quic to {} shutdown failed", self.remote))?;
            send.stopped().await.map_err(into_p2p_err!(P2pErrorCode::IoError, "quic to {} shutdown failed", self.remote))?;
        }
        let mut recv = self.recv.lock().unwrap();
        if let Some(recv) = recv.as_mut() {
            recv.stop(VarInt::from_u32(0)).map_err(into_p2p_err!(P2pErrorCode::IoError, "quic to {} shutdown failed", self.remote))?;
        }
        self.socket.close(VarInt::from_u32(0), &[]);
        Ok(())
    }

    pub async fn open_bi_stream(&mut self) -> P2pResult<()> {
        let (send, recv) = self.socket.open_bi().await
            .map_err(into_p2p_err!(P2pErrorCode::ConnectFailed, "quic to {} open bi failed", self.remote))?;
        *self.send.lock().unwrap() = Some(send);
        *self.recv.lock().unwrap() = Some(recv);
        self.is_stream = true;
        Ok(())
    }

    pub async fn open_ui(&mut self) -> P2pResult<()> {
        let send = self.socket.open_uni().await
            .map_err(into_p2p_err!(P2pErrorCode::ConnectFailed, "quic to {} open uni failed", self.remote))?;
        *self.send.lock().unwrap() = Some(send);
        self.is_stream = false;
        Ok(())
    }
}

impl Drop for QuicConnection {
    fn drop(&mut self) {
        log::info!("quic to {} drop", self.remote);
        let _ = Executor::block_on(self.shutdown());
    }
}
struct MockRead;

impl runtime::AsyncRead for MockRead {
    fn poll_read(self: Pin<&mut Self>, _cx: &mut Context<'_>, _buf: &mut tokio::io::ReadBuf<'_>) -> Poll<Result<(), io::Error>> {
        Poll::Ready(Ok(()))
    }
}

struct QuicRead {
    recv: Option<quinn::RecvStream>,
}

impl QuicRead {
    pub fn new(recv: quinn::RecvStream) -> Self {
        Self {
            recv: Some(recv),
        }
    }

    pub fn take(&mut self) -> quinn::RecvStream {
        self.recv.take().unwrap()
    }

    fn stop(&mut self) {
        if let Some(mut recv) = self.recv.take() {
            let _ = recv.stop(VarInt::from_u32(0));
        }
    }
}

impl Drop for QuicRead {
    fn drop(&mut self) {
        self.stop();
    }
}

impl runtime::AsyncRead for QuicRead {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut tokio::io::ReadBuf<'_>) -> Poll<Result<(), io::Error>> {
        let recv = self.recv.as_mut().unwrap();
        match recv.poll_read(cx, buf.initialized_mut()) {
            Poll::Ready(ret) => {
                match ret {
                    Ok(size) => {
                        buf.set_filled(size);
                        Poll::Ready(Ok(()))
                    },
                    Err(e) => {
                        Poll::Ready(Err(io::Error::new(io::ErrorKind::Other, e)))
                    }
                }
            }
            Poll::Pending => {
                Poll::Pending
            }
        }
    }
}

struct QuicWrite {
    send: Option<quinn::SendStream>,
}

impl QuicWrite {
    pub fn new(send: quinn::SendStream) -> Self {
        Self {
            send: Some(send),
        }
    }

    pub fn take(&mut self) -> quinn::SendStream {
        self.send.take().unwrap()
    }

    async fn stop(&mut self) {
        if let Some(mut send) = self.send.take() {
            let _ = send.finish();
            let _ = send.stopped().await;
        }
    }
}

impl Drop for QuicWrite {
    fn drop(&mut self) {
        Executor::block_on(self.stop());
    }
}

impl runtime::AsyncWrite for QuicWrite {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<Result<usize, Error>> {
        let send = self.send.as_mut().unwrap();
        match Pin::new(send).poll_write(cx, buf) {
            Poll::Ready(ret) => {
                match ret {
                    Ok(size) => {
                        Poll::Ready(Ok(size))
                    },
                    Err(e) => {
                        Poll::Ready(Err(io::Error::new(io::ErrorKind::Other, e)))
                    }
                }
            }
            Poll::Pending => {
                Poll::Pending
            }
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        let send = self.send.as_mut().unwrap();
        match Pin::new(send).poll_flush(cx) {
            Poll::Ready(ret) => {
                match ret {
                    Ok(()) => {
                        Poll::Ready(Ok(()))
                    },
                    Err(e) => {
                        Poll::Ready(Err(io::Error::new(io::ErrorKind::Other, e)))
                    }
                }
            }
            Poll::Pending => {
                Poll::Pending
            }
        }
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        let send = self.send.as_mut().unwrap();
        match Pin::new(send).poll_shutdown(cx) {
            Poll::Ready(ret) => {
                match ret {
                    Ok(()) => {
                        Poll::Ready(Ok(()))
                    },
                    Err(e) => {
                        Poll::Ready(Err(io::Error::new(io::ErrorKind::Other, e)))
                    }
                }
            }
            Poll::Pending => {
                Poll::Pending
            }
        }
    }
}

impl P2pConnection for QuicConnection {
    fn is_stream(&self) -> bool {
        self.is_stream
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

    fn split(&self) -> P2pResult<(Box<dyn runtime::AsyncRead + Send + Sync + 'static + Unpin>, Box<dyn runtime::AsyncWrite + Send + Sync + 'static + Unpin>)> {
        if self.is_stream {
            Ok((Box::new(QuicRead::new(self.recv.lock().unwrap().take().unwrap())), Box::new(QuicWrite::new(self.send.lock().unwrap().take().unwrap()))))
        } else {
            Ok((Box::new(MockRead), Box::new(self.send.lock().unwrap().take().unwrap())))
        }
    }

    fn unsplit(&self, mut read: Box<dyn runtime::AsyncRead + Send + Sync + 'static + Unpin>, mut write: Box<dyn runtime::AsyncWrite + Send + Sync + 'static + Unpin>) {
        if self.is_stream {
            if let Some(read) = read.downcast_mut::<QuicRead>() {
                *self.recv.lock().unwrap() = Some(read.take());
            }
            if let Some(write) = write.downcast_mut::<QuicWrite>() {
                *self.send.lock().unwrap() = Some(write.take());
            }
        } else {
            if let Some(write) = write.downcast_mut::<QuicWrite>() {
                *self.send.lock().unwrap() = Some(write.take());
            }
        }
    }
}
