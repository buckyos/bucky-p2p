use bucky_crypto::Signature;
use bucky_objects::{Device, NamedObject, SingleKeyObjectDesc};
use bucky_raw_codec::RawFrom;
use rustls::{CertificateError, DigitallySignedStruct, DistinguishedName, Error, SignatureScheme};
use rustls::client::danger::HandshakeSignatureValid;
use rustls::pki_types::{CertificateDer, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};

#[derive(Debug)]
pub struct BuckyClientCertVerifier {
    pub subjects: Vec<DistinguishedName>,
}

impl ClientCertVerifier for BuckyClientCertVerifier {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        self.subjects.as_slice()
    }

    fn verify_client_cert(&self, end_entity: &CertificateDer<'_>, _intermediates: &[CertificateDer<'_>], _now: UnixTime) -> Result<ClientCertVerified, Error> {
        let _device = Device::clone_from_slice(end_entity.as_ref()).map_err(|_e| Error::InvalidCertificate(CertificateError::BadEncoding))?;
        Ok(ClientCertVerified::assertion())
    }

    fn verify_tls12_signature(&self, _message: &[u8], _cert: &CertificateDer<'_>, _dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, Error> {
        let device = Device::clone_from_slice(cert.as_ref()).map_err(|_e| Error::InvalidCertificate(CertificateError::BadEncoding))?;
        let public_key = device.desc().public_key();
        let sign = Signature::clone_from_slice(dss.signature()).map_err(|_e| Error::General("Invalid signature".to_string()))?;
        if public_key.verify(message, &sign) {
            Ok(HandshakeSignatureValid::assertion())
        } else {
            Err(Error::General("Invalid signature".to_string()))
        }
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![SignatureScheme::RSA_PSS_SHA256]
    }
}
