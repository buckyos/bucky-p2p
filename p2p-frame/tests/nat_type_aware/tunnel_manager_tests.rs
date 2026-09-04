use super::*;
use crate::nat_type::NatProfile;
use std::sync::atomic::AtomicBool;

struct PunchDropFlag(Arc<AtomicBool>);

impl Drop for PunchDropFlag {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

struct PendingPunchNetwork {
    started: Arc<AtomicBool>,
    dropped: Arc<AtomicBool>,
}

struct PredictionValidationNetwork {
    current_generation: u64,
    closed: AtomicBool,
}

impl PredictionValidationNetwork {
    fn new(current_generation: u64, closed: bool) -> Self {
        Self {
            current_generation,
            closed: AtomicBool::new(closed),
        }
    }
}

#[async_trait::async_trait]
impl TunnelNetwork for PredictionValidationNetwork {
    fn protocol(&self) -> Protocol {
        Protocol::Quic
    }

    fn is_udp(&self) -> bool {
        true
    }

    fn as_udp_tunnel_network(&self) -> Option<&dyn UdpTunnelNetwork> {
        Some(self)
    }

    async fn listen(
        &self,
        _local: &Endpoint,
        _out: Option<Endpoint>,
        _mapping_port: Option<u16>,
        _on_incoming_tunnel: IncomingTunnelCallback,
    ) -> P2pResult<()> {
        Err(p2p_err!(
            P2pErrorCode::NotSupport,
            "prediction validation mock listen"
        ))
    }

    async fn close_all_listener(&self) -> P2pResult<()> {
        self.closed.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn listener_infos(&self) -> Vec<TunnelListenerInfo> {
        vec![]
    }

    async fn create_tunnel_with_intent(
        &self,
        _local_identity: &P2pIdentityRef,
        _remote: &Endpoint,
        _remote_id: &P2pId,
        _remote_name: Option<String>,
        _intent: TunnelConnectIntent,
    ) -> P2pResult<TunnelRef> {
        Err(p2p_err!(
            P2pErrorCode::NotSupport,
            "prediction validation mock connect"
        ))
    }

    async fn create_tunnel_with_local_ep_and_intent(
        &self,
        local_identity: &P2pIdentityRef,
        _local_ep: &Endpoint,
        remote: &Endpoint,
        remote_id: &P2pId,
        remote_name: Option<String>,
        intent: TunnelConnectIntent,
    ) -> P2pResult<TunnelRef> {
        self.create_tunnel_with_intent(local_identity, remote, remote_id, remote_name, intent)
            .await
    }

}

#[async_trait::async_trait]
impl UdpTunnelNetwork for PredictionValidationNetwork {
    async fn punch_only(
        &self,
        _remote: &Endpoint,
        _intent: TunnelConnectIntent,
        _max_duration: Duration,
    ) -> P2pResult<()> {
        Err(p2p_err!(
            P2pErrorCode::NotSupport,
            "prediction validation mock does not punch"
        ))
    }

    async fn predict_traversal_endpoints(
        &self,
        _probe_targets: &[Endpoint],
        _expected_signer: &P2pIdentityCertRef,
        _per_target_timeout: Duration,
        _ttl: Duration,
    ) -> P2pResult<crate::networks::TraversalEndpointPrediction> {
        Err(p2p_err!(
            P2pErrorCode::NotSupport,
            "prediction validation mock does not predict"
        ))
    }

    fn validate_traversal_prediction(
        &self,
        prediction: &crate::networks::TraversalEndpointPrediction,
        now: crate::types::Timestamp,
    ) -> P2pResult<()> {
        if prediction.socket_binding_generation != self.current_generation {
            return Err(p2p_err!(
                P2pErrorCode::Expired,
                "prediction belongs to a closed listener generation"
            ));
        }
        if prediction.valid_until < now {
            return Err(p2p_err!(P2pErrorCode::Expired, "prediction expired"));
        }
        if self.closed.load(Ordering::SeqCst) {
            return Err(p2p_err!(
                P2pErrorCode::ErrorState,
                "prediction listener is closed"
            ));
        }
        Ok(())
    }
}

impl PendingPunchNetwork {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            started: Arc::new(AtomicBool::new(false)),
            dropped: Arc::new(AtomicBool::new(false)),
        })
    }

    async fn wait_started(&self) {
        runtime::timeout(Duration::from_secs(1), async {
            while !self.started.load(Ordering::SeqCst) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }
}

#[async_trait::async_trait]
impl TunnelNetwork for PendingPunchNetwork {
    fn protocol(&self) -> Protocol {
        Protocol::Quic
    }

    fn is_udp(&self) -> bool {
        true
    }

    fn as_udp_tunnel_network(&self) -> Option<&dyn UdpTunnelNetwork> {
        Some(self)
    }

    async fn listen(
        &self,
        _local: &Endpoint,
        _out: Option<Endpoint>,
        _mapping_port: Option<u16>,
        _on_incoming_tunnel: IncomingTunnelCallback,
    ) -> P2pResult<()> {
        Err(p2p_err!(P2pErrorCode::NotSupport, "mock punch listen"))
    }

    async fn close_all_listener(&self) -> P2pResult<()> {
        Ok(())
    }

    fn listener_infos(&self) -> Vec<TunnelListenerInfo> {
        vec![]
    }

    async fn create_tunnel_with_intent(
        &self,
        _local_identity: &P2pIdentityRef,
        _remote: &Endpoint,
        _remote_id: &P2pId,
        _remote_name: Option<String>,
        _intent: TunnelConnectIntent,
    ) -> P2pResult<TunnelRef> {
        Err(p2p_err!(P2pErrorCode::NotSupport, "mock punch connect"))
    }

    async fn create_tunnel_with_local_ep_and_intent(
        &self,
        local_identity: &P2pIdentityRef,
        _local_ep: &Endpoint,
        remote: &Endpoint,
        remote_id: &P2pId,
        remote_name: Option<String>,
        intent: TunnelConnectIntent,
    ) -> P2pResult<TunnelRef> {
        self.create_tunnel_with_intent(local_identity, remote, remote_id, remote_name, intent)
            .await
    }

}

#[async_trait::async_trait]
impl UdpTunnelNetwork for PendingPunchNetwork {
    async fn punch_only(
        &self,
        _remote: &Endpoint,
        _intent: TunnelConnectIntent,
        _max_duration: Duration,
    ) -> P2pResult<()> {
        let _drop = PunchDropFlag(self.dropped.clone());
        self.started.store(true, Ordering::SeqCst);
        std::future::pending::<()>().await;
        Ok(())
    }

    async fn predict_traversal_endpoints(
        &self,
        _probe_targets: &[Endpoint],
        _expected_signer: &P2pIdentityCertRef,
        _per_target_timeout: Duration,
        _ttl: Duration,
    ) -> P2pResult<crate::networks::TraversalEndpointPrediction> {
        Err(p2p_err!(
            P2pErrorCode::NotSupport,
            "pending punch mock does not predict"
        ))
    }

    fn validate_traversal_prediction(
        &self,
        _prediction: &crate::networks::TraversalEndpointPrediction,
        _now: crate::types::Timestamp,
    ) -> P2pResult<()> {
        Err(p2p_err!(
            P2pErrorCode::NotSupport,
            "pending punch mock does not validate predictions"
        ))
    }
}

fn observed_endpoint(port: u16) -> Endpoint {
    let mut endpoint = Endpoint::from((
        Protocol::Quic,
        "198.51.100.1".parse::<std::net::IpAddr>().unwrap(),
        port,
    ));
    endpoint.set_area(EndpointArea::ServerReflexive);
    endpoint
}

#[test]
fn rendezvous_prediction_validation_accepts_current_listener_generation() {
    let now = bucky_time_now();
    let profile = NatProfile::from_observations(
        &[observed_endpoint(3900), observed_endpoint(3902)],
        now,
        Duration::from_secs(30),
    );
    let prediction = crate::networks::TraversalEndpointPrediction {
        endpoints: vec![observed_endpoint(4900)],
        socket_binding_generation: 17,
        valid_until: profile.valid_until,
        profile,
    };
    let network = PredictionValidationNetwork::new(17, false);

    network
        .validate_traversal_prediction(&prediction, now)
        .unwrap();
}

#[test]
fn rendezvous_prediction_validation_rejects_expired_and_closed_listener_generation() {
    let now = bucky_time_now();
    let profile = NatProfile::from_observations(
        &[observed_endpoint(3910), observed_endpoint(3912)],
        now,
        Duration::from_secs(30),
    );
    let prediction = crate::networks::TraversalEndpointPrediction {
        endpoints: vec![observed_endpoint(4910)],
        socket_binding_generation: 23,
        valid_until: profile.valid_until,
        profile,
    };
    let current = PredictionValidationNetwork::new(23, false);
    let mut expired = prediction.clone();
    expired.valid_until = now.saturating_sub(1);
    assert_eq!(
        current
            .validate_traversal_prediction(&expired, now)
            .unwrap_err()
            .code(),
        P2pErrorCode::Expired
    );

    let closed_generation = PredictionValidationNetwork::new(24, false);
    assert_eq!(
        closed_generation
            .validate_traversal_prediction(&prediction, now)
            .unwrap_err()
            .code(),
        P2pErrorCode::Expired
    );

    let closed_listener = PredictionValidationNetwork::new(23, true);
    assert_eq!(
        closed_listener
            .validate_traversal_prediction(&prediction, now)
            .unwrap_err()
            .code(),
        P2pErrorCode::ErrorState
    );
}

#[test]
fn predicted_candidates_apply_hint_to_current_sn_base_and_obey_total_cap() {
    let now = bucky_time_now();
    let profile = NatProfile::from_observations(
        &[
            observed_endpoint(4000),
            observed_endpoint(4002),
            observed_endpoint(4004),
        ],
        now,
        Duration::from_secs(10),
    );
    let base = observed_endpoint(5000);
    let candidates =
        TunnelManager::nat_candidates(&[base], &profile, NatCandidateMode::Predicted, now);
    assert_eq!(candidates.len(), MAX_NAT_PLAN_CANDIDATES);
    assert_eq!(candidates[0], base);
    assert_eq!(candidates[1].addr().port(), 5002);
    assert_eq!(candidates.last().unwrap().addr().port(), 5014);
    assert!(
        candidates
            .iter()
            .all(|candidate| candidate.get_area() == EndpointArea::ServerReflexive)
    );
}

#[test]
fn predicted_candidates_reject_non_server_reflexive_and_lan_bases() {
    let now = bucky_time_now();
    let profile = NatProfile::from_observations(
        &[
            observed_endpoint(4000),
            observed_endpoint(4002),
            observed_endpoint(4004),
        ],
        now,
        Duration::from_secs(10),
    );
    let mut wan = observed_endpoint(5000);
    wan.set_area(EndpointArea::Wan);
    let mut lan = Endpoint::from((
        Protocol::Quic,
        "192.168.1.5:5000".parse::<SocketAddr>().unwrap(),
    ));
    lan.set_area(EndpointArea::ServerReflexive);

    assert!(
        TunnelManager::nat_candidates(&[wan], &profile, NatCandidateMode::Predicted, now)
            .is_empty()
    );
    assert!(
        TunnelManager::nat_candidates(&[lan], &profile, NatCandidateMode::Predicted, now)
            .is_empty()
    );
}

#[tokio::test]
async fn punch_only_stops_on_incoming_success_and_owner_drop() {
    let local = new_identity("punch-lifecycle-local");
    let remote_id = new_identity("punch-lifecycle-remote").get_id();
    let network = PendingPunchNetwork::new();
    let manager =
        new_test_manager_with_networks(local, HashMap::new(), None, vec![network.clone()]);
    let tunnel_id = next_test_tunnel_id();
    let endpoint = observed_endpoint(5050);
    let (notify, waiter) = Notify::new();
    let mut registration = IncomingPlanWaitRegistration::register(
        manager.as_ref(),
        remote_id.clone(),
        tunnel_id,
        true,
        notify,
    );
    let action_manager = manager.clone();
    let action_remote_id = remote_id.clone();
    let action = tokio::spawn(async move {
        action_manager
            .punch_and_wait_incoming(vec![endpoint], &action_remote_id, tunnel_id, false, waiter)
            .await
    });
    network.wait_started().await;

    let incoming: TunnelRef = Arc::new(MockTunnel {
        tunnel_id,
        candidate_id: TunnelCandidateId::from(5051),
        form: TunnelForm::Active,
        local_id: manager.local_identity.get_id(),
        remote_id: remote_id.clone(),
        state: TunnelState::Connected,
        is_reverse: true,
    });
    assert!(manager.on_incoming_tunnel(incoming.clone()).await.unwrap());
    assert!(Arc::ptr_eq(&action.await.unwrap().unwrap(), &incoming));
    assert!(network.dropped.load(Ordering::SeqCst));
    registration.dismiss();

    let owner_network = PendingPunchNetwork::new();
    let owner_manager = new_test_manager_with_networks(
        new_identity("punch-owner-local"),
        HashMap::new(),
        None,
        vec![owner_network.clone()],
    );
    let owner = tokio::spawn({
        let owner_manager = owner_manager.clone();
        async move {
            owner_manager
                .punch_candidates(vec![observed_endpoint(5052)], next_test_tunnel_id(), false)
                .await;
        }
    });
    owner_network.wait_started().await;
    owner.abort();
    let _ = owner.await;
    assert!(owner_network.dropped.load(Ordering::SeqCst));
}

#[tokio::test]
async fn nat_action_failure_reaches_proxy_fallback_even_when_sn_call_fails() {
    use crate::sn::client::SNClientService;
    use crate::types::SequenceGenerator;

    let local = new_identity("nat-proxy-local");
    let remote = new_identity("nat-proxy-remote");
    let remote_id = remote.get_id();
    let net_manager =
        crate::networks::NetManager::new(vec![], DefaultTlsServerCertResolver::new()).unwrap();
    let sn_client = SNClientService::new(
        net_manager.clone(),
        vec![],
        local.clone(),
        Arc::new(SequenceGenerator::new()),
        Arc::new(TunnelIdGenerator::new()),
        Arc::new(X509IdentityCertFactory),
        1,
        Duration::from_millis(50),
        Duration::from_millis(50),
        Duration::from_millis(50),
    );
    let proxy = MockProxyNetwork::new(local.get_id(), Ok(()));
    let manager = TunnelManager::new(
        local.clone(),
        None,
        net_manager,
        Some(sn_client),
        Arc::new(X509IdentityCertFactory),
        Some(proxy),
        DefaultP2pConnectionInfoCache::new(),
        Arc::new(TunnelIdGenerator::new()),
        Duration::from_millis(100),
        Duration::from_secs(30),
        PROXY_UPGRADE_INITIAL_INTERVAL,
    )
    .unwrap();
    let now = bucky_time_now();
    let profile = NatProfile::from_observations(
        &[observed_endpoint(4100), observed_endpoint(4100)],
        now,
        Duration::from_secs(10),
    );
    let context =
        NatTraversalContext::new(local.get_id(), remote_id.clone(), profile.clone(), profile);
    let plan = select_connect_plan(&context, now, false, false);

    let tunnel = manager
        .open_nat_aware_tunnel(
            vec![Endpoint::from((
                Protocol::Ext(77),
                "127.0.0.1:7700".parse::<SocketAddr>().unwrap(),
            ))],
            &remote_id,
            Some("nat-proxy-remote".to_owned()),
            &P2pId::from(vec![9; 32]),
            context,
            plan,
        )
        .await
        .unwrap();

    assert_eq!(tunnel.form(), TunnelForm::Proxy);
}

#[tokio::test]
async fn rendezvous_deterministic_failure_uses_legacy_predicted_direct_without_proxy() {
    use crate::sn::client::SNClientService;
    use crate::types::SequenceGenerator;

    let now = bucky_time_now();
    let profile = NatProfile::from_observations(
        &[
            observed_endpoint(4200),
            observed_endpoint(4202),
            observed_endpoint(4204),
        ],
        now,
        Duration::from_secs(30),
    );
    let base = observed_endpoint(5200);
    let predicted = observed_endpoint(5202);

    let local = new_identity("predicted-hit-local");
    let remote = new_identity("predicted-hit-remote");
    let remote_id = remote.get_id();
    let hit_dial = MockDialNetwork::new(
        Protocol::Quic,
        local.get_id(),
        HashMap::from([(
            predicted,
            MockDialBehavior {
                delay: Duration::from_millis(10),
                result: Ok(()),
            },
        )]),
    );
    let hit_net_manager = crate::networks::NetManager::new(
        vec![hit_dial.clone()],
        DefaultTlsServerCertResolver::new(),
    )
    .unwrap();
    let hit_sn_client = SNClientService::new(
        hit_net_manager.clone(),
        vec![],
        local.clone(),
        Arc::new(SequenceGenerator::new()),
        Arc::new(TunnelIdGenerator::new()),
        Arc::new(X509IdentityCertFactory),
        1,
        Duration::from_millis(50),
        Duration::from_millis(50),
        Duration::from_millis(50),
    );
    let hit_manager = TunnelManager::new(
        local.clone(),
        None,
        hit_net_manager,
        Some(hit_sn_client),
        Arc::new(X509IdentityCertFactory),
        None,
        DefaultP2pConnectionInfoCache::new(),
        Arc::new(TunnelIdGenerator::new()),
        Duration::from_millis(100),
        Duration::from_secs(30),
        PROXY_UPGRADE_INITIAL_INTERVAL,
    )
    .unwrap();
    let hit_context = NatTraversalContext::new(
        local.get_id(),
        remote_id.clone(),
        profile.clone(),
        profile.clone(),
    );
    let hit_plan = select_connect_plan(&hit_context, now, false, false);
    let hit = hit_manager
        .open_nat_aware_tunnel(
            vec![base],
            &remote_id,
            Some("predicted-hit-remote".to_owned()),
            &P2pId::from(vec![21; 32]),
            hit_context,
            hit_plan,
        )
        .await
        .expect("legacy predicted direct action must remain viable without PN");
    assert_eq!(hit.form(), TunnelForm::Active);
    assert!(
        hit_dial.intent_for(&predicted).is_some(),
        "legacy fallback must derive predicted candidates from the original endpoint"
    );
    assert!(
        hit_dial.call_count() > 0,
        "deterministic rendezvous failure must enter the legacy caller action"
    );
}

#[tokio::test]
async fn rendezvous_deterministic_failure_attempts_legacy_direct_before_proxy() {
    use crate::sn::client::SNClientService;
    use crate::types::SequenceGenerator;

    let miss_local = new_identity("predicted-miss-local");
    let miss_remote_id = new_identity("predicted-miss-remote").get_id();
    let original_lan_endpoint = Endpoint::from((
        Protocol::Quic,
        "192.168.7.9:6200".parse::<SocketAddr>().unwrap(),
    ));
    let miss_dial = MockDialNetwork::new(
        Protocol::Quic,
        miss_local.get_id(),
        HashMap::from([(
            original_lan_endpoint,
            MockDialBehavior {
                delay: Duration::ZERO,
                result: Err(P2pErrorCode::ConnectFailed),
            },
        )]),
    );
    let miss_net_manager =
        crate::networks::NetManager::new(vec![miss_dial.clone()], DefaultTlsServerCertResolver::new())
            .unwrap();
    let miss_sn_client = SNClientService::new(
        miss_net_manager.clone(),
        vec![],
        miss_local.clone(),
        Arc::new(SequenceGenerator::new()),
        Arc::new(TunnelIdGenerator::new()),
        Arc::new(X509IdentityCertFactory),
        1,
        Duration::from_millis(50),
        Duration::from_millis(50),
        Duration::from_millis(50),
    );
    let proxy = MockProxyNetwork::new(miss_local.get_id(), Ok(()));
    let miss_manager = TunnelManager::new(
        miss_local.clone(),
        None,
        miss_net_manager,
        Some(miss_sn_client),
        Arc::new(X509IdentityCertFactory),
        Some(proxy),
        DefaultP2pConnectionInfoCache::new(),
        Arc::new(TunnelIdGenerator::new()),
        Duration::from_millis(100),
        Duration::from_secs(30),
        PROXY_UPGRADE_INITIAL_INTERVAL,
    )
    .unwrap();
    let now = bucky_time_now();
    let stable_profile = NatProfile::from_observations(
        &[observed_endpoint(4200), observed_endpoint(4200)],
        now,
        Duration::from_secs(30),
    );
    let miss_context = NatTraversalContext::new(
        miss_local.get_id(),
        miss_remote_id.clone(),
        stable_profile.clone(),
        stable_profile,
    );
    let miss_plan = select_connect_plan(&miss_context, now, false, false);
    let miss = miss_manager
        .open_nat_aware_tunnel(
            vec![original_lan_endpoint],
            &miss_remote_id,
            Some("predicted-miss-remote".to_owned()),
            &P2pId::from(vec![22; 32]),
            miss_context,
            miss_plan,
        )
        .await
        .unwrap();
    assert_eq!(miss.form(), TunnelForm::Proxy);
    assert_eq!(
        miss_dial.call_count(),
        1,
        "legacy fallback must attempt the full original endpoint set before PN"
    );
    assert!(
        miss_dial.start_offset(&original_lan_endpoint).is_some(),
        "the legacy caller action must be observable before the final Proxy result"
    );
    let registered_fallback = miss_manager.get_tunnel(&miss_remote_id).unwrap();
    assert_eq!(registered_fallback.tunnel_id(), miss.tunnel_id());
    assert_eq!(registered_fallback.form(), TunnelForm::Proxy);
}

#[tokio::test]
async fn rendezvous_and_legacy_fallback_share_one_bounded_total_deadline() {
    use crate::sn::client::SNClientService;
    use crate::types::SequenceGenerator;

    let local = new_identity("bounded-fallback-local");
    let remote_id = new_identity("bounded-fallback-remote").get_id();
    let endpoint = observed_endpoint(6300);
    let dial = MockDialNetwork::new(
        Protocol::Quic,
        local.get_id(),
        HashMap::from([(
            endpoint,
            MockDialBehavior {
                delay: Duration::from_secs(2),
                result: Ok(()),
            },
        )]),
    );
    let net_manager = crate::networks::NetManager::new(
        vec![dial.clone()],
        DefaultTlsServerCertResolver::new(),
    )
    .unwrap();
    let sn_client = SNClientService::new(
        net_manager.clone(),
        vec![],
        local.clone(),
        Arc::new(SequenceGenerator::new()),
        Arc::new(TunnelIdGenerator::new()),
        Arc::new(X509IdentityCertFactory),
        1,
        Duration::from_millis(50),
        Duration::from_millis(50),
        Duration::from_millis(50),
    );
    let manager = TunnelManager::new(
        local.clone(),
        None,
        net_manager,
        Some(sn_client),
        Arc::new(X509IdentityCertFactory),
        None,
        DefaultP2pConnectionInfoCache::new(),
        Arc::new(TunnelIdGenerator::new()),
        Duration::from_millis(100),
        Duration::from_secs(30),
        PROXY_UPGRADE_INITIAL_INTERVAL,
    )
    .unwrap();
    let now = bucky_time_now();
    let stable_profile = NatProfile::from_observations(
        &[observed_endpoint(4300), observed_endpoint(4300)],
        now,
        Duration::from_secs(30),
    );
    let context = NatTraversalContext::new(
        local.get_id(),
        remote_id.clone(),
        stable_profile.clone(),
        stable_profile,
    );
    let plan = select_connect_plan(&context, now, false, false);
    let started = Instant::now();
    let err = manager
        .open_nat_aware_tunnel(
            vec![endpoint],
            &remote_id,
            Some("bounded-fallback-remote".to_owned()),
            &P2pId::from(vec![23; 32]),
            context,
            plan,
        )
        .await
        .err()
        .expect("shared total deadline must expire before the slow legacy dial completes");
    let elapsed = started.elapsed();

    assert_eq!(err.code(), P2pErrorCode::Timeout);
    assert!(dial.call_count() > 0, "legacy fallback action must start");
    assert!(
        elapsed >= Duration::from_millis(150) && elapsed < Duration::from_millis(750),
        "the shared 250ms budget must bound both stages with scheduler slack: {elapsed:?}"
    );
}

#[tokio::test]
async fn direction_aware_waiter_matches_active_tunnel_without_consuming_reverse_key() {
    let manager = new_test_manager(new_identity("direction-local"), HashMap::new(), None);
    let remote = new_identity("direction-remote");
    let remote_id = remote.get_id();
    let tunnel_id = next_test_tunnel_id();
    let (notify, waiter) = Notify::new();
    let mut registration = IncomingPlanWaitRegistration::register(
        manager.as_ref(),
        remote_id.clone(),
        tunnel_id,
        false,
        notify,
    );
    let active: TunnelRef = Arc::new(MockTunnel {
        tunnel_id,
        candidate_id: TunnelCandidateId::from(1),
        form: TunnelForm::Active,
        local_id: manager.local_identity.get_id(),
        remote_id: remote_id.clone(),
        state: TunnelState::Connected,
        is_reverse: false,
    });

    assert!(manager.on_incoming_tunnel(active.clone()).await.unwrap());
    let matched = runtime::timeout(Duration::from_secs(1), waiter)
        .await
        .unwrap()
        .unwrap();
    assert!(Arc::ptr_eq(&matched, &active));
    assert!(
        manager
            .take_incoming_waiter(&remote_id, &tunnel_id, true)
            .is_none()
    );
    registration.dismiss();
}

#[tokio::test]
async fn wrong_direction_does_not_consume_active_plan_waiter() {
    let manager = new_test_manager(new_identity("wrong-direction-local"), HashMap::new(), None);
    let remote_id = new_identity("wrong-direction-remote").get_id();
    let tunnel_id = next_test_tunnel_id();
    let (notify, _waiter) = Notify::new();
    let _registration = IncomingPlanWaitRegistration::register(
        manager.as_ref(),
        remote_id.clone(),
        tunnel_id,
        false,
        notify,
    );
    let reverse: TunnelRef = Arc::new(MockTunnel {
        tunnel_id,
        candidate_id: TunnelCandidateId::from(2),
        form: TunnelForm::Active,
        local_id: manager.local_identity.get_id(),
        remote_id: remote_id.clone(),
        state: TunnelState::Connected,
        is_reverse: true,
    });

    assert!(!manager.on_incoming_tunnel(reverse).await.unwrap());
    assert!(
        manager
            .take_incoming_waiter(&remote_id, &tunnel_id, false)
            .is_some()
    );
}

#[tokio::test]
async fn action_success_does_not_wait_for_sn_call_response_and_cancels_call_owner() {
    struct DropFlag(Arc<AtomicBool>);
    impl Drop for DropFlag {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    let call_dropped = Arc::new(AtomicBool::new(false));
    let started = Instant::now();
    let (result, call_result) = drive_nat_rendezvous_and_action(
        {
            let call_dropped = call_dropped.clone();
            async move {
                let _guard = DropFlag(call_dropped);
                runtime::sleep(Duration::from_secs(5)).await;
                Ok(())
            }
        },
        async {
            runtime::sleep(Duration::from_millis(10)).await;
            Ok::<_, crate::error::P2pError>(7u8)
        },
    )
    .await;

    assert_eq!(result.unwrap(), 7);
    assert!(call_result.is_none());
    assert!(started.elapsed() < Duration::from_millis(250));
    assert!(call_dropped.load(Ordering::SeqCst));
}

#[tokio::test]
async fn rendezvous_collision_uses_stable_peer_order_and_cancels_displaced_owner() {
    let manager = new_test_manager(
        new_identity("rendezvous-collision-local"),
        HashMap::new(),
        None,
    );
    let remote_id = new_identity("rendezvous-collision-remote").get_id();
    let local_initiator_wins = manager.local_identity.get_id().as_slice() < remote_id.as_slice();
    let displaced_cancel = Arc::new(AsyncNotify::new());
    let owner = |seq: u32, tunnel_id: u32, initiator_local, cancel| RendezvousAttemptOwner {
        seq: Sequence::from(seq),
        initiator_local,
        cancel,
        task: None,
        expected_incoming_reverse: None,
        tunnel_id: TunnelId::from(tunnel_id),
        yielded: Arc::new(AtomicBool::new(false)),
        winner_completions: Vec::new(),
        token: Arc::new(()),
    };

    manager
        .install_rendezvous_owner(
            remote_id.clone(),
            owner(1, 11, !local_initiator_wins, displaced_cancel.clone()),
            None,
        )
        .unwrap();
    manager
        .install_rendezvous_owner(
            remote_id.clone(),
            owner(1, 11, local_initiator_wins, Arc::new(AsyncNotify::new())),
            None,
        )
        .unwrap();
    runtime::timeout(Duration::from_secs(1), displaced_cancel.notified())
        .await
        .unwrap();
    manager.complete_rendezvous_owner(
        &remote_id,
        Sequence::from(1),
        TunnelId::from(99),
        &Arc::new(()),
        RendezvousWinnerCompletion::cancelled("stale completion"),
    );
    manager.cancel_rendezvous_owner(
        &remote_id,
        Sequence::from(99),
        TunnelId::from(11),
        &Arc::new(()),
    );
    assert!(
        manager
            .state
            .lock()
            .unwrap()
            .rendezvous_attempts
            .contains_key(&remote_id)
    );

    let loser_cancel = Arc::new(AsyncNotify::new());
    let loser_result = manager.install_rendezvous_owner(
        remote_id.clone(),
        owner(1, 11, !local_initiator_wins, loser_cancel.clone()),
        None,
    );
    if local_initiator_wins {
        assert_eq!(loser_result.unwrap_err().code(), P2pErrorCode::Conflict);
    } else {
        loser_result.unwrap();
        runtime::timeout(Duration::from_secs(1), loser_cancel.notified())
            .await
            .unwrap();
    }
    assert!(
        manager
            .state
            .lock()
            .unwrap()
            .rendezvous_attempts
            .get(&remote_id)
            .is_some_and(|owner| owner.initiator_local == local_initiator_wins)
    );

    let attach_remote = new_identity("rendezvous-attach-remote").get_id();
    manager
        .install_rendezvous_owner(
            attach_remote.clone(),
            owner(4, 44, true, Arc::new(AsyncNotify::new())),
            None,
        )
        .unwrap();
    let attach_token = manager
        .state
        .lock()
        .unwrap()
        .rendezvous_attempts
        .get(&attach_remote)
        .unwrap()
        .token
        .clone();
    let start = Arc::new(AsyncNotify::new());
    let start_for_task = start.clone();
    let started = Arc::new(AtomicBool::new(false));
    let started_for_task = started.clone();
    let task = Executor::spawn_with_handle(async move {
        start_for_task.notified().await;
        started_for_task.store(true, Ordering::SeqCst);
    })
    .unwrap();
    assert!(manager.attach_rendezvous_task(
        &attach_remote,
        Sequence::from(4),
        TunnelId::from(44),
        &attach_token,
        task,
        start.as_ref()
    ));
    runtime::timeout(Duration::from_secs(1), async {
        while !started.load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    let missing_start = Arc::new(AsyncNotify::new());
    let missing_start_for_task = missing_start.clone();
    let missing_task = Executor::spawn_with_handle(async move {
        missing_start_for_task.notified().await;
    })
    .unwrap();
    assert!(!manager.attach_rendezvous_task(
        &new_identity("rendezvous-missing-owner").get_id(),
        Sequence::from(5),
        TunnelId::from(55),
        &Arc::new(()),
        missing_task,
        missing_start.as_ref(),
    ));
}

#[tokio::test]
async fn rendezvous_waiter_owner_duplicate_preserves_incumbent() {
    let manager = new_test_manager(
        new_identity("rendezvous-duplicate-local"),
        HashMap::new(),
        None,
    );
    let remote_id = new_identity("rendezvous-duplicate-remote").get_id();
    let seq = Sequence::from(51);
    let tunnel_id = TunnelId::from(52);
    let incumbent_token = Arc::new(());
    let contender_token = Arc::new(());
    let owner = |token| RendezvousAttemptOwner {
        seq,
        initiator_local: false,
        cancel: Arc::new(AsyncNotify::new()),
        task: None,
        expected_incoming_reverse: Some(false),
        tunnel_id,
        yielded: Arc::new(AtomicBool::new(false)),
        winner_completions: Vec::new(),
        token,
    };
    let (incumbent_notify, incumbent_waiter) = Notify::new();
    manager
        .install_rendezvous_owner(
            remote_id.clone(),
            owner(incumbent_token.clone()),
            Some(incumbent_notify),
        )
        .unwrap();

    let (contender_notify, _contender_waiter) = Notify::new();
    let duplicate = manager.install_rendezvous_owner(
        remote_id.clone(),
        owner(contender_token),
        Some(contender_notify),
    );
    assert_eq!(duplicate.unwrap_err().code(), P2pErrorCode::AlreadyExists);
    {
        let state = manager.state.lock().unwrap();
        assert!(
            state
                .rendezvous_attempts
                .get(&remote_id)
                .is_some_and(|owner| Arc::ptr_eq(&owner.token, &incumbent_token))
        );
        assert!(
            state
                .pending_reverse_waiters
                .get(&(remote_id.clone(), tunnel_id, false))
                .is_some_and(|entry| Arc::ptr_eq(&entry.token, &incumbent_token))
        );
    }

    let incoming: TunnelRef = Arc::new(MockTunnel {
        tunnel_id,
        candidate_id: TunnelCandidateId::from(53),
        form: TunnelForm::Active,
        local_id: manager.local_identity.get_id(),
        remote_id: remote_id.clone(),
        state: TunnelState::Connected,
        is_reverse: false,
    });
    assert!(manager.on_incoming_tunnel(incoming.clone()).await.unwrap());
    let matched = runtime::timeout(Duration::from_secs(1), incumbent_waiter)
        .await
        .unwrap()
        .unwrap();
    assert!(Arc::ptr_eq(&matched, &incoming));

    manager.complete_rendezvous_owner(
        &remote_id,
        seq,
        tunnel_id,
        &incumbent_token,
        RendezvousWinnerCompletion::Success(incoming),
    );
}

#[tokio::test]
async fn rendezvous_waiter_owner_duplicate_notify_completes_incumbent_action() {
    let local = new_identity("rendezvous-duplicate-notify-local");
    let remote = new_identity("rendezvous-duplicate-notify-remote");
    let remote_id = remote.get_id();
    let manager = new_test_manager(local, HashMap::new(), None);
    let seq = Sequence::from(58);
    let tunnel_id = TunnelId::from(59);
    let notify = SnTunnelRendezvousNotify {
        seq,
        tunnel_id,
        peer_info: remote
            .get_identity_cert()
            .unwrap()
            .get_encoded_cert()
            .unwrap(),
        operation: SnTunnelRendezvousOperation::WaitIncoming,
        end_point_array: Vec::new(),
        need_predict_endpoint: false,
    };
    let serving_sn_id = P2pId::from(vec![60; 32]);

    let ack = manager
        .on_sn_rendezvous(notify.clone(), serving_sn_id.clone())
        .await
        .unwrap();
    assert_eq!(ack, SnTunnelRendezvousActionAck::without_prediction());
    let incumbent_token = {
        let state = manager.state.lock().unwrap();
        let owner = state.rendezvous_attempts.get(&remote_id).unwrap();
        assert_eq!(owner.seq, seq);
        assert_eq!(owner.tunnel_id, tunnel_id);
        assert!(!owner.initiator_local);
        assert_eq!(owner.expected_incoming_reverse, Some(false));
        let token = owner.token.clone();
        assert!(
            state
                .pending_reverse_waiters
                .get(&(remote_id.clone(), tunnel_id, false))
                .is_some_and(|entry| Arc::ptr_eq(&entry.token, &token))
        );
        token
    };

    let duplicate = manager
        .on_sn_rendezvous(notify, serving_sn_id)
        .await
        .unwrap_err();
    assert_eq!(duplicate.code(), P2pErrorCode::AlreadyExists);
    {
        let state = manager.state.lock().unwrap();
        assert!(
            state
                .rendezvous_attempts
                .get(&remote_id)
                .is_some_and(|owner| Arc::ptr_eq(&owner.token, &incumbent_token))
        );
        assert!(
            state
                .pending_reverse_waiters
                .get(&(remote_id.clone(), tunnel_id, false))
                .is_some_and(|entry| Arc::ptr_eq(&entry.token, &incumbent_token))
        );
    }

    let incoming: TunnelRef = Arc::new(MockTunnel {
        tunnel_id,
        candidate_id: TunnelCandidateId::from(60),
        form: TunnelForm::Active,
        local_id: manager.local_identity.get_id(),
        remote_id: remote_id.clone(),
        state: TunnelState::Connected,
        is_reverse: false,
    });
    assert!(manager.on_incoming_tunnel(incoming.clone()).await.unwrap());

    let registered = runtime::timeout(Duration::from_secs(1), async {
        loop {
            let owner_completed = !manager
                .state
                .lock()
                .unwrap()
                .rendezvous_attempts
                .contains_key(&remote_id);
            if owner_completed {
                if let Some(registered) = manager.get_tunnel(&remote_id) {
                    break registered;
                }
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(Arc::ptr_eq(&registered, &incoming));
}

#[tokio::test]
async fn rendezvous_waiter_owner_displaced_preserves_replacement() {
    let manager = new_test_manager(
        new_identity("rendezvous-replacement-local"),
        HashMap::new(),
        None,
    );
    let mut remote_bytes = vec![0; 32];
    remote_bytes[31] = 1;
    let remote_id = P2pId::from(remote_bytes);
    assert!(manager.local_identity.get_id().as_slice() > remote_id.as_slice());
    let displaced_seq = Sequence::from(54);
    let replacement_seq = Sequence::from(57);
    let tunnel_id = TunnelId::from(55);
    let displaced_token = Arc::new(());
    let replacement_token = Arc::new(());
    let displaced_cancel = Arc::new(AsyncNotify::new());
    let owner = |seq, cancel, token| RendezvousAttemptOwner {
        seq,
        initiator_local: false,
        cancel,
        task: None,
        expected_incoming_reverse: Some(false),
        tunnel_id,
        yielded: Arc::new(AtomicBool::new(false)),
        winner_completions: Vec::new(),
        token,
    };
    let (displaced_notify, _displaced_waiter) = Notify::new();
    manager
        .install_rendezvous_owner(
            remote_id.clone(),
            owner(
                displaced_seq,
                displaced_cancel.clone(),
                displaced_token.clone(),
            ),
            Some(displaced_notify),
        )
        .unwrap();
    let stale_registration = RendezvousOwnerRegistration::new(
        &manager,
        remote_id.clone(),
        displaced_seq,
        tunnel_id,
        displaced_token,
    );

    let (replacement_notify, replacement_waiter) = Notify::new();
    manager
        .install_rendezvous_owner(
            remote_id.clone(),
            owner(
                replacement_seq,
                Arc::new(AsyncNotify::new()),
                replacement_token.clone(),
            ),
            Some(replacement_notify),
        )
        .unwrap();
    runtime::timeout(Duration::from_secs(1), displaced_cancel.notified())
        .await
        .unwrap();
    drop(stale_registration);

    {
        let state = manager.state.lock().unwrap();
        assert!(
            state
                .rendezvous_attempts
                .get(&remote_id)
                .is_some_and(|owner| Arc::ptr_eq(&owner.token, &replacement_token))
        );
        assert!(
            state
                .pending_reverse_waiters
                .get(&(remote_id.clone(), tunnel_id, false))
                .is_some_and(|entry| Arc::ptr_eq(&entry.token, &replacement_token))
        );
    }

    let incoming: TunnelRef = Arc::new(MockTunnel {
        tunnel_id,
        candidate_id: TunnelCandidateId::from(56),
        form: TunnelForm::Active,
        local_id: manager.local_identity.get_id(),
        remote_id: remote_id.clone(),
        state: TunnelState::Connected,
        is_reverse: false,
    });
    assert!(manager.on_incoming_tunnel(incoming.clone()).await.unwrap());
    let matched = runtime::timeout(Duration::from_secs(1), replacement_waiter)
        .await
        .unwrap()
        .unwrap();
    assert!(Arc::ptr_eq(&matched, &incoming));

    manager.complete_rendezvous_owner(
        &remote_id,
        replacement_seq,
        tunnel_id,
        &replacement_token,
        RendezvousWinnerCompletion::Success(incoming),
    );
}

#[tokio::test]
async fn rendezvous_owner_drop_cleans_tuple_and_incoming_waiter() {
    let manager = new_test_manager(new_identity("rendezvous-drop-local"), HashMap::new(), None);
    let mut remote_bytes = vec![0; 32];
    remote_bytes[31] = 1;
    let remote_id = P2pId::from(remote_bytes);
    let seq = Sequence::from(61);
    let tunnel_id = TunnelId::from(62);
    let cancel = Arc::new(AsyncNotify::new());
    let owner_token = Arc::new(());
    let (incoming_notify, _incoming_waiter) = Notify::new();
    manager
        .install_rendezvous_owner(
            remote_id.clone(),
            RendezvousAttemptOwner {
                seq,
                initiator_local: true,
                cancel: cancel.clone(),
                task: None,
                expected_incoming_reverse: Some(true),
                tunnel_id,
                yielded: Arc::new(AtomicBool::new(false)),
                winner_completions: Vec::new(),
                token: owner_token.clone(),
            },
            Some(incoming_notify),
        )
        .unwrap();
    let registration =
        RendezvousOwnerRegistration::new(&manager, remote_id.clone(), seq, tunnel_id, owner_token);
    drop(registration);

    assert!(
        !manager
            .state
            .lock()
            .unwrap()
            .rendezvous_attempts
            .contains_key(&remote_id)
    );
    assert!(
        manager
            .take_incoming_waiter(&remote_id, &tunnel_id, true)
            .is_none()
    );
    runtime::timeout(Duration::from_secs(1), cancel.notified())
        .await
        .unwrap();
}

#[tokio::test]
async fn rendezvous_outbound_collision_waits_for_incoming_winner_result() {
    let manager = new_test_manager(
        new_identity("rendezvous-handoff-local"),
        HashMap::new(),
        None,
    );
    let mut remote_bytes = vec![0; 32];
    remote_bytes[31] = 1;
    let remote_id = P2pId::from(remote_bytes);
    assert!(manager.local_identity.get_id().as_slice() > remote_id.as_slice());
    let seq = Sequence::from(63);
    let tunnel_id = TunnelId::from(64);
    let outbound_cancel = Arc::new(AsyncNotify::new());
    let yielded = Arc::new(AtomicBool::new(false));
    let outbound_token = Arc::new(());
    let outbound_token_for_stale_completion = outbound_token.clone();
    let inbound_token = Arc::new(());
    let (winner_completion, winner_waiter) = oneshot::channel();
    let (loser_incoming_notify, _loser_incoming_waiter) = Notify::new();

    manager
        .install_rendezvous_owner(
            remote_id.clone(),
            RendezvousAttemptOwner {
                seq,
                initiator_local: true,
                cancel: outbound_cancel.clone(),
                task: None,
                expected_incoming_reverse: Some(true),
                tunnel_id,
                yielded: yielded.clone(),
                winner_completions: vec![winner_completion],
                token: outbound_token.clone(),
            },
            Some(loser_incoming_notify),
        )
        .unwrap();
    let stale_outbound_registration = RendezvousOwnerRegistration::new(
        &manager,
        remote_id.clone(),
        seq,
        tunnel_id,
        outbound_token,
    );
    manager
        .install_rendezvous_owner(
            remote_id.clone(),
            RendezvousAttemptOwner {
                seq,
                initiator_local: false,
                cancel: Arc::new(AsyncNotify::new()),
                task: None,
                expected_incoming_reverse: None,
                tunnel_id,
                yielded: Arc::new(AtomicBool::new(false)),
                winner_completions: Vec::new(),
                token: inbound_token.clone(),
            },
            None,
        )
        .unwrap();
    let stale_start = Arc::new(AsyncNotify::new());
    let stale_start_for_task = stale_start.clone();
    let stale_task = Executor::spawn_with_handle(async move {
        stale_start_for_task.notified().await;
        panic!("stale rendezvous task must not start");
    })
    .unwrap();
    assert!(!manager.attach_rendezvous_task(
        &remote_id,
        seq,
        tunnel_id,
        &outbound_token_for_stale_completion,
        stale_task,
        stale_start.as_ref(),
    ));
    runtime::timeout(Duration::from_secs(1), outbound_cancel.notified())
        .await
        .unwrap();
    assert!(yielded.load(Ordering::SeqCst));
    drop(stale_outbound_registration);
    manager.complete_rendezvous_owner(
        &remote_id,
        seq,
        tunnel_id,
        &outbound_token_for_stale_completion,
        RendezvousWinnerCompletion::cancelled("stale loser completion"),
    );
    assert!(
        manager
            .state
            .lock()
            .unwrap()
            .rendezvous_attempts
            .get(&remote_id)
            .is_some_and(|owner| Arc::ptr_eq(&owner.token, &inbound_token))
    );
    assert!(
        manager
            .take_incoming_waiter(&remote_id, &tunnel_id, true)
            .is_none()
    );

    let winner: TunnelRef = Arc::new(MockTunnel {
        tunnel_id,
        candidate_id: TunnelCandidateId::from(65),
        form: TunnelForm::Active,
        local_id: manager.local_identity.get_id(),
        remote_id: remote_id.clone(),
        state: TunnelState::Connected,
        is_reverse: true,
    });
    let (incoming_notify, incoming_waiter) = Notify::new();
    manager.add_incoming_waiter(remote_id.clone(), tunnel_id, true, incoming_notify);
    assert!(manager.on_incoming_tunnel(winner.clone()).await.unwrap());
    let registered = manager
        .wait_planned_incoming(&remote_id, true, incoming_waiter)
        .await
        .unwrap();
    assert!(Arc::ptr_eq(&registered, &winner));
    manager.complete_rendezvous_owner(
        &remote_id,
        seq,
        tunnel_id,
        &inbound_token,
        RendezvousWinnerCompletion::Success(winner.clone()),
    );

    let handed_off = runtime::timeout(Duration::from_secs(1), winner_waiter)
        .await
        .unwrap()
        .unwrap()
        .into_result()
        .unwrap();
    assert!(Arc::ptr_eq(&handed_off, &winner));
    assert!(Arc::ptr_eq(
        &manager.get_tunnel(&remote_id).unwrap(),
        &winner
    ));
    assert!(
        !manager
            .state
            .lock()
            .unwrap()
            .rendezvous_attempts
            .contains_key(&remote_id)
    );
}

#[tokio::test]
async fn rendezvous_target_uses_local_timeout_and_cleans_owner_without_terminal() {
    let local = new_identity("rendezvous-local-timeout-local");
    let remote = new_identity("rendezvous-local-timeout-remote");
    let remote_id = remote.get_id();
    let manager = new_test_manager(local, HashMap::new(), None);
    let seq = Sequence::from(71);
    let tunnel_id = TunnelId::from(72);
    let notify = SnTunnelRendezvousNotify {
        seq,
        tunnel_id,
        peer_info: remote
            .get_identity_cert()
            .unwrap()
            .get_encoded_cert()
            .unwrap(),
        operation: SnTunnelRendezvousOperation::WaitIncoming,
        end_point_array: Vec::new(),
        need_predict_endpoint: false,
    };

    let ack = manager
        .on_sn_rendezvous(notify, P2pId::from(vec![73; 32]))
        .await
        .unwrap();
    assert_eq!(ack, SnTunnelRendezvousActionAck::without_prediction());
    assert!(
        manager
            .state
            .lock()
            .unwrap()
            .rendezvous_attempts
            .get(&remote_id)
            .is_some_and(|owner| owner.seq == seq && owner.tunnel_id == tunnel_id)
    );

    runtime::timeout(Duration::from_secs(2), async {
        loop {
            if !manager
                .state
                .lock()
                .unwrap()
                .rendezvous_attempts
                .contains_key(&remote_id)
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(
        manager
            .take_incoming_waiter(&remote_id, &tunnel_id, false)
            .is_none()
    );
}
