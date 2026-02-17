use crate::error::{P2pResult, p2p_err};
use crate::executor::*;
use crate::p2p_connection::P2pConnection;
use crate::p2p_identity::{P2pIdentityCertFactoryRef, P2pIdentityRef};
use crate::pn::PnClientRef;
use crate::protocol::v0::{SnCalled, SynReverseSession, SynSession, TunnelType};
use crate::runtime;
use crate::sn::client::SNClientServiceRef;
use crate::sockets::NetManagerRef;
use crate::tunnel::proxy_connection::{ProxyConnectionRead, ProxyConnectionWrite};
use crate::tunnel::{
    TunnelConnection, TunnelConnectionRead, TunnelConnectionRef, TunnelConnectionWrite,
    TunnelSession,
};
use crate::types::TunnelId;
use callback_result::SingleCallbackWaiter;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

struct TunnelListenerState {
    waiter_map: HashMap<TunnelType, Arc<dyn NewTunnelEvent>>,
    pn_listen_handle: Option<SpawnHandle<()>>,
}

#[async_trait::async_trait]
pub trait NewTunnelEvent: Send + Sync + 'static {
    async fn on_new_tunnel(
        &self,
        session: SynSession,
        conn: TunnelConnectionRef,
        read: TunnelConnectionRead,
        write: TunnelConnectionWrite,
    ) -> P2pResult<()>;
    async fn on_new_reverse_tunnel(
        &self,
        session: SynReverseSession,
        conn: TunnelConnectionRef,
        read: TunnelConnectionRead,
        write: TunnelConnectionWrite,
    ) -> P2pResult<()>;
    async fn on_sn_called(&self, called: SnCalled) -> P2pResult<()>;
}

pub struct TunnelListener {
    local_identity: P2pIdentityRef,
    net_manager: NetManagerRef,
    pn_client: Option<PnClientRef>,
    sn_service: SNClientServiceRef,
    state: Mutex<TunnelListenerState>,
    protocol_version: u8,
    cert_factory: P2pIdentityCertFactoryRef,
    conn_timeout: Duration,
}
pub type TunnelListenerRef = Arc<TunnelListener>;

impl TunnelListener {
    pub fn new(
        local_identity: P2pIdentityRef,
        net_manager: NetManagerRef,
        sn_service: SNClientServiceRef,
        pn_client: Option<PnClientRef>,
        protocol_version: u8,
        cert_factory: P2pIdentityCertFactoryRef,
        conn_timeout: Duration,
    ) -> TunnelListenerRef {
        Arc::new(TunnelListener {
            local_identity,
            net_manager,
            pn_client,
            sn_service,
            state: Mutex::new(TunnelListenerState {
                waiter_map: Default::default(),
                pn_listen_handle: None,
            }),
            protocol_version,
            cert_factory,
            conn_timeout,
        })
    }

    pub fn register_new_tunnel_event(&self, session_type: TunnelType, event: impl NewTunnelEvent) {
        let mut state = self.state.lock().unwrap();
        state.waiter_map.insert(session_type, Arc::new(event));
    }

    pub fn remove_new_tunnel_event(&self, session_type: TunnelType) {
        let mut state = self.state.lock().unwrap();
        state.waiter_map.remove(&session_type);
    }

    pub fn listen(self: &Arc<Self>) {
        let this = self.clone();
        self.net_manager
            .set_connection_event_listener(self.local_identity.get_id(), move |conn| {
                let this = this.clone();
                async move {
                    Executor::spawn_ok(async move {
                        if let Err(e) = this.accept_tunnel(conn).await {
                            error!("accept tunnel error: {:?}", e);
                        }
                    });
                    Ok(())
                }
            });

        if self.pn_client.is_some() {
            let pn_client = self.pn_client.clone().unwrap();
            let local_id = self.local_identity.get_id();
            let this = self.clone();
            let handle = Executor::spawn_with_handle(async move {
                loop {
                    match pn_client.accept().await {
                        Ok((read, write)) => {
                            let read = Box::new(ProxyConnectionRead::new(read, local_id.clone()));
                            let write =
                                Box::new(ProxyConnectionWrite::new(write, local_id.clone()));
                            let proxy_conn = P2pConnection::new(read, write);
                            let this = this.clone();
                            Executor::spawn_ok(async move {
                                if let Err(e) = this.accept_tunnel(proxy_conn).await {
                                    error!("accept tunnel error: {:?}", e);
                                }
                            });
                        }
                        Err(e) => {
                            error!("accept pn connection error: {:?}", e);
                            break;
                        }
                    }
                }
            })
            .unwrap();
            let mut state = self.state.lock().unwrap();
            state.pn_listen_handle = Some(handle);
        }

        let this = Arc::downgrade(self);
        self.sn_service.set_listener(move |called: SnCalled| {
            let this = this.clone();
            async move {
                Executor::spawn(async move {
                    if let Some(listener) = this.upgrade() {
                        if let Err(e) = listener.on_sn_called(called).await {
                            error!("on sn called error: {:?}", e);
                        }
                    }
                });
                Ok(())
            }
        });
    }

    async fn on_sn_called(&self, called: SnCalled) -> P2pResult<()> {
        match called.call_type {
            TunnelType::Stream => {
                let listener = {
                    let state = self.state.lock().unwrap();
                    state.waiter_map.get(&called.call_type).map(|v| v.clone())
                };
                if let Some(listener) = listener {
                    listener.on_sn_called(called).await?;
                }
            }
            TunnelType::Datagram => {
                let listener = {
                    let state = self.state.lock().unwrap();
                    state.waiter_map.get(&called.call_type).map(|v| v.clone())
                };
                if let Some(listener) = listener {
                    listener.on_sn_called(called).await?;
                }
            }
        }
        Ok(())
    }

    async fn accept_tunnel(&self, conn: P2pConnection) -> P2pResult<()> {
        let tunnel_conn = TunnelConnection::new(
            TunnelId::from(0),
            self.local_identity.clone(),
            conn.remote_id(),
            conn.remote(),
            self.conn_timeout,
            self.protocol_version,
            conn,
            self.cert_factory.clone(),
        );
        match tunnel_conn.accept_session().await? {
            TunnelSession::Forward((session, read, write)) => {
                let listener = {
                    let state = self.state.lock().unwrap();
                    state
                        .waiter_map
                        .get(&session.tunnel_type)
                        .map(|v| v.clone())
                };
                if let Some(listener) = listener {
                    listener
                        .on_new_tunnel(session, tunnel_conn.clone(), read, write)
                        .await?;
                }
                Ok(())
            }
            TunnelSession::Reverse((session, read, write)) => {
                let listener = {
                    let state = self.state.lock().unwrap();
                    state
                        .waiter_map
                        .get(&session.tunnel_type)
                        .map(|v| v.clone())
                };
                if let Some(listener) = listener {
                    listener
                        .on_new_reverse_tunnel(session, tunnel_conn.clone(), read, write)
                        .await?;
                }
                Ok(())
            }
        }
    }

    pub fn stop(&self) {
        self.net_manager
            .remove_connection_event_listener(&self.local_identity.get_id());
        if let Some(handle) = self.state.lock().unwrap().pn_listen_handle.take() {
            handle.abort();
        }
    }
}

impl Drop for TunnelListener {
    fn drop(&mut self) {
        self.stop();
    }
}
