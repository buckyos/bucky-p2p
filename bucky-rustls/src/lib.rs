mod hmac;
mod hash;
mod aead;
mod sign;
mod kx;
mod server_cert_verifier;
mod client_cert_verifier;
mod server_cert_resolver;

use std::sync::Arc;
use rustls::crypto::{CryptoProvider, GetRandomFailed, WebPkiSupportedAlgorithms};
use rustls::crypto::ring::cipher_suite::TLS13_AES_128_GCM_SHA256;
use rustls::Error;
use rustls::pki_types::PrivateKeyDer;
use rustls::sign::SigningKey;
use crate::sign::BuckyKey;
pub use server_cert_verifier::*;
pub use client_cert_verifier::*;
pub use server_cert_resolver::*;

pub fn provider() -> CryptoProvider {
    CryptoProvider {
        cipher_suites: ALL_CIPHER_SUITES.to_vec(),
        kx_groups: kx::ALL_KX_GROUPS.to_vec(),
        signature_verification_algorithms: WebPkiSupportedAlgorithms {
            all: &[],
            mapping: &[
            ],
        },
        secure_random: &Provider,
        key_provider: &Provider,
    }
}
#[derive(Debug)]
struct Provider;

impl rustls::crypto::SecureRandom for Provider {
    fn fill(&self, buf: &mut [u8]) -> Result<(), GetRandomFailed> {
        use rand_core::RngCore;
        rand_core::OsRng
            .try_fill_bytes(buf)
            .map_err(|_| rustls::crypto::GetRandomFailed)
    }
}

impl rustls::crypto::KeyProvider for Provider {
    fn load_private_key(&self, key_der: PrivateKeyDer<'static>) -> Result<Arc<dyn SigningKey>, Error> {
        BuckyKey::try_from(key_der)
            .map(|key| Arc::new(key) as Arc<dyn SigningKey>).map_err(|e| Error::General(e.to_string()))
    }
}

static ALL_CIPHER_SUITES: &[rustls::SupportedCipherSuite] = &[
    TLS13_AES_128_GCM_SHA256,
];
