use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

use tokio::sync::Notify;

use crate::endpoint::{Endpoint, Protocol};
use crate::error::{P2pErrorCode, P2pResult, p2p_err};
use crate::networks::{
    ListenVPortsRef, Tunnel, TunnelCommandResult, TunnelDatagramRead, TunnelDatagramWrite,
    TunnelForm, TunnelPurpose, TunnelState, TunnelStreamRead, TunnelStreamWrite,
    validate_server_name,
};
use crate::p2p_identity::{P2pId, P2pIdentityCertFactoryRef, P2pIdentityRef};
use crate::pn::{PnChannelKind, ProxyOpenReq, ProxyOpenResp};
use crate::runtime;
use crate::tls::{DefaultTlsServerCertResolver, TlsServerCertResolver};
use crate::types::{TunnelCandidateId, TunnelId};
use rustls::ServerConfig;
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
use rustls::version::TLS13;

use super::pn_client::{PnShared, read_pn_command, write_pn_command};

const FIRST_LISTEN_WAIT_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(300);
const FIRST_LISTEN_WAIT_NOT_STARTED: u8 = 0;
const FIRST_LISTEN_WAIT_IN_PROGRESS: u8 = 1;
const FIRST_LISTEN_WAIT_DONE: u8 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PnProxyStreamSecurityMode {
    Disabled,
    TlsRequired,
}

impl PnProxyStreamSecurityMode {
    pub(super) fn to_atomic(self) -> u8 {
        match self {
            Self::Disabled => 0,
            Self::TlsRequired => 1,
        }
    }

    pub(super) fn from_atomic(value: u8) -> Self {
        match value {
            1 => Self::TlsRequired,
            _ => Self::Disabled,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PnTunnelOptions {
    pub stream_security_mode: PnProxyStreamSecurityMode,
}

impl Default for PnTunnelOptions {
    fn default() -> Self {
        Self {
            stream_security_mode: PnProxyStreamSecurityMode::Disabled,
        }
    }
}

#[derive(Clone)]
pub(super) struct PnTlsContext {
    pub(super) local_identity: P2pIdentityRef,
    pub(super) cert_factory: P2pIdentityCertFactoryRef,
}

struct ProxyTlsIo {
    read: TunnelStreamRead,
    write: TunnelStreamWrite,
}

impl ProxyTlsIo {
    fn new(read: TunnelStreamRead, write: TunnelStreamWrite) -> Self {
        Self { read, write }
    }
}

impl tokio::io::AsyncRead for ProxyTlsIo {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.read).poll_read(cx, buf)
    }
}

impl tokio::io::AsyncWrite for ProxyTlsIo {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.write).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.write).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.write).poll_shutdown(cx)
    }
}

struct PassivePnTunnel {
    request: ProxyOpenReq,
    read: Mutex<Option<TunnelStreamRead>>,
    write: Mutex<Option<TunnelStreamWrite>>,
}

enum PnTunnelRole {
    Active { network: Arc<PnShared> },
    Passive(PassivePnTunnel),
}

pub struct PnTunnel {
    tunnel_id: TunnelId,
    candidate_id: TunnelCandidateId,
    local_id: P2pId,
    remote_id: P2pId,
    role: PnTunnelRole,
    tls_context: Option<PnTlsContext>,
    stream_security_mode: PnProxyStreamSecurityMode,
    stream_vports: RwLock<Option<ListenVPortsRef>>,
    stream_first_listen_wait_state: AtomicU8,
    stream_vports_notify: Notify,
    datagram_vports: RwLock<Option<ListenVPortsRef>>,
    datagram_first_listen_wait_state: AtomicU8,
    datagram_vports_notify: Notify,
    closed: AtomicBool,
}

impl PnTunnel {
    pub(super) fn new_active(
        tunnel_id: TunnelId,
        candidate_id: TunnelCandidateId,
        local_id: P2pId,
        remote_id: P2pId,
        network: Arc<PnShared>,
        stream_security_mode: PnProxyStreamSecurityMode,
    ) -> Arc<Self> {
        Arc::new(Self {
            tunnel_id,
            candidate_id,
            local_id,
            remote_id,
            tls_context: network.tls_context(),
            stream_security_mode,
            role: PnTunnelRole::Active { network },
            stream_vports: RwLock::new(None),
            stream_first_listen_wait_state: AtomicU8::new(FIRST_LISTEN_WAIT_NOT_STARTED),
            stream_vports_notify: Notify::new(),
            datagram_vports: RwLock::new(None),
            datagram_first_listen_wait_state: AtomicU8::new(FIRST_LISTEN_WAIT_NOT_STARTED),
            datagram_vports_notify: Notify::new(),
            closed: AtomicBool::new(false),
        })
    }

    pub(super) fn new_passive(
        local_id: P2pId,
        request: ProxyOpenReq,
        read: TunnelStreamRead,
        write: TunnelStreamWrite,
        tls_context: Option<PnTlsContext>,
        stream_security_mode: PnProxyStreamSecurityMode,
    ) -> Arc<Self> {
        Arc::new(Self {
            tunnel_id: request.tunnel_id,
            candidate_id: TunnelCandidateId::from(request.tunnel_id.value()),
            local_id,
            remote_id: request.from.clone(),
            tls_context,
            stream_security_mode,
            role: PnTunnelRole::Passive(PassivePnTunnel {
                request,
                read: Mutex::new(Some(read)),
                write: Mutex::new(Some(write)),
            }),
            stream_vports: RwLock::new(None),
            stream_first_listen_wait_state: AtomicU8::new(FIRST_LISTEN_WAIT_NOT_STARTED),
            stream_vports_notify: Notify::new(),
            datagram_vports: RwLock::new(None),
            datagram_first_listen_wait_state: AtomicU8::new(FIRST_LISTEN_WAIT_NOT_STARTED),
            datagram_vports_notify: Notify::new(),
            closed: AtomicBool::new(false),
        })
    }

    fn current_listen_vports(&self, kind: PnChannelKind) -> Option<ListenVPortsRef> {
        match kind {
            PnChannelKind::Stream => self.stream_vports.read().unwrap().clone(),
            PnChannelKind::Datagram => self.datagram_vports.read().unwrap().clone(),
        }
    }

    fn role_name(&self) -> &'static str {
        match &self.role {
            PnTunnelRole::Active { .. } => "active",
            PnTunnelRole::Passive(_) => "passive",
        }
    }

    pub fn stream_security_mode(&self) -> PnProxyStreamSecurityMode {
        self.stream_security_mode
    }

    async fn wait_first_listen_if_needed(&self, kind: PnChannelKind) {
        if self.current_listen_vports(kind).is_some() {
            return;
        }

        log::debug!(
            "pn passive wait first listen local={} remote={} kind={:?}",
            self.local_id,
            self.remote_id,
            kind
        );

        let (wait_state, notify) = match kind {
            PnChannelKind::Stream => (
                &self.stream_first_listen_wait_state,
                &self.stream_vports_notify,
            ),
            PnChannelKind::Datagram => (
                &self.datagram_first_listen_wait_state,
                &self.datagram_vports_notify,
            ),
        };

        loop {
            match wait_state.load(Ordering::SeqCst) {
                FIRST_LISTEN_WAIT_DONE => return,
                FIRST_LISTEN_WAIT_IN_PROGRESS => {
                    let _ = runtime::timeout(FIRST_LISTEN_WAIT_TIMEOUT, async {
                        loop {
                            if self.closed.load(Ordering::SeqCst)
                                || self.current_listen_vports(kind).is_some()
                                || wait_state.load(Ordering::SeqCst)
                                    != FIRST_LISTEN_WAIT_IN_PROGRESS
                            {
                                break;
                            }
                            notify.notified().await;
                        }
                    })
                    .await;
                    let listen_ready = self.current_listen_vports(kind).is_some();
                    log::debug!(
                        "pn passive wait first listen done local={} remote={} kind={:?} listen_ready={} closed={}",
                        self.local_id,
                        self.remote_id,
                        kind,
                        listen_ready,
                        self.closed.load(Ordering::SeqCst)
                    );
                    return;
                }
                FIRST_LISTEN_WAIT_NOT_STARTED => {
                    if wait_state
                        .compare_exchange(
                            FIRST_LISTEN_WAIT_NOT_STARTED,
                            FIRST_LISTEN_WAIT_IN_PROGRESS,
                            Ordering::SeqCst,
                            Ordering::SeqCst,
                        )
                        .is_err()
                    {
                        continue;
                    }

                    let _ = runtime::timeout(FIRST_LISTEN_WAIT_TIMEOUT, async {
                        loop {
                            if self.closed.load(Ordering::SeqCst)
                                || self.current_listen_vports(kind).is_some()
                            {
                                break;
                            }
                            notify.notified().await;
                        }
                    })
                    .await;
                    wait_state.store(FIRST_LISTEN_WAIT_DONE, Ordering::SeqCst);
                    notify.notify_waiters();
                    let listen_ready = self.current_listen_vports(kind).is_some();
                    log::debug!(
                        "pn passive first listen window finished local={} remote={} kind={:?} listen_ready={} closed={}",
                        self.local_id,
                        self.remote_id,
                        kind,
                        listen_ready,
                        self.closed.load(Ordering::SeqCst)
                    );
                    return;
                }
                _ => return,
            }
        }
    }

    async fn take_passive_channel(
        &self,
    ) -> P2pResult<(ProxyOpenReq, TunnelStreamRead, TunnelStreamWrite)> {
        let PnTunnelRole::Passive(passive) = &self.role else {
            return Err(p2p_err!(
                P2pErrorCode::NotSupport,
                "active pn tunnel cannot accept"
            ));
        };

        let mut read = passive.read.lock().unwrap();
        let mut write = passive.write.lock().unwrap();
        let read = read
            .take()
            .ok_or_else(|| p2p_err!(P2pErrorCode::Interrupted, "pn tunnel already consumed"))?;
        let write = write
            .take()
            .ok_or_else(|| p2p_err!(P2pErrorCode::Interrupted, "pn tunnel already consumed"))?;
        self.closed.store(true, Ordering::SeqCst);
        Ok((passive.request.clone(), read, write))
    }

    async fn accept_passive_stream(
        &self,
        expected_kind: PnChannelKind,
    ) -> P2pResult<(ProxyOpenReq, TunnelStreamRead, TunnelStreamWrite)> {
        let PnTunnelRole::Passive(passive) = &self.role else {
            return Err(p2p_err!(
                P2pErrorCode::Interrupted,
                "active pn tunnel has no accept loop"
            ));
        };

        if passive.request.kind != expected_kind {
            return Err(p2p_err!(
                P2pErrorCode::NotSupport,
                "incoming pn tunnel kind mismatch"
            ));
        }

        let stream_tls_required = expected_kind == PnChannelKind::Stream
            && self.stream_security_mode == PnProxyStreamSecurityMode::TlsRequired;

        let (result, post_error) = if passive.request.to != self.local_id {
            (
                TunnelCommandResult::InvalidParam,
                Some(p2p_err!(
                    P2pErrorCode::InvalidParam,
                    "pn request target mismatch"
                )),
            )
        } else if stream_tls_required && self.tls_context.is_none() {
            (
                TunnelCommandResult::InternalError,
                Some(p2p_err!(
                    P2pErrorCode::NotSupport,
                    "pn tunnel tls context unavailable"
                )),
            )
        } else {
            self.wait_first_listen_if_needed(expected_kind).await;

            if let Some(listened) = self.current_listen_vports(expected_kind) {
                if listened.is_listen(&passive.request.purpose) {
                    log::debug!(
                        "pn passive accept allow local={} remote={} kind={:?} purpose={}",
                        self.local_id,
                        self.remote_id,
                        passive.request.kind,
                        passive.request.purpose
                    );
                    (TunnelCommandResult::Success, None)
                } else {
                    log::warn!(
                        "pn passive reject port-not-listen local={} remote={} kind={:?} purpose={} expected_kind={:?}",
                        self.local_id,
                        self.remote_id,
                        passive.request.kind,
                        passive.request.purpose,
                        expected_kind
                    );
                    (
                        TunnelCommandResult::PortNotListen,
                        Some(TunnelCommandResult::PortNotListen.into_p2p_error(format!(
                            "pn open rejected kind {:?} purpose {}",
                            passive.request.kind, passive.request.purpose
                        ))),
                    )
                }
            } else {
                log::warn!(
                    "pn passive reject listener-closed local={} remote={} kind={:?} purpose={} expected_kind={:?}",
                    self.local_id,
                    self.remote_id,
                    passive.request.kind,
                    passive.request.purpose,
                    expected_kind
                );
                (
                    TunnelCommandResult::ListenerClosed,
                    Some(p2p_err!(
                        P2pErrorCode::Interrupted,
                        "pn accept requires listen before accept"
                    )),
                )
            }
        };

        let (req, read, mut write) = self.take_passive_channel().await?;
        write_pn_command(
            &mut write,
            ProxyOpenResp {
                tunnel_id: req.tunnel_id,
                result: result as u8,
            },
        )
        .await?;

        if let Some(err) = post_error {
            log::debug!(
                "pn passive accept finished with error local={} remote={} kind={:?} purpose={} code={:?} msg={}",
                self.local_id,
                self.remote_id,
                req.kind,
                req.purpose,
                err.code(),
                err.msg()
            );
            return Err(err);
        }

        log::debug!(
            "pn passive accept success local={} remote={} kind={:?} purpose={} tls_mode={:?}",
            self.local_id,
            self.remote_id,
            req.kind,
            req.purpose,
            self.stream_security_mode
        );
        if stream_tls_required {
            let tls_context = self
                .tls_context
                .clone()
                .ok_or_else(|| p2p_err!(P2pErrorCode::NotSupport, "pn tunnel tls unavailable"))?;
            let (read, write) =
                wrap_stream_with_server_tls(read, write, &tls_context, &req.from).await?;
            Ok((req, read, write))
        } else {
            Ok((req, read, write))
        }
    }
}

#[async_trait::async_trait]
impl Tunnel for PnTunnel {
    fn tunnel_id(&self) -> TunnelId {
        self.tunnel_id
    }

    fn candidate_id(&self) -> TunnelCandidateId {
        self.candidate_id
    }

    fn form(&self) -> TunnelForm {
        TunnelForm::Proxy
    }

    fn is_reverse(&self) -> bool {
        false
    }

    fn protocol(&self) -> Protocol {
        Protocol::Ext(1)
    }

    fn local_id(&self) -> P2pId {
        self.local_id.clone()
    }

    fn remote_id(&self) -> P2pId {
        self.remote_id.clone()
    }

    fn local_ep(&self) -> Option<Endpoint> {
        None
    }

    fn remote_ep(&self) -> Option<Endpoint> {
        None
    }

    fn state(&self) -> TunnelState {
        if self.closed.load(Ordering::SeqCst) {
            TunnelState::Closed
        } else {
            TunnelState::Connected
        }
    }

    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    async fn close(&self) -> P2pResult<()> {
        self.closed.store(true, Ordering::SeqCst);
        if let PnTunnelRole::Passive(passive) = &self.role {
            let _ = passive.read.lock().unwrap().take();
            let _ = passive.write.lock().unwrap().take();
        }
        Ok(())
    }

    async fn listen_stream(&self, vports: ListenVPortsRef) -> P2pResult<()> {
        *self.stream_vports.write().unwrap() = Some(vports);
        log::debug!(
            "pn tunnel listen stream local={} remote={} role={} tunnel_id={:?}",
            self.local_id,
            self.remote_id,
            self.role_name(),
            self.tunnel_id
        );
        self.stream_vports_notify.notify_waiters();
        Ok(())
    }

    async fn listen_datagram(&self, vports: ListenVPortsRef) -> P2pResult<()> {
        *self.datagram_vports.write().unwrap() = Some(vports);
        log::debug!(
            "pn tunnel listen datagram local={} remote={} role={} tunnel_id={:?}",
            self.local_id,
            self.remote_id,
            self.role_name(),
            self.tunnel_id
        );
        self.datagram_vports_notify.notify_waiters();
        Ok(())
    }

    async fn open_stream(
        &self,
        purpose: TunnelPurpose,
    ) -> P2pResult<(TunnelStreamRead, TunnelStreamWrite)> {
        let PnTunnelRole::Active { network } = &self.role else {
            return Err(p2p_err!(
                P2pErrorCode::NotSupport,
                "incoming pn tunnel cannot open stream"
            ));
        };
        if self.closed.load(Ordering::SeqCst) {
            return Err(p2p_err!(P2pErrorCode::ErrorState, "pn tunnel closed"));
        }

        let (read, write) = network
            .open_channel(
                self.tunnel_id,
                self.remote_id.clone(),
                PnChannelKind::Stream,
                purpose,
            )
            .await?;
        if self.stream_security_mode == PnProxyStreamSecurityMode::TlsRequired {
            let tls_context = self
                .tls_context
                .clone()
                .ok_or_else(|| p2p_err!(P2pErrorCode::NotSupport, "pn tunnel tls unavailable"))?;
            wrap_stream_with_client_tls(read, write, &tls_context, &self.remote_id).await
        } else {
            Ok((read, write))
        }
    }

    async fn accept_stream(
        &self,
    ) -> P2pResult<(TunnelPurpose, TunnelStreamRead, TunnelStreamWrite)> {
        let (req, read, write) = self.accept_passive_stream(PnChannelKind::Stream).await?;
        Ok((req.purpose, read, write))
    }

    async fn open_datagram(&self, purpose: TunnelPurpose) -> P2pResult<TunnelDatagramWrite> {
        let PnTunnelRole::Active { network } = &self.role else {
            return Err(p2p_err!(
                P2pErrorCode::NotSupport,
                "incoming pn tunnel cannot open datagram"
            ));
        };
        if self.closed.load(Ordering::SeqCst) {
            return Err(p2p_err!(P2pErrorCode::ErrorState, "pn tunnel closed"));
        }
        let (_read, write) = network
            .open_channel(
                self.tunnel_id,
                self.remote_id.clone(),
                PnChannelKind::Datagram,
                purpose,
            )
            .await?;
        Ok(write)
    }

    async fn accept_datagram(&self) -> P2pResult<(TunnelPurpose, TunnelDatagramRead)> {
        let (req, read, _write) = self.accept_passive_stream(PnChannelKind::Datagram).await?;
        Ok((req.purpose, read))
    }
}

async fn wrap_stream_with_client_tls(
    read: TunnelStreamRead,
    write: TunnelStreamWrite,
    tls_context: &PnTlsContext,
    remote_id: &P2pId,
) -> P2pResult<(TunnelStreamRead, TunnelStreamWrite)> {
    let client_config = rustls::ClientConfig::builder_with_provider(crate::tls::provider().into())
        .with_protocol_versions(&[&TLS13])
        .unwrap()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(crate::tls::TlsServerCertVerifier::new(
            tls_context.cert_factory.clone(),
            remote_id.clone(),
        )))
        .with_client_auth_cert(
            vec![CertificateDer::from(
                tls_context
                    .local_identity
                    .get_identity_cert()?
                    .get_encoded_cert()?,
            )],
            PrivatePkcs8KeyDer::from(tls_context.local_identity.get_encoded_identity()?).into(),
        )
        .map_err(crate::error::into_p2p_err!(P2pErrorCode::TlsError))?;
    let tls_connector = runtime::TlsConnector::from(Arc::new(client_config));
    let stream = ProxyTlsIo::new(read, write);
    let tls_stream = tls_connector
        .connect(
            validate_server_name(remote_id.to_string())
                .try_into()
                .unwrap(),
            stream,
        )
        .await
        .map_err(crate::error::into_p2p_err!(
            P2pErrorCode::TlsError,
            "pn proxy tls client handshake failed"
        ))?;
    let (read, write) = runtime::split(runtime::TlsStream::from(tls_stream));
    Ok((Box::pin(read), Box::pin(write)))
}

async fn wrap_stream_with_server_tls(
    read: TunnelStreamRead,
    write: TunnelStreamWrite,
    tls_context: &PnTlsContext,
    remote_id: &P2pId,
) -> P2pResult<(TunnelStreamRead, TunnelStreamWrite)> {
    let resolver = DefaultTlsServerCertResolver::new();
    resolver
        .add_server_identity(tls_context.local_identity.clone())
        .await?;
    let mut server_config = ServerConfig::builder_with_provider(crate::tls::provider().into())
        .with_protocol_versions(&[&TLS13])
        .unwrap()
        .with_client_cert_verifier(Arc::new(crate::tls::TlsClientCertVerifier::new(
            tls_context.cert_factory.clone(),
        )))
        .with_cert_resolver(resolver.clone().get_resolves_server_cert());
    server_config.key_log = Arc::new(rustls::KeyLogFile::new());
    let acceptor = runtime::TlsAcceptor::from(Arc::new(server_config));
    let stream = ProxyTlsIo::new(read, write);
    let tls_stream = acceptor
        .accept(stream)
        .await
        .map_err(crate::error::into_p2p_err!(
            P2pErrorCode::TlsError,
            "pn proxy tls server handshake failed"
        ))?;
    let (_, tls_conn) = tls_stream.get_ref();
    let cert = tls_conn
        .peer_certificates()
        .ok_or_else(|| p2p_err!(P2pErrorCode::CertError, "no cert"))?;
    if cert.is_empty() {
        return Err(p2p_err!(P2pErrorCode::CertError, "no cert"));
    }
    let remote_cert = tls_context
        .cert_factory
        .create(&cert[0].as_ref().to_vec())?;
    if remote_cert.get_id() != *remote_id {
        return Err(p2p_err!(
            P2pErrorCode::CertError,
            "pn proxy tls client id mismatch expected={} actual={}",
            remote_id,
            remote_cert.get_id()
        ));
    }
    let (read, write) = runtime::split(runtime::TlsStream::from(tls_stream));
    Ok((Box::pin(read), Box::pin(write)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::networks::ListenVPortRegistry;
    use crate::networks::NetManager;
    use crate::networks::allow_all_listen_vports;
    use crate::p2p_identity::{
        EncodedP2pIdentity, EncodedP2pIdentityCert, P2pIdentity, P2pIdentityCert,
        P2pIdentityCertFactory, P2pIdentityCertRef, P2pIdentityFactory, P2pIdentityRef,
        P2pSignature, P2pSn,
    };
    use crate::tls::{DefaultTlsServerCertResolver, init_tls};
    use crate::types::TunnelId;
    #[cfg(feature = "x509")]
    use crate::x509::{
        X509IdentityCertFactory, X509IdentityFactory, generate_ed25519_x509_identity,
    };
    use sha2::{Digest, Sha256};
    use std::sync::Once;
    use tokio::io::{AsyncReadExt, AsyncWriteExt, split};
    use tokio::time::timeout;

    fn test_p2p_id(byte: u8) -> P2pId {
        P2pId::from(vec![byte; 32])
    }

    fn purpose_of(vport: u16) -> TunnelPurpose {
        TunnelPurpose::from_value(&vport).unwrap()
    }

    static INIT_TLS_ONCE: Once = Once::new();

    #[derive(Clone)]
    struct FakeTlsIdentity {
        key: Vec<u8>,
    }

    impl FakeTlsIdentity {
        fn new(key: Vec<u8>) -> Self {
            Self { key }
        }

        fn id(&self) -> P2pId {
            P2pId::from(self.key.clone())
        }

        fn name(&self) -> String {
            self.id().to_string()
        }
    }

    struct FakeTlsCert {
        key: Vec<u8>,
    }

    impl FakeTlsCert {
        fn new(key: Vec<u8>) -> Self {
            Self { key }
        }

        fn id(&self) -> P2pId {
            P2pId::from(self.key.clone())
        }

        fn name(&self) -> String {
            self.id().to_string()
        }
    }

    struct FakeTlsIdentityFactory;

    impl P2pIdentityFactory for FakeTlsIdentityFactory {
        fn create(&self, id: &EncodedP2pIdentity) -> P2pResult<P2pIdentityRef> {
            Ok(Arc::new(FakeTlsIdentity::new(id.clone())))
        }
    }

    struct FakeTlsCertFactory;

    impl P2pIdentityCertFactory for FakeTlsCertFactory {
        fn create(&self, cert: &EncodedP2pIdentityCert) -> P2pResult<P2pIdentityCertRef> {
            Ok(Arc::new(FakeTlsCert::new(cert.clone())))
        }
    }

    fn fake_signature(key: &[u8], message: &[u8]) -> Vec<u8> {
        let mut hasher = Sha256::new();
        hasher.update(key);
        hasher.update(message);
        hasher.finalize().to_vec()
    }

    impl P2pIdentity for FakeTlsIdentity {
        fn get_identity_cert(&self) -> P2pResult<P2pIdentityCertRef> {
            Ok(Arc::new(FakeTlsCert::new(self.key.clone())))
        }

        fn get_id(&self) -> P2pId {
            self.id()
        }

        fn get_name(&self) -> String {
            self.name()
        }

        fn sign_type(&self) -> crate::p2p_identity::P2pIdentitySignType {
            crate::p2p_identity::P2pIdentitySignType::Ed25519
        }

        fn sign(&self, message: &[u8]) -> P2pResult<P2pSignature> {
            Ok(fake_signature(self.key.as_slice(), message))
        }

        fn get_encoded_identity(&self) -> P2pResult<EncodedP2pIdentity> {
            Ok(self.key.clone())
        }

        fn endpoints(&self) -> Vec<Endpoint> {
            vec![]
        }

        fn update_endpoints(&self, _eps: Vec<Endpoint>) -> P2pIdentityRef {
            Arc::new(self.clone())
        }
    }

    impl P2pIdentityCert for FakeTlsCert {
        fn get_id(&self) -> P2pId {
            self.id()
        }

        fn get_name(&self) -> String {
            self.name()
        }

        fn sign_type(&self) -> crate::p2p_identity::P2pIdentitySignType {
            crate::p2p_identity::P2pIdentitySignType::Ed25519
        }

        fn verify(&self, message: &[u8], sign: &P2pSignature) -> bool {
            fake_signature(self.key.as_slice(), message) == *sign
        }

        fn verify_cert(&self, name: &str) -> bool {
            crate::networks::parse_server_name(name) == self.name()
        }

        fn get_encoded_cert(&self) -> P2pResult<EncodedP2pIdentityCert> {
            Ok(self.key.clone())
        }

        fn endpoints(&self) -> Vec<Endpoint> {
            vec![]
        }

        fn sn_list(&self) -> Vec<P2pSn> {
            vec![]
        }

        fn update_endpoints(&self, _eps: Vec<Endpoint>) -> P2pIdentityCertRef {
            Arc::new(Self::new(self.key.clone()))
        }
    }

    #[cfg(not(feature = "x509"))]
    fn init_fake_tls() {
        INIT_TLS_ONCE.call_once(|| {
            init_tls(Arc::new(FakeTlsIdentityFactory));
        });
    }

    #[cfg(feature = "x509")]
    fn tls_context(byte: u8) -> (P2pIdentityRef, PnTlsContext) {
        INIT_TLS_ONCE.call_once(|| {
            init_tls(Arc::new(X509IdentityFactory));
        });
        let identity: P2pIdentityRef =
            Arc::new(generate_ed25519_x509_identity(Some(format!("pn-tls-{byte}"))).unwrap());
        let cert_factory: P2pIdentityCertFactoryRef = Arc::new(X509IdentityCertFactory);
        (
            identity.clone(),
            PnTlsContext {
                local_identity: identity,
                cert_factory,
            },
        )
    }

    #[cfg(not(feature = "x509"))]
    fn tls_context(_byte: u8) -> (P2pIdentityRef, PnTlsContext) {
        init_fake_tls();
        let identity: P2pIdentityRef = Arc::new(FakeTlsIdentity::new(vec![42; 32]));
        let cert_factory: P2pIdentityCertFactoryRef = Arc::new(FakeTlsCertFactory);
        (
            identity.clone(),
            PnTlsContext {
                local_identity: identity,
                cert_factory,
            },
        )
    }

    fn passive_tunnel(
        kind: PnChannelKind,
        vport: u16,
        stream_security_mode: PnProxyStreamSecurityMode,
    ) -> (Arc<PnTunnel>, TunnelStreamRead, TunnelStreamWrite) {
        let local_id = test_p2p_id(1);
        let remote_id = test_p2p_id(2);
        let request = ProxyOpenReq {
            tunnel_id: TunnelId::from(42),
            from: remote_id,
            to: local_id.clone(),
            kind,
            purpose: purpose_of(vport),
        };
        let (local, remote) = tokio::io::duplex(1024);
        let (local_read, local_write) = split(local);
        let (remote_read, remote_write) = split(remote);
        (
            PnTunnel::new_passive(
                local_id,
                request,
                Box::pin(local_read),
                Box::pin(local_write),
                None,
                stream_security_mode,
            ),
            Box::pin(remote_read),
            Box::pin(remote_write),
        )
    }

    #[tokio::test]
    async fn passive_stream_accept_returns_channel_and_success_response() {
        let (tunnel, mut peer_read, mut peer_write) = passive_tunnel(
            PnChannelKind::Stream,
            1001,
            PnProxyStreamSecurityMode::Disabled,
        );
        tunnel
            .listen_stream(allow_all_listen_vports())
            .await
            .unwrap();

        let (purpose, mut read, mut write) = tunnel.accept_stream().await.unwrap();
        assert_eq!(purpose, purpose_of(1001));

        let resp = read_pn_command::<_, ProxyOpenResp>(&mut peer_read)
            .await
            .unwrap();
        assert_eq!(resp.tunnel_id, TunnelId::from(42));
        assert_eq!(resp.result, TunnelCommandResult::Success as u8);

        peer_write.write_all(b"ping").await.unwrap();
        let mut buf = [0u8; 4];
        read.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"ping");

        write.write_all(b"pong").await.unwrap();
        let mut buf = [0u8; 4];
        peer_read.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"pong");
    }

    #[tokio::test]
    async fn passive_datagram_accept_returns_read_and_success_response() {
        let (tunnel, mut peer_read, mut peer_write) = passive_tunnel(
            PnChannelKind::Datagram,
            2002,
            PnProxyStreamSecurityMode::Disabled,
        );
        tunnel
            .listen_datagram(allow_all_listen_vports())
            .await
            .unwrap();

        let (purpose, mut read) = tunnel.accept_datagram().await.unwrap();
        assert_eq!(purpose, purpose_of(2002));

        let resp = read_pn_command::<_, ProxyOpenResp>(&mut peer_read)
            .await
            .unwrap();
        assert_eq!(resp.tunnel_id, TunnelId::from(42));
        assert_eq!(resp.result, TunnelCommandResult::Success as u8);

        peer_write.write_all(b"data").await.unwrap();
        let mut buf = [0u8; 4];
        read.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"data");
    }

    #[tokio::test]
    async fn passive_stream_accept_rejects_unlistened_port() {
        let (tunnel, mut peer_read, _peer_write) = passive_tunnel(
            PnChannelKind::Stream,
            3003,
            PnProxyStreamSecurityMode::Disabled,
        );
        let empty_vports = ListenVPortRegistry::<()>::new();
        tunnel
            .listen_stream(empty_vports.as_listen_vports_ref())
            .await
            .unwrap();

        let err = tunnel.accept_stream().await.err().unwrap();
        assert_eq!(err.code(), P2pErrorCode::PortNotListen);

        let resp = read_pn_command::<_, ProxyOpenResp>(&mut peer_read)
            .await
            .unwrap();
        assert_eq!(resp.result, TunnelCommandResult::PortNotListen as u8);
    }

    #[tokio::test]
    async fn passive_stream_accept_requires_listen_first() {
        let (tunnel, mut peer_read, _peer_write) = passive_tunnel(
            PnChannelKind::Stream,
            4004,
            PnProxyStreamSecurityMode::Disabled,
        );

        let err = tunnel.accept_stream().await.err().unwrap();
        assert_eq!(err.code(), P2pErrorCode::Interrupted);

        let resp = read_pn_command::<_, ProxyOpenResp>(&mut peer_read)
            .await
            .unwrap();
        assert_eq!(resp.result, TunnelCommandResult::ListenerClosed as u8);
    }

    #[tokio::test]
    async fn passive_stream_accept_waits_for_late_listen() {
        let (tunnel, mut peer_read, mut peer_write) = passive_tunnel(
            PnChannelKind::Stream,
            5005,
            PnProxyStreamSecurityMode::Disabled,
        );

        let mut pending_accept = Box::pin({
            let tunnel = tunnel.clone();
            async move { tunnel.accept_stream().await }
        });

        assert!(
            timeout(FIRST_LISTEN_WAIT_TIMEOUT / 3, &mut pending_accept)
                .await
                .is_err()
        );

        tunnel
            .listen_stream(allow_all_listen_vports())
            .await
            .unwrap();

        let (purpose, mut read, mut write) = pending_accept.await.unwrap();
        assert_eq!(purpose, purpose_of(5005));

        let resp = read_pn_command::<_, ProxyOpenResp>(&mut peer_read)
            .await
            .unwrap();
        assert_eq!(resp.result, TunnelCommandResult::Success as u8);

        peer_write.write_all(b"late").await.unwrap();
        let mut buf = [0u8; 4];
        read.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"late");

        write.write_all(b"sync").await.unwrap();
        let mut buf = [0u8; 4];
        peer_read.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"sync");
    }

    #[tokio::test]
    async fn passive_stream_wait_is_independent_from_datagram_listen() {
        let (tunnel, mut peer_read, _peer_write) = passive_tunnel(
            PnChannelKind::Stream,
            5006,
            PnProxyStreamSecurityMode::Disabled,
        );

        let mut pending_accept = Box::pin({
            let tunnel = tunnel.clone();
            async move { tunnel.accept_stream().await }
        });

        tunnel
            .listen_datagram(allow_all_listen_vports())
            .await
            .unwrap();

        assert!(
            timeout(FIRST_LISTEN_WAIT_TIMEOUT / 3, &mut pending_accept)
                .await
                .is_err()
        );

        tunnel
            .listen_stream(allow_all_listen_vports())
            .await
            .unwrap();

        let (purpose, _read, _write) = pending_accept.await.unwrap();
        assert_eq!(purpose, purpose_of(5006));

        let resp = read_pn_command::<_, ProxyOpenResp>(&mut peer_read)
            .await
            .unwrap();
        assert_eq!(resp.result, TunnelCommandResult::Success as u8);
    }

    #[tokio::test]
    async fn passive_stream_accept_wraps_stream_with_tls_when_requested() {
        let (server_identity, server_tls) = tls_context(11);
        let (client_identity, client_tls) = tls_context(12);
        let request = ProxyOpenReq {
            tunnel_id: TunnelId::from(52),
            from: client_identity.get_id(),
            to: server_identity.get_id(),
            kind: PnChannelKind::Stream,
            purpose: purpose_of(6001),
        };
        let (local, remote) = tokio::io::duplex(4096);
        let (local_read, local_write) = split(local);
        let (remote_read, remote_write) = split(remote);
        let tunnel = PnTunnel::new_passive(
            server_identity.get_id(),
            request.clone(),
            Box::pin(local_read),
            Box::pin(local_write),
            Some(server_tls),
            PnProxyStreamSecurityMode::TlsRequired,
        );
        tunnel
            .listen_stream(allow_all_listen_vports())
            .await
            .unwrap();

        let accept_task = tokio::spawn({
            let tunnel = tunnel.clone();
            async move { tunnel.accept_stream().await }
        });

        let mut peer_read = Box::pin(remote_read) as TunnelStreamRead;
        let peer_write = Box::pin(remote_write) as TunnelStreamWrite;

        let resp = read_pn_command::<_, ProxyOpenResp>(&mut peer_read)
            .await
            .unwrap();
        assert_eq!(resp.result, TunnelCommandResult::Success as u8);

        let (mut client_read, mut client_write) = wrap_stream_with_client_tls(
            peer_read,
            peer_write,
            &client_tls,
            &server_identity.get_id(),
        )
        .await
        .unwrap();

        let (purpose, mut server_read, mut server_write) = accept_task.await.unwrap().unwrap();
        assert_eq!(purpose, purpose_of(6001));

        client_write.write_all(b"ping").await.unwrap();
        let mut ping_buf = [0u8; 4];
        server_read.read_exact(&mut ping_buf).await.unwrap();
        assert_eq!(&ping_buf, b"ping");

        server_write.write_all(b"pong").await.unwrap();
        let mut pong_buf = [0u8; 4];
        client_read.read_exact(&mut pong_buf).await.unwrap();
        assert_eq!(&pong_buf, b"pong");
    }

    #[tokio::test]
    async fn client_tls_wrapper_rejects_wrong_remote_identity() {
        let (server_identity, server_tls) = tls_context(21);
        let (client_identity, client_tls) = tls_context(22);
        let wrong_remote_id = test_p2p_id(23);
        let (server_side, client_side) = tokio::io::duplex(4096);
        let (server_read, server_write) = split(server_side);
        let (client_read, client_write) = split(client_side);

        let server_task = tokio::spawn(async move {
            wrap_stream_with_server_tls(
                Box::pin(server_read),
                Box::pin(server_write),
                &server_tls,
                &client_identity.get_id(),
            )
            .await
        });

        let client_result = wrap_stream_with_client_tls(
            Box::pin(client_read),
            Box::pin(client_write),
            &client_tls,
            &wrong_remote_id,
        )
        .await;
        assert!(client_result.is_err());

        let server_result = server_task.await.unwrap();
        assert!(server_result.is_err());
        assert_ne!(server_identity.get_id(), wrong_remote_id);
    }

    #[tokio::test]
    async fn passive_datagram_ignores_tls_mode_and_returns_read_and_success_response() {
        let (tunnel, mut peer_read, mut peer_write) = passive_tunnel(
            PnChannelKind::Datagram,
            6002,
            PnProxyStreamSecurityMode::TlsRequired,
        );
        tunnel
            .listen_datagram(allow_all_listen_vports())
            .await
            .unwrap();

        let (purpose, mut read) = tunnel.accept_datagram().await.unwrap();
        assert_eq!(purpose, purpose_of(6002));

        let resp = read_pn_command::<_, ProxyOpenResp>(&mut peer_read)
            .await
            .unwrap();
        assert_eq!(resp.result, TunnelCommandResult::Success as u8);

        peer_write.write_all(b"data").await.unwrap();
        let mut buf = [0u8; 4];
        read.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"data");
    }

    #[tokio::test]
    async fn create_tunnel_with_options_sets_tls_mode_without_local_datagram_rejection() {
        let (local_identity, _tls) = tls_context(41);
        let net_manager = NetManager::new(vec![], DefaultTlsServerCertResolver::new()).unwrap();
        let ttp_client = crate::ttp::TtpClient::new(local_identity.clone(), net_manager);
        let pn_client = super::super::pn_client::PnClient::new_with_tls_material(
            ttp_client,
            local_identity.clone(),
            Arc::new(FakeTlsCertFactory),
        );
        let remote = Endpoint::from((Protocol::Ext(1), "0.0.0.0:0".parse().unwrap()));
        let tunnel = pn_client
            .create_tunnel_with_options(
                &local_identity,
                &remote,
                &test_p2p_id(42),
                None,
                crate::networks::TunnelConnectIntent::default(),
                PnTunnelOptions {
                    stream_security_mode: PnProxyStreamSecurityMode::TlsRequired,
                },
            )
            .await
            .unwrap();

        assert_eq!(
            tunnel.stream_security_mode(),
            PnProxyStreamSecurityMode::TlsRequired
        );
        if let Err(err) = tunnel.open_datagram(purpose_of(6003)).await {
            assert_ne!(err.code(), P2pErrorCode::NotSupport);
        }
    }
}
