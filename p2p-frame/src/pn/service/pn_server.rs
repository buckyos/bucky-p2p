use std::io;
use std::pin::Pin;
use std::sync::{
    Arc, Mutex,
    atomic::{self, AtomicBool},
};
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::io::{ReadBuf, copy_bidirectional};

use crate::error::{P2pError, P2pErrorCode, P2pResult, into_p2p_err, p2p_err};
use crate::executor::Executor;
use crate::networks::{
    TunnelCommand, TunnelCommandBody, TunnelCommandResult, TunnelPurpose, TunnelStreamRead,
    TunnelStreamWrite, ValidateResult, read_tunnel_command_body, read_tunnel_command_header,
    write_tunnel_command,
};
use crate::p2p_identity::P2pId;
use crate::pn::{PROXY_SERVICE, ProxyOpenReq, ProxyOpenResp};
use crate::runtime;
use crate::ttp::{TtpListenerRef, TtpPortListener, TtpServer, TtpServerRef};

const PN_OPEN_TIMEOUT: Duration = Duration::from_secs(5);

struct ProxyStream {
    read: TunnelStreamRead,
    write: TunnelStreamWrite,
}

impl ProxyStream {
    fn new(read: TunnelStreamRead, write: TunnelStreamWrite) -> Self {
        Self { read, write }
    }
}

impl tokio::io::AsyncRead for ProxyStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        self.read.as_mut().poll_read(cx, buf)
    }
}

impl tokio::io::AsyncWrite for ProxyStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.write.as_mut().poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.write.as_mut().poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.write.as_mut().poll_shutdown(cx)
    }
}

pub struct PnServer {
    ttp_server: TtpServerRef,
    service: PnServiceRef,
    started: AtomicBool,
    stopped: AtomicBool,
    accept_task: Mutex<Option<crate::executor::SpawnHandle<()>>>,
}

pub type PnServerRef = Arc<PnServer>;
pub type PnServiceRef = Arc<PnService>;

#[derive(Clone, Debug)]
pub struct PnConnectionValidateContext {
    pub from: P2pId,
    pub to: P2pId,
    pub tunnel_id: crate::types::TunnelId,
    pub kind: crate::pn::PnChannelKind,
    pub purpose: TunnelPurpose,
}

#[async_trait::async_trait]
pub trait PnConnectionValidator: Send + Sync + 'static {
    async fn validate(&self, ctx: &PnConnectionValidateContext) -> P2pResult<ValidateResult>;
}

pub type PnConnectionValidatorRef = Arc<dyn PnConnectionValidator>;

pub struct AllowAllPnConnectionValidator;

#[async_trait::async_trait]
impl PnConnectionValidator for AllowAllPnConnectionValidator {
    async fn validate(&self, _ctx: &PnConnectionValidateContext) -> P2pResult<ValidateResult> {
        Ok(ValidateResult::Accept)
    }
}

pub fn allow_all_pn_connection_validator() -> PnConnectionValidatorRef {
    Arc::new(AllowAllPnConnectionValidator)
}

#[async_trait::async_trait]
pub trait PnTargetStreamFactory: Send + Sync + 'static {
    async fn open_target_stream(
        &self,
        target: &P2pId,
    ) -> P2pResult<(TunnelStreamRead, TunnelStreamWrite)>;
}

pub type PnTargetStreamFactoryRef = Arc<dyn PnTargetStreamFactory>;

#[async_trait::async_trait]
impl PnTargetStreamFactory for TtpServer {
    async fn open_target_stream(
        &self,
        target: &P2pId,
    ) -> P2pResult<(TunnelStreamRead, TunnelStreamWrite)> {
        let (_meta, read, write) = self
            .open_stream_by_id(
                target,
                Some(target.to_string()),
                crate::networks::TunnelPurpose::from_value(&PROXY_SERVICE.to_string()).unwrap(),
            )
            .await?;
        Ok((read, write))
    }
}

pub struct PnService {
    target_stream_factory: PnTargetStreamFactoryRef,
    connection_validator: PnConnectionValidatorRef,
}

impl PnService {
    pub fn new(
        target_stream_factory: PnTargetStreamFactoryRef,
        connection_validator: PnConnectionValidatorRef,
    ) -> PnServiceRef {
        Arc::new(Self {
            target_stream_factory,
            connection_validator,
        })
    }

    async fn validate_proxy_open_req(&self, req: &ProxyOpenReq) -> P2pResult<()> {
        let ctx = PnConnectionValidateContext {
            from: req.from.clone(),
            to: req.to.clone(),
            tunnel_id: req.tunnel_id,
            kind: req.kind,
            purpose: req.purpose.clone(),
        };
        match self.connection_validator.validate(&ctx).await? {
            ValidateResult::Accept => Ok(()),
            ValidateResult::Reject(reason) => Err(p2p_err!(
                P2pErrorCode::PermissionDenied,
                "pn connection validate failed from={} to={} kind={:?} purpose={} reason={}",
                req.from,
                req.to,
                req.kind,
                req.purpose,
                reason
            )),
        }
    }

    async fn handle_proxy_open_req(
        self: &Arc<Self>,
        from: P2pId,
        mut req: ProxyOpenReq,
        read: TunnelStreamRead,
        write: TunnelStreamWrite,
    ) {
        req.from = from.clone();
        log::debug!(
            "pn server open recv tunnel_id={:?} from={} to={} kind={:?} requested_purpose={} proxy_service={}",
            req.tunnel_id,
            req.from,
            req.to,
            req.kind,
            req.purpose,
            PROXY_SERVICE
        );

        let mut source_write = write;
        let open_result = match self.validate_proxy_open_req(&req).await {
            Ok(()) => runtime::timeout(
                PN_OPEN_TIMEOUT,
                self.target_stream_factory.open_target_stream(&req.to),
            )
            .await
            .map_err(into_p2p_err!(P2pErrorCode::Timeout, "pn open timeout"))
            .and_then(|ret| ret),
            Err(err) => Err(err),
        };

        let open_result = match open_result {
            Ok((mut target_read, mut target_write)) => {
                log::debug!(
                    "pn server open upstream connected tunnel_id={:?} target={} requested_purpose={} proxy_service={}",
                    req.tunnel_id,
                    req.to,
                    req.purpose,
                    PROXY_SERVICE
                );
                let bridge_ready = async {
                    write_proxy_command(&mut target_write, req.clone()).await?;
                    let resp = runtime::timeout(
                        PN_OPEN_TIMEOUT,
                        read_proxy_command::<_, ProxyOpenResp>(&mut target_read),
                    )
                    .await
                    .map_err(into_p2p_err!(P2pErrorCode::Timeout, "pn open timeout"))??;
                    if resp.tunnel_id != req.tunnel_id {
                        return Err(p2p_err!(
                            P2pErrorCode::InvalidData,
                            "pn open response tunnel id mismatch"
                        ));
                    }
                    let result = TunnelCommandResult::from_u8(resp.result).ok_or_else(|| {
                        p2p_err!(
                            P2pErrorCode::InvalidData,
                            "invalid pn open result {}",
                            resp.result
                        )
                    })?;
                    Ok((result, target_read, target_write))
                }
                .await;

                match bridge_ready {
                    Ok((result, target_read, target_write)) => {
                        log::debug!(
                            "pn server open upstream resp tunnel_id={:?} target={} kind={:?} requested_purpose={} proxy_service={} result={:?}",
                            req.tunnel_id,
                            req.to,
                            req.kind,
                            req.purpose,
                            PROXY_SERVICE,
                            result
                        );
                        let _ = write_proxy_command(
                            &mut source_write,
                            ProxyOpenResp {
                                tunnel_id: req.tunnel_id,
                                result: result as u8,
                            },
                        )
                        .await;
                        if result == TunnelCommandResult::Success {
                            Ok((target_read, target_write))
                        } else {
                            Err(result.into_p2p_error(format!(
                                "pn open rejected kind {:?} purpose {}",
                                req.kind, req.purpose
                            )))
                        }
                    }
                    Err(err) => {
                        log::warn!(
                            "pn server open upstream failed tunnel_id={:?} target={} kind={:?} requested_purpose={} proxy_service={} code={:?} msg={}",
                            req.tunnel_id,
                            req.to,
                            req.kind,
                            req.purpose,
                            PROXY_SERVICE,
                            err.code(),
                            err.msg()
                        );
                        let _ = write_proxy_command(
                            &mut source_write,
                            ProxyOpenResp {
                                tunnel_id: req.tunnel_id,
                                result: result_from_error(&err) as u8,
                            },
                        )
                        .await;
                        Err(err)
                    }
                }
            }
            Err(err) => {
                log::warn!(
                    "pn server open target failed tunnel_id={:?} from={} to={} kind={:?} requested_purpose={} proxy_service={} code={:?} msg={}",
                    req.tunnel_id,
                    req.from,
                    req.to,
                    req.kind,
                    req.purpose,
                    PROXY_SERVICE,
                    err.code(),
                    err.msg()
                );
                let _ = write_proxy_command(
                    &mut source_write,
                    ProxyOpenResp {
                        tunnel_id: req.tunnel_id,
                        result: result_from_error(&err) as u8,
                    },
                )
                .await;
                Err(err)
            }
        };

        if let Ok((target_read, target_write)) = open_result {
            log::debug!(
                "pn server bridge start tunnel_id={:?} from={} to={} kind={:?} requested_purpose={} proxy_service={}",
                req.tunnel_id,
                from,
                req.to,
                req.kind,
                req.purpose,
                PROXY_SERVICE
            );
            let mut source_stream = ProxyStream::new(read, source_write);
            let mut target_stream = ProxyStream::new(target_read, target_write);
            let _ = copy_bidirectional(&mut source_stream, &mut target_stream).await;
        }
    }

    pub async fn handle_proxy_connection(
        self: &Arc<Self>,
        from: P2pId,
        mut read: TunnelStreamRead,
        write: TunnelStreamWrite,
    ) {
        let header = match read_tunnel_command_header(&mut read).await {
            Ok(v) => v,
            Err(e) => {
                log::warn!("read pn control frame failed from {}: {:?}", from, e);
                return;
            }
        };

        if header.command_id == ProxyOpenReq::COMMAND_ID {
            let command_id = header.command_id;
            let data_len = header.data_len;
            if let Ok(req) = read_tunnel_command_body::<_, ProxyOpenReq>(&mut read, header).await {
                log::debug!(
                    "pn server data connection control frame from={} command_id={} data_len={}",
                    from,
                    command_id,
                    data_len
                );
                self.handle_proxy_open_req(from, req.body, read, write)
                    .await;
            }
        }
    }
}

impl PnServer {
    pub fn new(ttp_server: TtpServerRef) -> PnServerRef {
        Self::new_with_connection_validator(ttp_server, allow_all_pn_connection_validator())
    }

    pub fn new_with_connection_validator(
        ttp_server: TtpServerRef,
        connection_validator: PnConnectionValidatorRef,
    ) -> PnServerRef {
        let target_stream_factory: PnTargetStreamFactoryRef = ttp_server.clone();
        let service = PnService::new(target_stream_factory, connection_validator);
        Arc::new(Self {
            ttp_server,
            service,
            started: AtomicBool::new(false),
            stopped: AtomicBool::new(false),
            accept_task: Mutex::new(None),
        })
    }

    pub async fn start(self: &Arc<Self>) -> P2pResult<()> {
        if self
            .started
            .compare_exchange(
                false,
                true,
                atomic::Ordering::SeqCst,
                atomic::Ordering::SeqCst,
            )
            .is_err()
        {
            return Ok(());
        }

        let listener = match self
            .ttp_server
            .listen_stream(
                crate::networks::TunnelPurpose::from_value(&PROXY_SERVICE.to_string()).unwrap(),
            )
            .await
        {
            Ok(listener) => listener,
            Err(err) => {
                self.started.store(false, atomic::Ordering::SeqCst);
                return Err(err);
            }
        };

        let this = self.clone();
        let task = Executor::spawn_with_handle(async move {
            this.run_accept_loop(listener).await;
        })
        .unwrap();
        *self.accept_task.lock().unwrap() = Some(task);
        Ok(())
    }

    async fn run_accept_loop(self: Arc<Self>, listener: TtpListenerRef) {
        loop {
            let (meta, read, write) = match listener.accept().await {
                Ok(accepted) => accepted,
                Err(err) => {
                    log::warn!("pn server accept stopped: {:?}", err);
                    break;
                }
            };

            let service = self.service.clone();
            Executor::spawn(async move {
                service
                    .handle_proxy_connection(meta.remote_id, read, write)
                    .await;
            });
        }
    }

    pub fn stop(&self) {
        self.stopped.store(true, atomic::Ordering::Relaxed);
        self.abort_accept_task();
    }

    pub fn is_stopped(&self) -> bool {
        self.stopped.load(atomic::Ordering::Relaxed)
    }

    fn abort_accept_task(&self) {
        if let Some(task) = self.accept_task.lock().unwrap().take() {
            task.abort();
        }
    }
}

impl Drop for PnServer {
    fn drop(&mut self) {
        self.abort_accept_task();
    }
}

fn result_from_error(err: &P2pError) -> TunnelCommandResult {
    match err.code() {
        P2pErrorCode::PortNotListen => TunnelCommandResult::PortNotListen,
        P2pErrorCode::Timeout => TunnelCommandResult::Timeout,
        P2pErrorCode::Interrupted | P2pErrorCode::NotFound => TunnelCommandResult::Interrupted,
        P2pErrorCode::InvalidParam | P2pErrorCode::PermissionDenied | P2pErrorCode::Reject => {
            TunnelCommandResult::InvalidParam
        }
        P2pErrorCode::InvalidData => TunnelCommandResult::ProtocolError,
        _ => TunnelCommandResult::InternalError,
    }
}

async fn write_proxy_command<T>(write: &mut TunnelStreamWrite, body: T) -> P2pResult<()>
where
    T: TunnelCommandBody,
{
    let command = TunnelCommand::new(body)?;
    write_tunnel_command(write, &command).await
}

async fn read_proxy_command<R, T>(read: &mut R) -> P2pResult<T>
where
    R: runtime::AsyncRead + Unpin,
    T: TunnelCommandBody,
{
    let header = read_tunnel_command_header(read).await?;
    let command = read_tunnel_command_body::<_, T>(read, header).await?;
    Ok(command.body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::endpoint::{Endpoint, Protocol};
    use crate::error::p2p_err;
    use crate::networks::{
        NetManager, Tunnel, TunnelCommand, TunnelListener, TunnelListenerInfo, TunnelListenerRef,
        TunnelNetwork, TunnelNetworkRef, TunnelState,
    };
    use crate::p2p_identity::{
        EncodedP2pIdentity, P2pIdentity, P2pIdentityCertRef, P2pIdentityRef, P2pSignature,
    };
    use crate::pn::PnChannelKind;
    use crate::tls::DefaultTlsServerCertResolver;
    use crate::ttp::TtpServer;
    use crate::types::{TunnelCandidateId, TunnelId};
    use std::sync::Mutex as StdMutex;
    use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream, ReadHalf, WriteHalf, split};
    use tokio::sync::{Mutex as AsyncMutex, mpsc, oneshot};
    use tokio::time::{Duration, timeout};

    struct DummyIdentity {
        id: P2pId,
        name: String,
        endpoints: Vec<Endpoint>,
    }

    impl P2pIdentity for DummyIdentity {
        fn get_identity_cert(&self) -> P2pResult<P2pIdentityCertRef> {
            Err(p2p_err!(P2pErrorCode::NotSupport, "unused in test"))
        }

        fn get_id(&self) -> P2pId {
            self.id.clone()
        }

        fn get_name(&self) -> String {
            self.name.clone()
        }

        fn sign_type(&self) -> crate::p2p_identity::P2pIdentitySignType {
            crate::p2p_identity::P2pIdentitySignType::Rsa
        }

        fn sign(&self, _message: &[u8]) -> P2pResult<P2pSignature> {
            Ok(vec![])
        }

        fn get_encoded_identity(&self) -> P2pResult<EncodedP2pIdentity> {
            Ok(vec![])
        }

        fn endpoints(&self) -> Vec<Endpoint> {
            self.endpoints.clone()
        }

        fn update_endpoints(&self, eps: Vec<Endpoint>) -> P2pIdentityRef {
            Arc::new(Self {
                id: self.id.clone(),
                name: self.name.clone(),
                endpoints: eps,
            })
        }
    }

    struct FakeTunnel {
        tunnel_id: TunnelId,
        candidate_id: TunnelCandidateId,
        local_id: P2pId,
        remote_id: P2pId,
        local_ep: Endpoint,
        remote_ep: Endpoint,
        incoming_rx: AsyncMutex<
            mpsc::UnboundedReceiver<(
                crate::networks::TunnelPurpose,
                TunnelStreamRead,
                TunnelStreamWrite,
            )>,
        >,
        opened_tx: Option<
            mpsc::UnboundedSender<(
                crate::networks::TunnelPurpose,
                ReadHalf<DuplexStream>,
                WriteHalf<DuplexStream>,
            )>,
        >,
        attached_tx: StdMutex<Option<oneshot::Sender<()>>>,
    }

    impl FakeTunnel {
        fn new(
            local_id: P2pId,
            remote_id: P2pId,
            local_ep: Endpoint,
            remote_ep: Endpoint,
        ) -> (
            Arc<Self>,
            mpsc::UnboundedSender<(
                crate::networks::TunnelPurpose,
                TunnelStreamRead,
                TunnelStreamWrite,
            )>,
            oneshot::Receiver<()>,
        ) {
            let (incoming_tx, incoming_rx) = mpsc::unbounded_channel();
            let (attached_tx, attached_rx) = oneshot::channel();
            (
                Arc::new(Self {
                    tunnel_id: TunnelId::from(1),
                    candidate_id: TunnelCandidateId::from(1),
                    local_id,
                    remote_id,
                    local_ep,
                    remote_ep,
                    incoming_rx: AsyncMutex::new(incoming_rx),
                    opened_tx: None,
                    attached_tx: StdMutex::new(Some(attached_tx)),
                }),
                incoming_tx,
                attached_rx,
            )
        }

        fn new_with_open_stream(
            local_id: P2pId,
            remote_id: P2pId,
            local_ep: Endpoint,
            remote_ep: Endpoint,
        ) -> (
            Arc<Self>,
            mpsc::UnboundedReceiver<(
                crate::networks::TunnelPurpose,
                ReadHalf<DuplexStream>,
                WriteHalf<DuplexStream>,
            )>,
            oneshot::Receiver<()>,
        ) {
            let (opened_tx, opened_rx) = mpsc::unbounded_channel();
            let (attached_tx, attached_rx) = oneshot::channel();
            (
                Arc::new(Self {
                    tunnel_id: TunnelId::from(2),
                    candidate_id: TunnelCandidateId::from(2),
                    local_id,
                    remote_id,
                    local_ep,
                    remote_ep,
                    incoming_rx: AsyncMutex::new(mpsc::unbounded_channel().1),
                    opened_tx: Some(opened_tx),
                    attached_tx: StdMutex::new(Some(attached_tx)),
                }),
                opened_rx,
                attached_rx,
            )
        }
    }

    #[async_trait::async_trait]
    impl Tunnel for FakeTunnel {
        fn tunnel_id(&self) -> TunnelId {
            self.tunnel_id
        }

        fn candidate_id(&self) -> TunnelCandidateId {
            self.candidate_id
        }

        fn form(&self) -> crate::networks::TunnelForm {
            crate::networks::TunnelForm::Active
        }

        fn is_reverse(&self) -> bool {
            false
        }

        fn protocol(&self) -> Protocol {
            self.local_ep.protocol()
        }

        fn local_id(&self) -> P2pId {
            self.local_id.clone()
        }

        fn remote_id(&self) -> P2pId {
            self.remote_id.clone()
        }

        fn local_ep(&self) -> Option<Endpoint> {
            Some(self.local_ep)
        }

        fn remote_ep(&self) -> Option<Endpoint> {
            Some(self.remote_ep)
        }

        fn state(&self) -> TunnelState {
            TunnelState::Connected
        }

        fn is_closed(&self) -> bool {
            false
        }

        async fn close(&self) -> P2pResult<()> {
            Ok(())
        }

        async fn listen_stream(&self, _vports: crate::networks::ListenVPortsRef) -> P2pResult<()> {
            if let Some(tx) = self.attached_tx.lock().unwrap().take() {
                let _ = tx.send(());
            }
            Ok(())
        }

        async fn listen_datagram(
            &self,
            _vports: crate::networks::ListenVPortsRef,
        ) -> P2pResult<()> {
            Ok(())
        }

        async fn open_stream(
            &self,
            purpose: crate::networks::TunnelPurpose,
        ) -> P2pResult<(TunnelStreamRead, TunnelStreamWrite)> {
            let ((local_read, local_write), (remote_read, remote_write)) = make_stream_pair();
            if let Some(opened_tx) = &self.opened_tx {
                opened_tx
                    .send((purpose, remote_read, remote_write))
                    .map_err(|_| {
                        p2p_err!(P2pErrorCode::Interrupted, "open stream observer closed")
                    })?;
                Ok((local_read, local_write))
            } else {
                Err(p2p_err!(P2pErrorCode::NotSupport, "unused in test"))
            }
        }

        async fn accept_stream(
            &self,
        ) -> P2pResult<(
            crate::networks::TunnelPurpose,
            TunnelStreamRead,
            TunnelStreamWrite,
        )> {
            let mut rx = self.incoming_rx.lock().await;
            rx.recv()
                .await
                .ok_or_else(|| p2p_err!(P2pErrorCode::Interrupted, "stream closed"))
        }

        async fn open_datagram(
            &self,
            _purpose: crate::networks::TunnelPurpose,
        ) -> P2pResult<crate::networks::TunnelDatagramWrite> {
            Err(p2p_err!(P2pErrorCode::NotSupport, "unused in test"))
        }

        async fn accept_datagram(
            &self,
        ) -> P2pResult<(
            crate::networks::TunnelPurpose,
            crate::networks::TunnelDatagramRead,
        )> {
            Err(p2p_err!(P2pErrorCode::NotSupport, "unused in test"))
        }
    }

    struct FakeTunnelListener {
        rx: AsyncMutex<mpsc::UnboundedReceiver<P2pResult<crate::networks::TunnelRef>>>,
    }

    #[async_trait::async_trait]
    impl TunnelListener for FakeTunnelListener {
        async fn accept_tunnel(&self) -> P2pResult<crate::networks::TunnelRef> {
            let mut rx = self.rx.lock().await;
            rx.recv()
                .await
                .ok_or_else(|| p2p_err!(P2pErrorCode::Interrupted, "tunnel listener closed"))?
        }
    }

    struct FakeTunnelNetwork {
        protocol: Protocol,
        listener: TunnelListenerRef,
        tx: mpsc::UnboundedSender<P2pResult<crate::networks::TunnelRef>>,
        infos: Mutex<Vec<TunnelListenerInfo>>,
    }

    impl FakeTunnelNetwork {
        fn new(protocol: Protocol) -> Arc<Self> {
            let (tx, rx) = mpsc::unbounded_channel();
            Arc::new(Self {
                protocol,
                listener: Arc::new(FakeTunnelListener {
                    rx: AsyncMutex::new(rx),
                }),
                tx,
                infos: Mutex::new(Vec::new()),
            })
        }

        fn push_tunnel(&self, tunnel: crate::networks::TunnelRef) {
            let _ = self.tx.send(Ok(tunnel));
        }
    }

    #[async_trait::async_trait]
    impl TunnelNetwork for FakeTunnelNetwork {
        fn protocol(&self) -> Protocol {
            self.protocol
        }

        fn is_udp(&self) -> bool {
            self.protocol == Protocol::Quic
        }

        async fn listen(
            &self,
            local: &Endpoint,
            _out: Option<Endpoint>,
            mapping_port: Option<u16>,
        ) -> P2pResult<TunnelListenerRef> {
            *self.infos.lock().unwrap() = vec![TunnelListenerInfo {
                local: *local,
                mapping_port,
            }];
            Ok(self.listener.clone())
        }

        async fn close_all_listener(&self) -> P2pResult<()> {
            Ok(())
        }

        fn listeners(&self) -> Vec<TunnelListenerRef> {
            vec![self.listener.clone()]
        }

        fn listener_infos(&self) -> Vec<TunnelListenerInfo> {
            self.infos.lock().unwrap().clone()
        }

        async fn create_tunnel_with_intent(
            &self,
            _local_identity: &P2pIdentityRef,
            _remote: &Endpoint,
            _remote_id: &P2pId,
            _remote_name: Option<String>,
            _intent: crate::networks::TunnelConnectIntent,
        ) -> P2pResult<crate::networks::TunnelRef> {
            Err(p2p_err!(P2pErrorCode::NotSupport, "unused in test"))
        }

        async fn create_tunnel_with_local_ep_and_intent(
            &self,
            _local_identity: &P2pIdentityRef,
            _local_ep: &Endpoint,
            _remote: &Endpoint,
            _remote_id: &P2pId,
            _remote_name: Option<String>,
            _intent: crate::networks::TunnelConnectIntent,
        ) -> P2pResult<crate::networks::TunnelRef> {
            Err(p2p_err!(P2pErrorCode::NotSupport, "unused in test"))
        }
    }

    fn test_identity(local_ep: Endpoint) -> P2pIdentityRef {
        Arc::new(DummyIdentity {
            id: P2pId::from(vec![1u8; 32]),
            name: "pn-server-test".to_owned(),
            endpoints: vec![local_ep],
        })
    }

    fn make_stream_pair() -> (
        (TunnelStreamRead, TunnelStreamWrite),
        (ReadHalf<DuplexStream>, WriteHalf<DuplexStream>),
    ) {
        let (test_end, tunnel_end) = tokio::io::duplex(256);
        let (test_read, test_write) = split(test_end);
        let (tunnel_read, tunnel_write) = split(tunnel_end);
        (
            (Box::pin(tunnel_read), Box::pin(tunnel_write)),
            (test_read, test_write),
        )
    }

    struct FakeTargetStreamFactory {
        target_stream: StdMutex<Option<(TunnelStreamRead, TunnelStreamWrite)>>,
        opened_target: StdMutex<Option<P2pId>>,
    }

    impl FakeTargetStreamFactory {
        fn new(target_stream: (TunnelStreamRead, TunnelStreamWrite)) -> Arc<Self> {
            Arc::new(Self {
                target_stream: StdMutex::new(Some(target_stream)),
                opened_target: StdMutex::new(None),
            })
        }

        fn opened_target(&self) -> Option<P2pId> {
            self.opened_target.lock().unwrap().clone()
        }
    }

    #[async_trait::async_trait]
    impl PnTargetStreamFactory for FakeTargetStreamFactory {
        async fn open_target_stream(
            &self,
            target: &P2pId,
        ) -> P2pResult<(TunnelStreamRead, TunnelStreamWrite)> {
            *self.opened_target.lock().unwrap() = Some(target.clone());
            self.target_stream
                .lock()
                .unwrap()
                .take()
                .ok_or_else(|| p2p_err!(P2pErrorCode::Interrupted, "target stream already opened"))
        }
    }

    struct TestPnConnectionValidator {
        decision: ValidateResult,
        last_ctx: StdMutex<Option<PnConnectionValidateContext>>,
    }

    impl TestPnConnectionValidator {
        fn new(decision: ValidateResult) -> Arc<Self> {
            Arc::new(Self {
                decision,
                last_ctx: StdMutex::new(None),
            })
        }

        fn last_ctx(&self) -> Option<PnConnectionValidateContext> {
            self.last_ctx.lock().unwrap().clone()
        }
    }

    #[async_trait::async_trait]
    impl PnConnectionValidator for TestPnConnectionValidator {
        async fn validate(&self, ctx: &PnConnectionValidateContext) -> P2pResult<ValidateResult> {
            *self.last_ctx.lock().unwrap() = Some(ctx.clone());
            match &self.decision {
                ValidateResult::Accept => Ok(ValidateResult::Accept),
                ValidateResult::Reject(reason) => Ok(ValidateResult::Reject(reason.clone())),
            }
        }
    }

    fn init_executor() {
        Executor::init_new_multi_thread(None);
    }

    #[tokio::test]
    async fn pn_service_uses_injected_target_stream_factory() {
        let source_id = P2pId::from(vec![2u8; 32]);
        let target_id = P2pId::from(vec![3u8; 32]);
        let ((service_target_read, service_target_write), (mut target_read, mut target_write)) =
            make_stream_pair();
        let target_stream_factory =
            FakeTargetStreamFactory::new((service_target_read, service_target_write));
        let service = PnService::new(
            target_stream_factory.clone(),
            allow_all_pn_connection_validator(),
        );

        let ((service_source_read, service_source_write), (mut source_read, mut source_write)) =
            make_stream_pair();
        let req = ProxyOpenReq {
            tunnel_id: TunnelId::from(24),
            from: P2pId::default(),
            to: target_id.clone(),
            kind: PnChannelKind::Stream,
            purpose: crate::networks::TunnelPurpose::from_value(&2000u16).unwrap(),
        };

        let service_task = tokio::spawn({
            let service = service.clone();
            let source_id = source_id.clone();
            let req = req.clone();
            async move {
                service
                    .handle_proxy_open_req(
                        source_id,
                        req,
                        service_source_read,
                        service_source_write,
                    )
                    .await;
            }
        });

        let target_header = read_tunnel_command_header(&mut target_read).await.unwrap();
        let target_req =
            read_tunnel_command_body::<_, ProxyOpenReq>(&mut target_read, target_header)
                .await
                .unwrap()
                .body;
        assert_eq!(
            target_stream_factory.opened_target(),
            Some(target_id.clone())
        );
        assert_eq!(target_req.tunnel_id, req.tunnel_id);
        assert_eq!(target_req.from, source_id);
        assert_eq!(target_req.to, target_id);

        let resp = TunnelCommand::new(ProxyOpenResp {
            tunnel_id: req.tunnel_id,
            result: TunnelCommandResult::Success as u8,
        })
        .unwrap();
        write_tunnel_command(&mut target_write, &resp)
            .await
            .unwrap();

        let source_header = read_tunnel_command_header(&mut source_read).await.unwrap();
        let source_resp =
            read_tunnel_command_body::<_, ProxyOpenResp>(&mut source_read, source_header)
                .await
                .unwrap()
                .body;
        assert_eq!(source_resp.tunnel_id, req.tunnel_id);
        assert_eq!(source_resp.result, TunnelCommandResult::Success as u8);

        source_write.write_all(b"ping").await.unwrap();
        let mut ping_buf = [0u8; 4];
        target_read.read_exact(&mut ping_buf).await.unwrap();
        assert_eq!(&ping_buf, b"ping");

        target_write.write_all(b"pong").await.unwrap();
        let mut pong_buf = [0u8; 4];
        source_read.read_exact(&mut pong_buf).await.unwrap();
        assert_eq!(&pong_buf, b"pong");

        drop(source_write);
        drop(target_write);
        drop(source_read);
        drop(target_read);
        timeout(Duration::from_secs(1), service_task)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn pn_service_rejects_proxy_open_when_validator_rejects() {
        let source_id = P2pId::from(vec![2u8; 32]);
        let target_id = P2pId::from(vec![3u8; 32]);
        let ((service_target_read, service_target_write), _) = make_stream_pair();
        let target_stream_factory =
            FakeTargetStreamFactory::new((service_target_read, service_target_write));
        let validator = TestPnConnectionValidator::new(ValidateResult::Reject(
            "source peer is not allowed".to_owned(),
        ));
        let service = PnService::new(target_stream_factory.clone(), validator.clone());

        let ((service_source_read, service_source_write), (mut source_read, _source_write)) =
            make_stream_pair();
        let req = ProxyOpenReq {
            tunnel_id: TunnelId::from(25),
            from: P2pId::default(),
            to: target_id.clone(),
            kind: PnChannelKind::Stream,
            purpose: crate::networks::TunnelPurpose::from_value(&2001u16).unwrap(),
        };

        service
            .handle_proxy_open_req(
                source_id.clone(),
                req.clone(),
                service_source_read,
                service_source_write,
            )
            .await;

        assert_eq!(target_stream_factory.opened_target(), None);

        let ctx = validator.last_ctx().unwrap();
        assert_eq!(ctx.from, source_id);
        assert_eq!(ctx.to, target_id);
        assert_eq!(ctx.tunnel_id, req.tunnel_id);
        assert_eq!(ctx.kind, req.kind);
        assert_eq!(ctx.purpose, req.purpose);

        let source_header = read_tunnel_command_header(&mut source_read).await.unwrap();
        let source_resp =
            read_tunnel_command_body::<_, ProxyOpenResp>(&mut source_read, source_header)
                .await
                .unwrap()
                .body;
        assert_eq!(source_resp.tunnel_id, req.tunnel_id);
        assert_eq!(source_resp.result, TunnelCommandResult::InvalidParam as u8);
    }

    #[tokio::test]
    async fn pn_server_listens_and_bridges_proxy_stream() {
        init_executor();

        let local_ep = Endpoint::from((Protocol::Quic, "127.0.0.1:23101".parse().unwrap()));
        let source_ep = Endpoint::from((Protocol::Quic, "127.0.0.1:23102".parse().unwrap()));
        let target_ep = Endpoint::from((Protocol::Quic, "127.0.0.1:23103".parse().unwrap()));
        let identity = test_identity(local_ep);
        let source_id = P2pId::from(vec![2u8; 32]);
        let target_id = P2pId::from(vec![3u8; 32]);

        let fake_network = FakeTunnelNetwork::new(Protocol::Quic);
        let net_manager = NetManager::new(
            vec![fake_network.clone() as TunnelNetworkRef],
            DefaultTlsServerCertResolver::new(),
        )
        .unwrap();
        net_manager.listen(&[local_ep], None).await.unwrap();
        let ttp_server = TtpServer::new(identity.clone(), net_manager).unwrap();

        let pn_server = PnServer::new(ttp_server.clone());
        pn_server.start().await.unwrap();

        let (target_tunnel, mut target_open_rx, target_attached) = FakeTunnel::new_with_open_stream(
            identity.get_id(),
            target_id.clone(),
            local_ep,
            target_ep,
        );
        fake_network.push_tunnel(target_tunnel);
        timeout(Duration::from_secs(1), target_attached)
            .await
            .unwrap()
            .unwrap();

        let (source_tunnel, source_stream_tx, source_attached) =
            FakeTunnel::new(identity.get_id(), source_id.clone(), local_ep, source_ep);
        fake_network.push_tunnel(source_tunnel);
        timeout(Duration::from_secs(1), source_attached)
            .await
            .unwrap()
            .unwrap();

        let ((server_read, server_write), (mut source_read, mut source_write)) = make_stream_pair();
        source_stream_tx
            .send((
                crate::networks::TunnelPurpose::from_value(&PROXY_SERVICE.to_string()).unwrap(),
                server_read,
                server_write,
            ))
            .unwrap();

        let req = ProxyOpenReq {
            tunnel_id: TunnelId::from(42),
            from: source_id.clone(),
            to: target_id.clone(),
            kind: PnChannelKind::Stream,
            purpose: crate::networks::TunnelPurpose::from_value(&2000u16).unwrap(),
        };
        let command = TunnelCommand::new(req.clone()).unwrap();
        write_tunnel_command(&mut source_write, &command)
            .await
            .unwrap();

        let (purpose, mut target_read, mut target_write) =
            timeout(Duration::from_secs(1), target_open_rx.recv())
                .await
                .unwrap()
                .unwrap();
        assert_eq!(
            purpose,
            crate::networks::TunnelPurpose::from_value(&PROXY_SERVICE.to_string()).unwrap()
        );

        let target_header = read_tunnel_command_header(&mut target_read).await.unwrap();
        let target_req =
            read_tunnel_command_body::<_, ProxyOpenReq>(&mut target_read, target_header)
                .await
                .unwrap()
                .body;
        assert_eq!(target_req.tunnel_id, req.tunnel_id);
        assert_eq!(target_req.from, source_id);
        assert_eq!(target_req.to, target_id);

        let resp = TunnelCommand::new(ProxyOpenResp {
            tunnel_id: req.tunnel_id,
            result: TunnelCommandResult::Success as u8,
        })
        .unwrap();
        write_tunnel_command(&mut target_write, &resp)
            .await
            .unwrap();

        let source_header = read_tunnel_command_header(&mut source_read).await.unwrap();
        let source_resp =
            read_tunnel_command_body::<_, ProxyOpenResp>(&mut source_read, source_header)
                .await
                .unwrap()
                .body;
        assert_eq!(source_resp.tunnel_id, req.tunnel_id);
        assert_eq!(source_resp.result, TunnelCommandResult::Success as u8);

        source_write.write_all(b"ping").await.unwrap();
        let mut ping_buf = [0u8; 4];
        target_read.read_exact(&mut ping_buf).await.unwrap();
        assert_eq!(&ping_buf, b"ping");

        target_write.write_all(b"pong").await.unwrap();
        let mut pong_buf = [0u8; 4];
        source_read.read_exact(&mut pong_buf).await.unwrap();
        assert_eq!(&pong_buf, b"pong");
    }
}
