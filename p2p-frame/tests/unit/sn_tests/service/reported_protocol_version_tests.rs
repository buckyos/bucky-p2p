#[test]
fn report_registration_tracks_known_zero_updates_and_removal() {
    let mgr = PeerManager::new();
    let remote_id = test_id(16);
    let cert = test_cert(remote_id.clone(), vec![wan_ep(7501)]);

    mgr.add_peer_info(remote_id.clone(), cert.clone());
    assert_eq!(mgr.find_peer(&remote_id).unwrap().protocol_version, None);

    mgr.add_or_update_peer(
        &remote_id,
        &Some(cert.clone()),
        0,
        vec![(Protocol::Quic, 7502)],
        &vec![lan_ep(7503)],
    );
    assert_eq!(mgr.find_peer(&remote_id).unwrap().protocol_version, Some(0));

    mgr.add_or_update_peer(
        &remote_id,
        &None,
        1,
        vec![(Protocol::Tcp, 7504)],
        &vec![lan_ep(7505)],
    );
    assert_eq!(mgr.find_peer(&remote_id).unwrap().protocol_version, Some(1));

    mgr.add_or_update_peer_with_profile(
        &remote_id,
        &Some(cert),
        Vec::new(),
        &Vec::new(),
        NatProfile::unknown(),
    );
    assert_eq!(mgr.find_peer(&remote_id).unwrap().protocol_version, Some(1));

    mgr.remove_peer(remote_id.clone());
    assert!(mgr.find_peer(&remote_id).is_none());
}

#[test]
fn report_without_certificate_cannot_create_version_only_entry() {
    let mgr = PeerManager::new();
    let remote_id = test_id(17);

    mgr.add_or_update_peer(&remote_id, &None, 1, Vec::new(), &Vec::new());

    assert!(mgr.find_peer(&remote_id).is_none());
}
