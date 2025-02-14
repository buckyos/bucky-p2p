use std::sync::Arc;
use bucky_raw_codec::{RawConvertTo, RawDecode, RawFrom};
use sfo_cmd_server::{CmdBodyRead, PeerId};
use sfo_cmd_server::errors::{into_cmd_err, CmdErrorCode, CmdResult};
use sfo_cmd_server::server::CmdServer;
use crate::p2p_identity::P2pId;
use crate::pn::{FromProxy, FromProxyResp, PnCmdHeader, ProxyHeart, ProxyHeartResp, ToProxy, ToProxyResp};
use crate::protocol::PackageCmdCode;
use crate::sn::types::CmdTunnelId;

pub struct PnServer<T: CmdServer<u16, u8>> {
    cmd_server: Arc<T>,
}

impl<T: CmdServer<u16, u8>> PnServer<T> {
    pub fn new(cmd_server: Arc<T>) -> Arc<Self> {
        Arc::new(Self {
            cmd_server
        })
    }

    fn register_cmd_handler(self: &Arc<Self>) {
        let this = self.clone();
        self.cmd_server.register_cmd_handler(PackageCmdCode::ToProxy as u8, move |peer_id: PeerId, _tunnel_id: CmdTunnelId, _header: PnCmdHeader, mut body: CmdBodyRead| {
            let this = this.clone();
            async move {
                let from = P2pId::from(peer_id.as_slice());
                let data = body.read_all().await?;
                let (to_proxy, buf) = ToProxy::raw_decode(data.as_slice()).map_err(into_cmd_err!(CmdErrorCode::RawCodecError))?;
                let from_proxy = FromProxy {
                    seq: to_proxy.seq,
                    from,
                    tunnel_id: to_proxy.tunnel_id,
                };

                let ret = this.cmd_server.send2(&PeerId::from(to_proxy.to.as_slice()),
                                         PackageCmdCode::FromProxy as u8,
                                         vec![from_proxy.to_vec().map_err(into_cmd_err!(CmdErrorCode::RawCodecError))?.as_slice(), buf].as_slice()).await;

                if ret.is_err() {
                    let to_proxy_resp = ToProxyResp {
                        to: to_proxy.to.clone(),
                        tunnel_id: to_proxy.tunnel_id,
                        result: 1,
                    };
                    this.cmd_server.send(&peer_id, PackageCmdCode::ToProxyResp as u8, to_proxy_resp.to_vec().map_err(into_cmd_err!(CmdErrorCode::RawCodecError))?.as_slice()).await?;
                }
                Ok(())
            }
        });

        let this = self.clone();
        self.cmd_server.register_cmd_handler(PackageCmdCode::FromProxyResp as u8, move |peer_id: PeerId, _tunnel_id: CmdTunnelId, _header: PnCmdHeader, mut body: CmdBodyRead| {
            let this = this.clone();
            async move {
                let data = body.read_all().await?;
                let from_proxy_resp = FromProxyResp::clone_from_slice(data.as_slice()).map_err(into_cmd_err!(CmdErrorCode::RawCodecError))?;
                let to_proxy_resp = ToProxyResp {
                    to: P2pId::from(peer_id.as_slice()),
                    tunnel_id: from_proxy_resp.tunnel_id,
                    result: from_proxy_resp.result,
                };
                this.cmd_server.send(&PeerId::from(from_proxy_resp.from.as_slice()),
                                     PackageCmdCode::ToProxyResp as u8,
                                     to_proxy_resp.to_vec().map_err(into_cmd_err!(CmdErrorCode::RawCodecError))?.as_slice()).await?;
                Ok(())
            }
        });

        let this = self.clone();
        self.cmd_server.register_cmd_handler(PackageCmdCode::ProxyHeart as u8, move |peer_id: PeerId, _tunnel_id: CmdTunnelId, _header: PnCmdHeader, mut body: CmdBodyRead| {
            let this = this.clone();
            async move {
                let from = P2pId::from(peer_id.as_slice());
                let data = body.read_all().await?;
                let proxy_heart= ProxyHeart::clone_from_slice(data.as_slice()).map_err(into_cmd_err!(CmdErrorCode::RawCodecError))?;
                this.cmd_server.send(&PeerId::from(proxy_heart.to.as_slice()),
                                     PackageCmdCode::ProxyHeartResp as u8,
                                     data.as_slice()).await?;
                Ok(())
            }
        });

        let this = self.clone();
        self.cmd_server.register_cmd_handler(PackageCmdCode::ProxyHeartResp as u8, move |peer_id: PeerId, _tunnel_id: CmdTunnelId, _header: PnCmdHeader, mut body: CmdBodyRead| {
            let this = this.clone();
            async move {
                let from = P2pId::from(peer_id.as_slice());
                let data = body.read_all().await?;
                let proxy_heart= ProxyHeartResp::clone_from_slice(data.as_slice()).map_err(into_cmd_err!(CmdErrorCode::RawCodecError))?;
                this.cmd_server.send(&PeerId::from(proxy_heart.to.as_slice()),
                                     PackageCmdCode::ProxyHeartResp as u8,
                                     data.as_slice()).await?;
                Ok(())
            }
        });
    }

    pub fn start(self: &Arc<Self>) {
        self.register_cmd_handler();
    }
}
