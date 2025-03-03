use std::collections::HashMap;
use std::fmt::{Debug, Formatter};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use rustls::pki_types::CertificateDer;
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;
use crate::error::P2pResult;
use crate::p2p_identity::{P2pId, P2pIdentity};
use crate::tls::sign::TlsKey;

#[async_trait::async_trait]
pub trait TlsServerCertResolver: ResolvesServerCert + Send + Sync + 'static {
    async fn add_server_identity(&self, id: Arc<dyn P2pIdentity>) -> P2pResult<()>;
    async fn remove_server_identity(&self, device_id: &str) -> P2pResult<()>;
    async fn get_server_identity(&self, device_id: &str) -> Option<Arc<dyn P2pIdentity>>;
    fn get_resolves_server_cert(self: Arc<Self>) -> Arc<dyn ResolvesServerCert>;
}

pub struct DefaultTlsServerCertResolver {
    device_cache: Mutex<HashMap<String, Arc<dyn P2pIdentity>>>
}

impl Debug for DefaultTlsServerCertResolver {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "TlsServerCertResolver")
    }
}

pub type ServerCertResolverRef = Arc<dyn TlsServerCertResolver>;

impl DefaultTlsServerCertResolver {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            device_cache: Mutex::new(Default::default()),
        })
    }
}

#[async_trait::async_trait]
impl TlsServerCertResolver for DefaultTlsServerCertResolver {
    async fn add_server_identity(&self, id: Arc<dyn P2pIdentity>) -> P2pResult<()> {
        let mut device_cache = self.device_cache.lock().unwrap();
        device_cache.insert(id.get_name(), id);
        Ok(())
    }

    async fn remove_server_identity(&self, device_id: &str) -> P2pResult<()> {
        let mut device_cache = self.device_cache.lock().unwrap();
        device_cache.remove(device_id);
        Ok(())
    }

    async fn get_server_identity(&self, device_id: &str) -> Option<Arc<dyn P2pIdentity>> {
        let device_cache = self.device_cache.lock().unwrap();
        match device_cache.get(device_id) {
            Some(device_info) => Some(device_info.clone()),
            None => None
        }
    }

    fn get_resolves_server_cert(self: Arc<Self>) -> Arc<dyn ResolvesServerCert> {
        self.clone()
    }
}

impl ResolvesServerCert for DefaultTlsServerCertResolver {
    fn resolve(&self, client_hello: ClientHello) -> Option<Arc<CertifiedKey>> {
        if client_hello.server_name().is_none() {
            return None;
        }

        let server_name = match client_hello.server_name() {
            Some(server_name) => server_name,
            None => return None
        };

        log::info!("resolve server = {}", server_name);
        let device_cache = self.device_cache.lock().unwrap();
        let device_info = match device_cache.get(server_name) {
            Some(device_info) => device_info,
            None => return None
        };

        Some(Arc::new(CertifiedKey::new(
            vec![CertificateDer::from(device_info.get_identity_cert().unwrap().get_encoded_cert().unwrap())],
            Arc::new(TlsKey::new(device_info.clone())),
        )))
    }
}
