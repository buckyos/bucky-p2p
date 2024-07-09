use std::future::Future;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use bucky_error::BuckyErrorCode;
use bucky_objects::{Device, DeviceId, Endpoint, NamedObject, Protocol};
use bucky_raw_codec::{RawConvertTo, RawFrom};
use bucky_time::bucky_time_now;
use callback_result::CallbackWaiter;
use futures::future::{abortable, Abortable, AbortHandle};
use notify_future::NotifyFuture;
use quinn::{Connection, ConnectionError};
use quinn::crypto::rustls::QuicClientConfig;
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
use rustls::version::TLS13;
use crate::{LocalDeviceRef, MixAesKey, runtime, TempSeq, TempSeqGenerator};
use crate::error::{bdt_err, BdtErrorCode, BdtResult, into_bdt_err};
use crate::executor::Executor;
use crate::history::keystore::Keystore;
use crate::protocol::{Package, PackageCmdCode, SnCall, ReportSn};
use crate::protocol::v0::{SnCalled, SnCalledResp, SnCallResp, SnPingResp};
use crate::receive_processor::{ReceiveProcessor, ReceiveProcessorRef};
use crate::sn::service::PeerConnection;
use crate::sn::types::PingSessionResp;
use crate::sockets::{NetManagerRef, QuicSocket};
use crate::sockets::tcp::TCPSocket;

#[async_trait::async_trait]
pub trait SNEvent: 'static + Send + Sync {
    async fn on_called(&self, called: &SnCalled) -> BdtResult<()>;
}
pub type SNEventRef = Arc<dyn SNEvent>;

#[derive(Clone)]
pub struct ActiveSN {
    pub sn: Device,
    pub latest_time: u64,
    pub conn_id: TempSeq,
    pub recv_future: Arc<Mutex<Option<NotifyFuture<SnCallResp>>>>,
    pub peer_connection: Arc<runtime::Mutex<PeerConnection>>,
}

pub struct SNServiceState {
    pub pinging_tasks: Vec<AbortHandle>,
    pub active_sn_list: Vec<ActiveSN>,
    pub latest_sn_interval: u64,
}

pub struct SNClientService {
    net_manager: NetManagerRef,
    sn_list: Vec<Device>,
    local_device: LocalDeviceRef,
    gen_seq: Arc<TempSeqGenerator>,
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
            sn_list,
            local_device,
            gen_seq,
            ping_timeout,
            call_timeout,
            conn_timeout,
            state: RwLock::new(SNServiceState {
                pinging_tasks: vec![],
                active_sn_list: vec![],
                latest_sn_interval: 0,
            }),
            listener,
        })
    }

    async fn handle(&self, conn_id: TempSeq, cmd_code: PackageCmdCode, cmd_body: Vec<u8>) -> BdtResult<()> {
        match cmd_code {
            PackageCmdCode::SnCallResp => {
                let resp = SnCallResp::clone_from_slice(cmd_body.as_slice()).map_err(into_bdt_err!(BdtErrorCode::RawCodecError))?;
                let mut state = self.state.write().unwrap();
                for active_sn in state.active_sn_list.iter_mut() {
                    if active_sn.conn_id == conn_id {
                        let mut recv_future = active_sn.recv_future.lock().unwrap();
                        if let Some(recv_future) = recv_future.take() {
                            recv_future.set_complete(resp);
                        }
                        break;
                    }
                }
            },
            PackageCmdCode::SnCalled => {
                let sn_called = SnCalled::clone_from_slice(cmd_body.as_slice()).map_err(into_bdt_err!(BdtErrorCode::RawCodecError))?;
                self.on_called(conn_id, &sn_called).await?;
            }
            _ => warn!("invalid cmd-package, conn: {:?} cmd_code {:?}.", conn_id, cmd_code),
        }
        Ok(())
    }

    async fn on_called(&self, conn_id: TempSeq, sn_called: &SnCalled) -> BdtResult<()> {
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

        let peer_conn = self.get_peer_connection(conn_id);
        if let Some(peer_conn) = peer_conn {
            peer_conn.lock().await.send(Package::new(PackageCmdCode::SnCallResp, resp)).await?;
        }
        Ok(())
    }

    fn get_peer_connection(&self, conn_id: TempSeq) -> Option<Arc<runtime::Mutex<PeerConnection>>> {
        let state = self.state.read().unwrap();
        for active_sn in state.active_sn_list.iter() {
            if active_sn.conn_id == conn_id {
                return Some(active_sn.peer_connection.clone());
            }
        }
        None
    }

    pub async fn start(self: &Arc<Self>) {
        let this = self.clone();
        let handle = Executor::spawn_with_handle(async move {
            this.ping_proc().await;
        }).unwrap();
    }

    async fn ping_proc(self: &Arc<Self>) {
        loop {
            {
                let (active_sn_count, latest_sn_interval, cur_sn_interval) = {
                    let mut state = self.state.write().unwrap();
                    if state.active_sn_list.len() > 0 {
                        state.latest_sn_interval = 10;
                        (state.active_sn_list.len(), state.latest_sn_interval, state.latest_sn_interval)
                    } else {
                        let cur_sn_interval = state.latest_sn_interval;
                        if state.latest_sn_interval == 0 {
                            state.latest_sn_interval = 10;
                        } else {
                            state.latest_sn_interval = state.latest_sn_interval * 2;
                        }
                        if state.latest_sn_interval > 600 {
                            state.latest_sn_interval = 600;
                        }
                        (state.active_sn_list.len(), cur_sn_interval, state.latest_sn_interval)
                    }
                };
                if latest_sn_interval != 0 {
                    runtime::sleep(Duration::from_secs(cur_sn_interval)).await;
                }
                if active_sn_count > 0 {
                    continue;
                }
            }
            for listener in self.net_manager.udp_listeners().iter() {
                let quic_ep = listener.quic_ep();
                for sn in self.sn_list.iter() {
                    for sn_ep in sn.connect_info().endpoints().iter() {
                        let mut peer_conn = match self.create_connection(listener.local(), quic_ep.clone(), sn, sn_ep).await {
                            Ok(peer_conn) => peer_conn,
                            Err(e) => {
                                log::error!("connect to sn {} failed: {:?}", sn.desc().device_id(), e);
                                continue;
                            }
                        };
                        if let Err(e) = self.report(&mut peer_conn).await {
                            log::error!("ping to {} failed: {:?}", sn.desc().device_id(), e);
                            continue;
                        }

                        let recv_handle = peer_conn.take_recv_handle().unwrap();
                        let conn_id = peer_conn.conn_id();
                        let active_sn = ActiveSN {
                            sn: sn.clone(),
                            latest_time: bucky_time_now(),
                            conn_id: peer_conn.conn_id(),
                            recv_future: Arc::new(Mutex::new(None)),
                            peer_connection: Arc::new(runtime::Mutex::new(peer_conn)),
                        };
                        let mut state = self.state.write().unwrap();
                        state.active_sn_list.push(active_sn);

                        let this = self.clone();
                        Executor::spawn_ok(async move {
                            recv_handle.await;
                            this.remote_sn_conn(conn_id);
                        });
                    }
                }
            }
        }
    }

    fn remote_sn_conn(&self, conn_id: TempSeq) {
        let mut state = self.state.write().unwrap();
        state.active_sn_list.retain(|sn| sn.conn_id != conn_id);
    }

    async fn create_connection(self: &Arc<Self>, local_ep: Endpoint, quic_ep: quinn::Endpoint, sn: &Device, sn_ep: &Endpoint) -> BdtResult<PeerConnection> {
        let client_key = self.local_device.key().to_vec().map_err(into_bdt_err!(BdtErrorCode::RawCodecError))?;
        let client_cert = self.local_device.device().to_vec().map_err(into_bdt_err!(BdtErrorCode::RawCodecError))?;
        let mut config =
            rustls::ClientConfig::builder_with_provider(bucky_rustls::provider().into())
                .with_protocol_versions(&[&TLS13])
                .unwrap()
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(bucky_rustls::BuckyServerCertVerifier {}))
                .with_client_auth_cert(vec![CertificateDer::from(client_cert)], PrivatePkcs8KeyDer::from(client_key).into())
                .map_err(into_bdt_err!(BdtErrorCode::TlsError))?;
        config.enable_early_data = true;

        let mut client_config =
            quinn::ClientConfig::new(Arc::new(QuicClientConfig::try_from(config).unwrap()));
        let mut transport_config = quinn::TransportConfig::default();
        transport_config.max_idle_timeout(Some(std::time::Duration::from_secs(240).try_into().unwrap()));
        transport_config.keep_alive_interval(Some(Duration::from_secs(60)));
        client_config.transport_config(Arc::new(transport_config));

        let conning = quic_ep.connect_with(client_config, sn_ep.addr().clone(), sn.desc().device_id().object_id().to_base36().as_str())
            .map_err(into_bdt_err!(BdtErrorCode::ConnectFailed, "connect to sn failed"))?;

        let conn = conning.await.map_err(into_bdt_err!(BdtErrorCode::ConnectFailed, "connect to sn failed"))?;
        let quic_socket = QuicSocket::new(conn, self.local_device.device_id().clone(), sn.desc().device_id().clone(), local_ep, sn_ep.clone());
        let conn_id = self.gen_seq.generate();
        let this = self.clone();
        let peer_conn = PeerConnection::connect(conn_id, quic_socket, move |conn_id: TempSeq, cmd_code: PackageCmdCode, cmd_body: Vec<u8>| {
            let this = this.clone();
            async move {
                if let Err(e) = this.handle(conn_id, cmd_code, cmd_body).await {
                    log::error!("handle cmd {:?} error: {:?}", cmd_code, e);
                    Err(e)
                } else {
                    Ok(())
                }
            }
        }).await.map_err(into_bdt_err!(BdtErrorCode::ConnectFailed, "connect to sn failed"))?;
        Ok(peer_conn)
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

    fn get_active_sn_list(&self) -> Vec<ActiveSN> {
        let state = self.state.read().unwrap();
        state.active_sn_list.clone()
    }

    async fn report(&self, conn: &mut PeerConnection) -> BdtResult<()> {
        let seq = self.gen_seq.generate();
        let mut report = ReportSn {
            protocol_version: 0,
            stack_version: 0,
            seq,
            sn_peer_id: conn.remote_device_id().clone(),
            from_peer_id: Some(self.local_device.device_id().clone()),
            peer_info: Some(self.local_device.device().clone()),
            send_time: bucky_time_now(),
            contract_id: None,
            receipt: None,
        };
        conn.send(Package::new(PackageCmdCode::ReportSn, report)).await?;

        Ok(())
    }

    pub async fn call(&self,
                      reverse_endpoints: Option<&[Endpoint]>,
                      remote: &DeviceId,
                      payload_pkg: Vec<u8>) -> BdtResult<SnCallResp> {
        let active_list = self.get_active_sn_list();
        for active in active_list.iter() {
            let seq = self.gen_seq.generate();
            let mut call = SnCall {
                protocol_version: 0,
                stack_version: 0,
                seq,
                sn_peer_id: active.sn.desc().device_id(),
                to_peer_id: remote.clone(),
                from_peer_id: self.local_device.device_id().clone(),
                reverse_endpoint_array: reverse_endpoints.map(|ep_list| Vec::from(ep_list)),
                active_pn_list: None,
                peer_info: Some(self.local_device.device().clone()),
                send_time: bucky_time_now(),
                payload: payload_pkg.clone(),
                is_always_call: false,
            };
            let mut peer_conn = active.peer_connection.lock().await;
            let future = NotifyFuture::<SnCallResp>::new();
            {
                let mut recv_future = active.recv_future.lock().unwrap();
                *recv_future = Some(future.clone());
            }
            if let Err(e) = peer_conn.send(Package::new(PackageCmdCode::SnCall, call)).await {
                log::error!("send call to {} failed: {:?}", active.sn.desc().device_id(), e);
                continue;
            }

            let resp = match runtime::timeout(self.call_timeout, future).await {
                Ok(resp) => resp,
                Err(_) => {
                    log::error!("call to {} timeout", active.sn.desc().device_id());
                    continue;
                }
            };

            return Ok(resp);
        }
        Err(bdt_err!(BdtErrorCode::ConnectFailed, "call timeout"))
    }

}
