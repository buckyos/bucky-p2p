use crate::endpoint::Endpoint;
use crate::error::{P2pError, P2pErrorCode, P2pResult};
use bucky_raw_codec::{RawDecode, RawEncode};
use serde::{Deserialize, Serialize};
use std::fmt::{Debug, Display, Formatter};
use std::str::FromStr;
use std::sync::Arc;

pub trait ToBase36 {
    fn to_base36(&self) -> String;
}

pub trait FromBase36 {
    fn from_base36(&self) -> P2pResult<Vec<u8>>;
}

const ALPHABET: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";

impl ToBase36 for [u8] {
    fn to_base36(&self) -> String {
        base_x::encode(ALPHABET, self)
    }
}

impl FromBase36 for str {
    fn from_base36(&self) -> P2pResult<Vec<u8>> {
        base_x::decode(ALPHABET, &self.to_ascii_lowercase()).map_err(|e| {
            let msg = format!("convert string to base36 error! {self}, {e}");
            P2pError::new(P2pErrorCode::InvalidFormat, msg)
        })
    }
}
pub type EncodedP2pIdentityCert = Vec<u8>;

#[derive(RawDecode, RawEncode, Clone, Hash, Eq, PartialEq, PartialOrd, Ord)]
pub struct P2pId(Vec<u8>);

impl P2pId {
    pub fn as_slice(&self) -> &[u8] {
        self.0.as_slice()
    }

    pub fn is_default(&self) -> bool {
        self.0.iter().all(|b| *b == 0)
    }
}

impl FromStr for P2pId {
    type Err = P2pError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(s.from_base36()?))
    }
}

impl From<Vec<u8>> for P2pId {
    fn from(v: Vec<u8>) -> Self {
        Self(v)
    }
}

impl From<&[u8]> for P2pId {
    fn from(v: &[u8]) -> Self {
        Self(v.to_vec())
    }
}

impl Display for P2pId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.as_slice().to_base36())
    }
}

impl Debug for P2pId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.as_slice().to_base36())
    }
}

impl Default for P2pId {
    fn default() -> Self {
        Self { 0: vec![0; 32] }
    }
}

pub type P2pSignature = Vec<u8>;

pub type EncodedP2pIdentity = Vec<u8>;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum P2pIdentitySignType {
    Rsa,
    Ed25519,
}

pub trait P2pIdentity: 'static + Send + Sync {
    fn get_identity_cert(&self) -> P2pResult<P2pIdentityCertRef>;
    fn get_id(&self) -> P2pId;
    fn get_name(&self) -> String;
    fn sign_type(&self) -> P2pIdentitySignType;
    fn sign(&self, message: &[u8]) -> P2pResult<P2pSignature>;
    fn get_encoded_identity(&self) -> P2pResult<EncodedP2pIdentity>;
    fn endpoints(&self) -> Vec<Endpoint>;
    fn update_endpoints(&self, eps: Vec<Endpoint>) -> P2pIdentityRef;
}

#[derive(Debug, Clone, RawEncode, RawDecode)]
pub struct P2pSn {
    id: P2pId,
    name: String,
    endpoints: Vec<Endpoint>,
}

impl P2pSn {
    pub fn new(id: P2pId, name: String, endpoints: Vec<Endpoint>) -> Self {
        Self {
            id,
            name,
            endpoints,
        }
    }

    pub fn get_id(&self) -> P2pId {
        self.id.clone()
    }

    pub fn get_name(&self) -> String {
        self.name.clone()
    }

    pub fn endpoints(&self) -> Vec<Endpoint> {
        self.endpoints.clone()
    }
}

pub type P2pIdentityRef = Arc<dyn P2pIdentity>;

pub trait P2pIdentityFactory: 'static + Send + Sync {
    fn create(&self, id: &EncodedP2pIdentity) -> P2pResult<P2pIdentityRef>;
}
pub type P2pIdentityFactoryRef = Arc<dyn P2pIdentityFactory>;

pub trait P2pIdentityCert: 'static + Send + Sync {
    fn get_id(&self) -> P2pId;
    fn get_name(&self) -> String;
    fn sign_type(&self) -> P2pIdentitySignType;
    fn verify(&self, message: &[u8], sign: &P2pSignature) -> bool;
    fn verify_cert(&self, name: &str) -> bool;
    fn get_encoded_cert(&self) -> P2pResult<EncodedP2pIdentityCert>;
    fn endpoints(&self) -> Vec<Endpoint>;
    fn sn_list(&self) -> Vec<P2pSn>;
    fn update_endpoints(&self, eps: Vec<Endpoint>) -> P2pIdentityCertRef;
}
pub type P2pIdentityCertRef = Arc<dyn P2pIdentityCert>;

pub trait P2pIdentityCertFactory: 'static + Send + Sync {
    fn create(&self, cert: &EncodedP2pIdentityCert) -> P2pResult<P2pIdentityCertRef>;
}
pub type P2pIdentityCertFactoryRef = Arc<dyn P2pIdentityCertFactory>;

#[async_trait::async_trait]
pub trait P2pIdentityCertCache: 'static + Send + Sync {
    async fn add(&self, id: &P2pId, device: &P2pIdentityCertRef) -> P2pResult<()>;
    async fn get(&self, id: &P2pId) -> Option<P2pIdentityCertRef>;
}
pub type P2pIdentityCertCacheRef = Arc<dyn P2pIdentityCertCache>;
