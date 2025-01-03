use std::collections::HashMap;
use std::sync::Arc;
use once_cell::sync::OnceCell;
use crate::endpoint::{Endpoint, Protocol};
use crate::error::{p2p_err, P2pErrorCode, P2pResult};
use crate::p2p_identity::P2pId;
use crate::runtime;

#[async_trait::async_trait]
pub trait P2pConnection: Send + Sync + 'static {
    fn is_stream(&self) -> bool;
    fn remote(&self) -> Endpoint;
    fn local(&self) -> Endpoint;
    fn remote_id(&self) -> P2pId;
    fn local_id(&self) -> P2pId;
    fn split(&self) -> P2pResult<(Box<dyn runtime::AsyncRead + Send + Sync + 'static + Unpin>, Box<dyn runtime::AsyncWrite + Send + Sync + 'static + Unpin>)>;
    fn unsplit(&self, read: Box<dyn runtime::AsyncRead + Send + Sync + 'static + Unpin>, write: Box<dyn runtime::AsyncWrite + Send + Sync + 'static + Unpin>);
}

pub type P2pConnectionRef = Arc<dyn P2pConnection>;

#[callback_trait::callback_trait]
pub trait P2pConnectionEventListener: 'static + Send + Sync {
    async fn on_new_connection(&self, conn: P2pConnectionRef) -> P2pResult<()>;
}

pub trait P2pListener {
    fn set_connection_event_listener(&self, event: impl P2pConnectionEventListener);
}

pub type P2pListenerRef = Arc<dyn P2pListener>;

#[async_trait::async_trait]
pub trait P2pConnectionFactory: Send + Sync + 'static {
    fn protocol(&self) -> Protocol;
    async fn create_stream_connect(&self, remote: &Endpoint, remote_id: &P2pId) -> P2pResult<Vec<P2pConnectionRef>>;
    async fn create_datagram_connect(&self, remote: &Endpoint, remote_id: &P2pId) -> P2pResult<Vec<P2pConnectionRef>>;
}
pub type P2pConnectionFactoryRef = Arc<dyn P2pConnectionFactory>;

pub struct P2pConnectionFactoryManager {
    factorys: HashMap<Protocol, P2pConnectionFactoryRef>,
}
static mut P2P_CONNECTION_FACTORY_MANAGER: OnceCell<P2pConnectionFactoryManager> = OnceCell::new();

impl P2pConnectionFactoryManager {
    pub fn get() -> &'static P2pConnectionFactoryManager {
        unsafe {
            P2P_CONNECTION_FACTORY_MANAGER.get_or_init(|| Self {
                factorys: HashMap::new(),
            })
        }
    }

    pub(crate) fn get_mut() -> &'static mut P2pConnectionFactoryManager {
        unsafe {
            Self::get();
            P2P_CONNECTION_FACTORY_MANAGER.get_mut().unwrap()
        }
    }

    pub fn add_factory(&mut self, factory: P2pConnectionFactoryRef) {
        self.factorys.insert(factory.protocol(), factory);
    }

    pub fn get_factory(&self, protocol: &Protocol) -> P2pResult<P2pConnectionFactoryRef> {
        self.factorys.get(protocol).map(|f| f.clone()).ok_or_else(|| {
            p2p_err!(P2pErrorCode::NotFound, "not found factory for protocol {:?}", protocol)
        })
    }
}
