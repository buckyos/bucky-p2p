use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use crate::endpoint::{Endpoint, Protocol};
use crate::error::{P2pErrorCode, P2pResult, into_p2p_err, p2p_err};
use crate::networks::{
    TunnelCommand, TunnelCommandBody, TunnelCommandResult, TunnelListenerInfo, TunnelListenerRef,
    TunnelNetwork, TunnelPurpose, TunnelRef, TunnelStreamRead, TunnelStreamWrite,
    read_tunnel_command_body, read_tunnel_command_header, write_tunnel_command,
};
use crate::p2p_identity::{P2pId, P2pIdentityCertFactoryRef, P2pIdentityRef};
use crate::pn::{PROXY_SERVICE, PnChannelKind, ProxyOpenReq, ProxyOpenResp};
use crate::runtime;
use crate::ttp::{TtpClientRef, TtpPortListener};
use crate::types::{TunnelId, TunnelIdGenerator};

use super::pn_listener::PnListener;
use super::pn_tunnel::{PnProxyStreamSecurityMode, PnTlsContext, PnTunnel, PnTunnelOptions};

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

    pub(super) fn dispatch_or_create_passive_tunnel(
        self: &Arc<Self>,
        key: PnTunnelKey,
        req: ProxyOpenReq,
        read: TunnelStreamRead,
        write: TunnelStreamWrite,
    ) -> PassiveTunnelDispatch {
        let mut tunnels = self.tunnels.lock().unwrap();
        if let Some(tunnel) = tunnels.get(&key).and_then(Weak::upgrade) {
            if !tunnel.is_closed_flag() {
                match tunnel.push_passive_channel(req, read, write) {
                    Ok(()) => return PassiveTunnelDispatch::Dispatched,
                    Err(rejected) => {
                        if tunnels
                            .get(&key)
                            .and_then(Weak::upgrade)
                            .is_none_or(|current| Arc::ptr_eq(&current, &tunnel))
                        {
                            tunnels.remove(&key);
                        }
                        let tunnel = PnTunnel::new_passive(
                            self.local_id(),
                            rejected.request,
                            rejected.read,
                            rejected.write,
                            Some(self.clone()),
                            self.tls_context(),
                            self.stream_security_mode(),
                        );
                        tunnels.insert(key, Arc::downgrade(&tunnel));
                        return PassiveTunnelDispatch::Created(tunnel);
                    }
                }
            }
            tunnels.remove(&key);
        } else {
            tunnels.remove(&key);
        }

        let tunnel = PnTunnel::new_passive(
            self.local_id(),
            req,
            read,
            write,
            Some(self.clone()),
            self.tls_context(),
            self.stream_security_mode(),
        );
        tunnels.insert(key, Arc::downgrade(&tunnel));
        PassiveTunnelDispatch::Created(tunnel)
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
    Created(Arc<PnTunnel>),
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
            listener_infos: Mutex::new(vec![TunnelListenerInfo {
                local: pn_virtual_endpoint(),
                mapping_port: None,
            }]),
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
        self.shared
            .register_tunnel(PnShared::tunnel_key(remote_id.clone(), tunnel_id), &tunnel);
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
    ) -> P2pResult<TunnelListenerRef> {
        {
            let listener = self.listener.lock().unwrap();
            if let Some(listener) = listener.as_ref() {
                log::debug!(
                    "pn client listen reuse local_id={} local_ep={} proxy_service={} mapping_port={:?}",
                    self.shared.local_id(),
                    local,
                    PROXY_SERVICE,
                    mapping_port
                );
                return Ok(listener.clone());
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
        log::debug!(
            "pn client listen ready local_id={} local_ep={} proxy_service={} protocol={:?}",
            self.shared.local_id(),
            local,
            PROXY_SERVICE,
            self.protocol()
        );
        Ok(listener)
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
        log::debug!(
            "pn client listener closed local_id={} proxy_service={}",
            self.shared.local_id(),
            PROXY_SERVICE
        );
        Ok(())
    }

    fn listeners(&self) -> Vec<TunnelListenerRef> {
        self.listener
            .lock()
            .unwrap()
            .as_ref()
            .cloned()
            .into_iter()
            .collect()
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
        self.shared
            .register_tunnel(PnShared::tunnel_key(remote_id.clone(), tunnel_id), &tunnel);
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

pub fn pn_virtual_endpoint() -> Endpoint {
    Endpoint::from((Protocol::Ext(1), "0.0.0.0:0".parse().unwrap()))
}
