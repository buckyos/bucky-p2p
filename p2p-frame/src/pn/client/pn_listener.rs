use crate::error::P2pResult;
use crate::networks::{TunnelListener, TunnelRef};
use crate::p2p_identity::P2pId;
use crate::pn::ProxyOpenReq;
use crate::ttp::TtpListenerRef;

use super::pn_client::read_pn_command;
use super::pn_tunnel::PnTunnel;

pub struct PnListener {
    local_id: P2pId,
    ttp_listener: TtpListenerRef,
}

impl PnListener {
    pub(super) fn new(local_id: P2pId, ttp_listener: TtpListenerRef) -> Self {
        Self {
            local_id,
            ttp_listener,
        }
    }
}

#[async_trait::async_trait]
impl TunnelListener for PnListener {
    async fn accept_tunnel(&self) -> P2pResult<TunnelRef> {
        let (_meta, mut read, write) = self.ttp_listener.accept().await?;
        let req = read_pn_command::<_, ProxyOpenReq>(&mut read).await?;
        log::debug!(
            "pn listener accept local={} from={} to={} kind={:?} vport={} tunnel_id={:?}",
            self.local_id,
            req.from,
            req.to,
            req.kind,
            req.vport,
            req.tunnel_id
        );
        let tunnel: TunnelRef = PnTunnel::new_passive(self.local_id.clone(), req, read, write);
        Ok(tunnel)
    }
}
