use super::super::*;

static REVERSE_TCP_TLS_INIT: std::sync::Once = std::sync::Once::new();

fn reverse_tcp_init_tls() {
    REVERSE_TCP_TLS_INIT.call_once(|| {
        crate::tls::init_tls(Arc::new(crate::x509::X509IdentityFactory));
    });
}

fn reverse_tcp_identity(name: &str, endpoints: Vec<Endpoint>) -> P2pIdentityRef {
    let identity: P2pIdentityRef = Arc::new(
        crate::x509::generate_rsa_x509_identity(Some(name.to_owned())).unwrap(),
    );
    identity.update_endpoints(endpoints)
}

fn reverse_tcp_network(
    resolver: Arc<DefaultTlsServerCertResolver>,
) -> crate::networks::TcpTunnelNetwork {
    let cert_factory: crate::p2p_identity::P2pIdentityCertFactoryRef =
        Arc::new(crate::x509::X509IdentityCertFactory);
    let network = crate::networks::TcpTunnelNetwork::new(
        resolver,
        cert_factory,
        Duration::from_secs(3),
        Duration::from_secs(5),
        Duration::from_secs(30),
        sfo_reuseport::ServerRuntime::start(sfo_reuseport::ServerRuntimeConfig::default())
            .expect("reverse tcp server runtime should start"),
    );
    network.set_reuse_address(true);
    network
}

fn reverse_tcp_ignore_incoming() -> IncomingTunnelCallback {
    Arc::new(|_| Box::pin(async {}))
}

#[tokio::test]
async fn tcp_reverse_data_first_claim_pn_proxy_stream_uses_real_reverse_tcp_target() {
    reverse_tcp_init_tls();

    let fake_ep = Endpoint::from((Protocol::Quic, "127.0.0.1:23901".parse().unwrap()));
    let pn_requested_ep = Endpoint::from((Protocol::Tcp, "127.0.0.1:0".parse().unwrap()));
    let pn_identity = reverse_tcp_identity("tcp-reverse-pn", vec![pn_requested_ep, fake_ep]);
    let b_identity = reverse_tcp_identity("tcp-reverse-b", Vec::new());

    let pn_resolver = DefaultTlsServerCertResolver::new();
    let pn_tcp_network = Arc::new(reverse_tcp_network(pn_resolver.clone()));
    let fake_network = FakeTunnelNetwork::new(Protocol::Quic);
    let net_manager = NetManager::new(
        vec![
            fake_network.clone() as TunnelNetworkRef,
            pn_tcp_network.clone() as TunnelNetworkRef,
        ],
        pn_resolver,
    )
    .unwrap();
    net_manager
        .add_listen_device(pn_identity.clone())
        .await
        .unwrap();
    net_manager
        .listen(&[pn_requested_ep, fake_ep], None)
        .await
        .unwrap();
    let pn_tcp_ep = net_manager.get_listener_info(Protocol::Tcp)[0].local;

    let ttp_server = TtpServer::new(pn_identity.clone(), net_manager).unwrap();
    let pn_server = PnServer::new_with_connection_validator(
        ttp_server,
        TestPnConnectionValidator::new(ValidateResult::Accept),
    );
    pn_server.start().await.unwrap();

    let b_resolver = DefaultTlsServerCertResolver::new();
    crate::tls::TlsServerCertResolver::add_server_identity(
        b_resolver.as_ref(),
        b_identity.clone(),
    )
    .await
    .unwrap();
    let b_network = reverse_tcp_network(b_resolver);
    b_network
        .listen(
            &Endpoint::from((Protocol::Tcp, "127.0.0.1:0".parse().unwrap())),
            None,
            None,
            reverse_tcp_ignore_incoming(),
        )
        .await
        .unwrap();
    let b_reusable_ep = b_network.listener_infos()[0].local;
    b_network.close_all_listener().await.unwrap();

    let b_tunnel = b_network
        .create_tunnel_with_local_ep(
            &b_identity,
            &b_reusable_ep,
            &pn_tcp_ep,
            &pn_identity.get_id(),
            Some(pn_identity.get_name()),
        )
        .await
        .unwrap();
    b_network
        .listen(
            &b_reusable_ep,
            None,
            None,
            reverse_tcp_ignore_incoming(),
        )
        .await
        .unwrap();

    let (target_tx, mut target_rx) = mpsc::channel(TEST_CHANNEL_CAPACITY);
    let target_callback: crate::networks::IncomingStreamCallback = Arc::new(move |accepted| {
        let target_tx = target_tx.clone();
        Box::pin(async move {
            let _ = target_tx.send(accepted).await;
        })
    });
    b_tunnel
        .listen_stream(crate::networks::allow_all_listen_vports(), target_callback)
        .await
        .unwrap();

    b_network.close_all_listener().await.unwrap();

    let source_id = P2pId::from(vec![0xA5; 32]);
    let (source_tunnel, source_stream_tx, source_attached) = FakeTunnel::new(
        pn_identity.get_id(),
        source_id.clone(),
        fake_ep,
        Endpoint::from((Protocol::Quic, "127.0.0.1:23902".parse().unwrap())),
    );
    fake_network.push_tunnel(source_tunnel).unwrap();
    timeout(Duration::from_secs(2), source_attached)
        .await
        .unwrap()
        .unwrap();

    let requested_purpose = crate::networks::TunnelPurpose::from_value(&3901u16).unwrap();
    let proxy_purpose = crate::networks::TunnelPurpose::from_value(&PROXY_SERVICE.to_string())
        .unwrap();
    let mut successful_source = None;
    for attempt in 0..8u32 {
        let ((server_read, server_write), (mut source_read, mut source_write)) = make_stream_pair();
        source_stream_tx
            .send((proxy_purpose.clone(), server_read, server_write))
            .await
            .unwrap();
        let req = ProxyOpenReq {
            tunnel_id: TunnelId::from(700 + attempt),
            from: source_id.clone(),
            to: b_identity.get_id(),
            kind: PnChannelKind::Stream,
            purpose: requested_purpose.clone(),
        };
        let source_command = TunnelCommand::new(req.clone()).unwrap();
        write_tunnel_command(&mut source_write, &source_command)
            .await
            .unwrap();

        let target = timeout(Duration::from_secs(3), target_rx.recv()).await;
        if let Ok(Some(Ok((purpose, mut target_read, mut target_write)))) = target {
            assert_eq!(purpose, proxy_purpose);
            let target_req = timeout(
                Duration::from_secs(2),
                read_proxy_command::<_, ProxyOpenReq>(&mut target_read),
            )
            .await
            .unwrap()
            .unwrap();
            assert_eq!(target_req.tunnel_id, req.tunnel_id);
            assert_eq!(target_req.from, source_id);
            assert_eq!(target_req.to, b_identity.get_id());
            write_proxy_command(
                &mut target_write,
                ProxyOpenResp {
                    tunnel_id: req.tunnel_id,
                    result: TunnelCommandResult::Success as u8,
                },
            )
            .await
            .unwrap();
            let source_resp = timeout(
                Duration::from_secs(2),
                read_proxy_command::<_, ProxyOpenResp>(&mut source_read),
            )
            .await
            .unwrap()
            .unwrap();
            assert_eq!(source_resp.result, TunnelCommandResult::Success as u8);
            successful_source = Some((source_read, source_write, target_read, target_write));
            break;
        }

        let failure = timeout(
            Duration::from_secs(2),
            read_proxy_command::<_, ProxyOpenResp>(&mut source_read),
        )
        .await
        .expect("bounded attach probe should receive an explicit response")
        .unwrap();
        assert_ne!(failure.result, TunnelCommandResult::Success as u8);
    }

    let (mut source_read, mut source_write, mut target_read, mut target_write) = successful_source
        .expect("PN should cache B control tunnel and open through reverse TCP fallback");
    source_write.write_all(b"a-through-pn").await.unwrap();
    let mut from_a = [0u8; 12];
    timeout(Duration::from_secs(2), target_read.read_exact(&mut from_a))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&from_a, b"a-through-pn");

    target_write.write_all(b"b-through-pn").await.unwrap();
    let mut from_b = [0u8; 12];
    timeout(Duration::from_secs(2), source_read.read_exact(&mut from_b))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&from_b, b"b-through-pn");

    source_write.shutdown().await.unwrap();
    target_write.shutdown().await.unwrap();
    pn_server.stop();
}
