use crate::p2p_identity::{P2pIdentityRef, P2pIdentitySignType};
use rustls::sign::{Signer, SigningKey};
use rustls::{SignatureAlgorithm, SignatureScheme};
use std::fmt::{Debug, Formatter};

pub(crate) fn signature_scheme_for_sign_type(sign_type: P2pIdentitySignType) -> SignatureScheme {
    match sign_type {
        P2pIdentitySignType::Rsa => SignatureScheme::RSA_PSS_SHA256,
        P2pIdentitySignType::Ed25519 => SignatureScheme::ED25519,
    }
}

pub(crate) fn signature_algorithm_for_sign_type(
    sign_type: P2pIdentitySignType,
) -> SignatureAlgorithm {
    match sign_type {
        P2pIdentitySignType::Rsa => SignatureAlgorithm::RSA,
        P2pIdentitySignType::Ed25519 => SignatureAlgorithm::ED25519,
    }
}

pub(crate) fn supported_verify_schemes() -> Vec<SignatureScheme> {
    vec![SignatureScheme::RSA_PSS_SHA256, SignatureScheme::ED25519]
}

#[derive(Clone)]
pub struct TlsKey {
    key: P2pIdentityRef,
    sign_type: P2pIdentitySignType,
    scheme: SignatureScheme,
}

impl Debug for TlsKey {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "TlsKey")
    }
}

impl TlsKey {
    pub fn new(key: P2pIdentityRef) -> Self {
        let sign_type = key.sign_type();
        Self {
            key,
            sign_type,
            scheme: signature_scheme_for_sign_type(sign_type),
        }
    }
}

impl SigningKey for TlsKey {
    fn choose_scheme(&self, offered: &[SignatureScheme]) -> Option<Box<dyn Signer>> {
        if offered.contains(&self.scheme) {
            Some(Box::new(self.clone()))
        } else {
            None
        }
    }

    fn algorithm(&self) -> SignatureAlgorithm {
        signature_algorithm_for_sign_type(self.sign_type)
    }
}

impl Signer for TlsKey {
    fn sign(&self, message: &[u8]) -> Result<Vec<u8>, rustls::Error> {
        let sign = self
            .key
            .sign(message)
            .map_err(|e| rustls::Error::General(format!("sign error: {:?}", e)))?;
        Ok(sign)
    }

    fn scheme(&self) -> SignatureScheme {
        self.scheme
    }
}
