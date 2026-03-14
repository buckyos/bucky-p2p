use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, RwLock};

use tokio::sync::{Mutex as AsyncMutex, Notify};

use crate::endpoint::{Endpoint, Protocol};
use crate::error::{P2pErrorCode, P2pResult, p2p_err};
use crate::networks::{
    ListenVPortsRef, Tunnel, TunnelCommandResult, TunnelDatagramRead, TunnelDatagramWrite,
    TunnelForm, TunnelPurpose, TunnelState, TunnelStreamRead, TunnelStreamWrite,
};
use crate::p2p_identity::P2pId;
use crate::pn::{PnChannelKind, ProxyOpenReq, ProxyOpenResp};
use crate::runtime;
use crate::types::{TunnelCandidateId, TunnelId};

use super::pn_client::{PnShared, read_pn_command, write_pn_command};

const FIRST_LISTEN_WAIT_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(300);
const FIRST_LISTEN_WAIT_NOT_STARTED: u8 = 0;
const FIRST_LISTEN_WAIT_IN_PROGRESS: u8 = 1;
const FIRST_LISTEN_WAIT_DONE: u8 = 2;

struct PassivePnTunnel {
    request: ProxyOpenReq,
    read: AsyncMutex<Option<TunnelStreamRead>>,
    write: AsyncMutex<Option<TunnelStreamWrite>>,
}

enum PnTunnelRole {
    Active { network: Arc<PnShared> },
    Passive(PassivePnTunnel),
}

pub struct PnTunnel {
    tunnel_id: TunnelId,
    candidate_id: TunnelCandidateId,
    local_id: P2pId,
    remote_id: P2pId,
    role: PnTunnelRole,
    stream_vports: RwLock<Option<ListenVPortsRef>>,
    stream_first_listen_wait_state: AtomicU8,
    stream_vports_notify: Notify,
    datagram_vports: RwLock<Option<ListenVPortsRef>>,
    datagram_first_listen_wait_state: AtomicU8,
    datagram_vports_notify: Notify,
    closed: AtomicBool,
}

impl PnTunnel {
    pub(super) fn new_active(
        tunnel_id: TunnelId,
        candidate_id: TunnelCandidateId,
        local_id: P2pId,
        remote_id: P2pId,
        network: Arc<PnShared>,
    ) -> Arc<Self> {
        Arc::new(Self {
            tunnel_id,
            candidate_id,
            local_id,
            remote_id,
            role: PnTunnelRole::Active { network },
            stream_vports: RwLock::new(None),
            stream_first_listen_wait_state: AtomicU8::new(FIRST_LISTEN_WAIT_NOT_STARTED),
            stream_vports_notify: Notify::new(),
            datagram_vports: RwLock::new(None),
            datagram_first_listen_wait_state: AtomicU8::new(FIRST_LISTEN_WAIT_NOT_STARTED),
            datagram_vports_notify: Notify::new(),
            closed: AtomicBool::new(false),
        })
    }

    pub(super) fn new_passive(
        local_id: P2pId,
        request: ProxyOpenReq,
        read: TunnelStreamRead,
        write: TunnelStreamWrite,
    ) -> Arc<Self> {
        Arc::new(Self {
            tunnel_id: request.tunnel_id,
            candidate_id: TunnelCandidateId::from(request.tunnel_id.value()),
            local_id,
            remote_id: request.from.clone(),
            role: PnTunnelRole::Passive(PassivePnTunnel {
                request,
                read: AsyncMutex::new(Some(read)),
                write: AsyncMutex::new(Some(write)),
            }),
            stream_vports: RwLock::new(None),
            stream_first_listen_wait_state: AtomicU8::new(FIRST_LISTEN_WAIT_NOT_STARTED),
            stream_vports_notify: Notify::new(),
            datagram_vports: RwLock::new(None),
            datagram_first_listen_wait_state: AtomicU8::new(FIRST_LISTEN_WAIT_NOT_STARTED),
            datagram_vports_notify: Notify::new(),
            closed: AtomicBool::new(false),
        })
    }

    fn current_listen_vports(&self, kind: PnChannelKind) -> Option<ListenVPortsRef> {
        match kind {
            PnChannelKind::Stream => self.stream_vports.read().unwrap().clone(),
            PnChannelKind::Datagram => self.datagram_vports.read().unwrap().clone(),
        }
    }

    fn role_name(&self) -> &'static str {
        match &self.role {
            PnTunnelRole::Active { .. } => "active",
            PnTunnelRole::Passive(_) => "passive",
        }
    }

    async fn wait_first_listen_if_needed(&self, kind: PnChannelKind) {
        if self.current_listen_vports(kind).is_some() {
            return;
        }

        log::debug!(
            "pn passive wait first listen local={} remote={} kind={:?}",
            self.local_id,
            self.remote_id,
            kind
        );

        let (wait_state, notify) = match kind {
            PnChannelKind::Stream => (
                &self.stream_first_listen_wait_state,
                &self.stream_vports_notify,
            ),
            PnChannelKind::Datagram => (
                &self.datagram_first_listen_wait_state,
                &self.datagram_vports_notify,
            ),
        };

        loop {
            match wait_state.load(Ordering::SeqCst) {
                FIRST_LISTEN_WAIT_DONE => return,
                FIRST_LISTEN_WAIT_IN_PROGRESS => {
                    let _ = runtime::timeout(FIRST_LISTEN_WAIT_TIMEOUT, async {
                        loop {
                            if self.closed.load(Ordering::SeqCst)
                                || self.current_listen_vports(kind).is_some()
                                || wait_state.load(Ordering::SeqCst)
                                    != FIRST_LISTEN_WAIT_IN_PROGRESS
                            {
                                break;
                            }
                            notify.notified().await;
                        }
                    })
                    .await;
                    let listen_ready = self.current_listen_vports(kind).is_some();
                    log::debug!(
                        "pn passive wait first listen done local={} remote={} kind={:?} listen_ready={} closed={}",
                        self.local_id,
                        self.remote_id,
                        kind,
                        listen_ready,
                        self.closed.load(Ordering::SeqCst)
                    );
                    return;
                }
                FIRST_LISTEN_WAIT_NOT_STARTED => {
                    if wait_state
                        .compare_exchange(
                            FIRST_LISTEN_WAIT_NOT_STARTED,
                            FIRST_LISTEN_WAIT_IN_PROGRESS,
                            Ordering::SeqCst,
                            Ordering::SeqCst,
                        )
                        .is_err()
                    {
                        continue;
                    }

                    let _ = runtime::timeout(FIRST_LISTEN_WAIT_TIMEOUT, async {
                        loop {
                            if self.closed.load(Ordering::SeqCst)
                                || self.current_listen_vports(kind).is_some()
                            {
                                break;
                            }
                            notify.notified().await;
                        }
                    })
                    .await;
                    wait_state.store(FIRST_LISTEN_WAIT_DONE, Ordering::SeqCst);
                    notify.notify_waiters();
                    let listen_ready = self.current_listen_vports(kind).is_some();
                    log::debug!(
                        "pn passive first listen window finished local={} remote={} kind={:?} listen_ready={} closed={}",
                        self.local_id,
                        self.remote_id,
                        kind,
                        listen_ready,
                        self.closed.load(Ordering::SeqCst)
                    );
                    return;
                }
                _ => return,
            }
        }
    }

    async fn take_passive_channel(
        &self,
    ) -> P2pResult<(ProxyOpenReq, TunnelStreamRead, TunnelStreamWrite)> {
        let PnTunnelRole::Passive(passive) = &self.role else {
            return Err(p2p_err!(
                P2pErrorCode::NotSupport,
                "active pn tunnel cannot accept"
            ));
        };

        let mut read = passive.read.lock().await;
        let mut write = passive.write.lock().await;
        let read = read
            .take()
            .ok_or_else(|| p2p_err!(P2pErrorCode::Interrupted, "pn tunnel already consumed"))?;
        let write = write
            .take()
            .ok_or_else(|| p2p_err!(P2pErrorCode::Interrupted, "pn tunnel already consumed"))?;
        self.closed.store(true, Ordering::SeqCst);
        Ok((passive.request.clone(), read, write))
    }

    async fn accept_passive_stream(
        &self,
        expected_kind: PnChannelKind,
    ) -> P2pResult<(ProxyOpenReq, TunnelStreamRead, TunnelStreamWrite)> {
        let PnTunnelRole::Passive(passive) = &self.role else {
            return Err(p2p_err!(
                P2pErrorCode::Interrupted,
                "active pn tunnel has no accept loop"
            ));
        };

        if passive.request.kind != expected_kind {
            return Err(p2p_err!(
                P2pErrorCode::NotSupport,
                "incoming pn tunnel kind mismatch"
            ));
        }

        let (result, post_error) = if passive.request.to != self.local_id {
            (
                TunnelCommandResult::InvalidParam,
                Some(p2p_err!(
                    P2pErrorCode::InvalidParam,
                    "pn request target mismatch"
                )),
            )
        } else {
            self.wait_first_listen_if_needed(expected_kind).await;

            if let Some(listened) = self.current_listen_vports(expected_kind) {
                if listened.is_listen(&passive.request.purpose) {
                    log::debug!(
                        "pn passive accept allow local={} remote={} kind={:?} purpose={}",
                        self.local_id,
                        self.remote_id,
                        passive.request.kind,
                        passive.request.purpose
                    );
                    (TunnelCommandResult::Success, None)
                } else {
                    log::warn!(
                        "pn passive reject port-not-listen local={} remote={} kind={:?} purpose={} expected_kind={:?}",
                        self.local_id,
                        self.remote_id,
                        passive.request.kind,
                        passive.request.purpose,
                        expected_kind
                    );
                    (
                        TunnelCommandResult::PortNotListen,
                        Some(TunnelCommandResult::PortNotListen.into_p2p_error(format!(
                            "pn open rejected kind {:?} purpose {}",
                            passive.request.kind, passive.request.purpose
                        ))),
                    )
                }
            } else {
                log::warn!(
                    "pn passive reject listener-closed local={} remote={} kind={:?} purpose={} expected_kind={:?}",
                    self.local_id,
                    self.remote_id,
                    passive.request.kind,
                    passive.request.purpose,
                    expected_kind
                );
                (
                    TunnelCommandResult::ListenerClosed,
                    Some(p2p_err!(
                        P2pErrorCode::Interrupted,
                        "pn accept requires listen before accept"
                    )),
                )
            }
        };

        let (req, read, mut write) = self.take_passive_channel().await?;
        write_pn_command(
            &mut write,
            ProxyOpenResp {
                tunnel_id: req.tunnel_id,
                result: result as u8,
            },
        )
        .await?;

        if let Some(err) = post_error {
            log::debug!(
                "pn passive accept finished with error local={} remote={} kind={:?} purpose={} code={:?} msg={}",
                self.local_id,
                self.remote_id,
                req.kind,
                req.purpose,
                err.code(),
                err.msg()
            );
            return Err(err);
        }

        log::debug!(
            "pn passive accept success local={} remote={} kind={:?} purpose={}",
            self.local_id,
            self.remote_id,
            req.kind,
            req.purpose
        );
        Ok((req, read, write))
    }
}

#[async_trait::async_trait]
impl Tunnel for PnTunnel {
    fn tunnel_id(&self) -> TunnelId {
        self.tunnel_id
    }

    fn candidate_id(&self) -> TunnelCandidateId {
        self.candidate_id
    }

    fn form(&self) -> TunnelForm {
        TunnelForm::Proxy
    }

    fn is_reverse(&self) -> bool {
        false
    }

    fn protocol(&self) -> Protocol {
        Protocol::Ext(1)
    }

    fn local_id(&self) -> P2pId {
        self.local_id.clone()
    }

    fn remote_id(&self) -> P2pId {
        self.remote_id.clone()
    }

    fn local_ep(&self) -> Option<Endpoint> {
        None
    }

    fn remote_ep(&self) -> Option<Endpoint> {
        None
    }

    fn state(&self) -> TunnelState {
        if self.closed.load(Ordering::SeqCst) {
            TunnelState::Closed
        } else {
            TunnelState::Connected
        }
    }

    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    async fn close(&self) -> P2pResult<()> {
        self.closed.store(true, Ordering::SeqCst);
        if let PnTunnelRole::Passive(passive) = &self.role {
            let _ = passive.read.lock().await.take();
            let _ = passive.write.lock().await.take();
        }
        Ok(())
    }

    async fn listen_stream(&self, vports: ListenVPortsRef) -> P2pResult<()> {
        *self.stream_vports.write().unwrap() = Some(vports);
        log::debug!(
            "pn tunnel listen stream local={} remote={} role={} tunnel_id={:?}",
            self.local_id,
            self.remote_id,
            self.role_name(),
            self.tunnel_id
        );
        self.stream_vports_notify.notify_waiters();
        Ok(())
    }

    async fn listen_datagram(&self, vports: ListenVPortsRef) -> P2pResult<()> {
        *self.datagram_vports.write().unwrap() = Some(vports);
        log::debug!(
            "pn tunnel listen datagram local={} remote={} role={} tunnel_id={:?}",
            self.local_id,
            self.remote_id,
            self.role_name(),
            self.tunnel_id
        );
        self.datagram_vports_notify.notify_waiters();
        Ok(())
    }

    async fn open_stream(
        &self,
        purpose: TunnelPurpose,
    ) -> P2pResult<(TunnelStreamRead, TunnelStreamWrite)> {
        let PnTunnelRole::Active { network } = &self.role else {
            return Err(p2p_err!(
                P2pErrorCode::NotSupport,
                "incoming pn tunnel cannot open stream"
            ));
        };
        if self.closed.load(Ordering::SeqCst) {
            return Err(p2p_err!(P2pErrorCode::ErrorState, "pn tunnel closed"));
        }

        network
            .open_channel(
                self.tunnel_id,
                self.remote_id.clone(),
                PnChannelKind::Stream,
                purpose,
            )
            .await
    }

    async fn accept_stream(
        &self,
    ) -> P2pResult<(TunnelPurpose, TunnelStreamRead, TunnelStreamWrite)> {
        let (req, read, write) = self.accept_passive_stream(PnChannelKind::Stream).await?;
        Ok((req.purpose, read, write))
    }

    async fn open_datagram(&self, purpose: TunnelPurpose) -> P2pResult<TunnelDatagramWrite> {
        let PnTunnelRole::Active { network } = &self.role else {
            return Err(p2p_err!(
                P2pErrorCode::NotSupport,
                "incoming pn tunnel cannot open datagram"
            ));
        };
        if self.closed.load(Ordering::SeqCst) {
            return Err(p2p_err!(P2pErrorCode::ErrorState, "pn tunnel closed"));
        }

        let (_read, write) = network
            .open_channel(
                self.tunnel_id,
                self.remote_id.clone(),
                PnChannelKind::Datagram,
                purpose,
            )
            .await?;
        Ok(write)
    }

    async fn accept_datagram(&self) -> P2pResult<(TunnelPurpose, TunnelDatagramRead)> {
        let (req, read, _write) = self.accept_passive_stream(PnChannelKind::Datagram).await?;
        Ok((req.purpose, read))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::networks::ListenVPortRegistry;
    use crate::networks::allow_all_listen_vports;
    use crate::types::TunnelId;
    use tokio::io::{AsyncReadExt, AsyncWriteExt, split};
    use tokio::time::timeout;

    fn test_p2p_id(byte: u8) -> P2pId {
        P2pId::from(vec![byte; 32])
    }

    fn purpose_of(vport: u16) -> TunnelPurpose {
        TunnelPurpose::from_value(&vport).unwrap()
    }

    fn passive_tunnel(
        kind: PnChannelKind,
        vport: u16,
    ) -> (Arc<PnTunnel>, TunnelStreamRead, TunnelStreamWrite) {
        let local_id = test_p2p_id(1);
        let remote_id = test_p2p_id(2);
        let request = ProxyOpenReq {
            tunnel_id: TunnelId::from(42),
            from: remote_id,
            to: local_id.clone(),
            kind,
            purpose: purpose_of(vport),
        };
        let (local, remote) = tokio::io::duplex(1024);
        let (local_read, local_write) = split(local);
        let (remote_read, remote_write) = split(remote);
        (
            PnTunnel::new_passive(
                local_id,
                request,
                Box::pin(local_read),
                Box::pin(local_write),
            ),
            Box::pin(remote_read),
            Box::pin(remote_write),
        )
    }

    #[tokio::test]
    async fn passive_stream_accept_returns_channel_and_success_response() {
        let (tunnel, mut peer_read, mut peer_write) = passive_tunnel(PnChannelKind::Stream, 1001);
        tunnel
            .listen_stream(allow_all_listen_vports())
            .await
            .unwrap();

        let (purpose, mut read, mut write) = tunnel.accept_stream().await.unwrap();
        assert_eq!(purpose, purpose_of(1001));

        let resp = read_pn_command::<_, ProxyOpenResp>(&mut peer_read)
            .await
            .unwrap();
        assert_eq!(resp.tunnel_id, TunnelId::from(42));
        assert_eq!(resp.result, TunnelCommandResult::Success as u8);

        peer_write.write_all(b"ping").await.unwrap();
        let mut buf = [0u8; 4];
        read.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"ping");

        write.write_all(b"pong").await.unwrap();
        let mut buf = [0u8; 4];
        peer_read.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"pong");
    }

    #[tokio::test]
    async fn passive_datagram_accept_returns_read_and_success_response() {
        let (tunnel, mut peer_read, mut peer_write) = passive_tunnel(PnChannelKind::Datagram, 2002);
        tunnel
            .listen_datagram(allow_all_listen_vports())
            .await
            .unwrap();

        let (purpose, mut read) = tunnel.accept_datagram().await.unwrap();
        assert_eq!(purpose, purpose_of(2002));

        let resp = read_pn_command::<_, ProxyOpenResp>(&mut peer_read)
            .await
            .unwrap();
        assert_eq!(resp.tunnel_id, TunnelId::from(42));
        assert_eq!(resp.result, TunnelCommandResult::Success as u8);

        peer_write.write_all(b"data").await.unwrap();
        let mut buf = [0u8; 4];
        read.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"data");
    }

    #[tokio::test]
    async fn passive_stream_accept_rejects_unlistened_port() {
        let (tunnel, mut peer_read, _peer_write) = passive_tunnel(PnChannelKind::Stream, 3003);
        let empty_vports = ListenVPortRegistry::<()>::new();
        tunnel
            .listen_stream(empty_vports.as_listen_vports_ref())
            .await
            .unwrap();

        let err = tunnel.accept_stream().await.err().unwrap();
        assert_eq!(err.code(), P2pErrorCode::PortNotListen);

        let resp = read_pn_command::<_, ProxyOpenResp>(&mut peer_read)
            .await
            .unwrap();
        assert_eq!(resp.result, TunnelCommandResult::PortNotListen as u8);
    }

    #[tokio::test]
    async fn passive_stream_accept_requires_listen_first() {
        let (tunnel, mut peer_read, _peer_write) = passive_tunnel(PnChannelKind::Stream, 4004);

        let err = tunnel.accept_stream().await.err().unwrap();
        assert_eq!(err.code(), P2pErrorCode::Interrupted);

        let resp = read_pn_command::<_, ProxyOpenResp>(&mut peer_read)
            .await
            .unwrap();
        assert_eq!(resp.result, TunnelCommandResult::ListenerClosed as u8);
    }

    #[tokio::test]
    async fn passive_stream_accept_waits_for_late_listen() {
        let (tunnel, mut peer_read, mut peer_write) = passive_tunnel(PnChannelKind::Stream, 5005);

        let mut pending_accept = Box::pin({
            let tunnel = tunnel.clone();
            async move { tunnel.accept_stream().await }
        });

        assert!(
            timeout(FIRST_LISTEN_WAIT_TIMEOUT / 3, &mut pending_accept)
                .await
                .is_err()
        );

        tunnel
            .listen_stream(allow_all_listen_vports())
            .await
            .unwrap();

        let (purpose, mut read, mut write) = pending_accept.await.unwrap();
        assert_eq!(purpose, purpose_of(5005));

        let resp = read_pn_command::<_, ProxyOpenResp>(&mut peer_read)
            .await
            .unwrap();
        assert_eq!(resp.result, TunnelCommandResult::Success as u8);

        peer_write.write_all(b"late").await.unwrap();
        let mut buf = [0u8; 4];
        read.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"late");

        write.write_all(b"sync").await.unwrap();
        let mut buf = [0u8; 4];
        peer_read.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"sync");
    }

    #[tokio::test]
    async fn passive_stream_wait_is_independent_from_datagram_listen() {
        let (tunnel, mut peer_read, _peer_write) = passive_tunnel(PnChannelKind::Stream, 5006);

        let mut pending_accept = Box::pin({
            let tunnel = tunnel.clone();
            async move { tunnel.accept_stream().await }
        });

        tunnel
            .listen_datagram(allow_all_listen_vports())
            .await
            .unwrap();

        assert!(
            timeout(FIRST_LISTEN_WAIT_TIMEOUT / 3, &mut pending_accept)
                .await
                .is_err()
        );

        tunnel
            .listen_stream(allow_all_listen_vports())
            .await
            .unwrap();

        let (purpose, _read, _write) = pending_accept.await.unwrap();
        assert_eq!(purpose, purpose_of(5006));

        let resp = read_pn_command::<_, ProxyOpenResp>(&mut peer_read)
            .await
            .unwrap();
        assert_eq!(resp.result, TunnelCommandResult::Success as u8);
    }
}
