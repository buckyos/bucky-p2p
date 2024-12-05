use std::collections::HashMap;
use std::sync::Arc;
use once_cell::sync::OnceCell;
use crate::endpoint::{Endpoint, Protocol};
use crate::error::{p2p_err, P2pErrorCode, P2pResult};
use crate::runtime;

#[async_trait::async_trait]
pub trait P2pConnection {
    fn is_stream(&self) -> bool;
    fn remote(&self) -> Endpoint;
    fn local(&self) -> Endpoint;
    fn split(&self) -> P2pResult<(Box<dyn runtime::AsyncRead>, Box<dyn runtime::AsyncWrite>)>;
    fn unsplit(&self, read: Box<dyn runtime::AsyncRead>, write: Box<dyn runtime::AsyncWrite>);
}

pub type P2pConnectionRef = Arc<dyn P2pConnection>;

#[async_trait::async_trait]
pub trait P2pListener {

}
pub type P2pListenerRef = Arc<dyn P2pListener>;

#[async_trait::async_trait]
pub trait P2pConnectionFactory {
    fn protocol(&self) -> Protocol;
    async fn create_stream_connect(&self, remote: &Endpoint) -> P2pResult<P2pConnectionRef>;
    async fn create_datagram_connect(&self, remote: &Endpoint) -> P2pResult<P2pConnectionRef>;
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
