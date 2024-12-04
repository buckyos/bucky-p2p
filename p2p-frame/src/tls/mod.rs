use std::fmt::{Debug, Formatter};
use std::sync::{Arc, OnceLock};
use aes::cipher::crypto_common::rand_core;
use once_cell::sync::OnceCell;
use rustls::crypto::{CryptoProvider, GetRandomFailed, WebPkiSupportedAlgorithms};
use rustls::crypto::ring::cipher_suite::TLS13_AES_128_GCM_SHA256;
use rustls::Error;
use rustls::pki_types::PrivateKeyDer;
use rustls::sign::SigningKey;

pub mod kx;
mod hmac;
mod hash;
mod aead;
pub mod sign;
mod server_cert_verifier;
mod client_cert_verifier;
mod server_cert_resolver;

pub use server_cert_resolver::*;
pub use server_cert_verifier::*;
pub use client_cert_verifier::*;
use crate::p2p_identity::P2pIdentityFactoryRef;
use crate::tls::sign::TlsKey;

static key_provider: OnceCell<KeyProvider> = OnceCell::new();
pub fn init_tls(factory: P2pIdentityFactoryRef) {
    key_provider.get_or_init(|| {
        KeyProvider {
            factory
        }
    });
}
pub fn provider() -> CryptoProvider {
    CryptoProvider {
        cipher_suites: ALL_CIPHER_SUITES.to_vec(),
        kx_groups: kx::ALL_KX_GROUPS.to_vec(),
        signature_verification_algorithms: WebPkiSupportedAlgorithms {
            all: &[],
            mapping: &[
            ],
        },
        secure_random: &SecureRandomProvider,
        key_provider: key_provider.get().unwrap(),
    }
}

struct KeyProvider {
    factory: P2pIdentityFactoryRef,
}

impl Debug for KeyProvider {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "KeyProvider")
    }
}

#[derive(Debug)]
struct SecureRandomProvider;

impl rustls::crypto::SecureRandom for SecureRandomProvider {
    fn fill(&self, buf: &mut [u8]) -> Result<(), GetRandomFailed> {
        use rand_core::RngCore;
        rand_core::OsRng
            .try_fill_bytes(buf)
            .map_err(|_| rustls::crypto::GetRandomFailed)
    }
}

impl rustls::crypto::KeyProvider for KeyProvider {
    fn load_private_key(&self, key_der: PrivateKeyDer<'static>) -> Result<Arc<dyn SigningKey>, Error> {
        match key_der {
            PrivateKeyDer::Pkcs8(der) => {
                let id = self.factory.create(&der.secret_pkcs8_der().to_vec()).map_err(|e| Error::General(e.to_string()))?;
                Ok(Arc::new(TlsKey::new(id)))
            },
            _ => panic!("unsupported private key format"),
        }
    }
}

static ALL_CIPHER_SUITES: &[rustls::SupportedCipherSuite] = &[
    TLS13_AES_128_GCM_SHA256,
];
