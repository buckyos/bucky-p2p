fn directive_test_endpoint(protocol: Protocol, addr: &str) -> Endpoint {
    Endpoint::from((protocol, addr.parse().unwrap()))
}

fn directive_test_value(sn: P2pId, peer: P2pId) -> NatProbeDirective {
    NatProbeDirective {
        version: crate::sn::protocol::NAT_PROBE_CONTROL_VERSION,
        sn_peer_id: sn,
        peer_id: peer,
        registration_generation: 4,
        request_id: 9,
        probe_config_generation: 2,
        expires_at: 1_000,
        endpoints: vec![
            directive_test_endpoint(Protocol::Quic, "198.51.100.20:30001"),
            directive_test_endpoint(Protocol::Quic, "198.51.100.20:30002"),
        ],
    }
}

#[test]
fn nat_probe_directive_gate_requires_quic_identity_deadline_and_new_request() {
    let sn = P2pId::from(vec![31; 32]);
    let peer = P2pId::from(vec![32; 32]);
    let directive = directive_test_value(sn.clone(), peer.clone());
    assert!(SNClientService::valid_probe_directive(
        &sn,
        &peer,
        Protocol::Quic,
        4,
        8,
        1_000,
        &directive,
    ));
    assert!(!SNClientService::valid_probe_directive(
        &sn,
        &peer,
        Protocol::Tcp,
        4,
        8,
        999,
        &directive,
    ));
    assert!(!SNClientService::valid_probe_directive(
        &sn,
        &peer,
        Protocol::Quic,
        4,
        9,
        999,
        &directive,
    ));
    assert!(!SNClientService::valid_probe_directive(
        &sn,
        &peer,
        Protocol::Quic,
        5,
        0,
        999,
        &directive,
    ));
    assert!(!SNClientService::valid_probe_directive(
        &sn,
        &peer,
        Protocol::Quic,
        4,
        8,
        1_001,
        &directive,
    ));
    assert!(!SNClientService::valid_probe_directive(
        &P2pId::from(vec![33; 32]),
        &peer,
        Protocol::Quic,
        4,
        8,
        999,
        &directive,
    ));
}

#[test]
fn nat_probe_directive_gate_rejects_invalid_or_amplifying_endpoint_sets() {
    let sn = P2pId::from(vec![34; 32]);
    let peer = P2pId::from(vec![35; 32]);
    let mut directive = directive_test_value(sn.clone(), peer.clone());

    directive.endpoints.truncate(1);
    assert!(!SNClientService::valid_probe_directive(
        &sn,
        &peer,
        Protocol::Quic,
        0,
        0,
        999,
        &directive,
    ));

    directive = directive_test_value(sn.clone(), peer.clone());
    directive.endpoints[1] = directive_test_endpoint(Protocol::Quic, "203.0.113.21:30002");
    assert!(!SNClientService::valid_probe_directive(
        &sn,
        &peer,
        Protocol::Quic,
        0,
        0,
        999,
        &directive,
    ));

    directive = directive_test_value(sn.clone(), peer.clone());
    directive.endpoints[1] = directive.endpoints[0];
    assert!(!SNClientService::valid_probe_directive(
        &sn,
        &peer,
        Protocol::Quic,
        0,
        0,
        999,
        &directive,
    ));

    directive = directive_test_value(sn.clone(), peer.clone());
    directive.endpoints[1] = directive_test_endpoint(Protocol::Tcp, "198.51.100.20:30002");
    assert!(!SNClientService::valid_probe_directive(
        &sn,
        &peer,
        Protocol::Quic,
        0,
        0,
        999,
        &directive,
    ));

    directive = directive_test_value(sn.clone(), peer.clone());
    directive.endpoints = (0..=MAX_NAT_PROBE_ENDPOINTS)
        .map(|index| {
            directive_test_endpoint(
                Protocol::Quic,
                format!("198.51.100.20:{}", 31000 + index).as_str(),
            )
        })
        .collect();
    assert!(!SNClientService::valid_probe_directive(
        &sn,
        &peer,
        Protocol::Quic,
        0,
        0,
        999,
        &directive,
    ));
}

#[test]
fn nat_probe_directive_rejection_reasons_are_specific_and_stable() {
    let sn = P2pId::from(vec![37; 32]);
    let peer = P2pId::from(vec![38; 32]);
    let directive = directive_test_value(sn.clone(), peer.clone());
    let validate = |active_sn: &P2pId,
                    local_peer: &P2pId,
                    protocol: Protocol,
                    last_generation: u64,
                    last_request: u64,
                    now: u64,
                    directive: &NatProbeDirective| {
        SNClientService::validate_probe_directive(
            active_sn,
            local_peer,
            protocol,
            last_generation,
            last_request,
            now,
            directive,
        )
        .unwrap_err()
        .as_str()
    };

    assert_eq!(
        validate(&sn, &peer, Protocol::Tcp, 4, 8, 999, &directive),
        "transport_not_quic"
    );
    let mut unsupported = directive.clone();
    unsupported.version = u8::MAX;
    assert_eq!(
        validate(&sn, &peer, Protocol::Quic, 4, 8, 999, &unsupported),
        "version_unsupported"
    );
    assert_eq!(
        validate(
            &P2pId::from(vec![39; 32]),
            &peer,
            Protocol::Quic,
            4,
            8,
            999,
            &directive,
        ),
        "sn_mismatch"
    );
    assert_eq!(
        validate(
            &sn,
            &P2pId::from(vec![40; 32]),
            Protocol::Quic,
            4,
            8,
            999,
            &directive,
        ),
        "peer_mismatch"
    );
    assert_eq!(
        validate(&sn, &peer, Protocol::Quic, 4, 8, 1_001, &directive),
        "deadline_expired"
    );
    assert_eq!(
        validate(&sn, &peer, Protocol::Quic, 4, 9, 999, &directive),
        "replay"
    );

    let mut invalid = directive.clone();
    invalid.endpoints.truncate(1);
    assert_eq!(
        validate(&sn, &peer, Protocol::Quic, 0, 0, 999, &invalid),
        "endpoint_count"
    );
    invalid = directive.clone();
    invalid.endpoints[1] = directive_test_endpoint(Protocol::Tcp, "198.51.100.20:30002");
    assert_eq!(
        validate(&sn, &peer, Protocol::Quic, 0, 0, 999, &invalid),
        "endpoint_protocol"
    );
    invalid = directive.clone();
    invalid.endpoints[1] = directive_test_endpoint(Protocol::Quic, "[2001:db8::1]:30002");
    assert_eq!(
        validate(&sn, &peer, Protocol::Quic, 0, 0, 999, &invalid),
        "endpoint_not_ipv4"
    );
    invalid = directive.clone();
    invalid.endpoints[1] = directive_test_endpoint(Protocol::Quic, "203.0.113.21:30002");
    assert_eq!(
        validate(&sn, &peer, Protocol::Quic, 0, 0, 999, &invalid),
        "endpoint_ip_mismatch"
    );
    invalid = directive.clone();
    invalid.endpoints[1] = directive_test_endpoint(Protocol::Quic, "198.51.100.20:0");
    assert_eq!(
        validate(&sn, &peer, Protocol::Quic, 0, 0, 999, &invalid),
        "endpoint_port"
    );
    invalid = directive.clone();
    invalid.endpoints[1] = invalid.endpoints[0];
    assert_eq!(
        validate(&sn, &peer, Protocol::Quic, 0, 0, 999, &invalid),
        "endpoint_duplicate"
    );
}

#[test]
fn nat_probe_directive_does_not_gate_initial_online_publication() {
    let sn = P2pId::from(vec![36; 32]);
    let active = ActiveSN {
        sn_peer_id: sn.clone(),
        latest_time: 1,
        conn_id: CmdTunnelId::from(41),
        protocol: Protocol::Quic,
        wan_ep_list: vec![],
        nat_probe_endpoints: vec![],
        nat_probe_signer: None,
        net_profile: NatProfile::unknown(),
        nat_probe_registration_generation: 0,
        last_nat_probe_request_id: 0,
    };
    let mut active_sn_list = Vec::new();

    assert!(publish_active_sn(&mut active_sn_list, active));
    assert_eq!(active_sn_list.len(), 1);
    assert_eq!(active_sn_list[0].sn_peer_id, sn);
    assert_eq!(
        active_sn_list[0].net_profile.observation,
        crate::nat_type::NatMappingObservation::Unknown
    );
}

#[cfg(feature = "x509")]
#[derive(Clone)]
struct SelfInvalidCert {
    inner: P2pIdentityCertRef,
}

#[cfg(feature = "x509")]
impl crate::p2p_identity::P2pIdentityCert for SelfInvalidCert {
    fn get_id(&self) -> P2pId {
        self.inner.get_id()
    }

    fn get_name(&self) -> String {
        self.inner.get_name()
    }

    fn sign_type(&self) -> crate::p2p_identity::P2pIdentitySignType {
        self.inner.sign_type()
    }

    fn verify(&self, message: &[u8], sign: &crate::p2p_identity::P2pSignature) -> bool {
        self.inner.verify(message, sign)
    }

    fn verify_cert(&self, _name: &str) -> bool {
        false
    }

    fn get_encoded_cert(&self) -> P2pResult<crate::p2p_identity::EncodedP2pIdentityCert> {
        self.inner.get_encoded_cert()
    }

    fn endpoints(&self) -> Vec<Endpoint> {
        self.inner.endpoints()
    }

    fn sn_list(&self) -> Vec<crate::p2p_identity::P2pSn> {
        self.inner.sn_list()
    }

    fn update_endpoints(&self, eps: Vec<Endpoint>) -> P2pIdentityCertRef {
        self.inner.update_endpoints(eps)
    }
}

#[cfg(feature = "x509")]
struct SelfInvalidCertFactory {
    cert: P2pIdentityCertRef,
}

#[cfg(feature = "x509")]
impl crate::p2p_identity::P2pIdentityCertFactory for SelfInvalidCertFactory {
    fn create(
        &self,
        _cert: &crate::p2p_identity::EncodedP2pIdentityCert,
    ) -> P2pResult<P2pIdentityCertRef> {
        Ok(Arc::new(SelfInvalidCert {
            inner: self.cert.clone(),
        }))
    }
}

#[cfg(feature = "x509")]
fn signer_validation_service(
    cert_factory: crate::p2p_identity::P2pIdentityCertFactoryRef,
) -> Arc<SNClientService> {
    use crate::networks::NetManager;
    use crate::tls::DefaultTlsServerCertResolver;
    use crate::types::{SequenceGenerator, TunnelIdGenerator};
    use crate::x509::generate_rsa_x509_identity;

    let local_identity: P2pIdentityRef =
        Arc::new(generate_rsa_x509_identity(Some("pnat-validator-client".to_owned())).unwrap());
    SNClientService::new(
        NetManager::new(Vec::new(), DefaultTlsServerCertResolver::new()).unwrap(),
        Vec::new(),
        local_identity,
        Arc::new(SequenceGenerator::new()),
        Arc::new(TunnelIdGenerator::new()),
        cert_factory,
        1,
        Duration::from_secs(1),
        Duration::from_secs(1),
        Duration::from_secs(1),
    )
}

#[cfg(feature = "x509")]
#[tokio::test]
async fn nat_probe_signer_validation_rejects_untrusted_reports_and_allows_refresh_after_clear() {
    use crate::p2p_identity::P2pIdentityCertFactory;
    use crate::x509::{X509IdentityCertFactory, generate_rsa_x509_identity};

    let expected: P2pIdentityRef =
        Arc::new(generate_rsa_x509_identity(Some("pnat-expected-sn".to_owned())).unwrap());
    let expected_id = expected.get_id();
    let encoded = expected
        .get_identity_cert()
        .unwrap()
        .get_encoded_cert()
        .unwrap();
    let wrong: P2pIdentityRef =
        Arc::new(generate_rsa_x509_identity(Some("pnat-wrong-sn".to_owned())).unwrap());
    let wrong_encoded = wrong
        .get_identity_cert()
        .unwrap()
        .get_encoded_cert()
        .unwrap();
    let service = signer_validation_service(Arc::new(X509IdentityCertFactory));

    assert!(
        service
            .validate_nat_probe_signer(&expected_id, None)
            .is_none()
    );
    assert!(
        service
            .validate_nat_probe_signer(&expected_id, Some(&vec![1, 2, 3]))
            .is_none()
    );
    assert!(
        service
            .validate_nat_probe_signer(&expected_id, Some(&wrong_encoded))
            .is_none()
    );

    let mut active_signer = service.validate_nat_probe_signer(&expected_id, Some(&encoded));
    assert_eq!(active_signer.as_ref().unwrap().get_id(), expected_id);
    active_signer = service.validate_nat_probe_signer(&expected_id, None);
    assert!(
        active_signer.is_none(),
        "missing refresh did not clear trust"
    );
    active_signer = service.validate_nat_probe_signer(&expected_id, Some(&encoded));
    assert_eq!(active_signer.as_ref().unwrap().get_id(), expected_id);

    let parsed = X509IdentityCertFactory.create(&encoded).unwrap();
    let invalid_service =
        signer_validation_service(Arc::new(SelfInvalidCertFactory { cert: parsed }));
    assert!(
        invalid_service
            .validate_nat_probe_signer(&expected_id, Some(&encoded))
            .is_none()
    );
}

#[cfg(feature = "x509")]
#[derive(Clone)]
struct EncodedCertOverride {
    inner: P2pIdentityCertRef,
    encoded: Vec<u8>,
}

#[cfg(feature = "x509")]
impl crate::p2p_identity::P2pIdentityCert for EncodedCertOverride {
    fn get_id(&self) -> P2pId {
        self.inner.get_id()
    }

    fn get_name(&self) -> String {
        self.inner.get_name()
    }

    fn sign_type(&self) -> crate::p2p_identity::P2pIdentitySignType {
        self.inner.sign_type()
    }

    fn verify(&self, message: &[u8], sign: &crate::p2p_identity::P2pSignature) -> bool {
        self.inner.verify(message, sign)
    }

    fn verify_cert(&self, name: &str) -> bool {
        self.inner.verify_cert(name)
    }

    fn get_encoded_cert(&self) -> P2pResult<crate::p2p_identity::EncodedP2pIdentityCert> {
        Ok(self.encoded.clone())
    }

    fn endpoints(&self) -> Vec<Endpoint> {
        self.inner.endpoints()
    }

    fn sn_list(&self) -> Vec<crate::p2p_identity::P2pSn> {
        self.inner.sn_list()
    }

    fn update_endpoints(&self, eps: Vec<Endpoint>) -> P2pIdentityCertRef {
        Arc::new(Self {
            inner: self.inner.update_endpoints(eps),
            encoded: self.encoded.clone(),
        })
    }
}

#[cfg(feature = "x509")]
#[derive(Clone)]
struct MutableReportIdentity {
    inner: P2pIdentityRef,
    report_mode: Arc<std::sync::atomic::AtomicUsize>,
    wrong_cert: P2pIdentityCertRef,
}

#[cfg(feature = "x509")]
impl crate::p2p_identity::P2pIdentity for MutableReportIdentity {
    fn get_identity_cert(&self) -> P2pResult<P2pIdentityCertRef> {
        let cert = self.inner.get_identity_cert()?;
        match self.report_mode.load(std::sync::atomic::Ordering::SeqCst) {
            1 => Ok(Arc::new(EncodedCertOverride {
                inner: cert,
                encoded: vec![0xff, 0x00, 0x48],
            })),
            2 => Ok(self.wrong_cert.clone()),
            3 => Ok(Arc::new(EncodedCertOverride {
                inner: cert,
                encoded: b"PNAT-SELF-INVALID".to_vec(),
            })),
            _ => Ok(cert),
        }
    }

    fn get_id(&self) -> P2pId {
        self.inner.get_id()
    }

    fn get_name(&self) -> String {
        self.inner.get_name()
    }

    fn sign_type(&self) -> crate::p2p_identity::P2pIdentitySignType {
        self.inner.sign_type()
    }

    fn sign(&self, message: &[u8]) -> P2pResult<crate::p2p_identity::P2pSignature> {
        self.inner.sign(message)
    }

    fn get_encoded_identity(&self) -> P2pResult<crate::p2p_identity::EncodedP2pIdentity> {
        self.inner.get_encoded_identity()
    }

    fn endpoints(&self) -> Vec<Endpoint> {
        self.inner.endpoints()
    }

    fn update_endpoints(&self, eps: Vec<Endpoint>) -> P2pIdentityRef {
        Arc::new(Self {
            inner: self.inner.update_endpoints(eps),
            report_mode: self.report_mode.clone(),
            wrong_cert: self.wrong_cert.clone(),
        })
    }
}

#[cfg(feature = "x509")]
struct ReportTestCertFactory {
    valid_cert: P2pIdentityCertRef,
}

#[cfg(feature = "x509")]
impl crate::p2p_identity::P2pIdentityCertFactory for ReportTestCertFactory {
    fn create(
        &self,
        cert: &crate::p2p_identity::EncodedP2pIdentityCert,
    ) -> P2pResult<P2pIdentityCertRef> {
        if cert.as_slice() == b"PNAT-SELF-INVALID" {
            return Ok(Arc::new(SelfInvalidCert {
                inner: self.valid_cert.clone(),
            }));
        }
        crate::x509::X509IdentityCertFactory.create(cert)
    }
}

#[cfg(feature = "x509")]
fn live_test_endpoint() -> Endpoint {
    let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let addr = socket.local_addr().unwrap();
    drop(socket);
    Endpoint::from((Protocol::Quic, addr))
}

#[cfg(feature = "x509")]
fn live_test_runtime() -> sfo_reuseport::ServerRuntime {
    sfo_reuseport::ServerRuntime::start(sfo_reuseport::ServerRuntimeConfig::new().with_workers(1))
        .unwrap()
}

#[cfg(feature = "x509")]
async fn wait_for_live_probe_snapshot(
    service: &Arc<SNClientService>,
    sn_id: &P2pId,
    expected_present: bool,
) {
    tokio::time::timeout(Duration::from_secs(13), async {
        loop {
            if service.get_nat_probe_snapshot_for_sn(sn_id).is_some() == expected_present {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "live ActiveSN signer did not become {}",
            if expected_present {
                "present"
            } else {
                "absent"
            }
        )
    });
}

#[cfg(feature = "x509")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn periodic_report_update_clears_and_restores_live_active_sn_signer() {
    use crate::p2p_identity::P2pIdentity;
    use crate::sn::service::{SnServiceConfig, create_sn_service};
    use crate::stack::{P2pConfig, P2pStackConfig, create_p2p_env, create_p2p_stack};
    use crate::x509::{X509IdentityCertFactory, X509IdentityFactory, generate_rsa_x509_identity};

    let identity_factory = Arc::new(X509IdentityFactory);
    let cert_factory = Arc::new(X509IdentityCertFactory);
    let sn_endpoint = live_test_endpoint();
    let stable_sn: P2pIdentityRef =
        Arc::new(generate_rsa_x509_identity(Some("pnat-live-state-sn".to_owned())).unwrap())
            .update_endpoints(vec![sn_endpoint]);
    let sn_id = stable_sn.get_id();
    let sn_entry =
        crate::p2p_identity::P2pSn::new(sn_id.clone(), stable_sn.get_name(), stable_sn.endpoints());
    let wrong_sn: P2pIdentityRef =
        Arc::new(generate_rsa_x509_identity(Some("pnat-live-state-wrong-sn".to_owned())).unwrap());
    let report_mode = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mutable_sn: P2pIdentityRef = Arc::new(MutableReportIdentity {
        inner: stable_sn.clone(),
        report_mode: report_mode.clone(),
        wrong_cert: wrong_sn.get_identity_cert().unwrap(),
    });
    let server = create_sn_service(SnServiceConfig::new(
        mutable_sn,
        identity_factory.clone(),
        cert_factory.clone(),
        live_test_runtime(),
    ))
    .await
    .unwrap();
    server.start().await.unwrap();

    let client_endpoint = live_test_endpoint();
    let client_identity: P2pIdentityRef =
        Arc::new(generate_rsa_x509_identity(Some("pnat-live-state-client".to_owned())).unwrap())
            .update_endpoints(vec![client_endpoint]);
    let client_cert_factory: crate::p2p_identity::P2pIdentityCertFactoryRef =
        Arc::new(ReportTestCertFactory {
            valid_cert: stable_sn.get_identity_cert().unwrap(),
        });
    let env = create_p2p_env(P2pConfig::new(
        identity_factory,
        client_cert_factory,
        vec![client_endpoint],
        live_test_runtime(),
    ))
    .await
    .unwrap();
    let stack = create_p2p_stack(
        P2pStackConfig::new(env, client_identity)
            .add_sn_list(vec![sn_entry])
            .set_conn_timeout(Duration::from_secs(3))
            .set_sn_ping_interval(Duration::from_millis(100))
            .set_sn_call_timeout(Duration::from_secs(3))
            .set_sn_query_interval(Duration::from_secs(1))
            .set_sn_tunnel_count(1),
    )
    .await
    .unwrap();
    stack
        .wait_online(Some(Duration::from_secs(10)))
        .await
        .unwrap();
    let service = stack.sn_client();
    wait_for_live_probe_snapshot(&service, &sn_id, true).await;

    for invalid_mode in [1, 2, 3] {
        report_mode.store(invalid_mode, std::sync::atomic::Ordering::SeqCst);
        {
            let mut state = service.state.write().unwrap();
            state
                .active_sn_list
                .iter_mut()
                .find(|active| active.sn_peer_id == sn_id)
                .unwrap()
                .latest_time = 0;
        }
        wait_for_live_probe_snapshot(&service, &sn_id, false).await;

        report_mode.store(0, std::sync::atomic::Ordering::SeqCst);
        {
            let mut state = service.state.write().unwrap();
            state
                .active_sn_list
                .iter_mut()
                .find(|active| active.sn_peer_id == sn_id)
                .unwrap()
                .latest_time = 0;
        }
        wait_for_live_probe_snapshot(&service, &sn_id, true).await;
    }

    service.stop();
    server.stop();
}
