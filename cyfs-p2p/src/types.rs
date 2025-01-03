use futures::future::{AbortHandle, AbortRegistration, Abortable};
use rand::Rng;
use std::fmt;
use std::{
    hash::{Hash, Hasher},
    collections::LinkedList,
    sync::{
        atomic::{AtomicU32, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH}
};
use std::sync::Arc;
use bucky_crypto::{AesKey, KeyMixHash, PrivateKey};
use bucky_error::BuckyError;
use bucky_objects::{Device, DeviceId, Endpoint, NamedObject, Protocol};
use bucky_raw_codec::{RawConvertTo, RawDecode, RawEncode, RawEncodePurpose, RawFixedBytes};
use bucky_time::bucky_time_now;
use sha2::Digest;
use p2p_frame::error::P2pResult;
use p2p_frame::p2p_identity::{P2pIdentity, EncodedP2pIdentityCert, P2pId, P2pSignature, EncodedP2pIdentity, P2pIdentityRef};

#[derive(Clone)]
pub struct MixAesKey {
    pub enc_key: AesKey,
    pub mix_key: AesKey
}

impl std::fmt::Display for MixAesKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "enc {}, mix {}", self.enc_key.to_hex().unwrap(), self.mix_key.to_hex().unwrap())
    }
}


impl MixAesKey {
    pub fn mix_hash(&self, local_id: &DeviceId, remote_id: &DeviceId) -> KeyMixHash {
        let mut sha = sha2::Sha256::new();
        sha.input(local_id.object_id().as_slice());
        sha.input(remote_id.object_id().as_slice());
        let hash = sha.result();
        let salt = u64::from_be_bytes([hash[0], hash[1], hash[2], hash[3], hash[4], hash[5], hash[6], hash[7]]);
        self.mix_key.mix_hash(Some(salt))
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct EndpointPair(Endpoint, Endpoint);

impl std::fmt::Display for EndpointPair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{{{},{}}}", self.0, self.1)
    }
}

impl From<(Endpoint, Endpoint)> for EndpointPair {
    fn from(ep_pair: (Endpoint, Endpoint)) -> Self {
        assert!(ep_pair.0.is_same_ip_version(&ep_pair.1));
        assert!(ep_pair.0.protocol() == ep_pair.1.protocol());
        Self(ep_pair.0, ep_pair.1)
    }
}

impl EndpointPair {
    pub fn local(&self) -> &Endpoint {
        &self.0
    }

    pub fn remote(&self) -> &Endpoint {
        &self.1
    }

    pub fn protocol(&self) -> Protocol {
        self.0.protocol()
    }

    pub fn is_ipv4(&self) -> bool {
        self.0.addr().is_ipv4()
    }

    pub fn is_ipv6(&self) -> bool {
        self.0.addr().is_ipv6()
    }

    pub fn is_tcp(&self) -> bool {
        self.0.is_tcp() && self.0.addr().port() == 0
    }

    pub fn is_udp(&self) -> bool {
        self.0.is_udp()
    }

    pub fn is_reverse_tcp(&self) -> bool {
        self.0.is_tcp() && self.0.addr().port() != 0
    }
}
