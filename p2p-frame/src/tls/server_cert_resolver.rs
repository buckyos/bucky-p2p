use std::collections::HashMap;
use std::fmt::{Debug, Formatter};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use rustls::pki_types::CertificateDer;
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;
use crate::p2p_identity::{DeviceId, P2pIdentity};
use crate::tls::sign::TlsKey;

pub struct TlsServerCertResolver {
    device_cache: Mutex<HashMap<DeviceId, Arc<dyn P2pIdentity>>>
}

impl Debug for TlsServerCertResolver {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "TlsServerCertResolver")
    }
}

pub type ServerCertResolverRef = Arc<TlsServerCertResolver>;

impl TlsServerCertResolver {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            device_cache: Mutex::new(Default::default()),
        })
    }

    pub fn add_device(&self, id: Arc<dyn P2pIdentity>) {
        let mut device_cache = self.device_cache.lock().unwrap();
        device_cache.insert(id.get_id(), id);
    }

    pub fn remove_device(&self, device_id: &DeviceId) {
        let mut device_cache = self.device_cache.lock().unwrap();
        device_cache.remove(device_id);
    }

    pub fn get_device(&self, device_id: &DeviceId) -> Option<Arc<dyn P2pIdentity>> {
        let device_cache = self.device_cache.lock().unwrap();
        match device_cache.get(device_id) {
            Some(device_info) => Some(device_info.clone()),
            None => None
        }
    }
}

impl ResolvesServerCert for TlsServerCertResolver {
    fn resolve(&self, client_hello: ClientHello) -> Option<Arc<CertifiedKey>> {
        if client_hello.server_name().is_none() {
            return None;
        }

        let server_name = client_hello.server_name().unwrap();
        let device_id = match DeviceId::from_str(server_name) {
            Ok(device_id) => device_id,
            Err(_) => return None
        };

        log::info!("resolve device_id = {}", device_id);
        let device_cache = self.device_cache.lock().unwrap();
        let device_info = match device_cache.get(&device_id) {
            Some(device_info) => device_info,
            None => return None
        };

        Some(Arc::new(CertifiedKey::new(
            vec![CertificateDer::from(device_info.get_identity_cert().unwrap().get_encoded_cert().unwrap())],
            Arc::new(TlsKey::new(device_info.clone())),
        )))
    }
}
