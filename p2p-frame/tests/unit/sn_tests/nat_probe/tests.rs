use super::*;

#[cfg(feature = "x509")]
use crate::p2p_identity::{
    EncodedP2pIdentity, EncodedP2pIdentityCert, P2pIdentity, P2pIdentityCert, P2pIdentityCertRef,
    P2pIdentityRef, P2pIdentitySignType, P2pSignature, P2pSn,
};
#[cfg(feature = "x509")]
use crate::x509::{generate_ed25519_x509_identity, generate_rsa_x509_identity};
#[cfg(feature = "x509")]
use sha2::{Digest, Sha256};
#[cfg(feature = "x509")]
use std::sync::atomic::{AtomicUsize, Ordering};
#[cfg(feature = "x509")]
use std::sync::{Arc, Condvar};

#[cfg(feature = "x509")]
fn rsa_identity(name: &str) -> P2pIdentityRef {
    Arc::new(generate_rsa_x509_identity(Some(name.to_owned())).unwrap())
}

#[cfg(feature = "x509")]
fn ed25519_identity(name: &str) -> P2pIdentityRef {
    Arc::new(generate_ed25519_x509_identity(Some(name.to_owned())).unwrap())
}

#[cfg(feature = "x509")]
async fn signed_response(
    signer: P2pIdentityRef,
    token: [u8; NAT_PROBE_TOKEN_LEN],
    observed: SocketAddr,
) -> [u8; NAT_PROBE_PACKET_LEN] {
    NatProbeSigningContext::new(signer)
        .await
        .unwrap()
        .encode_response(token, observed)
        .await
        .unwrap()
        .unwrap()
}

#[cfg(feature = "x509")]
async fn assert_signed_roundtrip(signer: P2pIdentityRef, expected_signature_len: usize) {
    let token = [7u8; NAT_PROBE_TOKEN_LEN];
    let observed = "198.51.100.1:4567".parse().unwrap();
    let cert = signer.get_identity_cert().unwrap();
    let response = signed_response(signer, token, observed).await;
    assert_eq!(response.len(), NAT_PROBE_PACKET_LEN);
    assert_eq!(
        u16::from_be_bytes([
            response[NAT_PROBE_SIGNATURE_LEN_OFFSET],
            response[NAT_PROBE_SIGNATURE_LEN_OFFSET + 1],
        ]) as usize,
        expected_signature_len
    );
    let decoded = decode_response_datagram(&response).unwrap();
    assert_eq!(decoded.token, token);
    assert_eq!(decoded.observed, observed);
    assert!(decoded.verify(&cert));
}

#[cfg(feature = "x509")]
#[tokio::test]
async fn pnat_v2_signed_response_verifies_for_rsa_and_ed25519() {
    assert_signed_roundtrip(rsa_identity("pnat-rsa"), 256).await;
    assert_signed_roundtrip(ed25519_identity("pnat-ed25519"), 64).await;
}

#[cfg(feature = "x509")]
#[tokio::test]
async fn reflector_survives_one_udp_send_failure_without_retrying_it() {
    let signer = ed25519_identity("pnat-reflector-send-recovery");
    let cert = signer.get_identity_cert().unwrap();
    let reflector = Arc::new(
        NatProbeReflector::bind(
            SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)),
            signer,
        )
        .await
        .unwrap(),
    );
    let reflector_addr = reflector.local_addr().unwrap();
    let client = runtime::UdpSocket::bind(SocketAddr::V4(SocketAddrV4::new(
        Ipv4Addr::LOCALHOST,
        0,
    )))
    .await
    .unwrap();
    let client_addr = client.local_addr().unwrap();

    reflector.fail_next_send();
    let running_reflector = reflector.clone();
    let reflector_task = tokio::spawn(async move { running_reflector.run().await });

    let token_a = [0xa1; NAT_PROBE_TOKEN_LEN];
    client
        .send_to(&encode_request(token_a), reflector_addr)
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let attempts = reflector.send_attempts();
            assert!(attempts <= 1, "failed response was retried: {attempts}");
            if attempts == 1 {
                break;
            }
            assert!(
                !reflector_task.is_finished(),
                "reflector exited before the injected send failure was observed"
            );
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("reflector did not attempt the injected failing send");
    assert!(
        !reflector_task.is_finished(),
        "injected UDP send failure terminated the reflector"
    );

    let token_b = [0xb2; NAT_PROBE_TOKEN_LEN];
    client
        .send_to(&encode_request(token_b), reflector_addr)
        .await
        .unwrap();
    let mut response = [0u8; NAT_PROBE_PACKET_LEN + 1];
    let (len, source) = tokio::time::timeout(
        Duration::from_secs(2),
        client.recv_from(&mut response),
    )
    .await
    .expect("reflector did not answer the request after the send failure")
    .unwrap();
    assert_eq!(source, reflector_addr);
    assert_eq!(len, NAT_PROBE_PACKET_LEN);
    let decoded = decode_response_datagram(&response[..len]).unwrap();
    assert_eq!(decoded.token, token_b);
    assert_eq!(decoded.observed, client_addr);
    assert!(decoded.verify(&cert));
    assert_eq!(
        reflector.send_attempts(),
        2,
        "the failed token A response must not be retried"
    );
    assert!(
        !reflector_task.is_finished(),
        "reflector exited after sending the recovery response"
    );
    reflector_task.abort();
}

#[cfg(feature = "x509")]
#[test]
fn pnat_v2_request_is_fixed_size_and_v1_is_rejected() {
    let token = [11u8; NAT_PROBE_TOKEN_LEN];
    let request = encode_request(token);
    assert_eq!(request.len(), NAT_PROBE_PACKET_LEN);
    assert_eq!(decode_request(&request), Some(token));
    assert!(request[24..].iter().all(|byte| *byte == 0));

    let mut v1 = [0u8; 32];
    v1[..4].copy_from_slice(b"PNAT");
    v1[4] = 1;
    v1[5] = NAT_PROBE_REQUEST;
    v1[8..24].copy_from_slice(&token);
    assert!(decode_request(&v1).is_none());
    v1[5] = NAT_PROBE_RESPONSE;
    assert!(decode_response_datagram(&v1).is_none());
}

#[cfg(feature = "x509")]
#[tokio::test]
async fn pnat_v2_response_rejects_wrong_signer_and_tampering() {
    let signer = rsa_identity("pnat-signer");
    let wrong_signer = rsa_identity("pnat-wrong-signer");
    let cert = signer.get_identity_cert().unwrap();
    let response = signed_response(
        signer.clone(),
        [13u8; NAT_PROBE_TOKEN_LEN],
        "198.51.100.8:42001".parse().unwrap(),
    )
    .await;
    assert!(
        !decode_response_datagram(&response)
            .unwrap()
            .verify(&wrong_signer.get_identity_cert().unwrap())
    );

    for index in [
        NAT_PROBE_TOKEN_OFFSET,
        NAT_PROBE_IPV4_OFFSET,
        NAT_PROBE_PORT_OFFSET,
        NAT_PROBE_SIGNATURE_OFFSET,
    ] {
        let mut tampered = response;
        tampered[index] ^= 1;
        let decoded = decode_response_datagram(&tampered).unwrap();
        assert!(!decoded.verify(&cert), "tampered byte {index} verified");
    }

    for (index, value) in [
        (4, NAT_PROBE_PROTOCOL_VERSION.wrapping_add(1)),
        (5, NAT_PROBE_REQUEST),
        (7, 1),
    ] {
        let mut malformed = response;
        malformed[index] = value;
        assert!(decode_response_datagram(&malformed).is_none());
    }

    let signature_len = u16::from_be_bytes([
        response[NAT_PROBE_SIGNATURE_LEN_OFFSET],
        response[NAT_PROBE_SIGNATURE_LEN_OFFSET + 1],
    ]) as usize;
    let mut shortened = response;
    shortened[NAT_PROBE_SIGNATURE_LEN_OFFSET..NAT_PROBE_SIGNATURE_OFFSET]
        .copy_from_slice(&((signature_len - 1) as u16).to_be_bytes());
    assert!(decode_response_datagram(&shortened).is_none());

    let mut enlarged = response;
    enlarged[NAT_PROBE_SIGNATURE_LEN_OFFSET..NAT_PROBE_SIGNATURE_OFFSET]
        .copy_from_slice(&((signature_len + 1) as u16).to_be_bytes());
    assert!(!decode_response_datagram(&enlarged).unwrap().verify(&cert));

    let mut padded = response;
    padded[NAT_PROBE_SIGNATURE_OFFSET + signature_len] = 1;
    assert!(decode_response_datagram(&padded).is_none());
    assert!(decode_response_datagram(&response[..NAT_PROBE_PACKET_LEN - 1]).is_none());
}

#[cfg(feature = "x509")]
#[derive(Clone)]
struct SizedIdentity {
    id: P2pId,
    lengths: Arc<Vec<usize>>,
    calls: Arc<AtomicUsize>,
}

#[cfg(feature = "x509")]
impl SizedIdentity {
    fn new(lengths: Vec<usize>) -> (P2pIdentityRef, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        (
            Arc::new(Self {
                id: P2pId::from(vec![0x51; 32]),
                lengths: Arc::new(lengths),
                calls: calls.clone(),
            }),
            calls,
        )
    }
}

#[cfg(feature = "x509")]
impl P2pIdentity for SizedIdentity {
    fn get_identity_cert(&self) -> P2pResult<P2pIdentityCertRef> {
        Err(p2p_err!(
            P2pErrorCode::NotSupport,
            "test identity has no cert"
        ))
    }

    fn get_id(&self) -> P2pId {
        self.id.clone()
    }

    fn get_name(&self) -> String {
        "sized-identity".to_owned()
    }

    fn sign_type(&self) -> P2pIdentitySignType {
        P2pIdentitySignType::Ed25519
    }

    fn sign(&self, _message: &[u8]) -> P2pResult<P2pSignature> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        let len = self.lengths[call.min(self.lengths.len() - 1)];
        Ok(vec![0x5a; len])
    }

    fn get_encoded_identity(&self) -> P2pResult<EncodedP2pIdentity> {
        Ok(Vec::new())
    }

    fn endpoints(&self) -> Vec<crate::endpoint::Endpoint> {
        Vec::new()
    }

    fn update_endpoints(&self, _eps: Vec<crate::endpoint::Endpoint>) -> P2pIdentityRef {
        Arc::new(self.clone())
    }
}

#[cfg(feature = "x509")]
#[derive(Clone)]
struct HashIdentity {
    id: P2pId,
}

#[cfg(feature = "x509")]
#[derive(Clone)]
struct TrailingTolerantHashCert {
    id: P2pId,
}

#[cfg(feature = "x509")]
impl HashIdentity {
    fn new() -> P2pIdentityRef {
        Arc::new(Self {
            id: P2pId::from(vec![0x61; 32]),
        })
    }
}

#[cfg(feature = "x509")]
fn hash_signature(message: &[u8]) -> Vec<u8> {
    Sha256::digest(message).to_vec()
}

#[cfg(feature = "x509")]
impl P2pIdentity for HashIdentity {
    fn get_identity_cert(&self) -> P2pResult<P2pIdentityCertRef> {
        Ok(Arc::new(TrailingTolerantHashCert {
            id: self.id.clone(),
        }))
    }

    fn get_id(&self) -> P2pId {
        self.id.clone()
    }

    fn get_name(&self) -> String {
        "hash-identity".to_owned()
    }

    fn sign_type(&self) -> P2pIdentitySignType {
        P2pIdentitySignType::Ed25519
    }

    fn sign(&self, message: &[u8]) -> P2pResult<P2pSignature> {
        Ok(hash_signature(message))
    }

    fn get_encoded_identity(&self) -> P2pResult<EncodedP2pIdentity> {
        Ok(Vec::new())
    }

    fn endpoints(&self) -> Vec<crate::endpoint::Endpoint> {
        Vec::new()
    }

    fn update_endpoints(&self, _eps: Vec<crate::endpoint::Endpoint>) -> P2pIdentityRef {
        Arc::new(self.clone())
    }
}

#[cfg(feature = "x509")]
impl P2pIdentityCert for TrailingTolerantHashCert {
    fn get_id(&self) -> P2pId {
        self.id.clone()
    }

    fn get_name(&self) -> String {
        "hash-identity".to_owned()
    }

    fn sign_type(&self) -> P2pIdentitySignType {
        P2pIdentitySignType::Ed25519
    }

    fn verify(&self, message: &[u8], sign: &P2pSignature) -> bool {
        sign.starts_with(hash_signature(message).as_slice())
    }

    fn verify_cert(&self, _name: &str) -> bool {
        true
    }

    fn get_encoded_cert(&self) -> P2pResult<EncodedP2pIdentityCert> {
        Ok(Vec::new())
    }

    fn endpoints(&self) -> Vec<crate::endpoint::Endpoint> {
        Vec::new()
    }

    fn sn_list(&self) -> Vec<P2pSn> {
        Vec::new()
    }

    fn update_endpoints(&self, _eps: Vec<crate::endpoint::Endpoint>) -> P2pIdentityCertRef {
        Arc::new(self.clone())
    }
}

#[cfg(feature = "x509")]
#[tokio::test]
async fn pnat_v2_signature_length_boundaries_and_signer_drift_fail_closed() {
    assert!(
        encode_response_fields(
            [31u8; NAT_PROBE_TOKEN_LEN],
            "192.0.2.31:43101".parse().unwrap(),
            0,
        )
        .is_err()
    );
    assert!(
        encode_response_fields(
            [31u8; NAT_PROBE_TOKEN_LEN],
            "192.0.2.31:43101".parse().unwrap(),
            MAX_NAT_PROBE_SIGNATURE_LEN + 1,
        )
        .is_err()
    );

    for signature_len in [0, MAX_NAT_PROBE_SIGNATURE_LEN + 1] {
        let (identity, _) = SizedIdentity::new(vec![signature_len]);
        assert!(NatProbeSigningContext::new(identity).await.is_err());
    }

    let (drifting, calls) = SizedIdentity::new(vec![64, 65]);
    let context = NatProbeSigningContext::new(drifting).await.unwrap();
    let error = context
        .encode_response(
            [32u8; NAT_PROBE_TOKEN_LEN],
            "192.0.2.32:43102".parse().unwrap(),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), P2pErrorCode::InvalidSignature);
    assert_eq!(calls.load(Ordering::SeqCst), 2);

    let identity = HashIdentity::new();
    let cert = identity.get_identity_cert().unwrap();
    let packet = signed_response(
        identity,
        [33u8; NAT_PROBE_TOKEN_LEN],
        "192.0.2.33:43103".parse().unwrap(),
    )
    .await;
    let signature_len = u16::from_be_bytes([packet[30], packet[31]]) as usize;
    assert_eq!(signature_len, 32);

    let mut zero = packet;
    zero[30..32].copy_from_slice(&0u16.to_be_bytes());
    assert!(decode_response_datagram(&zero).is_none());
    let mut oversized = packet;
    oversized[30..32].copy_from_slice(&((MAX_NAT_PROBE_SIGNATURE_LEN + 1) as u16).to_be_bytes());
    assert!(decode_response_datagram(&oversized).is_none());
    let mut shortened = packet;
    shortened[30..32].copy_from_slice(&((signature_len - 1) as u16).to_be_bytes());
    assert!(decode_response_datagram(&shortened).is_none());
    let mut enlarged = packet;
    enlarged[30..32].copy_from_slice(&((signature_len + 1) as u16).to_be_bytes());
    let enlarged = decode_response_datagram(&enlarged).unwrap();
    assert!(
        !enlarged.verify(&cert),
        "signature length is part of the preimage even for a trailing-tolerant verifier"
    );
    let mut non_zero_padding = packet;
    non_zero_padding[32 + signature_len] = 1;
    assert!(decode_response_datagram(&non_zero_padding).is_none());
}

#[cfg(feature = "x509")]
#[tokio::test]
async fn shared_signing_context_enforces_rolling_rate_and_in_flight_admission_before_sign() {
    let (identity, calls) = SizedIdentity::new(vec![64]);
    let context = NatProbeSigningContext::new(identity).await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    {
        let mut budget = context.budget.lock().unwrap();
        budget.admitted =
            std::iter::repeat_n(Instant::now(), MAX_NAT_PROBE_RESPONSES_PER_SECOND).collect();
    }
    assert!(
        context
            .encode_response(
                [41u8; NAT_PROBE_TOKEN_LEN],
                "192.0.2.41:44101".parse().unwrap(),
            )
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1, "rate rejection signed");

    {
        let mut budget = context.budget.lock().unwrap();
        budget.admitted.clear();
    }
    let permits: Vec<_> = (0..MAX_NAT_PROBE_IN_FLIGHT_SIGNATURES)
        .map(|_| context.try_acquire().unwrap().unwrap())
        .collect();
    assert!(context.clone().try_acquire().unwrap().is_none());
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "in-flight rejection signed"
    );
    drop(permits);

    {
        let mut budget = context.budget.lock().unwrap();
        budget.admitted = std::iter::repeat_n(
            Instant::now() - Duration::from_secs(2),
            MAX_NAT_PROBE_RESPONSES_PER_SECOND,
        )
        .collect();
    }
    assert!(context.try_acquire().unwrap().is_some());
}

#[cfg(feature = "x509")]
#[derive(Clone)]
struct BlockingIdentity {
    id: P2pId,
    calls: Arc<AtomicUsize>,
    gate: Arc<(Mutex<bool>, Condvar)>,
}

#[cfg(feature = "x509")]
impl P2pIdentity for BlockingIdentity {
    fn get_identity_cert(&self) -> P2pResult<P2pIdentityCertRef> {
        Err(p2p_err!(
            P2pErrorCode::NotSupport,
            "test identity has no cert"
        ))
    }

    fn get_id(&self) -> P2pId {
        self.id.clone()
    }

    fn get_name(&self) -> String {
        "blocking-identity".to_owned()
    }

    fn sign_type(&self) -> P2pIdentitySignType {
        P2pIdentitySignType::Ed25519
    }

    fn sign(&self, _message: &[u8]) -> P2pResult<P2pSignature> {
        if self.calls.fetch_add(1, Ordering::SeqCst) > 0 {
            let (lock, wake) = &*self.gate;
            let mut released = lock.lock().unwrap();
            while !*released {
                released = wake.wait(released).unwrap();
            }
        }
        Ok(vec![0x6a; 64])
    }

    fn get_encoded_identity(&self) -> P2pResult<EncodedP2pIdentity> {
        Ok(Vec::new())
    }

    fn endpoints(&self) -> Vec<crate::endpoint::Endpoint> {
        Vec::new()
    }

    fn update_endpoints(&self, _eps: Vec<crate::endpoint::Endpoint>) -> P2pIdentityRef {
        Arc::new(self.clone())
    }
}

#[cfg(feature = "x509")]
#[tokio::test(flavor = "current_thread")]
async fn blocking_signer_does_not_stall_current_thread_timer() {
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let identity: P2pIdentityRef = Arc::new(BlockingIdentity {
        id: P2pId::from(vec![0x71; 32]),
        calls: Arc::new(AtomicUsize::new(0)),
        gate: gate.clone(),
    });
    let context = NatProbeSigningContext::new(identity).await.unwrap();
    let mut response = Box::pin(context.encode_response(
        [51u8; NAT_PROBE_TOKEN_LEN],
        "192.0.2.51:45101".parse().unwrap(),
    ));

    tokio::select! {
        _ = tokio::time::sleep(Duration::from_millis(25)) => {}
        result = &mut response => panic!("blocking signer completed unexpectedly: {result:?}"),
    }
    {
        let (lock, wake) = &*gate;
        *lock.lock().unwrap() = true;
        wake.notify_all();
    }
    assert!(response.await.unwrap().is_some());
}
