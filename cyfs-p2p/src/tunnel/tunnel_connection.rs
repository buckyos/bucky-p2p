use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;
use callback_result::CallbackWaiter;
use cyfs_base::{bucky_time_now, BuckyError, BuckyErrorCode, BuckyResult, Device, DeviceDesc, DeviceId, Endpoint, TailedOwnedData};
use crate::protocol::{AckTunnel, DynamicPackage, PackageCmdCode, SynTunnel};
use crate::sockets::{DataSender, DataSenderFactory, NetManagerRef, TcpExtraParams};
use crate::{IncreaseId, LocalDeviceRef, TempSeq};
use crate::history::keystore::{FoundKey, Keystore};
use crate::protocol::v0::{AckAckTunnel, TcpAckAckConnection, TcpAckConnection, TcpSynConnection};

#[derive(Clone, Eq, PartialEq)]
pub(super) struct TunnelConnectionKey {
    seq: TempSeq,
    local_ep: Endpoint,
    remote_ep: Endpoint,
}

impl TunnelConnectionKey {
    pub fn new(seq: TempSeq, local_ep: Endpoint, remote_ep: Endpoint) -> Self {
        Self {
            seq,
            local_ep,
            remote_ep,
        }
    }
}

impl Hash for TunnelConnectionKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u32(self.seq.value());
        state.write(self.local_ep.to_string().as_bytes());
        state.write(self.remote_ep.to_string().as_bytes());
    }
}

pub type TunnelWaiter = CallbackWaiter<TunnelConnectionKey, DynamicPackage>;
pub type TunnelWaiterRef = Arc<TunnelWaiter>;

#[async_trait::async_trait]
pub trait TunnelConnection: Send + Sync + 'static {
    fn sequence(&self) -> TempSeq;
    async fn connect_tunnel(&mut self) -> BuckyResult<()>;
    async fn connect_stream(&mut self, vport: u16, session_id: IncreaseId) -> BuckyResult<()>;
    async fn send(&self, data: &[u8]) -> BuckyResult<()>;
    async fn recv(&self, data: &[u8]) -> BuckyResult<usize>;
    async fn recv_pkg(&self) -> BuckyResult<DynamicPackage>;
}

pub(crate) async fn connect_tunnel<T: DataSender>(
    data_sender: &T,
    resp_waiter: &TunnelWaiterRef,
    protocol_version: u8,
    stack_version: u32,
    sequence: TempSeq,
    from_device_desc: Device,
    conn_timeout: Duration) -> BuckyResult<()> {
    let syn = SynTunnel {
        protocol_version,
        stack_version,
        to_device_id: data_sender.remote_device_id().clone(),
        sequence,
        from_device_desc,
        send_time: bucky_time_now(),
    };
    let result_future = resp_waiter.create_timeout_result_future(
        TunnelConnectionKey::new(sequence, data_sender.local().clone(), data_sender.remote().clone()), conn_timeout);
    data_sender.send_dynamic_pkg(DynamicPackage::from(syn)).await?;
    let result = result_future.await.map_err(|_| BuckyError::from((BuckyErrorCode::Timeout, "syn timeout")))?;
    if result.cmd_code() != PackageCmdCode::AckTunnel {
        return Err(BuckyError::from((BuckyErrorCode::InvalidData, "invalid ack tunnel")));
    }

    let ack: &AckTunnel = result.as_ref();
    if BuckyErrorCode::from(ack.result as u16) != BuckyErrorCode::Ok {
        return Err(BuckyError::from((BuckyErrorCode::InvalidData, "ack tunnel failed")));
    }

    let syn_ack_ack = AckAckTunnel {
        seq: sequence,
    };
    data_sender.send_dynamic_pkg(DynamicPackage::from(syn_ack_ack)).await?;

    Ok(())
}

pub(crate) async fn connect_stream<T: DataSender>(
    data_sender: &T,
    resp_waiter: &TunnelWaiterRef,
    protocol_version: u8,
    stack_version: u32,
    sequence: TempSeq,
    from_device_desc: Device,
    to_vport: u16,
    session_id: IncreaseId,
    conn_timeout: Duration) -> BuckyResult<()> {

    let syn = SynTunnel {
        protocol_version,
        stack_version,
        to_device_id: data_sender.remote_device_id().clone(),
        sequence,
        from_device_desc,
        send_time: bucky_time_now(),
    };
    let tcp_syn = TcpSynConnection {
        sequence,
        to_vport: 0,
        from_session_id: session_id,
        to_device_id: data_sender.remote_device_id().clone(),
        payload: TailedOwnedData::from(Vec::new()),
    };
    let result_future = resp_waiter.create_timeout_result_future(
        TunnelConnectionKey::new(sequence, data_sender.local().clone(), data_sender.remote().clone()), conn_timeout);
    data_sender.send_dynamic_pkgs(vec![DynamicPackage::from(syn), DynamicPackage::from(tcp_syn)]).await?;
    let result = result_future.await.map_err(|_| BuckyError::from((BuckyErrorCode::Timeout, "syn timeout")))?;
    if result.cmd_code() != PackageCmdCode::AckTunnel {
        return Err(BuckyError::from((BuckyErrorCode::InvalidData, "invalid ack tunnel")));
    }

    let ack: &AckTunnel = result.as_ref();
    if BuckyErrorCode::from(ack.result as u16) != BuckyErrorCode::Ok {
        return Err(BuckyError::from((BuckyErrorCode::InvalidData, "ack tunnel failed")));
    }

    let result_future = resp_waiter.create_timeout_result_future(
        TunnelConnectionKey::new(sequence, data_sender.local().clone(), data_sender.remote().clone()), conn_timeout);
    let result = result_future.await.map_err(|_| BuckyError::from((BuckyErrorCode::Timeout, "syn timeout")))?;
    if result.cmd_code() != PackageCmdCode::TcpAckConnection {
        return Err(BuckyError::from((BuckyErrorCode::InvalidData, "invalid tcp ack tunnel")));
    }

    let ack: &TcpAckConnection = result.as_ref();
    if BuckyErrorCode::from(ack.result as u16) != BuckyErrorCode::Ok {
        return Err(BuckyError::from((BuckyErrorCode::InvalidData, "tcp ack tunnel failed")));
    }

    let syn_ack_ack = AckAckTunnel {
        seq: sequence
    };

    let tcp_ack_ack = TcpAckAckConnection {
        sequence,
        result: 0,
    };

    data_sender.send_dynamic_pkgs(vec![DynamicPackage::from(syn_ack_ack), DynamicPackage::from(tcp_ack_ack)]).await?;

    Ok(())
}
