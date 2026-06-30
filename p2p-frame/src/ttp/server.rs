use crate::endpoint::{Endpoint, Protocol};
use crate::error::{P2pErrorCode, P2pResult, p2p_err};
use crate::networks::{
    NetManagerRef, TunnelDatagramWrite, TunnelPurpose, TunnelRef, TunnelStreamRead,
    TunnelStreamWrite, ValidateResult,
};
use crate::p2p_identity::{P2pId, P2pIdentityRef};
use crate::types::{TunnelCandidateId, TunnelId};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::client::{TtpConnector, find_existing_tunnel_in, remember_tunnel_in};
use super::listener::{
    TtpIncomingControlStreamCallback, TtpIncomingDatagramCallback, TtpIncomingStreamCallback,
    TtpPortListener,
};
use super::runtime::TtpRuntime;
use super::{TtpDatagramMeta, TtpStreamMeta, TtpTarget};

pub struct TtpServer {
    local_identity: P2pIdentityRef,
    net_manager: NetManagerRef,
    runtime: Arc<TtpRuntime>,
    tunnels: Mutex<HashMap<P2pId, TunnelRef>>,
    incoming_tunnel_validator: TtpIncomingTunnelValidatorRef,
}

pub type TtpServerRef = Arc<TtpServer>;

#[derive(Debug, Clone)]
pub struct TtpIncomingTunnelValidateContext {
    pub local_id: P2pId,
    pub remote_id: P2pId,
    pub protocol: Protocol,
    pub tunnel_id: TunnelId,
    pub candidate_id: TunnelCandidateId,
    pub local_ep: Option<Endpoint>,
    pub remote_ep: Option<Endpoint>,
}

#[async_trait::async_trait]
pub trait TtpIncomingTunnelValidator: Send + Sync + 'static {
    async fn validate(&self, ctx: &TtpIncomingTunnelValidateContext) -> P2pResult<ValidateResult>;
}

pub type TtpIncomingTunnelValidatorRef = Arc<dyn TtpIncomingTunnelValidator>;

pub struct AllowAllTtpIncomingTunnelValidator;

#[async_trait::async_trait]
impl TtpIncomingTunnelValidator for AllowAllTtpIncomingTunnelValidator {
    async fn validate(&self, _ctx: &TtpIncomingTunnelValidateContext) -> P2pResult<ValidateResult> {
        Ok(ValidateResult::Accept)
    }
}

pub fn allow_all_ttp_incoming_tunnel_validator() -> TtpIncomingTunnelValidatorRef {
    Arc::new(AllowAllTtpIncomingTunnelValidator)
}

impl TtpServer {
    pub fn new(
        local_identity: P2pIdentityRef,
        net_manager: NetManagerRef,
    ) -> P2pResult<TtpServerRef> {
        Self::new_with_incoming_tunnel_validator(
            local_identity,
            net_manager,
            allow_all_ttp_incoming_tunnel_validator(),
        )
    }

    pub fn new_with_incoming_tunnel_validator(
        local_identity: P2pIdentityRef,
        net_manager: NetManagerRef,
        incoming_tunnel_validator: TtpIncomingTunnelValidatorRef,
    ) -> P2pResult<TtpServerRef> {
        let server = Arc::new(Self {
            local_identity,
            net_manager,
            runtime: TtpRuntime::new(),
            tunnels: Mutex::new(HashMap::new()),
            incoming_tunnel_validator,
        });
        server.register_accept_callback()?;
        Ok(server)
    }

    fn register_accept_callback(self: &Arc<Self>) -> P2pResult<()> {
        let weak = Arc::downgrade(self);
        self.net_manager.register_incoming_tunnel_subscriber(
            self.local_identity.get_id(),
            Arc::new(move |result| {
                let Some(server) = weak.upgrade() else {
                    return Box::pin(async { false });
                };
                let tunnel = match result {
                    Ok(tunnel) => tunnel,
                    Err(err) => {
                        log::debug!("ttp server accept tunnel failed: {:?}", err);
                        return Box::pin(async { true });
                    }
                };

                Box::pin(async move {
                    let context = TtpIncomingTunnelValidateContext {
                        local_id: tunnel.local_id(),
                        remote_id: tunnel.remote_id(),
                        protocol: tunnel.protocol(),
                        tunnel_id: tunnel.tunnel_id(),
                        candidate_id: tunnel.candidate_id(),
                        local_ep: tunnel.local_ep(),
                        remote_ep: tunnel.remote_ep(),
                    };
                    match server
                        .incoming_tunnel_validator
                        .validate(&context)
                        .await
                    {
                        Ok(ValidateResult::Accept) => {}
                        Ok(ValidateResult::Reject(reason)) => {
                            log::warn!(
                                "ttp server rejected incoming tunnel local={} remote={} protocol={:?} tunnel_id={:?} candidate_id={:?} reason={}",
                                context.local_id,
                                context.remote_id,
                                context.protocol,
                                context.tunnel_id,
                                context.candidate_id,
                                reason
                            );
                            if let Err(err) = tunnel.close() {
                                log::debug!("ttp close rejected incoming tunnel failed: {:?}", err);
                            }
                            return true;
                        }
                        Err(err) => {
                            log::warn!(
                                "ttp server incoming tunnel validator failed local={} remote={} protocol={:?} tunnel_id={:?} candidate_id={:?} err={:?}",
                                context.local_id,
                                context.remote_id,
                                context.protocol,
                                context.tunnel_id,
                                context.candidate_id,
                                err
                            );
                            if let Err(close_err) = tunnel.close() {
                                log::debug!(
                                    "ttp close validator-failed incoming tunnel failed: {:?}",
                                    close_err
                                );
                            }
                            return true;
                        }
                    }
                    log::debug!(
                        "ttp server accepted incoming tunnel local={} remote={} protocol={:?} tunnel_id={:?} candidate_id={:?}",
                        tunnel.local_id(),
                        tunnel.remote_id(),
                        tunnel.protocol(),
                        tunnel.tunnel_id(),
                        tunnel.candidate_id()
                    );
                    if let Err(err) = server.runtime.attach_tunnel(tunnel.clone()).await {
                        log::warn!(
                            "ttp attach incoming tunnel failed local={} remote={} protocol={:?} tunnel_id={:?} candidate_id={:?} err={:?}",
                            tunnel.local_id(),
                            tunnel.remote_id(),
                            tunnel.protocol(),
                            tunnel.tunnel_id(),
                            tunnel.candidate_id(),
                            err
                        );
                        return true;
                    }
                    log::debug!(
                        "ttp attach incoming tunnel success local={} remote={} protocol={:?} tunnel_id={:?} candidate_id={:?}",
                        tunnel.local_id(),
                        tunnel.remote_id(),
                        tunnel.protocol(),
                        tunnel.tunnel_id(),
                        tunnel.candidate_id()
                    );
                    server.remember_tunnel(tunnel);
                    true
                })
            }),
        )
    }

    fn remember_tunnel(&self, tunnel: TunnelRef) {
        remember_tunnel_in(&self.tunnels, tunnel);
    }

    fn find_existing_tunnel(&self, target: &TtpTarget) -> Option<TunnelRef> {
        find_existing_tunnel_in(&self.tunnels, target)
    }

    fn get_existing_tunnel(&self, target: &TtpTarget) -> P2pResult<TunnelRef> {
        self.find_existing_tunnel(target).ok_or_else(|| {
            p2p_err!(
                P2pErrorCode::NotFound,
                "ttp server has no incoming tunnel for {} {}",
                target.remote_id,
                target.remote_ep
            )
        })
    }
}

#[async_trait::async_trait]
impl TtpPortListener for TtpServer {
    async fn listen_stream(
        &self,
        purpose: TunnelPurpose,
        callback: TtpIncomingStreamCallback,
    ) -> P2pResult<()> {
        self.runtime.listen_stream(purpose, callback)
    }

    async fn unlisten_stream(&self, purpose: &TunnelPurpose) -> P2pResult<()> {
        self.runtime.unlisten_stream(purpose);
        Ok(())
    }

    async fn listen_control_stream(
        &self,
        purpose: TunnelPurpose,
        callback: TtpIncomingControlStreamCallback,
    ) -> P2pResult<()> {
        self.runtime.listen_control_stream(purpose, callback)
    }

    async fn unlisten_control_stream(&self, purpose: &TunnelPurpose) -> P2pResult<()> {
        self.runtime.unlisten_control_stream(purpose);
        Ok(())
    }

    async fn listen_datagram(
        &self,
        purpose: TunnelPurpose,
        callback: TtpIncomingDatagramCallback,
    ) -> P2pResult<()> {
        self.runtime.listen_datagram(purpose, callback)
    }

    async fn unlisten_datagram(&self, purpose: &TunnelPurpose) -> P2pResult<()> {
        self.runtime.unlisten_datagram(purpose);
        Ok(())
    }
}

#[async_trait::async_trait]
impl TtpConnector for TtpServer {
    async fn open_stream(
        &self,
        target: &TtpTarget,
        purpose: TunnelPurpose,
    ) -> P2pResult<(TtpStreamMeta, TunnelStreamRead, TunnelStreamWrite)> {
        let tunnel = self.get_existing_tunnel(target)?;
        let (read, write) = tunnel.open_stream(purpose.clone()).await?;
        Ok((
            TtpStreamMeta {
                local_ep: tunnel.local_ep(),
                remote_ep: tunnel.remote_ep().or(Some(target.remote_ep)),
                local_id: tunnel.local_id(),
                remote_id: tunnel.remote_id(),
                remote_name: target.remote_name.clone(),
                purpose,
            },
            read,
            write,
        ))
    }

    async fn open_control_stream(
        &self,
        target: &TtpTarget,
        purpose: TunnelPurpose,
    ) -> P2pResult<(TtpStreamMeta, TunnelStreamRead, TunnelStreamWrite)> {
        let tunnel = self.get_existing_tunnel(target)?;
        let (read, write) = tunnel.open_control_stream(purpose.clone()).await?;
        Ok((
            TtpStreamMeta {
                local_ep: tunnel.local_ep(),
                remote_ep: tunnel.remote_ep().or(Some(target.remote_ep)),
                local_id: tunnel.local_id(),
                remote_id: tunnel.remote_id(),
                remote_name: target.remote_name.clone(),
                purpose,
            },
            read,
            write,
        ))
    }

    async fn open_datagram(
        &self,
        target: &TtpTarget,
        purpose: TunnelPurpose,
    ) -> P2pResult<(TtpDatagramMeta, TunnelDatagramWrite)> {
        let tunnel = self.get_existing_tunnel(target)?;
        let write = tunnel.open_datagram(purpose.clone()).await?;
        Ok((
            TtpDatagramMeta {
                local_ep: tunnel.local_ep(),
                remote_ep: tunnel.remote_ep().or(Some(target.remote_ep)),
                local_id: tunnel.local_id(),
                remote_id: tunnel.remote_id(),
                remote_name: target.remote_name.clone(),
                purpose,
            },
            write,
        ))
    }
}

impl Drop for TtpServer {
    fn drop(&mut self) {
        self.net_manager
            .unregister_incoming_tunnel_subscriber(&self.local_identity.get_id());
    }
}
