use crate::error::{P2pErrorCode, P2pResult, into_p2p_err, p2p_err};
use crate::p2p_identity::{P2pId, P2pIdentityCertRef, P2pIdentityRef};
use crate::runtime;
use std::collections::VecDeque;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
#[cfg(test)]
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const NAT_PROBE_MAGIC: [u8; 4] = *b"PNAT";
const NAT_PROBE_REQUEST: u8 = 1;
const NAT_PROBE_RESPONSE: u8 = 2;
const NAT_PROBE_TOKEN_OFFSET: usize = 8;
pub(crate) const NAT_PROBE_TOKEN_LEN: usize = 16;
const NAT_PROBE_IPV4_OFFSET: usize = 24;
const NAT_PROBE_PORT_OFFSET: usize = 28;
const NAT_PROBE_SIGNATURE_LEN_OFFSET: usize = 30;
const NAT_PROBE_SIGNATURE_OFFSET: usize = 32;
const NAT_PROBE_SIGNED_FIELDS_LEN: usize = NAT_PROBE_SIGNATURE_OFFSET;
const NAT_PROBE_SIGNATURE_DOMAIN: &[u8] = b"CYFS-P2P/PNAT/RESPONSE/V2\0";
const NAT_PROBE_SIGNATURE_CALIBRATION_DOMAIN: &[u8] = b"CYFS-P2P/PNAT/SIGNATURE-LENGTH/V2\0";

pub const NAT_PROBE_PROTOCOL_VERSION: u8 = 2;

/// Request and response datagrams are deliberately the same fixed size so the
/// reflector cannot amplify traffic. The signature area supports current
/// Ed25519 identities and RSA keys up to 8192 bits.
pub const NAT_PROBE_PACKET_LEN: usize = 1200;
pub const MAX_NAT_PROBE_SIGNATURE_LEN: usize = NAT_PROBE_PACKET_LEN - NAT_PROBE_SIGNATURE_OFFSET;

/// A local resource ceiling. Configured endpoints above this count are invalid
/// rather than silently changing the evidence set used for classification.
pub const MAX_NAT_PROBE_ENDPOINTS: usize = 8;

/// Maximum signatures admitted by one identity-bound signing context over any
/// rolling one-second interval.
pub const MAX_NAT_PROBE_RESPONSES_PER_SECOND: usize = 128;

/// Maximum private-key operations concurrently running for one identity-bound
/// signing context.
pub const MAX_NAT_PROBE_IN_FLIGHT_SIGNATURES: usize = 4;

struct NatProbeSigningBudget {
    admitted: VecDeque<Instant>,
    in_flight: usize,
}

/// Shared signing state for every NAT probe reflector socket owned by one SN.
///
/// Construction performs exactly one blocking-pool signature to establish the
/// identity's fixed wire signature length. Each response then performs exactly
/// one additional private-key operation after aggregate rate and concurrency
/// admission.
pub(crate) struct NatProbeSigningContext {
    local_identity: P2pIdentityRef,
    signer_id: P2pId,
    signature_len: usize,
    budget: Mutex<NatProbeSigningBudget>,
}

struct NatProbeSigningPermit {
    context: Arc<NatProbeSigningContext>,
}

impl Drop for NatProbeSigningPermit {
    fn drop(&mut self) {
        let mut budget = self
            .context
            .budget
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        debug_assert!(budget.in_flight > 0);
        budget.in_flight = budget.in_flight.saturating_sub(1);
    }
}

impl NatProbeSigningContext {
    pub(crate) async fn new(local_identity: P2pIdentityRef) -> P2pResult<Arc<Self>> {
        let signer_id = local_identity.get_id();
        let calibration_preimage = signature_calibration_preimage(&signer_id)?;
        let calibration_identity = local_identity.clone();
        let signature =
            runtime::task::spawn_blocking(move || calibration_identity.sign(&calibration_preimage))
                .await
                .map_err(|error| {
                    p2p_err!(
                        P2pErrorCode::InternalError,
                        "NAT probe signature calibration task failed: {error}"
                    )
                })??;
        validate_signature_len(signature.len())?;

        Ok(Arc::new(Self {
            local_identity,
            signer_id,
            signature_len: signature.len(),
            budget: Mutex::new(NatProbeSigningBudget {
                admitted: VecDeque::with_capacity(MAX_NAT_PROBE_RESPONSES_PER_SECOND),
                in_flight: 0,
            }),
        }))
    }

    fn try_acquire(self: &Arc<Self>) -> P2pResult<Option<NatProbeSigningPermit>> {
        let now = Instant::now();
        let mut budget = self.budget.lock().map_err(|_| {
            p2p_err!(
                P2pErrorCode::InternalError,
                "NAT probe signing budget lock is poisoned"
            )
        })?;

        while budget
            .admitted
            .front()
            .is_some_and(|admitted| now.duration_since(*admitted) >= Duration::from_secs(1))
        {
            budget.admitted.pop_front();
        }
        if budget.admitted.len() >= MAX_NAT_PROBE_RESPONSES_PER_SECOND {
            return Ok(None);
        }
        if budget.in_flight >= MAX_NAT_PROBE_IN_FLIGHT_SIGNATURES {
            return Ok(None);
        }

        budget.admitted.push_back(now);
        budget.in_flight += 1;
        drop(budget);
        Ok(Some(NatProbeSigningPermit {
            context: self.clone(),
        }))
    }

    async fn encode_response(
        self: &Arc<Self>,
        token: [u8; NAT_PROBE_TOKEN_LEN],
        observed: SocketAddr,
    ) -> P2pResult<Option<[u8; NAT_PROBE_PACKET_LEN]>> {
        let mut packet = encode_response_fields(token, observed, self.signature_len)?;
        let preimage =
            response_signature_preimage(&self.signer_id, &packet[..NAT_PROBE_SIGNED_FIELDS_LEN])?;
        let Some(permit) = self.try_acquire()? else {
            return Ok(None);
        };
        let local_identity = self.local_identity.clone();
        let expected_signature_len = self.signature_len;

        let response = runtime::task::spawn_blocking(move || {
            // The permit intentionally lives inside the blocking closure. If
            // the caller drops the JoinHandle/future, admission remains held
            // until the private-key operation has actually exited.
            let _permit = permit;
            let signature = local_identity.sign(&preimage)?;
            validate_signature_len(signature.len())?;
            if signature.len() != expected_signature_len {
                return Err(p2p_err!(
                    P2pErrorCode::InvalidSignature,
                    "NAT probe signature length drifted: {} != {}",
                    signature.len(),
                    expected_signature_len
                ));
            }
            packet[NAT_PROBE_SIGNATURE_OFFSET..NAT_PROBE_SIGNATURE_OFFSET + signature.len()]
                .copy_from_slice(&signature);
            Ok(packet)
        })
        .await
        .map_err(|error| {
            p2p_err!(
                P2pErrorCode::InternalError,
                "NAT probe signing task failed: {error}"
            )
        })??;
        Ok(Some(response))
    }
}

/// An owned UDP reflector. `run` does not detach work; its caller owns the
/// returned future and may stop it by dropping/aborting that owner.
pub struct NatProbeReflector {
    socket: runtime::UdpSocket,
    signing_context: Arc<NatProbeSigningContext>,
    #[cfg(test)]
    send_test_state: NatProbeSendTestState,
}

#[cfg(test)]
#[derive(Default)]
struct NatProbeSendTestState {
    fail_next: AtomicBool,
    attempts: AtomicUsize,
}

impl NatProbeReflector {
    pub async fn bind(addr: SocketAddr, local_identity: P2pIdentityRef) -> P2pResult<Self> {
        validate_reflector_addr(addr)?;
        let signing_context = NatProbeSigningContext::new(local_identity).await?;
        Self::bind_with_context(addr, signing_context).await
    }

    pub(crate) async fn bind_with_context(
        addr: SocketAddr,
        signing_context: Arc<NatProbeSigningContext>,
    ) -> P2pResult<Self> {
        validate_reflector_addr(addr)?;
        let socket = runtime::UdpSocket::bind(addr).await.map_err(into_p2p_err!(
            P2pErrorCode::IoError,
            "bind NAT probe reflector"
        ))?;
        Ok(Self {
            socket,
            signing_context,
            #[cfg(test)]
            send_test_state: NatProbeSendTestState::default(),
        })
    }

    pub fn local_addr(&self) -> P2pResult<SocketAddr> {
        self.socket.local_addr().map_err(into_p2p_err!(
            P2pErrorCode::IoError,
            "read NAT probe reflector address"
        ))
    }

    #[cfg(test)]
    fn fail_next_send(&self) {
        self.send_test_state.fail_next.store(true, Ordering::SeqCst);
    }

    #[cfg(test)]
    fn send_attempts(&self) -> usize {
        self.send_test_state.attempts.load(Ordering::SeqCst)
    }

    async fn send_response(
        &self,
        response: &[u8; NAT_PROBE_PACKET_LEN],
        target: SocketAddr,
    ) -> std::io::Result<usize> {
        #[cfg(test)]
        {
            self.send_test_state.attempts.fetch_add(1, Ordering::SeqCst);
            if self.send_test_state.fail_next.swap(false, Ordering::SeqCst) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "injected NAT probe response send failure",
                ));
            }
        }

        self.socket.send_to(response, target).await
    }

    pub async fn run(&self) -> P2pResult<()> {
        let mut packet = [0u8; NAT_PROBE_PACKET_LEN + 1];

        loop {
            let (len, source) = self
                .socket
                .recv_from(&mut packet)
                .await
                .map_err(into_p2p_err!(
                    P2pErrorCode::IoError,
                    "receive NAT probe request"
                ))?;
            if len != NAT_PROBE_PACKET_LEN || !source.is_ipv4() {
                continue;
            }

            let Some(token) = decode_request(&packet[..len]) else {
                continue;
            };

            let response = match self.signing_context.encode_response(token, source).await {
                Ok(Some(response)) => response,
                Ok(None) => continue,
                Err(error) => {
                    log::warn!(
                        "drop NAT probe response because signing failed: source={source}, error={error}"
                    );
                    continue;
                }
            };
            if let Err(error) = self.send_response(&response, source).await {
                match self.socket.local_addr() {
                    Ok(local_addr) => log::warn!(
                        "drop NAT probe response because UDP send failed: source={local_addr}, target={source}, error={error}"
                    ),
                    Err(local_addr_error) => log::warn!(
                        "drop NAT probe response because UDP send failed: source=<unavailable: {local_addr_error}>, target={source}, error={error}"
                    ),
                }
                continue;
            }
        }
    }
}

fn validate_reflector_addr(addr: SocketAddr) -> P2pResult<()> {
    if !addr.is_ipv4() {
        return Err(p2p_err!(
            P2pErrorCode::InvalidParam,
            "NAT probe reflector requires an IPv4 UDP address"
        ));
    }
    Ok(())
}

pub(crate) fn encode_request(token: [u8; NAT_PROBE_TOKEN_LEN]) -> [u8; NAT_PROBE_PACKET_LEN] {
    let mut packet = [0u8; NAT_PROBE_PACKET_LEN];
    packet[..4].copy_from_slice(&NAT_PROBE_MAGIC);
    packet[4] = NAT_PROBE_PROTOCOL_VERSION;
    packet[5] = NAT_PROBE_REQUEST;
    packet[NAT_PROBE_TOKEN_OFFSET..NAT_PROBE_TOKEN_OFFSET + NAT_PROBE_TOKEN_LEN]
        .copy_from_slice(&token);
    packet
}

fn decode_request(packet: &[u8]) -> Option<[u8; NAT_PROBE_TOKEN_LEN]> {
    if !valid_header(packet, NAT_PROBE_REQUEST) || packet[24..].iter().any(|byte| *byte != 0) {
        return None;
    }

    packet[NAT_PROBE_TOKEN_OFFSET..NAT_PROBE_TOKEN_OFFSET + NAT_PROBE_TOKEN_LEN]
        .try_into()
        .ok()
}

fn encode_response_fields(
    token: [u8; NAT_PROBE_TOKEN_LEN],
    observed: SocketAddr,
    signature_len: usize,
) -> P2pResult<[u8; NAT_PROBE_PACKET_LEN]> {
    validate_signature_len(signature_len)?;
    let mut packet = [0u8; NAT_PROBE_PACKET_LEN];
    packet[..4].copy_from_slice(&NAT_PROBE_MAGIC);
    packet[4] = NAT_PROBE_PROTOCOL_VERSION;
    packet[5] = NAT_PROBE_RESPONSE;
    packet[NAT_PROBE_TOKEN_OFFSET..NAT_PROBE_TOKEN_OFFSET + NAT_PROBE_TOKEN_LEN]
        .copy_from_slice(&token);

    let SocketAddr::V4(observed) = observed else {
        return Err(p2p_err!(
            P2pErrorCode::InvalidParam,
            "NAT probe response requires an IPv4 observed address"
        ));
    };
    if observed.ip().is_unspecified() || observed.port() == 0 {
        return Err(p2p_err!(
            P2pErrorCode::InvalidParam,
            "NAT probe response requires a non-zero observed IPv4 endpoint"
        ));
    }
    packet[NAT_PROBE_IPV4_OFFSET..NAT_PROBE_IPV4_OFFSET + 4]
        .copy_from_slice(&observed.ip().octets());
    packet[NAT_PROBE_PORT_OFFSET..NAT_PROBE_PORT_OFFSET + 2]
        .copy_from_slice(&observed.port().to_be_bytes());
    packet[NAT_PROBE_SIGNATURE_LEN_OFFSET..NAT_PROBE_SIGNATURE_OFFSET]
        .copy_from_slice(&(signature_len as u16).to_be_bytes());
    Ok(packet)
}

fn validate_signature_len(signature_len: usize) -> P2pResult<()> {
    if signature_len == 0 {
        return Err(p2p_err!(
            P2pErrorCode::InvalidSignature,
            "NAT probe signer returned an empty signature"
        ));
    }
    if signature_len > MAX_NAT_PROBE_SIGNATURE_LEN {
        return Err(p2p_err!(
            P2pErrorCode::OutOfLimit,
            "NAT probe signature is too large: {} > {}",
            signature_len,
            MAX_NAT_PROBE_SIGNATURE_LEN
        ));
    }
    Ok(())
}

/// A structurally valid signed response. Verification is deliberately
/// separate from decoding so the listener can reject packets without a live
/// token before doing public-key work.
pub(crate) struct DecodedNatProbeResponse<'a> {
    pub(crate) token: [u8; NAT_PROBE_TOKEN_LEN],
    pub(crate) observed: SocketAddr,
    signature: &'a [u8],
    signed_fields: &'a [u8],
}

impl DecodedNatProbeResponse<'_> {
    pub(crate) fn verify(&self, expected_signer: &P2pIdentityCertRef) -> bool {
        let Ok(preimage) =
            response_signature_preimage(&expected_signer.get_id(), self.signed_fields)
        else {
            return false;
        };
        expected_signer.verify(&preimage, &self.signature.to_vec())
    }
}

/// Decode a response before its token owner is known. QUIC listener sockets
/// use this to divert PNAT replies away from Quinn and into the correlated
/// rendezvous prediction waiter.
pub(crate) fn decode_response_datagram(packet: &[u8]) -> Option<DecodedNatProbeResponse<'_>> {
    if !valid_header(packet, NAT_PROBE_RESPONSE) {
        return None;
    }
    let token = packet[NAT_PROBE_TOKEN_OFFSET..NAT_PROBE_TOKEN_OFFSET + NAT_PROBE_TOKEN_LEN]
        .try_into()
        .ok()?;
    let ip = Ipv4Addr::new(
        packet[NAT_PROBE_IPV4_OFFSET],
        packet[NAT_PROBE_IPV4_OFFSET + 1],
        packet[NAT_PROBE_IPV4_OFFSET + 2],
        packet[NAT_PROBE_IPV4_OFFSET + 3],
    );
    let port = u16::from_be_bytes([
        packet[NAT_PROBE_PORT_OFFSET],
        packet[NAT_PROBE_PORT_OFFSET + 1],
    ]);
    if ip.is_unspecified() || port == 0 {
        return None;
    }

    let signature_len = u16::from_be_bytes([
        packet[NAT_PROBE_SIGNATURE_LEN_OFFSET],
        packet[NAT_PROBE_SIGNATURE_LEN_OFFSET + 1],
    ]) as usize;
    if signature_len == 0 || signature_len > MAX_NAT_PROBE_SIGNATURE_LEN {
        return None;
    }
    let signature_end = NAT_PROBE_SIGNATURE_OFFSET + signature_len;
    if packet[signature_end..].iter().any(|byte| *byte != 0) {
        return None;
    }

    Some(DecodedNatProbeResponse {
        token,
        observed: SocketAddr::V4(SocketAddrV4::new(ip, port)),
        signature: &packet[NAT_PROBE_SIGNATURE_OFFSET..signature_end],
        signed_fields: &packet[..NAT_PROBE_SIGNED_FIELDS_LEN],
    })
}

fn response_signature_preimage(signer_id: &P2pId, signed_fields: &[u8]) -> P2pResult<Vec<u8>> {
    if signed_fields.len() != NAT_PROBE_SIGNED_FIELDS_LEN {
        return Err(p2p_err!(
            P2pErrorCode::InvalidParam,
            "invalid NAT probe signed fields length: {}",
            signed_fields.len()
        ));
    }
    let signer_id_len = u16::try_from(signer_id.as_slice().len()).map_err(|_| {
        p2p_err!(
            P2pErrorCode::OutOfLimit,
            "NAT probe signer id is too large: {}",
            signer_id.as_slice().len()
        )
    })?;
    let mut preimage = Vec::with_capacity(
        NAT_PROBE_SIGNATURE_DOMAIN.len() + 2 + signer_id.as_slice().len() + signed_fields.len(),
    );
    preimage.extend_from_slice(NAT_PROBE_SIGNATURE_DOMAIN);
    preimage.extend_from_slice(&signer_id_len.to_be_bytes());
    preimage.extend_from_slice(signer_id.as_slice());
    preimage.extend_from_slice(signed_fields);
    Ok(preimage)
}

fn signature_calibration_preimage(signer_id: &P2pId) -> P2pResult<Vec<u8>> {
    let signer_id_len = u16::try_from(signer_id.as_slice().len()).map_err(|_| {
        p2p_err!(
            P2pErrorCode::OutOfLimit,
            "NAT probe signer id is too large: {}",
            signer_id.as_slice().len()
        )
    })?;
    let mut preimage = Vec::with_capacity(
        NAT_PROBE_SIGNATURE_CALIBRATION_DOMAIN.len() + 2 + signer_id.as_slice().len(),
    );
    preimage.extend_from_slice(NAT_PROBE_SIGNATURE_CALIBRATION_DOMAIN);
    preimage.extend_from_slice(&signer_id_len.to_be_bytes());
    preimage.extend_from_slice(signer_id.as_slice());
    Ok(preimage)
}

fn valid_header(packet: &[u8], kind: u8) -> bool {
    packet.len() == NAT_PROBE_PACKET_LEN
        && packet[..4] == NAT_PROBE_MAGIC
        && packet[4] == NAT_PROBE_PROTOCOL_VERSION
        && packet[5] == kind
        && packet[6] == 0
        && packet[7] == 0
}

#[cfg(test)]
mod tests {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/unit/sn_tests/nat_probe/tests.rs"
    ));
}
