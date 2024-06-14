use std::future::Future;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use bucky_error::BuckyErrorCode;
use bucky_objects::{Device, Endpoint, NamedObject, Protocol};
use bucky_time::bucky_time_now;
use callback_result::CallbackWaiter;
use futures::future::{abortable, Abortable, AbortHandle};
use crate::{LocalDeviceRef, MixAesKey, runtime, TempSeqGenerator};
use crate::error::BdtResult;
use crate::executor::Executor;
use crate::history::keystore::Keystore;
use crate::protocol::{DynamicPackage, PackageBox, PackageCmdCode};
use crate::protocol::v0::{SnCalled, SnCalledResp, SnPingResp};
use crate::receive_processor::{ReceiveProcessor, ReceiveProcessorRef, RespSender};
use crate::sn::client::{SNClient, SNResp, SNRespWaiter, SNRespWaiterEx, SNRespWaiterRef};
use crate::sn::types::PingSessionResp;
use crate::sockets::{DataSender, DataSenderFactory, NetManagerRef, TcpExtraParams, UdpDataSender, UdpExtraParams};
use crate::sockets::tcp::TCPSocket;

#[async_trait::async_trait]
pub trait SNEvent: 'static + Send + Sync {
    async fn on_called(&self, called: &SnCalled) -> BdtResult<()>;
}
pub type SNEventRef = Arc<dyn SNEvent>;

pub struct ActiveSN {
    pub sn: Device,
    pub local_ep: Endpoint,
    pub remote_ep: Endpoint,
    pub latest_time: u64,
}

pub struct SNServiceState {
    pub pinging_tasks: Vec<AbortHandle>,
    pub active_sn_list: Vec<ActiveSN>,
}

pub struct SNClientService {
    net_manager: NetManagerRef,
    sn_list: RwLock<Vec<Device>>,
    local_device: LocalDeviceRef,
    gen_seq: Arc<TempSeqGenerator>,
    resp_waiter: SNRespWaiterRef,
    ping_timeout: Duration,
    call_timeout: Duration,
    conn_timeout: Duration,
    state: RwLock<SNServiceState>,
    listener: SNEventRef,
}
pub type SNClientServiceRef = Arc<SNClientService>;

impl SNClientService {
    pub fn new(net_manager: NetManagerRef,
               sn_list: Vec<Device>,
               local_device: LocalDeviceRef,
               gen_seq: Arc<TempSeqGenerator>,
               listener: SNEventRef,
               ping_timeout: Duration,
               call_timeout: Duration,
               conn_timeout: Duration,) -> Arc<Self> {
        Arc::new(Self {
            net_manager,
            sn_list: RwLock::new(sn_list),
            local_device,
            gen_seq,
            resp_waiter: SNRespWaiterRef::new(CallbackWaiter::new()),
            ping_timeout,
            call_timeout,
            conn_timeout,
            state: RwLock::new(SNServiceState {
                pinging_tasks: vec![],
                active_sn_list: vec![],
            }),
            listener,
        })
    }

    pub fn register_pkg_processor(self: &Arc<Self>, processor: &mut ReceiveProcessor) {
        let this = self.clone();
        processor.add_package_box_processor(PackageCmdCode::SnPingResp, move |resp_sender: &'static mut RespSender,
                                                                              pkg: DynamicPackage| {
            let service = this.clone();
            async move {
                let ping_resp: &SnPingResp = pkg.as_ref();
                service.resp_waiter.set_result(SNRespWaiter::get_ping_id(ping_resp.seq.value()), SNResp::Ping(ping_resp.clone()));
                Ok(())
            }
        });

        let this = self.clone();
        processor.add_package_box_processor(PackageCmdCode::SnCallResp, move |resp_sender: &'static mut RespSender,
                                                                         pkg: DynamicPackage| {
            let service = this.clone();
            async move {
                Ok(())
            }
        });

        let this = self.clone();
        processor.add_package_box_processor(PackageCmdCode::SnCalled, move |resp_sender: &'static mut RespSender,
                                                                            pkg: DynamicPackage| {
            let service = this.clone();
            async move {
                service.on_called(resp_sender, pkg.as_ref()).await
            }
        });
    }

    async fn on_called(&self, resp_sender: &'static mut RespSender, sn_called: &SnCalled) -> BdtResult<()> {
        assert_eq!(self.local_device.device_id(), resp_sender.local_device_id());

        let resp = match self.listener.on_called(sn_called).await {
            Ok(_) => {
                SnCalledResp {
                    seq: sn_called.call_seq.clone(),
                    sn_peer_id: sn_called.sn_peer_id.clone(),
                    result: 0,
                }
            }
            Err(e) => {
                log::info!("on called to {} failed: {:?}", sn_called.to_peer_id, e);
                SnCalledResp {
                    seq: sn_called.call_seq.clone(),
                    sn_peer_id: sn_called.sn_peer_id.clone(),
                    result: e.code().into_u8(),
                }
            }
        };

        resp_sender.send_dynamic_pkg(DynamicPackage::from(resp)).await?;
        Ok(())
    }

    fn add_active_sn(&self, active_sn: ActiveSN) {
        let mut state = self.state.write().unwrap();
        state.active_sn_list.retain(|sn| {
            sn.sn.desc().device_id() != active_sn.sn.desc().device_id()
        });

        state.active_sn_list.push(active_sn);
    }

    fn clear_active_sn(&self) {
        let mut state = self.state.write().unwrap();
        state.active_sn_list.clear();
    }

    fn clear_timeout_active_sn(&self) {
        let mut state = self.state.write().unwrap();
        let now = bucky_time_now();
        state.active_sn_list.retain(|sn| {
            now - sn.latest_time < 60
        });
    }

    pub async fn start(self: &Arc<Self>) {
        let this = self.clone();
        Executor::spawn_ok(async move {
            this.ping_proc().await;
        });
    }

    pub async fn wait_online(&self, timeout: Option<Duration>) -> BdtResult<()> {
        loop {
            {
                let state = self.state.read().unwrap();
                if state.active_sn_list.len() > 0 {
                    break;
                }
            }
            runtime::sleep(Duration::from_secs(1)).await;
        }
        Ok(())
    }

    async fn ping_proc(&self) {
        loop {
            let sn_list = {
                self.sn_list.read().unwrap().clone()
            };
            for sn in sn_list.iter() {
                if let Err(e) = self.ping(sn.clone()).await {
                    log::error!("ping sn {} failed: {}", sn.desc().device_id(), e);
                }
            }
            runtime::sleep(Duration::from_secs(30)).await;
        }
    }

    async fn ping(&self, sn: Device) -> BdtResult<()> {
        let sn_id = sn.desc().device_id();
        let mut tasks = vec![];
        let mut task_handles = vec![];
        for sn_ep in sn.connect_info().endpoints().iter() {
            let sender_list: Vec<Box<dyn DataSender>> = if sn_ep.protocol() == Protocol::Tcp {
                // let data_sender: TCPSocket = self.net_manager.create_sender(self.local_device.device_id().clone(), sn.desc().clone(), sn_ep.clone(), TcpExtraParams {
                //     timeout: self.conn_timeout,
                // }).await?;
                // vec![GeneralDataSender::from(data_sender)]
                continue;
            } else {
                let mut sender_list: Vec<Box<dyn DataSender>> = vec![];
                let udps = self.net_manager.udp_listeners();
                for udp_listener in udps.iter() {
                    let data_sender: UdpDataSender = self.net_manager.create_sender(self.local_device.device_id().clone(), sn.desc().clone(), sn_ep.clone(), UdpExtraParams {
                        local_ep: udp_listener.local()
                    }).await?;
                    sender_list.push(Box::new(data_sender));
                }
                sender_list
            };
            for sender in sender_list {
                let sn_client = SNClient::new(self.local_device.clone(),
                                              sn.desc().device_id(),
                                              sn.clone(),
                                              sn_ep.clone(),
                                              self.gen_seq.clone(),
                                              sender,
                                              self.resp_waiter.clone(),
                                              true,
                                              self.net_manager.key_store().clone(),
                                              self.ping_timeout,
                                              self.call_timeout);
                let (task, task_handle) = futures::future::abortable(async move {
                    let local_ep = sn_client.data_sender().local().clone();
                    let remote_ep = sn_client.data_sender().remote().clone();
                    sn_client.ping().await.map(|v| (v, local_ep, remote_ep))
                });
                tasks.push(Box::pin(async move {
                    task.await
                }));
                task_handles.push(task_handle);
            }
        }
        if tasks.len() == 0 {
            return Ok(());
        }
        let ret = futures::future::select_ok(tasks).await;

        if ret.is_ok() {
            let (ret, _) = ret.unwrap();
            if ret.is_ok() {
                let (resp, local_ep, remote_ep) = ret.unwrap();
                if BuckyErrorCode::from(resp.result as u16) == BuckyErrorCode::Ok {
                    self.add_active_sn(ActiveSN {
                        sn,
                        local_ep,
                        remote_ep,
                        latest_time: bucky_time_now(),
                    });
                }
            }
        }

        Ok(())
    }

    async fn call(&self, remote_sn: ActiveSN) -> BdtResult<()> {
        Ok(())
    }
}
