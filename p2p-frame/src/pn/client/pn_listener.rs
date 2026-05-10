use std::sync::Arc;

use crate::error::P2pResult;
use crate::networks::{
    TunnelCommandBody, TunnelCommandResult, TunnelListener, TunnelRef, read_tunnel_command_body,
    read_tunnel_command_header,
};
use crate::pn::{ProxyControlOpenReq, ProxyControlOpenResp, ProxyOpenReq};
use crate::ttp::TtpListenerRef;

use super::pn_client::write_pn_command;
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
            let header = read_tunnel_command_header(&mut read).await?;
            if header.command_id == ProxyControlOpenReq::COMMAND_ID {
                let req = read_tunnel_command_body::<_, ProxyControlOpenReq>(&mut read, header)
                    .await?
                    .body;
                log::debug!(
                    "pn listener control accept local={} from={} to={} tunnel_id={:?}",
                    self.shared.local_id(),
                    req.from,
                    req.to,
                    req.tunnel_id
                );
                let mut write = write;
                match self.shared.create_passive_control_tunnel(req.clone()) {
                    Ok(tunnel) => {
                        write_pn_command(
                            &mut write,
                            ProxyControlOpenResp {
                                tunnel_id: req.tunnel_id,
                                result: TunnelCommandResult::Success as u8,
                            },
                        )
                        .await?;
                        tunnel.set_control_channel(read, write).await;
                        return Ok(tunnel);
                    }
                    Err(err) => {
                        let _ = write_pn_command(
                            &mut write,
                            ProxyControlOpenResp {
                                tunnel_id: req.tunnel_id,
                                result: TunnelCommandResult::InternalError as u8,
                            },
                        )
                        .await;
                        return Err(err);
                    }
                }
            }
            let req = read_tunnel_command_body::<_, ProxyOpenReq>(&mut read, header)
                .await?
                .body;
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
