use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use p2p_frame::endpoint::{Endpoint, Protocol};
use p2p_frame::error::{P2pErrorCode, P2pResult, p2p_err};
use p2p_frame::networks::{
    IncomingControlStreamCallback, IncomingDatagramCallback, IncomingStreamCallback,
    ListenVPortsRef, NetManager, Tunnel, TunnelDatagramWrite, TunnelForm, TunnelPurpose, TunnelRef,
    TunnelState, TunnelStreamRead, TunnelStreamWrite, ValidateResult,
};
use p2p_frame::p2p_identity::P2pIdentityRef;
use p2p_frame::tls::DefaultTlsServerCertResolver;
use p2p_frame::ttp::{
    TtpConnector, TtpIncomingStreamCallback, TtpIncomingStreamCallbackFuture,
    TtpIncomingTunnelValidateContext, TtpIncomingTunnelValidator, TtpNode, TtpPortListener,
    TtpServer, TtpTarget,
};
use p2p_frame::types::{TunnelCandidateId, TunnelId};
use p2p_frame::x509::generate_rsa_x509_identity;
use tokio::io::split;
use tokio::sync::mpsc;
use tokio::time::{Duration, timeout};

fn identity(name: &str, endpoint: Endpoint) -> P2pIdentityRef {
    let identity: P2pIdentityRef = Arc::new(
        generate_rsa_x509_identity(Some(name.to_owned())).expect("generate test identity"),
    );
    identity.update_endpoints(vec![endpoint])
}

fn manager() -> Arc<NetManager> {
    NetManager::new(Vec::new(), DefaultTlsServerCertResolver::new())
        .expect("create controlled NetManager")
}

fn purpose(label: &str) -> TunnelPurpose {
    TunnelPurpose::from_bytes(format!("ttp-shared-runtime/{label}").into_bytes())
}

struct ControlledTunnel {
    local: P2pIdentityRef,
    remote: P2pIdentityRef,
    local_ep: Endpoint,
    remote_ep: Endpoint,
    stream_callback: Mutex<Option<IncomingStreamCallback>>,
    close_count: AtomicUsize,
}

impl ControlledTunnel {
    fn new(
        local: P2pIdentityRef,
        remote: P2pIdentityRef,
        local_ep: Endpoint,
        remote_ep: Endpoint,
    ) -> Arc<Self> {
        Arc::new(Self {
            local,
            remote,
            local_ep,
            remote_ep,
            stream_callback: Mutex::new(None),
            close_count: AtomicUsize::new(0),
        })
    }

    async fn emit_stream(&self, purpose: TunnelPurpose) {
        let callback = self
            .stream_callback
            .lock()
            .unwrap()
            .clone()
            .expect("accepted tunnel registered its stream listener");
        let (peer, local) = tokio::io::duplex(64);
        let (read, write) = split(local);
        drop(peer);
        callback(Ok((purpose, Box::pin(read), Box::pin(write)))).await;
    }
}

#[async_trait::async_trait]
impl Tunnel for ControlledTunnel {
    fn tunnel_id(&self) -> TunnelId {
        TunnelId::from(35)
    }

    fn candidate_id(&self) -> TunnelCandidateId {
        TunnelCandidateId::from(35)
    }

    fn form(&self) -> TunnelForm {
        TunnelForm::Passive
    }

    fn is_reverse(&self) -> bool {
        false
    }

    fn protocol(&self) -> Protocol {
        self.local_ep.protocol()
    }

    fn local_id(&self) -> p2p_frame::p2p_identity::P2pId {
        self.local.get_id()
    }

    fn remote_id(&self) -> p2p_frame::p2p_identity::P2pId {
        self.remote.get_id()
    }

    fn local_ep(&self) -> Option<Endpoint> {
        Some(self.local_ep)
    }

    fn remote_ep(&self) -> Option<Endpoint> {
        Some(self.remote_ep)
    }

    fn state(&self) -> TunnelState {
        TunnelState::Connected
    }

    fn is_closed(&self) -> bool {
        self.close_count.load(Ordering::SeqCst) > 0
    }

    fn close(&self) -> P2pResult<()> {
        self.close_count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn listen_stream(
        &self,
        _purposes: ListenVPortsRef,
        callback: IncomingStreamCallback,
    ) -> P2pResult<()> {
        *self.stream_callback.lock().unwrap() = Some(callback);
        Ok(())
    }

    async fn listen_datagram(
        &self,
        _purposes: ListenVPortsRef,
        _callback: IncomingDatagramCallback,
    ) -> P2pResult<()> {
        Ok(())
    }

    async fn listen_control_stream(
        &self,
        _purposes: ListenVPortsRef,
        _callback: IncomingControlStreamCallback,
    ) -> P2pResult<()> {
        Ok(())
    }

    async fn open_stream(
        &self,
        _purpose: TunnelPurpose,
    ) -> P2pResult<(TunnelStreamRead, TunnelStreamWrite)> {
        let (peer, local) = tokio::io::duplex(64);
        let (read, write) = split(local);
        drop(peer);
        Ok((Box::pin(read), Box::pin(write)))
    }

    async fn open_datagram(&self, _purpose: TunnelPurpose) -> P2pResult<TunnelDatagramWrite> {
        let (peer, local) = tokio::io::duplex(64);
        let (_read, write) = split(local);
        drop(peer);
        Ok(Box::pin(write))
    }

    async fn open_control_stream(
        &self,
        purpose: TunnelPurpose,
    ) -> P2pResult<(TunnelStreamRead, TunnelStreamWrite)> {
        self.open_stream(purpose).await
    }
}

enum ValidatorDecision {
    Reject,
    Error,
}

struct DecisionValidator {
    decision: ValidatorDecision,
    calls: AtomicUsize,
}

#[async_trait::async_trait]
impl TtpIncomingTunnelValidator for DecisionValidator {
    async fn validate(&self, _ctx: &TtpIncomingTunnelValidateContext) -> P2pResult<ValidateResult> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match self.decision {
            ValidatorDecision::Reject => Ok(ValidateResult::Reject("blocked".to_owned())),
            ValidatorDecision::Error => {
                Err(p2p_err!(P2pErrorCode::ErrorState, "validator failure"))
            }
        }
    }
}

#[test]
fn public_runtime_facade_signatures_remain_additive() {
    let _: fn(P2pIdentityRef, Arc<NetManager>) -> P2pResult<_> = TtpServer::new;
    let _: fn(P2pIdentityRef, Arc<NetManager>) -> P2pResult<_> = TtpNode::new;
    let _: fn(p2p_frame::ttp::TtpRuntime) -> _ = TtpServer::new_with_runtime;
    let _: fn(p2p_frame::ttp::TtpRuntime) -> _ = TtpNode::new_with_runtime;
    let _: fn(&TtpServer) -> p2p_frame::ttp::TtpRuntime = TtpServer::runtime;
    let _: fn(&TtpNode) -> p2p_frame::ttp::TtpRuntime = TtpNode::runtime;
}

#[tokio::test]
async fn duplicate_failure_and_shared_drops_preserve_incumbent_until_last_handle() {
    let local_ep = Endpoint::from((Protocol::Tcp, "127.0.0.1:25001".parse().unwrap()));
    let remote_ep = Endpoint::from((Protocol::Tcp, "127.0.0.1:25002".parse().unwrap()));
    let local = identity("shared-runtime-local", local_ep);
    let remote = identity("shared-runtime-remote", remote_ep);
    let manager = manager();
    let server = TtpServer::new(local.clone(), manager.clone()).unwrap();
    let runtime = server.runtime();
    let node = TtpNode::new_with_runtime(runtime.clone());
    let stream_purpose = purpose("incumbent");
    let (tx, mut rx) = mpsc::channel(1);
    let callback: TtpIncomingStreamCallback = Arc::new(move |result| {
        let tx = tx.clone();
        Box::pin(async move {
            let _ = tx.send(result).await;
        }) as TtpIncomingStreamCallbackFuture
    });
    server
        .listen_stream(stream_purpose.clone(), callback)
        .await
        .unwrap();
    let duplicate = node
        .listen_stream(stream_purpose.clone(), Arc::new(|_| Box::pin(async {})))
        .await
        .unwrap_err();
    assert_eq!(duplicate.code(), P2pErrorCode::AlreadyExists);
    assert_eq!(
        TtpServer::new(local.clone(), manager.clone())
            .err()
            .unwrap()
            .code(),
        P2pErrorCode::AlreadyExists
    );

    drop(server);
    drop(runtime);
    let tunnel = ControlledTunnel::new(local.clone(), remote, local_ep, remote_ep);
    manager.incoming_tunnel_callback()(Ok(tunnel.clone() as TunnelRef)).await;
    tunnel.emit_stream(stream_purpose).await;
    timeout(Duration::from_secs(1), rx.recv())
        .await
        .expect("incumbent dispatch remains bounded")
        .expect("incumbent callback remains installed")
        .expect("incumbent receives controlled tunnel stream");

    drop(node);
    let node = TtpNode::new(local.clone(), manager.clone()).unwrap();
    let runtime = node.runtime();
    let server = TtpServer::new_with_runtime(runtime.clone());
    drop(node);
    drop(runtime);
    assert_eq!(
        TtpNode::new(local.clone(), manager.clone())
            .err()
            .unwrap()
            .code(),
        P2pErrorCode::AlreadyExists
    );
    drop(server);
    TtpServer::new(local, manager).expect("last handle drop permits exact re-registration");
}

#[tokio::test]
async fn shared_validator_reject_and_error_never_attach_or_cache() {
    for decision in [ValidatorDecision::Reject, ValidatorDecision::Error] {
        let local_ep = Endpoint::from((Protocol::Tcp, "127.0.0.1:25011".parse().unwrap()));
        let remote_ep = Endpoint::from((Protocol::Tcp, "127.0.0.1:25012".parse().unwrap()));
        let local = identity("validator-local", local_ep);
        let remote = identity("validator-remote", remote_ep);
        let manager = manager();
        let validator = Arc::new(DecisionValidator {
            decision,
            calls: AtomicUsize::new(0),
        });
        let server = TtpServer::new_with_incoming_tunnel_validator(
            local.clone(),
            manager.clone(),
            validator.clone(),
        )
        .unwrap();
        let _node = TtpNode::new_with_runtime(server.runtime());
        let tunnel = ControlledTunnel::new(local, remote.clone(), local_ep, remote_ep);
        manager.incoming_tunnel_callback()(Ok(tunnel.clone() as TunnelRef)).await;

        let target = TtpTarget {
            local_ep: None,
            remote_ep,
            remote_id: remote.get_id(),
            remote_name: Some(remote.get_name()),
        };
        assert_eq!(validator.calls.load(Ordering::SeqCst), 1);
        assert!(tunnel.stream_callback.lock().unwrap().is_none());
        assert_eq!(tunnel.close_count.load(Ordering::SeqCst), 1);
        assert_eq!(
            server
                .open_stream(&target, purpose("rejected"))
                .await
                .err()
                .unwrap()
                .code(),
            P2pErrorCode::NotFound
        );
    }
}

#[tokio::test]
async fn shared_cache_matches_remote_id_and_endpoint_while_server_stays_passive() {
    let local_ep = Endpoint::from((Protocol::Tcp, "127.0.0.1:25021".parse().unwrap()));
    let remote_ep = Endpoint::from((Protocol::Tcp, "127.0.0.1:25022".parse().unwrap()));
    let other_ep = Endpoint::from((Protocol::Tcp, "127.0.0.1:25023".parse().unwrap()));
    let local_port_zero = Endpoint::from((Protocol::Tcp, "127.0.0.1:0".parse().unwrap()));
    let local_port_mismatch = Endpoint::from((Protocol::Tcp, "127.0.0.1:25024".parse().unwrap()));
    let local_ip_mismatch = Endpoint::from((Protocol::Tcp, "127.0.0.2:0".parse().unwrap()));
    let local_protocol_mismatch = Endpoint::from((Protocol::Quic, "127.0.0.1:0".parse().unwrap()));
    let local = identity("cache-local", local_ep);
    let remote = identity("cache-remote", remote_ep);
    let other = identity("cache-other", remote_ep);
    let manager = manager();
    let node = TtpNode::new(local.clone(), manager.clone()).unwrap();
    let server = TtpServer::new_with_runtime(node.runtime());
    let tunnel = ControlledTunnel::new(local, remote.clone(), local_ep, remote_ep);
    manager.incoming_tunnel_callback()(Ok(tunnel as TunnelRef)).await;

    let target = TtpTarget {
        local_ep: None,
        remote_ep,
        remote_id: remote.get_id(),
        remote_name: Some(remote.get_name()),
    };
    node.open_stream(&target, purpose("node-reuse"))
        .await
        .expect("node reuses runtime incoming cache");
    server
        .open_control_stream(&target, purpose("server-reuse"))
        .await
        .expect("server passively reuses the same runtime cache");
    server
        .open_stream(
            &TtpTarget {
                local_ep: Some(local_port_zero),
                ..target.clone()
            },
            purpose("local-port-zero"),
        )
        .await
        .expect("local port zero matches the cached tunnel with the same protocol and IP");

    for (label, local_ep) in [
        ("nonzero-port-mismatch", local_port_mismatch),
        ("ip-mismatch", local_ip_mismatch),
        ("protocol-mismatch", local_protocol_mismatch),
    ] {
        let mismatched = TtpTarget {
            local_ep: Some(local_ep),
            ..target.clone()
        };
        assert_eq!(
            server
                .open_stream(&mismatched, purpose(label))
                .await
                .err()
                .unwrap()
                .code(),
            P2pErrorCode::NotFound,
            "{label} must not reuse the cached tunnel"
        );
    }

    let wrong_endpoint = TtpTarget {
        remote_ep: other_ep,
        ..target.clone()
    };
    assert_eq!(
        server
            .open_stream(&wrong_endpoint, purpose("wrong-endpoint"))
            .await
            .err()
            .unwrap()
            .code(),
        P2pErrorCode::NotFound
    );
    let wrong_remote = TtpTarget {
        remote_id: other.get_id(),
        remote_name: Some(other.get_name()),
        ..target
    };
    assert_eq!(
        server
            .open_stream(&wrong_remote, purpose("wrong-remote"))
            .await
            .err()
            .unwrap()
            .code(),
        P2pErrorCode::NotFound
    );
}
