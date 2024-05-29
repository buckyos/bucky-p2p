use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;
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

#[derive(Debug, Clone, Copy, Ord, PartialOrd, PartialEq, Eq)]
pub enum TunnelType {
    TUNNEL,
    STREAM,
}

pub struct TunnelDataReceiver {
    conn_receiver: Mutex<HashMap<TunnelConnectionKey, Weak<dyn TunnelSocketReceiver>>>,
}

impl TunnelDataReceiver {
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

    fn add_connection_receiver(&self, key: TunnelConnectionKey, receiver: Weak<dyn TunnelSocketReceiver>) {
        let mut conn_receiver = self.conn_receiver.lock().unwrap();
        conn_receiver.insert(key, receiver);
    }

    fn remove_connection_receiver(&self, key: &TunnelConnectionKey) {
        let mut conn_receiver = self.conn_receiver.lock().unwrap();
        conn_receiver.remove(key);
    }
}


pub type TunnelDataReceiverRef = Arc<TunnelDataReceiver>;

#[async_trait::async_trait]
pub trait TunnelSocketReceiver: Send + Sync + 'static {
    async fn on_recv_pkg(&self, pkg: DynamicPackage) -> BdtResult<()>;
    async fn on_recv_pkg_with_resp(&self, pkg: DynamicPackage, timeout: Duration) -> BdtResult<DynamicPackage>;
}

type  TunnelRespFuture = NotifyFuture<DynamicPackage>;
pub struct TunnelSocket {
    receiver: TunnelDataReceiverRef,
    seq: TempSeq,
    data_sender: Arc<dyn DataSender>,
    resp_futures: Mutex<Vec<TunnelRespFuture>>,
    resp_waiter: SingleCallbackWaiter<(DynamicPackage, Option<TunnelRespFuture>)>
}

impl TunnelSocket {
    pub fn new(receiver: TunnelDataReceiverRef, seq: TempSeq, data_sender: Arc<dyn DataSender>) -> Arc<Self> {
        let key = TunnelConnectionKey::new(seq, data_sender.local().clone(), data_sender.remote().clone());
        let this = Arc::new(Self {
            receiver: receiver.clone(),
            seq,
            data_sender,
            resp_futures: Mutex::new(Vec::new()),
            resp_waiter: SingleCallbackWaiter::new(),
        });

        receiver.add_connection_receiver(key, Arc::downgrade(&this) as Weak<dyn TunnelSocketReceiver>);
        this
    }

    fn add_resp_future(&self, future: TunnelRespFuture) {
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

impl Drop for TunnelSocket {
    fn drop(&mut self) {
        let key = TunnelConnectionKey::new(self.seq, self.data_sender.local().clone(), self.data_sender.remote().clone());
        self.receiver.remove_connection_receiver(&key);
    }
}
#[async_trait::async_trait]
impl TunnelSocketReceiver for TunnelSocket {
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
impl DataSender for TunnelSocket {
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

#[async_trait::async_trait]
pub trait TunnelConnection: Send + Sync + 'static {
    fn sequence(&self) -> TempSeq;
    fn socket_type(&self) -> SocketType;
    fn tunnel_type(&self) -> TunnelType;
    async fn accept_tunnel(&mut self) -> BdtResult<()>;
    async fn connect_tunnel(&mut self) -> BdtResult<()>;
    async fn connect_stream(&mut self, vport: u16, session_id: IncreaseId) -> BdtResult<()>;
    async fn send(&self, data: &[u8]) -> BdtResult<()>;
    async fn recv(&self, data: &[u8]) -> BdtResult<usize>;
    async fn recv_pkg(&self) -> BdtResult<DynamicPackage>;
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
