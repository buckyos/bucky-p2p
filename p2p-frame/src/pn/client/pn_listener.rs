use std::sync::Arc;

use crate::error::P2pResult;
use crate::networks::{
    Tunnel, TunnelCommandBody, TunnelCommandResult, TunnelRef, read_tunnel_command_body,
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

impl PnListener {
    pub(super) async fn accept_tunnel(&self) -> P2pResult<TunnelRef> {
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
                match self.shared.register_passive_control_tunnel(req.clone()) {
                    Ok(tunnel) => {
                        tunnel.set_control_channel(read, write).await;
                        if let Err(err) = tunnel
                            .send_control_open_response(TunnelCommandResult::Success)
                            .await
                        {
                            let _ = tunnel.close().await;
                            return Err(err);
                        }
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
            match self.shared.dispatch_passive_channel(key, req, read, write) {
                PassiveTunnelDispatch::Dispatched => continue,
                PassiveTunnelDispatch::Rejected(rejected) => {
                    let mut write = rejected.write;
                    let _ = write_pn_command(
                        &mut write,
                        crate::pn::ProxyOpenResp {
                            tunnel_id: rejected.request.tunnel_id,
                            result: TunnelCommandResult::InvalidParam as u8,
                        },
                    )
                    .await;
                    log::warn!(
                        "pn listener rejected business open without live control local={} from={} to={} kind={:?} tunnel_id={:?} code={:?} msg={}",
                        self.shared.local_id(),
                        rejected.request.from,
                        rejected.request.to,
                        rejected.request.kind,
                        rejected.request.tunnel_id,
                        rejected.error.code(),
                        rejected.error.msg()
                    );
                    continue;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::endpoint::{Endpoint, Protocol};
    use crate::error::{P2pErrorCode, p2p_err};
    use crate::networks::{TunnelPurpose, TunnelStreamRead, TunnelStreamWrite};
    use crate::p2p_identity::{EncodedP2pIdentity, P2pIdentity, P2pIdentityRef, P2pSignature};
    use crate::pn::client::pn_client::read_pn_command;
    use crate::pn::{PROXY_SERVICE, PnChannelKind, ProxyOpenResp};
    use crate::ttp::{TtpListener, TtpStreamMeta};
    use tokio::io::split;
    use tokio::sync::{Mutex as AsyncMutex, mpsc};
    use tokio::time::{Duration, timeout};

    const TEST_CHANNEL_CAPACITY: usize = 8;

    struct FakeTtpListener {
        rx: AsyncMutex<
            mpsc::Receiver<
                P2pResult<(TtpStreamMeta, TunnelStreamRead, TunnelStreamWrite)>,
            >,
        >,
    }

    #[derive(Clone)]
    struct FakeIdentity {
        id: crate::p2p_identity::P2pId,
    }

    impl FakeIdentity {
        fn new(byte: u8) -> Self {
            Self { id: p2p_id(byte) }
        }
    }

    impl P2pIdentity for FakeIdentity {
        fn get_identity_cert(&self) -> P2pResult<crate::p2p_identity::P2pIdentityCertRef> {
            Err(p2p_err!(P2pErrorCode::NotSupport, "no cert"))
        }

        fn get_id(&self) -> crate::p2p_identity::P2pId {
            self.id.clone()
        }

        fn get_name(&self) -> String {
            self.id.to_string()
        }

        fn sign_type(&self) -> crate::p2p_identity::P2pIdentitySignType {
            crate::p2p_identity::P2pIdentitySignType::Ed25519
        }

        fn sign(&self, _message: &[u8]) -> P2pResult<P2pSignature> {
            Ok(vec![1; 64])
        }

        fn get_encoded_identity(&self) -> P2pResult<EncodedP2pIdentity> {
            Ok(self.id.as_slice().to_vec())
        }

        fn endpoints(&self) -> Vec<Endpoint> {
            vec![]
        }

        fn update_endpoints(&self, _eps: Vec<Endpoint>) -> P2pIdentityRef {
            Arc::new(self.clone())
        }
    }

    #[async_trait::async_trait]
    impl TtpListener for FakeTtpListener {
        async fn accept(&self) -> P2pResult<(TtpStreamMeta, TunnelStreamRead, TunnelStreamWrite)> {
            self.rx
                .lock()
                .await
                .recv()
                .await
                .ok_or_else(|| p2p_err!(P2pErrorCode::Interrupted, "fake ttp listener closed"))?
        }
    }

    fn p2p_id(byte: u8) -> crate::p2p_identity::P2pId {
        crate::p2p_identity::P2pId::from(vec![byte; 32])
    }

    fn purpose(vport: u16) -> TunnelPurpose {
        TunnelPurpose::from_value(&vport.to_string()).unwrap()
    }

    fn proxy_purpose() -> TunnelPurpose {
        TunnelPurpose::from_value(&PROXY_SERVICE.to_string()).unwrap()
    }

    fn meta(
        local_id: crate::p2p_identity::P2pId,
        remote_id: crate::p2p_identity::P2pId,
    ) -> TtpStreamMeta {
        TtpStreamMeta {
            local_ep: None,
            remote_ep: Some(Endpoint::from((
                Protocol::Ext(1),
                "127.0.0.1:1".parse().unwrap(),
            ))),
            local_id,
            remote_id,
            remote_name: None,
            purpose: proxy_purpose(),
        }
    }

    fn make_listener(
        shared: Arc<PnShared>,
    ) -> (
        Arc<PnListener>,
        mpsc::Sender<P2pResult<(TtpStreamMeta, TunnelStreamRead, TunnelStreamWrite)>>,
    ) {
        let (tx, rx) = mpsc::channel(TEST_CHANNEL_CAPACITY);
        let ttp_listener = Arc::new(FakeTtpListener {
            rx: AsyncMutex::new(rx),
        });
        (Arc::new(PnListener::new(shared, ttp_listener)), tx)
    }

    fn make_stream_pair() -> (
        TunnelStreamRead,
        TunnelStreamWrite,
        TunnelStreamRead,
        TunnelStreamWrite,
    ) {
        let (listener_side, peer_side) = tokio::io::duplex(2048);
        let (listener_read, listener_write) = split(listener_side);
        let (peer_read, peer_write) = split(peer_side);
        (
            Box::pin(listener_read),
            Box::pin(listener_write),
            Box::pin(peer_read),
            Box::pin(peer_write),
        )
    }

    fn send_ttp_stream(
        tx: &mpsc::Sender<P2pResult<(TtpStreamMeta, TunnelStreamRead, TunnelStreamWrite)>>,
        meta: TtpStreamMeta,
        read: TunnelStreamRead,
        write: TunnelStreamWrite,
    ) -> P2pResult<()> {
        tx.try_send(Ok((meta, read, write))).map_err(|err| {
            p2p_err!(P2pErrorCode::OutOfLimit, "test ttp stream queue failed: {}", err)
        })
    }

    #[tokio::test]
    async fn pn_tunnel_control_open_creates_passive_tunnel() {
        let local_identity: P2pIdentityRef = Arc::new(FakeIdentity::new(91));
        let local_id = local_identity.get_id();
        let remote_id = p2p_id(92);
        let shared = PnShared::new_for_test(local_identity);
        let (listener, tx) = make_listener(shared.clone());
        let (listener_read, listener_write, mut peer_read, mut peer_write) = make_stream_pair();
        let tunnel_id = crate::types::TunnelId::from(91);

        send_ttp_stream(
            &tx,
            meta(local_id.clone(), remote_id.clone()),
            listener_read,
            listener_write,
        )
        .unwrap();
        write_pn_command(
            &mut peer_write,
            ProxyControlOpenReq {
                tunnel_id,
                from: remote_id.clone(),
                to: local_id.clone(),
            },
        )
        .await
        .unwrap();

        let accept_task = tokio::spawn({
            let listener = listener.clone();
            async move { listener.accept_tunnel().await }
        });

        let resp = read_pn_command::<_, ProxyControlOpenResp>(&mut peer_read)
            .await
            .unwrap();
        assert_eq!(resp.result, TunnelCommandResult::Success as u8);

        let key = PnShared::tunnel_key(remote_id.clone(), tunnel_id);
        let registered = shared.get_tunnel(&key).unwrap();
        assert_eq!(registered.tunnel_id(), tunnel_id);
        assert_eq!(registered.state(), crate::networks::TunnelState::Connected);
        assert_eq!(registered.lifecycle_counts_for_test(), (0, 0, 0));

        let accepted = accept_task.await.unwrap().unwrap();
        assert_eq!(accepted.tunnel_id(), tunnel_id);
    }

    #[tokio::test]
    async fn pn_tunnel_control_open_does_not_consume_business_accept() {
        let local_identity: P2pIdentityRef = Arc::new(FakeIdentity::new(93));
        let local_id = local_identity.get_id();
        let remote_id = p2p_id(94);
        let shared = PnShared::new_for_test(local_identity);
        let (listener, tx) = make_listener(shared.clone());
        let (listener_read, listener_write, mut peer_read, mut peer_write) = make_stream_pair();
        let tunnel_id = crate::types::TunnelId::from(92);

        send_ttp_stream(
            &tx,
            meta(local_id.clone(), remote_id.clone()),
            listener_read,
            listener_write,
        )
        .unwrap();
        write_pn_command(
            &mut peer_write,
            ProxyControlOpenReq {
                tunnel_id,
                from: remote_id.clone(),
                to: local_id.clone(),
            },
        )
        .await
        .unwrap();

        let tunnel = listener.accept_tunnel().await.unwrap();
        let resp = read_pn_command::<_, ProxyControlOpenResp>(&mut peer_read)
            .await
            .unwrap();
        assert_eq!(resp.result, TunnelCommandResult::Success as u8);
        tunnel
            .listen_stream(crate::networks::ListenVPortRegistry::<()>::new().as_listen_vports_ref())
            .await
            .unwrap();

        let mut pending_accept = Box::pin({
            let tunnel = tunnel.clone();
            async move { tunnel.accept_stream().await }
        });
        assert!(
            timeout(Duration::from_millis(30), &mut pending_accept)
                .await
                .is_err()
        );

        let (business_read, business_write, mut business_peer_read, mut business_peer_write) =
            make_stream_pair();
        send_ttp_stream(
            &tx,
            meta(local_id.clone(), remote_id.clone()),
            business_read,
            business_write,
        )
        .unwrap();
        write_pn_command(
            &mut business_peer_write,
            ProxyOpenReq {
                tunnel_id,
                from: remote_id,
                to: local_id,
                kind: PnChannelKind::Stream,
                purpose: purpose(7001),
            },
        )
        .await
        .unwrap();

        let dispatch_task = tokio::spawn({
            let listener = listener.clone();
            async move { listener.accept_tunnel().await }
        });
        let err = timeout(Duration::from_secs(1), &mut pending_accept)
            .await
            .unwrap()
            .err()
            .unwrap();
        assert_eq!(err.code(), P2pErrorCode::PortNotListen);
        let resp = read_pn_command::<_, ProxyOpenResp>(&mut business_peer_read)
            .await
            .unwrap();
        assert_eq!(resp.result, TunnelCommandResult::PortNotListen as u8);
        dispatch_task.abort();
    }
}
