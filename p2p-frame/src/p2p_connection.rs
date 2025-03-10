use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use as_any::AsAny;
use bucky_raw_codec::{RawDecode, RawEncode};
use once_cell::sync::OnceCell;
use sfo_split::{RHalf, Splittable, WHalf};
use crate::endpoint::{Endpoint, Protocol};
use crate::error::{p2p_err, P2pErrorCode, P2pResult};
use crate::p2p_identity::P2pId;
use crate::runtime;


pub trait P2pRead: runtime::AsyncRead + Send + 'static + Unpin + Any + AsAny {
    fn remote(&self) -> Endpoint;
    fn local(&self) -> Endpoint;
    fn remote_id(&self) -> P2pId;
    fn local_id(&self) -> P2pId;
    fn remote_name(&self) -> String;
}

pub trait P2pWrite: runtime::AsyncWrite + Send + 'static + Unpin + Any + AsAny {
    fn remote(&self) -> Endpoint;
    fn local(&self) -> Endpoint;
    fn remote_id(&self) -> P2pId;
    fn local_id(&self) -> P2pId;
    fn remote_name(&self) -> String;
}


#[async_trait::async_trait]
pub trait P2pConnectionMeta: Send + 'static {
    fn remote(&self) -> Endpoint;
    fn local(&self) -> Endpoint;
    fn remote_id(&self) -> P2pId;
    fn local_id(&self) -> P2pId;
    fn remote_name(&self) -> String;
}

pub type P2pConnection = Splittable<Box<dyn P2pRead>, Box<dyn P2pWrite>>;
pub type P2pReadHalf = RHalf<Box<dyn P2pRead>, Box<dyn P2pWrite>>;
pub type P2pWriteHalf = WHalf<Box<dyn P2pRead>, Box<dyn P2pWrite>>;

// impl P2pConnectionMeta for P2pConnection {
//     fn remote(&self) -> Endpoint {
//         self.get_r().remote()
//     }
//
//     fn local(&self) -> Endpoint {
//         self.get_r().local()
//     }
//
//     fn remote_id(&self) -> P2pId {
//         self.get_r().remote_id()
//     }
//
//     fn local_id(&self) -> P2pId {
//         self.get_r().local_id()
//     }
// }

#[callback_trait::callback_trait]
pub trait P2pConnectionEventListener: 'static + Send + Sync {
    async fn on_new_connection(&self, conn: P2pConnection) -> P2pResult<()>;
}

pub trait P2pListener: 'static + Send + Sync {
    fn mapping_port(&self) -> Option<u16>;
    fn local(&self) -> Endpoint;
}

pub type P2pListenerRef = Arc<dyn P2pListener>;

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
}
