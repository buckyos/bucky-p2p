use std::net::IpAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use bucky_crypto::{PrivateKey, Signature};
use bucky_objects::{Device, Endpoint, NamedObject, ObjectDesc, Protocol, SingleKeyObjectDesc};
use bucky_raw_codec::{CodecResult, RawConvertTo, RawDecode, RawEncode, RawFrom};
use p2p_frame::endpoint::EndpointArea;
use p2p_frame::error::{into_p2p_err, P2pErrorCode, P2pResult};
use p2p_frame::p2p_identity::{P2pId, EncodedP2pIdentity, EncodedP2pIdentityCert, P2pIdentity, P2pIdentityCert, P2pIdentityCertRef, P2pIdentityRef, P2pSignature, P2pIdentityFactory, P2pIdentityCertFactory, P2pSn};
use p2p_frame::stack::{create_p2p_stack, init_p2p, P2pConfig, P2pStackConfig, P2pStackRef};

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

    fn get_name(&self) -> String {
        self.device.desc().device_id().object_id().to_base36()
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

    fn endpoints(&self) -> Vec<p2p_frame::endpoint::Endpoint> {
        self.device.connect_info().endpoints().iter().filter(|ep| !ep.addr().ip().is_unspecified()).map(|ep| p2p_frame::endpoint::Endpoint::from_str(ep.to_string().as_str()).unwrap()).collect::<Vec<_>>()
    }

    fn sn_list(&self) -> Vec<P2pSn> {
        Vec::new()
    }

    fn update_endpoints(&self, eps: Vec<p2p_frame::endpoint::Endpoint>) -> P2pIdentityCertRef {
        let mut device = self.device.clone();
        let ep_list = device.mut_connect_info().mut_endpoints();
        ep_list.clear();
        for ep in eps {
            let mut cyfs_ep = if ep.protocol() == p2p_frame::endpoint::Protocol::Tcp {
                bucky_objects::Endpoint::from((bucky_objects::Protocol::Tcp, ep.addr().clone()))
            } else {
                bucky_objects::Endpoint::from((bucky_objects::Protocol::Udp, ep.addr().clone()))
            };
            match ep.get_area() {
                EndpointArea::Lan => {
                    cyfs_ep.set_area(bucky_objects::EndpointArea::Lan);
                }
                EndpointArea::Default => {
                    cyfs_ep.set_area(bucky_objects::EndpointArea::Default);
                }
                EndpointArea::Wan => {
                    cyfs_ep.set_area(bucky_objects::EndpointArea::Wan);
                }
                EndpointArea::Mapped => {
                    cyfs_ep.set_area(bucky_objects::EndpointArea::Mapped);
                }
            }
            ep_list.push(cyfs_ep);
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

    fn endpoints(&self) -> Vec<p2p_frame::endpoint::Endpoint> {
        self.device.connect_info().endpoints().iter().map(|ep| p2p_frame::endpoint::Endpoint::from_str(ep.to_string().as_str()).unwrap()).collect::<Vec<_>>()
    }

    fn update_endpoints(&self, eps: Vec<p2p_frame::endpoint::Endpoint>) -> P2pIdentityRef {
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
        let device = Device::clone_from_slice(cert.as_slice())
            .map_err(into_p2p_err!(P2pErrorCode::Failed, "decode device from vec failed"))?;
        Ok(Arc::new(CyfsIdentityCert::new(device)))
    }
}

pub fn cyfs_to_p2p_endpoint(ep: &Endpoint) -> p2p_frame::endpoint::Endpoint {
    match ep.protocol() {
        Protocol::Unk => {
            p2p_frame::endpoint::Endpoint::from((p2p_frame::endpoint::Protocol::Quic, ep.addr().clone()))
        }
        Protocol::Tcp => {
            p2p_frame::endpoint::Endpoint::from((p2p_frame::endpoint::Protocol::Tcp, ep.addr().clone()))
        }
        Protocol::Udp => {
            p2p_frame::endpoint::Endpoint::from((p2p_frame::endpoint::Protocol::Quic, ep.addr().clone()))
        }
    }
}

pub fn create_cyfs_p2p_config(ep: Vec<p2p_frame::endpoint::Endpoint>) -> P2pConfig {
    let identity_factory = Arc::new(CyfsIdentityFactory);
    let cert_factory = Arc::new(CyfsIdentityCertFactory);
    let config = P2pConfig::new(identity_factory, cert_factory, ep);
    config
}

pub fn create_cyfs_p2p_stack_config(local_identity: Device, local_key: PrivateKey, sn_list: Vec<Device>) -> P2pStackConfig {
    let mut p2p_sn_list: Vec<P2pSn> = Vec::new();
    for sn in sn_list {
        let p2p_id = P2pId::from(sn.desc().object_id().as_slice());
        p2p_sn_list.push(P2pSn::new(p2p_id.clone(), p2p_id.to_string(), sn.connect_info().endpoints().iter().map(|ep| cyfs_to_p2p_endpoint(ep)).collect()));
    }
    P2pStackConfig::new(Arc::new(CyfsIdentity::new(local_identity, local_key))).add_sn_list(p2p_sn_list)
}
