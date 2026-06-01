use crate::endpoint::{Endpoint, Protocol};
use crate::error::{P2pError, P2pErrorCode, P2pResult, p2p_err};
use crate::p2p_identity::{P2pId, P2pIdentityRef};
use crate::tls::ServerCertResolverRef;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use super::{
    IncomingTunnelCallback, IncomingTunnelValidateContext, IncomingTunnelValidatorRef,
    TunnelListenerInfo, TunnelNetworkRef, ValidateResult, allow_all_incoming_tunnel_validator,
};

pub type IncomingTunnelSubscriber = Arc<
    dyn Fn(P2pResult<super::TunnelRef>) -> Pin<Box<dyn Future<Output = bool> + Send>> + Send + Sync,
>;

pub struct NetManager {
    cert_resolver: ServerCertResolverRef,
    incoming_tunnel_validator: IncomingTunnelValidatorRef,
    tunnel_networks: HashMap<Protocol, TunnelNetworkRef>,
    listener_meta: Mutex<HashMap<Protocol, Vec<TunnelListenerInfo>>>,
    subscriptions: RwLock<HashMap<P2pId, IncomingTunnelSubscriber>>,
    is_listening: AtomicBool,
    channel_capacity: usize,
}

pub type NetManagerRef = Arc<NetManager>;

impl NetManager {
    pub fn new(
        tunnel_networks: Vec<TunnelNetworkRef>,
        cert_resolver: ServerCertResolverRef,
        channel_capacity: usize,
    ) -> P2pResult<NetManagerRef> {
        Self::new_with_incoming_tunnel_validator(
            tunnel_networks,
            cert_resolver,
            allow_all_incoming_tunnel_validator(),
            channel_capacity,
        )
    }

    pub fn new_with_incoming_tunnel_validator(
        tunnel_networks: Vec<TunnelNetworkRef>,
        cert_resolver: ServerCertResolverRef,
        incoming_tunnel_validator: IncomingTunnelValidatorRef,
        channel_capacity: usize,
    ) -> P2pResult<NetManagerRef> {
        let tunnel_networks = tunnel_networks
            .into_iter()
            .map(|network| (network.protocol(), network))
            .collect::<HashMap<Protocol, TunnelNetworkRef>>();
        Ok(Arc::new(Self {
            cert_resolver,
            incoming_tunnel_validator,
            tunnel_networks,
            listener_meta: Mutex::new(HashMap::new()),
            subscriptions: RwLock::new(HashMap::new()),
            is_listening: AtomicBool::new(false),
            channel_capacity,
        }))
    }

    pub fn channel_capacity(&self) -> usize {
        self.channel_capacity
    }

    pub async fn listen(
        self: &Arc<Self>,
        endpoints: &[Endpoint],
        port_mapping: Option<Vec<(Endpoint, u16)>>,
    ) -> P2pResult<()> {
        if self.is_listening.load(Ordering::SeqCst) {
            self.refresh_listener_meta();
            return Ok(());
        }

        let ep_len = endpoints.len();
        if ep_len == 0 {
            return Err(p2p_err!(P2pErrorCode::InvalidParam, "no endpoint"));
        }

        let mut port_mapping = port_mapping.unwrap_or_default();
        let mut ep_index = 0;
        while ep_index < ep_len {
            let ep = &endpoints[ep_index];
            let ep_pair = if ep.is_mapped_wan() {
                let local_index = ep_index + 1;
                let ep_pair = if local_index == ep_len {
                    Err(P2pError::new(
                        P2pErrorCode::InvalidParam,
                        format!("mapped wan endpoint {} has no local endpoint", ep),
                    ))
                } else {
                    let local_ep = &endpoints[local_index];
                    if !(local_ep.is_same_ip_version(ep)
                        && local_ep.protocol() == ep.protocol()
                        && !local_ep.is_static_wan())
                    {
                        Err(P2pError::new(
                            P2pErrorCode::InvalidParam,
                            format!(
                                "mapped wan endpoint {} has invalid local endpoint {}",
                                ep, local_ep
                            ),
                        ))
                    } else {
                        Ok((*local_ep, Some(*ep)))
                    }
                };
                ep_index = local_index;
                ep_pair
            } else {
                Ok((*ep, None))
            };
            ep_index += 1;

            let (local, out) = ep_pair?;
            let mapping_port = take_mapping_port(&mut port_mapping, ep);
            let network = self
                .get_network(local.protocol())
                .map_err(|_| p2p_err!(P2pErrorCode::NotFound, "network not found: {}", local))?;
            let on_incoming_tunnel = self.incoming_tunnel_callback();
            network
                .listen(&local, out, mapping_port, on_incoming_tunnel)
                .await?;
        }

        self.refresh_listener_meta();
        self.is_listening.store(true, Ordering::SeqCst);
        Ok(())
    }

    pub fn get_network(&self, protocol: Protocol) -> P2pResult<TunnelNetworkRef> {
        self.tunnel_networks.get(&protocol).cloned().ok_or_else(|| {
            p2p_err!(
                P2pErrorCode::NotFound,
                "no network for protocol {:?}",
                protocol
            )
        })
    }

    pub fn get_listener_info(&self, protocol: Protocol) -> Vec<TunnelListenerInfo> {
        self.listener_meta
            .lock()
            .unwrap()
            .get(&protocol)
            .cloned()
            .unwrap_or_default()
    }

    pub fn listener_info_entries(&self) -> Vec<(Protocol, Vec<TunnelListenerInfo>)> {
        self.listener_meta
            .lock()
            .unwrap()
            .iter()
            .map(|(protocol, listeners)| (*protocol, listeners.clone()))
            .collect()
    }

    pub fn protocols(&self) -> Vec<Protocol> {
        self.tunnel_networks.keys().copied().collect()
    }

    pub fn incoming_tunnel_callback(self: &Arc<Self>) -> IncomingTunnelCallback {
        let manager = self.clone();
        Arc::new(move |result| {
            let manager = manager.clone();
            Box::pin(async move {
                manager.dispatch_tunnel_result(result).await;
            })
        })
    }

    pub async fn add_listen_device(&self, device: P2pIdentityRef) -> P2pResult<()> {
        self.cert_resolver.add_server_identity(device).await
    }

    pub async fn remove_listen_device(&self, device_id: &str) -> P2pResult<()> {
        self.cert_resolver.remove_server_identity(device_id).await
    }

    pub async fn get_listen_device(&self, device_id: &str) -> Option<P2pIdentityRef> {
        self.cert_resolver.get_server_identity(device_id).await
    }

    pub fn register_incoming_tunnel_subscriber(
        &self,
        local_id: P2pId,
        callback: IncomingTunnelSubscriber,
    ) -> P2pResult<()> {
        let mut subscriptions = self.subscriptions.write().unwrap();
        if subscriptions.contains_key(&local_id) {
            return Err(p2p_err!(
                P2pErrorCode::AlreadyExists,
                "tunnel acceptor already exists for {}",
                local_id
            ));
        }
        log::debug!("register incoming tunnel subscriber local_id={}", local_id);
        subscriptions.insert(local_id, callback);
        Ok(())
    }

    pub fn unregister_incoming_tunnel_subscriber(&self, local_id: &P2pId) {
        log::debug!(
            "unregister incoming tunnel subscriber local_id={}",
            local_id
        );
        self.subscriptions.write().unwrap().remove(local_id);
    }

    fn refresh_listener_meta(&self) {
        let mut listener_meta = self.listener_meta.lock().unwrap();
        listener_meta.clear();
        for (protocol, network) in &self.tunnel_networks {
            listener_meta.insert(*protocol, network.listener_infos());
        }
    }

    async fn dispatch_tunnel_result(&self, result: P2pResult<super::TunnelRef>) {
        match result {
            Ok(tunnel) => self.dispatch_tunnel(tunnel).await,
            Err(err) => log::warn!("accept tunnel failed: {:?}", err),
        }
    }

    async fn dispatch_tunnel(&self, tunnel: super::TunnelRef) {
        let ctx = Self::build_validate_context(&tunnel);
        log::debug!(
            "dispatch incoming tunnel local={} remote={} protocol={:?} tunnel_id={:?} candidate_id={:?} reverse={} local_ep={:?} remote_ep={:?}",
            ctx.local_id,
            ctx.remote_id,
            ctx.protocol,
            ctx.tunnel_id,
            ctx.candidate_id,
            ctx.is_reverse,
            ctx.local_ep,
            ctx.remote_ep
        );
        match self.incoming_tunnel_validator.validate(&ctx).await {
            Ok(ValidateResult::Accept) => self.publish_tunnel(ctx.local_id, tunnel).await,
            Ok(ValidateResult::Reject(reason)) => {
                log::warn!(
                    "incoming tunnel rejected local={} remote={} protocol={:?} tunnel_id={:?} candidate_id={:?} reason={}",
                    ctx.local_id,
                    ctx.remote_id,
                    ctx.protocol,
                    ctx.tunnel_id,
                    ctx.candidate_id,
                    reason
                );
                self.close_tunnel(tunnel).await;
            }
            Err(err) => {
                log::error!(
                    "incoming tunnel validator failed local={} remote={} protocol={:?} tunnel_id={:?} candidate_id={:?} code={:?} msg={}",
                    ctx.local_id,
                    ctx.remote_id,
                    ctx.protocol,
                    ctx.tunnel_id,
                    ctx.candidate_id,
                    err.code(),
                    err.msg()
                );
                self.close_tunnel(tunnel).await;
            }
        }
    }

    fn build_validate_context(tunnel: &super::TunnelRef) -> IncomingTunnelValidateContext {
        IncomingTunnelValidateContext {
            local_id: tunnel.local_id(),
            remote_id: tunnel.remote_id(),
            protocol: tunnel.protocol(),
            tunnel_id: tunnel.tunnel_id(),
            candidate_id: tunnel.candidate_id(),
            is_reverse: tunnel.is_reverse(),
            local_ep: tunnel.local_ep(),
            remote_ep: tunnel.remote_ep(),
        }
    }

    async fn publish_tunnel(&self, local_id: P2pId, tunnel: super::TunnelRef) {
        let subscriber = { self.subscriptions.read().unwrap().get(&local_id).cloned() };
        if let Some(subscriber) = subscriber {
            log::debug!(
                "publish incoming tunnel local={} remote={} protocol={:?} tunnel_id={:?} candidate_id={:?}",
                local_id,
                tunnel.remote_id(),
                tunnel.protocol(),
                tunnel.tunnel_id(),
                tunnel.candidate_id()
            );
            if !subscriber(Ok(tunnel)).await {
                log::warn!(
                    "publish incoming tunnel failed because subscriber closed local={}",
                    local_id
                );
                let mut subscriptions = self.subscriptions.write().unwrap();
                if let Some(current) = subscriptions.get(&local_id) {
                    if Arc::ptr_eq(current, &subscriber) {
                        subscriptions.remove(&local_id);
                    }
                }
            }
        } else {
            log::warn!(
                "drop incoming tunnel because no subscriber local={} remote={} protocol={:?} tunnel_id={:?} candidate_id={:?}",
                local_id,
                tunnel.remote_id(),
                tunnel.protocol(),
                tunnel.tunnel_id(),
                tunnel.candidate_id()
            );
        }
    }

    async fn close_tunnel(&self, tunnel: super::TunnelRef) {
        if let Err(err) = tunnel.close().await {
            log::warn!(
                "close rejected tunnel failed local={} remote={} protocol={:?} code={:?} msg={}",
                tunnel.local_id(),
                tunnel.remote_id(),
                tunnel.protocol(),
                err.code(),
                err.msg()
            );
        }
    }
}

fn take_mapping_port(port_mapping: &mut Vec<(Endpoint, u16)>, src: &Endpoint) -> Option<u16> {
    let index = port_mapping.iter().position(|(ep, _)| ep == src)?;
    Some(port_mapping.remove(index).1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::networks::{
        IncomingTunnelValidator, ListenVPortsRef, Tunnel, TunnelDatagramRead, TunnelDatagramWrite,
        TunnelForm, TunnelPurpose, TunnelRef, TunnelState, TunnelStreamRead, TunnelStreamWrite,
    };
    use crate::tls::DefaultTlsServerCertResolver;
    use crate::types::{TunnelCandidateId, TunnelId};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::mpsc;
    use tokio::time::{Duration, timeout};

    const TEST_CHANNEL_CAPACITY: usize = 8;

    enum TestDecision {
        Accept,
        Reject,
        Error,
    }

    struct TestValidator {
        decisions: Mutex<HashMap<P2pId, TestDecision>>,
    }

    impl TestValidator {
        fn new(decisions: HashMap<P2pId, TestDecision>) -> Arc<Self> {
            Arc::new(Self {
                decisions: Mutex::new(decisions),
            })
        }
    }

    #[async_trait::async_trait]
    impl IncomingTunnelValidator for TestValidator {
        async fn validate(&self, ctx: &IncomingTunnelValidateContext) -> P2pResult<ValidateResult> {
            let decision = self
                .decisions
                .lock()
                .unwrap()
                .remove(&ctx.remote_id)
                .unwrap_or(TestDecision::Accept);
            match decision {
                TestDecision::Accept => Ok(ValidateResult::Accept),
                TestDecision::Reject => Ok(ValidateResult::Reject(format!(
                    "remote {} is not allowed to connect local {}",
                    ctx.remote_id, ctx.local_id
                ))),
                TestDecision::Error => Err(P2pError::new(
                    P2pErrorCode::InternalError,
                    format!("validator failed for remote {}", ctx.remote_id),
                )),
            }
        }
    }

    struct TestTunnel {
        local_id: P2pId,
        remote_id: P2pId,
        tunnel_id: TunnelId,
        candidate_id: TunnelCandidateId,
        close_count: AtomicUsize,
    }

    impl TestTunnel {
        fn new(local_id: P2pId, remote_id: P2pId, tunnel_id: u32, candidate_id: u32) -> Arc<Self> {
            Arc::new(Self {
                local_id,
                remote_id,
                tunnel_id: TunnelId::from(tunnel_id),
                candidate_id: TunnelCandidateId::from(candidate_id),
                close_count: AtomicUsize::new(0),
            })
        }

        fn close_count(&self) -> usize {
            self.close_count.load(Ordering::SeqCst)
        }
    }

    #[async_trait::async_trait]
    impl Tunnel for TestTunnel {
        fn tunnel_id(&self) -> TunnelId {
            self.tunnel_id
        }

        fn candidate_id(&self) -> TunnelCandidateId {
            self.candidate_id
        }

        fn form(&self) -> TunnelForm {
            TunnelForm::Passive
        }

        fn is_reverse(&self) -> bool {
            false
        }

        fn protocol(&self) -> Protocol {
            Protocol::Tcp
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
            self.close_count() > 0
        }

        async fn close(&self) -> P2pResult<()> {
            self.close_count.fetch_add(1, Ordering::SeqCst);
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
            Err(p2p_err!(P2pErrorCode::NotSupport, "test tunnel"))
        }

        async fn accept_stream(
            &self,
        ) -> P2pResult<(TunnelPurpose, TunnelStreamRead, TunnelStreamWrite)> {
            Err(p2p_err!(P2pErrorCode::NotSupport, "test tunnel"))
        }

        async fn open_datagram(&self, _purpose: TunnelPurpose) -> P2pResult<TunnelDatagramWrite> {
            Err(p2p_err!(P2pErrorCode::NotSupport, "test tunnel"))
        }

        async fn accept_datagram(&self) -> P2pResult<(TunnelPurpose, TunnelDatagramRead)> {
            Err(p2p_err!(P2pErrorCode::NotSupport, "test tunnel"))
        }
    }

    fn test_id(seed: u8) -> P2pId {
        P2pId::from(vec![seed; 32])
    }

    fn new_test_manager(validator: IncomingTunnelValidatorRef) -> NetManagerRef {
        NetManager::new_with_incoming_tunnel_validator(
            vec![],
            DefaultTlsServerCertResolver::new(),
            validator,
            TEST_CHANNEL_CAPACITY,
        )
        .unwrap()
    }

    fn register_test_acceptor(
        manager: &NetManagerRef,
        local_id: P2pId,
    ) -> mpsc::Receiver<P2pResult<TunnelRef>> {
        let (tx, rx) = mpsc::channel(TEST_CHANNEL_CAPACITY);
        manager
            .register_incoming_tunnel_subscriber(
                local_id,
                Arc::new(move |result| {
                    let tx = tx.clone();
                    Box::pin(async move { tx.send(result).await.is_ok() })
                }),
            )
            .unwrap();
        rx
    }

    async fn accept_test_tunnel(
        acceptor: &mut mpsc::Receiver<P2pResult<TunnelRef>>,
    ) -> P2pResult<TunnelRef> {
        acceptor
            .recv()
            .await
            .ok_or_else(|| p2p_err!(P2pErrorCode::Interrupted, "test acceptor closed"))?
    }

    #[tokio::test]
    async fn dispatch_tunnel_accepts_when_validator_allows() {
        let manager = new_test_manager(TestValidator::new(HashMap::new()));
        let local_id = test_id(1);
        let remote_id = test_id(2);
        let mut acceptor = register_test_acceptor(&manager, local_id.clone());
        let tunnel = TestTunnel::new(local_id, remote_id, 1, 11);
        let tunnel_ref: TunnelRef = tunnel.clone();

        manager.dispatch_tunnel(tunnel_ref.clone()).await;

        let accepted = accept_test_tunnel(&mut acceptor).await.unwrap();
        assert!(Arc::ptr_eq(&accepted, &tunnel_ref));
        assert_eq!(tunnel.close_count(), 0);
    }

    #[tokio::test]
    async fn dispatch_tunnel_rejects_when_validator_denies() {
        let local_id = test_id(3);
        let remote_id = test_id(4);
        let manager = new_test_manager(TestValidator::new(HashMap::from([(
            remote_id.clone(),
            TestDecision::Reject,
        )])));
        let mut acceptor = register_test_acceptor(&manager, local_id.clone());
        let tunnel = TestTunnel::new(local_id, remote_id, 2, 12);

        manager.dispatch_tunnel(tunnel.clone()).await;

        assert!(
            timeout(
                Duration::from_millis(100),
                accept_test_tunnel(&mut acceptor)
            )
            .await
            .is_err()
        );
        assert_eq!(tunnel.close_count(), 1);
    }

    #[tokio::test]
    async fn dispatch_tunnel_rejects_validator_errors_without_breaking_future_dispatch() {
        let local_id = test_id(5);
        let error_remote_id = test_id(6);
        let accepted_remote_id = test_id(7);
        let manager = new_test_manager(TestValidator::new(HashMap::from([
            (error_remote_id.clone(), TestDecision::Error),
            (accepted_remote_id.clone(), TestDecision::Accept),
        ])));
        let mut acceptor = register_test_acceptor(&manager, local_id.clone());
        let failed_tunnel = TestTunnel::new(local_id.clone(), error_remote_id, 3, 13);
        let accepted_tunnel = TestTunnel::new(local_id, accepted_remote_id, 4, 14);
        let accepted_tunnel_ref: TunnelRef = accepted_tunnel.clone();

        manager.dispatch_tunnel(failed_tunnel.clone()).await;
        manager.dispatch_tunnel(accepted_tunnel_ref.clone()).await;

        let accepted = accept_test_tunnel(&mut acceptor).await.unwrap();
        assert!(Arc::ptr_eq(&accepted, &accepted_tunnel_ref));
        assert_eq!(failed_tunnel.close_count(), 1);
        assert_eq!(accepted_tunnel.close_count(), 0);
    }

    #[tokio::test]
    async fn dispatch_tunnel_result_continues_after_recoverable_accept_error() {
        let manager = new_test_manager(TestValidator::new(HashMap::new()));
        let local_id = test_id(8);
        let remote_id = test_id(9);
        let mut acceptor = register_test_acceptor(&manager, local_id.clone());
        let accepted_tunnel = TestTunnel::new(local_id, remote_id, 5, 15);
        let accepted_tunnel_ref: TunnelRef = accepted_tunnel.clone();

        manager
            .dispatch_tunnel_result(Err(P2pError::new(
                P2pErrorCode::QuicError,
                "transient accept failure".to_owned(),
            )))
            .await;
        manager
            .dispatch_tunnel_result(Ok(accepted_tunnel_ref.clone()))
            .await;

        let accepted = timeout(Duration::from_secs(1), accept_test_tunnel(&mut acceptor))
            .await
            .unwrap()
            .unwrap();
        assert!(Arc::ptr_eq(&accepted, &accepted_tunnel_ref));
        assert_eq!(accepted_tunnel.close_count(), 0);
    }

    #[tokio::test]
    async fn incoming_tunnel_callback_dispatches_results() {
        let manager = new_test_manager(TestValidator::new(HashMap::new()));
        let local_id = test_id(10);
        let remote_id = test_id(11);
        let mut acceptor = register_test_acceptor(&manager, local_id.clone());
        let callback = manager.incoming_tunnel_callback();
        let accepted_tunnel = TestTunnel::new(local_id, remote_id, 6, 16);
        let accepted_tunnel_ref: TunnelRef = accepted_tunnel.clone();

        callback(Err(P2pError::new(
            P2pErrorCode::QuicError,
            "transient accept failure".to_owned(),
        )))
        .await;
        callback(Ok(accepted_tunnel_ref.clone())).await;

        let accepted = timeout(Duration::from_secs(1), accept_test_tunnel(&mut acceptor))
            .await
            .unwrap()
            .unwrap();
        assert!(Arc::ptr_eq(&accepted, &accepted_tunnel_ref));
        assert_eq!(accepted_tunnel.close_count(), 0);
    }
}
