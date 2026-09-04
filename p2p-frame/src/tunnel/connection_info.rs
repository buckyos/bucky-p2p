use crate::endpoint::Endpoint;
use crate::p2p_identity::P2pId;
use bucky_raw_codec::{RawDecode, RawEncode};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, RawDecode, RawEncode, Eq, PartialEq)]
pub enum ConnectDirection {
    Direct,
    Reverse,
    Proxy,
}

#[derive(Debug, Clone, RawDecode, RawEncode)]
pub struct P2pConnectionInfo {
    pub direct: ConnectDirection,
    pub local_ep: Endpoint,
    pub remote_ep: Endpoint,
}

#[async_trait::async_trait]
pub trait P2pConnectionInfoCache: Send + Sync + 'static {
    async fn get(&self, conn_id: &P2pId) -> Option<P2pConnectionInfo>;
    async fn add(&self, conn_id: P2pId, info: P2pConnectionInfo);
    /// Remove a cached connection entry, if present.
    ///
    /// The default no-op keeps existing third-party implementors
    /// source-compatible; caching implementations should override this so
    /// stale Direct entries discovered by `TunnelManager` can be invalidated.
    async fn remove(&self, _conn_id: &P2pId) {}
}

pub type P2pConnectionInfoCacheRef = Arc<dyn P2pConnectionInfoCache>;

pub struct DefaultP2pConnectionInfoCache {
    cache: Mutex<HashMap<P2pId, P2pConnectionInfo>>,
}

impl DefaultP2pConnectionInfoCache {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            cache: Mutex::new(HashMap::new()),
        })
    }
}

#[async_trait::async_trait]
impl P2pConnectionInfoCache for DefaultP2pConnectionInfoCache {
    async fn get(&self, conn_id: &P2pId) -> Option<P2pConnectionInfo> {
        self.cache.lock().unwrap().get(conn_id).cloned()
    }

    async fn add(&self, conn_id: P2pId, info: P2pConnectionInfo) {
        self.cache.lock().unwrap().insert(conn_id, info);
    }

    async fn remove(&self, conn_id: &P2pId) {
        self.cache.lock().unwrap().remove(conn_id);
    }
}
