use crate::endpoint::{Endpoint, Protocol};
use crate::error::{P2pErrorCode, P2pResult, into_p2p_err, p2p_err};
use crate::executor::Executor;
use crate::p2p_connection::{P2pRead, P2pWrite};
use crate::p2p_identity::{P2pId, P2pIdentityCertFactoryRef, P2pIdentityRef};
use crate::runtime;
use crate::sockets::{QuicCongestionAlgorithm, validate_server_name};
use as_any::{AsAny, Downcast};
use quinn::VarInt;
use quinn::crypto::rustls::QuicClientConfig;
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer, ServerName};
use rustls::version::TLS13;
use std::any::Any;
use std::io;
use std::io::Error;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;

pub struct QuicConnection {
    socket: quinn::Connection,
    local: Endpoint,
    remote: Endpoint,
}

impl QuicConnection {
    pub fn new(socket: quinn::Connection, local: Endpoint, remote: Endpoint) -> Self {
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

    pub async fn connect(
        local_identity_ref: P2pIdentityRef,
        cert_factory: P2pIdentityCertFactoryRef,
        remote_identity_id: P2pId,
        remote_name: Option<String>,
        remote: Endpoint,
        congestion_algorithm: QuicCongestionAlgorithm,
        timeout: Duration,
        idle_timeout: Duration,
    ) -> P2pResult<Self> {
        log::info!("quic to {} connect begin", remote);
        let client_key = local_identity_ref.get_encoded_identity()?;
        let client_cert = local_identity_ref.get_identity_cert()?.get_encoded_cert()?;
        let mut config = rustls::ClientConfig::builder_with_provider(crate::tls::provider().into())
            .with_protocol_versions(&[&TLS13])
            .unwrap()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(crate::tls::TlsServerCertVerifier::new(
                cert_factory,
                remote_identity_id.clone(),
            )))
            .with_client_auth_cert(
                vec![CertificateDer::from(client_cert)],
                PrivatePkcs8KeyDer::from(client_key).into(),
            )
            .map_err(into_p2p_err!(P2pErrorCode::TlsError))?;
        config.enable_early_data = true;

        let mut client_config =
            quinn::ClientConfig::new(Arc::new(QuicClientConfig::try_from(config).unwrap()));
        let mut transport_config = quinn::TransportConfig::default();
        transport_config.max_idle_timeout(Some(idle_timeout.try_into().unwrap()));
        if idle_timeout > Duration::from_secs(15) {
            transport_config.keep_alive_interval(Some(Duration::from_secs(15)));
        }
        match congestion_algorithm {
            QuicCongestionAlgorithm::Bbr => {
                transport_config.congestion_controller_factory(Arc::new(
                    quinn::congestion::BbrConfig::default(),
                ));
            }
            QuicCongestionAlgorithm::Cubic => {
                transport_config.congestion_controller_factory(Arc::new(
                    quinn::congestion::CubicConfig::default(),
                ));
            }
            QuicCongestionAlgorithm::NewReno => {
                transport_config.congestion_controller_factory(Arc::new(
                    quinn::congestion::NewRenoConfig::default(),
                ));
            }
        }
        // transport_config.max_concurrent_bidi_streams(1_u8.into());
        // transport_config.max_concurrent_uni_streams(1_u8.into());
        client_config.transport_config(Arc::new(transport_config));
        let mut endpoint = quinn::Endpoint::client("0.0.0.0:0".parse().unwrap()).unwrap();
        endpoint.set_default_client_config(client_config);

        let remote_name = remote_name.unwrap_or(remote_identity_id.to_string());
        let remote_name = validate_server_name(remote_name);
        let conn = runtime::timeout(
            timeout,
            endpoint
                .connect(remote.addr().clone(), remote_name.as_str())
                .unwrap(),
        )
        .await
        .map_err(into_p2p_err!(
            P2pErrorCode::ConnectFailed,
            "quic to {} connect failed",
            remote
        ))?
        .map_err(into_p2p_err!(
            P2pErrorCode::ConnectFailed,
            "quic to {} connect failed",
            remote
        ))?;
        Ok(Self::new(
            conn,
            Endpoint::from((
                Protocol::Quic,
                endpoint
                    .local_addr()
                    .map_err(into_p2p_err!(P2pErrorCode::TlsError))?,
            )),
            remote,
        ))
    }

    pub async fn connect_with_ep(
        ep: quinn::Endpoint,
        local_identity_ref: P2pIdentityRef,
        cert_factory: P2pIdentityCertFactoryRef,
        remote_identity_id: P2pId,
        remote_name: Option<String>,
        remote: Endpoint,
        congestion_algorithm: QuicCongestionAlgorithm,
        timeout: Duration,
        idle_timeout: Duration,
    ) -> P2pResult<Self> {
        log::info!("connect with ep remote = {}", remote);
        let client_key = local_identity_ref.get_encoded_identity()?;
        let client_cert = local_identity_ref.get_identity_cert()?.get_encoded_cert()?;
        let mut config = rustls::ClientConfig::builder_with_provider(crate::tls::provider().into())
            .with_protocol_versions(&[&TLS13])
            .unwrap()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(crate::tls::TlsServerCertVerifier::new(
                cert_factory,
                remote_identity_id.clone(),
            )))
            .with_client_auth_cert(
                vec![CertificateDer::from(client_cert)],
                PrivatePkcs8KeyDer::from(client_key).into(),
            )
            .map_err(into_p2p_err!(P2pErrorCode::TlsError))?;
        config.enable_early_data = true;

        let mut client_config =
            quinn::ClientConfig::new(Arc::new(QuicClientConfig::try_from(config).unwrap()));
        let mut transport_config = quinn::TransportConfig::default();
        transport_config.max_idle_timeout(Some(idle_timeout.try_into().unwrap()));
        if idle_timeout > Duration::from_secs(15) {
            transport_config.keep_alive_interval(Some(Duration::from_secs(15)));
        }
        match congestion_algorithm {
            QuicCongestionAlgorithm::Bbr => {
                transport_config.congestion_controller_factory(Arc::new(
                    quinn::congestion::BbrConfig::default(),
                ));
            }
            QuicCongestionAlgorithm::Cubic => {
                transport_config.congestion_controller_factory(Arc::new(
                    quinn::congestion::CubicConfig::default(),
                ));
            }
            QuicCongestionAlgorithm::NewReno => {
                transport_config.congestion_controller_factory(Arc::new(
                    quinn::congestion::NewRenoConfig::default(),
                ));
            }
        }
        client_config.transport_config(Arc::new(transport_config));

        let remote_name = remote_name.unwrap_or(remote_identity_id.to_string());
        let remote_name = validate_server_name(remote_name);
        let conn = runtime::timeout(
            timeout,
            ep.connect_with(client_config, remote.addr().clone(), remote_name.as_str())
                .unwrap(),
        )
        .await
        .map_err(into_p2p_err!(
            P2pErrorCode::ConnectFailed,
            "quic to {} connect failed",
            remote
        ))?
        .map_err(into_p2p_err!(
            P2pErrorCode::ConnectFailed,
            "quic to {} connect failed",
            remote
        ))?;
        Ok(Self::new(
            conn,
            Endpoint::from((
                Protocol::Quic,
                ep.local_addr()
                    .map_err(into_p2p_err!(P2pErrorCode::TlsError))?,
            )),
            remote,
        ))
    }

    pub fn socket(&self) -> &quinn::Connection {
        &self.socket
    }

    pub async fn open_bi_stream(&mut self) -> P2pResult<(quinn::RecvStream, quinn::SendStream)> {
        let (send, recv) = self.socket.open_bi().await.map_err(into_p2p_err!(
            P2pErrorCode::ConnectFailed,
            "quic to {} open bi failed",
            self.remote
        ))?;
        Ok((recv, send))
    }

    pub async fn open_ui(&mut self) -> P2pResult<quinn::SendStream> {
        let send = self.socket.open_uni().await.map_err(into_p2p_err!(
            P2pErrorCode::ConnectFailed,
            "quic to {} open uni failed",
            self.remote
        ))?;
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
    fn poll_read(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<Result<(), io::Error>> {
        Poll::Ready(Ok(()))
    }
}

pub(crate) struct MockWrite;

impl runtime::AsyncWrite for MockWrite {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
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
    remote_name: String,
}

impl QuicRead {
    pub fn new(
        socket: quinn::Connection,
        recv: quinn::RecvStream,
        remote_identity_id: P2pId,
        local_identity_id: P2pId,
        local: Endpoint,
        remote: Endpoint,
        remote_name: String,
    ) -> Self {
        Self {
            socket,
            recv,
            remote_identity_id,
            local_identity_id,
            local,
            remote,
            remote_name,
        }
    }

    fn stop(&mut self) {
        let _ = self.recv.stop(VarInt::from_u32(0));
    }
}

impl Drop for QuicRead {
    fn drop(&mut self) {
        log::trace!(
            "quic conn {} read stream {} drop",
            self.socket.stable_id(),
            self.recv.id()
        );
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

    fn remote_name(&self) -> String {
        self.remote_name.clone()
    }
}

impl runtime::AsyncRead for QuicRead {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<Result<(), io::Error>> {
        match self.recv.poll_read(cx, buf.initialize_unfilled()) {
            Poll::Ready(ret) => match ret {
                Ok(size) => {
                    buf.advance(size);
                    log::trace!(
                        "quic conn {} stream {} read size {} data {}",
                        self.socket.stable_id(),
                        self.recv.id(),
                        size,
                        hex::encode(buf.filled())
                    );
                    Poll::Ready(Ok(()))
                }
                Err(e) => Poll::Ready(Err(io::Error::new(io::ErrorKind::Other, e))),
            },
            Poll::Pending => Poll::Pending,
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
    remote_name: String,
}

impl QuicWrite {
    pub fn new(
        socket: quinn::Connection,
        send: quinn::SendStream,
        remote_identity_id: P2pId,
        local_identity_id: P2pId,
        local: Endpoint,
        remote: Endpoint,
        remote_name: String,
    ) -> Self {
        Self {
            socket: Some(socket),
            send: Some(send),
            remote_identity_id,
            local_identity_id,
            local,
            remote,
            remote_name,
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

    fn remote_name(&self) -> String {
        self.remote_name.clone()
    }
}

pub(crate) struct QuicUniWrite {
    socket: quinn::Connection,
    remote_identity_id: P2pId,
    local_identity_id: P2pId,
    local: Endpoint,
    remote: Endpoint,
    remote_name: String,
}

impl QuicUniWrite {
    pub fn new(
        socket: quinn::Connection,
        remote_identity_id: P2pId,
        local_identity_id: P2pId,
        local: Endpoint,
        remote: Endpoint,
        remote_name: String,
    ) -> Self {
        Self {
            socket,
            remote_identity_id,
            local_identity_id,
            local,
            remote,
            remote_name,
        }
    }
}

impl P2pWrite for QuicUniWrite {
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

impl runtime::AsyncWrite for QuicUniWrite {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buf: &[u8],
    ) -> Poll<Result<usize, Error>> {
        Poll::Ready(Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "quic uni stream does not support write",
        )))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        Poll::Ready(Ok(()))
    }
}

impl Drop for QuicUniWrite {
    fn drop(&mut self) {
        log::trace!(
            "quic conn {} uni write side drop",
            self.socket.stable_id(),
        );
    }
}

impl Drop for QuicWrite {
    fn drop(&mut self) {
        log::trace!(
            "quic conn {} write stream {} drop",
            self.socket.as_ref().unwrap().stable_id(),
            self.send.as_ref().unwrap().id()
        );
        self.stop();
    }
}

impl runtime::AsyncWrite for QuicWrite {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, Error>> {
        let conn_id = self.socket.as_ref().unwrap().stable_id();
        let send = self.send.as_mut().unwrap();
        let stream_id = send.id();
        match Pin::new(send).poll_write(cx, buf) {
            Poll::Ready(ret) => match ret {
                Ok(size) => {
                    log::trace!(
                        "quic connection {} stream {} send data {}",
                        conn_id,
                        stream_id,
                        hex::encode(buf)
                    );
                    Poll::Ready(Ok(size))
                }
                Err(e) => Poll::Ready(Err(io::Error::new(io::ErrorKind::Other, e))),
            },
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        let send = self.send.as_mut().unwrap();
        match Pin::new(send).poll_flush(cx) {
            Poll::Ready(ret) => match ret {
                Ok(()) => Poll::Ready(Ok(())),
                Err(e) => Poll::Ready(Err(io::Error::new(io::ErrorKind::Other, e))),
            },
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        let send = self.send.as_mut().unwrap();
        match Pin::new(send).poll_shutdown(cx) {
            Poll::Ready(ret) => match ret {
                Ok(()) => Poll::Ready(Ok(())),
                Err(e) => Poll::Ready(Err(io::Error::new(io::ErrorKind::Other, e))),
            },
            Poll::Pending => Poll::Pending,
        }
    }
}

#[cfg(all(test, feature = "x509"))]
mod tests {
    use std::net::SocketAddr;
    use std::sync::{Arc, Mutex, Once};
    use std::time::Duration;

    use crate::endpoint::{Endpoint, Protocol};
    use crate::executor::Executor;
    use crate::finder::{DeviceCache, DeviceCacheConfig};
    use crate::p2p_connection::{P2pConnection, P2pConnectionEventListener};
    use crate::p2p_identity::{
        P2pId, P2pIdentityCertCacheRef, P2pIdentityCertFactoryRef, P2pIdentityRef,
    };
    use crate::runtime::{AsyncReadExt, AsyncWriteExt};
    use crate::sockets::{QuicCongestionAlgorithm, QuicListener};
    use crate::tls::{DefaultTlsServerCertResolver, TlsServerCertResolver};
    use crate::x509::{X509IdentityCertFactory, X509IdentityFactory, generate_x509_identity};

    use super::QuicConnection;

    static TLS_INIT: Once = Once::new();

    struct NoopConnListener;

    struct CaptureConnListener {
        tx: Mutex<Option<tokio::sync::oneshot::Sender<P2pConnection>>>,
    }

    #[async_trait::async_trait]
    impl P2pConnectionEventListener for NoopConnListener {
        async fn on_new_connection(&self, _conn: P2pConnection) -> crate::error::P2pResult<()> {
            Ok(())
        }
    }

    #[async_trait::async_trait]
    impl P2pConnectionEventListener for CaptureConnListener {
        async fn on_new_connection(&self, conn: P2pConnection) -> crate::error::P2pResult<()> {
            if let Some(tx) = self.tx.lock().unwrap().take() {
                let _ = tx.send(conn);
            }
            Ok(())
        }
    }

    fn init_tls_once() {
        TLS_INIT.call_once(|| {
            Executor::init();
            crate::tls::init_tls(Arc::new(X509IdentityFactory));
        });
    }

    fn new_cert_cache() -> P2pIdentityCertCacheRef {
        Arc::new(DeviceCache::new(
            &DeviceCacheConfig {
                expire: Duration::from_secs(60),
                capacity: 32,
            },
            None,
        ))
    }

    fn new_identity() -> P2pIdentityRef {
        Arc::new(generate_x509_identity(None).unwrap())
    }

    fn new_cert_factory() -> P2pIdentityCertFactoryRef {
        Arc::new(X509IdentityCertFactory)
    }

    fn loopback_ep(port: u16) -> Endpoint {
        let addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
        Endpoint::from((Protocol::Quic, addr))
    }

    async fn setup_server_listener() -> (
        Arc<crate::tls::DefaultTlsServerCertResolver>,
        P2pIdentityRef,
        Arc<QuicListener>,
        Endpoint,
    ) {
        init_tls_once();
        let cert_factory = new_cert_factory();
        let cert_cache = new_cert_cache();

        let server_identity = new_identity();
        let resolver = DefaultTlsServerCertResolver::new();
        resolver
            .add_server_identity(server_identity.clone())
            .await
            .unwrap();

        let listener = QuicListener::new(
            cert_cache,
            resolver.clone(),
            cert_factory,
            QuicCongestionAlgorithm::Bbr,
        );
        listener.bind(loopback_ep(0), None, None).await.unwrap();
        listener.set_connection_event_listener(Arc::new(NoopConnListener));
        listener.start();

        let local_addr = listener.quic_ep().local_addr().unwrap();
        (
            resolver,
            server_identity,
            listener,
            Endpoint::from((Protocol::Quic, local_addr)),
        )
    }

    async fn setup_server_listener_no_start() -> (
        Arc<crate::tls::DefaultTlsServerCertResolver>,
        P2pIdentityRef,
        Arc<QuicListener>,
        Endpoint,
    ) {
        init_tls_once();
        let cert_factory = new_cert_factory();
        let cert_cache = new_cert_cache();

        let server_identity = new_identity();
        let resolver = DefaultTlsServerCertResolver::new();
        resolver
            .add_server_identity(server_identity.clone())
            .await
            .unwrap();

        let listener = QuicListener::new(
            cert_cache,
            resolver.clone(),
            cert_factory,
            QuicCongestionAlgorithm::Bbr,
        );
        listener.bind(loopback_ep(0), None, None).await.unwrap();

        let local_addr = listener.quic_ep().local_addr().unwrap();
        (
            resolver,
            server_identity,
            listener,
            Endpoint::from((Protocol::Quic, local_addr)),
        )
    }

    #[tokio::test]
    async fn quic_connection_connect_with_ep_and_open_stream_ok() {
        let (_resolver, server_identity, _listener, remote_ep) = setup_server_listener().await;

        let client_identity = new_identity();
        let cert_factory = new_cert_factory();
        let client_ep = quinn::Endpoint::client("0.0.0.0:0".parse().unwrap()).unwrap();
        let mut conn = QuicConnection::connect_with_ep(
            client_ep,
            client_identity,
            cert_factory,
            server_identity.get_id(),
            Some(server_identity.get_name()),
            remote_ep,
            QuicCongestionAlgorithm::Bbr,
            Duration::from_secs(3),
            Duration::from_secs(10),
        )
        .await
        .unwrap();

        assert_eq!(conn.remote(), remote_ep);
        let ret = conn.open_bi_stream().await;
        assert!(ret.is_ok());
    }

    #[tokio::test]
    async fn quic_connection_connect_without_ep_ok() {
        let (_resolver, server_identity, _listener, remote_ep) = setup_server_listener().await;
        let client_identity = new_identity();
        let cert_factory = new_cert_factory();

        let mut conn = QuicConnection::connect(
            client_identity,
            cert_factory,
            server_identity.get_id(),
            Some(server_identity.get_name()),
            remote_ep,
            QuicCongestionAlgorithm::Bbr,
            Duration::from_secs(3),
            Duration::from_secs(20),
        )
        .await
        .unwrap();

        assert!(conn.open_bi_stream().await.is_ok());
    }

    #[tokio::test]
    async fn quic_connection_open_uni_ok() {
        let (_resolver, server_identity, _listener, remote_ep) = setup_server_listener().await;

        let client_identity = new_identity();
        let cert_factory = new_cert_factory();
        let client_ep = quinn::Endpoint::client("0.0.0.0:0".parse().unwrap()).unwrap();
        let mut conn = QuicConnection::connect_with_ep(
            client_ep,
            client_identity,
            cert_factory,
            server_identity.get_id(),
            Some(server_identity.get_name()),
            remote_ep,
            QuicCongestionAlgorithm::Bbr,
            Duration::from_secs(3),
            Duration::from_secs(10),
        )
        .await
        .unwrap();

        let ret = conn.open_ui().await;
        assert!(ret.is_ok());
    }

    #[tokio::test]
    async fn quic_connection_connect_with_ep_wrong_remote_id_should_fail() {
        let (_resolver, server_identity, _listener, remote_ep) = setup_server_listener().await;

        let wrong_remote_id = {
            let id = new_identity();
            assert_ne!(id.get_id(), server_identity.get_id());
            id.get_id()
        };

        let client_identity = new_identity();
        let cert_factory = new_cert_factory();
        let client_ep = quinn::Endpoint::client("0.0.0.0:0".parse().unwrap()).unwrap();
        let ret = QuicConnection::connect_with_ep(
            client_ep,
            client_identity,
            cert_factory,
            wrong_remote_id,
            Some(server_identity.get_name()),
            remote_ep,
            QuicCongestionAlgorithm::Bbr,
            Duration::from_secs(3),
            Duration::from_secs(10),
        )
        .await;

        assert!(ret.is_err());
    }

    #[tokio::test]
    async fn quic_connection_connect_with_ep_unreachable_should_fail_fast() {
        init_tls_once();

        let client_identity = new_identity();
        let cert_factory = new_cert_factory();
        let client_ep = quinn::Endpoint::client("0.0.0.0:0".parse().unwrap()).unwrap();

        let unused_addr = {
            let sock = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
            sock.local_addr().unwrap()
        };

        let ret = QuicConnection::connect_with_ep(
            client_ep,
            client_identity,
            cert_factory,
            P2pId::default(),
            None,
            Endpoint::from((Protocol::Quic, unused_addr)),
            QuicCongestionAlgorithm::Bbr,
            Duration::from_millis(500),
            Duration::from_secs(2),
        )
        .await;

        assert!(ret.is_err());
    }

    #[tokio::test]
    async fn quic_connection_connect_with_ep_cubic_and_newreno_ok() {
        let (_resolver, server_identity, _listener, remote_ep) = setup_server_listener().await;

        let client_identity = new_identity();
        let cert_factory = new_cert_factory();
        let client_ep = quinn::Endpoint::client("0.0.0.0:0".parse().unwrap()).unwrap();
        let mut conn = QuicConnection::connect_with_ep(
            client_ep,
            client_identity,
            cert_factory,
            server_identity.get_id(),
            Some(server_identity.get_name()),
            remote_ep,
            QuicCongestionAlgorithm::Cubic,
            Duration::from_secs(3),
            Duration::from_secs(10),
        )
        .await
        .unwrap();
        assert!(conn.open_bi_stream().await.is_ok());

        let (_resolver2, server_identity2, _listener2, remote_ep2) = setup_server_listener().await;
        let client_identity2 = new_identity();
        let cert_factory2 = new_cert_factory();
        let client_ep2 = quinn::Endpoint::client("0.0.0.0:0".parse().unwrap()).unwrap();
        let mut conn2 = QuicConnection::connect_with_ep(
            client_ep2,
            client_identity2,
            cert_factory2,
            server_identity2.get_id(),
            Some(server_identity2.get_name()),
            remote_ep2,
            QuicCongestionAlgorithm::NewReno,
            Duration::from_secs(3),
            Duration::from_secs(10),
        )
        .await
        .unwrap();
        assert!(conn2.open_bi_stream().await.is_ok());
    }

    #[tokio::test]
    async fn quic_connection_accept_bi_and_uni_dispatch_ok() {
        let (_resolver, server_identity, listener, remote_ep) =
            setup_server_listener_no_start().await;
        let (tx, rx) = tokio::sync::oneshot::channel();
        listener.set_connection_event_listener(Arc::new(CaptureConnListener {
            tx: Mutex::new(Some(tx)),
        }));
        listener.start();

        let client_identity = new_identity();
        let cert_factory = new_cert_factory();
        let client_ep = quinn::Endpoint::client("0.0.0.0:0".parse().unwrap()).unwrap();
        let mut client_conn = QuicConnection::connect_with_ep(
            client_ep,
            client_identity,
            cert_factory,
            server_identity.get_id(),
            Some(server_identity.get_name()),
            remote_ep,
            QuicCongestionAlgorithm::Bbr,
            Duration::from_secs(3),
            Duration::from_secs(10),
        )
        .await
        .unwrap();

        let _ = client_conn.open_bi_stream().await.unwrap();
        let conn = tokio::time::timeout(Duration::from_secs(2), rx)
            .await
            .unwrap()
            .unwrap();
        drop(conn);

        let (_resolver2, server_identity2, listener2, remote_ep2) =
            setup_server_listener_no_start().await;
        let (tx2, rx2) = tokio::sync::oneshot::channel();
        listener2.set_connection_event_listener(Arc::new(CaptureConnListener {
            tx: Mutex::new(Some(tx2)),
        }));
        listener2.start();
        let client_identity2 = new_identity();
        let cert_factory2 = new_cert_factory();
        let client_ep2 = quinn::Endpoint::client("0.0.0.0:0".parse().unwrap()).unwrap();
        let mut client_conn2 = QuicConnection::connect_with_ep(
            client_ep2,
            client_identity2,
            cert_factory2,
            server_identity2.get_id(),
            Some(server_identity2.get_name()),
            remote_ep2,
            QuicCongestionAlgorithm::Bbr,
            Duration::from_secs(3),
            Duration::from_secs(10),
        )
        .await
        .unwrap();
        let _ = client_conn2.open_ui().await.unwrap();
        let uni_ret = tokio::time::timeout(Duration::from_millis(300), rx2).await;
        assert!(uni_ret.is_ok());
    }

    #[tokio::test]
    async fn quic_connection_read_write_wrappers_io_ok() {
        let (_resolver, server_identity, listener, remote_ep) =
            setup_server_listener_no_start().await;
        let (tx, rx) = tokio::sync::oneshot::channel();
        listener.set_connection_event_listener(Arc::new(CaptureConnListener {
            tx: Mutex::new(Some(tx)),
        }));
        listener.start();

        let client_identity = new_identity();
        let cert_factory = new_cert_factory();
        let client_ep = quinn::Endpoint::client("0.0.0.0:0".parse().unwrap()).unwrap();
        let mut client_conn = QuicConnection::connect_with_ep(
            client_ep,
            client_identity,
            cert_factory,
            server_identity.get_id(),
            Some(server_identity.get_name()),
            remote_ep,
            QuicCongestionAlgorithm::Bbr,
            Duration::from_secs(3),
            Duration::from_secs(10),
        )
        .await
        .unwrap();

        let (mut client_recv, mut client_send) = client_conn.open_bi_stream().await.unwrap();
        client_send.write_all(b"hello").await.unwrap();
        client_send.flush().await.unwrap();

        let server_conn = tokio::time::timeout(Duration::from_secs(2), rx)
            .await
            .unwrap()
            .unwrap();
        let (mut server_read, mut server_write) = server_conn.split();
        let mut read_buf = [0u8; 5];
        server_read.read_exact(&mut read_buf).await.unwrap();
        assert_eq!(&read_buf, b"hello");

        server_write.write_all(b"world").await.unwrap();
        server_write.flush().await.unwrap();
        let mut client_buf = [0u8; 5];
        client_recv.read_exact(&mut client_buf).await.unwrap();
        assert_eq!(&client_buf, b"world");

        server_write.shutdown().await.unwrap();
        client_send.shutdown().await.unwrap();
    }

    #[test]
    fn mock_read_write_poll_basic() {
        use crate::runtime::{AsyncRead, AsyncWrite};
        use std::pin::Pin;
        use std::task::{Context, Poll, Waker};
        use tokio::io::ReadBuf;

        let waker = Waker::noop();
        let mut cx = Context::from_waker(waker);

        let mut mock_read = super::MockRead;
        let mut bytes = [0u8; 8];
        let mut read_buf = ReadBuf::new(&mut bytes);
        let r = Pin::new(&mut mock_read).poll_read(&mut cx, &mut read_buf);
        assert!(matches!(r, Poll::Ready(Ok(()))));

        let mut mock_write = super::MockWrite;
        let w = Pin::new(&mut mock_write).poll_write(&mut cx, b"abc");
        assert!(matches!(w, Poll::Ready(Ok(0))));
        let f = Pin::new(&mut mock_write).poll_flush(&mut cx);
        assert!(matches!(f, Poll::Ready(Ok(()))));
        let s = Pin::new(&mut mock_write).poll_shutdown(&mut cx);
        assert!(matches!(s, Poll::Ready(Ok(()))));
    }
}
