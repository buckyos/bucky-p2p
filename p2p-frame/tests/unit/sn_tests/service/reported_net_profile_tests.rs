#[test]
fn reported_net_profile_is_available_only_until_its_own_expiry() {
    let mgr = PeerManager::new();
    let remote_id = test_id(14);
    let cert = test_cert(remote_id.clone(), vec![wan_ep(7101)]);
    let observed_at = 1_000_000;
    let observed = wan_ep(7201);
    let profile =
        NatProfile::from_observations(&[observed, observed], observed_at, Duration::from_secs(1));

    mgr.add_or_update_peer_with_profile(
        &remote_id,
        &Some(cert),
        Vec::new(),
        &Vec::new(),
        profile.clone(),
    );

    let cached = mgr.find_peer(&remote_id).unwrap();
    assert_eq!(cached.fresh_net_profile(observed_at), Some(profile));
    assert!(cached.fresh_net_profile(observed_at + 1_000_001).is_none());
}

#[test]
fn registration_refresh_preserves_scheduler_owned_profile_until_explicit_invalidation() {
    let mgr = PeerManager::new();
    let remote_id = test_id(15);
    let cert = test_cert(remote_id.clone(), vec![wan_ep(7301)]);
    let observed_at = 1_000_000;
    let observed = wan_ep(7302);
    let profile = NatProfile::from_observations(
        &[observed, observed],
        observed_at,
        Duration::from_secs(2 * 60 * 60),
    );

    mgr.add_or_update_peer_with_profile(
        &remote_id,
        &Some(cert.clone()),
        Vec::new(),
        &Vec::new(),
        profile.clone(),
    );
    mgr.add_or_update_peer(
        &remote_id,
        &Some(cert),
        0,
        vec![(Protocol::Quic, 7400)],
        &vec![wan_ep(7401)],
    );
    assert_eq!(
        mgr.find_peer(&remote_id)
            .unwrap()
            .fresh_net_profile(observed_at + Duration::from_secs(11 * 60).as_micros() as u64),
        Some(profile)
    );

    assert!(mgr.invalidate_net_profile(&remote_id));
    assert!(mgr
        .find_peer(&remote_id)
        .unwrap()
        .fresh_net_profile(observed_at)
        .is_none());
}
