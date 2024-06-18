use bucky_crypto::PrivateKey;
use bucky_error::BuckyError;
use bucky_raw_codec::{RawConvertTo, RawFrom};

use rustls::pki_types::PrivateKeyDer;
use rustls::sign::{Signer, SigningKey};
use rustls::{SignatureAlgorithm, SignatureScheme};

#[derive(Clone, Debug)]
pub struct EcdsaSigningKeyP256 {
    key: PrivateKey,
    scheme: SignatureScheme,
}

impl TryFrom<PrivateKeyDer<'_>> for EcdsaSigningKeyP256 {
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

impl SigningKey for EcdsaSigningKeyP256 {
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

impl Signer for EcdsaSigningKeyP256 {
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
