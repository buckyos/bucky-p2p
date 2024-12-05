use std::fmt::Display;
use std::str::FromStr;
use std::sync::Arc;
use bucky_raw_codec::{RawDecode, RawEncode};
use crate::endpoint::Endpoint;
use crate::error::{P2pError, P2pResult};

pub type EncodedP2pIdentityCert = Vec<u8>;

#[derive(RawDecode, RawEncode, Clone, Hash, Eq, PartialEq, PartialOrd, Ord, Debug)]
pub struct P2pId(Vec<u8>);

impl FromStr for P2pId {
    type Err = P2pError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Vec::from(s.as_bytes())))
    }
}

impl Display for P2pId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", String::from_utf8_lossy(self.0.as_slice()).to_string())
    }
}

pub type P2pSignature = Vec<u8>;

pub type EncodedP2pIdentity = Vec<u8>;

pub trait P2pIdentity: 'static + Send + Sync {
    fn get_identity_cert(&self) -> P2pResult<P2pIdentityCertRef>;
    fn get_id(&self) -> P2pId;
    fn get_name(&self) -> String;
    fn sign(&self, message: &[u8]) -> P2pResult<P2pSignature>;
    fn get_encoded_identity(&self) -> P2pResult<EncodedP2pIdentity>;
    fn endpoints(&self) -> Vec<Endpoint>;
    fn update_endpoints(&self, eps: Vec<Endpoint>) -> P2pIdentityRef;
}

// pub type P2pIdentityRef = Arc<dyn P2pIdentity>;
pub type P2pIdentityRef = Arc<dyn P2pIdentity>;

pub trait P2pIdentityFactory: 'static + Send + Sync {
    fn create(&self, id: &EncodedP2pIdentity) -> P2pResult<P2pIdentityRef>;
}
pub type P2pIdentityFactoryRef = Arc<dyn P2pIdentityFactory>;

pub trait P2pIdentityCert: 'static + Send + Sync {
    fn get_id(&self) -> P2pId;
    fn verify(&self, message: &[u8], sign: &P2pSignature) -> bool;
    fn verify_cert(&self, name: &str) -> bool;
    fn get_encoded_cert(&self) -> P2pResult<EncodedP2pIdentityCert>;
    fn endpoints(&self) -> Vec<Endpoint>;
    fn sn_list(&self) -> Vec<P2pIdentityCertRef>;
    fn update_endpoints(&self, eps: Vec<Endpoint>) -> P2pIdentityCertRef;
}
pub type P2pIdentityCertRef = Arc<dyn P2pIdentityCert>;

pub trait P2pIdentityCertFactory: 'static + Send + Sync {
    fn create(&self, cert: &EncodedP2pIdentityCert) -> P2pResult<P2pIdentityCertRef>;
}
pub type P2pIdentityCertFactoryRef = Arc<dyn P2pIdentityCertFactory>;
