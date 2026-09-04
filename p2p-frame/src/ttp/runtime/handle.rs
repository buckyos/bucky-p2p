#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex};

use crate::error::{P2pErrorCode, P2pResult, p2p_err};
use crate::networks::{IncomingTunnelSubscriptionGuard, NetManagerRef, TunnelPurpose, TunnelRef};
use crate::p2p_identity::P2pIdentityRef;

use super::super::TtpTarget;
use super::super::client::{
    TtpTunnelCache, find_existing_tunnel_in_multi, get_or_create_tunnel_for_multi, match_target,
    remember_tunnel_in_multi,
};
use super::super::listener::{
    TtpIncomingControlStreamCallback, TtpIncomingDatagramCallback, TtpIncomingStreamCallback,
};
use super::dispatch::TtpDispatchRuntime;
use super::server::{TtpIncomingTunnelValidateContext, TtpIncomingTunnelValidatorRef};

#[derive(Clone)]
pub struct TtpRuntime(Arc<RuntimeCore>);

pub(super) struct RuntimeCore {
    local_identity: P2pIdentityRef,
    net_manager: NetManagerRef,
    dispatch: Arc<TtpDispatchRuntime>,
    tunnels: Mutex<TtpTunnelCache>,
    incoming_validator: TtpIncomingTunnelValidatorRef,
    subscription: Mutex<Option<IncomingTunnelSubscriptionGuard>>,
    #[cfg(test)]
    accept_progress: AtomicUsize,
}

impl TtpRuntime {
    pub(super) fn new(
        local_identity: P2pIdentityRef,
        net_manager: NetManagerRef,
        incoming_validator: TtpIncomingTunnelValidatorRef,
    ) -> P2pResult<Self> {
        RuntimeCore::new(local_identity, net_manager, incoming_validator).map(Self)
    }

    pub(super) fn core(&self) -> &RuntimeCore {
        &self.0
    }
}

impl RuntimeCore {
    fn new(
        local_identity: P2pIdentityRef,
        net_manager: NetManagerRef,
        incoming_validator: TtpIncomingTunnelValidatorRef,
    ) -> P2pResult<Arc<Self>> {
        let core = Arc::new(Self {
            local_identity,
            net_manager,
            dispatch: TtpDispatchRuntime::new(),
            tunnels: Mutex::new(TtpTunnelCache::new()),
            incoming_validator,
            subscription: Mutex::new(None),
            #[cfg(test)]
            accept_progress: AtomicUsize::new(0),
        });

        let weak = Arc::downgrade(&core);
        let guard = core.net_manager.register_owned_incoming_tunnel_subscriber(
            core.local_identity.get_id(),
            Arc::new(move |result| {
                let weak = weak.clone();
                Box::pin(async move {
                    let Some(core) = weak.upgrade() else {
                        return false;
                    };
                    let tunnel = match result {
                        Ok(tunnel) => tunnel,
                        Err(err) => {
                            log::debug!("ttp runtime accept tunnel failed: {:?}", err);
                            return true;
                        }
                    };
                    core.accept_incoming_tunnel(tunnel).await;
                    true
                })
            }),
        )?;
        *core.subscription.lock().unwrap() = Some(guard);
        Ok(core)
    }

    async fn accept_incoming_tunnel(self: &Arc<Self>, tunnel: TunnelRef) {
        #[cfg(test)]
        self.accept_progress.store(1, AtomicOrdering::SeqCst);
        let context = TtpIncomingTunnelValidateContext {
            local_id: tunnel.local_id(),
            remote_id: tunnel.remote_id(),
            protocol: tunnel.protocol(),
            tunnel_id: tunnel.tunnel_id(),
            candidate_id: tunnel.candidate_id(),
            local_ep: tunnel.local_ep(),
            remote_ep: tunnel.remote_ep(),
        };
        match self.incoming_validator.validate(&context).await {
            Ok(crate::networks::ValidateResult::Accept) => {}
            Ok(crate::networks::ValidateResult::Reject(reason)) => {
                log::warn!(
                    "ttp runtime rejected incoming tunnel local={} remote={} protocol={:?} tunnel_id={:?} candidate_id={:?} reason={}",
                    context.local_id,
                    context.remote_id,
                    context.protocol,
                    context.tunnel_id,
                    context.candidate_id,
                    reason
                );
                #[cfg(test)]
                self.accept_progress.store(10, AtomicOrdering::SeqCst);
                if let Err(err) = tunnel.close() {
                    log::debug!("ttp close rejected incoming tunnel failed: {:?}", err);
                }
                return;
            }
            Err(err) => {
                log::warn!(
                    "ttp runtime incoming tunnel validator failed local={} remote={} protocol={:?} tunnel_id={:?} candidate_id={:?} err={:?}",
                    context.local_id,
                    context.remote_id,
                    context.protocol,
                    context.tunnel_id,
                    context.candidate_id,
                    err
                );
                #[cfg(test)]
                self.accept_progress.store(20, AtomicOrdering::SeqCst);
                if let Err(close_err) = tunnel.close() {
                    log::debug!(
                        "ttp close validator-failed incoming tunnel failed: {:?}",
                        close_err
                    );
                }
                return;
            }
        }
        #[cfg(test)]
        self.accept_progress.store(2, AtomicOrdering::SeqCst);

        if let Err(err) = self.dispatch.attach_tunnel(tunnel.clone()).await {
            log::warn!(
                "ttp runtime attach incoming tunnel failed local={} remote={} protocol={:?} tunnel_id={:?} candidate_id={:?} err={:?}",
                tunnel.local_id(),
                tunnel.remote_id(),
                tunnel.protocol(),
                tunnel.tunnel_id(),
                tunnel.candidate_id(),
                err
            );
            #[cfg(test)]
            self.accept_progress.store(30, AtomicOrdering::SeqCst);
            return;
        }
        #[cfg(test)]
        self.accept_progress.store(3, AtomicOrdering::SeqCst);
        remember_tunnel_in_multi(&self.tunnels, tunnel);
        #[cfg(test)]
        self.accept_progress.store(4, AtomicOrdering::SeqCst);
    }

    pub(super) fn listen_stream(
        &self,
        purpose: TunnelPurpose,
        callback: TtpIncomingStreamCallback,
    ) -> P2pResult<()> {
        self.dispatch.listen_stream(purpose, callback)
    }

    pub(super) fn unlisten_stream(&self, purpose: &TunnelPurpose) {
        self.dispatch.unlisten_stream(purpose);
    }

    pub(super) fn listen_control_stream(
        &self,
        purpose: TunnelPurpose,
        callback: TtpIncomingControlStreamCallback,
    ) -> P2pResult<()> {
        self.dispatch.listen_control_stream(purpose, callback)
    }

    pub(super) fn unlisten_control_stream(&self, purpose: &TunnelPurpose) {
        self.dispatch.unlisten_control_stream(purpose);
    }

    pub(super) fn listen_datagram(
        &self,
        purpose: TunnelPurpose,
        callback: TtpIncomingDatagramCallback,
    ) -> P2pResult<()> {
        self.dispatch.listen_datagram(purpose, callback)
    }

    pub(super) fn unlisten_datagram(&self, purpose: &TunnelPurpose) {
        self.dispatch.unlisten_datagram(purpose);
    }

    pub(super) fn get_existing_tunnel(&self, target: &TtpTarget) -> P2pResult<TunnelRef> {
        find_existing_tunnel_in_multi(&self.tunnels, target).ok_or_else(|| {
            p2p_err!(
                P2pErrorCode::NotFound,
                "ttp server has no incoming tunnel for {} {}",
                target.remote_id,
                target.remote_ep
            )
        })
    }

    pub(super) async fn get_or_create_tunnel(&self, target: &TtpTarget) -> P2pResult<TunnelRef> {
        get_or_create_tunnel_for_multi(
            &self.local_identity,
            &self.net_manager,
            &self.dispatch,
            &self.tunnels,
            target,
        )
        .await
    }
}

#[cfg(test)]
use crate::ttp::has_cached_tunnel_in_multi;

#[cfg(test)]
#[derive(Clone, Debug)]
pub(crate) struct TtpCacheTunnelSnapshot {
    pub remote_id: crate::p2p_identity::P2pId,
    pub state: crate::networks::TunnelState,
    pub closed: bool,
    pub matches_target: bool,
    pub local_ep: Option<crate::endpoint::Endpoint>,
    pub remote_ep: Option<crate::endpoint::Endpoint>,
}

#[cfg(test)]
impl RuntimeCore {
    pub(super) fn has_cached_tunnel_for_test(&self, target: &TtpTarget) -> bool {
        has_cached_tunnel_in_multi(&self.tunnels, target)
    }

    pub(super) fn cache_snapshot_for_test(
        &self,
        target: &TtpTarget,
    ) -> Vec<TtpCacheTunnelSnapshot> {
        let tunnels = self.tunnels.lock().unwrap();
        tunnels
            .iter()
            .flat_map(|(remote_id, entries)| {
                entries.values().map(move |tunnel| TtpCacheTunnelSnapshot {
                    remote_id: remote_id.clone(),
                    state: tunnel.state(),
                    closed: tunnel.is_closed(),
                    matches_target: match_target(tunnel.as_ref(), target),
                    local_ep: tunnel.local_ep(),
                    remote_ep: tunnel.remote_ep(),
                })
            })
            .collect()
    }

    pub(super) fn accept_progress_for_test(&self) -> usize {
        self.accept_progress.load(AtomicOrdering::SeqCst)
    }
}

impl Drop for RuntimeCore {
    fn drop(&mut self) {
        match self.subscription.get_mut() {
            Ok(subscription) => {
                subscription.take();
            }
            Err(poisoned) => {
                poisoned.into_inner().take();
            }
        }
    }
}
