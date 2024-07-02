use bucky_crypto::Signature;
use bucky_objects::{Device, NamedObject, SingleKeyObjectDesc};
use bucky_raw_codec::RawFrom;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::{CertificateError, DigitallySignedStruct, Error, SignatureScheme};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};

#[derive(Debug)]
pub struct BuckyServerCertVerifier {

}

impl ServerCertVerifier for BuckyServerCertVerifier {
    fn verify_server_cert(&self, end_entity: &CertificateDer<'_>, _intermediates: &[CertificateDer<'_>], server_name: &ServerName<'_>, _ocsp_response: &[u8], _now: UnixTime) -> Result<ServerCertVerified, Error> {
        let device = Device::clone_from_slice(end_entity.as_ref()).map_err(|_e| Error::InvalidCertificate(CertificateError::BadEncoding))?;
        let server_name = server_name.to_str().to_string();
        if device.desc().device_id().object_id().to_base36() == server_name {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(Error::General("Invalid server name".to_string()))
        }
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
