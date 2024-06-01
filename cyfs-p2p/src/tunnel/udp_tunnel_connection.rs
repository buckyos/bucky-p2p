use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;
use callback_result::SingleCallbackWaiter;
use cyfs_base::{bucky_time_now, BuckyError, BuckyErrorCode, DeviceDesc, DeviceId, Endpoint, TailedOwnedData};
use notify_future::NotifyFuture;
use crate::sockets::{DataSender, DataSenderFactory, NetManagerRef, SocketType, UdpExtraParams};
use crate::{IncreaseId, LocalDeviceRef, MixAesKey, TempSeq};
use crate::error::{bdt_err, BdtErrorCode, BdtResult};
use crate::history::keystore::EncryptedKey;
use crate::protocol::{AckTunnel, DynamicPackage, Exchange, MTU, PackageBox, PackageCmdCode, SynTunnel};
use crate::protocol::v0::{AckAckTunnel, PingTunnel, SessionData, SESSIONDATA_FLAG_ACK, SESSIONDATA_FLAG_ACK_ACK, SESSIONDATA_FLAG_SYN, SessionSynInfo};
use crate::tunnel::{TunnelConnection, TunnelType};
use crate::tunnel::tunnel_connection::TunnelConnectionKey;

pub struct UdpTunnelDataReceiver {
    conn_receiver: Mutex<HashMap<TunnelConnectionKey, Weak<dyn UdpTunnelSocketReceiver>>>,
}

impl UdpTunnelDataReceiver {
    pub fn new() -> Self {
        Self {
            conn_receiver: Mutex::new(HashMap::new()),
        }
    }

    pub async fn on_recv_pkg(&self, key: &TunnelConnectionKey, pkg: DynamicPackage) -> BdtResult<()> {
        let receiver = {
            let mut conn_receiver = self.conn_receiver.lock().unwrap();
            if let Some(receiver) = conn_receiver.get(key) {
                if let Some(receiver) = receiver.upgrade() {
                    receiver
                } else {
                    conn_receiver.remove(key);
                    return Err(bdt_err!(BdtErrorCode::ErrorState, "receiver {:?} is none", key));
                }
            } else {
                return Err(bdt_err!(BdtErrorCode::ErrorState, "receiver {:?} is none", key));
            }
        };
        receiver.on_recv_pkg(pkg).await?;
        Ok(())
    }

    pub async fn on_recv_pkg_with_resp(&self, key: &TunnelConnectionKey, pkg: DynamicPackage, timeout: Duration) -> BdtResult<DynamicPackage> {
        let receiver = {
            let mut conn_receiver = self.conn_receiver.lock().unwrap();
            if let Some(receiver) = conn_receiver.get(key) {
                if let Some(receiver) = receiver.upgrade() {
                    receiver
                } else {
                    conn_receiver.remove(key);
                    return Err(bdt_err!(BdtErrorCode::ErrorState, "receiver {:?} is none", key));
                }
            } else {
                return Err(bdt_err!(BdtErrorCode::ErrorState, "receiver {:?} is none", key));
            }
        };

        receiver.on_recv_pkg_with_resp(pkg, timeout).await
    }

    fn add_connection_receiver(&self, key: TunnelConnectionKey, receiver: Weak<dyn UdpTunnelSocketReceiver>) {
        log::debug!("add connection receiver {:?}", key);
        let mut conn_receiver = self.conn_receiver.lock().unwrap();
        conn_receiver.insert(key, receiver);
    }

    fn remove_connection_receiver(&self, key: &TunnelConnectionKey) {
        log::debug!("remove connection receiver {:?}", key);
        let mut conn_receiver = self.conn_receiver.lock().unwrap();
        conn_receiver.remove(key);
    }
}


pub type UdpTunnelDataReceiverRef = Arc<UdpTunnelDataReceiver>;

#[async_trait::async_trait]
pub trait UdpTunnelSocketReceiver: Send + Sync + 'static {
    async fn on_recv_pkg(&self, pkg: DynamicPackage) -> BdtResult<()>;
    async fn on_recv_pkg_with_resp(&self, pkg: DynamicPackage, timeout: Duration) -> BdtResult<DynamicPackage>;
}

type UdpTunnelRespFuture = NotifyFuture<DynamicPackage>;
pub struct UdpTunnelSocket {
    receiver: UdpTunnelDataReceiverRef,
    seq: TempSeq,
    data_sender: Arc<dyn DataSender>,
    resp_futures: Mutex<Vec<UdpTunnelRespFuture>>,
    resp_waiter: SingleCallbackWaiter<(DynamicPackage, Option<UdpTunnelRespFuture>)>
}

impl UdpTunnelSocket {
    pub fn new(receiver: UdpTunnelDataReceiverRef, seq: TempSeq, data_sender: Arc<dyn DataSender>) -> Arc<Self> {
        let key = TunnelConnectionKey::new(seq, data_sender.local_device_id().clone(), data_sender.remote_device_id().clone());
        let this = Arc::new(Self {
            receiver: receiver.clone(),
            seq,
            data_sender,
            resp_futures: Mutex::new(Vec::new()),
            resp_waiter: SingleCallbackWaiter::new(),
        });

        receiver.add_connection_receiver(key, Arc::downgrade(&this) as Weak<dyn UdpTunnelSocketReceiver>);
        this
    }

    fn add_resp_future(&self, future: UdpTunnelRespFuture) {
        let mut resp_futures = self.resp_futures.lock().unwrap();
        resp_futures.push(future);
    }

    pub async fn recv_resp(&self, timeout: Duration) -> BdtResult<DynamicPackage> {
        let (pkg, futuer) = self.resp_waiter.create_timeout_result_future(timeout).await
            .map_err(|_|bdt_err!(BdtErrorCode::Timeout, "syn timeout"))?;
        if let Some(future) = futuer {
            self.add_resp_future(future);
        }
        Ok(pkg)
    }
}

impl Drop for UdpTunnelSocket {
    fn drop(&mut self) {
        let key = TunnelConnectionKey::new(self.seq, self.data_sender.local_device_id().clone(), self.data_sender.remote_device_id().clone());
        self.receiver.remove_connection_receiver(&key);
    }
}
#[async_trait::async_trait]
impl UdpTunnelSocketReceiver for UdpTunnelSocket {
    async fn on_recv_pkg(&self, pkg: DynamicPackage) -> BdtResult<()> {
        self.resp_waiter.set_result_with_cache((pkg, None));
        Ok(())
    }

    async fn on_recv_pkg_with_resp(&self, pkg: DynamicPackage, timeout: Duration) -> BdtResult<DynamicPackage> {
        let future = NotifyFuture::new();
        self.resp_waiter.set_result_with_cache((pkg, Some(future.clone())));

        let ret = async_std::future::timeout(timeout, future).await;
        match ret {
            Ok(ret) => Ok(ret),
            Err(_) => Err(bdt_err!(BdtErrorCode::Timeout, "timeout"))
        }
    }
}

#[async_trait::async_trait]
impl DataSender for UdpTunnelSocket {
    async fn send_resp(&self, data: &[u8]) -> BdtResult<()> {
        self.data_sender.send_resp(data).await
    }

    async fn send_dynamic_pkg(&self, dynamic_package: DynamicPackage) -> BdtResult<()> {
        let future = {
            let mut resp_futures = self.resp_futures.lock().unwrap();
            resp_futures.pop()
        };
        if future.is_some() {
            future.unwrap().set_complete(dynamic_package);
            Ok(())
        } else {
            self.data_sender.send_dynamic_pkg(dynamic_package).await
        }
    }

    async fn send_dynamic_pkgs(&self, dynamic_packages: Vec<DynamicPackage>) -> BdtResult<()> {
        self.data_sender.send_dynamic_pkgs(dynamic_packages).await
    }

    async fn send_pkg_box(&self, pkg: &PackageBox) -> BdtResult<()> {
        self.data_sender.send_pkg_box(pkg).await
    }


    fn remote(&self) -> &Endpoint {
        self.data_sender.remote()
    }

    fn local(&self) -> &Endpoint {
        self.data_sender.local()
    }

    fn remote_device_id(&self) -> &DeviceId {
        self.data_sender.remote_device_id()
    }

    fn local_device_id(&self) -> &DeviceId {
        self.data_sender.local_device_id()
    }

    fn key(&self) -> &MixAesKey {
        self.data_sender.key()
    }

    fn socket_type(&self) -> SocketType {
        self.data_sender.socket_type()
    }
}

pub struct UdpTunnelConnection {
    net_manager: NetManagerRef,
    sequence: TempSeq,
    local_device: LocalDeviceRef,
    remote_desc: DeviceDesc,
    remote_ep: Endpoint,
    conn_timeout: Duration,
    data_socket: Option<Arc<UdpTunnelSocket>>,
    protocol_version: u8,
    stack_version: u32,
    data_receiver: UdpTunnelDataReceiverRef,
    local_ep: Endpoint,
    tunnel_type: TunnelType
}

impl UdpTunnelConnection {
    pub fn new(net_manager: NetManagerRef,
               data_receiver: UdpTunnelDataReceiverRef,
               sequence: TempSeq,
               local_device: LocalDeviceRef,
               remote_desc: DeviceDesc,
               remote_ep: Endpoint,
               conn_timeout: Duration,
               protocol_version: u8,
               stack_version: u32,
               local_ep: Endpoint,
               data_sender: Option<Arc<dyn DataSender>>) -> Self {
        let data_sender = data_sender.map(|data_sender| {
            UdpTunnelSocket::new(data_receiver.clone(), sequence, data_sender)
        });
        Self {
            net_manager,
            sequence,
            local_device,
            remote_desc,
            remote_ep,
            conn_timeout,
            data_socket: data_sender,
            protocol_version,
            stack_version,
            data_receiver,
            local_ep,
            tunnel_type: TunnelType::TUNNEL,
        }
    }

    pub async fn ping(&self) -> BdtResult<()> {
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

    pub async fn accept_tunnel(&mut self, listen_ports: HashSet<u16>) -> BdtResult<()> {
        let result = self.data_socket.as_ref().unwrap().recv_resp(self.conn_timeout).await?;
        if result.cmd_code() != PackageCmdCode::SynTunnel {
            return Err(bdt_err!(BdtErrorCode::InvalidData, "invalid syn tunnel"));
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
        } else if result.cmd_code() == PackageCmdCode::SessionData {
            let syn: &SessionData = result.as_ref();
            if !syn.is_flags_contain(SESSIONDATA_FLAG_SYN) || syn.syn_info.is_none() {
                return Err(bdt_err!(BdtErrorCode::InvalidData, "invalid syn tunnel"));
            }
            let vport = syn.syn_info.as_ref().unwrap().to_vport;

            let mut result = 0;
            if !listen_ports.contains(&vport) {
                log::debug!("unaccepted vport: {}", vport);
                result = BdtErrorCode::ConnectionRefused.into_u8();
            }

            let ack = SessionData {
                stream_pos: 0,
                ack_stream_pos: 0,
                sack: None,
                session_id: syn.session_id,
                send_time: bucky_time_now(),
                syn_info: Some(SessionSynInfo {
                    sequence: self.sequence(),
                    from_session_id: syn.session_id,
                    to_vport: 0,
                }),
                to_session_id: Some(syn.session_id),
                id_part: None,
                payload: TailedOwnedData::from(Vec::new()),
                flags: SESSIONDATA_FLAG_ACK,
            };
            self.data_socket.as_ref().unwrap().send_dynamic_pkg(DynamicPackage::from(ack)).await?;

            let result = self.data_socket.as_ref().unwrap().recv_resp(self.conn_timeout).await?;
            if result.cmd_code() != PackageCmdCode::AckAckTunnel {
                return Err(bdt_err!(BdtErrorCode::InvalidData, "invalid syn tunnel"));
            }

            let result = self.data_socket.as_ref().unwrap().recv_resp(self.conn_timeout).await?;
            if result.cmd_code() != PackageCmdCode::SessionData {
                return Err(bdt_err!(BdtErrorCode::InvalidData, "invalid syn tunnel"));
            }
            let tcp_ack_ack: &SessionData = result.as_ref();
            if !tcp_ack_ack.is_flags_contain(SESSIONDATA_FLAG_ACK_ACK) {
                return Err(bdt_err!(BdtErrorCode::InvalidData, "invalid syn tunnel"));
            }
            self.tunnel_type = TunnelType::STREAM(vport);
            Ok(())
        } else {
            return Err(bdt_err!(BdtErrorCode::InvalidData, "invalid ack tunnel"));
        }
    }

    pub async fn send_pkgs(&self, pkgs: Vec<DynamicPackage>) -> BdtResult<()> {
        if !self.tunnel_type.is_stream() {
            return Err(bdt_err!(BdtErrorCode::ErrorState, "invalid tunnel type"));
        }

        self.data_socket.as_ref().unwrap().send_dynamic_pkgs(pkgs).await
    }

    pub async fn recv_session_data(&self) -> BdtResult<DynamicPackage> {
        if !self.tunnel_type.is_stream() {
            return Err(bdt_err!(BdtErrorCode::ErrorState, "invalid tunnel type"));
        }

        let pkg = self.data_socket.as_ref().unwrap().recv_resp(self.conn_timeout).await?;
        if pkg.cmd_code() != PackageCmdCode::SessionData {
            return Err(bdt_err!(BdtErrorCode::InvalidData, "invalid session data"));
        }

        Ok(pkg)
    }
}

#[async_trait::async_trait]
impl TunnelConnection for UdpTunnelConnection {
    fn sequence(&self) -> TempSeq {
        self.sequence
    }

    fn socket_type(&self) -> SocketType {
        SocketType::UDP
    }

    fn tunnel_type(&self) -> TunnelType {
        self.tunnel_type
    }

    async fn connect_tunnel(&mut self) -> BdtResult<()> {
        if self.data_socket.is_some() {
            return Ok(());
        }
        log::info!("client {} remote {}", self.local_device.device_id().to_string(), self.remote_desc.device_id().to_string());
        let data_sender = self.net_manager.create_sender(
            self.local_device.device_id().clone(),
            self.remote_desc.clone(),
            self.remote_ep.clone(),
            UdpExtraParams {
                local_ep: self.local_ep.clone(),
            }).await?;

        let data_socket = UdpTunnelSocket::new(self.data_receiver.clone(), self.sequence, Arc::new(data_sender));

        let key_stub = self.net_manager.key_store().get_key_by_mix_hash(&data_socket.key().mix_hash(data_socket.local_device_id(), data_socket.remote_device_id()));
        if key_stub.is_none() {
            return Err(bdt_err!(BdtErrorCode::NotFound, "key not found"));
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
        let result = data_socket.recv_resp(self.conn_timeout).await?;
        if result.cmd_code() != PackageCmdCode::AckTunnel {
            return Err(bdt_err!(BdtErrorCode::InvalidData, "invalid ack tunnel"));
        }

        let ack: &AckTunnel = result.as_ref();
        if BuckyErrorCode::from(ack.result as u16) != BuckyErrorCode::Ok {
            return Err(bdt_err!(BdtErrorCode::InvalidData, "ack tunnel failed"));
        }

        let syn_ack_ack = AckAckTunnel {
            seq: self.sequence,
        };
        data_socket.send_dynamic_pkg(DynamicPackage::from(syn_ack_ack)).await?;
        self.data_socket = Some(data_socket);

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
            UdpExtraParams {
                local_ep: self.local_ep.clone(),
            }).await?;

        let data_socket = UdpTunnelSocket::new(self.data_receiver.clone(), self.sequence, Arc::new(data_sender));

        let key_stub = self.net_manager.key_store().get_key_by_mix_hash(&data_socket.key().mix_hash(data_socket.local_device_id(), data_socket.remote_device_id()));
        if key_stub.is_none() {
            return Err(bdt_err!(BdtErrorCode::NotFound, "key not found"));
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

        let tcp_syn = SessionData {
            stream_pos: 0,
            ack_stream_pos: 0,
            sack: None,
            session_id,
            send_time: bucky_time_now(),
            syn_info: Some(SessionSynInfo {
                sequence: self.sequence,
                from_session_id: session_id,
                to_vport: vport,
            }),
            to_session_id: None,
            id_part: None,
            payload: TailedOwnedData::from(Vec::new()),
            flags: SESSIONDATA_FLAG_SYN,
        };
        pkgs.push(DynamicPackage::from(tcp_syn));

        data_socket.send_dynamic_pkgs(pkgs).await?;
        let result = data_socket.recv_resp(self.conn_timeout).await?;
        if result.cmd_code() != PackageCmdCode::AckTunnel {
            return Err(bdt_err!(BdtErrorCode::InvalidData, "invalid ack tunnel"));
        }

        let ack: &AckTunnel = result.as_ref();
        if BuckyErrorCode::from(ack.result as u16) != BuckyErrorCode::Ok {
            return Err(bdt_err!(BdtErrorCode::InvalidData, "ack tunnel failed"));
        }

        let syn_ack_ack = AckAckTunnel {
            seq: self.sequence
        };
        data_socket.send_dynamic_pkg(DynamicPackage::from(syn_ack_ack)).await?;

        let result = data_socket.recv_resp(self.conn_timeout).await?;
        if result.cmd_code() != PackageCmdCode::SessionData {
            return Err(bdt_err!(BdtErrorCode::InvalidData, "invalid tcp ack tunnel"));
        }

        let ack: &SessionData = result.as_ref();
        if !ack.is_flags_contain(SESSIONDATA_FLAG_ACK) {
            return Err(bdt_err!(BdtErrorCode::InvalidData, "invalid tcp ack tunnel"));
        }

        let tcp_ack_ack = SessionData {
            stream_pos: 0,
            ack_stream_pos: 0,
            sack: None,
            session_id,
            send_time: bucky_time_now(),
            syn_info: Some(SessionSynInfo {
                sequence: self.sequence,
                from_session_id: session_id,
                to_vport: vport,
            }),
            to_session_id: None,
            id_part: None,
            payload: TailedOwnedData::from(Vec::new()),
            flags: SESSIONDATA_FLAG_ACK_ACK,
        };

        data_socket.send_dynamic_pkg(DynamicPackage::from(tcp_ack_ack)).await?;

        self.data_socket = Some(data_socket);

        Ok(())
    }
}
