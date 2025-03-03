use std::fmt::Debug;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::{CertificateError, DigitallySignedStruct, Error, SignatureScheme};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use crate::p2p_identity::{P2pId, P2pIdentityCertFactoryRef};

pub struct TlsServerCertVerifier {
    cert_factory: P2pIdentityCertFactoryRef,
    server_id: P2pId,
}

impl Debug for TlsServerCertVerifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TlsServerCertVerifier")
    }
}

impl TlsServerCertVerifier {
    pub fn new(cert_factory: P2pIdentityCertFactoryRef, server_id: P2pId) -> Self {
        Self {
            cert_factory,
            server_id,
        }
    }
}

impl ServerCertVerifier for TlsServerCertVerifier {
    fn verify_server_cert(&self, end_entity: &CertificateDer<'_>, _intermediates: &[CertificateDer<'_>], server_name: &ServerName<'_>, _ocsp_response: &[u8], _now: UnixTime) -> Result<ServerCertVerified, Error> {
        let device = end_entity.as_ref().to_vec();
        let server_name = server_name.to_str().to_string();
        let cert = self.cert_factory.create(&device).map_err(|_| Error::InvalidCertificate(CertificateError::BadEncoding))?;
        if cert.verify_cert(server_name.as_str()) && cert.get_id() == self.server_id {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(Error::General("Invalid server name".to_string()))
        }
    }

    fn verify_tls12_signature(&self, _message: &[u8], _cert: &CertificateDer<'_>, _dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, Error> {
        let device = cert.as_ref().to_vec();
        let sign = dss.signature().to_vec();
        let cert = self.cert_factory.create(&device).map_err(|_| Error::InvalidCertificate(CertificateError::BadEncoding))?;
        if cert.verify(message, &sign) && cert.get_id() == self.server_id {
            Ok(HandshakeSignatureValid::assertion())
        } else {
            Err(Error::General("Invalid signature".to_string()))
        }
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![SignatureScheme::RSA_PSS_SHA256]
    }
}
