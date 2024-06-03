use std::collections::HashSet;
use std::net::Shutdown;
use std::sync::Arc;
use std::time::Duration;
use async_std::future;
use cyfs_base::{bucky_time_now, DeviceDesc, DeviceId, Endpoint, RawDecode, RawFixedBytes, TailedOwnedData};
use num_traits::FromPrimitive;
use crate::sockets::{DataSender, DataSenderFactory, NetManagerRef, SocketType, TcpExtraParams};
use crate::{IncreaseId, LocalDeviceRef, MixAesKey, TempSeq};
use crate::error::{bdt_err, BdtErrorCode, BdtResult, into_bdt_err};
use crate::history::keystore::{EncryptedKey, Keystore};
use crate::protocol::{AckTunnel, DynamicPackage, Exchange, MTU, MTU_LARGE, OtherBoxTcpDecodeContext, PackageBox, PackageCmdCode, SynTunnel};
use crate::protocol::v0::{AckAckTunnel, TcpAckAckConnection, TcpAckConnection, TcpSynConnection};
use crate::receive_processor::{ReceiveProcessorRef, TCPReceiver};
use crate::sockets::tcp::{RecvBox, TCPSocket};
use crate::tunnel::{TunnelConnection, TunnelType};
use crate::tunnel::tunnel_connection::TunnelConnectionKey;

struct TcpTunnelSocket {
    socket: Arc<TCPSocket>,
}

impl TcpTunnelSocket {
    pub fn new(socket: Arc<TCPSocket>) -> Self {
        Self {
            socket,
        }
    }

    async fn send_dynamic_pkg(&self, dynamic_package: DynamicPackage) -> BdtResult<()> {
        self.socket.send_dynamic_pkg(dynamic_package).await
    }

    async fn send_dynamic_pkgs(&self, dynamic_packages: Vec<DynamicPackage>) -> BdtResult<()> {
        self.socket.send_dynamic_pkgs(dynamic_packages).await
    }

    async fn send_resp(&self, data: &[u8]) -> BdtResult<()> {
        self.socket.send(data).await?;
        Ok(())
    }

    async fn send_pkg_box(&self, pkg: &PackageBox) -> BdtResult<()> {
        self.socket.send_pkg_box(pkg).await
    }

    fn remote(&self) -> &Endpoint {
        self.socket.remote()
    }

    fn local(&self) -> &Endpoint {
        self.socket.local()
    }

    fn remote_device_id(&self) -> &DeviceId {
        self.socket.remote_device_id()
    }

    fn local_device_id(&self) -> &DeviceId {
        self.socket.local_device_id()
    }

    fn key(&self) -> &MixAesKey {
        self.socket.key()
    }

    pub(crate) async fn recv_resp_pkgs(&self, timeout: Duration) -> BdtResult<Vec<DynamicPackage>> {
        let mut recv_buf = [0u8; MTU_LARGE];
        let recv_box = future::timeout(timeout, self.socket.receive_package(&mut recv_buf)).await.map_err(into_bdt_err!(BdtErrorCode::Timeout))??;
        match recv_box {
            RecvBox::Package(pkgs) => {
                Ok(pkgs.into())
            }
            RecvBox::RawData(_) => {
                Err(bdt_err!(BdtErrorCode::InvalidData, "invalid data"))
            }
        }
    }

    async fn recv(&self, buf: &mut [u8]) -> BdtResult<usize> {
        self.socket.recv(buf).await
    }

    async fn recv_exact(&self, buf: &mut [u8]) -> BdtResult<()> {
        self.socket.recv_exact(buf).await
    }

    async fn flush(&self) -> BdtResult<()> {
        self.socket.flush().await
    }

    async fn shutdown(&self, how: Shutdown) -> BdtResult<()> {
        self.socket.shutdown(how).await
    }
}

pub struct TcpTunnelConnection {
    net_manager: NetManagerRef,
    sequence: TempSeq,
    local_device: LocalDeviceRef,
    remote_desc: DeviceDesc,
    remote_ep: Endpoint,
    conn_timeout: Duration,
    data_socket: Option<TcpTunnelSocket>,
    protocol_version: u8,
    stack_version: u32,
    processor: ReceiveProcessorRef,
    tunnel_type: TunnelType,
}

impl TcpTunnelConnection {
    pub(crate) fn new(net_manager: NetManagerRef,
               sequence: TempSeq,
               local_device: LocalDeviceRef,
               remote_desc: DeviceDesc,
               remote_ep: Endpoint,
               conn_timeout: Duration,
               protocol_version: u8,
               stack_version: u32,
               tcp_socket: Option<Arc<TCPSocket>>,
               processor: ReceiveProcessorRef,) -> Self {
        Self {
            net_manager,
            sequence,
            local_device,
            remote_desc,
            remote_ep,
            conn_timeout,
            data_socket: tcp_socket.map(|v| TcpTunnelSocket::new(v)),
            protocol_version,
            stack_version,
            processor,
            tunnel_type: TunnelType::TUNNEL,
        }
    }

    pub(crate) async fn accept_tunnel(&mut self, listen_ports: HashSet<u16>, pkgs: &[DynamicPackage]) -> BdtResult<()> {
        if pkgs.len() == 0 {
            return Err(bdt_err!(BdtErrorCode::InvalidData, "tunnel {:?} invalid syn tunnel, self.sequence", self.sequence));
        }
        let result = pkgs.get(0).unwrap();
        if result.cmd_code() != PackageCmdCode::SynTunnel {
            return Err(bdt_err!(BdtErrorCode::InvalidData, "tunnel {:?} invalid syn tunnel, self.sequence", self.sequence));
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
        if pkgs.len() == 1 {
            self.data_socket.as_ref().unwrap().send_dynamic_pkg(DynamicPackage::from(ack)).await?;
            let resp_pkgs = self.data_socket.as_ref().unwrap().recv_resp_pkgs(self.conn_timeout).await?;
            if resp_pkgs.len() != 1 {
                return Err(bdt_err!(BdtErrorCode::InvalidData, "tunnel {:?} invalid ack tunnel, self.sequence", self.sequence));
            }
            let result = resp_pkgs.get(0).unwrap();
            if result.cmd_code() == PackageCmdCode::AckAckTunnel {
                self.tunnel_type = TunnelType::TUNNEL;
                return Ok(());
            } else {
                return Err(bdt_err!(BdtErrorCode::InvalidData, "tunnel {:?} invalid tunnel, self.sequence", self.sequence));
            }
        } else {
            let result = pkgs.get(1).unwrap();
            if result.cmd_code() == PackageCmdCode::TcpSynConnection {
                let syn: &TcpSynConnection = result.as_ref();
                let mut result = 0;
                if !listen_ports.contains(&syn.to_vport) {
                    log::debug!("unaccepted vport: {}", syn.to_vport);
                    result = BdtErrorCode::ConnectionRefused.into_u8();
                }
                let tcp_ack = TcpAckConnection {
                    sequence: self.sequence,
                    to_session_id: syn.from_session_id,
                    result,
                    payload: TailedOwnedData::from(Vec::new()),
                };
                self.data_socket.as_ref().unwrap().send_dynamic_pkgs(vec![DynamicPackage::from(ack), DynamicPackage::from(tcp_ack)]).await?;
                let resp_pkgs = self.data_socket.as_ref().unwrap().recv_resp_pkgs(self.conn_timeout).await?;
                if resp_pkgs.len() != 2 {
                    return Err(bdt_err!(BdtErrorCode::InvalidData, "tunnel {:?} invalid tunnel resp", self.sequence));
                }

                let result = resp_pkgs.get(0).unwrap();
                if result.cmd_code() != PackageCmdCode::AckAckTunnel {
                    return Err(bdt_err!(BdtErrorCode::InvalidData, "tunnel {:?} invalid tunnel", self.sequence));
                }

                let result = resp_pkgs.get(1).unwrap();
                if result.cmd_code() != PackageCmdCode::TcpAckAckConnection {
                    return Err(bdt_err!(BdtErrorCode::InvalidData, "tunnel {:?} invalid tunnel", self.sequence));
                }
                self.tunnel_type = TunnelType::STREAM(syn.to_vport);
                Ok(())
            } else {
                return Err(bdt_err!(BdtErrorCode::InvalidData, "tunnel {:?} invalid tcp tunnel", self.sequence));
            }
        }
    }

    pub async fn send(&self, data: &[u8]) -> BdtResult<()> {
        if !self.tunnel_type.is_stream() {
            return Err(bdt_err!(BdtErrorCode::ErrorState, "tunnel {:?} invalid tunnel type", self.sequence));
        }
        self.data_socket.as_ref().unwrap().send_resp(data).await.map_err(into_bdt_err!(BdtErrorCode::IoError, "tunnel {:?}", self.sequence))
    }

    pub async fn flush(&self) -> BdtResult<()> {
        if !self.tunnel_type.is_stream() {
            return Err(bdt_err!(BdtErrorCode::ErrorState, "tunnel {:?} invalid tunnel type", self.sequence));
        }
        self.data_socket.as_ref().unwrap().flush().await.map_err(into_bdt_err!(BdtErrorCode::IoError, "tunnel {:?}", self.sequence))
    }

    pub async fn recv(&self, data: &mut [u8]) -> BdtResult<usize> {
        if !self.tunnel_type.is_stream() {
            return Err(bdt_err!(BdtErrorCode::ErrorState, "tunnel {:?} invalid tunnel type", self.sequence));
        }
        self.data_socket.as_ref().unwrap().recv(data).await.map_err(into_bdt_err!(BdtErrorCode::IoError, "tunnel {:?}", self.sequence))
    }

    pub async fn recv_exact(&self, data: &mut [u8]) -> BdtResult<()> {
        if !self.tunnel_type.is_stream() {
            return Err(bdt_err!(BdtErrorCode::ErrorState, "tunnel {:?} invalid tunnel type", self.sequence));
        }
        self.data_socket.as_ref().unwrap().recv_exact(data).await.map_err(into_bdt_err!(BdtErrorCode::IoError, "tunnel {:?}", self.sequence))
    }

    pub async fn send_pkgs(&mut self, pkgs: Vec<DynamicPackage>) -> BdtResult<()> {
        if self.tunnel_type != TunnelType::TUNNEL {
            return Err(bdt_err!(BdtErrorCode::ErrorState, "tunnel {:?} invalid tunnel type", self.sequence));
        }
        self.data_socket.as_ref().unwrap().send_dynamic_pkgs(pkgs).await.map_err(into_bdt_err!(BdtErrorCode::IoError, "tunnel {:?}", self.sequence))
    }
    pub async fn recv_pkgs(&mut self) -> BdtResult<Vec<DynamicPackage>> {
        if self.tunnel_type != TunnelType::TUNNEL {
            return Err(bdt_err!(BdtErrorCode::ErrorState, "tunnel {:?} invalid tunnel type", self.sequence));
        }
        self.data_socket.as_ref().unwrap().recv_resp_pkgs(self.conn_timeout).await.map_err(into_bdt_err!(BdtErrorCode::IoError, "tunnel {:?}", self.sequence))
    }
}

#[async_trait::async_trait]
impl TunnelConnection for TcpTunnelConnection {
    fn sequence(&self) -> TempSeq {
        self.sequence
    }

    fn socket_type(&self) -> SocketType {
        SocketType::TCP
    }

    fn tunnel_type(&self) -> TunnelType {
        self.tunnel_type
    }

    fn tunnel_key(&self) -> &MixAesKey {
        self.data_socket.as_ref().unwrap().socket.key()
    }

    async fn connect_tunnel(&mut self) -> BdtResult<()> {
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
        let data_socket = TcpTunnelSocket::new(Arc::new(data_sender));

        let key_stub = self.net_manager.key_store().get_key_by_mix_hash(&data_socket.key().mix_hash(data_socket.local_device_id(), data_socket.remote_device_id()));
        if key_stub.is_none() {
            return Err(bdt_err!(BdtErrorCode::NotFound, "tunnel {:?} key not found", self.sequence));
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
        data_socket.send_dynamic_pkgs(pkgs).await?;
        let resp_pkgs = data_socket.recv_resp_pkgs(self.conn_timeout).await?;
        if resp_pkgs.len() != 1 {
            return Err(bdt_err!(BdtErrorCode::InvalidData, "tunnel {:?} invalid tunnel resp pkgs", self.sequence));
        }
        let result = resp_pkgs.get(0).unwrap();
        if result.cmd_code() != PackageCmdCode::AckTunnel {
            return Err(bdt_err!(BdtErrorCode::InvalidData, "tunnel {:?} invalid tunnel resp", self.sequence));
        }

        let ack: &AckTunnel = result.as_ref();
        if BdtErrorCode::from_u8(ack.result).unwrap_or(BdtErrorCode::Failed) != BdtErrorCode::Ok {
            return Err(bdt_err!(BdtErrorCode::ConnectionRefused, "tunnel {:?} ack tunnel failed result {}", self.sequence, ack.result));
        }

        let syn_ack_ack = AckAckTunnel {
            seq: self.sequence,
        };
        data_socket.send_dynamic_pkg(DynamicPackage::from(syn_ack_ack)).await?;

        self.data_socket = Some(data_socket);
        self.tunnel_type = TunnelType::TUNNEL;

        Ok(())
    }

    async fn connect_stream(&mut self, vport: u16, session_id: IncreaseId) -> BdtResult<()> {
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
        let data_socket = TcpTunnelSocket::new(Arc::new(data_sender));

        let key_stub = self.net_manager.key_store().get_key_by_mix_hash(&data_socket.key().mix_hash(data_socket.local_device_id(), data_socket.remote_device_id()));
        if key_stub.is_none() {
            return Err(bdt_err!(BdtErrorCode::NotFound, "tunnel {:?} key not found", self.sequence));
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
        let resp_pkgs = data_socket.recv_resp_pkgs(self.conn_timeout).await?;
        if resp_pkgs.len() != 2 {
            return Err(bdt_err!(BdtErrorCode::InvalidData, "tunnel {:?} invalid tunnel resp pkgs", self.sequence));
        }
        let result = resp_pkgs.get(0).unwrap();
        if result.cmd_code() != PackageCmdCode::AckTunnel {
            return Err(bdt_err!(BdtErrorCode::InvalidData, "tunnel {:?} invalid ack tunnel", self.sequence));
        }

        let ack: &AckTunnel = result.as_ref();
        if BdtErrorCode::from_u8(ack.result).unwrap_or(BdtErrorCode::Failed) != BdtErrorCode::Ok {
            let err = BdtErrorCode::from_u8(ack.result).unwrap_or(BdtErrorCode::Failed);
            return Err(bdt_err!(err, "tunnel {:?} ack tunnel failed", self.sequence));
        }

        let syn_ack_ack = AckAckTunnel {
            seq: self.sequence
        };

        let result = resp_pkgs.get(1).unwrap();
        if result.cmd_code() != PackageCmdCode::TcpAckConnection {
            return Err(bdt_err!(BdtErrorCode::InvalidData, "tunnel {:?} invalid tcp ack tunnel", self.sequence));
        }

        let ack: &TcpAckConnection = result.as_ref();
        if BdtErrorCode::from_u8(ack.result).unwrap_or(BdtErrorCode::Failed) != BdtErrorCode::Ok {
            let err = BdtErrorCode::from_u8(ack.result).unwrap_or(BdtErrorCode::Failed);
            return Err(bdt_err!(err, "tunnel {:?} tcp ack tunnel failed", self.sequence));
        }

        let tcp_ack_ack = TcpAckAckConnection {
            sequence: self.sequence,
            result: 0,
        };
        data_socket.send_dynamic_pkgs(vec![DynamicPackage::from(syn_ack_ack), DynamicPackage::from(tcp_ack_ack)]).await?;

        self.data_socket = Some(data_socket);
        self.tunnel_type = TunnelType::STREAM(vport);

        Ok(())
    }

    async fn shutdown(&self, how: Shutdown) -> BdtResult<()> {
        self.data_socket.as_ref().unwrap().shutdown(how).await
    }
}
