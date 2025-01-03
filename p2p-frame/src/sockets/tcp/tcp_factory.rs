use std::sync::Arc;
use std::time::Duration;
use crate::endpoint::{Endpoint, Protocol};
use crate::error::P2pResult;
use crate::p2p_connection::{P2pConnectionFactory, P2pConnectionRef};
use crate::p2p_identity::{P2pId, P2pIdentityCertFactoryRef, P2pIdentityRef};
use crate::sockets::tcp::TCPConnection;

pub struct TcpConnectionFactory {
    local_identity: P2pIdentityRef,
    cert_factory: P2pIdentityCertFactoryRef,
    timeout: Duration,
}

#[async_trait::async_trait]
impl P2pConnectionFactory for TcpConnectionFactory {
    fn protocol(&self) -> Protocol {
        Protocol::Tcp
    }

    async fn create_stream_connect(&self, remote: &Endpoint, remote_id: &P2pId) -> P2pResult<Vec<P2pConnectionRef>> {
        let conn = TCPConnection::connect(self.cert_factory.clone(), self.local_identity.clone(), remote.clone(), remote_id.clone(), self.timeout).await?;
        Ok(vec![Arc::new(conn)])
    }

    async fn create_datagram_connect(&self, remote: &Endpoint, remote_id: &P2pId) -> P2pResult<Vec<P2pConnectionRef>> {
        self.create_stream_connect(remote, remote_id).await
    }
}
