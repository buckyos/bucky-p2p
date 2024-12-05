use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use bucky_crypto::{PrivateKey, Signature};
use bucky_objects::{Device, NamedObject, SingleKeyObjectDesc};
use bucky_raw_codec::{CodecResult, RawConvertTo, RawDecode, RawEncode, RawFrom};
use p2p_frame::endpoint::Endpoint;
use p2p_frame::error::{into_p2p_err, P2pErrorCode, P2pResult};
use p2p_frame::p2p_identity::{P2pId, EncodedP2pIdentity, EncodedP2pIdentityCert, P2pIdentity, P2pIdentityCert, P2pIdentityCertRef, P2pIdentityRef, P2pSignature, P2pIdentityFactory, P2pIdentityCertFactory};
use p2p_frame::stack::{create_p2p_stack, init_p2p, P2pStackRef};
use p2p_frame::tunnel::{DeviceFinder, DeviceFinderRef};

pub struct CyfsIdentityCert {
    device: Device,
}

impl CyfsIdentityCert {
    pub fn new(device: Device) -> Self {
        Self { device }
    }
}

impl P2pIdentityCert for CyfsIdentityCert {
    fn get_id(&self) -> P2pId {
        P2pId::from_str(self.device.desc().device_id().object_id().to_base36().as_str()).unwrap()
    }

    fn verify(&self, message: &[u8], sign: &P2pSignature) -> bool {
        let sign = match Signature::clone_from_slice(sign.as_slice()) {
            Ok(sign) => {sign}
            Err(_) => {
                return false;
            }
        };
        self.device.desc().public_key().verify(message, &sign)
    }

    fn verify_cert(&self, name: &str) -> bool {
        self.device.desc().device_id().object_id().to_base36() == name
    }

    fn get_encoded_cert(&self) -> P2pResult<EncodedP2pIdentityCert> {
        self.device.to_vec().map_err(into_p2p_err!(P2pErrorCode::CertError, "encode device to vec failed"))
    }

    fn endpoints(&self) -> Vec<Endpoint> {
        self.device.connect_info().endpoints().iter().map(|ep| Endpoint::from_str(ep.to_string().as_str()).unwrap()).collect()
    }

    fn sn_list(&self) -> Vec<P2pIdentityCertRef> {
        Vec::new()
    }

    fn update_endpoints(&self, eps: Vec<Endpoint>) -> P2pIdentityCertRef {
        let mut device = self.device.clone();
        let ep_list = device.mut_connect_info().mut_endpoints();
        ep_list.clear();
        for ep in eps {
            ep_list.push(bucky_objects::Endpoint::from_str(ep.to_string().as_str()).unwrap());
        }
        Arc::new(CyfsIdentityCert::new(device))
    }
}

#[derive(RawEncode, RawDecode)]
pub struct CyfsIdentity {
    pub device: Device,
    pub key: PrivateKey,
}

impl CyfsIdentity {
    pub fn new(device: Device, key: PrivateKey) -> Self {
        Self { device, key }
    }

}

impl P2pIdentity for CyfsIdentity {
    fn get_identity_cert(&self) -> P2pResult<P2pIdentityCertRef> {
        Ok(Arc::new(CyfsIdentityCert::new(self.device.clone())))
    }

    fn get_id(&self) -> P2pId {
        P2pId::from_str(self.device.desc().device_id().object_id().to_base36().as_str()).unwrap()
    }

    fn get_name(&self) -> String {
        self.device.desc().device_id().object_id().to_base36()
    }

    fn sign(&self, message: &[u8]) -> P2pResult<P2pSignature> {
        self.key.sign(message).map_err(into_p2p_err!(P2pErrorCode::SignError, "sign error"))?
            .to_vec().map_err(into_p2p_err!(P2pErrorCode::SignError, "sign error"))
    }

    fn get_encoded_identity(&self) -> P2pResult<EncodedP2pIdentity> {
        self.to_vec().map_err(into_p2p_err!(P2pErrorCode::Failed, "encode identity to vec failed"))
    }

    fn endpoints(&self) -> Vec<Endpoint> {
        self.device.connect_info().endpoints().iter().map(|ep| Endpoint::from_str(ep.to_string().as_str()).unwrap()).collect()
    }

    fn update_endpoints(&self, eps: Vec<Endpoint>) -> P2pIdentityRef {
        let mut device = self.device.clone();
        let ep_list = device.mut_connect_info().mut_endpoints();
        ep_list.clear();
        for ep in eps {
            ep_list.push(bucky_objects::Endpoint::from_str(ep.to_string().as_str()).unwrap());
        }
        Arc::new(CyfsIdentity::new(device, self.key.clone()))
    }
}

pub struct CyfsIdentityFactory;

impl P2pIdentityFactory for CyfsIdentityFactory {
    fn create(&self, id: &EncodedP2pIdentity) -> P2pResult<P2pIdentityRef> {
        Ok(Arc::new(CyfsIdentity::clone_from_slice(id.as_slice())
            .map_err(into_p2p_err!(P2pErrorCode::Failed, "decode identity from vec failed"))?))
    }
}

pub struct CyfsIdentityCertFactory;

impl P2pIdentityCertFactory for CyfsIdentityCertFactory {
    fn create(&self, cert: &EncodedP2pIdentityCert) -> P2pResult<P2pIdentityCertRef> {
        Device::clone_from_slice(cert.as_slice())
            .map_err(into_p2p_err!(P2pErrorCode::Failed, "decode device from vec failed"))
            .map(|device| Arc::new(CyfsIdentityCert::new(device)))
    }
}

pub async fn init_cyfs_p2p(eps: &[Endpoint],
                           port_mapping: Option<Vec<(Endpoint, u16)>>,
                           tcp_accept_timout: Duration,) -> P2pResult<()> {
    let identity_factory = Arc::new(CyfsIdentityFactory);
    let cert_factory = Arc::new(CyfsIdentityCertFactory);
    init_p2p(identity_factory, cert_factory, eps, port_mapping, tcp_accept_timout).await
}

pub struct P2pStackBuilder {
    local_identity: Device,
    local_key: PrivateKey,
    sn_list: Vec<Device>,
    conn_timeout: Duration,
    idle_timeout: Duration,
    sn_ping_interval: Duration,
    sn_call_timeout: Duration,
    device_finder: Option<DeviceFinderRef>,
}

impl P2pStackBuilder {
    pub fn new(local_identity: Device, local_key: PrivateKey, sn_list: Vec<Device>) -> Self {
        Self {
            local_identity,
            local_key,
            sn_list,
            conn_timeout: Duration::from_secs(30),
            idle_timeout: Duration::from_secs(600),
            sn_ping_interval: Duration::from_secs(300),
            sn_call_timeout: Duration::from_secs(30),
            device_finder: None,
        }
    }

    pub fn set_conn_timeout(mut self, conn_timeout: Duration) -> Self {
        self.conn_timeout = conn_timeout;
        self
    }

    pub fn set_conn_idle_timeout(mut self, idle_timeout: Duration) -> Self {
        self.idle_timeout = idle_timeout;
        self
    }

    pub fn set_sn_ping_interval(mut self, sn_ping_interval: Duration) -> Self {
        self.sn_ping_interval = sn_ping_interval;
        self
    }

    pub fn set_sn_call_timeout(mut self, sn_call_timeout: Duration) -> Self {
        self.sn_call_timeout = sn_call_timeout;
        self
    }

    pub fn set_device_finder(mut self, device_finder: impl DeviceFinder) -> Self {
        self.device_finder = Some(Arc::new(device_finder));
        self
    }

    pub async fn build(self) -> P2pResult<P2pStackRef> {
        create_p2p_stack(Arc::new(CyfsIdentity::new(self.local_identity, self.local_key)),
                         self.sn_list.iter().map(|d| Arc::new(CyfsIdentityCert::new(d.clone()))).collect(),
                         self.device_finder,
                         self.conn_timeout,
                         self.idle_timeout,
                         self.sn_ping_interval,
                         self.sn_call_timeout).await
    }
}
