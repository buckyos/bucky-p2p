use std::sync::Arc;

use crate::error::P2pResult;
use crate::networks::{TunnelListener, TunnelRef};
use crate::pn::ProxyOpenReq;
use crate::ttp::TtpListenerRef;

use super::pn_client::read_pn_command;
use super::pn_client::{PassiveTunnelDispatch, PnShared};

pub struct PnListener {
    shared: Arc<PnShared>,
    ttp_listener: TtpListenerRef,
}

impl PnListener {
    pub(super) fn new(shared: Arc<PnShared>, ttp_listener: TtpListenerRef) -> Self {
        Self {
            shared,
            ttp_listener,
        }
    }
}

#[async_trait::async_trait]
impl TunnelListener for PnListener {
    async fn accept_tunnel(&self) -> P2pResult<TunnelRef> {
        loop {
            let (_meta, mut read, write) = self.ttp_listener.accept().await?;
            let req = read_pn_command::<_, ProxyOpenReq>(&mut read).await?;
            log::debug!(
                "pn listener accept local={} from={} to={} kind={:?} purpose={} tunnel_id={:?}",
                self.shared.local_id(),
                req.from,
                req.to,
                req.kind,
                req.purpose,
                req.tunnel_id
            );
            let key = PnShared::tunnel_key(req.from.clone(), req.tunnel_id);
            match self
                .shared
                .dispatch_or_create_passive_tunnel(key, req, read, write)
            {
                PassiveTunnelDispatch::Dispatched => continue,
                PassiveTunnelDispatch::Created(tunnel) => return Ok(tunnel),
            }
        }
    }
}
