use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use crate::endpoint::{Endpoint, Protocol};
use crate::error::{P2pErrorCode, P2pResult, into_p2p_err, p2p_err};
use crate::executor::Executor;
use crate::networks::{
    IncomingTunnelCallback, Tunnel, TunnelCommand, TunnelCommandBody, TunnelCommandResult,
    TunnelListener, TunnelListenerInfo, TunnelListenerRef, TunnelNetwork, TunnelPurpose, TunnelRef,
    TunnelStreamRead,
    TunnelStreamWrite, read_tunnel_command_body, read_tunnel_command_header, write_tunnel_command,
};
use crate::p2p_identity::{P2pId, P2pIdentityCertFactoryRef, P2pIdentityRef};
use crate::pn::{
    PROXY_SERVICE, PnChannelKind, ProxyControlOpenReq, ProxyControlOpenResp, ProxyOpenReq,
    ProxyOpenResp,
};
use crate::runtime;
use crate::ttp::{TtpClientRef, TtpPortListener};
use crate::types::{TunnelId, TunnelIdGenerator};

use super::pn_listener::PnListener;
use super::pn_tunnel::{
    PnProxyStreamSecurityMode, PnTlsContext, PnTunnel, PnTunnelOptions, RejectedPassivePnChannel,
};

const PN_OPEN_TIMEOUT: Duration = Duration::from_secs(5);

pub(super) struct PnShared {
    ttp_client: TtpClientRef,
    gen_id: Arc<TunnelIdGenerator>,
    tls_context: Option<PnTlsContext>,
    stream_security_mode: AtomicU8,
    tunnel_idle_timeout: Mutex<Option<Duration>>,
    tunnels: Mutex<HashMap<PnTunnelKey, Weak<PnTunnel>>>,
}

impl PnShared {
    pub(super) fn local_id(&self) -> P2pId {
        self.ttp_client.local_id()
    }

    pub(super) fn tls_context(&self) -> Option<PnTlsContext> {
        self.tls_context.clone()
    }

    pub(super) fn stream_security_mode(&self) -> PnProxyStreamSecurityMode {
        PnProxyStreamSecurityMode::from_atomic(self.stream_security_mode.load(Ordering::SeqCst))
    }

    pub(super) fn set_stream_security_mode(&self, mode: PnProxyStreamSecurityMode) {
        self.stream_security_mode
            .store(mode.to_atomic(), Ordering::SeqCst);
    }

    pub(super) fn tunnel_idle_timeout(&self) -> Option<Duration> {
        *self.tunnel_idle_timeout.lock().unwrap()
    }

    pub(super) fn set_tunnel_idle_timeout(&self, timeout: Option<Duration>) {
        *self.tunnel_idle_timeout.lock().unwrap() = timeout;
    }

    pub(super) fn tunnel_key(remote_id: P2pId, tunnel_id: TunnelId) -> PnTunnelKey {
        PnTunnelKey {
            remote_id,
            tunnel_id,
        }
    }

    pub(super) fn get_tunnel(&self, key: &PnTunnelKey) -> Option<Arc<PnTunnel>> {
        let mut tunnels = self.tunnels.lock().unwrap();
        match tunnels.get(key).and_then(Weak::upgrade) {
            Some(tunnel) if !tunnel.is_closed_flag() => Some(tunnel),
            _ => {
                tunnels.remove(key);
                None
            }
        }
    }

    pub(super) fn unregister_tunnel(&self, key: &PnTunnelKey, tunnel: &PnTunnel) {
        let mut tunnels = self.tunnels.lock().unwrap();
        let should_remove = match tunnels.get(key).and_then(Weak::upgrade) {
            Some(current) => std::ptr::eq(Arc::as_ptr(&current), tunnel as *const PnTunnel),
            None => true,
        };
        if should_remove {
            tunnels.remove(key);
        }
    }

    pub(super) fn register_tunnel(&self, key: PnTunnelKey, tunnel: &Arc<PnTunnel>) {
        self.tunnels
            .lock()
            .unwrap()
            .insert(key, Arc::downgrade(tunnel));
    }

    pub(super) fn register_control_tunnel_if_absent(
        &self,
        key: PnTunnelKey,
        tunnel: &Arc<PnTunnel>,
    ) -> P2pResult<()> {
        if tunnel.is_closed_flag() {
            return Err(p2p_err!(
                P2pErrorCode::Interrupted,
                "pn control tunnel closed before registration"
            ));
        }
        let mut tunnels = self.tunnels.lock().unwrap();
        if let Some(current) = tunnels.get(&key).and_then(Weak::upgrade) {
            if !current.is_closed_flag() {
                return Err(p2p_err!(
                    P2pErrorCode::AlreadyExists,
                    "pn control tunnel already exists"
                ));
            }
        }
        tunnels.insert(key, Arc::downgrade(tunnel));
        Ok(())
    }

    pub(super) fn dispatch_passive_channel(
        self: &Arc<Self>,
        key: PnTunnelKey,
        req: ProxyOpenReq,
        read: TunnelStreamRead,
        write: TunnelStreamWrite,
    ) -> PassiveTunnelDispatch {
        let tunnel = {
            let mut tunnels = self.tunnels.lock().unwrap();
            match tunnels.get(&key).and_then(Weak::upgrade) {
                Some(tunnel) if !tunnel.is_closed_flag() => Some(tunnel),
                _ => {
                    tunnels.remove(&key);
                    None
                }
            }
        };

        if let Some(tunnel) = tunnel {
            match tunnel.push_passive_channel(req, read, write) {
                Ok(()) => return PassiveTunnelDispatch::Dispatched,
                Err(rejected) => {
                    if tunnel.is_closed_flag() {
                        self.unregister_tunnel(&key, &tunnel);
                    }
                    return PassiveTunnelDispatch::Rejected(rejected);
                }
            }
        }

        PassiveTunnelDispatch::Rejected(RejectedPassivePnChannel {
            request: req,
            read,
            write,
            error: p2p_err!(
                P2pErrorCode::ErrorState,
                "pn tunnel control channel is not established"
            ),
        })
    }

    pub(super) fn register_passive_control_tunnel(
        self: &Arc<Self>,
        req: ProxyControlOpenReq,
    ) -> P2pResult<Arc<PnTunnel>> {
        let key = PnShared::tunnel_key(req.from.clone(), req.tunnel_id);
        let mut tunnels = self.tunnels.lock().unwrap();
        if let Some(tunnel) = tunnels.get(&key).and_then(Weak::upgrade) {
            if !tunnel.is_closed_flag() {
                return Err(p2p_err!(
                    P2pErrorCode::AlreadyExists,
                    "pn control tunnel already exists"
                ));
            }
            tunnels.remove(&key);
        } else {
            tunnels.remove(&key);
        }
        let tunnel = PnTunnel::new_passive_control(
            self.local_id(),
            req,
            Some(self.clone()),
            self.tls_context(),
            self.stream_security_mode(),
        );
        tunnels.insert(key, Arc::downgrade(&tunnel));
        Ok(tunnel)
    }

    #[cfg(test)]
    pub(super) fn new_for_test(local_identity: P2pIdentityRef) -> Arc<Self> {
        let net_manager = crate::networks::NetManager::new(
            vec![],
            crate::tls::DefaultTlsServerCertResolver::new(),
        )
        .unwrap();
        Arc::new(Self {
            ttp_client: crate::ttp::TtpClient::new(local_identity, net_manager),
            gen_id: Arc::new(TunnelIdGenerator::new()),
            tls_context: None,
            stream_security_mode: AtomicU8::new(PnProxyStreamSecurityMode::Disabled.to_atomic()),
            tunnel_idle_timeout: Mutex::new(Some(super::pn_tunnel::DEFAULT_PN_TUNNEL_IDLE_TIMEOUT)),
            tunnels: Mutex::new(HashMap::new()),
        })
    }

    pub(super) async fn open_channel(
        &self,
        tunnel_id: crate::types::TunnelId,
        remote_id: P2pId,
        kind: PnChannelKind,
        purpose: TunnelPurpose,
    ) -> P2pResult<(TunnelStreamRead, TunnelStreamWrite)> {
        let remote_id_for_log = remote_id.clone();
        let req = ProxyOpenReq {
            tunnel_id,
            from: self.local_id(),
            to: remote_id,
            kind,
            purpose: purpose.clone(),
        };
        log::debug!(
            "pn open send tunnel_id={:?} from={} to={} kind={:?} purpose={}",
            tunnel_id,
            req.from,
            req.to,
            kind,
            purpose
        );
        let (mut read, mut write) = self.create_data_connection().await?;
        write_pn_command(&mut write, req).await?;

        let resp = match runtime::timeout(
            PN_OPEN_TIMEOUT,
            read_pn_command::<_, ProxyOpenResp>(&mut read),
        )
        .await
        {
            Ok(resp) => resp?,
            Err(err) => {
                log::warn!(
                    "pn open timeout tunnel_id={:?} to={} kind={:?} purpose={} timeout_ms={}",
                    tunnel_id,
                    remote_id_for_log,
                    kind,
                    purpose,
                    PN_OPEN_TIMEOUT.as_millis()
                );
                return Err(into_p2p_err!(P2pErrorCode::Timeout, "pn open timeout")(err));
            }
        };
        if resp.tunnel_id != tunnel_id {
            return Err(p2p_err!(
                P2pErrorCode::InvalidData,
                "pn open response tunnel id mismatch"
            ));
        }
        log::debug!(
            "pn open resp tunnel_id={:?} to={} kind={:?} purpose={} result={}",
            tunnel_id,
            remote_id_for_log,
            kind,
            purpose,
            resp.result
        );
        let result = TunnelCommandResult::from_u8(resp.result).ok_or_else(|| {
            p2p_err!(
                P2pErrorCode::InvalidData,
                "invalid pn open result {}",
                resp.result
            )
        })?;
        if result != TunnelCommandResult::Success {
            return Err(result.into_p2p_error(format!(
                "pn open rejected kind {:?} purpose {}",
                kind, purpose
            )));
        }
        Ok((read, write))
    }

    pub(super) async fn open_control_channel(
        &self,
        tunnel_id: crate::types::TunnelId,
        remote_id: P2pId,
    ) -> P2pResult<(TunnelStreamRead, TunnelStreamWrite)> {
        let req = ProxyControlOpenReq {
            tunnel_id,
            from: self.local_id(),
            to: remote_id.clone(),
        };
        log::debug!(
            "pn control open send tunnel_id={:?} from={} to={}",
            tunnel_id,
            req.from,
            req.to
        );
        let (mut read, mut write) = self.create_data_connection().await?;
        write_pn_command(&mut write, req).await?;

        let resp = match runtime::timeout(
            PN_OPEN_TIMEOUT,
            read_pn_command::<_, ProxyControlOpenResp>(&mut read),
        )
        .await
        {
            Ok(resp) => resp?,
            Err(err) => {
                log::warn!(
                    "pn control open timeout tunnel_id={:?} to={} timeout_ms={}",
                    tunnel_id,
                    remote_id,
                    PN_OPEN_TIMEOUT.as_millis()
                );
                return Err(into_p2p_err!(
                    P2pErrorCode::Timeout,
                    "pn control open timeout"
                )(err));
            }
        };
        if resp.tunnel_id != tunnel_id {
            return Err(p2p_err!(
                P2pErrorCode::InvalidData,
                "pn control open response tunnel id mismatch"
            ));
        }
        let result = TunnelCommandResult::from_u8(resp.result).ok_or_else(|| {
            p2p_err!(
                P2pErrorCode::InvalidData,
                "invalid pn control open result {}",
                resp.result
            )
        })?;
        if result != TunnelCommandResult::Success {
            return Err(result.into_p2p_error("pn control open rejected".to_owned()));
        }
        Ok((read, write))
    }

    async fn create_data_connection(&self) -> P2pResult<(TunnelStreamRead, TunnelStreamWrite)> {
        let (_meta, read, write) = self
            .ttp_client
            .open_stream_on_latest_tunnel(TunnelPurpose::from_value(&PROXY_SERVICE.to_string())?)
            .await?;
        Ok((read, write))
    }
}

pub(super) enum PassiveTunnelDispatch {
    Dispatched,
    Rejected(RejectedPassivePnChannel),
}

pub struct PnClient {
    shared: Arc<PnShared>,
    listener: Mutex<Option<TunnelListenerRef>>,
    listener_infos: Mutex<Vec<TunnelListenerInfo>>,
}

impl PnClient {
    pub fn new(ttp_client: TtpClientRef) -> Arc<Self> {
        Self::new_with_tls_context(ttp_client, None)
    }

    fn new_with_tls_context(
        ttp_client: TtpClientRef,
        tls_context: Option<PnTlsContext>,
    ) -> Arc<Self> {
        Arc::new(Self {
            shared: Arc::new(PnShared {
                ttp_client,
                gen_id: Arc::new(TunnelIdGenerator::new()),
                tls_context,
                stream_security_mode: AtomicU8::new(
                    PnProxyStreamSecurityMode::Disabled.to_atomic(),
                ),
                tunnel_idle_timeout: Mutex::new(Some(
                    super::pn_tunnel::DEFAULT_PN_TUNNEL_IDLE_TIMEOUT,
                )),
                tunnels: Mutex::new(HashMap::new()),
            }),
            listener: Mutex::new(None),
            listener_infos: Mutex::new(Vec::new()),
        })
    }

    pub fn new_with_tls_material(
        ttp_client: TtpClientRef,
        local_identity: P2pIdentityRef,
        cert_factory: P2pIdentityCertFactoryRef,
    ) -> Arc<Self> {
        Self::new_with_tls_context(
            ttp_client,
            Some(PnTlsContext {
                local_identity,
                cert_factory,
            }),
        )
    }

    pub async fn create_tunnel_with_options(
        &self,
        _local_identity: &P2pIdentityRef,
        _remote: &Endpoint,
        remote_id: &P2pId,
        _remote_name: Option<String>,
        intent: crate::networks::TunnelConnectIntent,
        options: PnTunnelOptions,
    ) -> P2pResult<Arc<PnTunnel>> {
        let tunnel_id = if intent.tunnel_id == crate::types::TunnelId::default() {
            self.shared.gen_id.generate()
        } else {
            intent.tunnel_id
        };
        let candidate_id = if intent.candidate_id == crate::types::TunnelCandidateId::default() {
            crate::types::TunnelCandidateId::from(tunnel_id.value())
        } else {
            intent.candidate_id
        };
        let tunnel = PnTunnel::new_active(
            tunnel_id,
            candidate_id,
            self.shared.local_id(),
            remote_id.clone(),
            self.shared.clone(),
            options.stream_security_mode,
        );
        let (control_read, control_write) = match self
            .shared
            .open_control_channel(tunnel_id, remote_id.clone())
            .await
        {
            Ok(control) => control,
            Err(err) => {
                let _ = tunnel.close().await;
                return Err(err);
            }
        };
        if let Err(err) = self.shared.register_control_tunnel_if_absent(
            PnShared::tunnel_key(remote_id.clone(), tunnel_id),
            &tunnel,
        ) {
            let _ = tunnel.close().await;
            return Err(err);
        }
        tunnel
            .set_control_channel(control_read, control_write)
            .await;
        Ok(tunnel)
    }

    pub fn set_stream_security_mode(&self, mode: PnProxyStreamSecurityMode) {
        self.shared.set_stream_security_mode(mode);
    }

    pub fn stream_security_mode(&self) -> PnProxyStreamSecurityMode {
        self.shared.stream_security_mode()
    }

    pub fn set_tunnel_idle_timeout(&self, timeout: Option<Duration>) {
        self.shared.set_tunnel_idle_timeout(timeout);
    }

    pub fn tunnel_idle_timeout(&self) -> Option<Duration> {
        self.shared.tunnel_idle_timeout()
    }

    #[cfg(test)]
    pub(super) fn get_tunnel_for_test(
        &self,
        remote_id: &P2pId,
        tunnel_id: TunnelId,
    ) -> Option<Arc<PnTunnel>> {
        self.shared
            .get_tunnel(&PnShared::tunnel_key(remote_id.clone(), tunnel_id))
    }
}

#[async_trait::async_trait]
impl TunnelNetwork for PnClient {
    fn protocol(&self) -> Protocol {
        Protocol::Ext(1)
    }

    fn is_udp(&self) -> bool {
        false
    }

    async fn listen(
        &self,
        local: &Endpoint,
        _out: Option<Endpoint>,
        mapping_port: Option<u16>,
        on_incoming_tunnel: IncomingTunnelCallback,
    ) -> P2pResult<()> {
        {
            let listener = self.listener.lock().unwrap();
            if listener.is_some() {
                log::debug!(
                    "pn client listen reuse local_id={} local_ep={} proxy_service={} mapping_port={:?}",
                    self.shared.local_id(),
                    local,
                    PROXY_SERVICE,
                    mapping_port
                );
                return Ok(());
            }
        }
        log::debug!(
            "pn client listen start local_id={} local_ep={} proxy_service={} mapping_port={:?}",
            self.shared.local_id(),
            local,
            PROXY_SERVICE,
            mapping_port
        );
        let ttp_listener = self
            .shared
            .ttp_client
            .listen_stream(TunnelPurpose::from_value(&PROXY_SERVICE.to_string())?)
            .await?;
        let listener: TunnelListenerRef =
            Arc::new(PnListener::new(self.shared.clone(), ttp_listener));
        *self.listener_infos.lock().unwrap() = vec![TunnelListenerInfo {
            local: *local,
            mapping_port,
        }];
        *self.listener.lock().unwrap() = Some(listener.clone());
        Executor::spawn_ok(async move {
            loop {
                match listener.accept_tunnel().await {
                    Ok(tunnel) => (on_incoming_tunnel)(Ok(tunnel)).await,
                    Err(err) => {
                        if matches!(
                            err.code(),
                            P2pErrorCode::Interrupted | P2pErrorCode::ErrorState
                        ) {
                            break;
                        }
                        (on_incoming_tunnel)(Err(err)).await;
                    }
                }
            }
        });
        log::debug!(
            "pn client listen ready local_id={} local_ep={} proxy_service={} protocol={:?}",
            self.shared.local_id(),
            local,
            PROXY_SERVICE,
            self.protocol()
        );
        Ok(())
    }

    async fn close_all_listener(&self) -> P2pResult<()> {
        log::debug!(
            "pn client close listener local_id={} proxy_service={} had_listener={}",
            self.shared.local_id(),
            PROXY_SERVICE,
            self.listener.lock().unwrap().is_some()
        );
        self.shared
            .ttp_client
            .unlisten_stream(&TunnelPurpose::from_value(&PROXY_SERVICE.to_string())?)
            .await?;
        *self.listener.lock().unwrap() = None;
        self.listener_infos.lock().unwrap().clear();
        log::debug!(
            "pn client listener closed local_id={} proxy_service={}",
            self.shared.local_id(),
            PROXY_SERVICE
        );
        Ok(())
    }

    fn listener_infos(&self) -> Vec<TunnelListenerInfo> {
        self.listener_infos.lock().unwrap().clone()
    }

    async fn create_tunnel_with_intent(
        &self,
        _local_identity: &P2pIdentityRef,
        _remote: &Endpoint,
        remote_id: &P2pId,
        _remote_name: Option<String>,
        intent: crate::networks::TunnelConnectIntent,
    ) -> P2pResult<TunnelRef> {
        let tunnel_id = if intent.tunnel_id == crate::types::TunnelId::default() {
            self.shared.gen_id.generate()
        } else {
            intent.tunnel_id
        };
        let candidate_id = if intent.candidate_id == crate::types::TunnelCandidateId::default() {
            crate::types::TunnelCandidateId::from(tunnel_id.value())
        } else {
            intent.candidate_id
        };
        let tunnel = PnTunnel::new_active(
            tunnel_id,
            candidate_id,
            self.shared.local_id(),
            remote_id.clone(),
            self.shared.clone(),
            self.shared.stream_security_mode(),
        );
        let (control_read, control_write) = match self
            .shared
            .open_control_channel(tunnel_id, remote_id.clone())
            .await
        {
            Ok(control) => control,
            Err(err) => {
                let _ = tunnel.close().await;
                return Err(err);
            }
        };
        if let Err(err) = self.shared.register_control_tunnel_if_absent(
            PnShared::tunnel_key(remote_id.clone(), tunnel_id),
            &tunnel,
        ) {
            let _ = tunnel.close().await;
            return Err(err);
        }
        tunnel
            .set_control_channel(control_read, control_write)
            .await;
        let tunnel: TunnelRef = tunnel;
        Ok(tunnel)
    }

    async fn create_tunnel_with_local_ep_and_intent(
        &self,
        local_identity: &P2pIdentityRef,
        _local_ep: &Endpoint,
        remote: &Endpoint,
        remote_id: &P2pId,
        remote_name: Option<String>,
        intent: crate::networks::TunnelConnectIntent,
    ) -> P2pResult<TunnelRef> {
        self.create_tunnel_with_intent(local_identity, remote, remote_id, remote_name, intent)
            .await
    }
}

#[derive(Clone, Eq, Hash, PartialEq)]
pub(super) struct PnTunnelKey {
    remote_id: P2pId,
    tunnel_id: TunnelId,
}

pub(super) async fn write_pn_command<W, T>(write: &mut W, body: T) -> P2pResult<()>
where
    W: runtime::AsyncWrite + Unpin,
    T: TunnelCommandBody,
{
    let command = TunnelCommand::new(body)?;
    write_tunnel_command(write, &command).await
}

pub(super) async fn read_pn_command<R, T>(read: &mut R) -> P2pResult<T>
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
    use crate::networks::{
        ListenVPortsRef, NetManager, TunnelDatagramRead, TunnelDatagramWrite, TunnelForm,
        TunnelState,
    };
    use crate::p2p_identity::{EncodedP2pIdentity, P2pIdentity, P2pIdentityCertRef, P2pSignature};
    use crate::types::TunnelCandidateId;
    use std::sync::Mutex as StdMutex;
    use tokio::io::split;
    use tokio::time::{Duration, timeout};

    #[derive(Clone)]
    struct FakeIdentity {
        id: P2pId,
    }

    impl FakeIdentity {
        fn new(byte: u8) -> Self {
            Self {
                id: P2pId::from(vec![byte; 32]),
            }
        }
    }

    impl P2pIdentity for FakeIdentity {
        fn get_identity_cert(&self) -> P2pResult<P2pIdentityCertRef> {
            Err(p2p_err!(P2pErrorCode::NotSupport, "no cert"))
        }

        fn get_id(&self) -> P2pId {
            self.id.clone()
        }

        fn get_name(&self) -> String {
            self.id.to_string()
        }

        fn sign_type(&self) -> crate::p2p_identity::P2pIdentitySignType {
            crate::p2p_identity::P2pIdentitySignType::Ed25519
        }

        fn sign(&self, _message: &[u8]) -> P2pResult<P2pSignature> {
            Ok(vec![1; 64])
        }

        fn get_encoded_identity(&self) -> P2pResult<EncodedP2pIdentity> {
            Ok(self.id.as_slice().to_vec())
        }

        fn endpoints(&self) -> Vec<Endpoint> {
            vec![]
        }

        fn update_endpoints(&self, _eps: Vec<Endpoint>) -> P2pIdentityRef {
            Arc::new(self.clone())
        }
    }

    struct FakeCachedTunnel {
        local_id: P2pId,
        remote_id: P2pId,
        stream: StdMutex<Option<(TunnelStreamRead, TunnelStreamWrite)>>,
    }

    impl FakeCachedTunnel {
        fn new(
            local_id: P2pId,
            remote_id: P2pId,
        ) -> (Arc<Self>, TunnelStreamRead, TunnelStreamWrite) {
            let (client_stream, remote_stream) = tokio::io::duplex(1024);
            let (client_read, client_write) = split(client_stream);
            let (remote_read, remote_write) = split(remote_stream);
            (
                Arc::new(Self {
                    local_id,
                    remote_id,
                    stream: StdMutex::new(Some((Box::pin(client_read), Box::pin(client_write)))),
                }),
                Box::pin(remote_read),
                Box::pin(remote_write),
            )
        }
    }

    #[async_trait::async_trait]
    impl Tunnel for FakeCachedTunnel {
        fn tunnel_id(&self) -> TunnelId {
            TunnelId::from(900)
        }

        fn candidate_id(&self) -> TunnelCandidateId {
            TunnelCandidateId::from(900)
        }

        fn form(&self) -> TunnelForm {
            TunnelForm::Active
        }

        fn is_reverse(&self) -> bool {
            false
        }

        fn protocol(&self) -> Protocol {
            Protocol::Ext(99)
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
            TunnelState::Connected
        }

        fn is_closed(&self) -> bool {
            false
        }

        async fn close(&self) -> P2pResult<()> {
            Ok(())
        }

        async fn listen_stream(&self, _vports: ListenVPortsRef) -> P2pResult<()> {
            Ok(())
        }

        async fn listen_datagram(&self, _vports: ListenVPortsRef) -> P2pResult<()> {
            Ok(())
        }

        async fn open_stream(
            &self,
            _purpose: TunnelPurpose,
        ) -> P2pResult<(TunnelStreamRead, TunnelStreamWrite)> {
            self.stream
                .lock()
                .unwrap()
                .take()
                .ok_or_else(|| p2p_err!(P2pErrorCode::Interrupted, "stream already opened"))
        }

        async fn accept_stream(
            &self,
        ) -> P2pResult<(TunnelPurpose, TunnelStreamRead, TunnelStreamWrite)> {
            Err(p2p_err!(P2pErrorCode::NotSupport, "no accept"))
        }

        async fn open_datagram(&self, _purpose: TunnelPurpose) -> P2pResult<TunnelDatagramWrite> {
            Err(p2p_err!(P2pErrorCode::NotSupport, "no datagram"))
        }

        async fn accept_datagram(&self) -> P2pResult<(TunnelPurpose, TunnelDatagramRead)> {
            Err(p2p_err!(P2pErrorCode::NotSupport, "no datagram"))
        }
    }

    #[tokio::test]
    async fn pn_client_listener_infos_empty_until_listen() {
        Executor::init();
        let local_identity: P2pIdentityRef = Arc::new(FakeIdentity::new(9));
        let net_manager =
            NetManager::new(vec![], crate::tls::DefaultTlsServerCertResolver::new()).unwrap();
        let ttp_client = crate::ttp::TtpClient::new(local_identity, net_manager);
        let client = PnClient::new(ttp_client);

        assert!(client.listener_infos().is_empty());

        let local = pn_virtual_endpoint();
        client
            .listen(&local, None, None, Arc::new(|_| Box::pin(async {})))
            .await
            .unwrap();

        assert_eq!(
            client.listener_infos(),
            vec![TunnelListenerInfo {
                local,
                mapping_port: None
            }]
        );
    }

    #[tokio::test]
    async fn pn_tunnel_create_waits_for_control_ready() {
        let local_identity: P2pIdentityRef = Arc::new(FakeIdentity::new(1));
        let remote_id = P2pId::from(vec![2; 32]);
        let net_manager =
            NetManager::new(vec![], crate::tls::DefaultTlsServerCertResolver::new()).unwrap();
        let ttp_client = crate::ttp::TtpClient::new(local_identity.clone(), net_manager);
        let (cached, mut relay_read, mut relay_write) =
            FakeCachedTunnel::new(local_identity.get_id(), remote_id.clone());
        ttp_client.remember_tunnel_for_test(cached);
        let client = PnClient::new(ttp_client);

        let create_task = tokio::spawn({
            let client = client.clone();
            let local_identity = local_identity.clone();
            let remote_id = remote_id.clone();
            async move {
                client
                    .create_tunnel_with_intent(
                        &local_identity,
                        &pn_virtual_endpoint(),
                        &remote_id,
                        None,
                        crate::networks::TunnelConnectIntent::default(),
                    )
                    .await
            }
        });

        let req = read_pn_command::<_, ProxyControlOpenReq>(&mut relay_read)
            .await
            .unwrap();
        assert_eq!(req.to, remote_id);
        assert!(!create_task.is_finished());
        assert!(
            client
                .get_tunnel_for_test(&remote_id, req.tunnel_id)
                .is_none()
        );

        write_pn_command(
            &mut relay_write,
            ProxyControlOpenResp {
                tunnel_id: req.tunnel_id,
                result: TunnelCommandResult::Success as u8,
            },
        )
        .await
        .unwrap();

        let tunnel = timeout(Duration::from_secs(1), create_task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(tunnel.tunnel_id(), req.tunnel_id);
        assert!(
            client
                .get_tunnel_for_test(&remote_id, req.tunnel_id)
                .is_some()
        );
    }

    #[tokio::test]
    async fn pn_tunnel_control_ready_failure_fails_create() {
        let local_identity: P2pIdentityRef = Arc::new(FakeIdentity::new(3));
        let remote_id = P2pId::from(vec![4; 32]);
        let net_manager =
            NetManager::new(vec![], crate::tls::DefaultTlsServerCertResolver::new()).unwrap();
        let ttp_client = crate::ttp::TtpClient::new(local_identity.clone(), net_manager);
        let (cached, mut relay_read, mut relay_write) =
            FakeCachedTunnel::new(local_identity.get_id(), remote_id.clone());
        ttp_client.remember_tunnel_for_test(cached);
        let client = PnClient::new(ttp_client);

        let create_task = tokio::spawn({
            let client = client.clone();
            let local_identity = local_identity.clone();
            let remote_id = remote_id.clone();
            async move {
                client
                    .create_tunnel_with_intent(
                        &local_identity,
                        &pn_virtual_endpoint(),
                        &remote_id,
                        None,
                        crate::networks::TunnelConnectIntent::default(),
                    )
                    .await
            }
        });

        let req = read_pn_command::<_, ProxyControlOpenReq>(&mut relay_read)
            .await
            .unwrap();
        write_pn_command(
            &mut relay_write,
            ProxyControlOpenResp {
                tunnel_id: req.tunnel_id,
                result: TunnelCommandResult::ListenerClosed as u8,
            },
        )
        .await
        .unwrap();

        let err = timeout(Duration::from_secs(1), create_task)
            .await
            .unwrap()
            .unwrap()
            .err()
            .unwrap();
        assert_eq!(err.code(), P2pErrorCode::Interrupted);
        assert!(
            client
                .get_tunnel_for_test(&remote_id, req.tunnel_id)
                .is_none()
        );
    }
}

pub fn pn_virtual_endpoint() -> Endpoint {
    Endpoint::from((Protocol::Ext(1), "0.0.0.0:0".parse().unwrap()))
}
