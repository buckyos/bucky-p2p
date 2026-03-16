use super::{X509Identity, generate_x509_identity_with_key_pair};
use crate::error::{P2pErrorCode, P2pResult, into_p2p_err, p2p_err};
use rcgen::{KeyPair, PKCS_ED25519};
use ring::rand::SystemRandom;
use ring::signature::Ed25519KeyPair;
use rustls::pki_types::PrivatePkcs8KeyDer;

pub fn generate_ed25519_x509_identity(name: Option<String>) -> P2pResult<X509Identity> {
    let rng = SystemRandom::new();
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng)
        .map_err(|_| p2p_err!(P2pErrorCode::CertError, "generate ed25519 key failed"))?;
    let pkcs8 = PrivatePkcs8KeyDer::from(pkcs8.as_ref().to_vec());
    let key_pair = KeyPair::from_pkcs8_der_and_sign_algo(&pkcs8, &PKCS_ED25519)
        .map_err(into_p2p_err!(P2pErrorCode::CertError))?;
    generate_x509_identity_with_key_pair(name, key_pair)
}
