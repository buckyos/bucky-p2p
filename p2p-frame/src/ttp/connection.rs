use std::future::Future;

use crate::error::{P2pErrorCode, P2pResult, p2p_err};
use crate::networks::{NetManagerRef, TunnelRef};
use crate::p2p_identity::P2pIdentityRef;

use super::TtpTarget;

pub(super) struct TtpDialer<'a> {
    local_identity: &'a P2pIdentityRef,
    net_manager: &'a NetManagerRef,
}

impl<'a> TtpDialer<'a> {
    pub(super) fn new(
        local_identity: &'a P2pIdentityRef,
        net_manager: &'a NetManagerRef,
    ) -> Self {
        Self {
            local_identity,
            net_manager,
        }
    }

    pub(super) async fn connect<F, Fut>(
        &self,
        target: &TtpTarget,
        attach: F,
    ) -> P2pResult<TunnelRef>
    where
        F: Fn(TunnelRef) -> Fut,
        Fut: Future<Output = P2pResult<()>>,
    {
        if target.remote_ep.is_empty() {
            return Err(p2p_err!(
                P2pErrorCode::InvalidParam,
                "ttp target {} has no remote endpoint",
                target.remote_id
            ));
        }

        let mut last_code = P2pErrorCode::ConnectFailed;
        let mut failures = Vec::new();
        for remote_ep in &target.remote_ep {
            let result: P2pResult<TunnelRef> = async {
                if let Some(local_ep) = target.local_ep.as_ref() {
                    if local_ep.protocol() != remote_ep.protocol()
                        || !local_ep.is_same_ip_version(remote_ep)
                    {
                        return Err(p2p_err!(
                            P2pErrorCode::InvalidParam,
                            "local endpoint {} is incompatible with {}",
                            local_ep,
                            remote_ep
                        ));
                    }
                }

                let network = self.net_manager.get_network(remote_ep.protocol())?;
                let tunnel = if let Some(local_ep) = target.local_ep.as_ref() {
                    network
                        .create_tunnel_with_local_ep(
                            self.local_identity,
                            local_ep,
                            remote_ep,
                            &target.remote_id,
                            target.remote_name.clone(),
                        )
                        .await?
                } else {
                    network
                        .create_tunnel(
                            self.local_identity,
                            remote_ep,
                            &target.remote_id,
                            target.remote_name.clone(),
                        )
                        .await?
                };
                attach(tunnel.clone()).await?;
                Ok(tunnel)
            }
            .await;

            match result {
                Ok(tunnel) => return Ok(tunnel),
                Err(err) => {
                    last_code = err.code();
                    failures.push(format!("{}: {}", remote_ep, err));
                }
            }
        }

        Err(p2p_err!(
            last_code,
            "ttp connect to {} candidates {:?} failed: {}",
            target.remote_id,
            target.remote_ep,
            failures.join("; ")
        ))
    }
}
