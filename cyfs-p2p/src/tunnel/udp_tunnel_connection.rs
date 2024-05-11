use std::sync::Arc;
use std::time::Duration;
use cyfs_base::{bucky_time_now, BuckyError, BuckyErrorCode, BuckyResult, DeviceDesc, Endpoint};
use crate::sockets::{DataSender, DataSenderFactory, NetManagerRef, SocketType, UdpExtraParams};
use crate::{IncreaseId, LocalDeviceRef, TempSeq};
use crate::protocol::{AckTunnel, DynamicPackage, PackageCmdCode, SynTunnel};
use crate::protocol::v0::PingTunnel;
use crate::tunnel::{TunnelConnection, TunnelDataReceiverRef, TunnelType};
use crate::tunnel::tunnel_connection::TunnelConnectionKey;

pub struct UdpTunnelConnection {
    net_manager: NetManagerRef,
    sequence: TempSeq,
    local_device: LocalDeviceRef,
    remote_desc: DeviceDesc,
    remote_ep: Endpoint,
    conn_timeout: Duration,
    data_sender: Option<Arc<dyn DataSender>>,
    protocol_version: u8,
    stack_version: u32,
    data_receiver: TunnelDataReceiverRef,
    local_ep: Endpoint,
}

impl UdpTunnelConnection {
    pub fn new(net_manager: NetManagerRef,
               data_receiver: TunnelDataReceiverRef,
               sequence: TempSeq,
               local_device: LocalDeviceRef,
               remote_desc: DeviceDesc,
               remote_ep: Endpoint,
               conn_timeout: Duration,
               protocol_version: u8,
               stack_version: u32,
               local_ep: Endpoint,
               data_sender: Option<Arc<dyn DataSender>>) -> Self {
        Self {
            net_manager,
            sequence,
            local_device,
            remote_desc,
            remote_ep,
            conn_timeout,
            data_sender,
            protocol_version,
            stack_version,
            data_receiver,
            local_ep,
        }
    }

    pub async fn ping(&self) -> BuckyResult<()> {
        let ping = PingTunnel {
            package_id: self.sequence.value(),
            send_time: bucky_time_now(),
            recv_data: 0,
        };

        // let result_future = self.data_receiver.create_timeout_result_future(
        //     TunnelConnectionKey::new(self.sequence, self.data_sender.as_ref().unwrap().local().clone(), self.data_sender.as_ref().unwrap().remote().clone()), self.conn_timeout);
        // self.data_sender.as_ref().unwrap().send_dynamic_pkg(DynamicPackage::from(ping)).await?;
        // let result = result_future.await.map_err(|_| BuckyError::from((BuckyErrorCode::Timeout, "ping timeout")))?;
        // if result.cmd_code() != PackageCmdCode::PingTunnelResp {
        //     return Err(BuckyError::from((BuckyErrorCode::InvalidData, "invalid ping tunnel resp")));
        // }
        Ok(())
    }
}

#[async_trait::async_trait]
impl TunnelConnection for UdpTunnelConnection {
    fn sequence(&self) -> TempSeq {
        self.sequence
    }

    fn socket_type(&self) -> SocketType {
        todo!()
    }

    fn tunnel_type(&self) -> TunnelType {
        todo!()
    }

    async fn accept_tunnel(&mut self) -> BuckyResult<()> {
        todo!()
    }

    async fn connect_tunnel(&mut self) -> BuckyResult<()> {
        if self.data_sender.is_some() {
            return Ok(());
        }
        let data_sender = self.net_manager.create_sender(
            self.local_device.device_id().clone(),
            self.remote_desc.clone(),
            self.remote_ep.clone(),
            UdpExtraParams {
                local_ep: self.local_ep.clone(),
            }).await?;
        //
        // connect_tunnel(
        //     &data_sender,
        //     &self.data_receiver,
        //     self.protocol_version,
        //     self.stack_version,
        //     self.sequence,
        //     self.local_device.device().clone(),
        //     self.conn_timeout).await?;
        //
        // self.data_sender = Some(Arc::new(data_sender));

        Ok(())
    }

    async fn connect_stream(&mut self, vport: u16, session_id: IncreaseId) -> BuckyResult<()> {
        if self.data_sender.is_some() {
            return Ok(());
        }
        let data_sender = self.net_manager.create_sender(
            self.local_device.device_id().clone(),
            self.remote_desc.clone(),
            self.remote_ep.clone(),
            UdpExtraParams {
                local_ep: self.local_ep.clone(),
            }).await?;

        // connect_stream(
        //     &data_sender,
        //     &self.data_receiver,
        //     self.protocol_version,
        //     self.stack_version,
        //     self.sequence,
        //     self.local_device.device().clone(),
        //     vport,
        //     session_id,
        //     self.conn_timeout).await?;

        self.data_sender = Some(Arc::new(data_sender));

        Ok(())
    }

    async fn send(&self, data: &[u8]) -> BuckyResult<()> {
        todo!()
    }

    async fn recv(&self, data: &[u8]) -> BuckyResult<usize> {
        todo!()
    }

    async fn recv_pkg(&self) -> BuckyResult<DynamicPackage> {
        todo!()
    }
}
