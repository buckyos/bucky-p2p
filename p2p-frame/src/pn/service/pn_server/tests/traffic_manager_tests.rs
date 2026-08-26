use super::super::*;
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

fn user(value: u8) -> P2pId {
    P2pId::from(vec![value; 32])
}

fn active_entry(
    traffic_manager: &PnTrafficManager,
    user: &P2pId,
) -> Arc<PnUserTrafficEntry> {
    traffic_manager
        .shared
        .state
        .lock()
        .unwrap()
        .users
        .get(user)
        .unwrap()
        .entry
        .clone()
}

async fn wait_until_async(timeout: Duration, mut predicate: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if predicate() {
            return true;
        }
        runtime::sleep(Duration::from_millis(5)).await;
    }
    predicate()
}

fn idle_state(traffic_manager: &PnTrafficManager, user: &P2pId) -> Instant {
    traffic_manager
        .shared
        .state
        .lock()
        .unwrap()
        .users
        .get(user)
        .unwrap()
        .idle_deadline
        .unwrap()
}

#[test]
fn traffic_snapshot_iterator_is_empty_and_advances_without_holding_the_manager_lock() {
    let traffic_manager = PnTrafficManager::new();
    assert!(traffic_manager.iter().next().is_none());

    let high_session = traffic_manager.begin_session(&user(3), &user(4));
    let low_session = traffic_manager.begin_session(&user(1), &user(2));
    let mut snapshots = traffic_manager.iter();

    assert_eq!(snapshots.next().unwrap().0, user(1));
    drop(low_session);
    assert_eq!(snapshots.next().unwrap().0, user(3));
    assert_eq!(snapshots.next().unwrap().0, user(4));
    assert!(snapshots.next().is_none());

    drop(high_session);
    assert!(traffic_manager.iter().next().is_none());
}

#[test]
fn traffic_snapshot_reports_first_and_subsequent_deltas_without_consuming_peek() {
    let traffic_manager = PnTrafficManager::new();
    let source = user(1);
    let target = user(2);
    let session = traffic_manager.begin_session(&source, &target);

    session.source_tracker.add_read_data_size(10);
    session.source_tracker.add_write_data_size(4);

    let peek = traffic_manager.peek_snapshot(&source).unwrap();
    assert_eq!((peek.tx_bytes, peek.rx_bytes), (10, 4));
    assert_eq!((peek.tx_delta_bytes, peek.rx_delta_bytes), (10, 4));

    let first = traffic_manager.snapshot(&source).unwrap();
    assert_eq!((first.tx_bytes, first.rx_bytes), (10, 4));
    assert_eq!((first.tx_delta_bytes, first.rx_delta_bytes), (10, 4));

    let unchanged = traffic_manager.snapshot(&source).unwrap();
    assert_eq!((unchanged.tx_bytes, unchanged.rx_bytes), (10, 4));
    assert_eq!(
        (unchanged.tx_delta_bytes, unchanged.rx_delta_bytes),
        (0, 0)
    );

    session.source_tracker.add_read_data_size(3);
    session.source_tracker.add_write_data_size(2);
    let next = traffic_manager.snapshot(&source).unwrap();
    assert_eq!((next.tx_bytes, next.rx_bytes), (13, 6));
    assert_eq!((next.tx_delta_bytes, next.rx_delta_bytes), (3, 2));

    assert_eq!(traffic_manager.peek_snapshot(&user(9)), None);
    drop(session);
}

#[test]
fn point_lookup_and_iterator_share_one_delta_baseline() {
    let traffic_manager = PnTrafficManager::new();
    let source = user(1);
    let target = user(2);
    let session = traffic_manager.begin_session(&source, &target);

    session.source_tracker.add_read_data_size(8);
    assert_eq!(
        traffic_manager
            .snapshot(&source)
            .unwrap()
            .tx_delta_bytes,
        8
    );

    session.source_tracker.add_read_data_size(5);
    let (iter_user, iter_snapshot) = traffic_manager.iter().next().unwrap();
    assert_eq!(iter_user, source);
    assert_eq!(iter_snapshot.tx_bytes, 13);
    assert_eq!(iter_snapshot.tx_delta_bytes, 5);

    drop(session);
}

#[test]
fn concurrent_snapshot_acquisitions_do_not_double_report_delta_bytes() {
    let traffic_manager = PnTrafficManager::new();
    let source = user(1);
    let session = traffic_manager.begin_session(&source, &user(2));
    session.source_tracker.add_read_data_size(11);

    let barrier = Arc::new(Barrier::new(3));
    let mut handles = Vec::new();
    for _ in 0..2 {
        let traffic_manager = traffic_manager.clone();
        let source = source.clone();
        let barrier = barrier.clone();
        handles.push(thread::spawn(move || {
            barrier.wait();
            traffic_manager
                .snapshot(&source)
                .unwrap()
                .tx_delta_bytes
        }));
    }
    barrier.wait();

    let mut deltas = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect::<Vec<_>>();
    deltas.sort_unstable();
    assert_eq!(deltas, vec![0, 11]);

    drop(session);
}

#[test]
fn live_entry_is_released_only_after_each_users_last_distinct_session() {
    let traffic_manager = PnTrafficManager::new();
    let source = user(1);
    let first_target = user(2);
    let second_target = user(3);
    let first = traffic_manager.begin_session(&source, &first_target);
    let second = traffic_manager.begin_session(&source, &second_target);

    drop(first);
    assert!(traffic_manager.snapshot(&source).is_some());
    assert_eq!(traffic_manager.snapshot(&first_target), None);
    assert!(traffic_manager.snapshot(&second_target).is_some());

    drop(second);
    assert_eq!(traffic_manager.snapshot(&source), None);
    assert_eq!(traffic_manager.snapshot(&second_target), None);
    assert!(traffic_manager.iter().next().is_none());
}

#[test]
fn default_and_explicit_zero_retention_release_immediately() {
    let traffic_manager = PnTrafficManager::new();
    let default_source = user(10);
    let default_session = traffic_manager.begin_session(&default_source, &user(11));
    drop(default_session);
    assert_eq!(traffic_manager.snapshot(&default_source), None);

    traffic_manager.set_retention(Duration::from_millis(100));
    traffic_manager.set_retention(Duration::ZERO);
    let explicit_source = user(12);
    let explicit_session = traffic_manager.begin_session(&explicit_source, &user(13));
    drop(explicit_session);
    assert_eq!(traffic_manager.snapshot(&explicit_source), None);
    assert!(traffic_manager
        .shared
        .state
        .lock()
        .unwrap()
        .cleanup_deadlines
        .is_empty());
}

#[test]
fn traffic_retention_public_setter_has_expected_compile_surface() {
    let setter: fn(&PnServer, Duration) = PnServer::set_user_traffic_retention;
    let _ = setter;
}

#[test]
fn traffic_async_cleanup_unit_manager_construction_outside_runtime_creates_no_task() {
    let traffic_manager = PnTrafficManager::new();

    assert!(traffic_manager.cleanup_task.lock().unwrap().is_none());
}

#[test]
fn traffic_async_cleanup_unit_source_uses_async_task_not_os_thread() {
    let source = include_str!("../../pn_server.rs");

    assert!(!source.contains("Condvar"));
    assert!(!source.contains("std::thread"));
    assert!(source.contains("Executor::spawn_with_handle"));
    assert!(source.contains("tokio::select!"));
}

#[tokio::test(flavor = "current_thread")]
async fn traffic_async_cleanup_dv_start_is_idempotent_and_keeps_one_handle() {
    let traffic_manager = PnTrafficManager::new();
    traffic_manager.start_cleanup_task().unwrap();
    let first_handle = {
        let cleanup_task = traffic_manager.cleanup_task.lock().unwrap();
        cleanup_task
            .as_ref()
            .map(|handle| handle as *const _)
            .unwrap()
    };

    traffic_manager.start_cleanup_task().unwrap();
    let second_handle = {
        let cleanup_task = traffic_manager.cleanup_task.lock().unwrap();
        cleanup_task
            .as_ref()
            .map(|handle| handle as *const _)
            .unwrap()
    };

    assert_eq!(first_handle, second_handle);
    traffic_manager.shutdown();
}

#[test]
fn retained_idle_user_is_visible_and_reconnect_continues_entry_and_delta_baseline() {
    let traffic_manager = PnTrafficManager::new();
    traffic_manager.set_retention(Duration::from_secs(2));
    let source = user(20);
    let target = user(21);
    let first_session = traffic_manager.begin_session(&source, &target);
    let first_entry = active_entry(&traffic_manager, &source);
    first_session.source_tracker.add_read_data_size(10);
    assert_eq!(traffic_manager.snapshot(&source).unwrap().tx_delta_bytes, 10);

    drop(first_session);
    let idle_snapshot = traffic_manager.snapshot(&source).unwrap();
    assert_eq!(idle_snapshot.tx_bytes, 10);
    assert_eq!(idle_snapshot.tx_delta_bytes, 0);
    let traversed = traffic_manager
        .iter()
        .find(|(candidate, _)| candidate == &source)
        .unwrap();
    assert_eq!(traversed.1.tx_bytes, 10);
    assert_eq!(traversed.1.tx_delta_bytes, 0);

    let second_session = traffic_manager.begin_session(&source, &target);
    let second_entry = active_entry(&traffic_manager, &source);
    assert!(Arc::ptr_eq(&first_entry, &second_entry));
    second_session.source_tracker.add_read_data_size(4);
    let reconnected = traffic_manager.snapshot(&source).unwrap();
    assert_eq!(reconnected.tx_bytes, 14);
    assert_eq!(reconnected.tx_delta_bytes, 4);
    assert!(traffic_manager
        .shared
        .state
        .lock()
        .unwrap()
        .cleanup_deadlines
        .is_empty());

    traffic_manager.set_retention(Duration::ZERO);
    drop(second_session);
}

#[tokio::test(flavor = "current_thread")]
async fn traffic_async_cleanup_dv_idle_entry_expires_without_blocking_current_thread_runtime() {
    let traffic_manager = PnTrafficManager::new();
    traffic_manager.start_cleanup_task().unwrap();
    traffic_manager.set_retention(Duration::from_millis(60));
    let source = user(30);
    let session = traffic_manager.begin_session(&source, &user(31));
    drop(session);

    let runtime_progress = tokio::spawn(async {
        runtime::sleep(Duration::from_millis(20)).await;
        true
    });
    assert!(runtime_progress.await.unwrap());
    runtime::sleep(Duration::from_millis(160)).await;
    let state = traffic_manager.shared.state.lock().unwrap();
    assert!(!state.users.contains_key(&source));
    assert!(state.cleanup_deadlines.is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn traffic_async_cleanup_dv_traffic_release_simplification_dv_max_duration_clamps_caps_wait_and_shutdown_takes_handle() {
    let traffic_manager = PnTrafficManager::new();
    traffic_manager.start_cleanup_task().unwrap();
    traffic_manager.set_retention(PN_TRAFFIC_RETENTION_MAX);
    assert_eq!(
        traffic_manager.shared.state.lock().unwrap().retention,
        PN_TRAFFIC_RETENTION_MAX
    );
    traffic_manager.set_retention(Duration::MAX);
    let source = user(32);
    let target = user(33);
    let before_disconnect = Instant::now();
    let session = traffic_manager.begin_session(&source, &target);
    drop(session);
    let after_disconnect = Instant::now();

    {
        let state = traffic_manager.shared.state.lock().unwrap();
        assert_eq!(state.retention, PN_TRAFFIC_RETENTION_MAX);
        assert_eq!(state.cleanup_deadlines.len(), 2);
        for tracked_user in [&source, &target] {
            let idle_deadline = state
                .users
                .get(tracked_user)
                .unwrap()
                .idle_deadline
                .unwrap();
            assert!(
                idle_deadline
                    >= before_disconnect
                        .checked_add(PN_TRAFFIC_RETENTION_MAX)
                        .unwrap()
            );
            assert!(
                idle_deadline
                    <= after_disconnect
                        .checked_add(PN_TRAFFIC_RETENTION_MAX)
                        .unwrap()
            );
            assert!(state.cleanup_deadlines.iter().any(|deadline| {
                &deadline.user == tracked_user
                    && deadline.deadline == idle_deadline
            }));
        }
    }

    assert!(matches!(
        PnTrafficManager::cleanup_action(&traffic_manager.shared),
        PnTrafficCleanupAction::WaitForDeadline(wait)
            if wait == PN_TRAFFIC_CLEANUP_MAX_WAIT
    ));
    runtime::sleep(Duration::from_millis(30)).await;
    assert!(traffic_manager.cleanup_task.lock().unwrap().is_some());

    traffic_manager.shutdown();
    assert!(traffic_manager.cleanup_task.lock().unwrap().is_none());
    let state = traffic_manager.shared.state.lock().unwrap();
    assert!(state.shutdown);
    assert!(state.users.is_empty());
    assert!(state.cleanup_deadlines.is_empty());
    assert_eq!(state.retention, Duration::ZERO);
}

#[tokio::test(flavor = "current_thread")]
async fn traffic_async_cleanup_dv_traffic_release_simplification_dv_reconnect_cancels_old_exact_deadline_and_expires_at_new_deadline() {
    let traffic_manager = PnTrafficManager::new();
    traffic_manager.start_cleanup_task().unwrap();
    let retention = Duration::from_millis(500);
    traffic_manager.set_retention(retention);
    let source = user(40);
    let target = user(41);
    let first_session = traffic_manager.begin_session(&source, &target);
    let entry = active_entry(&traffic_manager, &source);
    drop(first_session);
    let first_deadline = idle_state(&traffic_manager, &source);

    runtime::sleep(Duration::from_millis(200)).await;
    let second_session = traffic_manager.begin_session(&source, &target);
    {
        let state = traffic_manager.shared.state.lock().unwrap();
        let source_state = state.users.get(&source).unwrap();
        assert_eq!(source_state.active_sessions, 1);
        assert_eq!(source_state.idle_deadline, None);
        assert!(!state.cleanup_deadlines.iter().any(|deadline| {
            deadline.user == source && deadline.deadline == first_deadline
        }));
    }
    assert!(Arc::ptr_eq(&entry, &active_entry(&traffic_manager, &source)));

    drop(second_session);
    let second_deadline = idle_state(&traffic_manager, &source);
    assert!(second_deadline > first_deadline);
    {
        let state = traffic_manager.shared.state.lock().unwrap();
        assert!(!state.cleanup_deadlines.iter().any(|deadline| {
            deadline.user == source && deadline.deadline == first_deadline
        }));
        assert!(state.cleanup_deadlines.iter().any(|deadline| {
            deadline.user == source && deadline.deadline == second_deadline
        }));
    }

    runtime::sleep(
        first_deadline.saturating_duration_since(Instant::now())
            + Duration::from_millis(40),
    )
    .await;
    assert!(traffic_manager.snapshot(&source).is_some());
    assert_eq!(idle_state(&traffic_manager, &source), second_deadline);

    assert!(wait_until_async(
        second_deadline.saturating_duration_since(Instant::now())
            + Duration::from_millis(150),
        || traffic_manager.snapshot(&source).is_none(),
    )
    .await);
}

#[tokio::test(flavor = "current_thread")]
async fn traffic_async_cleanup_dv_setter_only_affects_future_idle_transitions() {
    let traffic_manager = PnTrafficManager::new();
    traffic_manager.start_cleanup_task().unwrap();
    traffic_manager.set_retention(Duration::from_millis(300));
    let old_source = user(50);
    let old_session = traffic_manager.begin_session(&old_source, &user(51));
    drop(old_session);
    let old_idle = idle_state(&traffic_manager, &old_source);

    traffic_manager.set_retention(Duration::from_millis(50));
    assert_eq!(idle_state(&traffic_manager, &old_source), old_idle);
    let new_source = user(52);
    let new_session = traffic_manager.begin_session(&new_source, &user(53));
    drop(new_session);
    let new_idle = idle_state(&traffic_manager, &new_source);
    assert!(new_idle < old_idle);

    assert!(wait_until_async(Duration::from_millis(180), || {
        traffic_manager.snapshot(&new_source).is_none()
    })
    .await);
    assert!(traffic_manager.snapshot(&old_source).is_some());
    assert_eq!(idle_state(&traffic_manager, &old_source), old_idle);
}

#[test]
fn deadline_set_has_one_item_per_idle_user_and_only_final_session_makes_user_idle() {
    let traffic_manager = PnTrafficManager::new();
    traffic_manager.set_retention(Duration::from_secs(2));
    let source = user(60);
    let first_target = user(61);
    let second_target = user(62);
    let first = traffic_manager.begin_session(&source, &first_target);
    let second = traffic_manager.begin_session(&source, &second_target);

    drop(first);
    {
        let state = traffic_manager.shared.state.lock().unwrap();
        let source_state = state.users.get(&source).unwrap();
        assert_eq!(source_state.active_sessions, 1);
        assert_eq!(source_state.idle_deadline, None);
        assert_eq!(state.cleanup_deadlines.len(), 1);
        assert_eq!(state.cleanup_deadlines.first().unwrap().user, first_target);
    }

    drop(second);
    let state = traffic_manager.shared.state.lock().unwrap();
    assert_eq!(state.cleanup_deadlines.len(), 3);
    for tracked_user in [&source, &first_target, &second_target] {
        let user_state = state.users.get(tracked_user).unwrap();
        assert_eq!(user_state.active_sessions, 0);
        let idle_deadline = user_state.idle_deadline.unwrap();
        assert_eq!(
            state
                .cleanup_deadlines
                .iter()
                .filter(|deadline| {
                    &deadline.user == tracked_user
                        && deadline.deadline == idle_deadline
                })
                .count(),
            1
        );
    }
}

#[test]
fn same_source_and_target_count_as_one_lifecycle_participant() {
    let traffic_manager = PnTrafficManager::new();
    let same_user = user(1);
    let session = traffic_manager.begin_session(&same_user, &same_user);

    assert_eq!(
        traffic_manager
            .shared
            .state
            .lock()
            .unwrap()
            .users
            .get(&same_user)
            .unwrap()
            .active_sessions,
        1
    );
    drop(session);
    assert_eq!(traffic_manager.snapshot(&same_user), None);
}

#[tokio::test]
async fn retained_limit_policy_survives_live_entry_recreation_and_updates_live_entry() {
    let traffic_manager = PnTrafficManager::new();
    let source = user(1);
    let target = user(2);
    let initial = PnTrafficLimitConfig {
        tx_rate: NonZeroU32::new(10),
        tx_weight: NonZeroU32::new(1),
        rx_rate: None,
        rx_weight: None,
    };
    let updated = PnTrafficLimitConfig {
        tx_rate: NonZeroU32::new(20),
        tx_weight: NonZeroU32::new(2),
        rx_rate: NonZeroU32::new(30),
        rx_weight: NonZeroU32::new(3),
    };

    traffic_manager.set_user_limit(source.clone(), initial);
    assert_eq!(traffic_manager.snapshot(&source), None);
    let first_session = traffic_manager.begin_session(&source, &target);
    let first_entry = active_entry(&traffic_manager, &source);
    traffic_manager.set_user_limit(source.clone(), updated);
    assert_eq!(
        traffic_manager
            .shared
            .state
            .lock()
            .unwrap()
            .limit_configs
            .get(&source),
        Some(&updated)
    );
    let mut first_limit_session = first_entry.tx_limiter.new_limit_session();
    assert_eq!(first_limit_session.until_ready().await, 2);

    drop(first_session);
    assert_eq!(traffic_manager.snapshot(&source), None);
    let second_session = traffic_manager.begin_session(&source, &target);
    let second_entry = active_entry(&traffic_manager, &source);
    assert!(!Arc::ptr_eq(&first_entry, &second_entry));
    assert_eq!(
        traffic_manager
            .shared
            .state
            .lock()
            .unwrap()
            .limit_configs
            .get(&source),
        Some(&updated)
    );
    let mut second_limit_session = second_entry.tx_limiter.new_limit_session();
    assert_eq!(second_limit_session.until_ready().await, 2);

    drop(second_session);
}

#[tokio::test]
async fn concurrent_limit_setters_keep_live_limiter_aligned_with_retained_config() {
    let traffic_manager = PnTrafficManager::new();
    let source = user(1);
    let target = user(2);
    let session = traffic_manager.begin_session(&source, &target);
    let barrier = Arc::new(Barrier::new(3));
    let configs = [
        PnTrafficLimitConfig {
            tx_rate: None,
            tx_weight: NonZeroU32::new(11),
            rx_rate: None,
            rx_weight: None,
        },
        PnTrafficLimitConfig {
            tx_rate: None,
            tx_weight: NonZeroU32::new(22),
            rx_rate: None,
            rx_weight: None,
        },
    ];
    let mut handles = Vec::new();
    for config in configs {
        let traffic_manager = traffic_manager.clone();
        let source = source.clone();
        let barrier = barrier.clone();
        handles.push(thread::spawn(move || {
            barrier.wait();
            traffic_manager.set_user_limit(source, config);
        }));
    }
    barrier.wait();
    for handle in handles {
        handle.join().unwrap();
    }

    let retained = traffic_manager
        .shared
        .state
        .lock()
        .unwrap()
        .limit_configs
        .get(&source)
        .copied()
        .unwrap();
    let entry = active_entry(&traffic_manager, &source);
    let mut live_limit_session = entry.tx_limiter.new_limit_session();
    assert_eq!(
        live_limit_session.until_ready().await,
        retained.tx_weight.unwrap().get() as usize
    );

    drop(session);
}

#[tokio::test(flavor = "current_thread")]
async fn traffic_async_cleanup_dv_expiry_keeps_limit_policy_and_reapplies_it() {
    let traffic_manager = PnTrafficManager::new();
    traffic_manager.start_cleanup_task().unwrap();
    traffic_manager.set_retention(Duration::from_millis(60));
    let source = user(70);
    let target = user(71);
    let config = PnTrafficLimitConfig {
        tx_rate: None,
        tx_weight: NonZeroU32::new(7),
        rx_rate: None,
        rx_weight: None,
    };
    traffic_manager.set_user_limit(source.clone(), config);
    let first_session = traffic_manager.begin_session(&source, &target);
    let first_entry = active_entry(&traffic_manager, &source);
    drop(first_session);

    assert!(wait_until_async(Duration::from_millis(250), || {
        traffic_manager.snapshot(&source).is_none()
    })
    .await);
    assert_eq!(
        traffic_manager
            .shared
            .state
            .lock()
            .unwrap()
            .limit_configs
            .get(&source),
        Some(&config)
    );

    let second_session = traffic_manager.begin_session(&source, &target);
    let second_entry = active_entry(&traffic_manager, &source);
    assert!(!Arc::ptr_eq(&first_entry, &second_entry));
    let mut limit_session = second_entry.tx_limiter.new_limit_session();
    assert_eq!(limit_session.until_ready().await, 7);
    drop(second_session);
}

#[tokio::test(flavor = "current_thread")]
async fn traffic_async_cleanup_dv_shutdown_clears_and_rejects_late_tracking() {
    let traffic_manager = PnTrafficManager::new();
    traffic_manager.start_cleanup_task().unwrap();
    traffic_manager.set_retention(Duration::from_secs(2));
    let source = user(80);
    traffic_manager.set_user_limit(source.clone(), PnTrafficLimitConfig::default());
    let session = traffic_manager.begin_session(&source, &user(81));
    drop(session);
    assert!(!traffic_manager
        .shared
        .state
        .lock()
        .unwrap()
        .cleanup_deadlines
        .is_empty());

    traffic_manager.shutdown();
    traffic_manager.set_retention(Duration::from_secs(1));
    traffic_manager.set_user_limit(
        source.clone(),
        PnTrafficLimitConfig {
            tx_rate: None,
            tx_weight: NonZeroU32::new(9),
            rx_rate: None,
            rx_weight: None,
        },
    );
    {
        let state = traffic_manager.shared.state.lock().unwrap();
        assert!(state.shutdown);
        assert!(state.users.is_empty());
        assert!(state.limit_configs.is_empty());
        assert!(state.cleanup_deadlines.is_empty());
        assert_eq!(state.retention, Duration::ZERO);
    }
    assert!(traffic_manager.cleanup_task.lock().unwrap().is_none());

    let late_session = traffic_manager.begin_session(&source, &user(82));
    assert!(traffic_manager.shared.state.lock().unwrap().users.is_empty());
    drop(late_session);
    tokio::task::yield_now().await;
    assert!(traffic_manager.shared.state.lock().unwrap().users.is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn traffic_async_cleanup_dv_notify_preempts_later_deadline_for_new_earlier_deadline() {
    let traffic_manager = PnTrafficManager::new();
    traffic_manager.start_cleanup_task().unwrap();
    traffic_manager.set_retention(Duration::from_millis(800));
    let later_source = user(90);
    let later_target = user(91);
    let later_session = traffic_manager.begin_session(&later_source, &later_target);
    drop(later_session);

    runtime::sleep(Duration::from_millis(20)).await;
    traffic_manager.set_retention(Duration::from_millis(50));
    let earlier_source = user(92);
    let earlier_target = user(93);
    let earlier_session = traffic_manager.begin_session(&earlier_source, &earlier_target);
    drop(earlier_session);

    assert!(wait_until_async(Duration::from_millis(250), || {
        let state = traffic_manager.shared.state.lock().unwrap();
        !state.users.contains_key(&earlier_source)
            && !state.users.contains_key(&earlier_target)
    })
    .await);
    let state = traffic_manager.shared.state.lock().unwrap();
    assert!(state.users.contains_key(&later_source));
    assert!(state.users.contains_key(&later_target));
}

#[tokio::test(flavor = "current_thread")]
async fn traffic_async_cleanup_dv_fixed_batch_yields_then_remaining_work_progresses() {
    let traffic_manager = PnTrafficManager::new();
    let deadline = Instant::now()
        .checked_sub(Duration::from_millis(1))
        .unwrap();
    {
        let mut state = traffic_manager.shared.state.lock().unwrap();
        for value in 0..129u8 {
            let tracked_user = user(value);
            state.users.insert(
                tracked_user.clone(),
                PnTrafficUserState {
                    entry: Arc::new(PnUserTrafficEntry::new()),
                    active_sessions: 0,
                    idle_deadline: Some(deadline),
                },
            );
            assert!(state.cleanup_deadlines.insert(PnTrafficCleanupDeadline {
                deadline,
                user: tracked_user,
            }));
        }
    }

    assert!(matches!(
        PnTrafficManager::cleanup_action(&traffic_manager.shared),
        PnTrafficCleanupAction::Yield
    ));
    {
        let state = traffic_manager.shared.state.lock().unwrap();
        assert_eq!(state.users.len(), 129 - PN_TRAFFIC_CLEANUP_BATCH_SIZE);
        assert_eq!(
            state.cleanup_deadlines.len(),
            129 - PN_TRAFFIC_CLEANUP_BATCH_SIZE
        );
    }

    traffic_manager.start_cleanup_task().unwrap();
    let runtime_progress = tokio::spawn(async {
        tokio::task::yield_now().await;
        true
    });
    assert!(runtime_progress.await.unwrap());
    assert!(wait_until_async(Duration::from_millis(250), || {
        let state = traffic_manager.shared.state.lock().unwrap();
        state.users.is_empty() && state.cleanup_deadlines.is_empty()
    })
    .await);
}

#[tokio::test(flavor = "current_thread")]
async fn traffic_async_cleanup_dv_task_does_not_retain_manager() {
    let traffic_manager = PnTrafficManager::new();
    traffic_manager.start_cleanup_task().unwrap();
    let weak_manager = Arc::downgrade(&traffic_manager);

    drop(traffic_manager);
    tokio::task::yield_now().await;

    assert!(weak_manager.upgrade().is_none());
}

#[test]
fn session_guard_handles_manager_drop_before_session_drop() {
    let traffic_manager = PnTrafficManager::new();
    let orphan_session = traffic_manager.begin_session(&user(3), &user(4));
    drop(traffic_manager);
    drop(orphan_session);
}

#[cfg(feature = "x509")]
mod reverse_tcp_proxy_tests {
    include!("reverse_tcp_proxy_tests.rs");
}
