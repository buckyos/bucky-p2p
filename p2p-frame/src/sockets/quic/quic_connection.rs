use std::any::Any;
use std::io;
use std::io::Error;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;
use as_any::{AsAny, Downcast};
use quinn::crypto::rustls::QuicClientConfig;
use quinn::{VarInt};
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
use rustls::version::TLS13;
use crate::endpoint::{Endpoint, Protocol};
use crate::error::{P2pErrorCode, P2pResult, into_p2p_err, p2p_err};
use crate::executor::Executor;
use crate::p2p_connection::{P2pRead, P2pWrite};
use crate::p2p_identity::{P2pId, P2pIdentityRef, P2pIdentityCertFactoryRef};
use crate::runtime;

pub struct QuicConnection {
    socket: quinn::Connection,
    local: Endpoint,
    remote: Endpoint,
}

impl QuicConnection {
    pub fn new(
        socket: quinn::Connection,
        local: Endpoint,
        remote: Endpoint,
    ) -> Self {
        Self {
            socket,
            local,
            remote,
        }
    }

    pub fn local(&self) -> Endpoint {
        self.local.clone()
    }

    pub fn remote(&self) -> Endpoint {
        self.remote.clone()
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
                     Endpoint::from((Protocol::Quic, ep.local_addr().map_err(into_p2p_err!(P2pErrorCode::TlsError))?)),
                     remote))
    }

    pub fn socket(&self) -> &quinn::Connection {
        &self.socket
    }

    pub async fn open_bi_stream(&mut self) -> P2pResult<(quinn::RecvStream, quinn::SendStream)> {
        let (send, recv) = self.socket.open_bi().await
            .map_err(into_p2p_err!(P2pErrorCode::ConnectFailed, "quic to {} open bi failed", self.remote))?;
        Ok((recv, send))
    }

    pub async fn open_ui(&mut self) -> P2pResult<quinn::SendStream> {
        let send = self.socket.open_uni().await
            .map_err(into_p2p_err!(P2pErrorCode::ConnectFailed, "quic to {} open uni failed", self.remote))?;
        Ok(send)
    }

    pub async fn accept(&mut self) -> P2pResult<(quinn::RecvStream, Option<quinn::SendStream>)> {
        let (bi_accept, uni_accept) = {
            let bi_accept = self.socket.accept_bi();
            let uni_accept = self.socket.accept_uni();
            (bi_accept, uni_accept)
        };
        runtime::select! {
            ret = bi_accept => {
                match ret {
                    Ok((send, recv)) => {
                        Ok((recv, Some(send)))
                    },
                    Err(e) => {
                        return Err(p2p_err!(P2pErrorCode::IoError, "{:?}", e));
                    }
                }
            },
            ret = uni_accept => {
                match ret {
                    Ok(mut recv) => {
                        Ok((recv, None))
                    },
                    Err(e) => {
                        return Err(p2p_err!(P2pErrorCode::IoError, "{:?}", e));
                    }
                }
            }
        }
    }
}

pub(crate) struct MockRead;

impl runtime::AsyncRead for MockRead {
    fn poll_read(self: Pin<&mut Self>, _cx: &mut Context<'_>, _buf: &mut tokio::io::ReadBuf<'_>) -> Poll<Result<(), io::Error>> {
        Poll::Ready(Ok(()))
    }
}

pub(crate) struct MockWrite;

impl runtime::AsyncWrite for MockWrite {
    fn poll_write(self: Pin<&mut Self>, _cx: &mut Context<'_>, _buf: &[u8]) -> Poll<Result<usize, io::Error>> {
        Poll::Ready(Ok(0))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Poll::Ready(Ok(()))
    }
}

pub(crate) struct QuicRead {
    socket: quinn::Connection,
    recv: quinn::RecvStream,
    remote_identity_id: P2pId,
    local_identity_id: P2pId,
    local: Endpoint,
    remote: Endpoint,
}

impl QuicRead {
    pub fn new(
        socket: quinn::Connection,
        recv: quinn::RecvStream,
        remote_identity_id: P2pId,
        local_identity_id: P2pId,
        local: Endpoint,
        remote: Endpoint, ) -> Self {
        Self {
            socket,
            recv,
            remote_identity_id,
            local_identity_id,
            local,
            remote,
        }
    }

    fn stop(&mut self) {
        let _ = self.recv.stop(VarInt::from_u32(0));
    }
}

impl Drop for QuicRead {
    fn drop(&mut self) {
        self.stop();
    }
}

impl P2pRead for QuicRead {
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
}

impl runtime::AsyncRead for QuicRead {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut tokio::io::ReadBuf<'_>) -> Poll<Result<(), io::Error>> {
        match self.recv.poll_read(cx, buf.initialize_unfilled()) {
            Poll::Ready(ret) => {
                match ret {
                    Ok(size) => {
                        buf.advance(size);
                        log::trace!("quic read size {} data {}", size, hex::encode(buf.filled()));
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

pub(crate) struct QuicWrite {
    socket: Option<quinn::Connection>,
    send: Option<quinn::SendStream>,
    remote_identity_id: P2pId,
    local_identity_id: P2pId,
    local: Endpoint,
    remote: Endpoint,
}

impl QuicWrite {
    pub fn new(
        socket: quinn::Connection,
        send: quinn::SendStream,
        remote_identity_id: P2pId,
        local_identity_id: P2pId,
        local: Endpoint,
        remote: Endpoint, ) -> Self {
        Self {
            socket: Some(socket),
            send: Some(send),
            remote_identity_id,
            local_identity_id,
            local,
            remote,
        }
    }

    fn stop(&mut self) {
        if let Some(mut send) = self.send.take() {
            if let Some(socket) = self.socket.take() {
                Executor::spawn(async move {
                    let _ = send.finish();
                    let _ = send.stopped().await;
                    drop(socket);
                });
            }
        }
    }
}

impl P2pWrite for QuicWrite {
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
}

impl Drop for QuicWrite {
    fn drop(&mut self) {
        self.stop();
    }
}

impl runtime::AsyncWrite for QuicWrite {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<Result<usize, Error>> {
        let send = self.send.as_mut().unwrap();
        match Pin::new(send).poll_write(cx, buf) {
            Poll::Ready(ret) => {
                match ret {
                    Ok(size) => {
                        log::trace!("quic connection send data {}", hex::encode(buf));
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
