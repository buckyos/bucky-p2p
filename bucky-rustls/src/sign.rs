use bucky_crypto::PrivateKey;
use bucky_error::BuckyError;
use bucky_raw_codec::{RawConvertTo, RawFrom};

use rustls::pki_types::PrivateKeyDer;
use rustls::sign::{Signer, SigningKey};
use rustls::{SignatureAlgorithm, SignatureScheme};

#[derive(Clone, Debug)]
pub struct BuckyKey {
    key: PrivateKey,
    scheme: SignatureScheme,
}

impl BuckyKey {
    pub fn new(key: PrivateKey) -> Self {
        Self {
            key,
            scheme: SignatureScheme::RSA_PSS_SHA256,
        }
    }
}

impl TryFrom<PrivateKeyDer<'_>> for BuckyKey {
    type Error = BuckyError;

    fn try_from(value: PrivateKeyDer<'_>) -> Result<Self, Self::Error> {
        match value {
            PrivateKeyDer::Pkcs8(der) => {
                Ok(Self {
                    key: PrivateKey::clone_from_slice(der.secret_pkcs8_der()).unwrap(),
                    scheme: SignatureScheme::RSA_PSS_SHA256
                })

            }
            _ => panic!("unsupported private key format"),
        }
    }
}

impl SigningKey for BuckyKey {
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

impl Signer for BuckyKey {
    fn sign(&self, message: &[u8]) -> Result<Vec<u8>, rustls::Error> {
        let sign = self.key.sign(message).map_err(|e| {
            rustls::Error::General(format!("sign error: {:?}", e))
        })?;
        Ok(sign.to_vec().unwrap())
    }

    fn scheme(&self) -> SignatureScheme {
        self.scheme
    }
}
