use crate::p2p_identity::P2pIdentityRef;
use bucky_raw_codec::RawConvertTo;
use rustls::sign::{Signer, SigningKey};
use rustls::{SignatureAlgorithm, SignatureScheme};
use std::fmt::{Debug, Formatter};

#[derive(Clone)]
pub struct TlsKey {
    key: P2pIdentityRef,
    scheme: SignatureScheme,
}

impl Debug for TlsKey {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "TlsKey")
    }
}

impl TlsKey {
    pub fn new(key: P2pIdentityRef) -> Self {
        Self {
            key,
            scheme: SignatureScheme::RSA_PSS_SHA256,
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
        SignatureAlgorithm::RSA
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
