use std::collections::HashMap;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use bucky_crypto::PrivateKey;
use bucky_objects::{Device, DeviceId, NamedObject};
use bucky_raw_codec::RawConvertTo;
use rustls::pki_types::CertificateDer;
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;
use crate::sign::BuckyKey;

#[derive(Debug)]
struct DeviceInfo {
    device: Device,
    key: PrivateKey
}

#[derive(Debug)]
pub struct ServerCertResolver {
    device_cache: Mutex<HashMap<DeviceId, DeviceInfo>>
}
pub type ServerCertResolverRef = Arc<ServerCertResolver>;

impl ServerCertResolver {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            device_cache: Mutex::new(Default::default()),
        })
    }

    pub fn add_device(&self, device: Device, key: PrivateKey) {
        let mut device_cache = self.device_cache.lock().unwrap();
        device_cache.insert(device.desc().device_id().clone(), DeviceInfo {
            device,
            key
        });
    }

    pub fn remove_device(&self, device_id: &DeviceId) {
        let mut device_cache = self.device_cache.lock().unwrap();
        device_cache.remove(device_id);
    }

    pub fn get_device(&self, device_id: &DeviceId) -> Option<Device> {
        let device_cache = self.device_cache.lock().unwrap();
        match device_cache.get(device_id) {
            Some(device_info) => Some(device_info.device.clone()),
            None => None
        }
    }
}

impl ResolvesServerCert for ServerCertResolver {
    fn resolve(&self, client_hello: ClientHello) -> Option<Arc<CertifiedKey>> {
        if client_hello.server_name().is_none() {
            return None;
        }

        let server_name = client_hello.server_name().unwrap();
        let device_id = match DeviceId::from_str(server_name) {
            Ok(device_id) => device_id,
            Err(_) => return None
        };

        println!("resolve device_id = {}", device_id);
        let device_cache = self.device_cache.lock().unwrap();
        let device_info = match device_cache.get(&device_id) {
            Some(device_info) => device_info,
            None => return None
        };

        Some(Arc::new(CertifiedKey::new(
            vec![CertificateDer::from(device_info.device.to_vec().unwrap())],
            Arc::new(BuckyKey::new(device_info.key.clone())),
        )))
    }
}
