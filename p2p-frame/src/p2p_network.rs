use crate::endpoint::{Endpoint, Protocol};
use crate::error::P2pResult;
use crate::p2p_connection::{P2pConnection, P2pConnectionEventListener, P2pListenerRef};
use crate::p2p_identity::{P2pId, P2pIdentityRef};
use std::sync::Arc;

#[async_trait::async_trait]
pub trait P2pNetwork: Send + Sync + 'static {
    fn protocol(&self) -> Protocol;
    fn is_udp(&self) -> bool;
    async fn listen(
        &self,
        local: &Endpoint,
        out: Option<Endpoint>,
        mapping_port: Option<u16>,
        event: Arc<dyn P2pConnectionEventListener>,
    ) -> P2pResult<P2pListenerRef>;
    async fn close_all_listener(&self) -> P2pResult<()>;
    fn listeners(&self) -> Vec<P2pListenerRef>;
    async fn create_stream_connect(
        &self,
        local_identity: &P2pIdentityRef,
        remote: &Endpoint,
        remote_id: &P2pId,
        remote_name: Option<String>,
    ) -> P2pResult<Vec<P2pConnection>>;
    async fn create_stream_connect_with_local_ep(
        &self,
        local_identity: &P2pIdentityRef,
        local_ep: &Endpoint,
        remote: &Endpoint,
        remote_id: &P2pId,
        remote_name: Option<String>,
    ) -> P2pResult<P2pConnection>;
    async fn create_datagram_connect(
        &self,
        local_identity: &P2pIdentityRef,
        remote: &Endpoint,
        remote_id: &P2pId,
        remote_name: Option<String>,
    ) -> P2pResult<Vec<P2pConnection>>;
    async fn create_datagram_connect_with_local_ep(
        &self,
        local_identity: &P2pIdentityRef,
        local_ep: &Endpoint,
        remote: &Endpoint,
        remote_id: &P2pId,
        remote_name: Option<String>,
    ) -> P2pResult<P2pConnection>;
}
pub type P2pNetworkRef = Arc<dyn P2pNetwork>;
