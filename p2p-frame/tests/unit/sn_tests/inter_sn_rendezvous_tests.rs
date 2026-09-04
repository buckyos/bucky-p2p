use crate::sn::protocol::{
    SN_TUNNEL_RENDEZVOUS_CMD_VERSION, SnTunnelRendezvousNotify, SnTunnelRendezvousOperation,
    SnTunnelRendezvousResp,
};
use crate::sn::types::OwnerCmdPkgLen;
use crate::types::{Sequence, TunnelId};

struct RendezvousRelayPeer {
    local_sn_id: P2pId,
    requests: Mutex<Vec<(P2pId, P2pId, SnTunnelRendezvousNotify)>>,
}

impl RendezvousRelayPeer {
    fn new(local_sn_id: P2pId) -> Arc<Self> {
        Arc::new(Self {
            local_sn_id,
            requests: Mutex::new(Vec::new()),
        })
    }
}

#[async_trait]
impl InterSnPeer for RendezvousRelayPeer {
    fn sn_id(&self) -> Option<P2pId> {
        Some(self.local_sn_id.clone())
    }

    async fn relay_rendezvous_from_sn(
        &self,
        remote_sn_id: P2pId,
        target_peer_id: P2pId,
        notify: SnTunnelRendezvousNotify,
    ) -> P2pResult<SnTunnelRendezvousResp> {
        self.requests
            .lock()
            .unwrap()
            .push((remote_sn_id, target_peer_id, notify.clone()));
        Ok(SnTunnelRendezvousResp::success(notify.seq, Vec::new()))
    }
}

fn relay_notify() -> SnTunnelRendezvousNotify {
    SnTunnelRendezvousNotify {
        seq: Sequence::from(301),
        tunnel_id: TunnelId::from(302),
        peer_info: vec![42; 32],
        operation: SnTunnelRendezvousOperation::WaitIncoming,
        end_point_array: Vec::new(),
        need_predict_endpoint: false,
    }
}

fn owner_header(
    version: u8,
    command: InterSnCommandCode,
    body_len: usize,
) -> CmdHeader<OwnerCmdPkgLen, InterSnCommandCode> {
    CmdHeader::new(
        version,
        false,
        Some(1),
        command,
        OwnerCmdPkgLen::new(body_len as u32).unwrap(),
    )
}

#[tokio::test]
async fn inter_sn_rendezvous_dispatches_explicit_target_and_flat_notify() {
    let local_peer = RendezvousRelayPeer::new(test_id(40));
    let remote_sn_id = test_id(41);
    let target_peer_id = test_id(43);
    let notify = relay_notify();
    let request = InterSnRequest::RelayRendezvous(target_peer_id.clone(), notify.clone());
    assert_eq!(
        inter_sn_command_code(&request),
        InterSnCommandCode::RelayRendezvousV2
    );
    assert_eq!(InterSnCommandCode::RelayRendezvousV2 as u8, 0x86);

    let response = dispatch_owner_cmd(
        local_peer.clone(),
        remote_sn_id.clone(),
        InterSnCommandCode::RelayRendezvousV2,
        request,
    )
    .await;
    assert!(matches!(
        response,
        InterSnResponse::Rendezvous(ref value)
            if value.is_success()
                && value.validate(notify.seq, notify.need_predict_endpoint).is_ok()
    ));
    assert_eq!(
        local_peer.requests.lock().unwrap().as_slice(),
        &[(remote_sn_id, target_peer_id, notify)]
    );
}

#[tokio::test]
async fn inter_sn_rendezvous_rejects_command_payload_mismatch() {
    let target_peer_id = test_id(53);
    let response = dispatch_owner_cmd(
        RendezvousRelayPeer::new(test_id(50)),
        test_id(51),
        InterSnCommandCode::QueryDetail,
        InterSnRequest::RelayRendezvous(target_peer_id, relay_notify()),
    )
    .await;

    assert!(matches!(
        response,
        InterSnResponse::Error(InterSnError {
            code: P2pErrorCode::InvalidData,
            ..
        })
    ));
}

#[tokio::test]
async fn inter_sn_rendezvous_rejects_version_mismatch_and_trailing_bytes() {
    let local_peer = RendezvousRelayPeer::new(test_id(60));
    let remote_sn_id = test_id(61);
    let request = InterSnRequest::RelayRendezvous(test_id(62), relay_notify());
    let encoded = request.to_vec().unwrap();

    let mut version_body = CmdBody::from_bytes(encoded.clone());
    let version_error = handle_owner_cmd(
        local_peer.clone(),
        remote_sn_id.clone(),
        owner_header(0, InterSnCommandCode::RelayRendezvousV2, encoded.len()),
        &mut version_body,
    )
    .await
    .unwrap_err();
    assert_eq!(version_error.code(), CmdErrorCode::InvalidParam);

    let mut with_trailing = encoded;
    with_trailing.push(0xff);
    let mut trailing_body = CmdBody::from_bytes(with_trailing.clone());
    let trailing_error = handle_owner_cmd(
        local_peer.clone(),
        remote_sn_id,
        owner_header(
            SN_TUNNEL_RENDEZVOUS_CMD_VERSION,
            InterSnCommandCode::RelayRendezvousV2,
            with_trailing.len(),
        ),
        &mut trailing_body,
    )
    .await
    .unwrap_err();
    assert_eq!(trailing_error.code(), CmdErrorCode::InvalidParam);
    assert!(local_peer.requests.lock().unwrap().is_empty());
}

#[test]
fn inter_sn_rendezvous_response_rejects_sequence_mismatch() {
    let notify = relay_notify();
    let mismatched =
        SnTunnelRendezvousResp::success(Sequence::from(notify.seq.value() + 1), Vec::new());
    let error = mismatched
        .validate(notify.seq, notify.need_predict_endpoint)
        .unwrap_err();
    assert_eq!(error.code(), P2pErrorCode::Unmatch);
}
