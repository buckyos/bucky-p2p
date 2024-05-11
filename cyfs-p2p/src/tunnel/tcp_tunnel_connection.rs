use std::sync::Arc;
use std::time::Duration;
use cyfs_base::{bucky_time_now, BuckyError, BuckyErrorCode, BuckyResult, DeviceDesc, Endpoint, TailedOwnedData};
use crate::sockets::{DataSender, DataSenderFactory, NetManagerRef, SocketType, TcpExtraParams};
use crate::{IncreaseId, LocalDeviceRef, TempSeq};
use crate::history::keystore::{EncryptedKey, Keystore};
use crate::protocol::{AckTunnel, DynamicPackage, Exchange, MTU, PackageCmdCode, SynTunnel};
use crate::protocol::v0::{AckAckTunnel, TcpAckAckConnection, TcpAckConnection, TcpSynConnection};
use crate::receive_processor::{ReceiveProcessorRef, TCPReceiver};
use crate::tunnel::{TunnelConnection, TunnelSocketReceiver, TunnelSocket, TunnelDataReceiverRef, TunnelDataReceiver, TunnelType};
use crate::tunnel::tunnel_connection::TunnelConnectionKey;

pub struct TcpTunnelConnection {
    net_manager: NetManagerRef,
    sequence: TempSeq,
    local_device: LocalDeviceRef,
    remote_desc: DeviceDesc,
    remote_ep: Endpoint,
    conn_timeout: Duration,
    data_socket: Option<Arc<TunnelSocket>>,
    protocol_version: u8,
    stack_version: u32,
    data_receiver: TunnelDataReceiverRef,
    processor: ReceiveProcessorRef,
    tunnel_type: TunnelType,
}

impl TcpTunnelConnection {
    pub fn new(net_manager: NetManagerRef,
               data_receiver: TunnelDataReceiverRef,
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
            data_socket: data_sender.map(|v| TunnelSocket::new(data_receiver.clone(), sequence, v)),
            protocol_version,
            stack_version,
            data_receiver,
            processor,
            tunnel_type: TunnelType::TUNNEL,
        }
    }
}

#[async_trait::async_trait]
impl TunnelConnection for TcpTunnelConnection {
    fn sequence(&self) -> TempSeq {
        self.sequence
    }

    fn socket_type(&self) -> SocketType {
        self.data_socket.as_ref().unwrap().socket_type()
    }

    fn tunnel_type(&self) -> TunnelType {
        self.tunnel_type
    }

    async fn accept_tunnel(&mut self) -> BuckyResult<()> {
        let result = self.data_socket.as_ref().unwrap().recv_resp(self.conn_timeout).await?;
        if result.cmd_code() != PackageCmdCode::SynTunnel {
            return Err(BuckyError::from((BuckyErrorCode::InvalidData, "invalid syn tunnel")));
        }
        let req: &SynTunnel = result.as_ref();
        let ack = AckTunnel {
            protocol_version: self.protocol_version,
            stack_version: self.stack_version,
            sequence: req.sequence,
            result: 0,
            send_time: bucky_time_now(),
            mtu: MTU as u16,
            to_device_desc: self.local_device.device().clone(),
        };
        self.data_socket.as_ref().unwrap().send_dynamic_pkg(DynamicPackage::from(ack)).await?;

        let result = self.data_socket.as_ref().unwrap().recv_resp(self.conn_timeout).await?;
        if result.cmd_code() == PackageCmdCode::AckAckTunnel {
            self.tunnel_type = TunnelType::TUNNEL;
            return Ok(());
        } else if result.cmd_code() == PackageCmdCode::TcpSynConnection {
            let syn: &TcpSynConnection = result.as_ref();
            let ack = TcpAckConnection {
                sequence: self.sequence,
                to_session_id: syn.from_session_id,
                result: 0,
                payload: TailedOwnedData::from(Vec::new()),
            };
            self.data_socket.as_ref().unwrap().send_dynamic_pkg(DynamicPackage::from(ack)).await?;

            let result = self.data_socket.as_ref().unwrap().recv_resp(self.conn_timeout).await?;
            if result.cmd_code() != PackageCmdCode::AckAckTunnel {
                return Err(BuckyError::from((BuckyErrorCode::InvalidData, "invalid syn tunnel")));
            }

            let result = self.data_socket.as_ref().unwrap().recv_resp(self.conn_timeout).await?;
            if result.cmd_code() != PackageCmdCode::TcpAckAckConnection {
                return Err(BuckyError::from((BuckyErrorCode::InvalidData, "invalid syn tunnel")));
            }
            self.tunnel_type = TunnelType::STREAM;
            Ok(())
        } else {
            return Err(BuckyError::from((BuckyErrorCode::InvalidData, "invalid ack tunnel")));
        }
    }

    async fn connect_tunnel(&mut self) -> BuckyResult<()> {
        if self.data_socket.is_some() {
            return Ok(());
        }
        let data_sender = self.net_manager.create_sender(
            self.local_device.device_id().clone(),
            self.remote_desc.clone(),
            self.remote_ep.clone(),
            TcpExtraParams {
                timeout: self.conn_timeout,
            }).await?;
        let data_socket = TunnelSocket::new(self.data_receiver.clone(), self.sequence,
                                            TCPReceiver::new(Arc::new(data_sender), self.processor.clone(), self.net_manager.key_store().clone()));

        let key_stub = self.net_manager.key_store().get_key_by_mix_hash(&data_socket.key().mix_hash(), false, false);
        if key_stub.is_none() {
            return Err(BuckyError::from((BuckyErrorCode::NotFound, "key not found")));
        }
        let key_stub = key_stub.unwrap();
        let mut pkgs = Vec::new();
        let syn = SynTunnel {
            protocol_version: self.protocol_version,
            stack_version: self.stack_version,
            to_device_id: data_socket.remote_device_id().clone(),
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
        data_socket.send_dynamic_pkgs(pkgs).await?;
        let result = data_socket.recv_resp(self.conn_timeout).await?;
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
        data_socket.send_dynamic_pkg(DynamicPackage::from(syn_ack_ack)).await?;

        self.data_socket = Some(data_socket);
        Ok(())
    }

    async fn connect_stream(&mut self, vport: u16, session_id: IncreaseId) -> BuckyResult<()> {
        if self.data_socket.is_some() {
            return Ok(());
        }
        let data_sender = self.net_manager.create_sender(
            self.local_device.device_id().clone(),
            self.remote_desc.clone(),
            self.remote_ep.clone(),
            TcpExtraParams {
                timeout: self.conn_timeout,
            }).await?;
        let data_socket = TunnelSocket::new(self.data_receiver.clone(), self.sequence,
                                            TCPReceiver::new(Arc::new(data_sender), self.processor.clone(), self.net_manager.key_store().clone()));

        let key_stub = self.net_manager.key_store().get_key_by_mix_hash(&data_socket.key().mix_hash(), false, false);
        if key_stub.is_none() {
            return Err(BuckyError::from((BuckyErrorCode::NotFound, "key not found")));
        }
        let key_stub = key_stub.unwrap();
        let mut pkgs = Vec::new();

        let syn = SynTunnel {
            protocol_version: self.protocol_version,
            stack_version: self.stack_version,
            to_device_id: data_socket.remote_device_id().clone(),
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
            to_device_id: data_socket.remote_device_id().clone(),
            payload: TailedOwnedData::from(Vec::new()),
        };
        pkgs.push(DynamicPackage::from(tcp_syn));

        data_socket.send_dynamic_pkgs(pkgs).await?;
        let result = data_socket.recv_resp(self.conn_timeout).await?;
        if result.cmd_code() != PackageCmdCode::AckTunnel {
            return Err(BuckyError::from((BuckyErrorCode::InvalidData, "invalid ack tunnel")));
        }

        let ack: &AckTunnel = result.as_ref();
        if BuckyErrorCode::from(ack.result as u16) != BuckyErrorCode::Ok {
            return Err(BuckyError::from((BuckyErrorCode::InvalidData, "ack tunnel failed")));
        }

        let syn_ack_ack = AckAckTunnel {
            seq: self.sequence
        };
        data_socket.send_dynamic_pkg(DynamicPackage::from(syn_ack_ack)).await?;

        let result = data_socket.recv_resp(self.conn_timeout).await?;
        if result.cmd_code() != PackageCmdCode::TcpAckConnection {
            return Err(BuckyError::from((BuckyErrorCode::InvalidData, "invalid tcp ack tunnel")));
        }

        let ack: &TcpAckConnection = result.as_ref();
        if BuckyErrorCode::from(ack.result as u16) != BuckyErrorCode::Ok {
            return Err(BuckyError::from((BuckyErrorCode::InvalidData, "tcp ack tunnel failed")));
        }

        let tcp_ack_ack = TcpAckAckConnection {
            sequence: self.sequence,
            result: 0,
        };
        data_socket.send_dynamic_pkg(DynamicPackage::from(tcp_ack_ack)).await?;

        self.data_socket = Some(data_socket);

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
