use crate::networks::{IncomingDatagram, TunnelDatagramWrite};

fn assert_data_creation_owners(
    active: &TcpTunnel,
    passive: &TcpTunnel,
    active_local: usize,
    passive_local: usize,
) {
    let active_snapshot = active.test_reverse_open_snapshot();
    let passive_snapshot = passive.test_reverse_open_snapshot();
    assert_eq!(active_snapshot.data_connections, 1);
    assert_eq!(passive_snapshot.data_connections, 1);
    assert_eq!(active_snapshot.locally_created_connections, active_local);
    assert_eq!(
        passive_snapshot.locally_created_connections,
        passive_local
    );
}

async fn open_datagram_channel(
    opening: &dyn Tunnel,
    accepting: &dyn Tunnel,
    purpose: TunnelPurpose,
) -> (TunnelDatagramWrite, IncomingDatagram) {
    let (tx, mut rx) = mpsc::channel(1);
    let callback: crate::networks::IncomingDatagramCallback = Arc::new(move |accepted| {
        let tx = tx.clone();
        Box::pin(async move {
            let _ = tx.send(accepted).await;
        })
    });
    accepting
        .listen_datagram(allow_all_listen_vports(), callback)
        .await
        .unwrap();
    let (opened, accepted) = tokio::join!(opening.open_datagram(purpose), rx.recv());
    (
        opened.unwrap(),
        accepted
            .expect("datagram callback should stay registered")
            .unwrap(),
    )
}

async fn release_stream_for_reuse(
    opened_stream: (TunnelStreamRead, TunnelStreamWrite),
    accepted_stream: IncomingStream,
    expected_purpose: TunnelPurpose,
    outbound: &[u8],
    inbound: &[u8],
    active: &TcpTunnel,
    passive: &TcpTunnel,
) {
    let (mut opened_read, mut opened_write) = opened_stream;
    let (purpose, mut accepted_read, mut accepted_write) = accepted_stream;
    assert_eq!(purpose, expected_purpose);

    opened_write.write_all(outbound).await.unwrap();
    let mut received = vec![0; outbound.len()];
    accepted_read.read_exact(&mut received).await.unwrap();
    assert_eq!(received, outbound);

    accepted_write.write_all(inbound).await.unwrap();
    let mut reply = vec![0; inbound.len()];
    opened_read.read_exact(&mut reply).await.unwrap();
    assert_eq!(reply, inbound);

    drop(opened_write);
    drop(accepted_write);
    drop(opened_read);
    drop(accepted_read);

    timeout(Duration::from_secs(5), async {
        loop {
            if active.test_reverse_open_snapshot().idle_connections == 1
                && passive.test_reverse_open_snapshot().idle_connections == 1
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("released data connection should return to both idle pools");
}

#[tokio::test]
async fn tcp_data_control_direction_priority_prefers_active_local_creation_and_reuses_it() {
    let (pair, opened, accepted) = setup_reverse_network_pair().await;
    listen_stream_collect(&*accepted, allow_all_listen_vports()).await;
    let active = concrete_tunnel(&pair.client_network, &*opened);
    let passive = concrete_tunnel(&pair.server_network, &*accepted);

    let first_purpose = purpose_of(3520);
    let (first_opened, first_accepted) =
        open_reverse_stream(&*opened, &*accepted, first_purpose.clone()).await;
    assert_data_creation_owners(&active, &passive, 1, 0);
    release_stream_for_reuse(
        first_opened,
        first_accepted,
        first_purpose,
        b"active-first",
        b"active-return",
        &active,
        &passive,
    )
    .await;

    let second_purpose = purpose_of(3521);
    let (second_opened, second_accepted) =
        open_reverse_stream(&*opened, &*accepted, second_purpose.clone()).await;
    assert_data_creation_owners(&active, &passive, 1, 0);
    assert_bidirectional_stream(
        second_opened,
        second_accepted,
        second_purpose,
        b"active-reuse",
        b"reuse-return",
    )
    .await;

    opened.close().unwrap();
    accepted.close().unwrap();
}

#[tokio::test]
async fn tcp_data_control_direction_priority_prefers_passive_peer_creation_and_reuses_it() {
    let (pair, opened, accepted) = setup_reverse_network_pair().await;
    listen_stream_collect(&*opened, allow_all_listen_vports()).await;
    let active = concrete_tunnel(&pair.client_network, &*opened);
    let passive = concrete_tunnel(&pair.server_network, &*accepted);

    let first_purpose = purpose_of(3522);
    let (first_opened, first_accepted) =
        open_reverse_stream(&*accepted, &*opened, first_purpose.clone()).await;
    assert_data_creation_owners(&active, &passive, 1, 0);
    release_stream_for_reuse(
        first_opened,
        first_accepted,
        first_purpose,
        b"passive-first",
        b"passive-return",
        &active,
        &passive,
    )
    .await;

    let second_purpose = purpose_of(3523);
    let (second_opened, second_accepted) =
        open_reverse_stream(&*accepted, &*opened, second_purpose.clone()).await;
    assert_data_creation_owners(&active, &passive, 1, 0);
    assert_bidirectional_stream(
        second_opened,
        second_accepted,
        second_purpose,
        b"passive-reuse",
        b"reuse-return",
    )
    .await;

    opened.close().unwrap();
    accepted.close().unwrap();
}

#[tokio::test]
async fn tcp_data_control_direction_priority_applies_passive_peer_creation_to_datagrams() {
    let (pair, opened, accepted) = setup_reverse_network_pair().await;
    let active = concrete_tunnel(&pair.client_network, &*opened);
    let passive = concrete_tunnel(&pair.server_network, &*accepted);
    let purpose = purpose_of(3528);
    let (mut writer, (accepted_purpose, mut reader)) =
        open_datagram_channel(&*accepted, &*opened, purpose.clone()).await;

    assert_eq!(accepted_purpose, purpose);
    assert_data_creation_owners(&active, &passive, 1, 0);
    writer.write_all(b"passive-datagram").await.unwrap();
    writer.shutdown().await.unwrap();
    let mut received = Vec::new();
    reader.read_to_end(&mut received).await.unwrap();
    assert_eq!(received, b"passive-datagram");

    opened.close().unwrap();
    accepted.close().unwrap();
}

#[tokio::test]
async fn tcp_data_control_direction_priority_falls_back_for_active_and_passive_opens() {
    {
        let (pair, opened, accepted) = setup_reverse_network_pair().await;
        listen_stream_collect(&*accepted, allow_all_listen_vports()).await;
        let active = concrete_tunnel(&pair.client_network, &*opened);
        let passive = concrete_tunnel(&pair.server_network, &*accepted);
        pair.server_network.close_all_listener().await.unwrap();

        let purpose = purpose_of(3524);
        let (opened_stream, accepted_stream) =
            open_reverse_stream(&*opened, &*accepted, purpose.clone()).await;
        assert_data_creation_owners(&active, &passive, 0, 1);
        assert_bidirectional_stream(
            opened_stream,
            accepted_stream,
            purpose,
            b"active-fallback",
            b"peer-created",
        )
        .await;
        opened.close().unwrap();
        accepted.close().unwrap();
    }

    {
        let (pair, opened, accepted) = setup_reverse_network_pair().await;
        listen_stream_collect(&*opened, allow_all_listen_vports()).await;
        let active = concrete_tunnel(&pair.client_network, &*opened);
        let passive = concrete_tunnel(&pair.server_network, &*accepted);
        pair.server_network.close_all_listener().await.unwrap();

        let purpose = purpose_of(3525);
        let (opened_stream, accepted_stream) =
            open_reverse_stream(&*accepted, &*opened, purpose.clone()).await;
        assert_data_creation_owners(&active, &passive, 0, 1);
        assert_bidirectional_stream(
            opened_stream,
            accepted_stream,
            purpose,
            b"passive-fallback",
            b"local-created",
        )
        .await;
        opened.close().unwrap();
        accepted.close().unwrap();
    }
}

#[tokio::test]
async fn tcp_data_control_direction_priority_labels_both_failed_directions_in_attempt_order() {
    let (pair, opened, accepted) = setup_reverse_network_pair().await;
    listen_stream_collect(&*opened, allow_all_listen_vports()).await;
    listen_stream_collect(&*accepted, allow_all_listen_vports()).await;
    pair.server_network.close_all_listener().await.unwrap();
    pair.client_network.close_all_listener().await.unwrap();

    let active_err = timeout(
        Duration::from_secs(10),
        opened.open_stream(purpose_of(3526)),
    )
    .await
    .expect("active dual failure should be bounded")
    .err()
    .expect("active open should fail when both listeners are closed");
    assert_eq!(active_err.code(), P2pErrorCode::ConnectFailed);
    let active_message = format!("{active_err:?}");
    let active_preferred = active_message
        .find("preferred local-created data connection failed")
        .expect("active error should identify its preferred direction");
    let active_fallback = active_message
        .find("fallback peer-created data connection failed")
        .expect("active error should identify its fallback direction");
    assert!(active_preferred < active_fallback);

    let passive_err = timeout(
        Duration::from_secs(10),
        accepted.open_stream(purpose_of(3527)),
    )
    .await
    .expect("passive dual failure should be bounded")
    .err()
    .expect("passive open should fail when both listeners are closed");
    assert_eq!(passive_err.code(), P2pErrorCode::ConnectFailed);
    let passive_message = format!("{passive_err:?}");
    let passive_preferred = passive_message
        .find("preferred peer-created data connection failed")
        .expect("passive error should identify its preferred direction");
    let passive_fallback = passive_message
        .find("fallback local-created data connection failed")
        .expect("passive error should identify its fallback direction");
    assert!(passive_preferred < passive_fallback);

    opened.close().unwrap();
    accepted.close().unwrap();
}
