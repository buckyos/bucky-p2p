use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;
use once_cell::sync::OnceCell;
use crate::endpoint::{Endpoint, Protocol};
use crate::error::{p2p_err, P2pErrorCode, P2pResult};
use crate::p2p_identity::P2pId;
use crate::runtime;

pub trait P2pRead: runtime::AsyncRead + Send + Sync + 'static + Unpin + Any {
    fn get_any(&self) -> &dyn Any;
    fn get_any_mut(&mut self) -> &mut dyn Any;
}

pub trait P2pWrite: runtime::AsyncWrite + Send + Sync + 'static + Unpin + Any {
    fn get_any(&self) -> &dyn Any;
    fn get_any_mut(&mut self) -> &mut dyn Any;
}

#[async_trait::async_trait]
pub trait P2pConnection: Send + Sync + 'static {
    fn is_stream(&self) -> bool;
    fn remote(&self) -> Endpoint;
    fn local(&self) -> Endpoint;
    fn remote_id(&self) -> P2pId;
    fn local_id(&self) -> P2pId;
    fn split(&self) -> P2pResult<(Box<dyn P2pRead>, Box<dyn P2pWrite>)>;
    fn unsplit(&self, read: Box<dyn P2pRead>, write: Box<dyn P2pWrite>);
}

pub type P2pConnectionRef = Arc<dyn P2pConnection>;

#[callback_trait::callback_trait]
pub trait P2pConnectionEventListener: 'static + Send + Sync {
    async fn on_new_connection(&self, conn: P2pConnectionRef) -> P2pResult<()>;
}

pub trait P2pListener {
    fn mapping_port(&self) -> Option<u16>;
    fn local(&self) -> Endpoint;
}

pub type P2pListenerRef = Arc<dyn P2pListener>;
