use super::super::*;
use crate::runtime::{AsyncReadExt, AsyncWriteExt};
use rustls::pki_types::PrivatePkcs8KeyDer;
use tokio::time::timeout;

struct Fixture {
    _client_endpoint: quinn::Endpoint,
    _server_endpoint: quinn::Endpoint,
    tunnel: Arc<QuicTunnel>,
    peer: quinn::Connection,
    peer_command_read: quinn::RecvStream,
    peer_command_write: quinn::SendStream,
}

impl Fixture {
    async fn new() -> Self {
        let certified = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let certificate = certified.cert.der().clone();
        let key = PrivatePkcs8KeyDer::from(certified.signing_key.serialize_der());
        let server_config = quinn::ServerConfig::with_single_cert(vec![certificate.clone()], key.into()).unwrap();
        let server_endpoint = quinn::Endpoint::server(server_config, "127.0.0.1:0".parse().unwrap()).unwrap();
        let mut roots = rustls::RootCertStore::empty();
        roots.add(certificate).unwrap();
        let client_config = quinn::ClientConfig::with_root_certificates(Arc::new(roots)).unwrap();
        let mut client_endpoint = quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
        client_endpoint.set_default_client_config(client_config);
        let remote = server_endpoint.local_addr().unwrap();
        let (client, peer) = timeout(Duration::from_secs(5), async {
            tokio::join!(client_endpoint.connect(remote, "localhost").unwrap(), async {
                server_endpoint.accept().await.unwrap().await
            })
        }).await.unwrap();
        let client = client.unwrap();
        let peer = peer.unwrap();
        let (mut command_write, command_read) = client.open_bi().await.unwrap();
        command_write.write_all(b"ready").await.unwrap();
        let (peer_command_write, mut peer_command_read) = peer.accept_bi().await.unwrap();
        let mut ready = [0; 5];
        peer_command_read.read_exact(&mut ready).await.unwrap();
        let tunnel = QuicTunnel::new_inner(client, TunnelId::from(81), TunnelCandidateId::from(81),
            TunnelForm::Active, false, P2pId::default(), P2pId::default(),
            Endpoint::from((Protocol::Quic, client_endpoint.local_addr().unwrap())),
            Endpoint::from((Protocol::Quic, remote)), command_read, command_write);
        Self { _client_endpoint: client_endpoint, _server_endpoint: server_endpoint,
            tunnel, peer, peer_command_read, peer_command_write }
    }

    async fn stop_command(&mut self) {
        self.peer_command_read.stop(quinn::VarInt::from_u32(81)).unwrap();
        let stopped = timeout(Duration::from_secs(2), async {
            self.tunnel.command_write.lock().await.stopped().await
        }).await.unwrap().unwrap();
        assert_eq!(stopped, Some(quinn::VarInt::from_u32(81)));
        assert!(self.tunnel.socket.close_reason().is_none(), "STOP_SENDING must not close connection itself");
    }

    async fn data(&self) -> (TunnelStreamRead, TunnelStreamWrite, quinn::RecvStream, quinn::SendStream) {
        let (mut send, recv) = self.tunnel.socket.open_bi().await.unwrap();
        send.write_all(b"ok").await.unwrap();
        let (peer_send, mut peer_recv) = self.peer.accept_bi().await.unwrap();
        let mut bytes = [0; 2];
        peer_recv.read_exact(&mut bytes).await.unwrap();
        assert_eq!(&bytes, b"ok");
        (Box::pin(recv), Box::pin(send), peer_recv, peer_send)
    }
}

#[tokio::test]
async fn unit_command_transport_failure_closes_connection_and_fails_pending_requests() {
    let mut fixture = Fixture::new().await;
    let (tx, rx) = oneshot::channel();
    fixture.tunnel.state.lock().unwrap().pending_open_requests.insert(99, tx);
    fixture.stop_command().await;
    let result = fixture.tunnel.send_command(TunnelPing { seq: 1, send_time: 0 }).await;
    assert_eq!(result.unwrap_err().code(), P2pErrorCode::IoError);
    assert!(fixture.tunnel.closed.load(Ordering::SeqCst));
    assert!(fixture.tunnel.socket.close_reason().is_some());
    assert!(timeout(Duration::from_secs(1), rx).await.unwrap().unwrap().is_err());
    assert!(fixture.tunnel.state.lock().unwrap().pending_open_requests.is_empty());
    fixture.tunnel.close_with_error(P2pErrorCode::Timeout, "repeat".into());
    assert!(fixture.tunnel.send_command(TunnelPing { seq: 2, send_time: 0 }).await.is_err());
}

#[tokio::test]
async fn dv_logical_control_data_failure_closes_whole_connection() {
    let mut fixture = Fixture::new().await;
    fixture.stop_command().await;
    let purpose = TunnelPurpose::from_value(&81u16).unwrap();
    let result = fixture.tunnel.control_streams.open(purpose).await;
    assert!(result.is_err());
    assert!(fixture.tunnel.closed.load(Ordering::SeqCst), "logical sender must use terminal send boundary");
    assert!(fixture.tunnel.socket.close_reason().is_some());
    let peer_closed = timeout(Duration::from_secs(1), fixture.peer.closed()).await.unwrap();
    assert!(matches!(peer_closed, quinn::ConnectionError::ApplicationClosed(_)));
}

#[tokio::test]
async fn dv_control_receive_eof_closes_connection() {
    let mut fixture = Fixture::new().await;
    fixture.peer_command_write.finish().unwrap();
    timeout(Duration::from_secs(1), fixture.tunnel.socket.closed()).await.unwrap();
    assert!(fixture.tunnel.closed.load(Ordering::SeqCst));
    assert!(fixture.tunnel.socket.close_reason().is_some());
}

#[tokio::test]
async fn integration_command_failure_wakes_borrowed_data_io_after_success() {
    let mut fixture = Fixture::new().await;
    let (mut read, mut write, mut peer_read, mut peer_write) = fixture.data().await;
    peer_write.write_all(b"in").await.unwrap();
    let mut bytes = [0; 2];
    read.read_exact(&mut bytes).await.unwrap();
    assert_eq!(&bytes, b"in");
    write.write_all(b"out").await.unwrap();
    let mut sent = [0; 3];
    peer_read.read_exact(&mut sent).await.unwrap();
    assert_eq!(&sent, b"out");
    let (tx, rx) = oneshot::channel();
    let pending = tokio::spawn(async move {
        use std::future::Future;
        let mut byte = [0];
        let mut future = Box::pin(read.read(&mut byte));
        let mut tx = Some(tx);
        std::future::poll_fn(|cx| {
            let result = future.as_mut().poll(cx);
            if result.is_pending() {
                if let Some(tx) = tx.take() { let _ = tx.send(()); }
            }
            result
        }).await
    });
    timeout(Duration::from_secs(1), rx).await.unwrap().unwrap();
    fixture.stop_command().await;
    assert!(fixture.tunnel.send_command(TunnelPong { seq: 1, send_time: 0 }).await.is_err());
    assert!(timeout(Duration::from_secs(1), pending).await.unwrap().unwrap().is_err());
    assert!(write.write_all(b"stale-data").await.is_err());
}
