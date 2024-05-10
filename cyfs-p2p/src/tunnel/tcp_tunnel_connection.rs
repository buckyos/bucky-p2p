use std::sync::Arc;
use std::time::Duration;
use cyfs_base::{bucky_time_now, BuckyError, BuckyErrorCode, BuckyResult, DeviceDesc, Endpoint, TailedOwnedData};
use crate::sockets::{DataSender, DataSenderFactory, NetManagerRef, TcpExtraParams};
use crate::{IncreaseId, LocalDeviceRef, TempSeq};
use crate::history::keystore::{EncryptedKey, Keystore};
use crate::protocol::{AckTunnel, DynamicPackage, Exchange, PackageCmdCode, SynTunnel};
use crate::protocol::v0::{AckAckTunnel, TcpAckAckConnection, TcpAckConnection, TcpSynConnection};
use crate::receive_processor::{ReceiveProcessorRef, TCPReceiver};
use crate::tunnel::{connect_stream, connect_tunnel, TunnelConnection, TunnelWaiterRef};
use crate::tunnel::tunnel_connection::TunnelConnectionKey;

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
    processor: ReceiveProcessorRef,
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
               stack_version: u32,
               data_sender: Option<Arc<dyn DataSender>>,
               processor: ReceiveProcessorRef,) -> Self {
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
            resp_waiter,
            processor,
        }
    }
}

#[async_trait::async_trait]
impl TunnelConnection for TcpTunnelConnection {
    fn sequence(&self) -> TempSeq {
        self.sequence
    }

    async fn connect_tunnel(&mut self) -> BuckyResult<()> {
        if self.data_sender.is_some() {
            return Ok(());
        }
        let data_sender = self.net_manager.create_sender(
            self.local_device.device_id().clone(),
            self.remote_desc.clone(),
            self.remote_ep.clone(),
            TcpExtraParams {
                timeout: self.conn_timeout,
            }).await?;
        let data_sender = TCPReceiver::new(Arc::new(data_sender), self.processor.clone(), self.net_manager.key_store().clone());

        let key_stub = self.net_manager.key_store().get_key_by_mix_hash(&data_sender.key().mix_hash(), false, false);
        if key_stub.is_none() {
            return Err(BuckyError::from((BuckyErrorCode::NotFound, "key not found")));
        }
        let key_stub = key_stub.unwrap();
        let mut pkgs = Vec::new();
        let syn = SynTunnel {
            protocol_version: self.protocol_version,
            stack_version: self.stack_version,
            to_device_id: data_sender.remote_device_id().clone(),
            sequence: self.sequence,
            from_device_desc: self.local_device.device().clone(),
            send_time: bucky_time_now(),
        };
        log::info!("send key {}", key_stub.key);
        if let EncryptedKey::Unconfirmed(encrypted) = key_stub.encrypted {
            let mut exchg = Exchange::from((&syn, encrypted, key_stub.key.mix_key));
            exchg.sign(self.net_manager.key_store().signer(self.local_device.device_id()).as_ref().unwrap()).await?;
            pkgs.push(DynamicPackage::from(exchg));
        }
        pkgs.push(DynamicPackage::from(syn));
        let result_future = self.resp_waiter.create_timeout_result_future(
            TunnelConnectionKey::new(self.sequence, data_sender.local().clone(), data_sender.remote().clone()), self.conn_timeout);
        data_sender.send_dynamic_pkgs(pkgs).await?;
        let result = result_future.await.map_err(|_| BuckyError::from((BuckyErrorCode::Timeout, "syn timeout")))?;
        if result.cmd_code() != PackageCmdCode::AckTunnel {
            return Err(BuckyError::from((BuckyErrorCode::InvalidData, "invalid ack tunnel")));
        }

        let ack: &AckTunnel = result.as_ref();
        if BuckyErrorCode::from(ack.result as u16) != BuckyErrorCode::Ok {
            return Err(BuckyError::from((BuckyErrorCode::InvalidData, "ack tunnel failed")));
        }

        let syn_ack_ack = AckAckTunnel {
            seq: self.sequence,
        };
        data_sender.send_dynamic_pkg(DynamicPackage::from(syn_ack_ack)).await?;

        self.data_sender = Some(data_sender);
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
            TcpExtraParams {
                timeout: self.conn_timeout,
            }).await?;
        let data_sender = TCPReceiver::new(Arc::new(data_sender), self.processor.clone(), self.net_manager.key_store().clone());

        let key_stub = self.net_manager.key_store().get_key_by_mix_hash(&data_sender.key().mix_hash(), false, false);
        if key_stub.is_none() {
            return Err(BuckyError::from((BuckyErrorCode::NotFound, "key not found")));
        }
        let key_stub = key_stub.unwrap();
        let mut pkgs = Vec::new();

        let syn = SynTunnel {
            protocol_version: self.protocol_version,
            stack_version: self.stack_version,
            to_device_id: data_sender.remote_device_id().clone(),
            sequence: self.sequence,
            from_device_desc: self.local_device.device().clone(),
            send_time: bucky_time_now(),
        };
        if let EncryptedKey::Unconfirmed(encrypted) = key_stub.encrypted {
            let mut exchg = Exchange::from((&syn, encrypted, key_stub.key.mix_key));
            exchg.sign(self.net_manager.key_store().signer(self.local_device.device_id()).as_ref().unwrap()).await?;
            pkgs.push(DynamicPackage::from(exchg));
        }
        pkgs.push(DynamicPackage::from(syn));
        let tcp_syn = TcpSynConnection {
            sequence: self.sequence,
            to_vport: 0,
            from_session_id: session_id,
            to_device_id: data_sender.remote_device_id().clone(),
            payload: TailedOwnedData::from(Vec::new()),
        };
        pkgs.push(DynamicPackage::from(tcp_syn));

        let result_future = self.resp_waiter.create_timeout_result_future(
            TunnelConnectionKey::new(self.sequence, data_sender.local().clone(), data_sender.remote().clone()), self.conn_timeout);
        data_sender.send_dynamic_pkgs(pkgs).await?;
        let result = result_future.await.map_err(|_| BuckyError::from((BuckyErrorCode::Timeout, "syn timeout")))?;
        if result.cmd_code() != PackageCmdCode::AckTunnel {
            return Err(BuckyError::from((BuckyErrorCode::InvalidData, "invalid ack tunnel")));
        }

        let ack: &AckTunnel = result.as_ref();
        if BuckyErrorCode::from(ack.result as u16) != BuckyErrorCode::Ok {
            return Err(BuckyError::from((BuckyErrorCode::InvalidData, "ack tunnel failed")));
        }

        let result_future = self.resp_waiter.create_timeout_result_future(
            TunnelConnectionKey::new(self.sequence, data_sender.local().clone(), data_sender.remote().clone()), self.conn_timeout);
        let result = result_future.await.map_err(|_| BuckyError::from((BuckyErrorCode::Timeout, "syn timeout")))?;
        if result.cmd_code() != PackageCmdCode::TcpAckConnection {
            return Err(BuckyError::from((BuckyErrorCode::InvalidData, "invalid tcp ack tunnel")));
        }

        let ack: &TcpAckConnection = result.as_ref();
        if BuckyErrorCode::from(ack.result as u16) != BuckyErrorCode::Ok {
            return Err(BuckyError::from((BuckyErrorCode::InvalidData, "tcp ack tunnel failed")));
        }

        let syn_ack_ack = AckAckTunnel {
            seq: self.sequence
        };

        let tcp_ack_ack = TcpAckAckConnection {
            sequence: self.sequence,
            result: 0,
        };

        data_sender.send_dynamic_pkgs(vec![DynamicPackage::from(syn_ack_ack), DynamicPackage::from(tcp_ack_ack)]).await?;

        self.data_sender = Some(data_sender);

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
