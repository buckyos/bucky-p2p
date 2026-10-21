use super::*;
use crate::ttp::connection::TtpDialer;

fn ep(port: u16) -> Endpoint {
    Endpoint::from((Protocol::Tcp, format!("127.0.0.1:{port}").parse().unwrap()))
}

struct DialCase {
    local: P2pIdentityRef,
    network: Arc<FakeTunnelNetwork>,
    manager: NetManagerRef,
    target: TtpTarget,
}

impl DialCase {
    fn new(candidates: Vec<Endpoint>) -> Self {
        let local = make_identity(101, "ordered-local", ep(25100));
        let network = FakeTunnelNetwork::new(Protocol::Tcp);
        let manager = make_manager(network.clone());
        Self {
            local,
            network,
            manager,
            target: TtpTarget {
                local_ep: None,
                remote_ep: candidates,
                remote_id: P2pId::from(vec![102; 32]),
                remote_name: Some("ordered-remote".to_owned()),
            },
        }
    }

    fn tunnel(&self, remote: Endpoint) -> Arc<FakeTunnel> {
        FakeTunnel::new(self.local.get_id(), self.target.remote_id.clone(), ep(25100), remote)
    }

    fn queue(&self, remote: Endpoint, result: P2pResult<TunnelRef>) {
        self.network.create_results.lock().unwrap().push_back((remote, result));
    }

    fn attempts(&self) -> Vec<Endpoint> {
        self.network.attempted.lock().unwrap().clone()
    }
}

#[tokio::test]
async fn unit_dialer_preserves_order_and_stops_at_first_attached_success() {
    let candidates = vec![ep(25101), ep(25102), ep(25103)];
    for successes_after in 0..candidates.len() {
        let case = DialCase::new(candidates.clone());
        for &remote in &candidates[..successes_after] {
            case.queue(remote, Err(p2p_err!(P2pErrorCode::ConnectFailed, "offline")));
        }
        let winner = case.tunnel(candidates[successes_after]);
        case.queue(candidates[successes_after], Ok(winner.clone()));
        let attached = AtomicUsize::new(0);
        let result = TtpDialer::new(&case.local, &case.manager)
            .connect(&case.target, |_| {
                attached.fetch_add(1, Ordering::SeqCst);
                async { Ok(()) }
            })
            .await
            .unwrap();
        assert_eq!(result.remote_ep(), Some(candidates[successes_after]));
        assert_eq!(case.attempts(), candidates[..=successes_after]);
        assert_eq!(attached.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn unit_dialer_reports_every_failure_and_rejects_empty_candidates() {
    let case = DialCase::new(vec![ep(25101), ep(25102)]);
    case.queue(ep(25101), Err(p2p_err!(P2pErrorCode::ConnectFailed, "first failure")));
    case.queue(ep(25102), Err(p2p_err!(P2pErrorCode::Timeout, "last failure")));
    let err = TtpDialer::new(&case.local, &case.manager)
        .connect(&case.target, |_| async { panic!("failed candidates cannot attach") })
        .await
        .err()
        .unwrap();
    assert_eq!(err.code(), P2pErrorCode::Timeout);
    for text in [ep(25101).to_string(), ep(25102).to_string(), "first failure".into(), "last failure".into()] {
        assert!(err.msg().contains(&text), "missing {text}: {err}");
    }
    assert_eq!(case.attempts(), case.target.remote_ep);

    let empty = DialCase::new(Vec::new());
    let err = TtpDialer::new(&empty.local, &empty.manager)
        .connect(&empty.target, |_| async { panic!("empty target cannot attach") })
        .await
        .err()
        .unwrap();
    assert_eq!(err.code(), P2pErrorCode::InvalidParam);
    assert!(empty.attempts().is_empty());
}

#[tokio::test]
async fn unit_dialer_skips_missing_network_and_incompatible_local_bindings() {
    let quic = Endpoint::from((Protocol::Quic, "127.0.0.1:25101".parse().unwrap()));
    let ipv6 = Endpoint::from((Protocol::Tcp, "[::1]:25102".parse().unwrap()));
    for local_ep in [None, Some(ep(0))] {
        let mut case = DialCase::new(vec![quic, ipv6, ep(25103)]);
        case.target.local_ep = local_ep;
        if local_ep.is_none() {
            case.queue(ipv6, Err(p2p_err!(P2pErrorCode::ConnectFailed, "IPv6 offline")));
        }
        case.queue(ep(25103), Ok(case.tunnel(ep(25103))));
        TtpDialer::new(&case.local, &case.manager)
            .connect(&case.target, |_| async { Ok(()) })
            .await
            .unwrap();
        let expected = if local_ep.is_some() { vec![ep(25103)] } else { vec![ipv6, ep(25103)] };
        assert_eq!(case.attempts(), expected);
        assert_eq!(case.network.bound_local.lock().unwrap().as_slice(), local_ep.as_slice());
    }
}

#[tokio::test]
async fn unit_dialer_attach_failure_cleans_up_then_tries_next() {
    let case = DialCase::new(vec![ep(25101), ep(25102), ep(25103)]);
    let rejected = case.tunnel(ep(25101));
    rejected.set_listen_error(FakeListenerKind::Stream, P2pErrorCode::ErrorState, "attach rejected");
    let accepted = case.tunnel(ep(25102));
    case.queue(ep(25101), Ok(rejected.clone()));
    case.queue(ep(25102), Ok(accepted.clone()));
    let dispatch = crate::ttp::runtime::TtpDispatchRuntime::new();
    let result = TtpDialer::new(&case.local, &case.manager)
        .connect(&case.target, |tunnel| dispatch.attach_tunnel(tunnel))
        .await
        .unwrap();
    assert_eq!(result.remote_ep(), Some(ep(25102)));
    assert_eq!(case.attempts(), vec![ep(25101), ep(25102)]);
    assert_eq!(rejected.close_count(), 1);
    assert_eq!(accepted.close_count(), 0);
}

#[tokio::test]
async fn unit_dialer_cancellation_does_not_start_another_candidate() {
    let case = DialCase::new(vec![ep(25101), ep(25102)]);
    case.queue(ep(25101), Ok(case.tunnel(ep(25101))));
    let dialer = TtpDialer::new(&case.local, &case.manager);
    let result = timeout(Duration::from_millis(20), dialer.connect(&case.target, |_| async {
        std::future::pending::<P2pResult<()>>().await
    })).await;
    assert!(result.is_err());
    assert_eq!(case.attempts(), vec![ep(25101)]);
}

#[test]
fn unit_cache_matching_preserves_identity_local_constraints_and_wildcards() {
    let mut case = DialCase::new(vec![ep(25101), ep(25102)]);
    let tunnel = case.tunnel(ep(25102));
    assert!(crate::ttp::match_target(tunnel.as_ref(), &case.target));
    case.target.remote_ep = vec![ep(25103)];
    assert!(!crate::ttp::match_target(tunnel.as_ref(), &case.target));
    case.target.remote_ep = vec![Endpoint::default()];
    assert!(crate::ttp::match_target(tunnel.as_ref(), &case.target));
    case.target.local_ep = Some(ep(0));
    assert!(crate::ttp::match_target(tunnel.as_ref(), &case.target));
    case.target.local_ep = Some(ep(25109));
    assert!(!crate::ttp::match_target(tunnel.as_ref(), &case.target));
    case.target.local_ep = None;
    *tunnel.reported_remote_ep.lock().unwrap() = None;
    case.target.remote_ep = vec![ep(25103)];
    assert!(crate::ttp::match_target(tunnel.as_ref(), &case.target));
    case.target.remote_ep.clear();
    assert!(!crate::ttp::match_target(tunnel.as_ref(), &case.target));
    case.target.remote_ep = vec![Endpoint::default()];
    case.target.remote_id = case.local.get_id();
    assert!(!crate::ttp::match_target(tunnel.as_ref(), &case.target));
}

#[tokio::test]
async fn dv_business_open_failure_does_not_replay_on_another_address() {
    for use_node in [false, true] {
        let case = DialCase::new(vec![ep(25101), ep(25102)]);
        let tunnel = case.tunnel(ep(25101));
        *tunnel.stream_open_error.lock().unwrap() = Some(P2pErrorCode::ConnectionAborted);
        case.queue(ep(25101), Ok(tunnel));
        let connector: Arc<dyn TtpConnector> = if use_node {
            TtpNode::new(case.local.clone(), case.manager.clone()).unwrap()
        } else {
            TtpClient::new(case.local.clone(), case.manager.clone())
        };
        let err = connector.open_stream(&case.target, purpose_of(1259)).await.err().unwrap();
        assert_eq!(err.code(), P2pErrorCode::ConnectionAborted);
        assert_eq!(case.attempts(), vec![ep(25101)]);
    }
}

#[tokio::test]
async fn dv_server_metadata_uses_the_cached_tunnel_endpoint() {
    for unknown in [false, true] {
        let case = DialCase::new(vec![ep(25101), ep(25102)]);
        let tunnel = case.tunnel(ep(25102));
        if unknown {
            *tunnel.reported_remote_ep.lock().unwrap() = None;
        }
        case.queue(ep(25101), Err(p2p_err!(P2pErrorCode::ConnectFailed, "first offline")));
        case.queue(ep(25102), Ok(tunnel));
        let node = TtpNode::new(case.local.clone(), case.manager.clone()).unwrap();
        node.open_datagram(&case.target, purpose_of(1260)).await.unwrap();
        let server = TtpServer::new_with_runtime(node.runtime());
        let (stream, _, _) = server.open_stream(&case.target, purpose_of(1261)).await.unwrap();
        let (control, _, _) = server.open_control_stream(&case.target, purpose_of(1262)).await.unwrap();
        let (datagram, _) = server.open_datagram(&case.target, purpose_of(1263)).await.unwrap();
        let expected = if unknown { None } else { Some(ep(25102)) };
        assert_eq!(stream.remote_ep, expected);
        assert_eq!(control.remote_ep, expected);
        assert_eq!(datagram.remote_ep, expected);
        assert_eq!(case.attempts(), vec![ep(25101), ep(25102)]);
    }
}

#[tokio::test]
async fn dv_client_and_node_use_fallback_metadata_and_reuse_the_winner() {
    for use_node in [false, true] {
        let case = DialCase::new(vec![ep(25101), ep(25102), ep(25103)]);
        case.queue(ep(25101), Err(p2p_err!(P2pErrorCode::ConnectFailed, "first offline")));
        case.queue(ep(25102), Ok(case.tunnel(ep(25102))));
        let connector: Arc<dyn TtpConnector> = if use_node {
            TtpNode::new(case.local.clone(), case.manager.clone()).unwrap()
        } else {
            TtpClient::new(case.local.clone(), case.manager.clone())
        };
        let (stream, _, _) = connector.open_stream(&case.target, purpose_of(1251)).await.unwrap();
        let (control, _, _) = connector.open_control_stream(&case.target, purpose_of(1252)).await.unwrap();
        let (datagram, _) = connector.open_datagram(&case.target, purpose_of(1253)).await.unwrap();
        assert_eq!(stream.remote_ep, Some(ep(25102)));
        assert_eq!(control.remote_ep, stream.remote_ep);
        assert_eq!(datagram.remote_ep, stream.remote_ep);
        assert_eq!(case.attempts(), vec![ep(25101), ep(25102)]);
    }
}

#[tokio::test]
async fn dv_client_target_identity_includes_the_full_ordered_list() {
    let case = DialCase::new(vec![ep(25101), ep(25102)]);
    case.queue(ep(25101), Ok(case.tunnel(ep(25101))));
    let client = TtpClient::new(case.local.clone(), case.manager.clone());
    client.connect_server(case.target.clone()).await.unwrap();
    client.connect_server(case.target.clone()).await.unwrap();
    let mut reversed = case.target.clone();
    reversed.remote_ep.reverse();
    client.remove_server(&reversed).unwrap();
    assert_eq!(client.configured_server_id().unwrap(), Some(case.target.remote_id.clone()));
    let mut subset = case.target.clone();
    subset.remote_ep.truncate(1);
    client.remove_server(&subset).unwrap();
    assert!(client.configured_server_id().unwrap().is_some());
    client.remove_server(&case.target).unwrap();
    client.remove_server(&case.target).unwrap();
    assert_eq!(client.configured_server_id().unwrap(), None);
    assert_eq!(case.attempts(), vec![ep(25101)]);
}

#[tokio::test]
async fn dv_maintained_target_recovers_from_the_beginning_using_backup_addresses() {
    let case = DialCase::new(vec![ep(25101), ep(25102), ep(25103)]);
    let original = case.tunnel(ep(25101));
    case.queue(ep(25101), Ok(original.clone()));
    let client = TtpClient::new(case.local.clone(), case.manager.clone());
    client.connect_server(case.target.clone()).await.unwrap();
    // Give the spawned maintenance loop its first poll before retiring the tunnel.
    tokio::task::yield_now().await;
    original.set_state(TunnelState::Closed);
    case.queue(ep(25101), Err(p2p_err!(P2pErrorCode::ConnectFailed, "original offline")));
    case.queue(ep(25102), Ok(case.tunnel(ep(25102))));
    timeout(Duration::from_secs(65), async {
        loop {
            if case.network.create_count() >= 3 {
                break;
            }
            sleep(Duration::from_millis(20)).await;
        }
    }).await.unwrap();
    let (meta, _) = client.open_datagram(&case.target, purpose_of(1254)).await.unwrap();
    assert_eq!(meta.remote_ep, Some(ep(25102)));
    assert_eq!(case.attempts(), vec![ep(25101), ep(25101), ep(25102)]);
    client.remove_server(&case.target).unwrap();
}

#[tokio::test]
async fn dv_failure_paths_do_not_publish_tunnels_or_make_server_dial() {
    let case = DialCase::new(vec![ep(25101), ep(25102)]);
    let server = TtpServer::new(case.local.clone(), case.manager.clone()).unwrap();
    let err = server.open_datagram(&case.target, purpose_of(1255)).await.err().unwrap();
    assert_eq!(err.code(), P2pErrorCode::NotFound);
    assert!(case.attempts().is_empty());
    drop(server);
    let node = TtpNode::new(case.local.clone(), case.manager.clone()).unwrap();
    let rejected = case.tunnel(ep(25101));
    rejected.set_listen_error(FakeListenerKind::Datagram, P2pErrorCode::ErrorState, "attach failed");
    case.queue(ep(25101), Ok(rejected.clone()));
    case.queue(ep(25102), Err(p2p_err!(P2pErrorCode::ConnectFailed, "offline")));
    assert!(node.open_datagram(&case.target, purpose_of(1255)).await.is_err());
    let view = TtpServer::new_with_runtime(node.runtime());
    assert!(!view.has_cached_tunnel_for_test(&case.target));
    assert_eq!(rejected.close_count(), 1);
}

#[tokio::test]
async fn dv_unknown_endpoint_metadata_stays_unknown_after_cache_reuse() {
    for use_node in [false, true] {
        let case = DialCase::new(vec![ep(25101), ep(25102)]);
        let tunnel = case.tunnel(ep(25102));
        *tunnel.reported_remote_ep.lock().unwrap() = None;
        case.network.set_created_tunnel(tunnel);
        let connector: Arc<dyn TtpConnector> = if use_node {
            TtpNode::new(case.local.clone(), case.manager.clone()).unwrap()
        } else {
            TtpClient::new(case.local.clone(), case.manager.clone())
        };
        let (stream, _, _) = connector.open_stream(&case.target, purpose_of(1256)).await.unwrap();
        let (control, _, _) = connector.open_control_stream(&case.target, purpose_of(1257)).await.unwrap();
        let (datagram, _) = connector.open_datagram(&case.target, purpose_of(1258)).await.unwrap();
        assert_eq!(stream.remote_ep, None);
        assert_eq!(control.remote_ep, None);
        assert_eq!(datagram.remote_ep, None);
        assert_eq!(case.network.create_count(), 1);
    }
}
