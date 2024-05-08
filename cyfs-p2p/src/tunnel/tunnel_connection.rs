use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;
use callback_result::CallbackWaiter;
use cyfs_base::{bucky_time_now, BuckyError, BuckyErrorCode, BuckyResult, DeviceDesc, DeviceId, Endpoint, TailedOwnedData};
use crate::protocol::{AckTunnel, DynamicPackage, PackageCmdCode, SynTunnel};
use crate::sockets::{DataSender, DataSenderFactory, NetManagerRef, TcpExtraParams};
use crate::{IncreaseId, LocalDeviceRef, TempSeq};
use crate::protocol::v0::{AckAckTunnel, TcpAckConnection, TcpSynConnection};

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
pub trait TunnelConnection {
    fn sequence(&self) -> TempSeq;
    async fn connect_tunnel(&mut self) -> BuckyResult<()>;
    async fn connect_stream(&mut self, vport: u16, session_id: IncreaseId) -> BuckyResult<()>;
    async fn send(&self, data: &[u8]) -> BuckyResult<()>;
    async fn recv(&self, data: &[u8]) -> BuckyResult<usize>;
}

pub struct TcpTunnelConnection {
    net_manager: NetManagerRef,
    sequence: TempSeq,
    local_device: LocalDeviceRef,
    remote_desc: DeviceDesc,
    remote_ep: Endpoint,
    conn_timeout: Duration,
    data_sender: Option<Arc<dyn DataSender>>,
    protocol_version: u8,
    stack_version: u32,
    resp_waiter: TunnelWaiterRef,
}

impl TcpTunnelConnection {
    pub fn new(net_manager: NetManagerRef,
               resp_waiter: TunnelWaiterRef,
               sequence: TempSeq,
               local_device: LocalDeviceRef,
               remote_desc: DeviceDesc,
               remote_ep: Endpoint,
               conn_timeout: Duration,
               protocol_version: u8,
               stack_version: u32, ) -> Self {
        Self {
            net_manager,
            sequence,
            local_device,
            remote_desc,
            remote_ep,
            conn_timeout,
            data_sender: None,
            protocol_version,
            stack_version,
            resp_waiter,
        }
    }
}

#[async_trait::async_trait]
impl TunnelConnection for TcpTunnelConnection {
    fn sequence(&self) -> TempSeq {
        self.sequence
    }

    async fn connect_tunnel(&mut self) -> BuckyResult<()> {
        let data_sender = self.net_manager.create_sender(self.local_device.device_id().clone(), self.remote_desc.clone(), self.remote_ep.clone(), TcpExtraParams {
            timeout: self.conn_timeout,
        }).await?;

        let syn = SynTunnel {
            protocol_version: self.protocol_version,
            stack_version: self.stack_version,
            to_device_id: data_sender.remote_device_id().clone(),
            sequence: self.sequence,
            from_device_desc: self.local_device.device().clone(),
            send_time: bucky_time_now(),
        };
        let result_future = self.resp_waiter.create_timeout_result_future(TunnelConnectionKey::new(self.sequence, data_sender.local().clone(), data_sender.remote().clone()), self.conn_timeout);
        data_sender.send_dynamic_pkg(DynamicPackage::from(syn)).await?;
        let result = result_future.await.map_err(|_| BuckyError::from((BuckyErrorCode::Timeout, "syn timeout")))?;
        if result.cmd_code() != PackageCmdCode::AckTunnel {
            return Err(BuckyError::from((BuckyErrorCode::InvalidData, "invalid ack tunnel")));
        }

        let ack: &AckTunnel = result.as_ref();
        if BuckyErrorCode::from(ack.result as u16) != BuckyErrorCode::Ok {
            return Err(BuckyError::from((BuckyErrorCode::InvalidData, "ack tunnel failed")));
        }

        self.data_sender = Some(Arc::new(data_sender));
        Ok(())
    }

    async fn connect_stream(&mut self, vport: u16, session_id: IncreaseId) -> BuckyResult<()> {
        let data_sender = self.net_manager.create_sender(self.local_device.device_id().clone(), self.remote_desc.clone(), self.remote_ep.clone(), TcpExtraParams {
            timeout: self.conn_timeout,
        }).await?;

        let syn = SynTunnel {
            protocol_version: self.protocol_version,
            stack_version: self.stack_version,
            to_device_id: self.data_sender.remote_device_id().clone(),
            sequence: self.sequence,
            from_device_desc: self.local_device.device().clone(),
            send_time: bucky_time_now(),
        };
        let tcp_syn = TcpSynConnection {
            sequence: self.sequence,
            to_vport: 0,
            from_session_id: session_id,
            to_device_id: self.data_sender.remote_device_id().clone(),
            payload: TailedOwnedData::from(Vec::new()),
        };
        let result_future = self.resp_waiter.create_timeout_result_future(TunnelConnectionKey::new(self.sequence, data_sender.local().clone(), data_sender.remote().clone()), self.conn_timeout);
        data_sender.send_dynamic_pkgs(vec![DynamicPackage::from(syn), DynamicPackage::from(tcp_syn)]).await?;
        let result = result_future.await.map_err(|_| BuckyError::from((BuckyErrorCode::Timeout, "syn timeout")))?;
        if result.cmd_code() != PackageCmdCode::AckTunnel {
            return Err(BuckyError::from((BuckyErrorCode::InvalidData, "invalid ack tunnel")));
        }

        let ack: &AckTunnel = result.as_ref();
        if BuckyErrorCode::from(ack.result as u16) != BuckyErrorCode::Ok {
            return Err(BuckyError::from((BuckyErrorCode::InvalidData, "ack tunnel failed")));
        }

        let result_future = self.resp_waiter.create_timeout_result_future(TunnelConnectionKey::new(self.sequence, data_sender.local().clone(), data_sender.remote().clone()), self.conn_timeout);
        let result = result_future.await.map_err(|_| BuckyError::from((BuckyErrorCode::Timeout, "syn timeout")))?;
        if result.cmd_code() != PackageCmdCode::TcpAckConnection {
            return Err(BuckyError::from((BuckyErrorCode::InvalidData, "invalid tcp ack tunnel")));
        }

        let ack: &TcpAckConnection = result.as_ref();
        if BuckyErrorCode::from(ack.result as u16) != BuckyErrorCode::Ok {
            return Err(BuckyError::from((BuckyErrorCode::InvalidData, "tcp ack tunnel failed")));
        }

        let ackack = AckAckTunnel {

        };
        Ok(())
    }

    async fn send(&self, data: &[u8]) -> BuckyResult<()> {
        todo!()
    }

    async fn recv(&self, data: &[u8]) -> BuckyResult<usize> {
        todo!()
    }
}
