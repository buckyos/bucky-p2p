use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::io::{ReadBuf, copy_bidirectional};
use tokio::sync::{Mutex as AsyncMutex, mpsc};

use crate::error::{P2pError, P2pErrorCode, P2pResult, into_p2p_err, p2p_err};
use crate::networks::{
    TunnelCommand, TunnelCommandBody, TunnelCommandResult, TunnelStreamRead, TunnelStreamWrite,
    read_tunnel_command_body, read_tunnel_command_header, write_tunnel_command,
};
use crate::p2p_identity::P2pId;
use crate::pn::{PN_PROXY_VPORT, ProxyOpenReq, ProxyOpenResp};
use crate::runtime;
use crate::ttp::TtpServerRef;

const PN_OPEN_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) struct ProxyStream {
    remote_id: P2pId,
    read: TunnelStreamRead,
    write: TunnelStreamWrite,
}

impl ProxyStream {
    pub(crate) fn new(remote_id: P2pId, read: TunnelStreamRead, write: TunnelStreamWrite) -> Self {
        Self {
            remote_id,
            read,
            write,
        }
    }
}

impl tokio::io::AsyncRead for ProxyStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        self.read.as_mut().poll_read(cx, buf)
    }
}

impl tokio::io::AsyncWrite for ProxyStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.write.as_mut().poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.write.as_mut().poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.write.as_mut().poll_shutdown(cx)
    }
}

pub struct PnServer {
    ttp_server: TtpServerRef,
    data_rx: AsyncMutex<mpsc::UnboundedReceiver<ProxyStream>>,
}

impl PnServer {
    pub(crate) fn new(
        ttp_server: TtpServerRef,
        data_rx: mpsc::UnboundedReceiver<ProxyStream>,
    ) -> Arc<Self> {
        Arc::new(Self {
            ttp_server,
            data_rx: AsyncMutex::new(data_rx),
        })
    }

    async fn handle_proxy_open_req(
        self: &Arc<Self>,
        from: P2pId,
        mut req: ProxyOpenReq,
        read: TunnelStreamRead,
        write: TunnelStreamWrite,
    ) {
        req.from = from.clone();
        log::debug!(
            "pn server open recv tunnel_id={:?} from={} to={} kind={:?} requested_vport={} proxy_vport={}",
            req.tunnel_id,
            req.from,
            req.to,
            req.kind,
            req.vport,
            PN_PROXY_VPORT
        );

        let mut source_write = write;
        let open_result = runtime::timeout(
            PN_OPEN_TIMEOUT,
            self.ttp_server
                .open_stream_by_id(&req.to, Some(req.to.to_string()), PN_PROXY_VPORT),
        )
        .await
        .map_err(into_p2p_err!(P2pErrorCode::Timeout, "pn open timeout"))
        .and_then(|ret| ret);

        let open_result = match open_result {
            Ok((_meta, mut target_read, mut target_write)) => {
                log::debug!(
                    "pn server open upstream connected tunnel_id={:?} target={} requested_vport={} proxy_vport={}",
                    req.tunnel_id,
                    req.to,
                    req.vport,
                    PN_PROXY_VPORT
                );
                let bridge_ready = async {
                    write_proxy_command(&mut target_write, req.clone()).await?;
                    let resp = runtime::timeout(
                        PN_OPEN_TIMEOUT,
                        read_proxy_command::<_, ProxyOpenResp>(&mut target_read),
                    )
                    .await
                    .map_err(into_p2p_err!(P2pErrorCode::Timeout, "pn open timeout"))??;
                    if resp.tunnel_id != req.tunnel_id {
                        return Err(p2p_err!(
                            P2pErrorCode::InvalidData,
                            "pn open response tunnel id mismatch"
                        ));
                    }
                    let result = TunnelCommandResult::from_u8(resp.result).ok_or_else(|| {
                        p2p_err!(
                            P2pErrorCode::InvalidData,
                            "invalid pn open result {}",
                            resp.result
                        )
                    })?;
                    Ok((result, target_read, target_write))
                }
                .await;

                match bridge_ready {
                    Ok((result, target_read, target_write)) => {
                        log::debug!(
                            "pn server open upstream resp tunnel_id={:?} target={} kind={:?} requested_vport={} proxy_vport={} result={:?}",
                            req.tunnel_id,
                            req.to,
                            req.kind,
                            req.vport,
                            PN_PROXY_VPORT,
                            result
                        );
                        let _ = write_proxy_command(
                            &mut source_write,
                            ProxyOpenResp {
                                tunnel_id: req.tunnel_id,
                                result: result as u8,
                            },
                        )
                        .await;
                        if result == TunnelCommandResult::Success {
                            Ok((target_read, target_write))
                        } else {
                            Err(result.into_p2p_error(format!(
                                "pn open rejected kind {:?} vport {}",
                                req.kind, req.vport
                            )))
                        }
                    }
                    Err(err) => {
                        log::warn!(
                            "pn server open upstream failed tunnel_id={:?} target={} kind={:?} requested_vport={} proxy_vport={} code={:?} msg={}",
                            req.tunnel_id,
                            req.to,
                            req.kind,
                            req.vport,
                            PN_PROXY_VPORT,
                            err.code(),
                            err.msg()
                        );
                        let _ = write_proxy_command(
                            &mut source_write,
                            ProxyOpenResp {
                                tunnel_id: req.tunnel_id,
                                result: result_from_error(&err) as u8,
                            },
                        )
                        .await;
                        Err(err)
                    }
                }
            }
            Err(err) => {
                log::warn!(
                    "pn server open target failed tunnel_id={:?} from={} to={} kind={:?} requested_vport={} proxy_vport={} code={:?} msg={}",
                    req.tunnel_id,
                    req.from,
                    req.to,
                    req.kind,
                    req.vport,
                    PN_PROXY_VPORT,
                    err.code(),
                    err.msg()
                );
                let _ = write_proxy_command(
                    &mut source_write,
                    ProxyOpenResp {
                        tunnel_id: req.tunnel_id,
                        result: result_from_error(&err) as u8,
                    },
                )
                .await;
                Err(err)
            }
        };

        if let Ok((target_read, target_write)) = open_result {
            log::debug!(
                "pn server bridge start tunnel_id={:?} from={} to={} kind={:?} requested_vport={} proxy_vport={}",
                req.tunnel_id,
                from,
                req.to,
                req.kind,
                req.vport,
                PN_PROXY_VPORT
            );
            let mut source_stream = ProxyStream::new(from, read, source_write);
            let mut target_stream = ProxyStream::new(req.to, target_read, target_write);
            let _ = copy_bidirectional(&mut source_stream, &mut target_stream).await;
        }
    }

    async fn handle_data_connection(self: &Arc<Self>, conn: ProxyStream) {
        let from = conn.remote_id.clone();
        let ProxyStream {
            mut read, write, ..
        } = conn;

        let header = match read_tunnel_command_header(&mut read).await {
            Ok(v) => v,
            Err(e) => {
                log::warn!("read pn control frame failed from {}: {:?}", from, e);
                return;
            }
        };

        if header.command_id == ProxyOpenReq::COMMAND_ID {
            let command_id = header.command_id;
            let data_len = header.data_len;
            if let Ok(req) = read_tunnel_command_body::<_, ProxyOpenReq>(&mut read, header).await {
                log::debug!(
                    "pn server data connection control frame from={} command_id={} data_len={}",
                    from,
                    command_id,
                    data_len
                );
                self.handle_proxy_open_req(from, req.body, read, write)
                    .await;
            }
        }
    }

    fn start_data_accept_loop(self: &Arc<Self>) {
        let this = self.clone();
        crate::executor::Executor::spawn(async move {
            loop {
                let conn = {
                    let mut rx = this.data_rx.lock().await;
                    rx.recv().await
                };
                let Some(conn) = conn else {
                    break;
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

fn result_from_error(err: &P2pError) -> TunnelCommandResult {
    match err.code() {
        P2pErrorCode::PortNotListen => TunnelCommandResult::PortNotListen,
        P2pErrorCode::Timeout => TunnelCommandResult::Timeout,
        P2pErrorCode::Interrupted | P2pErrorCode::NotFound => TunnelCommandResult::Interrupted,
        P2pErrorCode::InvalidParam => TunnelCommandResult::InvalidParam,
        P2pErrorCode::InvalidData => TunnelCommandResult::ProtocolError,
        _ => TunnelCommandResult::InternalError,
    }
}

async fn write_proxy_command<T>(write: &mut TunnelStreamWrite, body: T) -> P2pResult<()>
where
    T: TunnelCommandBody,
{
    let command = TunnelCommand::new(body)?;
    write_tunnel_command(write, &command).await
}

async fn read_proxy_command<R, T>(read: &mut R) -> P2pResult<T>
where
    R: runtime::AsyncRead + Unpin,
    T: TunnelCommandBody,
{
    let header = read_tunnel_command_header(read).await?;
    let command = read_tunnel_command_body::<_, T>(read, header).await?;
    Ok(command.body)
}
