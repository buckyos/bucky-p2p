use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;
use as_any::AsAny;
use callback_result::{CallbackWaiter, SingleCallbackWaiter};
use cyfs_base::{bucky_time_now, BuckyError, BuckyErrorCode, Device, DeviceDesc, DeviceId, Endpoint, TailedOwnedData};
use notify_future::NotifyFuture;
use crate::protocol::{AckTunnel, DynamicPackage, PackageBox, PackageCmdCode, SynTunnel};
use crate::sockets::{DataSender, DataSenderFactory, NetManagerRef, SocketType, TcpExtraParams};
use crate::{IncreaseId, LocalDeviceRef, MixAesKey, TempSeq};
use crate::error::{bdt_err, BdtErrorCode, BdtResult, into_bdt_err};
use crate::history::keystore::{FoundKey, Keystore};
use crate::protocol::v0::{AckAckTunnel, TcpAckAckConnection, TcpAckConnection, TcpSynConnection};

#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) struct TunnelConnectionKey {
    seq: TempSeq,
    local_id: DeviceId,
    remote_id: DeviceId,
}

impl TunnelConnectionKey {
    pub fn new(seq: TempSeq, local_id: DeviceId, remote_id: DeviceId) -> Self {
        Self {
            seq,
            local_id,
            remote_id,
        }
    }
}

impl Hash for TunnelConnectionKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u32(self.seq.value());
        state.write(self.local_id.as_ref().as_slice());
        state.write(self.remote_id.as_ref().as_slice());
    }
}

#[derive(Debug, Clone, Copy, Ord, PartialOrd, PartialEq, Eq)]
pub enum TunnelType {
    TUNNEL,
    STREAM(u16),
}

impl TunnelType {
    pub fn is_tunnel(&self) -> bool {
        matches!(self, TunnelType::TUNNEL)
    }

    pub fn is_stream(&self) -> bool {
        matches!(self, TunnelType::STREAM(_))
    }

    pub fn get_vport(&self) -> Option<u16> {
        match self {
            TunnelType::STREAM(vport) => Some(*vport),
            _ => None,
        }
    }
}

#[async_trait::async_trait]
pub trait TunnelConnection: AsAny + Send + Sync + 'static {
    fn sequence(&self) -> TempSeq;
    fn socket_type(&self) -> SocketType;
    fn tunnel_type(&self) -> TunnelType;
    async fn connect_tunnel(&mut self) -> BdtResult<()>;
    async fn connect_stream(&mut self, vport: u16, session_id: IncreaseId) -> BdtResult<()>;
}
//
// pub(crate) async fn connect_tunnel<T: DataSender>(
//     data_sender: &T,
//     resp_waiter: &TunnelDataReceiverRef,
//     protocol_version: u8,
//     stack_version: u32,
//     sequence: TempSeq,
//     from_device_desc: Device,
//     conn_timeout: Duration) -> BdtResult<()> {
//     let syn = SynTunnel {
//         protocol_version,
//         stack_version,
//         to_device_id: data_sender.remote_device_id().clone(),
//         sequence,
//         from_device_desc,
//         send_time: bucky_time_now(),
//     };
//     let result_future = resp_waiter.create_timeout_result_future(
//         TunnelConnectionKey::new(sequence, data_sender.local().clone(), data_sender.remote().clone()), conn_timeout);
//     data_sender.send_dynamic_pkg(DynamicPackage::from(syn)).await?;
//     let result = result_future.await.map_err(|_| BuckyError::from((BuckyErrorCode::Timeout, "syn timeout")))?;
//     if result.cmd_code() != PackageCmdCode::AckTunnel {
//         return Err(BuckyError::from((BuckyErrorCode::InvalidData, "invalid ack tunnel")));
//     }
//
//     let ack: &AckTunnel = result.as_ref();
//     if BuckyErrorCode::from(ack.result as u16) != BuckyErrorCode::Ok {
//         return Err(BuckyError::from((BuckyErrorCode::InvalidData, "ack tunnel failed")));
//     }
//
//     let syn_ack_ack = AckAckTunnel {
//         seq: sequence,
//     };
//     data_sender.send_dynamic_pkg(DynamicPackage::from(syn_ack_ack)).await?;
//
//     Ok(())
// }
//
// pub(crate) async fn connect_stream<T: DataSender>(
//     data_sender: &T,
//     resp_waiter: &TunnelDataReceiverRef,
//     protocol_version: u8,
//     stack_version: u32,
//     sequence: TempSeq,
//     from_device_desc: Device,
//     to_vport: u16,
//     session_id: IncreaseId,
//     conn_timeout: Duration) -> BdtResult<()> {
//
//     let syn = SynTunnel {
//         protocol_version,
//         stack_version,
//         to_device_id: data_sender.remote_device_id().clone(),
//         sequence,
//         from_device_desc,
//         send_time: bucky_time_now(),
//     };
//     let tcp_syn = TcpSynConnection {
//         sequence,
//         to_vport: 0,
//         from_session_id: session_id,
//         to_device_id: data_sender.remote_device_id().clone(),
//         payload: TailedOwnedData::from(Vec::new()),
//     };
//     let result_future = resp_waiter.create_timeout_result_future(
//         TunnelConnectionKey::new(sequence, data_sender.local().clone(), data_sender.remote().clone()), conn_timeout);
//     data_sender.send_dynamic_pkgs(vec![DynamicPackage::from(syn), DynamicPackage::from(tcp_syn)]).await?;
//     let result = result_future.await.map_err(|_| BuckyError::from((BuckyErrorCode::Timeout, "syn timeout")))?;
//     if result.cmd_code() != PackageCmdCode::AckTunnel {
//         return Err(BuckyError::from((BuckyErrorCode::InvalidData, "invalid ack tunnel")));
//     }
//
//     let ack: &AckTunnel = result.as_ref();
//     if BuckyErrorCode::from(ack.result as u16) != BuckyErrorCode::Ok {
//         return Err(BuckyError::from((BuckyErrorCode::InvalidData, "ack tunnel failed")));
//     }
//
//     let result_future = resp_waiter.create_timeout_result_future(
//         TunnelConnectionKey::new(sequence, data_sender.local().clone(), data_sender.remote().clone()), conn_timeout);
//     let result = result_future.await.map_err(|_| BuckyError::from((BuckyErrorCode::Timeout, "syn timeout")))?;
//     if result.cmd_code() != PackageCmdCode::TcpAckConnection {
//         return Err(BuckyError::from((BuckyErrorCode::InvalidData, "invalid tcp ack tunnel")));
//     }
//
//     let ack: &TcpAckConnection = result.as_ref();
//     if BuckyErrorCode::from(ack.result as u16) != BuckyErrorCode::Ok {
//         return Err(BuckyError::from((BuckyErrorCode::InvalidData, "tcp ack tunnel failed")));
//     }
//
//     let syn_ack_ack = AckAckTunnel {
//         seq: sequence
//     };
//
//     let tcp_ack_ack = TcpAckAckConnection {
//         sequence,
//         result: 0,
//     };
//
//     data_sender.send_dynamic_pkgs(vec![DynamicPackage::from(syn_ack_ack), DynamicPackage::from(tcp_ack_ack)]).await?;
//
//     Ok(())
// }
