use std::sync::Arc;
use std::time::Duration;
use crate::endpoint::{Endpoint, Protocol};
use crate::error::P2pResult;
use crate::p2p_connection::{P2pConnectionFactory, P2pConnectionRef};
use crate::p2p_identity::{P2pId, P2pIdentityCertFactoryRef, P2pIdentityRef};
use crate::sockets::{QuicConnection, QuicListenerRef};

pub struct QuicConnectionFactory {
    local_identity: P2pIdentityRef,
    cert_factory: P2pIdentityCertFactoryRef,
    quic_listener_list: Vec<QuicListenerRef>,
    timeout: Duration,
    idle_timeout: Duration
}

#[async_trait::async_trait]
impl P2pConnectionFactory for QuicConnectionFactory {
    fn protocol(&self) -> Protocol {
        Protocol::Quic
    }

    async fn create_stream_connect(&self, remote: &Endpoint, remote_id: &P2pId) -> P2pResult<Vec<P2pConnectionRef>> {
        let mut conn_list: Vec<P2pConnectionRef> = Vec::new();
        for listener in self.quic_listener_list.iter() {
            let mut conn = QuicConnection::connect_with_ep(listener.quic_ep(), self.local_identity.clone(), self.cert_factory.clone(), remote_id.clone(), remote.clone(), self.timeout, self.idle_timeout).await?;
            conn.open_bi_stream().await?;
            conn_list.push(Arc::new(conn));
        }
        Ok(conn_list)
    }

    async fn create_datagram_connect(&self, remote: &Endpoint, remote_id: &P2pId) -> P2pResult<Vec<P2pConnectionRef>> {
        self.create_stream_connect(remote, remote_id).await
    }
}
