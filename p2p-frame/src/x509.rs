use std::sync::Arc;
use bucky_raw_codec::{FileEncoder, RawConvertTo, RawDecode, RawEncode, RawFrom};
use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair, RsaKeySize, PKCS_RSA_SHA256};
use sha2::Digest;
use x509_cert::Certificate;
use x509_cert::der::{Decode, DecodePem, Encode};
use x509_cert::der::asn1::PrintableStringRef;
use x509_cert::ext::pkix::{SubjectAltName, ID_CE_ISSUER_ALT_NAME, ID_CE_SUBJECT_ALT_NAME};
use x509_cert::ext::pkix::name::GeneralName;
use x509_cert::spki::AlgorithmIdentifierOwned;
use x509_verify::{Error, Message, VerifyInfo, VerifyingKey};
use x509_verify::der::oid::db::rfc5912::SHA_256_WITH_RSA_ENCRYPTION;
use crate::endpoint::Endpoint;
use crate::error::{into_p2p_err, p2p_err, P2pError, P2pErrorCode, P2pResult};
use crate::p2p_identity::{EncodedP2pIdentity, EncodedP2pIdentityCert, P2pId, P2pIdentity, P2pIdentityCert, P2pIdentityCertFactory, P2pIdentityCertRef, P2pIdentityFactory, P2pIdentityRef, P2pSignature};

pub fn generate_x509_identity(name: Option<String>) -> P2pResult<X509Identity> {
    let key_pair = KeyPair::generate_rsa_for(&PKCS_RSA_SHA256, RsaKeySize::_2048).map_err(into_p2p_err!(P2pErrorCode::CertError))?;
    let mut sha256 = sha2::Sha256::new();
    sha256.update(key_pair.public_key_raw());
    let p2p_id = P2pId::from(sha256.finalize().as_slice());
    let subject_alt_names = if name.is_none() {
        vec![p2p_id.to_string()]
    } else {
        vec![name.unwrap()]
    };
    let mut params = CertificateParams::new(subject_alt_names)
        .map_err(into_p2p_err!(P2pErrorCode::CertError))?;
    let cert = params.self_signed(&key_pair).map_err(into_p2p_err!(P2pErrorCode::CertError))?;
    let id_data = X509IdentityData {
        key: key_pair.serialize_der(),
        cert: X509IdentityCertData {
            raw_cert: cert.der().to_vec(),
            sn_list: vec![],
            endpoints: vec![],
        }
    };
    X509Identity::from_data(id_data)
}

#[derive(Debug, Clone, RawEncode, RawDecode)]
struct X509IdentityCertData {
    raw_cert: Vec<u8>,
    sn_list: Vec<EncodedP2pIdentityCert>,
    endpoints: Vec<Endpoint>,
}
pub struct X509IdentityCert {
    cert: Certificate,
    data: X509IdentityCertData,
    sn_list: Vec<P2pIdentityCertRef>,
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

    pub fn set_sn_list(&mut self, sn_list: Vec<P2pIdentityCertRef>) {
        self.sn_list = sn_list;
        let mut sn_list = vec![];
        for sn in self.sn_list.iter() {
            sn_list.push(sn.get_encoded_cert().unwrap());
        }
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
        let der_cert = cert.to_der().map_err(into_p2p_err!(P2pErrorCode::CertError))?;
        Ok(Self::new(cert, der_cert))
    }

    fn from_data(data: X509IdentityCertData) -> P2pResult<Self> {
        let cert = Certificate::from_der(data.raw_cert.as_slice()).map_err(into_p2p_err!(P2pErrorCode::CertError))?;
        let mut sn_list = Vec::<Arc<dyn P2pIdentityCert>>::new();
        for sn in data.sn_list.iter() {
            let data = X509IdentityCertData::clone_from_slice(sn.as_slice()).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
            sn_list.push(Arc::new(Self::from_data(data)?));
        }
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
            data: self.data.clone(),
            sn_list: self.sn_list.clone(),
        })
    }

    pub fn get_name(&self) -> String {
        if let Some(ref extends) = self.cert.tbs_certificate.extensions {
            for extend in extends.iter() {
                if extend.extn_id == ID_CE_SUBJECT_ALT_NAME {
                    match SubjectAltName::from_der(extend.extn_value.as_ref()) {
                        Ok(name) => {
                            for general_name in name.0.iter() {
                                match general_name {
                                    GeneralName::DnsName(name) => {
                                        return name.to_string();
                                    }
                                    _ => {
                                        continue;
                                    }
                                }
                            }
                        }
                        Err(_) => {
                            continue;
                        }
                    }
                }
            }
        }
        "".to_string()
    }
}

impl P2pIdentityCert for X509IdentityCert {
    fn get_id(&self) -> P2pId {
        let mut sha256 = sha2::Sha256::new();
        sha256.update(self.cert.tbs_certificate.subject_public_key_info.subject_public_key.raw_bytes());
        P2pId::from(sha256.finalize().as_slice())
    }

    fn verify(&self, message: &[u8], sign: &P2pSignature) -> bool {
        let key = match VerifyingKey::try_from(&self.cert) {
            Ok(key) => key,
            Err(_) => return false,
        };
        let verify_info = VerifyInfo::new(Message::new(message), x509_verify::Signature::new(&AlgorithmIdentifierOwned{
            oid: SHA_256_WITH_RSA_ENCRYPTION,
            parameters: None,
        }, sign.as_slice()));
        match key.verify(&verify_info) {
            Ok(_) => true,
            Err(_) => false
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
        Ok(self.data.to_vec().map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?)
    }

    fn endpoints(&self) -> Vec<Endpoint> {
        self.data.endpoints.clone()
    }

    fn sn_list(&self) -> Vec<P2pIdentityCertRef> {
        self.sn_list.clone()
    }

    fn update_endpoints(&self, eps: Vec<Endpoint>) -> P2pIdentityCertRef {
        Arc::new(Self {
            cert: self.cert.clone(),
            data: self.data.clone(),
            sn_list: self.sn_list.clone(),
        })
    }
}

#[derive(Debug, Clone, RawEncode, RawDecode)]
struct X509IdentityData {
    key: Vec<u8>,
    cert: X509IdentityCertData,
}

pub struct X509Identity {
    cert: Arc<X509IdentityCert>,
    key: ring::rsa::KeyPair,
    raw_key: Vec<u8>,
}

impl X509Identity {
    fn from_data(data: X509IdentityData) -> P2pResult<Self> {
        let cert = X509IdentityCert::from_data(data.cert)?;
        Ok(Self {
            cert: Arc::new(cert),
            key: ring::rsa::KeyPair::from_pkcs8(data.key.as_slice()).map_err(|e| p2p_err!(P2pErrorCode::CertError, "{:?}", e))?,
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

    fn sign(&self, message: &[u8]) -> P2pResult<P2pSignature> {
        let rng = ring::rand::SystemRandom::new();
        let mut actual = vec![0u8; self.key.public().modulus_len()];
        self.key.sign(&ring::signature::RSA_PKCS1_SHA256, &rng, message, actual.as_mut_slice())
            .map_err(|e| p2p_err!(P2pErrorCode::SignError, "{:?}", e))?;
        Ok(actual)
    }

    fn get_encoded_identity(&self) -> P2pResult<EncodedP2pIdentity> {
        let data = X509IdentityData {
            key: self.raw_key.clone(),
            cert: self.cert.get_data().clone(),
        };

        data.to_vec().map_err(into_p2p_err!(P2pErrorCode::CertError))
    }

    fn endpoints(&self) -> Vec<Endpoint> {
        self.cert.endpoints()
    }

    fn update_endpoints(&self, eps: Vec<Endpoint>) -> P2pIdentityRef {
        Arc::new(Self {
            cert: self.cert.x509_update_endpoints(eps),
            key: ring::rsa::KeyPair::from_pkcs8(self.raw_key.as_slice()).unwrap(),
            raw_key: self.raw_key.clone(),
        })
    }
}

pub struct X509IdentityFactory;

impl P2pIdentityFactory for X509IdentityFactory {
    fn create(&self, id: &EncodedP2pIdentity) -> P2pResult<P2pIdentityRef> {
        let data = X509IdentityData::clone_from_slice(id.as_slice()).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
        let identity = X509Identity::from_data(data)?;
        Ok(Arc::new(identity))
    }
}

pub struct X509IdentityCertFactory;

impl P2pIdentityCertFactory for X509IdentityCertFactory {
    fn create(&self, cert: &EncodedP2pIdentityCert) -> P2pResult<P2pIdentityCertRef> {
        let data = X509IdentityCertData::clone_from_slice(cert.as_slice()).map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
        let cert = X509IdentityCert::from_data(data)?;
        Ok(Arc::new(cert))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::p2p_identity::P2pIdentityFactory;
    use crate::p2p_identity::P2pIdentityCertFactory;

    #[test]
    fn test_x509_identity() {
        let id = generate_x509_identity().unwrap();
        let id_ref = id.get_id();
        let id_name = id.get_name();
        assert_eq!(id_ref.to_string(), id_name);
        let id_cert = id.get_identity_cert().unwrap();
        let id_encoded = id.get_encoded_identity().unwrap();
        let id_endpoints = id.endpoints();
        let id_sign = id.sign(b"test").unwrap();
        let id_cert_id = id_cert.get_id();
        assert_eq!(id_ref, id_cert_id);
        let id_cert_verify = id_cert.verify(b"test", &id_sign);
        assert!(id_cert_verify);
        let id_cert_verify_cert = id_cert.verify_cert(id_cert_id.to_string().as_str());
        assert!(id_cert_verify_cert);
        let id_cert_encoded = id_cert.get_encoded_cert().unwrap();
        let id_cert_endpoints = id_cert.endpoints();
        let id_cert_sn_list = id_cert.sn_list();
        let id_cert_update = id_cert.update_endpoints(vec![]);

        let factory = X509IdentityFactory;
        let id2 = factory.create(&id_encoded).unwrap();
        let factory_cert = X509IdentityCertFactory;
        let id_cert2 = factory_cert.create(&id_cert_encoded).unwrap();
    }
}
