mod ed25519;

use crate::endpoint::Endpoint;
use crate::error::{P2pErrorCode, P2pResult, into_p2p_err, p2p_err};
use crate::p2p_identity::{
    EncodedP2pIdentity, EncodedP2pIdentityCert, P2pId, P2pIdentity, P2pIdentityCert,
    P2pIdentityCertFactory, P2pIdentityCertRef, P2pIdentityFactory, P2pIdentityRef,
    P2pIdentitySignType, P2pSignature, P2pSn,
};
use bucky_raw_codec::{RawConvertTo, RawDecode, RawEncode, RawFrom};
use rcgen::{CertificateParams, KeyPair, PKCS_RSA_SHA256, RsaKeySize};
use ring::signature::{self, Ed25519KeyPair};
use sha2::Digest;
use std::sync::Arc;
use x509_cert::Certificate;
use x509_cert::der::oid::ObjectIdentifier;
use x509_cert::der::{Decode, DecodePem, Encode};
use x509_cert::spki::AlgorithmIdentifierOwned;
use x509_verify::VerifyingKey;
use x509_verify::der::oid::db::rfc5912::RSA_ENCRYPTION;

pub use ed25519::generate_ed25519_x509_identity;

const ED25519_OID: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.101.112");

pub fn generate_x509_identity(name: Option<String>) -> P2pResult<X509Identity> {
    let key_pair = KeyPair::generate_rsa_for(&PKCS_RSA_SHA256, RsaKeySize::_2048)
        .map_err(into_p2p_err!(P2pErrorCode::CertError))?;
    generate_x509_identity_with_key_pair(name, key_pair)
}

fn generate_x509_identity_with_key_pair(
    name: Option<String>,
    key_pair: KeyPair,
) -> P2pResult<X509Identity> {
    let mut sha256 = sha2::Sha256::new();
    sha256.update(key_pair.public_key_raw());
    let p2p_id = P2pId::from(sha256.finalize().as_slice());
    let subject_alt_names = vec![name.unwrap_or_else(|| p2p_id.to_string())];
    let params = CertificateParams::new(subject_alt_names)
        .map_err(into_p2p_err!(P2pErrorCode::CertError))?;
    let cert = params
        .self_signed(&key_pair)
        .map_err(into_p2p_err!(P2pErrorCode::CertError))?;
    let id_data = X509IdentityData {
        key: key_pair.serialize_der(),
        cert: X509IdentityCertData {
            raw_cert: cert.der().to_vec(),
            sn_list: vec![],
            endpoints: vec![],
        },
    };
    X509Identity::from_data(id_data)
}

fn sign_type_from_algorithm(
    algorithm: &AlgorithmIdentifierOwned,
) -> P2pResult<P2pIdentitySignType> {
    if algorithm.oid == RSA_ENCRYPTION {
        Ok(P2pIdentitySignType::Rsa)
    } else if algorithm.oid == ED25519_OID {
        Ok(P2pIdentitySignType::Ed25519)
    } else {
        Err(p2p_err!(
            P2pErrorCode::NotSupport,
            "unsupported x509 key algorithm {}",
            algorithm.oid
        ))
    }
}

fn sign_type_from_cert(cert: &Certificate) -> P2pResult<P2pIdentitySignType> {
    sign_type_from_algorithm(&cert.tbs_certificate.subject_public_key_info.algorithm)
}

#[derive(Debug, Clone, RawEncode, RawDecode)]
struct X509IdentityCertData {
    raw_cert: Vec<u8>,
    sn_list: Vec<P2pSn>,
    endpoints: Vec<Endpoint>,
}

pub struct X509IdentityCert {
    cert: Certificate,
    data: X509IdentityCertData,
    sn_list: Vec<P2pSn>,
}

impl X509IdentityCert {
    pub fn new(cert: Certificate, raw_cert: Vec<u8>) -> Self {
        Self {
            cert,
            data: X509IdentityCertData {
                raw_cert,
                sn_list: vec![],
                endpoints: vec![],
            },
            sn_list: vec![],
        }
    }

    pub fn set_sn_list(&mut self, sn_list: Vec<P2pSn>) {
        self.sn_list = sn_list.clone();
        self.data.sn_list = sn_list;
    }

    pub fn set_endpoints(&mut self, endpoints: Vec<Endpoint>) {
        self.data.endpoints = endpoints;
    }

    pub fn from_der(der: &[u8]) -> P2pResult<Self> {
        let cert = Certificate::from_der(der).map_err(into_p2p_err!(P2pErrorCode::CertError))?;
        Ok(Self::new(cert, der.to_vec()))
    }

    pub fn from_pem(pem: &[u8]) -> P2pResult<Self> {
        let cert = Certificate::from_pem(pem).map_err(into_p2p_err!(P2pErrorCode::CertError))?;
        let der_cert = cert
            .to_der()
            .map_err(into_p2p_err!(P2pErrorCode::CertError))?;
        Ok(Self::new(cert, der_cert))
    }

    fn from_data(data: X509IdentityCertData) -> P2pResult<Self> {
        let cert = Certificate::from_der(data.raw_cert.as_slice())
            .map_err(into_p2p_err!(P2pErrorCode::CertError))?;
        let sn_list = data.sn_list.clone();
        Ok(Self {
            cert,
            data,
            sn_list,
        })
    }

    fn get_data(&self) -> &X509IdentityCertData {
        &self.data
    }

    fn x509_update_endpoints(&self, eps: Vec<Endpoint>) -> Arc<Self> {
        Arc::new(Self {
            cert: self.cert.clone(),
            data: X509IdentityCertData {
                raw_cert: self.data.raw_cert.clone(),
                endpoints: eps,
                sn_list: self.data.sn_list.clone(),
            },
            sn_list: self.sn_list.clone(),
        })
    }
}

impl P2pIdentityCert for X509IdentityCert {
    fn get_id(&self) -> P2pId {
        let mut sha256 = sha2::Sha256::new();
        sha256.update(
            self.cert
                .tbs_certificate
                .subject_public_key_info
                .subject_public_key
                .raw_bytes(),
        );
        P2pId::from(sha256.finalize().as_slice())
    }

    fn get_name(&self) -> String {
        if let Some(ref extends) = self.cert.tbs_certificate.extensions {
            for extend in extends.iter() {
                if extend.extn_id == x509_verify::der::oid::db::rfc5912::ID_CE_SUBJECT_ALT_NAME {
                    match x509_cert::ext::pkix::SubjectAltName::from_der(extend.extn_value.as_ref())
                    {
                        Ok(name) => {
                            for general_name in name.0.iter() {
                                if let x509_cert::ext::pkix::name::GeneralName::DnsName(name) =
                                    general_name
                                {
                                    return name.to_string();
                                }
                            }
                        }
                        Err(_) => continue,
                    }
                }
            }
        }
        self.get_id().to_string()
    }

    fn sign_type(&self) -> P2pIdentitySignType {
        sign_type_from_cert(&self.cert).unwrap_or(P2pIdentitySignType::Rsa)
    }

    fn verify(&self, message: &[u8], sign: &P2pSignature) -> bool {
        let public_key = self
            .cert
            .tbs_certificate
            .subject_public_key_info
            .subject_public_key
            .raw_bytes();

        match self.sign_type() {
            P2pIdentitySignType::Rsa => signature::UnparsedPublicKey::new(
                &signature::RSA_PKCS1_2048_8192_SHA256,
                public_key,
            )
            .verify(message, sign.as_slice())
            .is_ok(),
            P2pIdentitySignType::Ed25519 => {
                signature::UnparsedPublicKey::new(&signature::ED25519, public_key)
                    .verify(message, sign.as_slice())
                    .is_ok()
            }
        }
    }

    fn verify_cert(&self, name: &str) -> bool {
        if self.get_name() != name {
            return false;
        }
        let key = match VerifyingKey::try_from(&self.cert) {
            Ok(key) => key,
            Err(_) => return false,
        };
        match key.verify(&self.cert) {
            Ok(_) => true,
            Err(_) => false,
        }
    }

    fn get_encoded_cert(&self) -> P2pResult<EncodedP2pIdentityCert> {
        self.data
            .to_vec()
            .map_err(into_p2p_err!(P2pErrorCode::RawCodecError))
    }

    fn endpoints(&self) -> Vec<Endpoint> {
        self.data.endpoints.clone()
    }

    fn sn_list(&self) -> Vec<P2pSn> {
        self.sn_list.clone()
    }

    fn update_endpoints(&self, eps: Vec<Endpoint>) -> P2pIdentityCertRef {
        self.x509_update_endpoints(eps)
    }
}

#[derive(Debug, Clone, RawEncode, RawDecode)]
struct X509IdentityData {
    key: Vec<u8>,
    cert: X509IdentityCertData,
}

enum X509PrivateKey {
    Rsa(ring::rsa::KeyPair),
    Ed25519(Ed25519KeyPair),
}

pub struct X509Identity {
    cert: Arc<X509IdentityCert>,
    key: X509PrivateKey,
    raw_key: Vec<u8>,
}

impl X509Identity {
    fn from_data(data: X509IdentityData) -> P2pResult<Self> {
        let cert = X509IdentityCert::from_data(data.cert)?;
        let sign_type = cert.sign_type();
        let key = match sign_type {
            P2pIdentitySignType::Rsa => X509PrivateKey::Rsa(
                ring::rsa::KeyPair::from_pkcs8(data.key.as_slice())
                    .map_err(|e| p2p_err!(P2pErrorCode::CertError, "{:?}", e))?,
            ),
            P2pIdentitySignType::Ed25519 => X509PrivateKey::Ed25519(
                Ed25519KeyPair::from_pkcs8(data.key.as_slice())
                    .or_else(|_| Ed25519KeyPair::from_pkcs8_maybe_unchecked(data.key.as_slice()))
                    .map_err(|e| p2p_err!(P2pErrorCode::CertError, "{:?}", e))?,
            ),
        };
        Ok(Self {
            cert: Arc::new(cert),
            key,
            raw_key: data.key,
        })
    }
}

impl P2pIdentity for X509Identity {
    fn get_identity_cert(&self) -> P2pResult<P2pIdentityCertRef> {
        Ok(self.cert.clone())
    }

    fn get_id(&self) -> P2pId {
        self.cert.get_id()
    }

    fn get_name(&self) -> String {
        self.cert.get_name()
    }

    fn sign_type(&self) -> P2pIdentitySignType {
        self.cert.sign_type()
    }

    fn sign(&self, message: &[u8]) -> P2pResult<P2pSignature> {
        match &self.key {
            X509PrivateKey::Rsa(key) => {
                let rng = ring::rand::SystemRandom::new();
                let mut actual = vec![0u8; key.public().modulus_len()];
                key.sign(
                    &signature::RSA_PKCS1_SHA256,
                    &rng,
                    message,
                    actual.as_mut_slice(),
                )
                .map_err(|e| p2p_err!(P2pErrorCode::SignError, "{:?}", e))?;
                Ok(actual)
            }
            X509PrivateKey::Ed25519(key) => Ok(key.sign(message).as_ref().to_vec()),
        }
    }

    fn get_encoded_identity(&self) -> P2pResult<EncodedP2pIdentity> {
        let data = X509IdentityData {
            key: self.raw_key.clone(),
            cert: self.cert.get_data().clone(),
        };

        data.to_vec()
            .map_err(into_p2p_err!(P2pErrorCode::CertError))
    }

    fn endpoints(&self) -> Vec<Endpoint> {
        self.cert.endpoints()
    }

    fn update_endpoints(&self, eps: Vec<Endpoint>) -> P2pIdentityRef {
        let data = X509IdentityData {
            key: self.raw_key.clone(),
            cert: X509IdentityCertData {
                raw_cert: self.cert.get_data().raw_cert.clone(),
                sn_list: self.cert.get_data().sn_list.clone(),
                endpoints: eps,
            },
        };
        Arc::new(Self::from_data(data).unwrap())
    }
}

pub struct X509IdentityFactory;

impl P2pIdentityFactory for X509IdentityFactory {
    fn create(&self, id: &EncodedP2pIdentity) -> P2pResult<P2pIdentityRef> {
        let data = X509IdentityData::clone_from_slice(id.as_slice())
            .map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
        let identity = X509Identity::from_data(data)?;
        Ok(Arc::new(identity))
    }
}

pub struct X509IdentityCertFactory;

impl P2pIdentityCertFactory for X509IdentityCertFactory {
    fn create(&self, cert: &EncodedP2pIdentityCert) -> P2pResult<P2pIdentityCertRef> {
        let data = X509IdentityCertData::clone_from_slice(cert.as_slice())
            .map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
        let cert = X509IdentityCert::from_data(data)?;
        Ok(Arc::new(cert))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_identity_roundtrip(id: &dyn P2pIdentity, expected_sign_type: P2pIdentitySignType) {
        let id_ref = id.get_id();
        let id_name = id.get_name();
        assert_eq!(id_ref.to_string(), id_name);
        assert_eq!(id.sign_type(), expected_sign_type);

        let id_cert = id.get_identity_cert().unwrap();
        let id_encoded = id.get_encoded_identity().unwrap();
        let id_sign = id.sign(b"test").unwrap();

        assert_eq!(id_cert.sign_type(), expected_sign_type);
        assert_eq!(id_ref, id_cert.get_id());
        assert!(id_cert.verify(b"test", &id_sign));
        assert!(id_cert.verify_cert(id_cert.get_id().to_string().as_str()));

        let id_cert_encoded = id_cert.get_encoded_cert().unwrap();

        let factory = X509IdentityFactory;
        let id2 = factory.create(&id_encoded).unwrap();
        assert_eq!(id2.get_id(), id_ref);
        assert_eq!(id2.sign_type(), expected_sign_type);
        assert!(
            id2.get_identity_cert()
                .unwrap()
                .verify(b"test", &id2.sign(b"test").unwrap())
        );

        let factory_cert = X509IdentityCertFactory;
        let id_cert2 = factory_cert.create(&id_cert_encoded).unwrap();
        assert_eq!(id_cert2.get_id(), id_ref);
        assert_eq!(id_cert2.sign_type(), expected_sign_type);
    }

    #[test]
    fn test_x509_identity() {
        let id = generate_x509_identity(None).unwrap();
        assert_identity_roundtrip(&id, P2pIdentitySignType::Rsa);
    }

    #[test]
    fn test_ed25519_x509_identity() {
        let id = generate_ed25519_x509_identity(None).unwrap();
        assert_identity_roundtrip(&id, P2pIdentitySignType::Ed25519);
    }
}
