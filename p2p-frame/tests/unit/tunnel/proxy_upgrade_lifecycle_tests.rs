use super::*;

async fn register_published_proxy(
    case: &str,
) -> (TunnelManagerRef, P2pId, Arc<TrackableTunnel>) {
    init_tls_once();

    let local_identity = new_identity(&format!("local-{case}"));
    let remote_identity = new_identity(&format!("remote-{case}"));
    let remote_id = remote_identity.get_id();
    let manager = new_test_manager(local_identity.clone(), HashMap::new(), None);
    let proxy = TrackableTunnel::new(
        TunnelForm::Proxy,
        local_identity.get_id(),
        remote_id.clone(),
        TunnelState::Connected,
    );

    manager
        .register_tunnel_and_publish(proxy.clone())
        .await
        .unwrap();

    (manager, remote_id, proxy)
}

fn collect_due_generation(manager: &TunnelManagerRef, remote_id: &P2pId) -> u64 {
    {
        let mut state = manager.state.lock().unwrap();
        state
            .proxy_upgrade_states
            .get_mut(remote_id)
            .unwrap()
            .next_attempt_at = Instant::now();
    }

    let due = manager.collect_due_proxy_upgrades();
    assert_eq!(due.len(), 1);
    assert_eq!(&due[0].0, remote_id);
    due[0].1
}

fn assert_remote_lifecycle_removed(manager: &TunnelManagerRef, remote_id: &P2pId) {
    assert!(!manager.tunnels.read().unwrap().contains_key(remote_id));
    assert!(
        !manager
            .state
            .lock()
            .unwrap()
            .proxy_upgrade_states
            .contains_key(remote_id)
    );
}

fn current_proxy_upgrade_state(
    manager: &TunnelManagerRef,
    remote_id: &P2pId,
) -> ProxyUpgradeState {
    manager
        .state
        .lock()
        .unwrap()
        .proxy_upgrade_states
        .get(remote_id)
        .copied()
        .unwrap()
}

fn assert_proxy_upgrade_state_unchanged(
    before: ProxyUpgradeState,
    after: ProxyUpgradeState,
) {
    assert_eq!(after.generation, before.generation);
    assert_eq!(after.next_attempt_at, before.next_attempt_at);
    assert_eq!(after.retry_interval, before.retry_interval);
    assert_eq!(after.short_retry_index, before.short_retry_index);
    assert_eq!(after.in_progress, before.in_progress);
}

#[tokio::test]
async fn cleanup_last_proxy_removes_bucket_and_upgrade_generation() {
    let (manager, remote_id, proxy) = register_published_proxy("cleanup-last-proxy").await;
    let old_generation = collect_due_generation(&manager, &remote_id);

    proxy.close().unwrap();
    manager.cleanup_closed_tunnels(Duration::from_secs(0)).await;

    assert_remote_lifecycle_removed(&manager, &remote_id);
    assert_eq!(
        manager.on_proxy_upgrade_failed_generation(&remote_id, old_generation),
        None
    );
}

#[tokio::test]
async fn get_tunnel_prunes_last_proxy_and_clears_upgrade_state() {
    let (manager, remote_id, proxy) = register_published_proxy("get-prune-last-proxy").await;

    proxy.close().unwrap();

    assert!(manager.get_tunnel(&remote_id).is_none());
    assert_remote_lifecycle_removed(&manager, &remote_id);
}

#[tokio::test]
async fn availability_lookup_prunes_last_proxy_and_clears_upgrade_state() {
    let (manager, remote_id, proxy) =
        register_published_proxy("availability-prune-last-proxy").await;
    let tunnel_id = proxy.tunnel_id();

    proxy.close().unwrap();

    assert!(!manager.has_available_tunnel_id(&remote_id, tunnel_id));
    assert_remote_lifecycle_removed(&manager, &remote_id);
}

#[tokio::test]
async fn stale_failure_cannot_mutate_reregistered_proxy_generation() {
    let (manager, remote_id, old_proxy) = register_published_proxy("generation-old-proxy").await;
    let old_generation = collect_due_generation(&manager, &remote_id);

    old_proxy.close().unwrap();
    manager.cleanup_closed_tunnels(Duration::from_secs(0)).await;
    assert_remote_lifecycle_removed(&manager, &remote_id);

    let new_proxy = TrackableTunnel::new(
        TunnelForm::Proxy,
        manager.local_identity.get_id(),
        remote_id.clone(),
        TunnelState::Connected,
    );
    manager
        .register_tunnel_and_publish(new_proxy)
        .await
        .unwrap();
    let new_generation = collect_due_generation(&manager, &remote_id);
    assert_ne!(new_generation, old_generation);

    let before = current_proxy_upgrade_state(&manager, &remote_id);
    assert!(before.in_progress);

    assert_eq!(
        manager.on_proxy_upgrade_failed_generation(&remote_id, old_generation),
        None
    );

    let after = current_proxy_upgrade_state(&manager, &remote_id);
    assert_proxy_upgrade_state_unchanged(before, after);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn proxy_registration_holds_tunnel_lock_until_state_decision() {
    init_tls_once();

    let local_identity = new_identity("local-atomic-proxy-registration");
    let remote_identity = new_identity("remote-atomic-proxy-registration");
    let remote_id = remote_identity.get_id();
    let manager = new_test_manager(local_identity.clone(), HashMap::new(), None);
    let proxy = TrackableTunnel::new(
        TunnelForm::Proxy,
        local_identity.get_id(),
        remote_id.clone(),
        TunnelState::Connected,
    );
    let start = Arc::new(std::sync::Barrier::new(2));

    let task_manager = manager.clone();
    let task_start = start.clone();
    let registration = tokio::spawn(async move {
        task_start.wait();
        task_manager.register_tunnel_and_publish(proxy).await
    });

    let state_guard = manager.state.lock().unwrap();
    start.wait();

    let deadline = Instant::now() + Duration::from_secs(2);
    let registration_holds_tunnels = loop {
        if manager.tunnels.try_read().is_err() {
            break true;
        }
        if Instant::now() >= deadline {
            break false;
        }
        std::thread::yield_now();
    };
    assert!(
        registration_holds_tunnels,
        "registration must retain the tunnel write lock while waiting to make its state decision"
    );

    drop(state_guard);
    registration.await.unwrap().unwrap();

    assert!(manager.get_tunnel(&remote_id).is_some());
    assert!(
        manager
            .state
            .lock()
            .unwrap()
            .proxy_upgrade_states
            .contains_key(&remote_id)
    );
}

#[tokio::test]
async fn unavailable_returned_direct_cannot_finalize_replacement_proxy_lifecycle() {
    let (manager, remote_id, replacement_proxy) =
        register_published_proxy("unavailable-returned-direct").await;
    let returned_direct = TrackableTunnel::new(
        TunnelForm::Active,
        manager.local_identity.get_id(),
        remote_id.clone(),
        TunnelState::Connected,
    );

    manager
        .register_tunnel_and_publish(returned_direct.clone())
        .await
        .unwrap();
    returned_direct.close().unwrap();
    let selected = manager.get_tunnel(&remote_id).unwrap();
    let replacement_proxy_ref: TunnelRef = replacement_proxy.clone();
    assert!(Arc::ptr_eq(&selected, &replacement_proxy_ref));

    let replacement_state = current_proxy_upgrade_state(&manager, &remote_id);
    let returned_direct_ref: TunnelRef = returned_direct.clone();
    assert!(!manager.finalize_proxy_upgrade_success(&remote_id, &returned_direct_ref));

    let after = current_proxy_upgrade_state(&manager, &remote_id);
    assert_proxy_upgrade_state_unchanged(replacement_state, after);
    assert_eq!(replacement_proxy.close_count(), 0);
    let tunnels = manager.tunnels.read().unwrap();
    let entries = tunnels.get(&remote_id).unwrap();
    assert_eq!(entries.len(), 1);
    assert!(Arc::ptr_eq(&entries[0].tunnel, &replacement_proxy_ref));
}

#[tokio::test]
async fn live_registered_direct_finalization_retires_proxy_lifecycle() {
    let (manager, remote_id, proxy) = register_published_proxy("live-returned-direct").await;
    let direct = TrackableTunnel::new(
        TunnelForm::Active,
        manager.local_identity.get_id(),
        remote_id.clone(),
        TunnelState::Connected,
    );
    manager
        .register_tunnel_and_publish(direct.clone())
        .await
        .unwrap();

    manager.track_proxy_upgrade(&remote_id);
    assert!(
        manager
            .state
            .lock()
            .unwrap()
            .proxy_upgrade_states
            .contains_key(&remote_id)
    );

    let direct_ref: TunnelRef = direct.clone();
    assert!(manager.finalize_proxy_upgrade_success(&remote_id, &direct_ref));

    assert!(
        !manager
            .state
            .lock()
            .unwrap()
            .proxy_upgrade_states
            .contains_key(&remote_id)
    );
    assert_eq!(proxy.close_count(), 1);
    assert!(proxy.is_closed());
    assert!(!direct.is_closed());
    let tunnels = manager.tunnels.read().unwrap();
    let entries = tunnels.get(&remote_id).unwrap();
    assert_eq!(entries.len(), 1);
    assert!(Arc::ptr_eq(&entries[0].tunnel, &direct_ref));
}

#[tokio::test]
async fn new_proxy_beside_live_direct_does_not_schedule_upgrade() {
    init_tls_once();

    let local_identity = new_identity("local-mixed-direct-proxy");
    let remote_identity = new_identity("remote-mixed-direct-proxy");
    let remote_id = remote_identity.get_id();
    let manager = new_test_manager(local_identity.clone(), HashMap::new(), None);
    let direct = TrackableTunnel::new(
        TunnelForm::Active,
        local_identity.get_id(),
        remote_id.clone(),
        TunnelState::Connected,
    );
    let proxy = TrackableTunnel::new(
        TunnelForm::Proxy,
        local_identity.get_id(),
        remote_id.clone(),
        TunnelState::Connected,
    );

    manager
        .register_tunnel_and_publish(direct.clone())
        .await
        .unwrap();
    manager
        .register_tunnel_and_publish(proxy.clone())
        .await
        .unwrap();

    assert!(
        !manager
            .state
            .lock()
            .unwrap()
            .proxy_upgrade_states
            .contains_key(&remote_id)
    );
    let selected = manager.get_tunnel(&remote_id).unwrap();
    let direct_ref: TunnelRef = direct;
    assert!(Arc::ptr_eq(&selected, &direct_ref));
    assert_eq!(manager.tunnels.read().unwrap()[&remote_id].len(), 2);
}

#[tokio::test]
async fn unavailable_direct_registration_preserves_live_proxy_generation() {
    let (manager, remote_id, proxy) =
        register_published_proxy("unavailable-direct-registration").await;
    let before = current_proxy_upgrade_state(&manager, &remote_id);
    let unavailable_direct = TrackableTunnel::new(
        TunnelForm::Active,
        manager.local_identity.get_id(),
        remote_id.clone(),
        TunnelState::Closed,
    );

    let registered = manager
        .register_tunnel_and_publish(unavailable_direct)
        .await;
    assert!(
        registered.is_ok(),
        "registration continues to accept an unavailable candidate; lifecycle state follows the available bucket topology"
    );

    let after_registration = current_proxy_upgrade_state(&manager, &remote_id);
    assert_proxy_upgrade_state_unchanged(before, after_registration);
    let selected = manager.get_tunnel(&remote_id).unwrap();
    let proxy_ref: TunnelRef = proxy.clone();
    assert!(Arc::ptr_eq(&selected, &proxy_ref));
    assert!(!proxy.is_closed());
    assert_eq!(proxy.close_count(), 0);
    let after_prune = current_proxy_upgrade_state(&manager, &remote_id);
    assert_proxy_upgrade_state_unchanged(after_registration, after_prune);
}

#[tokio::test]
async fn unavailable_proxy_registration_does_not_create_orphan_generation() {
    init_tls_once();

    let local_identity = new_identity("local-unavailable-proxy-registration");
    let remote_identity = new_identity("remote-unavailable-proxy-registration");
    let remote_id = remote_identity.get_id();
    let manager = new_test_manager(local_identity.clone(), HashMap::new(), None);
    let unavailable_proxy = TrackableTunnel::new(
        TunnelForm::Proxy,
        local_identity.get_id(),
        remote_id.clone(),
        TunnelState::Closed,
    );

    let registered = manager
        .register_tunnel_and_publish(unavailable_proxy)
        .await;
    assert!(
        registered.is_ok(),
        "registration result remains unchanged for an unavailable proxy candidate"
    );
    assert!(
        !manager
            .state
            .lock()
            .unwrap()
            .proxy_upgrade_states
            .contains_key(&remote_id)
    );
    assert_eq!(manager.tunnels.read().unwrap()[&remote_id].len(), 1);

    assert!(manager.get_tunnel(&remote_id).is_none());
    assert_remote_lifecycle_removed(&manager, &remote_id);
    manager.cleanup_closed_tunnels(Duration::from_secs(0)).await;
    assert_remote_lifecycle_removed(&manager, &remote_id);
}
