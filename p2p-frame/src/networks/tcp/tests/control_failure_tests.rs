const LEGACY_TCP_CONTROL_MAX_COMMAND_ID: u8 = 11;

#[cfg(feature = "x509")]
mod control_failure_tests {
    use super::*;
    use crate::networks::tcp::connection::{accept_connection, build_acceptor};
    use crate::networks::tcp::protocol::TcpTunnelWireEncode;
    use crate::runtime::{AsyncReadExt, AsyncWriteExt};
    use crate::tls::{DefaultTlsServerCertResolver, ServerCertResolverRef, TlsServerCertResolver};
    use crate::x509::{X509IdentityCertFactory, X509IdentityFactory, generate_rsa_x509_identity};
    use std::future::Future;
    use std::sync::LazyLock;
    use tokio::sync::Barrier;
    use tokio::time::timeout;

    static IDENTITIES: LazyLock<(P2pIdentityRef, P2pIdentityRef)> = LazyLock::new(|| {
        crate::tls::init_tls(Arc::new(X509IdentityFactory));
        (
            Arc::new(generate_rsa_x509_identity(Some("failure-client".into())).unwrap()),
            Arc::new(generate_rsa_x509_identity(Some("failure-server".into())).unwrap()),
        )
    });

    async fn tls_pair() -> (TcpTlsConnection, TcpTlsConnection) {
        let (client_identity, server_identity) = &*IDENTITIES;
        let resolver = DefaultTlsServerCertResolver::new();
        resolver.add_server_identity(server_identity.clone()).await.unwrap();
        let resolver: ServerCertResolverRef = resolver;
        let factory: P2pIdentityCertFactoryRef = Arc::new(X509IdentityCertFactory);
        let acceptor = build_acceptor(resolver.clone(), factory.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let remote = Endpoint::from((crate::endpoint::Protocol::Tcp, listener.local_addr().unwrap()));
        let remote_id = server_identity.get_id();
        let hello = TcpConnectionHello {
            role: TcpConnectionRole::Control,
            tunnel_id: TunnelId::from(81),
            candidate_id: TunnelCandidateId::from(81),
            is_reverse: false,
            conn_id: None,
            open_request_id: None,
        };
        timeout(Duration::from_secs(10), async {
            let (client, server) = tokio::join!(
                connect_with_optional_local(factory.clone(), client_identity, None, &remote,
                    &remote_id, Some(server_identity.get_name()),
                    Duration::from_secs(5), false, &hello),
                async {
                    let (socket, _) = listener.accept().await.unwrap();
                    accept_connection(&acceptor, &factory, &resolver, socket).await
                }
            );
            (client.unwrap(), server.unwrap().0)
        }).await.expect("loopback TLS fixture must connect")
    }

    struct Fixture {
        tunnel: Arc<TcpTunnel>,
        control_peer: TcpTlsConnection,
    }

    impl Fixture {
        async fn new() -> Self {
            Self::with_heartbeat(Duration::from_secs(3600)).await
        }

        async fn with_heartbeat(interval: Duration) -> Self {
            let (control, control_peer) = tls_pair().await;
            let connector = TcpTunnelConnector {
                cert_factory: Arc::new(X509IdentityCertFactory),
                local_identity: IDENTITIES.0.clone(),
                local_ep: control.local_ep,
                remote_ep: Arc::new(Mutex::new(control.remote_ep)),
                remote_id: control.remote_id.clone(),
                remote_name: Some(control.remote_name.clone()),
                timeout: Duration::from_secs(3),
                tunnel_id: TunnelId::from(81),
                candidate_id: TunnelCandidateId::from(81),
            };
            let tunnel = TcpTunnel::new(control, connector, TunnelId::from(81),
                TunnelCandidateId::from(81), TunnelForm::Passive, false,
                interval, Duration::from_secs(7200), LocalTunnelPhase::Connected);
            Self { tunnel, control_peer }
        }

        async fn entry(&self, id: u32) -> (Arc<DataConnEntry>, TcpTlsConnection) {
            let (data, peer) = tls_pair().await;
            socket2::SockRef::from(data.stream.get_ref().0).set_send_buffer_size(4096).unwrap();
            socket2::SockRef::from(peer.stream.get_ref().0).set_recv_buffer_size(4096).unwrap();
            (DataConnEntry::new(data, TcpConnId::from(id), true, true), peer)
        }

        async fn lease(&self, kind: TcpChannelKind) ->
            (Arc<DataConnEntry>, Arc<Mutex<LeaseContext>>, TcpTlsConnection) {
            let (entry, peer) = self.entry(1).await;
            self.tunnel.register_entry(entry.clone()).unwrap();
            let lease = self.tunnel.bind_entry_to_lease(&entry, TcpChannelId::from(1),
                TcpLeaseSeq::from(1), kind, TunnelPurpose::from_value(&81u16).unwrap(), true).unwrap();
            (entry, lease, peer)
        }
    }

    async fn until(mut predicate: impl FnMut() -> bool) {
        timeout(Duration::from_secs(2), async {
            while !predicate() { tokio::time::sleep(Duration::from_millis(1)).await; }
        }).await.expect("state must converge without waiting for a kernel timeout");
    }

    async fn pending_read(lease: Arc<Mutex<LeaseContext>>) -> tokio::task::JoinHandle<std::io::Result<usize>> {
        let (tx, rx) = oneshot::channel();
        let task = tokio::spawn(async move {
            let mut read = TcpLeaseRead { lease };
            let mut buf = [0; 1];
            let mut future = Box::pin(read.read(&mut buf));
            let mut tx = Some(tx);
            std::future::poll_fn(|cx| {
                let result = future.as_mut().poll(cx);
                if result.is_pending() {
                    if let Some(tx) = tx.take() { let _ = tx.send(()); }
                }
                result
            }).await
        });
        timeout(Duration::from_secs(2), rx).await.unwrap().unwrap();
        task
    }

    fn fully_drained(lease: &Arc<Mutex<LeaseContext>>) {
        let mut inner = lease.lock().unwrap();
        inner.local_write_fin_sent = true;
        inner.peer_write_fin_received = true;
        inner.local_read_done_sent = true;
        inner.peer_read_done_received = true;
        inner.read_released = true;
        inner.write_released = true;
    }

    fn assert_terminal_callers(tunnel: &TcpTunnel, entry: &DataConnEntry) {
        let state = tunnel.state.lock().unwrap();
        assert_eq!(state.phase, LocalTunnelPhase::Error);
        assert!(state.pending_claims.is_empty());
        assert!(state.pending_open_requests.is_empty());
        assert!(state.data_conns.is_empty());
        drop(state);
        assert!(matches!(entry.inner.lock().unwrap().state, DataConnEntryState::Retired));
        assert!(entry.stream.lock().unwrap().is_none());
    }

    // Uses only pre-repair private interfaces so exactly this test can replay
    // against round 1, which already retired data but left real callers stuck.
    #[tokio::test]
    async fn dv_actual_claim_and_reverse_open_cancel_while_control_mutex_is_held() {
        let fixture = Fixture::new().await;
        let (entry, _peer) = fixture.entry(31).await;
        fixture.tunnel.register_entry(entry.clone()).unwrap();
        let write_guard = fixture.tunnel.control_write.lock().await;
        let claim = tokio::spawn({
            let tunnel = fixture.tunnel.clone();
            let entry = entry.clone();
            async move { tunnel.claim_entry(entry, TcpChannelKind::Stream,
                TunnelPurpose::from_value(&81u16).unwrap()).await }
        });
        let open = tokio::spawn({
            let tunnel = fixture.tunnel.clone();
            async move { tunnel.request_remote_data_connection().await }
        });
        until(|| {
            let state = fixture.tunnel.state.lock().unwrap();
            state.pending_claims.len() == 1 && state.pending_open_requests.len() == 1
        }).await;
        assert!(!claim.is_finished() && !open.is_finished());
        fixture.tunnel.close_local(LocalTunnelPhase::Error, P2pErrorCode::Timeout);
        let claim_result = timeout(Duration::from_secs(1), claim).await.expect("actual claim caller must cancel while control writer remains locked").unwrap();
        assert!(claim_result.is_err());
        assert!(timeout(Duration::from_secs(1), open).await.unwrap().unwrap().is_err());
        timeout(Duration::from_secs(1), fixture.tunnel.close_impl(
            LocalTunnelPhase::Error, P2pErrorCode::Timeout)).await.unwrap().unwrap();
        assert_terminal_callers(&fixture.tunnel, &entry);
        drop(write_guard);
        tokio::task::yield_now().await;
        assert_terminal_callers(&fixture.tunnel, &entry);
    }

    #[tokio::test]
    async fn integration_control_socket_backpressure_eof_cancels_actual_callers() {
        let fixture = Fixture::new().await;
        socket2::SockRef::from(fixture.control_peer.stream.get_ref().0)
            .set_recv_buffer_size(4096).unwrap();
        let (entry, mut data_peer) = fixture.entry(32).await;
        fixture.tunnel.register_entry(entry.clone()).unwrap();
        let writes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let pending_polls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (pending_tx, pending_rx) = oneshot::channel();
        let writer = tokio::spawn({
            let tunnel = fixture.tunnel.clone();
            let writes = writes.clone();
            let pending_polls = pending_polls.clone();
            async move {
                let mut pending_tx = Some(pending_tx);
                for _ in 0..1024 {
                    let command = TcpControlCmd::Data(TcpControlData { payload: vec![0x81; 32768] });
                    let mut send = Box::pin(tunnel.send_control(&command));
                    let result = std::future::poll_fn(|cx| {
                        let result = send.as_mut().poll(cx);
                        if result.is_pending() {
                            pending_polls.fetch_add(1, Ordering::SeqCst);
                            if let Some(tx) = pending_tx.take() { let _ = tx.send(()); }
                        }
                        result
                    }).await;
                    result?;
                    writes.fetch_add(1, Ordering::SeqCst);
                }
                Ok::<(), P2pError>(())
            }
        });
        timeout(Duration::from_secs(3), pending_rx).await.unwrap().unwrap();
        until(|| writes.load(Ordering::SeqCst) > 0 && fixture.tunnel.control_write.try_lock().is_err()).await;
        let completed = timeout(Duration::from_secs(3), async {
            loop {
                let completed = writes.load(Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(150)).await;
                if completed == writes.load(Ordering::SeqCst)
                    && fixture.tunnel.control_write.try_lock().is_err() && !writer.is_finished()
                { break completed; }
            }
        }).await.expect("unread real control socket must stop command progress");
        assert!(completed > 0);
        assert!(pending_polls.load(Ordering::SeqCst) > 0 && !writer.is_finished());
        assert!(fixture.tunnel.control_write.try_lock().is_err(), "Pending send must own the control write mutex");
        let claim = tokio::spawn({
            let tunnel = fixture.tunnel.clone();
            let entry = entry.clone();
            async move { tunnel.claim_entry(entry, TcpChannelKind::Stream,
                TunnelPurpose::from_value(&81u16).unwrap()).await }
        });
        let open = tokio::spawn({
            let tunnel = fixture.tunnel.clone();
            async move { tunnel.request_remote_data_connection().await }
        });
        until(|| {
            let state = fixture.tunnel.state.lock().unwrap();
            state.pending_claims.len() == 1 && state.pending_open_requests.len() == 1
        }).await;
        // Half-close only the peer's sending direction. Its unread receiving
        // direction stays owned by this fixture and never relieves backpressure.
        socket2::SockRef::from(fixture.control_peer.stream.get_ref().0)
            .shutdown(std::net::Shutdown::Write).unwrap();
        assert!(timeout(Duration::from_secs(1), claim).await.unwrap().unwrap().is_err());
        assert!(timeout(Duration::from_secs(1), open).await.unwrap().unwrap().is_err());
        assert!(timeout(Duration::from_secs(1), writer).await.unwrap().unwrap().is_err());
        assert_terminal_callers(&fixture.tunnel, &entry);
        let mut byte = [0];
        let result = timeout(Duration::from_secs(1), data_peer.stream.read(&mut byte)).await.unwrap();
        assert!(result.is_err() || result.unwrap() == 0, "claim's real data socket must be released");
        // Keep control_peer alive through all completion assertions.
        assert!(fixture.control_peer.stream.get_ref().0.peer_addr().is_ok());
    }

    #[tokio::test]
    async fn unit_closed_tunnel_rejects_actual_claim_and_reverse_registration() {
        let fixture = Fixture::new().await;
        let (entry, _peer) = fixture.entry(33).await;
        fixture.tunnel.register_entry(entry.clone()).unwrap();
        fixture.tunnel.close_local(LocalTunnelPhase::Error, P2pErrorCode::IoError);
        let write_guard = fixture.tunnel.control_write.lock().await;
        assert!(timeout(Duration::from_millis(100), fixture.tunnel.claim_entry(
            entry.clone(), TcpChannelKind::Stream, TunnelPurpose::from_value(&81u16).unwrap()))
            .await.unwrap().is_err());
        assert!(timeout(Duration::from_millis(100), fixture.tunnel.request_remote_data_connection())
            .await.unwrap().is_err());
        assert!(!fixture.tunnel.close_local(LocalTunnelPhase::Error, P2pErrorCode::Timeout));
        assert_terminal_callers(&fixture.tunnel, &entry);
        drop(write_guard);
    }

    #[tokio::test]
    async fn dv_passive_waiters_observe_promotion_error_and_late_terminal_state() {
        for connected in [true, false] {
            let fixture = Fixture::new().await;
            fixture.tunnel.state.lock().unwrap().phase = LocalTunnelPhase::PassiveReady;
            let (tx, rx) = oneshot::channel();
            let waiter = tokio::spawn({
                let tunnel = fixture.tunnel.clone();
                async move {
                    let mut waiting = Box::pin(tunnel.wait_until_connected());
                    let mut tx = Some(tx);
                    std::future::poll_fn(|cx| {
                        let result = waiting.as_mut().poll(cx);
                        if result.is_pending() {
                            if let Some(tx) = tx.take() { let _ = tx.send(()); }
                        }
                        result
                    }).await
                }
            });
            timeout(Duration::from_secs(1), rx).await.unwrap().unwrap();
            if connected { fixture.tunnel.promote_connected(); }
            else { fixture.tunnel.close_local(LocalTunnelPhase::Error, P2pErrorCode::Timeout); }
            let result = timeout(Duration::from_secs(1), waiter).await.unwrap().unwrap();
            assert_eq!(result.is_ok(), connected);
            assert_eq!(timeout(Duration::from_millis(100), fixture.tunnel.wait_until_connected())
                .await.unwrap().is_ok(), connected);
            fixture.tunnel.close_local(LocalTunnelPhase::Error, P2pErrorCode::Interrupted);
        }
    }

    #[tokio::test]
    async fn integration_passive_public_open_pending_at_error_completes() {
        let fixture = Fixture::new().await;
        fixture.tunnel.state.lock().unwrap().phase = LocalTunnelPhase::PassiveReady;
        let (tx, rx) = oneshot::channel();
        let open = tokio::spawn({
            let tunnel = fixture.tunnel.clone();
            async move {
                let mut opening = Box::pin(tunnel.open_stream(TunnelPurpose::from_value(&81u16).unwrap()));
                let mut tx = Some(tx);
                std::future::poll_fn(|cx| {
                    let result = opening.as_mut().poll(cx);
                    if result.is_pending() {
                        if let Some(tx) = tx.take() { let _ = tx.send(()); }
                    }
                    result
                }).await
            }
        });
        timeout(Duration::from_secs(1), rx).await.unwrap().unwrap();
        fixture.tunnel.close_local(LocalTunnelPhase::Error, P2pErrorCode::IoError);
        assert!(timeout(Duration::from_secs(1), open).await.unwrap().unwrap().is_err());
        assert!(timeout(Duration::from_millis(100), fixture.tunnel.open_stream(
            TunnelPurpose::from_value(&81u16).unwrap())).await.unwrap().is_err());
        let state = fixture.tunnel.state.lock().unwrap();
        assert!(state.pending_open_requests.is_empty() && state.pending_claims.is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dv_actual_registration_and_passive_promotion_race_terminal_drain() {
        for _ in 0..4 {
            let fixture = Fixture::new().await;
            let (entry, _peer) = fixture.entry(34).await;
            fixture.tunnel.register_entry(entry.clone()).unwrap();
            let guard = fixture.tunnel.control_write.lock().await;
            let barrier = Arc::new(Barrier::new(3));
            let claim = tokio::spawn({
                let tunnel = fixture.tunnel.clone();
                let entry = entry.clone();
                let barrier = barrier.clone();
                async move {
                    barrier.wait().await;
                    tunnel.claim_entry(entry, TcpChannelKind::Stream,
                        TunnelPurpose::from_value(&81u16).unwrap()).await
                }
            });
            let open = tokio::spawn({
                let tunnel = fixture.tunnel.clone();
                let barrier = barrier.clone();
                async move {
                    barrier.wait().await;
                    tunnel.request_remote_data_connection().await
                }
            });
            barrier.wait().await;
            fixture.tunnel.close_local(LocalTunnelPhase::Error, P2pErrorCode::Timeout);
            assert!(timeout(Duration::from_secs(1), claim).await.unwrap().unwrap().is_err());
            assert!(timeout(Duration::from_secs(1), open).await.unwrap().unwrap().is_err());
            assert_terminal_callers(&fixture.tunnel, &entry);
            drop(guard);

            let fixture = Fixture::new().await;
            fixture.tunnel.state.lock().unwrap().phase = LocalTunnelPhase::PassiveReady;
            let barrier = Arc::new(Barrier::new(3));
            let waiter = tokio::spawn({
                let tunnel = fixture.tunnel.clone();
                let barrier = barrier.clone();
                async move { barrier.wait().await; tunnel.wait_until_connected().await }
            });
            let promotion = tokio::spawn({
                let tunnel = fixture.tunnel.clone();
                let barrier = barrier.clone();
                async move { barrier.wait().await; tunnel.promote_connected(); }
            });
            barrier.wait().await;
            fixture.tunnel.close_local(LocalTunnelPhase::Error, P2pErrorCode::Timeout);
            timeout(Duration::from_secs(1), waiter).await.unwrap().unwrap().ok();
            timeout(Duration::from_secs(1), promotion).await.unwrap().unwrap();
            assert_eq!(fixture.tunnel.state.lock().unwrap().phase, LocalTunnelPhase::Error);
        }
    }

    #[tokio::test]
    async fn unit_negative_claim_ack_preserves_current_sequence_and_terminal_state() {
        let fixture = Fixture::new().await;
        for (id, committed, no_reuse, mismatch, result) in [
            (41, 0, false, false, ClaimConnAckResult::ConflictLost),
            (42, 1, false, false, ClaimConnAckResult::VportNotFound),
            (43, 0, true, false, ClaimConnAckResult::ConflictLost),
            (44, 1, true, false, ClaimConnAckResult::VportNotFound),
            (45, 0, false, true, ClaimConnAckResult::VportNotFound),
        ] {
            let (entry, _peer) = fixture.entry(id).await;
            fixture.tunnel.register_entry(entry.clone()).unwrap();
            let channel_id = TcpChannelId::from(id);
            let sequence = TcpLeaseSeq::from(committed + 1);
            {
                let mut inner = entry.inner.lock().unwrap();
                inner.committed_lease_seq = TcpLeaseSeq::from(committed);
                inner.no_reuse = no_reuse;
                inner.state = DataConnEntryState::Claiming(ClaimingState {
                    channel_id, lease_seq: sequence, claim_nonce: 81,
                    kind: TcpChannelKind::Stream, purpose: TunnelPurpose::from_value(&81u16).unwrap(),
                });
            }
            let (tx, mut rx) = oneshot::channel();
            fixture.tunnel.state.lock().unwrap().pending_claims.insert(channel_id, tx);
            fixture.tunnel.handle_claim_ack(ClaimConnAck {
                channel_id: TcpChannelId::from(99), conn_id: entry.conn_id,
                lease_seq: sequence, result: result as u8,
            }).await.unwrap();
            assert!(matches!(entry.inner.lock().unwrap().state, DataConnEntryState::Claiming(_)));
            fixture.tunnel.handle_claim_ack(ClaimConnAck {
                channel_id, conn_id: entry.conn_id,
                lease_seq: if mismatch { TcpLeaseSeq::from(99) } else { sequence }, result: result as u8,
            }).await.unwrap();
            if no_reuse || mismatch {
                assert!(matches!(entry.inner.lock().unwrap().state, DataConnEntryState::Claiming(_)));
                assert!(matches!(rx.try_recv(), Err(oneshot::error::TryRecvError::Empty)));
            } else {
                assert!(timeout(Duration::from_secs(1), rx).await.unwrap().unwrap().is_err());
                let inner = entry.inner.lock().unwrap();
                assert!(if committed == 0 { matches!(inner.state, DataConnEntryState::FirstClaimPending) }
                    else { matches!(inner.state, DataConnEntryState::Idle) });
            }
            fixture.tunnel.retire_plain_entry(&entry, P2pErrorCode::Interrupted);
            fixture.tunnel.handle_claim_ack(ClaimConnAck { channel_id, conn_id: entry.conn_id,
                lease_seq: sequence, result: result as u8 }).await.unwrap();
            assert!(matches!(entry.inner.lock().unwrap().state, DataConnEntryState::Retired));
            assert!(entry.stream.lock().unwrap().is_none());
            fixture.tunnel.state.lock().unwrap().pending_claims.remove(&channel_id);
        }
        fixture.tunnel.close_local(LocalTunnelPhase::Error, P2pErrorCode::Interrupted);
        assert!(fixture.tunnel.state.lock().unwrap().pending_claims.is_empty());
    }

    #[tokio::test]
    async fn dv_control_eof_wakes_borrowed_read_and_closes_data_socket() {
        let mut fixture = Fixture::new().await;
        let (_entry, lease, mut peer) = fixture.lease(TcpChannelKind::Stream).await;
        let read = pending_read(lease.clone()).await;
        let mut write = TcpLeaseWrite { lease: lease.clone() };
        fixture.control_peer.stream.shutdown().await.unwrap();
        let result = timeout(Duration::from_secs(1), read).await.unwrap().unwrap();
        assert!(result.is_err(), "borrowed read must fail after control EOF");
        assert!(write.write_all(b"stale-network-data").await.is_err());
        assert_eq!(fixture.tunnel.state.lock().unwrap().phase, LocalTunnelPhase::Error);
        assert!(fixture.tunnel.state.lock().unwrap().data_conns.is_empty());
        let mut byte = [0];
        let result = timeout(Duration::from_secs(1), peer.stream.read(&mut byte)).await.unwrap();
        assert!(result.is_err() || result.unwrap() == 0, "actual data socket must close");
    }

    #[tokio::test]
    async fn dv_heartbeat_timeout_retires_borrowed_data_and_wakes_reader() {
        let fixture = Fixture::with_heartbeat(Duration::from_millis(10)).await;
        let (_entry, lease, _peer) = fixture.lease(TcpChannelKind::Stream).await;
        let pending = pending_read(lease.clone()).await;
        {
            let mut state = fixture.tunnel.state.lock().unwrap();
            state.last_recv_at = Instant::now() - Duration::from_secs(7201);
            state.last_pong_at = Instant::now() - Duration::from_secs(7201);
        }
        assert!(timeout(Duration::from_secs(1), pending).await.unwrap().unwrap().is_err());
        assert_eq!(fixture.tunnel.state.lock().unwrap().phase, LocalTunnelPhase::Error);
        assert!(lease.lock().unwrap().retired);
    }

    #[tokio::test]
    async fn dv_protocol_error_retires_draining_lease_and_fails_claim_waiter() {
        let mut fixture = Fixture::new().await;
        let (entry, lease, _peer) = fixture.lease(TcpChannelKind::Stream).await;
        entry.inner.lock().unwrap().state = DataConnEntryState::Draining(lease.clone());
        let (claim_tx, claim_rx) = oneshot::channel();
        fixture.tunnel.state.lock().unwrap().pending_claims.insert(TcpChannelId::from(99), claim_tx);
        let read = pending_read(lease.clone()).await;
        let mut malformed = TcpControlCmd::Ping(PingCmd { seq: 1, send_time: 0 }).encode_wire().unwrap();
        malformed[1] = u8::MAX;
        fixture.control_peer.stream.write_all(&malformed).await.unwrap();
        fixture.control_peer.stream.flush().await.unwrap();
        assert!(timeout(Duration::from_secs(1), claim_rx).await.unwrap().unwrap().is_err());
        assert!(timeout(Duration::from_secs(1), read).await.unwrap().unwrap().is_err());
        assert!(lease.lock().unwrap().retired);
        assert!(fixture.tunnel.state.lock().unwrap().pending_claims.is_empty());
    }

    #[tokio::test]
    async fn dv_pending_socket_write_wakes_on_internal_error() {
        let fixture = Fixture::new().await;
        let (_entry, lease, _peer) = fixture.lease(TcpChannelKind::Stream).await;
        let (tx, rx) = oneshot::channel();
        let write = tokio::spawn({
            let lease = lease.clone();
            async move {
                let mut write = TcpLeaseWrite { lease };
                let payload = vec![0x5a; 8 * 1024 * 1024];
                let mut future = Box::pin(write.write_all(&payload));
                let mut tx = Some(tx);
                std::future::poll_fn(|cx| {
                    let result = future.as_mut().poll(cx);
                    if result.is_pending() {
                        if let Some(tx) = tx.take() { let _ = tx.send(()); }
                    }
                    result
                }).await
            }
        });
        timeout(Duration::from_secs(2), rx).await.unwrap().unwrap();
        assert!(!write.is_finished(), "fixture must establish an actual blocked writer");
        fixture.tunnel.close_local(LocalTunnelPhase::Error, P2pErrorCode::Timeout);
        assert!(timeout(Duration::from_secs(1), write).await.unwrap().unwrap().is_err());
    }

    #[tokio::test]
    async fn unit_physical_and_logical_control_write_failure_retire_owned_data() {
        for logical in [false, true] {
            let fixture = Fixture::new().await;
            let (_entry, lease, _peer) = fixture.lease(TcpChannelKind::Stream).await;
            let pending = pending_read(lease.clone()).await;
            {
                let mut write = fixture.tunnel.control_write.lock().await;
                write.shutdown().await.unwrap();
            }
            assert_eq!(fixture.tunnel.state.lock().unwrap().phase, LocalTunnelPhase::Connected);
            if logical {
                let purpose = TunnelPurpose::from_value(&81u16).unwrap();
                assert!(fixture.tunnel.control_streams.open(purpose).await.is_err());
            } else {
                assert_eq!(fixture.tunnel.send_ping().await.unwrap_err().code(), P2pErrorCode::IoError);
            }
            assert_eq!(fixture.tunnel.state.lock().unwrap().phase, LocalTunnelPhase::Error);
            assert!(timeout(Duration::from_secs(1), pending).await.unwrap().unwrap().is_err());
            assert!(lease.lock().unwrap().retired);
        }
    }

    #[tokio::test]
    async fn unit_retirement_wakes_registered_reader_outside_owner_locks() {
        struct ReentrantWake {
            lease: Weak<Mutex<LeaseContext>>,
            entry: Weak<DataConnEntry>,
            tunnel: Weak<TcpTunnel>,
            wakes: AtomicU32,
            expect_unlocked: AtomicBool,
        }
        impl std::task::Wake for ReentrantWake {
            fn wake(self: Arc<Self>) { self.wake_by_ref(); }
            fn wake_by_ref(self: &Arc<Self>) {
                if self.expect_unlocked.load(Ordering::SeqCst) {
                    let (Some(lease), Some(entry), Some(tunnel)) =
                        (self.lease.upgrade(), self.entry.upgrade(), self.tunnel.upgrade()) else { return; };
                    assert!(lease.try_lock().is_ok());
                    assert!(entry.inner.try_lock().is_ok());
                    assert!(tunnel.state.try_lock().is_ok());
                    self.wakes.fetch_add(1, Ordering::SeqCst);
                }
            }
        }
        let fixture = Fixture::new().await;
        let (entry, lease, _peer) = fixture.lease(TcpChannelKind::Stream).await;
        let wake = Arc::new(ReentrantWake { lease: Arc::downgrade(&lease),
            entry: Arc::downgrade(&entry), tunnel: Arc::downgrade(&fixture.tunnel),
            wakes: AtomicU32::new(0), expect_unlocked: AtomicBool::new(false) });
        let waker = std::task::Waker::from(wake.clone());
        let mut cx = Context::from_waker(&waker);
        let mut read = TcpLeaseRead { lease: lease.clone() };
        let mut byte = [0];
        let mut buf = tokio::io::ReadBuf::new(&mut byte);
        assert!(runtime::AsyncRead::poll_read(Pin::new(&mut read), &mut cx, &mut buf).is_pending());
        wake.expect_unlocked.store(true, Ordering::SeqCst);
        TcpTunnel::retire_lease_arc(&lease, P2pErrorCode::Interrupted);
        assert!(wake.wakes.load(Ordering::SeqCst) > 0);
    }

    #[tokio::test]
    async fn unit_repeated_close_is_idempotent_and_all_retired_io_is_terminal() {
        let fixture = Fixture::new().await;
        let (_entry, lease, _peer) = fixture.lease(TcpChannelKind::Stream).await;
        assert!(fixture.tunnel.close_local(LocalTunnelPhase::Error, P2pErrorCode::Interrupted));
        assert!(!fixture.tunnel.close_local(LocalTunnelPhase::Error, P2pErrorCode::Timeout));
        assert!(!fixture.tunnel.close_local(LocalTunnelPhase::Closed, P2pErrorCode::Interrupted));
        assert!(lease.lock().unwrap().retired);
        assert!(lease.lock().unwrap().read.is_none());
        assert!(lease.lock().unwrap().write.is_none());
        let mut read = TcpLeaseRead { lease: lease.clone() };
        let mut write = TcpLeaseWrite { lease: lease.clone() };
        assert!(read.read(&mut [0]).await.is_err());
        assert!(write.write_all(b"x").await.is_err());
        assert!(write.flush().await.is_err());
        assert!(write.shutdown().await.is_err());
        assert_eq!(fixture.tunnel.state.lock().unwrap().phase, LocalTunnelPhase::Error);
        assert!(lease.lock().unwrap().read.is_none());
        assert!(lease.lock().unwrap().write.is_none());
    }

    #[tokio::test]
    async fn integration_graceful_close_preserves_borrowed_bidirectional_io_until_release() {
        let fixture = Fixture::new().await;
        let (_entry, lease, mut peer) = fixture.lease(TcpChannelKind::Stream).await;
        let (mut read, mut write) = fixture.tunnel.make_stream_channel(lease.clone());
        fixture.tunnel.close().unwrap();
        assert!(!lease.lock().unwrap().retired);
        assert!(lease.lock().unwrap().no_reuse);
        write.write_all(b"out").await.unwrap();
        write.flush().await.unwrap();
        let mut bytes = [0; 3];
        peer.stream.read_exact(&mut bytes).await.unwrap();
        assert_eq!(&bytes, b"out");
        peer.stream.write_all(b"in!").await.unwrap();
        peer.stream.flush().await.unwrap();
        read.read_exact(&mut bytes).await.unwrap();
        assert_eq!(&bytes, b"in!");
        drop(read);
        drop(write);
        until(|| lease.lock().unwrap().retired).await;
        assert_eq!(fixture.tunnel.state.lock().unwrap().phase, LocalTunnelPhase::Closed);
        assert!(fixture.tunnel.state.lock().unwrap().data_conns.is_empty());
    }

    #[tokio::test]
    async fn integration_data_consumer_success_then_internal_failure() {
        let fixture = Fixture::new().await;
        let (_entry, lease, mut peer) = fixture.lease(TcpChannelKind::Stream).await;
        let (mut read, mut write) = fixture.tunnel.make_stream_channel(lease);
        write.write_all(b"ok").await.unwrap();
        write.flush().await.unwrap();
        let mut bytes = [0; 2];
        peer.stream.read_exact(&mut bytes).await.unwrap();
        assert_eq!(&bytes, b"ok");
        fixture.tunnel.close_local(LocalTunnelPhase::Error, P2pErrorCode::IoError);
        assert!(timeout(Duration::from_secs(1), read.read(&mut bytes)).await.unwrap().is_err());
        assert!(write.write_all(b"retry-through-new-tunnel").await.is_err());
    }

    #[tokio::test]
    async fn dv_hidden_datagram_observer_drops_all_halves_on_error() {
        let fixture = Fixture::new().await;
        let (_entry, lease, _peer) = fixture.lease(TcpChannelKind::Datagram).await;
        tokio::time::sleep(Duration::from_millis(20)).await;
        fixture.tunnel.close_local(LocalTunnelPhase::Error, P2pErrorCode::Timeout);
        {
            let inner = lease.lock().unwrap();
            assert!(inner.retired);
            assert!(inner.read.is_none() && inner.write.is_none());
        }
        until(|| Arc::strong_count(&lease) == 1).await;
        assert!(fixture.tunnel.state.lock().unwrap().data_conns.is_empty());
    }

    #[tokio::test]
    async fn unit_late_registration_and_binding_cannot_reopen_closed_tunnel() {
        let fixture = Fixture::new().await;
        let (entry, _peer) = fixture.entry(2).await;
        fixture.tunnel.close_local(LocalTunnelPhase::Error, P2pErrorCode::Timeout);
        assert!(fixture.tunnel.register_entry(entry.clone()).is_err());
        assert!(fixture.tunnel.bind_entry_to_lease(&entry, TcpChannelId::from(2),
            TcpLeaseSeq::from(1), TcpChannelKind::Stream,
            TunnelPurpose::from_value(&81u16).unwrap(), true).is_err());
        assert!(fixture.tunnel.state.lock().unwrap().data_conns.is_empty());
        assert!(entry.stream.lock().unwrap().is_none());
    }

    #[tokio::test]
    async fn unit_completed_old_lease_cannot_retire_reused_connection() {
        let fixture = Fixture::new().await;
        let (entry, old, _peer) = fixture.lease(TcpChannelKind::Stream).await;
        fully_drained(&old);
        TcpTunnel::maybe_finalize_lease(&old);
        assert!(matches!(entry.inner.lock().unwrap().state, DataConnEntryState::Idle));
        let new = fixture.tunnel.bind_entry_to_lease(&entry, TcpChannelId::from(2),
            TcpLeaseSeq::from(2), TcpChannelKind::Stream,
            TunnelPurpose::from_value(&82u16).unwrap(), true).unwrap();
        TcpTunnel::retire_lease_arc(&old, P2pErrorCode::Interrupted);
        assert!(!new.lock().unwrap().retired);
        assert!(fixture.tunnel.get_entry(entry.conn_id).is_some());
        fixture.tunnel.close_local(LocalTunnelPhase::Error, P2pErrorCode::Timeout);
        assert!(new.lock().unwrap().retired);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dv_close_races_registration_binding_and_idle_return_without_survivors() {
        for idle in [false, true] {
            for _ in 0..4 {
                let fixture = Fixture::new().await;
                let (entry, _peer) = fixture.entry(1).await;
                let lease = if idle {
                    fixture.tunnel.register_entry(entry.clone()).unwrap();
                    let lease = fixture.tunnel.bind_entry_to_lease(&entry, TcpChannelId::from(1),
                        TcpLeaseSeq::from(1), TcpChannelKind::Stream,
                        TunnelPurpose::from_value(&81u16).unwrap(), true).unwrap();
                    fully_drained(&lease);
                    Some(lease)
                } else { None };
                let barrier = Arc::new(Barrier::new(2));
                let closing = tokio::spawn({
                    let tunnel = fixture.tunnel.clone();
                    let barrier = barrier.clone();
                    async move {
                        barrier.wait().await;
                        tunnel.close_local(LocalTunnelPhase::Error, P2pErrorCode::Timeout);
                    }
                });
                barrier.wait().await;
                if let Some(lease) = &lease {
                    TcpTunnel::maybe_finalize_lease(lease);
                } else if fixture.tunnel.register_entry(entry.clone()).is_ok() {
                    let result = fixture.tunnel.bind_entry_to_lease(&entry, TcpChannelId::from(1),
                        TcpLeaseSeq::from(1), TcpChannelKind::Stream,
                        TunnelPurpose::from_value(&81u16).unwrap(), true);
                    if let Ok(lease) = result {
                        timeout(Duration::from_secs(1), closing).await.unwrap().unwrap();
                        assert!(lease.lock().unwrap().retired);
                        assert!(fixture.tunnel.state.lock().unwrap().data_conns.is_empty());
                        continue;
                    }
                } else { entry.drop_stream(); }
                timeout(Duration::from_secs(1), closing).await.unwrap().unwrap();
                assert!(fixture.tunnel.state.lock().unwrap().data_conns.is_empty());
                assert!(entry.stream.lock().unwrap().is_none());
            }
        }
    }
}
