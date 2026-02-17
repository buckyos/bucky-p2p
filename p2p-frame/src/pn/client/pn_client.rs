use std::io::Error;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use bucky_raw_codec::{RawConvertTo, RawFrom};
use callback_result::SingleCallbackWaiter;
use sfo_cmd_server::client::{CmdClient, CmdSend, SendGuard};
use sfo_cmd_server::errors::{CmdErrorCode, into_cmd_err};
use sfo_cmd_server::{CmdBody, PeerId};
use tokio::io::ReadBuf;

use crate::error::{P2pErrorCode, P2pResult, into_p2p_err, p2p_err};
use crate::p2p_connection::{P2pConnection, P2pReadHalf, P2pWriteHalf};
use crate::p2p_identity::{P2pId, P2pIdentityCertFactoryRef, P2pIdentityRef};
use crate::pn::{
    PN_DATA_FRAME_PROXY_OPEN_READY, PN_DATA_FRAME_PROXY_OPEN_REQ, PN_DATA_FRAME_PROXY_OPEN_RESP,
    PnClient, PnCmdHeader, PnTunnelRead, PnTunnelWrite, ProxyOpenNotify, ProxyOpenReady,
    ProxyOpenReq, ProxyOpenResp,
};
use crate::protocol::PackageCmdCode;
use crate::runtime;
use crate::sn::client::SNClientServiceRef;
use crate::sn::types::CmdTunnelId;
use crate::sockets::NetManagerRef;
use crate::types::TunnelId;

const PN_DATA_MAGIC: [u8; 4] = *b"PN2D";
const PN_OPEN_TIMEOUT: Duration = Duration::from_secs(5);
const PN_CONTROL_MAX_LEN: usize = 64 * 1024;

struct DefaultPnTunnelRead {
    p2p_id: P2pId,
    remote_name: String,
    tunnel_id: TunnelId,
    read: P2pReadHalf,
}

impl DefaultPnTunnelRead {
    fn new(p2p_id: P2pId, remote_name: String, tunnel_id: TunnelId, read: P2pReadHalf) -> Self {
        Self {
            p2p_id,
            remote_name,
            tunnel_id,
            read,
        }
    }
}

impl PnTunnelRead for DefaultPnTunnelRead {
    fn tunnel_id(&self) -> TunnelId {
        self.tunnel_id
    }

    fn remote_id(&self) -> P2pId {
        self.p2p_id.clone()
    }

    fn remote_name(&self) -> String {
        self.remote_name.clone()
    }
}

impl runtime::AsyncRead for DefaultPnTunnelRead {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(self.read.as_mut()).poll_read(cx, buf)
    }
}

struct DefaultPnTunnelWrite {
    tunnel_id: TunnelId,
    remote_id: P2pId,
    remote_name: String,
    write: P2pWriteHalf,
}

impl DefaultPnTunnelWrite {
    fn new(
        tunnel_id: TunnelId,
        remote_id: P2pId,
        remote_name: String,
        write: P2pWriteHalf,
    ) -> Self {
        Self {
            tunnel_id,
            remote_id,
            remote_name,
            write,
        }
    }
}

impl PnTunnelWrite for DefaultPnTunnelWrite {
    fn tunnel_id(&self) -> TunnelId {
        self.tunnel_id
    }

    fn remote_id(&self) -> P2pId {
        self.remote_id.clone()
    }

    fn remote_name(&self) -> String {
        self.remote_name.clone()
    }
}

impl runtime::AsyncWrite for DefaultPnTunnelWrite {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, Error>> {
        Pin::new(self.write.as_mut()).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        Pin::new(self.write.as_mut()).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        Pin::new(self.write.as_mut()).poll_shutdown(cx)
    }
}

struct DefaultPnClientImpl<
    CS: CmdSend<()> + Unpin,
    S: SendGuard<(), CS> + Unpin,
    T: CmdClient<u16, u8, (), CS, S>,
> {
    cmd_client: Arc<T>,
    accept_waiter: Arc<
        SingleCallbackWaiter<
            P2pResult<(
                Box<dyn crate::pn::PnTunnelRead>,
                Box<dyn crate::pn::PnTunnelWrite>,
            )>,
        >,
    >,
    local_identity: P2pIdentityRef,
    net_manager: NetManagerRef,
    sn_service: SNClientServiceRef,
    version: u8,
    _p: PhantomData<Arc<tokio::sync::Mutex<(CS, S)>>>,
}

pub struct DefaultPnClient<
    CS: CmdSend<()> + Unpin,
    S: SendGuard<(), CS> + Unpin,
    T: CmdClient<u16, u8, (), CS, S>,
> {
    inner: Arc<DefaultPnClientImpl<CS, S, T>>,
}

impl<CS: CmdSend<()> + Unpin, S: SendGuard<(), CS> + Unpin, T: CmdClient<u16, u8, (), CS, S>> Clone
    for DefaultPnClient<CS, S, T>
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<CS: CmdSend<()> + Unpin, S: SendGuard<(), CS> + Unpin, T: CmdClient<u16, u8, (), CS, S>>
    DefaultPnClient<CS, S, T>
{
    pub fn new(
        cmd_client: Arc<T>,
        local_identity: P2pIdentityRef,
        net_manager: NetManagerRef,
        sn_service: SNClientServiceRef,
    ) -> Arc<Self> {
        let this = Arc::new(Self {
            inner: Arc::new(DefaultPnClientImpl {
                cmd_client,
                accept_waiter: Arc::new(SingleCallbackWaiter::new()),
                local_identity,
                net_manager,
                sn_service,
                version: 0,
                _p: Default::default(),
            }),
        });
        this.register_cmd_handler();
        this
    }

    async fn write_control_frame(
        write: &mut P2pWriteHalf,
        frame_type: u8,
        body: &[u8],
    ) -> P2pResult<()> {
        runtime::AsyncWriteExt::write_u8(write.as_mut(), frame_type)
            .await
            .map_err(into_p2p_err!(P2pErrorCode::IoError))?;
        runtime::AsyncWriteExt::write_u32_le(write.as_mut(), body.len() as u32)
            .await
            .map_err(into_p2p_err!(P2pErrorCode::IoError))?;
        runtime::AsyncWriteExt::write_all(write.as_mut(), body)
            .await
            .map_err(into_p2p_err!(P2pErrorCode::IoError))?;
        runtime::AsyncWriteExt::flush(write.as_mut())
            .await
            .map_err(into_p2p_err!(P2pErrorCode::IoError))?;
        Ok(())
    }

    async fn read_control_frame(read: &mut P2pReadHalf) -> P2pResult<(u8, Vec<u8>)> {
        let frame_type = runtime::AsyncReadExt::read_u8(read.as_mut())
            .await
            .map_err(into_p2p_err!(P2pErrorCode::IoError))?;
        let len = runtime::AsyncReadExt::read_u32_le(read.as_mut())
            .await
            .map_err(into_p2p_err!(P2pErrorCode::IoError))? as usize;
        if len == 0 || len > PN_CONTROL_MAX_LEN {
            return Err(p2p_err!(
                P2pErrorCode::OutOfLimit,
                "invalid pn control frame len {}",
                len
            ));
        }
        let mut body = vec![0u8; len];
        runtime::AsyncReadExt::read_exact(read.as_mut(), body.as_mut_slice())
            .await
            .map_err(into_p2p_err!(P2pErrorCode::IoError))?;
        Ok((frame_type, body))
    }

    fn register_cmd_handler(self: &Arc<Self>) {
        let this = self.clone();
        self.inner.cmd_client.register_cmd_handler(
            PackageCmdCode::ProxyOpenNotify as u8,
            move |_peer_id: PeerId,
                  _tunnel_id: CmdTunnelId,
                  _header: PnCmdHeader,
                  mut body: CmdBody| {
                let this = this.clone();
                async move {
                    let notify =
                        ProxyOpenNotify::clone_from_slice(body.read_all().await?.as_slice())
                            .map_err(into_cmd_err!(CmdErrorCode::RawCodecError))?;
                    if let Err(e) = this.on_proxy_open_notify(notify).await {
                        log::warn!("handle ProxyOpenNotify failed: {:?}", e);
                    }
                    Ok(None)
                }
            },
        );
    }

    async fn on_proxy_open_notify(&self, notify: ProxyOpenNotify) -> P2pResult<()> {
        let conn = self.create_data_connection().await?;
        let (read, mut write) = conn.split();
        let ready = ProxyOpenReady {
            tunnel_id: notify.tunnel_id,
            from: notify.from.clone(),
            to: self.inner.local_identity.get_id(),
        };
        let body = ready
            .to_vec()
            .map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
        Self::write_control_frame(&mut write, PN_DATA_FRAME_PROXY_OPEN_READY, body.as_slice())
            .await?;

        let tunnel_read = DefaultPnTunnelRead::new(
            notify.from.clone(),
            notify.from.to_string(),
            notify.tunnel_id,
            read,
        );
        let tunnel_write = DefaultPnTunnelWrite::new(
            notify.tunnel_id,
            notify.from.clone(),
            notify.from.to_string(),
            write,
        );

        self.inner
            .accept_waiter
            .set_result_with_cache(Ok((Box::new(tunnel_read), Box::new(tunnel_write))));
        Ok(())
    }

    async fn create_data_connection(&self) -> P2pResult<P2pConnection> {
        let active_sn = self.inner.sn_service.get_active_sn_list();
        let sn_peer_id = active_sn
            .first()
            .map(|v| v.sn_peer_id.clone())
            .ok_or_else(|| {
                p2p_err!(
                    P2pErrorCode::NotFound,
                    "no active sn for pn data connection"
                )
            })?;

        let mut endpoints = Vec::new();
        for sn in self.inner.sn_service.get_sn_list().iter() {
            if sn.get_id() == sn_peer_id {
                endpoints = sn.endpoints();
                break;
            }
        }
        if endpoints.is_empty() {
            return Err(p2p_err!(
                P2pErrorCode::NotFound,
                "no sn endpoint for {}",
                sn_peer_id
            ));
        }

        let mut last_err = None;
        for ep in endpoints.iter() {
            let network = match self.inner.net_manager.get_network(ep.protocol()) {
                Ok(network) => network,
                Err(e) => {
                    last_err = Some(e);
                    continue;
                }
            };

            match network
                .create_stream_connect(
                    &self.inner.local_identity,
                    ep,
                    &sn_peer_id,
                    Some(sn_peer_id.to_string()),
                )
                .await
            {
                Ok(mut conns) => {
                    if conns.is_empty() {
                        continue;
                    }
                    let mut conn = conns.remove(0);
                    if runtime::AsyncWriteExt::write_all(&mut conn, &PN_DATA_MAGIC)
                        .await
                        .is_err()
                    {
                        continue;
                    }
                    return Ok(conn);
                }
                Err(e) => {
                    last_err = Some(e);
                }
            }
        }

        Err(last_err
            .unwrap_or_else(|| p2p_err!(P2pErrorCode::ConnectFailed, "connect pn data failed")))
    }
}

#[async_trait::async_trait]
impl<CS: CmdSend<()> + Unpin, S: SendGuard<(), CS> + Unpin, T: CmdClient<u16, u8, (), CS, S>>
    PnClient for DefaultPnClient<CS, S, T>
{
    async fn accept(&self) -> P2pResult<(Box<dyn PnTunnelRead>, Box<dyn PnTunnelWrite>)> {
        self.inner
            .accept_waiter
            .create_result_future()
            .map_err(into_p2p_err!(P2pErrorCode::Failed))?
            .await
            .map_err(into_p2p_err!(P2pErrorCode::IoError))?
    }

    async fn connect(
        &self,
        tunnel_id: TunnelId,
        to: P2pId,
        to_name: Option<String>,
    ) -> P2pResult<(Box<dyn PnTunnelRead>, Box<dyn PnTunnelWrite>)> {
        let conn = self.create_data_connection().await?;
        let (mut read, mut write) = conn.split();

        let open_req = ProxyOpenReq {
            tunnel_id,
            to: to.clone(),
        };
        let open_req_body = open_req
            .to_vec()
            .map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
        Self::write_control_frame(
            &mut write,
            PN_DATA_FRAME_PROXY_OPEN_REQ,
            open_req_body.as_slice(),
        )
        .await?;

        let (frame_type, body) =
            runtime::timeout(PN_OPEN_TIMEOUT, Self::read_control_frame(&mut read))
                .await
                .map_err(into_p2p_err!(P2pErrorCode::Timeout, "pn open timeout"))??;
        if frame_type != PN_DATA_FRAME_PROXY_OPEN_RESP {
            return Err(p2p_err!(
                P2pErrorCode::InvalidParam,
                "invalid pn open response frame type {}",
                frame_type
            ));
        }

        let resp = ProxyOpenResp::clone_from_slice(body.as_slice())
            .map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
        if resp.tunnel_id != tunnel_id || resp.result != 0 {
            return Err(p2p_err!(P2pErrorCode::ConnectFailed, "pn open failed"));
        }

        let remote_name = to_name.unwrap_or_else(|| to.to_string());
        let tunnel_read =
            DefaultPnTunnelRead::new(to.clone(), remote_name.clone(), tunnel_id, read);
        let tunnel_write = DefaultPnTunnelWrite::new(tunnel_id, to.clone(), remote_name, write);
        Ok((Box::new(tunnel_read), Box::new(tunnel_write)))
    }
}

impl<CS: CmdSend<()> + Unpin, S: SendGuard<(), CS> + Unpin, T: CmdClient<u16, u8, (), CS, S>> Drop
    for DefaultPnClientImpl<CS, S, T>
{
    fn drop(&mut self) {
        log::info!("drop DefaultPnClient");
    }
}
