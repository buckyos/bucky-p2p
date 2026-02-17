use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bucky_raw_codec::{RawConvertTo, RawFrom};
use callback_result::SingleCallbackWaiter;
use notify_future::Notify;
use sfo_cmd_server::PeerId;
use sfo_cmd_server::server::CmdServer;
use tokio::io::copy_bidirectional;

use crate::error::{P2pErrorCode, P2pResult, into_p2p_err, p2p_err};
use crate::p2p_connection::{P2pConnection, P2pReadHalf, P2pWriteHalf};
use crate::p2p_identity::P2pId;
use crate::pn::{
    PN_DATA_FRAME_PROXY_OPEN_READY, PN_DATA_FRAME_PROXY_OPEN_REQ, PN_DATA_FRAME_PROXY_OPEN_RESP,
    ProxyOpenNotify, ProxyOpenReady, ProxyOpenReq, ProxyOpenResp,
};
use crate::protocol::PackageCmdCode;
use crate::runtime;
use crate::types::TunnelId;

const PN_CONTROL_MAX_LEN: usize = 64 * 1024;
const PN_OPEN_TIMEOUT: Duration = Duration::from_millis(1500);

struct PendingOpen {
    to: P2pId,
    waiter: Notify<P2pResult<P2pConnection>>,
}

pub struct PnServer<T: CmdServer<u16, u8>> {
    cmd_server: Arc<T>,
    data_waiter: Arc<SingleCallbackWaiter<P2pConnection>>,
    pending_open: Arc<Mutex<HashMap<(P2pId, TunnelId), PendingOpen>>>,
    cmd_version: u8,
}

impl<T: CmdServer<u16, u8>> PnServer<T> {
    pub fn new(
        cmd_server: Arc<T>,
        data_waiter: Arc<SingleCallbackWaiter<P2pConnection>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            cmd_server,
            data_waiter,
            pending_open: Arc::new(Mutex::new(HashMap::new())),
            cmd_version: 0,
        })
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

    async fn handle_proxy_open_req(
        self: &Arc<Self>,
        from: P2pId,
        req: ProxyOpenReq,
        mut read: P2pReadHalf,
        mut write: P2pWriteHalf,
    ) {
        let (notify, waiter) = Notify::<P2pResult<P2pConnection>>::new();
        {
            let mut pending = self.pending_open.lock().unwrap();
            pending.insert(
                (from.clone(), req.tunnel_id),
                PendingOpen {
                    to: req.to.clone(),
                    waiter: notify,
                },
            );
        }

        let notify = ProxyOpenNotify {
            tunnel_id: req.tunnel_id,
            from: from.clone(),
        };
        let notify_result = match notify
            .to_vec()
            .map_err(into_p2p_err!(P2pErrorCode::RawCodecError))
        {
            Ok(body) => self
                .cmd_server
                .send(
                    &PeerId::from(req.to.as_slice()),
                    PackageCmdCode::ProxyOpenNotify as u8,
                    self.cmd_version,
                    body.as_slice(),
                )
                .await
                .map_err(into_p2p_err!(P2pErrorCode::IoError)),
            Err(e) => Err(e),
        };

        let open_result: P2pResult<P2pConnection> = if notify_result.is_ok() {
            runtime::timeout(PN_OPEN_TIMEOUT, waiter)
                .await
                .map_err(into_p2p_err!(P2pErrorCode::Timeout, "pn open timeout"))
                .and_then(|ret| ret)
        } else {
            Err(notify_result.err().unwrap())
        };

        {
            let mut pending = self.pending_open.lock().unwrap();
            pending.remove(&(from.clone(), req.tunnel_id));
        }

        let resp = ProxyOpenResp {
            tunnel_id: req.tunnel_id,
            result: if open_result.is_ok() { 0 } else { 1 },
        };
        if let Ok(body) = resp.to_vec() {
            let _ = Self::write_control_frame(
                &mut write,
                PN_DATA_FRAME_PROXY_OPEN_RESP,
                body.as_slice(),
            )
            .await;
        }

        if let Ok(mut other_conn) = open_result {
            let mut conn = read.unsplit(write);
            let _ = copy_bidirectional(&mut conn, &mut other_conn).await;
        }
    }

    async fn handle_proxy_open_ready(
        self: &Arc<Self>,
        peer: P2pId,
        ready: ProxyOpenReady,
        read: P2pReadHalf,
        write: P2pWriteHalf,
    ) {
        let waiter = {
            let mut pending = self.pending_open.lock().unwrap();
            if let Some(ctx) = pending.remove(&(ready.from.clone(), ready.tunnel_id)) {
                if ctx.to == peer && ready.to == peer {
                    Some(ctx.waiter)
                } else {
                    None
                }
            } else {
                None
            }
        };

        if let Some(waiter) = waiter {
            let conn = read.unsplit(write);
            waiter.notify(Ok(conn));
        }
    }

    async fn handle_data_connection(self: &Arc<Self>, conn: P2pConnection) {
        let (mut read, write) = conn.split();
        let from = read.remote_id();

        let first = Self::read_control_frame(&mut read).await;
        let (frame_type, body) = match first {
            Ok(v) => v,
            Err(e) => {
                log::warn!("read pn control frame failed from {}: {:?}", from, e);
                return;
            }
        };

        match frame_type {
            PN_DATA_FRAME_PROXY_OPEN_REQ => {
                if let Ok(req) = ProxyOpenReq::clone_from_slice(body.as_slice()) {
                    self.handle_proxy_open_req(from, req, read, write).await;
                }
            }
            PN_DATA_FRAME_PROXY_OPEN_READY => {
                if let Ok(ready) = ProxyOpenReady::clone_from_slice(body.as_slice()) {
                    self.handle_proxy_open_ready(from, ready, read, write).await;
                }
            }
            _ => {}
        }
    }

    fn start_data_accept_loop(self: &Arc<Self>) {
        let this = self.clone();
        crate::executor::Executor::spawn(async move {
            loop {
                let conn = match this.data_waiter.create_result_future() {
                    Ok(f) => f.await,
                    Err(e) => {
                        log::error!("pn data waiter error: {:?}", e);
                        runtime::sleep(std::time::Duration::from_millis(200)).await;
                        continue;
                    }
                };

                let conn = match conn {
                    Ok(c) => c,
                    Err(e) => {
                        log::error!("pn data accept error: {:?}", e);
                        continue;
                    }
                };

                let this_for_conn = this.clone();
                crate::executor::Executor::spawn(async move {
                    this_for_conn.handle_data_connection(conn).await;
                });
            }
        });
    }

    pub fn start(self: &Arc<Self>) {
        self.start_data_accept_loop();
    }
}
