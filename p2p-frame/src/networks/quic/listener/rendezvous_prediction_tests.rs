use crate::p2p_identity::{
    EncodedP2pIdentityCert, P2pIdentityCert, P2pIdentitySignType, P2pSignature, P2pSn,
};
use crate::sn::nat_probe::NatProbeReflector;
use futures::task::{ArcWake, waker_ref};
use std::sync::Condvar;

fn probe_identity(name: &str) -> P2pIdentityRef {
    Arc::new(generate_rsa_x509_identity(Some(name.to_owned())).unwrap())
}

fn signed_probe_response(
    token: [u8; NAT_PROBE_TOKEN_LEN],
    observed: SocketAddr,
    signer: &P2pIdentityRef,
) -> [u8; NAT_PROBE_PACKET_LEN] {
    let SocketAddr::V4(observed) = observed else {
        panic!("test response must be IPv4");
    };
    let mut packet = [0u8; NAT_PROBE_PACKET_LEN];
    packet[..4].copy_from_slice(b"PNAT");
    packet[4] = crate::sn::nat_probe::NAT_PROBE_PROTOCOL_VERSION;
    packet[5] = 2;
    packet[8..24].copy_from_slice(&token);
    packet[24..28].copy_from_slice(&observed.ip().octets());
    packet[28..30].copy_from_slice(&observed.port().to_be_bytes());

    let signer_id = signer.get_id();
    let signature_len = signer.sign(b"PNAT test signature length").unwrap().len();
    packet[30..32].copy_from_slice(&(signature_len as u16).to_be_bytes());
    let mut preimage = b"CYFS-P2P/PNAT/RESPONSE/V2\0".to_vec();
    preimage.extend_from_slice(&(signer_id.as_slice().len() as u16).to_be_bytes());
    preimage.extend_from_slice(signer_id.as_slice());
    preimage.extend_from_slice(&packet[..32]);
    let signature = signer.sign(&preimage).unwrap();
    assert_eq!(signature.len(), signature_len);
    packet[32..32 + signature.len()].copy_from_slice(&signature);
    packet
}

#[derive(Clone)]
struct CountingCert {
    inner: P2pIdentityCertRef,
    verify_calls: Arc<AtomicUsize>,
}

impl P2pIdentityCert for CountingCert {
    fn get_id(&self) -> P2pId {
        self.inner.get_id()
    }

    fn get_name(&self) -> String {
        self.inner.get_name()
    }

    fn sign_type(&self) -> P2pIdentitySignType {
        self.inner.sign_type()
    }

    fn verify(&self, message: &[u8], sign: &P2pSignature) -> bool {
        self.verify_calls.fetch_add(1, Ordering::SeqCst);
        self.inner.verify(message, sign)
    }

    fn verify_cert(&self, name: &str) -> bool {
        self.inner.verify_cert(name)
    }

    fn get_encoded_cert(&self) -> P2pResult<EncodedP2pIdentityCert> {
        self.inner.get_encoded_cert()
    }

    fn endpoints(&self) -> Vec<Endpoint> {
        self.inner.endpoints()
    }

    fn sn_list(&self) -> Vec<P2pSn> {
        self.inner.sn_list()
    }

    fn update_endpoints(&self, eps: Vec<Endpoint>) -> P2pIdentityCertRef {
        self.inner.update_endpoints(eps)
    }
}

#[derive(Clone)]
struct BlockingVerifyCert {
    inner: P2pIdentityCertRef,
    gate: Arc<(Mutex<(bool, bool)>, Condvar)>,
}

impl P2pIdentityCert for BlockingVerifyCert {
    fn get_id(&self) -> P2pId {
        self.inner.get_id()
    }

    fn get_name(&self) -> String {
        self.inner.get_name()
    }

    fn sign_type(&self) -> P2pIdentitySignType {
        self.inner.sign_type()
    }

    fn verify(&self, message: &[u8], sign: &P2pSignature) -> bool {
        let (lock, wake) = &*self.gate;
        let mut state = lock.lock().unwrap();
        state.0 = true;
        wake.notify_all();
        while !state.1 {
            state = wake.wait(state).unwrap();
        }
        drop(state);
        self.inner.verify(message, sign)
    }

    fn verify_cert(&self, name: &str) -> bool {
        self.inner.verify_cert(name)
    }

    fn get_encoded_cert(&self) -> P2pResult<EncodedP2pIdentityCert> {
        self.inner.get_encoded_cert()
    }

    fn endpoints(&self) -> Vec<Endpoint> {
        self.inner.endpoints()
    }

    fn sn_list(&self) -> Vec<P2pSn> {
        self.inner.sn_list()
    }

    fn update_endpoints(&self, eps: Vec<Endpoint>) -> P2pIdentityCertRef {
        self.inner.update_endpoints(eps)
    }
}

#[tokio::test]
async fn rendezvous_probe_waiter_rejects_unowned_wrong_source_and_wrong_signer_packets() {
    let waiters = NatProbeResponseWaiters::default();
    let signer = probe_identity("pnat-waiter-signer");
    let wrong_signer = probe_identity("pnat-waiter-wrong-signer");
    let cert = signer.get_identity_cert().unwrap();
    let token = [1u8; NAT_PROBE_TOKEN_LEN];
    let source: SocketAddr = "127.0.0.1:31001".parse().unwrap();
    let observed: SocketAddr = "198.51.100.8:41001".parse().unwrap();
    let mut receiver = waiters.register(token, source, &cert).unwrap();

    assert_eq!(
        waiters.register(token, source, &cert).unwrap_err().code(),
        P2pErrorCode::AlreadyExists
    );

    let unowned_packet = signed_probe_response([2u8; NAT_PROBE_TOKEN_LEN], observed, &signer);
    waiters.dispatch(decode_response_datagram(&unowned_packet).unwrap(), source);
    assert!(
        tokio::time::timeout(Duration::from_millis(10), &mut receiver)
            .await
            .is_err()
    );

    let valid_packet = signed_probe_response(token, observed, &signer);
    waiters.dispatch(
        decode_response_datagram(&valid_packet).unwrap(),
        "127.0.0.1:31002".parse().unwrap(),
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(10), &mut receiver)
            .await
            .is_err()
    );

    let wrong_signer_packet = signed_probe_response(token, observed, &wrong_signer);
    waiters.dispatch(
        decode_response_datagram(&wrong_signer_packet).unwrap(),
        source,
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(10), &mut receiver)
            .await
            .is_err()
    );

    waiters.dispatch(decode_response_datagram(&valid_packet).unwrap(), source);
    assert_eq!(receiver.await.unwrap(), observed);

    let pending = waiters
        .register([3u8; NAT_PROBE_TOKEN_LEN], source, &cert)
        .unwrap();
    waiters.clear();
    assert!(pending.await.is_err());
}

#[test]
fn rendezvous_waiter_drop_cleanup_is_owner_bound_and_replay_skips_verification() {
    let waiters = NatProbeResponseWaiters::default();
    let signer = probe_identity("pnat-waiter-owner");
    let source: SocketAddr = "127.0.0.1:32001".parse().unwrap();
    let observed: SocketAddr = "198.51.100.18:42001".parse().unwrap();
    let token = [61u8; NAT_PROBE_TOKEN_LEN];
    let verify_calls = Arc::new(AtomicUsize::new(0));
    let cert: P2pIdentityCertRef = Arc::new(CountingCert {
        inner: signer.get_identity_cert().unwrap(),
        verify_calls: verify_calls.clone(),
    });

    let dropped = waiters.register(token, source, &cert).unwrap();
    assert_eq!(waiters.pending.lock().unwrap().len(), 1);
    drop(dropped);
    assert!(waiters.pending.lock().unwrap().is_empty());

    let stale = waiters.register(token, source, &cert).unwrap();
    waiters.pending.lock().unwrap().remove(&token);
    let mut replacement = waiters.register(token, source, &cert).unwrap();
    drop(stale);
    assert!(waiters.pending.lock().unwrap().contains_key(&token));

    let packet = signed_probe_response(token, observed, &signer);
    waiters.dispatch(decode_response_datagram(&packet).unwrap(), source);
    assert_eq!(replacement.receiver.try_recv().unwrap(), observed);
    assert_eq!(verify_calls.load(Ordering::SeqCst), 1);

    waiters.dispatch(decode_response_datagram(&packet).unwrap(), source);
    assert_eq!(
        verify_calls.load(Ordering::SeqCst),
        1,
        "completed-token replay performed public-key verification"
    );
}

#[test]
fn rendezvous_waiter_rechecks_owner_after_verification_outside_the_lock() {
    let waiters = Arc::new(NatProbeResponseWaiters::default());
    let signer = probe_identity("pnat-waiter-race");
    let normal_cert = signer.get_identity_cert().unwrap();
    let source: SocketAddr = "127.0.0.1:32002".parse().unwrap();
    let observed: SocketAddr = "198.51.100.19:42002".parse().unwrap();
    let token = [62u8; NAT_PROBE_TOKEN_LEN];
    let gate = Arc::new((Mutex::new((false, false)), Condvar::new()));
    let blocking_cert: P2pIdentityCertRef = Arc::new(BlockingVerifyCert {
        inner: normal_cert.clone(),
        gate: gate.clone(),
    });
    let stale = waiters.register(token, source, &blocking_cert).unwrap();
    let packet = signed_probe_response(token, observed, &signer);

    let dispatch = {
        let waiters = waiters.clone();
        let packet = packet;
        std::thread::spawn(move || {
            waiters.dispatch(decode_response_datagram(&packet).unwrap(), source);
        })
    };
    {
        let (lock, wake) = &*gate;
        let mut state = lock.lock().unwrap();
        while !state.0 {
            state = wake.wait(state).unwrap();
        }
    }

    waiters.pending.lock().unwrap().remove(&token);
    let mut replacement = waiters.register(token, source, &normal_cert).unwrap();
    {
        let (lock, wake) = &*gate;
        lock.lock().unwrap().1 = true;
        wake.notify_all();
    }
    dispatch.join().unwrap();
    assert!(waiters.pending.lock().unwrap().contains_key(&token));
    assert!(matches!(
        replacement.receiver.try_recv(),
        Err(tokio::sync::oneshot::error::TryRecvError::Empty)
    ));

    waiters.dispatch(decode_response_datagram(&packet).unwrap(), source);
    assert_eq!(replacement.receiver.try_recv().unwrap(), observed);
    drop(stale);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rendezvous_prediction_future_drop_removes_pending_waiter() {
    let signer = probe_identity("pnat-prediction-cancel");
    let cert = signer.get_identity_cert().unwrap();
    let target_a = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let target_b = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let targets = [
        Endpoint::from((Protocol::Quic, target_a.local_addr().unwrap())),
        Endpoint::from((Protocol::Quic, target_b.local_addr().unwrap())),
    ];
    let listener = new_listener();
    listener
        .bind(
            Endpoint::from((Protocol::Quic, "127.0.0.1:0".parse().unwrap())),
            None,
            None,
            false,
        )
        .await
        .unwrap();
    listener.start().await.unwrap();

    let mut prediction = Box::pin(listener.predict_traversal_endpoints(
        &targets,
        &cert,
        Duration::from_secs(5),
        Duration::from_secs(10),
    ));
    assert!(
        tokio::time::timeout(Duration::from_millis(30), prediction.as_mut())
            .await
            .is_err()
    );
    assert_eq!(listener.nat_probe_waiters.pending.lock().unwrap().len(), 1);
    drop(prediction);
    assert!(
        listener
            .nat_probe_waiters
            .pending
            .lock()
            .unwrap()
            .is_empty()
    );
    listener.close();
}

struct PollWakeCounter(AtomicUsize);

impl ArcWake for PollWakeCounter {
    fn wake_by_ref(arc_self: &Arc<Self>) {
        arc_self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn auxiliary_datagram_poll_is_bounded_and_self_wakes_for_fairness() {
    let (_runtime, _server, socket) = sfo_udp_socket().await;
    let target = socket.local_addr().unwrap();
    let sender = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    for _ in 0..MAX_AUXILIARY_DATAGRAMS_PER_POLL + 2 {
        sender.send_to(UDP_PUNCH_MAGIC, target).unwrap();
    }
    tokio::time::sleep(Duration::from_millis(20)).await;

    let wrapped = SfoQuicUdpSocket::new(
        socket,
        0,
        Arc::new(AtomicUsize::new(1)),
        Arc::new(NatProbeResponseWaiters::default()),
    );
    let wake_count = Arc::new(PollWakeCounter(AtomicUsize::new(0)));
    let waker = waker_ref(&wake_count);
    let mut context = Context::from_waker(&waker);
    let mut storage = [0u8; NAT_PROBE_PACKET_LEN + 1];
    let mut bufs = [IoSliceMut::new(&mut storage)];
    let mut meta = [quinn::udp::RecvMeta {
        addr: "0.0.0.0:0".parse().unwrap(),
        len: 0,
        stride: 0,
        ecn: None,
        dst_ip: None,
    }];

    assert!(matches!(
        quinn::AsyncUdpSocket::poll_recv(&wrapped, &mut context, &mut bufs, &mut meta),
        Poll::Pending
    ));
    assert_eq!(wake_count.0.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rendezvous_prediction_uses_the_bound_quic_listener_socket_and_generation() {
    let signer = probe_identity("pnat-real-reflector");
    let cert = signer.get_identity_cert().unwrap();
    let reflector_a = Arc::new(
        NatProbeReflector::bind("127.0.0.1:0".parse().unwrap(), signer.clone())
            .await
            .unwrap(),
    );
    let reflector_b = Arc::new(
        NatProbeReflector::bind("127.0.0.1:0".parse().unwrap(), signer)
            .await
            .unwrap(),
    );
    let target_a = Endpoint::from((Protocol::Quic, reflector_a.local_addr().unwrap()));
    let target_b = Endpoint::from((Protocol::Quic, reflector_b.local_addr().unwrap()));
    let task_a = {
        let reflector = reflector_a.clone();
        tokio::spawn(async move { reflector.run().await })
    };
    let task_b = {
        let reflector = reflector_b.clone();
        tokio::spawn(async move { reflector.run().await })
    };

    let listener = new_listener();
    listener
        .bind(
            Endpoint::from((Protocol::Quic, "127.0.0.1:0".parse().unwrap())),
            None,
            None,
            false,
        )
        .await
        .unwrap();
    listener.start().await.unwrap();
    let bound = listener.bound_local().unwrap();
    let generation = listener.socket_binding_generation();

    let prediction = listener
        .predict_traversal_endpoints(
            &[target_a, target_b],
            &cert,
            Duration::from_secs(1),
            Duration::from_secs(10),
        )
        .await
        .unwrap();

    assert_eq!(
        prediction.profile.observation,
        NatMappingObservation::NonSymmetricLike
    );
    assert_eq!(prediction.socket_binding_generation, generation);
    assert_ne!(generation, 0);
    assert_eq!(prediction.endpoints.len(), 1);
    assert_eq!(prediction.endpoints[0].addr(), bound.addr());

    let wrong_cert = probe_identity("pnat-real-wrong-signer")
        .get_identity_cert()
        .unwrap();
    let wrong_signer = listener
        .predict_traversal_endpoints(
            &[target_a, target_b],
            &wrong_cert,
            Duration::from_millis(100),
            Duration::from_secs(1),
        )
        .await
        .unwrap_err();
    assert_eq!(wrong_signer.code(), P2pErrorCode::Timeout);

    listener.close();
    let closed = listener
        .predict_traversal_endpoints(
            &[target_a, target_b],
            &cert,
            Duration::from_millis(50),
            Duration::from_secs(1),
        )
        .await
        .unwrap_err();
    assert_eq!(closed.code(), P2pErrorCode::Interrupted);

    task_a.abort();
    task_b.abort();
}

async fn respond_with_observed_ports(
    listener: &QuicTunnelListener,
    observed_ports: &HashMap<SocketAddr, u16>,
    signer: &P2pIdentityRef,
    expected_registrations: usize,
) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut answered: Vec<[u8; NAT_PROBE_TOKEN_LEN]> = Vec::new();
    while answered.len() < expected_registrations {
        assert!(
            std::time::Instant::now() < deadline,
            "NAT probe registration never appeared"
        );
        let snapshot: Vec<([u8; NAT_PROBE_TOKEN_LEN], SocketAddr)> = {
            let pending = listener.nat_probe_waiters.pending.lock().unwrap();
            pending
                .iter()
                .map(|(token, waiter)| (*token, waiter.expected_source))
                .collect()
        };
        let mut dispatched = false;
        for (token, source) in snapshot {
            if answered.contains(&token) {
                continue;
            }
            let Some(&port) = observed_ports.get(&source) else {
                continue;
            };
            let observed: SocketAddr = format!("198.51.100.7:{port}").parse().unwrap();
            let packet = signed_probe_response(token, observed, signer);
            listener
                .nat_probe_waiters
                .dispatch(decode_response_datagram(&packet).unwrap(), source);
            answered.push(token);
            dispatched = true;
            break;
        }
        if !dispatched {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    }
}

fn fake_reflector_targets() -> (Vec<std::net::UdpSocket>, Vec<Endpoint>) {
    let mut sockets = Vec::new();
    let mut targets = Vec::new();
    for _ in 0..3 {
        let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        targets.push(Endpoint::from((
            Protocol::Quic,
            socket.local_addr().unwrap(),
        )));
        sockets.push(socket);
    }
    (sockets, targets)
}

async fn symmetric_probe_listener() -> Arc<QuicTunnelListener> {
    let listener = new_listener();
    listener
        .bind(
            Endpoint::from((Protocol::Quic, "127.0.0.1:0".parse().unwrap())),
            None,
            None,
            false,
        )
        .await
        .unwrap();
    listener.start().await.unwrap();
    listener
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn nat_profile_probe_keeps_symmetric_classification_without_prediction_hint() {
    let signer = probe_identity("pnat-unpredicted-symmetric");
    let cert = signer.get_identity_cert().unwrap();
    let (_reflectors, targets) = fake_reflector_targets();
    let observed_ports: HashMap<SocketAddr, u16> = targets
        .iter()
        .zip([40000u16, 40003, 40007])
        .map(|(target, port)| (*target.addr(), port))
        .collect();
    let listener = symmetric_probe_listener().await;

    let responder = tokio::spawn({
        let listener = listener.clone();
        let signer = signer.clone();
        let observed_ports = observed_ports.clone();
        async move {
            respond_with_observed_ports(&listener, &observed_ports, &signer, 3).await;
        }
    });
    let profile = listener
        .probe_nat_profile(
            &targets,
            &cert,
            Duration::from_secs(5),
            Duration::from_secs(10),
        )
        .await
        .unwrap();
    responder.await.unwrap();
    assert_eq!(
        profile.observation,
        NatMappingObservation::SymmetricLike,
        "unpredictable symmetric ports must keep the symmetric classification"
    );
    assert!(
        profile.prediction_hint.is_none(),
        "non-arithmetic port deltas must not produce a prediction hint"
    );
    assert_eq!(profile.observed_endpoint.unwrap().addr().port(), 40007);

    let responder = tokio::spawn({
        let listener = listener.clone();
        let signer = signer.clone();
        let observed_ports = observed_ports.clone();
        async move {
            respond_with_observed_ports(&listener, &observed_ports, &signer, 3).await;
        }
    });
    let prediction = listener
        .predict_traversal_endpoints(
            &targets,
            &cert,
            Duration::from_secs(5),
            Duration::from_secs(10),
        )
        .await
        .unwrap_err();
    responder.await.unwrap();
    assert_eq!(
        prediction.code(),
        P2pErrorCode::NotFound,
        "rendezvous prediction keeps requiring candidates for the same observation"
    );

    listener.close();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn nat_profile_probe_and_prediction_keep_arithmetic_symmetric_ports_usable() {
    let signer = probe_identity("pnat-predictable-symmetric");
    let cert = signer.get_identity_cert().unwrap();
    let (_reflectors, targets) = fake_reflector_targets();
    let observed_ports: HashMap<SocketAddr, u16> = targets
        .iter()
        .zip([40000u16, 40003, 40006])
        .map(|(target, port)| (*target.addr(), port))
        .collect();
    let listener = symmetric_probe_listener().await;

    let responder = tokio::spawn({
        let listener = listener.clone();
        let signer = signer.clone();
        let observed_ports = observed_ports.clone();
        async move {
            respond_with_observed_ports(&listener, &observed_ports, &signer, 3).await;
        }
    });
    let profile = listener
        .probe_nat_profile(
            &targets,
            &cert,
            Duration::from_secs(5),
            Duration::from_secs(10),
        )
        .await
        .unwrap();
    responder.await.unwrap();
    assert_eq!(profile.observation, NatMappingObservation::SymmetricLike);
    assert!(profile.prediction_hint.is_some());

    let responder = tokio::spawn({
        let listener = listener.clone();
        let signer = signer.clone();
        let observed_ports = observed_ports.clone();
        async move {
            respond_with_observed_ports(&listener, &observed_ports, &signer, 3).await;
        }
    });
    let prediction = listener
        .predict_traversal_endpoints(
            &targets,
            &cert,
            Duration::from_secs(5),
            Duration::from_secs(10),
        )
        .await
        .unwrap();
    responder.await.unwrap();
    assert_eq!(prediction.profile.observation, NatMappingObservation::SymmetricLike);
    assert_eq!(prediction.endpoints.len(), MAX_NAT_PREDICTION_PORTS);
    assert_eq!(prediction.endpoints[0].addr().port(), 40009);
    assert_eq!(prediction.endpoints[1].addr().port(), 40012);

    listener.close();
}
