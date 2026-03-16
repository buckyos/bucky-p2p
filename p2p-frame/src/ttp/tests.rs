use super::*;
use crate::endpoint::{Endpoint, Protocol};
use crate::error::{P2pError, P2pErrorCode, P2pResult, p2p_err};
use crate::executor::Executor;
use crate::networks::{
    NetManager, NetManagerRef, Tunnel, TunnelDatagramRead, TunnelDatagramWrite, TunnelForm,
    TunnelListener, TunnelListenerInfo, TunnelListenerRef, TunnelNetwork, TunnelNetworkRef,
    TunnelRef, TunnelState, TunnelStreamRead, TunnelStreamWrite,
};
use crate::p2p_identity::{
    EncodedP2pIdentity, P2pId, P2pIdentity, P2pIdentityCertRef, P2pIdentityRef, P2pSignature,
};
use crate::runtime;
use crate::tls::DefaultTlsServerCertResolver;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream, split};
use tokio::sync::{Mutex as AsyncMutex, mpsc};
use tokio::time::{Duration, timeout};

fn purpose_of(value: u16) -> crate::networks::TunnelPurpose {
    crate::networks::TunnelPurpose::from_value(&value).unwrap()
}

struct DummyIdentity {
    id: P2pId,
    name: String,
    endpoints: Vec<Endpoint>,
}

impl P2pIdentity for DummyIdentity {
    fn get_identity_cert(&self) -> P2pResult<P2pIdentityCertRef> {
        Err(p2p_err!(P2pErrorCode::NotSupport, "unused in test"))
    }

    fn get_id(&self) -> P2pId {
        self.id.clone()
    }

    fn get_name(&self) -> String {
        self.name.clone()
    }

    fn sign_type(&self) -> crate::p2p_identity::P2pIdentitySignType {
        crate::p2p_identity::P2pIdentitySignType::Rsa
    }

    fn sign(&self, _message: &[u8]) -> P2pResult<P2pSignature> {
        Err(p2p_err!(P2pErrorCode::NotSupport, "unused in test"))
    }

    fn get_encoded_identity(&self) -> P2pResult<EncodedP2pIdentity> {
        Err(p2p_err!(P2pErrorCode::NotSupport, "unused in test"))
    }

    fn endpoints(&self) -> Vec<Endpoint> {
        self.endpoints.clone()
    }

    fn update_endpoints(&self, eps: Vec<Endpoint>) -> P2pIdentityRef {
        Arc::new(Self {
            id: self.id.clone(),
            name: self.name.clone(),
            endpoints: eps,
        })
    }
}

struct FakeTunnel {
    tunnel_id: crate::types::TunnelId,
    candidate_id: crate::types::TunnelCandidateId,
    local_id: P2pId,
    remote_id: P2pId,
    local_ep: Endpoint,
    remote_ep: Endpoint,
    incoming_stream_rx: AsyncMutex<
        mpsc::UnboundedReceiver<
            P2pResult<(
                crate::networks::TunnelPurpose,
                TunnelStreamRead,
                TunnelStreamWrite,
            )>,
        >,
    >,
    incoming_stream_tx: mpsc::UnboundedSender<
        P2pResult<(
            crate::networks::TunnelPurpose,
            TunnelStreamRead,
            TunnelStreamWrite,
        )>,
    >,
    incoming_datagram_rx: AsyncMutex<
        mpsc::UnboundedReceiver<P2pResult<(crate::networks::TunnelPurpose, TunnelDatagramRead)>>,
    >,
    incoming_datagram_tx:
        mpsc::UnboundedSender<P2pResult<(crate::networks::TunnelPurpose, TunnelDatagramRead)>>,
    opened_stream_vports: Mutex<Vec<u16>>,
    opened_datagram_vports: Mutex<Vec<u16>>,
}

impl FakeTunnel {
    fn new(
        local_id: P2pId,
        remote_id: P2pId,
        local_ep: Endpoint,
        remote_ep: Endpoint,
    ) -> Arc<Self> {
        let (stream_tx, stream_rx) = mpsc::unbounded_channel();
        let (datagram_tx, datagram_rx) = mpsc::unbounded_channel();
        Arc::new(Self {
            tunnel_id: crate::types::TunnelId::from(1),
            candidate_id: crate::types::TunnelCandidateId::from(1),
            local_id,
            remote_id,
            local_ep,
            remote_ep,
            incoming_stream_rx: AsyncMutex::new(stream_rx),
            incoming_stream_tx: stream_tx,
            incoming_datagram_rx: AsyncMutex::new(datagram_rx),
            incoming_datagram_tx: datagram_tx,
            opened_stream_vports: Mutex::new(Vec::new()),
            opened_datagram_vports: Mutex::new(Vec::new()),
        })
    }

    fn push_stream(&self, vport: u16) {
        let (peer_end, tunnel_end) = tokio::io::duplex(64);
        let (tunnel_read, tunnel_write) = split(tunnel_end);
        drop(peer_end);
        let _ = self.incoming_stream_tx.send(Ok((
            purpose_of(vport),
            Box::pin(tunnel_read),
            Box::pin(tunnel_write),
        )));
    }

    fn push_datagram(&self, vport: u16, payload: &'static [u8]) {
        let (mut peer_end, tunnel_end) = tokio::io::duplex(64);
        let (tunnel_read, _tunnel_write) = split(tunnel_end);
        tokio::spawn(async move {
            let _ = peer_end.write_all(payload).await;
        });
        let _ = self
            .incoming_datagram_tx
            .send(Ok((purpose_of(vport), Box::pin(tunnel_read))));
    }

    fn opened_stream_vports(&self) -> Vec<u16> {
        self.opened_stream_vports.lock().unwrap().clone()
    }

    fn opened_datagram_vports(&self) -> Vec<u16> {
        self.opened_datagram_vports.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl Tunnel for FakeTunnel {
    fn tunnel_id(&self) -> crate::types::TunnelId {
        self.tunnel_id
    }

    fn candidate_id(&self) -> crate::types::TunnelCandidateId {
        self.candidate_id
    }

    fn form(&self) -> TunnelForm {
        TunnelForm::Active
    }

    fn is_reverse(&self) -> bool {
        false
    }

    fn protocol(&self) -> Protocol {
        self.local_ep.protocol()
    }

    fn local_id(&self) -> P2pId {
        self.local_id.clone()
    }

    fn remote_id(&self) -> P2pId {
        self.remote_id.clone()
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
        false
    }

    async fn close(&self) -> P2pResult<()> {
        Ok(())
    }

    async fn listen_stream(&self, _vports: crate::networks::ListenVPortsRef) -> P2pResult<()> {
        Ok(())
    }

    async fn listen_datagram(&self, _vports: crate::networks::ListenVPortsRef) -> P2pResult<()> {
        Ok(())
    }

    async fn open_stream(
        &self,
        purpose: crate::networks::TunnelPurpose,
    ) -> P2pResult<(TunnelStreamRead, TunnelStreamWrite)> {
        let vport = purpose.decode_as::<u16>().unwrap();
        self.opened_stream_vports.lock().unwrap().push(vport);
        let (peer_end, tunnel_end) = tokio::io::duplex(64);
        let (tunnel_read, tunnel_write) = split(tunnel_end);
        drop(peer_end);
        Ok((Box::pin(tunnel_read), Box::pin(tunnel_write)))
    }

    async fn accept_stream(
        &self,
    ) -> P2pResult<(
        crate::networks::TunnelPurpose,
        TunnelStreamRead,
        TunnelStreamWrite,
    )> {
        let mut rx = self.incoming_stream_rx.lock().await;
        rx.recv()
            .await
            .ok_or_else(|| p2p_err!(P2pErrorCode::Interrupted, "stream closed"))?
    }

    async fn open_datagram(
        &self,
        purpose: crate::networks::TunnelPurpose,
    ) -> P2pResult<TunnelDatagramWrite> {
        let vport = purpose.decode_as::<u16>().unwrap();
        self.opened_datagram_vports.lock().unwrap().push(vport);
        let (peer_end, tunnel_end) = tokio::io::duplex(64);
        let (_tunnel_read, tunnel_write) = split(tunnel_end);
        drop(peer_end);
        Ok(Box::pin(tunnel_write))
    }

    async fn accept_datagram(
        &self,
    ) -> P2pResult<(crate::networks::TunnelPurpose, TunnelDatagramRead)> {
        let mut rx = self.incoming_datagram_rx.lock().await;
        rx.recv()
            .await
            .ok_or_else(|| p2p_err!(P2pErrorCode::Interrupted, "datagram closed"))?
    }
}

struct FakeTunnelListener {
    rx: AsyncMutex<mpsc::UnboundedReceiver<P2pResult<TunnelRef>>>,
}

#[async_trait::async_trait]
impl TunnelListener for FakeTunnelListener {
    async fn accept_tunnel(&self) -> P2pResult<TunnelRef> {
        let mut rx = self.rx.lock().await;
        rx.recv()
            .await
            .ok_or_else(|| p2p_err!(P2pErrorCode::Interrupted, "listener closed"))?
    }
}

struct FakeTunnelNetwork {
    protocol: Protocol,
    listener: TunnelListenerRef,
    tx: mpsc::UnboundedSender<P2pResult<TunnelRef>>,
    infos: Mutex<Vec<TunnelListenerInfo>>,
    created_tunnel: Mutex<Option<TunnelRef>>,
    create_count: AtomicUsize,
}

impl FakeTunnelNetwork {
    fn new(protocol: Protocol) -> Arc<Self> {
        let (tx, rx) = mpsc::unbounded_channel();
        Arc::new(Self {
            protocol,
            listener: Arc::new(FakeTunnelListener {
                rx: AsyncMutex::new(rx),
            }),
            tx,
            infos: Mutex::new(Vec::new()),
            created_tunnel: Mutex::new(None),
            create_count: AtomicUsize::new(0),
        })
    }

    fn set_created_tunnel(&self, tunnel: TunnelRef) {
        *self.created_tunnel.lock().unwrap() = Some(tunnel);
    }

    fn push_tunnel(&self, tunnel: TunnelRef) {
        let _ = self.tx.send(Ok(tunnel));
    }

    fn create_count(&self) -> usize {
        self.create_count.load(Ordering::SeqCst)
    }
}

#[async_trait::async_trait]
impl TunnelNetwork for FakeTunnelNetwork {
    fn protocol(&self) -> Protocol {
        self.protocol
    }

    fn is_udp(&self) -> bool {
        self.protocol == Protocol::Quic
    }

    async fn listen(
        &self,
        local: &Endpoint,
        _out: Option<Endpoint>,
        mapping_port: Option<u16>,
    ) -> P2pResult<TunnelListenerRef> {
        *self.infos.lock().unwrap() = vec![TunnelListenerInfo {
            local: *local,
            mapping_port,
        }];
        Ok(self.listener.clone())
    }

    async fn close_all_listener(&self) -> P2pResult<()> {
        Ok(())
    }

    fn listeners(&self) -> Vec<TunnelListenerRef> {
        vec![self.listener.clone()]
    }

    fn listener_infos(&self) -> Vec<TunnelListenerInfo> {
        self.infos.lock().unwrap().clone()
    }

    async fn create_tunnel_with_intent(
        &self,
        _local_identity: &P2pIdentityRef,
        _remote: &Endpoint,
        _remote_id: &P2pId,
        _remote_name: Option<String>,
        _intent: crate::networks::TunnelConnectIntent,
    ) -> P2pResult<TunnelRef> {
        self.create_count.fetch_add(1, Ordering::SeqCst);
        self.created_tunnel
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| p2p_err!(P2pErrorCode::NotFound, "no created tunnel configured"))
    }

    async fn create_tunnel_with_local_ep_and_intent(
        &self,
        local_identity: &P2pIdentityRef,
        _local_ep: &Endpoint,
        remote: &Endpoint,
        remote_id: &P2pId,
        remote_name: Option<String>,
        intent: crate::networks::TunnelConnectIntent,
    ) -> P2pResult<TunnelRef> {
        self.create_tunnel_with_intent(local_identity, remote, remote_id, remote_name, intent)
            .await
    }
}

fn make_identity(id: u8, name: &str, endpoint: Endpoint) -> P2pIdentityRef {
    Arc::new(DummyIdentity {
        id: P2pId::from(vec![id; 32]),
        name: name.to_owned(),
        endpoints: vec![endpoint],
    })
}

fn make_manager(network: TunnelNetworkRef) -> NetManagerRef {
    NetManager::new(vec![network], DefaultTlsServerCertResolver::new()).unwrap()
}

fn init_executor() {
    Executor::init_new_multi_thread(None);
}

#[tokio::test]
async fn client_open_stream_and_datagram_reuse_same_tunnel() {
    init_executor();
    let local_ep = Endpoint::from((Protocol::Tcp, "127.0.0.1:24001".parse().unwrap()));
    let remote_ep = Endpoint::from((Protocol::Tcp, "127.0.0.1:24002".parse().unwrap()));
    let local = make_identity(1, "local-client", local_ep);
    let remote = make_identity(2, "remote-server", remote_ep);
    let tunnel = FakeTunnel::new(local.get_id(), remote.get_id(), local_ep, remote_ep);
    let network = FakeTunnelNetwork::new(Protocol::Tcp);
    network.set_created_tunnel(tunnel.clone());
    let manager = make_manager(network.clone() as TunnelNetworkRef);
    let client = TtpClient::new(local, manager);
    let target = TtpTarget {
        local_ep: None,
        remote_ep,
        remote_id: remote.get_id(),
        remote_name: Some(remote.get_name()),
    };

    let (stream_meta, _stream_read, _stream_write) =
        client.open_stream(&target, purpose_of(1001)).await.unwrap();
    let (datagram_meta, _datagram_write) = client
        .open_datagram(&target, purpose_of(1002))
        .await
        .unwrap();

    assert_eq!(stream_meta.remote_id, target.remote_id);
    assert_eq!(stream_meta.purpose, purpose_of(1001));
    assert_eq!(datagram_meta.purpose, purpose_of(1002));
    assert_eq!(network.create_count(), 1);
    assert_eq!(tunnel.opened_stream_vports(), vec![1001]);
    assert_eq!(tunnel.opened_datagram_vports(), vec![1002]);
}

#[tokio::test]
async fn server_listeners_receive_incoming_stream_and_datagram() {
    init_executor();
    let local_ep = Endpoint::from((Protocol::Quic, "127.0.0.1:24101".parse().unwrap()));
    let remote_ep = Endpoint::from((Protocol::Quic, "127.0.0.1:24102".parse().unwrap()));
    let local = make_identity(3, "local-server", local_ep);
    let remote = make_identity(4, "remote-client", remote_ep);
    let network = FakeTunnelNetwork::new(Protocol::Quic);
    let manager = make_manager(network.clone() as TunnelNetworkRef);
    manager.listen(&[local_ep], None).await.unwrap();
    let server = TtpServer::new(local.clone(), manager).unwrap();
    let stream_listener = server.listen_stream(purpose_of(2001)).await.unwrap();
    let datagram_listener = server.listen_datagram(purpose_of(2002)).await.unwrap();
    let tunnel = FakeTunnel::new(local.get_id(), remote.get_id(), local_ep, remote_ep);

    network.push_tunnel(tunnel.clone());
    tunnel.push_stream(2001);
    tunnel.push_datagram(2002, b"abc");

    let (stream_meta, _stream_read, _stream_write) =
        timeout(Duration::from_secs(1), stream_listener.accept())
            .await
            .unwrap()
            .unwrap();
    let datagram = timeout(Duration::from_secs(1), datagram_listener.accept())
        .await
        .unwrap()
        .unwrap();
    let mut buf = [0u8; 3];
    let mut read = datagram.read;
    read.read_exact(&mut buf).await.unwrap();

    assert_eq!(stream_meta.remote_id, remote.get_id());
    assert_eq!(stream_meta.purpose, purpose_of(2001));
    assert_eq!(datagram.meta.remote_id, remote.get_id());
    assert_eq!(datagram.meta.purpose, purpose_of(2002));
    assert_eq!(&buf, b"abc");
}

#[tokio::test]
async fn server_open_stream_requires_existing_incoming_tunnel() {
    init_executor();
    let local_ep = Endpoint::from((Protocol::Tcp, "127.0.0.1:24201".parse().unwrap()));
    let remote_ep = Endpoint::from((Protocol::Tcp, "127.0.0.1:24202".parse().unwrap()));
    let local = make_identity(5, "local-server-open", local_ep);
    let remote = make_identity(6, "remote-client-open", remote_ep);
    let network = FakeTunnelNetwork::new(Protocol::Tcp);
    let manager = make_manager(network as TunnelNetworkRef);
    manager.listen(&[local_ep], None).await.unwrap();
    let server = TtpServer::new(local, manager).unwrap();
    let target = TtpTarget {
        local_ep: None,
        remote_ep,
        remote_id: remote.get_id(),
        remote_name: Some(remote.get_name()),
    };

    let err = server
        .open_stream(&target, purpose_of(3001))
        .await
        .err()
        .unwrap();
    assert_eq!(err.code(), P2pErrorCode::NotFound);
}

#[tokio::test]
async fn server_open_stream_and_datagram_reuse_existing_incoming_tunnel() {
    init_executor();
    let local_ep = Endpoint::from((Protocol::Tcp, "127.0.0.1:24301".parse().unwrap()));
    let remote_ep = Endpoint::from((Protocol::Tcp, "127.0.0.1:24302".parse().unwrap()));
    let local = make_identity(7, "local-server-reuse", local_ep);
    let remote = make_identity(8, "remote-client-reuse", remote_ep);
    let network = FakeTunnelNetwork::new(Protocol::Tcp);
    let manager = make_manager(network.clone() as TunnelNetworkRef);
    manager.listen(&[local_ep], None).await.unwrap();
    let server = TtpServer::new(local.clone(), manager).unwrap();
    let listener = server.listen_stream(purpose_of(4001)).await.unwrap();
    let tunnel = FakeTunnel::new(local.get_id(), remote.get_id(), local_ep, remote_ep);

    network.push_tunnel(tunnel.clone());
    tunnel.push_stream(4001);
    let _ = timeout(Duration::from_secs(1), listener.accept())
        .await
        .unwrap()
        .unwrap();

    let target = TtpTarget {
        local_ep: None,
        remote_ep,
        remote_id: remote.get_id(),
        remote_name: Some(remote.get_name()),
    };
    let (stream_meta, _stream_read, _stream_write) =
        server.open_stream(&target, purpose_of(4002)).await.unwrap();
    let (datagram_meta, _datagram_write) = server
        .open_datagram(&target, purpose_of(4003))
        .await
        .unwrap();

    assert_eq!(stream_meta.purpose, purpose_of(4002));
    assert_eq!(datagram_meta.purpose, purpose_of(4003));
    assert_eq!(tunnel.opened_stream_vports(), vec![4002]);
    assert_eq!(tunnel.opened_datagram_vports(), vec![4003]);
}
