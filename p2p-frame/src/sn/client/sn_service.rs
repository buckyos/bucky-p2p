use crate::endpoint::{Endpoint, Protocol};
use crate::error::{P2pErrorCode, P2pResult, into_p2p_err, p2p_err};
use crate::executor::{Executor, SpawnHandle};
use crate::networks::{NetManagerRef, TunnelPurpose};
use crate::p2p_identity::{
    EncodedP2pIdentityCert, P2pId, P2pIdentityCertFactoryRef, P2pIdentityCertRef, P2pIdentityRef,
    P2pSn,
};
use crate::runtime;
use crate::sn::protocol::v0::{SnCallResp, SnCalled, SnCalledResp, TunnelType};
use crate::sn::protocol::{
    Package, PackageCmdCode, ReportSn, ReportSnResp, SnCall, SnQuery, SnQueryResp,
};
use crate::sn::types::{
    CmdTunnelId, SN_CMD_SERVICE, SnCmdHeader, SnTunnelClassification, SnTunnelRead, SnTunnelWrite,
};
use crate::ttp::{TtpClient, TtpClientRef, TtpConnector, TtpTarget};
use crate::types::{Sequence, SequenceGenerator, TunnelId, TunnelIdGenerator};
use bucky_raw_codec::{RawConvertTo, RawFrom};
use bucky_time::bucky_time_now;
use chrono::Utc;
use notify_future::Notify;
use sfo_cmd_server::client::{
    ClassifiedCmdClient, ClassifiedCmdSend, ClassifiedCmdTunnel, ClassifiedCmdTunnelFactory,
    CmdClient, DefaultClassifiedCmdClient,
};
use sfo_cmd_server::errors::{CmdErrorCode, CmdResult, cmd_err, into_cmd_err};
use sfo_cmd_server::{CmdBody, CmdTunnel, PeerId};
use std::collections::HashMap;
use std::net::IpAddr;
use std::ops::Add;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

#[callback_trait::callback_trait]
pub trait SNEvent: 'static + Send + Sync {
    async fn on_called(&self, called: SnCalled) -> P2pResult<()>;
}
pub type SNEventRef = Arc<dyn SNEvent>;

pub enum SnResp {
    SnCallResp(SnCallResp),
    SnQueryResp(SnQueryResp),
}

#[derive(Clone)]
pub struct ActiveSN {
    pub sn_peer_id: P2pId,
    pub latest_time: u64,
    pub conn_id: CmdTunnelId,
    pub recv_future: Arc<Mutex<HashMap<Sequence, Notify<SnResp>>>>,
    pub wan_ep_list: Vec<Endpoint>,
}

pub struct SNServiceState {
    pub pinging_handle: Option<SpawnHandle<()>>,
    pub active_sn_list: Vec<ActiveSN>,
    pub latest_sn_interval: u64,
    pub cur_report_future: Option<Notify<ReportSnResp>>,
}

pub struct SnList {
    sn_list: Mutex<Vec<P2pSn>>,
}

impl SnList {
    pub(crate) fn new(sn_list: Vec<P2pSn>) -> Self {
        Self {
            sn_list: Mutex::new(sn_list),
        }
    }

    pub fn get_sn_list(&self) -> Vec<P2pSn> {
        self.sn_list.lock().unwrap().clone()
    }

    pub fn update_sn_list(&self, sn_list: Vec<P2pSn>) {
        *self.sn_list.lock().unwrap() = sn_list;
    }
}

pub struct SnClientTunnelFactory {
    net_manager: NetManagerRef,
    sn_list: Arc<SnList>,
    ttp_client: TtpClientRef,
}

impl SnClientTunnelFactory {
    pub(crate) fn new(
        net_manager: NetManagerRef,
        sn_list: Arc<SnList>,
        ttp_client: TtpClientRef,
    ) -> Self {
        Self {
            net_manager,
            sn_list,
            ttp_client,
        }
    }

    async fn open_cmd_tunnel(
        &self,
        local_ep: Option<&Endpoint>,
        remote_ep: &Endpoint,
        remote_id: &P2pId,
        remote_name: String,
    ) -> CmdResult<ClassifiedCmdTunnel<SnTunnelRead, SnTunnelWrite>> {
        let (meta, read, write) =
            self.ttp_client
                .open_stream(
                    &TtpTarget {
                        local_ep: local_ep.copied(),
                        remote_ep: *remote_ep,
                        remote_id: remote_id.clone(),
                        remote_name: Some(remote_name.clone()),
                    },
                    TunnelPurpose::from_value(&SN_CMD_SERVICE.to_string()).map_err(
                        into_cmd_err!(CmdErrorCode::Failed, "encode sn cmd purpose failed"),
                    )?,
                )
                .await
                .map_err(into_cmd_err!(
                    CmdErrorCode::Failed,
                    "open sn cmd stream failed"
                ))?;
        let local = meta
            .local_ep
            .unwrap_or(local_ep.copied().unwrap_or_default());
        let remote = meta.remote_ep.unwrap_or(*remote_ep);
        let local_id = meta.local_id;
        let remote_id = meta.remote_id;
        Ok(ClassifiedCmdTunnel::new(
            SnTunnelRead::new(read, local, remote, local_id.clone(), remote_id.clone()),
            SnTunnelWrite::new(write, local, remote, local_id, remote_id),
        ))
    }

    async fn open_cmd_tunnel_to_sn(
        &self,
        local_ep: Option<&Endpoint>,
        remote_ep: &Endpoint,
    ) -> CmdResult<ClassifiedCmdTunnel<SnTunnelRead, SnTunnelWrite>> {
        for sn_cert in self.sn_list.get_sn_list().iter() {
            for sn_ep in sn_cert.endpoints().iter() {
                if sn_ep.protocol() == remote_ep.protocol() && sn_ep == remote_ep {
                    return self
                        .open_cmd_tunnel(local_ep, sn_ep, &sn_cert.get_id(), sn_cert.get_name())
                        .await;
                }
            }
        }
        Err(cmd_err!(CmdErrorCode::Failed, "create tunnel failed"))
    }
}

#[async_trait::async_trait]
impl ClassifiedCmdTunnelFactory<SnTunnelClassification, (), SnTunnelRead, SnTunnelWrite>
    for SnClientTunnelFactory
{
    async fn create_tunnel(
        &self,
        classification: Option<SnTunnelClassification>,
    ) -> CmdResult<ClassifiedCmdTunnel<SnTunnelRead, SnTunnelWrite>> {
        if let Some(classification) = classification {
            if let Some(local_ep) = classification.local_ep.as_ref() {
                return self
                    .open_cmd_tunnel_to_sn(Some(local_ep), &classification.remote_ep)
                    .await;
            }

            for info in self
                .net_manager
                .get_listener_info(classification.remote_ep.protocol())
            {
                if let Ok(tunnel) = self
                    .open_cmd_tunnel_to_sn(Some(&info.local), &classification.remote_ep)
                    .await
                {
                    return Ok(tunnel);
                }
            }

            return self
                .open_cmd_tunnel_to_sn(None, &classification.remote_ep)
                .await;
        }

        let mut listener_entries = self.net_manager.listener_info_entries();
        listener_entries
            .sort_by_key(|(protocol, _)| if *protocol == Protocol::Quic { 0 } else { 1 });
        for (protocol, listeners) in listener_entries {
            for sn_cert in self.sn_list.get_sn_list().iter() {
                for sn_ep in sn_cert.endpoints().iter() {
                    if sn_ep.protocol() != protocol {
                        continue;
                    }
                    for listener in listeners.iter() {
                        if let Ok(tunnel) = self
                            .open_cmd_tunnel(
                                Some(&listener.local),
                                sn_ep,
                                &sn_cert.get_id(),
                                sn_cert.get_name(),
                            )
                            .await
                        {
                            return Ok(tunnel);
                        }
                    }
                }
            }
        }
        Err(cmd_err!(CmdErrorCode::Failed, "create tunnel failed"))
    }
}

pub type SnCmdClient = DefaultClassifiedCmdClient<
    SnTunnelClassification,
    (),
    SnTunnelRead,
    SnTunnelWrite,
    SnClientTunnelFactory,
    u16,
    u8,
>;

pub type SnCmdClientRef = Arc<SnCmdClient>;

pub trait SnLocalIpProvider: 'static + Send + Sync {
    fn get_local_ips(&self) -> Vec<IpAddr>;
}

pub type SnLocalIpProviderRef = Arc<dyn SnLocalIpProvider>;

pub struct DefaultSnLocalIpProvider;

impl DefaultSnLocalIpProvider {
    fn should_ignore_interface(name: &str) -> bool {
        name.contains("VMware")
            || name.contains("VirtualBox")
            || name.contains("ZeroTier")
            || name.starts_with("zt")
            || name.contains("Tun")
            || name.contains("tun")
            || name.contains("utun")
            || name.contains("docker")
            || name.contains("lo")
            || name.contains("veth")
            || name.contains("feth")
            || name.contains("V-M")
            || name.contains("br-")
            || name.contains("vEthernet")
    }
}

impl SnLocalIpProvider for DefaultSnLocalIpProvider {
    fn get_local_ips(&self) -> Vec<IpAddr> {
        if_addrs::get_if_addrs()
            .map(|addrs| {
                addrs
                    .iter()
                    .filter(|addr| {
                        !Self::should_ignore_interface(&addr.name) && !addr.ip().is_loopback()
                    })
                    .map(|addr| addr.addr.ip())
                    .collect::<Vec<IpAddr>>()
            })
            .unwrap_or_default()
    }
}

pub struct SNClientService {
    net_manager: NetManagerRef,
    sn_list: Arc<SnList>,
    local_identity: P2pIdentityRef,
    gen_seq: Arc<SequenceGenerator>,
    gen_id: Arc<TunnelIdGenerator>,
    ping_timeout: Duration,
    call_timeout: Duration,
    conn_timeout: Duration,
    state: RwLock<SNServiceState>,
    listener: Mutex<Option<SNEventRef>>,
    cert_factory: P2pIdentityCertFactoryRef,
    cmd_client: SnCmdClientRef,
    ttp_client: TtpClientRef,
    cmd_version: u8,
    local_ip_provider: SnLocalIpProviderRef,
}
pub type SNClientServiceRef = Arc<SNClientService>;

impl Drop for SNClientService {
    fn drop(&mut self) {
        log::info!(
            "SNClientService drop.device = {}",
            self.local_identity.get_id()
        );
    }
}

impl SNClientService {
    pub fn new(
        net_manager: NetManagerRef,
        sn_list: Vec<P2pSn>,
        local_identity: P2pIdentityRef,
        gen_seq: Arc<SequenceGenerator>,
        gen_id: Arc<TunnelIdGenerator>,
        cert_factory: P2pIdentityCertFactoryRef,
        tunnel_count: u16,
        ping_timeout: Duration,
        call_timeout: Duration,
        conn_timeout: Duration,
    ) -> Arc<Self> {
        Self::new_with_local_ip_provider(
            net_manager,
            sn_list,
            local_identity,
            gen_seq,
            gen_id,
            cert_factory,
            tunnel_count,
            ping_timeout,
            call_timeout,
            conn_timeout,
            Arc::new(DefaultSnLocalIpProvider),
        )
    }

    pub fn new_with_local_ip_provider(
        net_manager: NetManagerRef,
        sn_list: Vec<P2pSn>,
        local_identity: P2pIdentityRef,
        gen_seq: Arc<SequenceGenerator>,
        gen_id: Arc<TunnelIdGenerator>,
        cert_factory: P2pIdentityCertFactoryRef,
        tunnel_count: u16,
        ping_timeout: Duration,
        call_timeout: Duration,
        conn_timeout: Duration,
        local_ip_provider: SnLocalIpProviderRef,
    ) -> Arc<Self> {
        let sn_list = Arc::new(SnList::new(sn_list));
        let ttp_client = TtpClient::new(local_identity.clone(), net_manager.clone());
        let cmd_client = DefaultClassifiedCmdClient::new(
            SnClientTunnelFactory::new(net_manager.clone(), sn_list.clone(), ttp_client.clone()),
            tunnel_count,
        );
        let this = Arc::new(Self {
            net_manager,
            sn_list,
            local_identity,
            gen_seq,
            gen_id,
            ping_timeout,
            call_timeout,
            conn_timeout,
            state: RwLock::new(SNServiceState {
                pinging_handle: None,
                active_sn_list: vec![],
                latest_sn_interval: 0,
                cur_report_future: None,
            }),
            listener: Mutex::new(None),
            cert_factory,
            cmd_client,
            ttp_client,
            cmd_version: 0,
            local_ip_provider,
        });
        this.register_cmd_handler();
        this
    }

    pub fn set_listener(&self, listener: impl SNEvent) {
        let mut _listener = self.listener.lock().unwrap();
        *_listener = Some(Arc::new(listener));
    }

    pub fn get_cmd_client(&self) -> &SnCmdClientRef {
        &self.cmd_client
    }

    pub fn get_ttp_client(&self) -> TtpClientRef {
        self.ttp_client.clone()
    }

    pub fn get_net_manager(&self) -> NetManagerRef {
        self.net_manager.clone()
    }

    pub fn get_sn_list(&self) -> Vec<P2pSn> {
        self.sn_list.get_sn_list()
    }

    pub fn get_wan_ip_list(&self) -> Vec<Endpoint> {
        let mut wan_list = Vec::new();
        self.get_active_sn_list()
            .iter()
            .map(|v| v.wan_ep_list.as_slice())
            .flatten()
            .for_each(|ep| {
                wan_list.push(ep.clone());
            });
        wan_list
    }

    pub fn is_same_lan(&self, reverse_list: &Vec<Endpoint>) -> bool {
        let local_wan_list = self.get_wan_ip_list();
        for ep in reverse_list.iter() {
            for wan_ip in local_wan_list.iter() {
                if ep.is_same_ip_addr(wan_ip) {
                    return true;
                }
            }
        }
        false
    }

    fn register_cmd_handler(self: &Arc<Self>) {
        let this = self.clone();
        self.cmd_client.register_cmd_handler(
            PackageCmdCode::SnCallResp as u8,
            move |_local_id: PeerId,
                  _peer_id: PeerId,
                  tunnel_id: CmdTunnelId,
                  _header: SnCmdHeader,
                  mut body: CmdBody| {
                let this = this.clone();
                async move {
                    let resp = SnCallResp::clone_from_slice(body.read_all().await?.as_slice())
                        .map_err(into_cmd_err!(CmdErrorCode::RawCodecError))?;
                    this.sn_call_resp_handle(tunnel_id, resp)
                        .await
                        .map_err(into_cmd_err!(
                            CmdErrorCode::Failed,
                            "sn call resp handle failed"
                        ))?;
                    Ok(None)
                }
            },
        );

        let this = self.clone();
        self.cmd_client.register_cmd_handler(
            PackageCmdCode::SnQueryResp as u8,
            move |_local_id: PeerId,
                  _peer_id: PeerId,
                  tunnel_id: CmdTunnelId,
                  _header: SnCmdHeader,
                  mut body: CmdBody| {
                let this = this.clone();
                async move {
                    let resp = SnQueryResp::clone_from_slice(body.read_all().await?.as_slice())
                        .map_err(into_cmd_err!(CmdErrorCode::RawCodecError))?;
                    this.sn_query_resp_handle(tunnel_id, resp)
                        .await
                        .map_err(into_cmd_err!(
                            CmdErrorCode::Failed,
                            "sn query resp handle failed"
                        ))?;
                    Ok(None)
                }
            },
        );

        let this = self.clone();
        self.cmd_client.register_cmd_handler(
            PackageCmdCode::SnCalled as u8,
            move |_local_id: PeerId,
                  _peer_id: PeerId,
                  tunnel_id: CmdTunnelId,
                  _header: SnCmdHeader,
                  mut body: CmdBody| {
                let this = this.clone();
                async move {
                    let sn_called = SnCalled::clone_from_slice(body.read_all().await?.as_slice())
                        .map_err(into_cmd_err!(CmdErrorCode::RawCodecError))?;
                    this.on_called(tunnel_id, sn_called)
                        .await
                        .map_err(into_cmd_err!(
                            CmdErrorCode::Failed,
                            "sn called handle failed"
                        ))?;
                    Ok(None)
                }
            },
        );

        let this = self.clone();
        self.cmd_client.register_cmd_handler(
            PackageCmdCode::ReportSnResp as u8,
            move |_local_id: PeerId,
                  _peer_id: PeerId,
                  tunnel_id: CmdTunnelId,
                  _header: SnCmdHeader,
                  mut body: CmdBody| {
                let this = this.clone();
                async move {
                    let resp = ReportSnResp::clone_from_slice(body.read_all().await?.as_slice())
                        .map_err(into_cmd_err!(CmdErrorCode::RawCodecError))?;
                    this.report_sn_resp_handle(tunnel_id, resp)
                        .await
                        .map_err(into_cmd_err!(
                            CmdErrorCode::Failed,
                            "report sn resp handle failed"
                        ))?;
                    Ok(None)
                }
            },
        );
    }

    async fn sn_call_resp_handle(&self, conn_id: CmdTunnelId, resp: SnCallResp) -> P2pResult<()> {
        let mut state = self.state.write().unwrap();
        for active_sn in state.active_sn_list.iter_mut() {
            if active_sn.conn_id == conn_id {
                let mut recv_futures = active_sn.recv_future.lock().unwrap();
                if let Some(recv_future) = recv_futures.remove(&resp.seq) {
                    recv_future.notify(SnResp::SnCallResp(resp));
                }
                break;
            }
        }
        Ok(())
    }

    async fn sn_query_resp_handle(&self, conn_id: CmdTunnelId, resp: SnQueryResp) -> P2pResult<()> {
        let mut state = self.state.write().unwrap();
        for active_sn in state.active_sn_list.iter_mut() {
            if active_sn.conn_id == conn_id {
                let mut recv_futures = active_sn.recv_future.lock().unwrap();
                if let Some(recv_future) = recv_futures.remove(&resp.seq) {
                    recv_future.notify(SnResp::SnQueryResp(resp));
                }
                break;
            }
        }
        Ok(())
    }

    async fn report_sn_resp_handle(
        &self,
        tunnel_id: CmdTunnelId,
        resp: ReportSnResp,
    ) -> P2pResult<()> {
        log::info!("report sn resp: {:?}", resp);
        if let Some(cur_report_future) = {
            let mut state = self.state.write().unwrap();
            state.cur_report_future.take()
        } {
            cur_report_future.notify(resp);
        }
        Ok(())
    }

    async fn on_called(&self, conn_id: CmdTunnelId, sn_called: SnCalled) -> P2pResult<()> {
        let listener = {
            let listener = self.listener.lock().unwrap();
            listener.clone()
        };
        let seq = sn_called.seq.clone();
        let sn_peer_id = sn_called.sn_peer_id.clone();
        let to_peer_id = sn_called.to_peer_id.clone();

        log::debug!(
            "sn called recv conn_id={:?} seq={} sn={} to={} reverse_eps={:?} pn_list={:?}",
            conn_id,
            seq.value(),
            sn_peer_id,
            to_peer_id,
            sn_called.reverse_endpoint_array,
            sn_called.active_pn_list
        );

        let resp = if to_peer_id == self.local_identity.get_id() {
            if listener.is_some() {
                log::debug!(
                    "sn called dispatch to listener seq={} conn_id={:?}",
                    seq.value(),
                    conn_id
                );
                match listener.as_ref().unwrap().on_called(sn_called).await {
                    Ok(_) => SnCalledResp {
                        seq,
                        sn_peer_id,
                        result: 0,
                    },
                    Err(e) => {
                        log::info!("on called to {} failed: {:?}", to_peer_id, e);
                        SnCalledResp {
                            seq,
                            sn_peer_id,
                            result: e.code().into_u8(),
                        }
                    }
                }
            } else {
                log::debug!(
                    "sn called seq={} has no listener, respond success directly",
                    seq.value()
                );
                SnCalledResp {
                    seq,
                    sn_peer_id,
                    result: 0,
                }
            }
        } else {
            SnCalledResp {
                seq,
                sn_peer_id,
                result: P2pErrorCode::TargetNotFound.into_u8(),
            }
        };

        log::debug!(
            "sn called resp conn_id={:?} seq={} result={}",
            conn_id,
            resp.seq.value(),
            resp.result
        );

        self.cmd_client
            .send_by_specify_tunnel(
                conn_id,
                PackageCmdCode::SnCalledResp as u8,
                self.cmd_version,
                resp.to_vec()
                    .map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?
                    .as_slice(),
            )
            .await
            .map_err(into_p2p_err!(
                P2pErrorCode::IoError,
                "send SnCalledResp failed"
            ))?;
        Ok(())
    }

    pub async fn start(self: &Arc<Self>) -> P2pResult<()> {
        let this = self.clone();
        let handle = Executor::spawn_with_handle(async move {
            this.ping_proc().await;
        })
        .map_err(into_p2p_err!(
            P2pErrorCode::Failed,
            "start sn ping proc failed"
        ))?;
        {
            let mut state = self.state.write().unwrap();
            state.pinging_handle = Some(handle);
        }
        Ok(())
    }

    pub async fn stop(&self) {
        {
            let mut state = self.state.write().unwrap();
            state.active_sn_list.clear();
            if let Some(handle) = state.pinging_handle.take() {
                handle.abort();
            }
        }
    }

    pub async fn reset_sn(self: &Arc<Self>, sn_list: Vec<P2pSn>) {
        self.sn_list.update_sn_list(sn_list);
        self.stop().await;
        self.cmd_client.clear_all_tunnel().await;
        self.start().await;
    }

    async fn ping_proc(self: &Arc<Self>) {
        loop {
            {
                let (active_sn_count, latest_sn_interval, cur_sn_interval) = {
                    let mut state = self.state.write().unwrap();
                    if state.active_sn_list.len() > 0 {
                        state.latest_sn_interval = 10;
                        (
                            state.active_sn_list.len(),
                            state.latest_sn_interval,
                            state.latest_sn_interval,
                        )
                    } else {
                        let cur_sn_interval = state.latest_sn_interval;
                        if state.latest_sn_interval == 0 {
                            state.latest_sn_interval = 1;
                        } else if state.latest_sn_interval == 10 {
                            state.latest_sn_interval = 1;
                        } else {
                            state.latest_sn_interval = state.latest_sn_interval * 2;
                        }
                        if state.latest_sn_interval > 600 {
                            state.latest_sn_interval = 600;
                        }
                        (
                            state.active_sn_list.len(),
                            cur_sn_interval,
                            state.latest_sn_interval,
                        )
                    }
                };
                if latest_sn_interval != 0 {
                    runtime::sleep(Duration::from_secs(cur_sn_interval)).await;
                }
                if active_sn_count > 0 {
                    let mut ping_sn_list = Vec::new();
                    {
                        let mut state = self.state.write().unwrap();
                        for active_sn in state.active_sn_list.iter_mut() {
                            if bucky_time_now() - active_sn.latest_time > 600 * 1000 * 1000 {
                                active_sn.latest_time = bucky_time_now();
                                ping_sn_list.push(active_sn.clone());
                            }
                        }
                    }

                    for active_sn in ping_sn_list.iter() {
                        match self
                            .report(active_sn.conn_id, active_sn.sn_peer_id.clone())
                            .await
                        {
                            Ok(_) => {}
                            Err(e) => {
                                log::error!("ping to {} failed: {:?}", active_sn.sn_peer_id, e);
                                continue;
                            }
                        }
                    }
                    continue;
                }
            }
            for (protocol, listeners) in self.net_manager.listener_info_entries() {
                for sn_cert in self.sn_list.get_sn_list().iter() {
                    for sn_ep in sn_cert.endpoints().iter() {
                        if sn_ep.protocol() != protocol {
                            continue;
                        }
                        for listener in listeners.iter() {
                            let local_ep = listener.local;
                            let ret = if local_ep.addr().ip().is_unspecified() {
                                self.cmd_client
                                    .find_tunnel_id_by_classified(SnTunnelClassification::new(
                                        None,
                                        sn_ep.clone(),
                                    ))
                                    .await
                            } else {
                                if local_ep.protocol() == Protocol::Tcp {
                                    let mut ep = local_ep;
                                    ep.mut_addr().set_port(0);
                                    self.cmd_client
                                        .find_tunnel_id_by_classified(SnTunnelClassification::new(
                                            Some(ep),
                                            sn_ep.clone(),
                                        ))
                                        .await
                                } else {
                                    self.cmd_client
                                        .find_tunnel_id_by_classified(SnTunnelClassification::new(
                                            Some(local_ep),
                                            sn_ep.clone(),
                                        ))
                                        .await
                                }
                            };

                            if ret.is_err() {
                                continue;
                            }

                            let tunnel_id = ret.unwrap();

                            let report_resp = match self.report(tunnel_id, sn_cert.get_id()).await {
                                Ok(resp) => resp,
                                Err(e) => {
                                    log::error!("ping to {} failed: {:?}", sn_cert.get_id(), e);
                                    continue;
                                }
                            };

                            let active_sn = ActiveSN {
                                sn_peer_id: sn_cert.get_id(),
                                latest_time: bucky_time_now(),
                                conn_id: tunnel_id,
                                recv_future: Arc::new(Mutex::new(HashMap::new())),
                                wan_ep_list: report_resp.end_point_array,
                            };
                            let mut state = self.state.write().unwrap();
                            state.active_sn_list.push(active_sn);
                        }
                    }
                }
            }
        }
    }

    fn remove_sn_conn(&self, conn_id: CmdTunnelId) {
        let mut state = self.state.write().unwrap();
        state.active_sn_list.retain(|sn| sn.conn_id != conn_id);
    }

    pub async fn wait_online(&self, timeout: Option<Duration>) -> P2pResult<()> {
        let expire = if timeout.is_some() {
            Some(Utc::now().add(timeout.unwrap()))
        } else {
            None
        };
        loop {
            {
                if expire.is_some() {
                    if Utc::now() > expire.unwrap() {
                        return Err(p2p_err!(P2pErrorCode::Timeout, "wait online timeout"));
                    }
                }
                let state = self.state.read().unwrap();
                if state.active_sn_list.len() > 0 {
                    break;
                }
            }
            runtime::sleep(Duration::from_secs(1)).await;
        }
        Ok(())
    }

    pub fn get_active_sn_list(&self) -> Vec<ActiveSN> {
        let state = self.state.read().unwrap();
        state.active_sn_list.clone()
    }

    async fn report(&self, tunnel_id: CmdTunnelId, sn_peer_id: P2pId) -> P2pResult<ReportSnResp> {
        let seq = self.gen_seq.generate();
        let local_ips = self.local_ip_provider.get_local_ips();

        let mut local_eps = Vec::new();
        let mut map_ports = Vec::new();
        for (protocol, listeners) in self.net_manager.listener_info_entries() {
            for listener in listeners.iter() {
                if let Some(tcp_map_port) = listener.mapping_port {
                    map_ports.push((protocol, tcp_map_port));
                }
                if listener.local.addr().ip().is_unspecified() {
                    for ip in local_ips.iter() {
                        local_eps.push(Endpoint::from((
                            protocol,
                            *ip,
                            listener.local.addr().port(),
                        )));
                    }
                } else {
                    local_eps.push(listener.local);
                }
            }
        }

        let report = ReportSn {
            protocol_version: 0,
            stack_version: 0,
            seq,
            sn_peer_id,
            from_peer_id: Some(self.local_identity.get_id()),
            peer_info: Some(
                self.local_identity
                    .get_identity_cert()?
                    .get_encoded_cert()?,
            ),
            send_time: bucky_time_now(),
            contract_id: None,
            receipt: None,
            map_ports,
            local_eps,
        };
        let (notify, waiter) = Notify::<ReportSnResp>::new();
        {
            let mut state = self.state.write().unwrap();
            state.cur_report_future = Some(notify);
        }
        match self
            .cmd_client
            .send_by_specify_tunnel(
                tunnel_id,
                PackageCmdCode::ReportSn as u8,
                self.cmd_version,
                report
                    .to_vec()
                    .map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?
                    .as_slice(),
            )
            .await
        {
            Ok(_) => {}
            Err(e) => {
                self.remove_sn_conn(tunnel_id);
                return Err(p2p_err!(P2pErrorCode::IoError));
            }
        }
        let resp = runtime::timeout(self.call_timeout, waiter)
            .await
            .map_err(into_p2p_err!(P2pErrorCode::ConnectFailed, "report timeout"))?;
        {
            let mut state = self.state.write().unwrap();
            state.cur_report_future = None;
        }
        Ok(resp)
    }

    pub async fn call(
        &self,
        tunnel_id: TunnelId,
        reverse_endpoints: Option<&[Endpoint]>,
        remote: &P2pId,
        call_type: TunnelType,
        payload_pkg: Vec<u8>,
    ) -> P2pResult<SnCallResp> {
        let active_list = self.get_active_sn_list();
        for active in active_list.iter() {
            let seq = self.gen_seq.generate();
            let call = SnCall {
                protocol_version: 0,
                stack_version: 0,
                seq,
                tunnel_id,
                sn_peer_id: active.sn_peer_id.clone(),
                to_peer_id: remote.clone(),
                from_peer_id: self.local_identity.get_id().clone(),
                reverse_endpoint_array: reverse_endpoints.map(|ep_list| Vec::from(ep_list)),
                active_pn_list: None,
                peer_info: Some(
                    self.local_identity
                        .get_identity_cert()?
                        .get_encoded_cert()?,
                ),
                send_time: bucky_time_now(),
                call_type,
                payload: payload_pkg.clone(),
                is_always_call: false,
            };

            log::debug!(
                "sn call send sn={} conn_id={:?} seq={} tunnel_id={:?} remote={} reverse_eps={:?} payload_len={} call_type={:?}",
                active.sn_peer_id,
                active.conn_id,
                seq.value(),
                tunnel_id,
                remote,
                call.reverse_endpoint_array,
                call.payload.len(),
                call.call_type
            );

            let (notify, waiter) = Notify::<SnResp>::new();
            {
                let mut recv_future = active.recv_future.lock().unwrap();
                recv_future.insert(seq, notify);
            }
            if let Err(e) = self
                .cmd_client
                .send_by_specify_tunnel(
                    active.conn_id,
                    PackageCmdCode::SnCall as u8,
                    self.cmd_version,
                    call.to_vec()
                        .map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?
                        .as_slice(),
                )
                .await
            {
                log::error!("send call to {} failed: {:?}", active.sn_peer_id, e);
                self.remove_sn_conn(active.conn_id);
                continue;
            }

            let resp = match runtime::timeout(self.call_timeout, waiter).await {
                Ok(resp) => match resp {
                    SnResp::SnCallResp(resp) => {
                        log::debug!(
                            "sn call resp sn={} conn_id={:?} seq={} result={}",
                            active.sn_peer_id,
                            active.conn_id,
                            resp.seq.value(),
                            resp.result
                        );
                        resp
                    }
                    SnResp::SnQueryResp(_) => {
                        log::error!("unexpect resp");
                        continue;
                    }
                },
                Err(_) => {
                    let mut recv_future = active.recv_future.lock().unwrap();
                    recv_future.remove(&seq);
                    log::warn!(
                        "sn call timeout sn={} conn_id={:?} seq={} remote={} timeout_ms={}",
                        active.sn_peer_id,
                        active.conn_id,
                        seq.value(),
                        remote,
                        self.call_timeout.as_millis()
                    );
                    continue;
                }
            };

            return Ok(resp);
        }
        Err(p2p_err!(P2pErrorCode::ConnectFailed, "call timeout"))
    }

    pub async fn query(&self, device_id: &P2pId) -> P2pResult<SnQueryResp> {
        let active_list = self.get_active_sn_list();
        for active in active_list.iter() {
            let seq = self.gen_seq.generate();
            let query = SnQuery {
                protocol_version: 0,
                stack_version: 0,
                seq,
                query_id: device_id.clone(),
            };
            let (notify, waiter) = Notify::<SnResp>::new();
            {
                let mut recv_future = active.recv_future.lock().unwrap();
                recv_future.insert(seq, notify);
            }
            if let Err(e) = self
                .cmd_client
                .send_by_specify_tunnel(
                    active.conn_id,
                    PackageCmdCode::SnQuery as u8,
                    self.cmd_version,
                    query
                        .to_vec()
                        .map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?
                        .as_slice(),
                )
                .await
            {
                log::error!("send call to {} failed: {:?}", active.sn_peer_id, e);
                self.remove_sn_conn(active.conn_id);
                continue;
            }

            let resp = match runtime::timeout(self.call_timeout, waiter).await {
                Ok(resp) => match resp {
                    SnResp::SnQueryResp(resp) => resp,
                    SnResp::SnCallResp(_) => {
                        log::error!("unexpect resp");
                        continue;
                    }
                },
                Err(_) => {
                    let mut recv_future = active.recv_future.lock().unwrap();
                    recv_future.remove(&seq);
                    log::error!("call to {} timeout", active.sn_peer_id);
                    continue;
                }
            };

            return Ok(resp);
        }
        Err(p2p_err!(P2pErrorCode::ConnectFailed, "no active sn"))
    }
}
