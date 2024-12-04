use std::fmt::{Debug, Formatter};
use bucky_raw_codec::RawFrom;
use rustls::{CertificateError, DigitallySignedStruct, DistinguishedName, Error, SignatureScheme};
use rustls::client::danger::HandshakeSignatureValid;
use rustls::pki_types::{CertificateDer, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use crate::p2p_identity::{EncodedP2pIdentityCert, P2pIdentityCertFactoryRef, P2pSignature};

pub struct TlsClientCertVerifier {
    pub subjects: Vec<DistinguishedName>,
    cert_factory: P2pIdentityCertFactoryRef,
}

impl Debug for TlsClientCertVerifier {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "TlsClientCertVerifier")
    }
}

impl TlsClientCertVerifier {
    pub fn new(cert_factory: P2pIdentityCertFactoryRef,) -> Self {
        Self {
            subjects: vec![],
            cert_factory,
        }
    }
}

impl ClientCertVerifier for TlsClientCertVerifier {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        self.subjects.as_slice()
    }

    fn verify_client_cert(&self, end_entity: &CertificateDer<'_>, _intermediates: &[CertificateDer<'_>], _now: UnixTime) -> Result<ClientCertVerified, Error> {
        let _ = EncodedP2pIdentityCert::clone_from_slice(end_entity.as_ref()).map_err(|_e| Error::InvalidCertificate(CertificateError::BadEncoding))?;
        return Ok(ClientCertVerified::assertion());
    }

    fn verify_tls12_signature(&self, _message: &[u8], _cert: &CertificateDer<'_>, _dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, Error> {
        let device = EncodedP2pIdentityCert::clone_from_slice(cert.as_ref()).map_err(|_e| Error::InvalidCertificate(CertificateError::BadEncoding))?;
        let sign = P2pSignature::clone_from_slice(dss.signature()).map_err(|_e| Error::General("Invalid signature".to_string()))?;
        let cert = self.cert_factory.create(&device).map_err(|_e| Error::General("Invalid certificate".to_string()))?;
        if cert.verify(message, &sign) {
            Ok(HandshakeSignatureValid::assertion())
        } else {
            Err(Error::General("Invalid signature".to_string()))
        }
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![SignatureScheme::RSA_PSS_SHA256]
    }
}
