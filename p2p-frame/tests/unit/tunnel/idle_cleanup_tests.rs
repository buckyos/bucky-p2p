use super::*;

use crate::networks::{
    IncomingDatagramCallback, IncomingStreamCallback, ListenVPortsRef, TunnelDatagramWrite,
    TunnelPurpose, TunnelStreamRead, TunnelStreamWrite,
};
use std::sync::atomic::{AtomicBool, AtomicUsize};

struct IdleAwareTunnel {
    tunnel_id: TunnelId,
    candidate_id: TunnelCandidateId,
    form: TunnelForm,
    local_id: P2pId,
    remote_id: P2pId,
    state: Mutex<TunnelState>,
    idle: AtomicBool,
    retired: AtomicBool,
    close_count: AtomicUsize,
    manager_on_close: Mutex<Option<Weak<TunnelManager>>>,
    fail_close: AtomicBool,
    close_locks_free: AtomicBool,
    observed_timeout: Mutex<Option<Duration>>,
}

impl IdleAwareTunnel {
    fn new(
        form: TunnelForm,
        local_id: P2pId,
        remote_id: P2pId,
        idle: bool,
    ) -> Arc<Self> {
        let tunnel_id = next_test_tunnel_id();
        Arc::new(Self {
            tunnel_id,
            candidate_id: TunnelCandidateId::from(tunnel_id.value()),
            form,
            local_id,
            remote_id,
            state: Mutex::new(TunnelState::Connected),
            idle: AtomicBool::new(idle),
            retired: AtomicBool::new(false),
            close_count: AtomicUsize::new(0),
            manager_on_close: Mutex::new(None),
            fail_close: AtomicBool::new(false),
            close_locks_free: AtomicBool::new(false),
            observed_timeout: Mutex::new(None),
        })
    }

    fn set_idle(&self, idle: bool) {
        self.idle.store(idle, Ordering::SeqCst);
    }

    fn fail_close_with_manager(&self, manager: &TunnelManagerRef) {
        *self.manager_on_close.lock().unwrap() = Some(Arc::downgrade(manager));
        self.fail_close.store(true, Ordering::SeqCst);
    }

    fn close_count(&self) -> usize {
        self.close_count.load(Ordering::SeqCst)
    }
}

#[async_trait::async_trait]
impl Tunnel for IdleAwareTunnel {
    fn tunnel_id(&self) -> TunnelId {
        self.tunnel_id
    }

    fn candidate_id(&self) -> TunnelCandidateId {
        self.candidate_id
    }

    fn form(&self) -> TunnelForm {
        self.form
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
        Some(loopback_tcp_ep())
    }

    fn remote_ep(&self) -> Option<Endpoint> {
        Some(loopback_tcp_ep())
    }

    fn state(&self) -> TunnelState {
        *self.state.lock().unwrap()
    }

    fn is_closed(&self) -> bool {
        self.state() == TunnelState::Closed
    }

    fn close(&self) -> P2pResult<()> {
        self.close_count.fetch_add(1, Ordering::SeqCst);
        if self.fail_close.load(Ordering::SeqCst) {
            let manager = self
                .manager_on_close
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .upgrade()
                .unwrap();
            let tunnels_lock_free = manager.tunnels.try_write().is_ok();
            let state_lock_free = manager.state.try_lock().is_ok();
            self.close_locks_free
                .store(tunnels_lock_free && state_lock_free, Ordering::SeqCst);
            return Err(p2p_err!(P2pErrorCode::IoError, "injected close failure"));
        }
        *self.state.lock().unwrap() = TunnelState::Closed;
        Ok(())
    }

    fn try_retire_idle(&self, _now: Instant, idle_timeout: Duration) -> bool {
        *self.observed_timeout.lock().unwrap() = Some(idle_timeout);
        self.idle.load(Ordering::SeqCst)
            && self
                .retired
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
    }

    async fn listen_stream(
        &self,
        _vports: ListenVPortsRef,
        _callback: IncomingStreamCallback,
    ) -> P2pResult<()> {
        Ok(())
    }

    async fn listen_datagram(
        &self,
        _vports: ListenVPortsRef,
        _callback: IncomingDatagramCallback,
    ) -> P2pResult<()> {
        Ok(())
    }

    async fn open_stream(
        &self,
        _purpose: TunnelPurpose,
    ) -> P2pResult<(TunnelStreamRead, TunnelStreamWrite)> {
        Err(p2p_err!(P2pErrorCode::NotSupport, "test tunnel"))
    }

    async fn open_datagram(&self, _purpose: TunnelPurpose) -> P2pResult<TunnelDatagramWrite> {
        Err(p2p_err!(P2pErrorCode::NotSupport, "test tunnel"))
    }
}

fn manager_and_ids(name: &str) -> (TunnelManagerRef, P2pIdentityRef, P2pIdentityRef) {
    init_tls_once();
    let local = new_identity(&format!("local-{name}"));
    let remote = new_identity(&format!("remote-{name}"));
    let manager = new_test_manager(local.clone(), HashMap::new(), None);
    (manager, local, remote)
}

#[tokio::test]
async fn cleanup_delegates_idle_decision_and_preserves_original_arc() {
    let (manager, local, remote) = manager_and_ids("delegated-idle");
    let source = IdleAwareTunnel::new(
        TunnelForm::Active,
        local.get_id(),
        remote.get_id(),
        true,
    );
    let source_ref: TunnelRef = source.clone();
    let registered = manager
        .register_tunnel_and_publish(source_ref.clone())
        .await
        .unwrap();

    assert!(Arc::ptr_eq(&registered, &source_ref));
    assert!(Arc::ptr_eq(
        &manager.get_tunnel(&remote.get_id()).unwrap(),
        &source_ref
    ));

    let timeout = Duration::from_secs(30);
    manager.cleanup_closed_tunnels(timeout).await;

    assert!(manager.get_tunnel(&remote.get_id()).is_none());
    assert_eq!(*source.observed_timeout.lock().unwrap(), Some(timeout));
    assert_eq!(source.close_count(), 1);
}

#[tokio::test]
async fn cleanup_keeps_tunnel_until_its_atomic_idle_decision_succeeds() {
    let (manager, local, remote) = manager_and_ids("atomic-idle-decision");
    let source = IdleAwareTunnel::new(
        TunnelForm::Active,
        local.get_id(),
        remote.get_id(),
        false,
    );
    let source_ref: TunnelRef = source.clone();
    manager
        .register_tunnel_and_publish(source_ref.clone())
        .await
        .unwrap();

    manager.cleanup_closed_tunnels(Duration::ZERO).await;
    assert!(Arc::ptr_eq(
        &manager.get_tunnel(&remote.get_id()).unwrap(),
        &source_ref
    ));
    assert_eq!(source.close_count(), 0);

    source.set_idle(true);
    manager.cleanup_closed_tunnels(Duration::ZERO).await;
    assert!(manager.get_tunnel(&remote.get_id()).is_none());
    assert_eq!(source.close_count(), 1);
}

#[tokio::test]
async fn custom_tunnel_default_hook_opts_out_of_idle_cleanup() {
    let (manager, local, remote) = manager_and_ids("custom-opt-out");
    let custom = TrackableTunnel::new(
        TunnelForm::Active,
        local.get_id(),
        remote.get_id(),
        TunnelState::Connected,
    );
    let custom_ref: TunnelRef = custom.clone();
    manager
        .register_tunnel_and_publish(custom_ref.clone())
        .await
        .unwrap();

    manager.cleanup_closed_tunnels(Duration::ZERO).await;

    assert!(Arc::ptr_eq(
        &manager.get_tunnel(&remote.get_id()).unwrap(),
        &custom_ref
    ));
    assert_eq!(custom.close_count(), 0);
}

#[tokio::test]
async fn cleanup_removes_only_the_exact_idle_candidate() {
    let (manager, local, remote) = manager_and_ids("exact-candidate");
    let idle = IdleAwareTunnel::new(
        TunnelForm::Active,
        local.get_id(),
        remote.get_id(),
        true,
    );
    let live = IdleAwareTunnel::new(
        TunnelForm::Active,
        local.get_id(),
        remote.get_id(),
        false,
    );
    let idle_ref: TunnelRef = idle.clone();
    let live_ref: TunnelRef = live.clone();
    manager.register_tunnel_and_publish(idle_ref).await.unwrap();
    manager
        .register_tunnel_and_publish(live_ref.clone())
        .await
        .unwrap();

    manager.cleanup_closed_tunnels(Duration::ZERO).await;

    assert_eq!(idle.close_count(), 1);
    assert_eq!(live.close_count(), 0);
    assert!(Arc::ptr_eq(
        &manager.get_tunnel(&remote.get_id()).unwrap(),
        &live_ref
    ));
}

#[tokio::test]
async fn idle_proxy_removal_clears_proxy_upgrade_tracking() {
    let (manager, local, remote) = manager_and_ids("proxy-reconcile");
    let proxy = IdleAwareTunnel::new(
        TunnelForm::Proxy,
        local.get_id(),
        remote.get_id(),
        true,
    );
    let proxy_ref: TunnelRef = proxy.clone();
    manager
        .register_tunnel_and_publish(proxy_ref)
        .await
        .unwrap();
    assert!(
        manager
            .state
            .lock()
            .unwrap()
            .proxy_upgrade_states
            .contains_key(&remote.get_id())
    );

    manager.cleanup_closed_tunnels(Duration::ZERO).await;

    assert!(manager.get_tunnel(&remote.get_id()).is_none());
    assert_eq!(proxy.close_count(), 1);
    assert!(
        !manager
            .state
            .lock()
            .unwrap()
            .proxy_upgrade_states
            .contains_key(&remote.get_id())
    );
}

#[tokio::test]
async fn close_error_happens_after_manager_locks_are_released() {
    let (manager, local, remote) = manager_and_ids("lock-free-close-error");
    let failing = IdleAwareTunnel::new(
        TunnelForm::Active,
        local.get_id(),
        remote.get_id(),
        true,
    );
    failing.fail_close_with_manager(&manager);
    let failing_ref: TunnelRef = failing.clone();
    manager
        .register_tunnel_and_publish(failing_ref)
        .await
        .unwrap();

    manager.cleanup_closed_tunnels(Duration::ZERO).await;

    assert!(manager.get_tunnel(&remote.get_id()).is_none());
    assert_eq!(failing.close_count(), 1);
    assert!(failing.close_locks_free.load(Ordering::SeqCst));
}

#[tokio::test]
async fn duplicate_registration_recognizes_the_original_arc() {
    let (manager, local, remote) = manager_and_ids("original-arc");
    let source = IdleAwareTunnel::new(
        TunnelForm::Active,
        local.get_id(),
        remote.get_id(),
        false,
    );
    let source_ref: TunnelRef = source.clone();
    let registered = manager.register_tunnel(source_ref.clone()).await.unwrap();

    assert!(Arc::ptr_eq(&registered, &source_ref));
    assert!(manager.register_tunnel(source_ref.clone()).await.is_err());
    assert_eq!(source.close_count(), 0);
    let entries = manager.tunnels.read().unwrap();
    assert!(Arc::ptr_eq(
        &entries.get(&remote.get_id()).unwrap()[0].tunnel,
        &source_ref
    ));
}
