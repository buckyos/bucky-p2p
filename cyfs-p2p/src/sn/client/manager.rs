use std::{
    time::Duration,
    sync::{Arc, RwLock},
};

use async_std::{
    task,
    future
};

use cyfs_base::*;
use crate::{
    types::*,
    interface::{NetListener, udp::{self, OnUdpPackageBox}},
    protocol::{*, v0::*},
};
use crate::finder::DeviceCache;
use crate::history::keystore::Keystore;
use crate::interface::NetManager;
use crate::pn::client::ProxyManager;
use super::{
    cache::*,
    ping::{PingConfig, PingClients, SnStatus},
    call::{CallConfig, CallManager}
};

pub trait PingClientCalledEvent: Send + Sync + 'static {
    fn on_called(&self, called: &SnCalled) -> Result<(), BuckyError>;
}


#[derive(Clone)]
pub struct Config {
    pub atomic_interval: Duration,
    pub ping: PingConfig,
    pub call: CallConfig,
}

struct ManagerImpl {
    gen_seq: Arc<TempSeqGenerator>,
    cache: Arc<SnCache>,
    ping: RwLock<PingClients>,
    call: CallManager,
    key_store: Arc<Keystore>,
    ping_config: PingConfig,
    local_device: Device,
    called_event_listener: Arc<dyn PingClientCalledEvent>,
}

#[derive(Clone)]
pub struct ClientManager(Arc<ManagerImpl>);

impl ClientManager {
    pub fn create(
        proxy_manager: Arc<ProxyManager>,
        device_cache: Arc<DeviceCache>,
        net_manager: Arc<NetManager>,
        key_store: Arc<Keystore>,
        call_config: CallConfig,
        net_listener: NetListener,
        local_device: Device,
        atomic_interval: Duration,
        ping_config: PingConfig,
        called_event_listener: Arc<dyn PingClientCalledEvent>,) -> Self {
        let gen_seq = Arc::new(TempSeqGenerator::new());
        let cache = Arc::new(SnCache::new());
        let ping = PingClients::new(key_store.clone(),
                                    cache.clone(),
                                    &local_device.desc().device_id(),
                                    gen_seq.clone(),
                                    net_listener,
                                    vec![],
                                    local_device.clone(),
                                    ping_config.clone(),
                                    called_event_listener.clone());
        let call = CallManager::create(proxy_manager, cache.clone(), device_cache, local_device.clone(), net_manager, key_store.clone(), call_config);
        let manager = Self(Arc::new(ManagerImpl {
            cache,
            ping: RwLock::new(ping),
            call,
            gen_seq,
            key_store,
            ping_config,
            local_device,
            called_event_listener,
        }));

        {
            let manager = manager.clone();
            task::spawn(async move {
                loop {
                    let now = bucky_time_now();
                    manager.ping().on_time_escape(now);
                    manager.call().on_time_escape(now);
                    let _ = future::timeout(atomic_interval, future::pending::<()>()).await;
                }
            });
        }
        manager
    }

    pub fn cache(&self) -> &SnCache {
        self.0.cache.as_ref()
    }

    pub fn ping(&self) -> PingClients {
        self.0.ping.read().unwrap().clone()
    }

    pub fn reset(&self) -> Option<PingClients> {
        let next = {
            let mut ping = self.0.ping.write().unwrap();
            if let Some(status) = ping.status() {
                if SnStatus::Offline == status {
                    let to_close = ping.clone();
                    let to_start = PingClients::new(
                        self.0.key_store.clone(),
                        self.0.cache.clone(),
                        &self.0.local_device.desc().device_id(),
                        self.0.gen_seq.clone(),
                        to_close.net_listener().reset(None),
                        to_close.sn_list().clone(),
                        to_close.default_local(),
                        self.0.ping_config.clone(),
                        self.0.called_event_listener.clone()
                    );
                    *ping = to_start.clone();
                    Some((to_start, to_close))
                } else {
                    None
                }
            } else {
                None
            }
        };

        if let Some((to_start, to_close)) = next {
            to_close.stop();
            Some(to_start)
        } else {
            None
        }
    }

    pub fn reset_sn_list(&self, sn_list: Vec<Device>) -> PingClients {
        let (to_start, to_close) = {
            let mut ping = self.0.ping.write().unwrap();
            let to_close = ping.clone();
            let to_start = PingClients::new(
                self.0.key_store.clone(),
                self.0.cache.clone(),
                &self.0.local_device.desc().device_id(),
                self.0.gen_seq.clone(),
                to_close.net_listener().reset(None),
                sn_list,
                to_close.default_local(),
                self.0.ping_config.clone(),
                self.0.called_event_listener.clone()
            );
            *ping = to_start.clone();
            (to_start, to_close)
        };
        to_close.stop();
        to_start
    }

    pub fn reset_endpoints(&self, net_listener: NetListener, local_device: Device) -> PingClients {
        let (to_start, to_close) = {
            let mut ping = self.0.ping.write().unwrap();
            let to_close = ping.clone();
            let to_start = to_close.reset(self.0.key_store.clone(),
                                          self.0.cache.clone(),
                                          net_listener,
                                          local_device,
                                          self.0.ping_config.clone(),
                                          self.0.called_event_listener.clone());
            *ping = to_start.clone();
            (to_start, to_close)
        };
        to_close.stop();
        to_start
    }

    pub fn call(&self) -> &CallManager {
        &self.0.call
    }
}

impl OnUdpPackageBox for ClientManager {
    fn on_udp_package_box(&self, package_box: udp::UdpPackageBox) -> Result<(), BuckyError> {
        let from = package_box.remote().clone();
        let from_interface = package_box.local();
        for pkg in package_box.as_ref().packages() {
            match pkg.cmd_code() {
                PackageCmdCode::SnPingResp => {
                    match pkg.as_any().downcast_ref::<SnPingResp>() {
                        None => return Err(BuckyError::new(BuckyErrorCode::InvalidData, "should be SnPingResp")),
                        Some(ping_resp) => {
                            let _ = self.ping().on_udp_ping_resp(ping_resp, &from, from_interface.clone());
                        }
                    }
                },
                PackageCmdCode::SnCalled => {
                    match pkg.as_any().downcast_ref::<SnCalled>() {
                        None => return Err(BuckyError::new(BuckyErrorCode::InvalidData, "should be SnCalled")),
                        Some(called) => {
                            let _ = self.ping().on_called(called, package_box.as_ref(), &from, from_interface.clone());
                        }
                    }
                },
                PackageCmdCode::SnCallResp => {
                    match pkg.as_any().downcast_ref::<SnCallResp>() {
                        None => return Err(BuckyError::new(BuckyErrorCode::InvalidData, "should be SnCallResp")),
                        Some(call_resp) => {
                            let _ = self.call().on_udp_call_resp(call_resp, from_interface, &from);
                        }
                    }
                },
                _ => {
                    return Err(BuckyError::new(BuckyErrorCode::InvalidData, format!("unkown package({:?})", pkg.cmd_code()).as_str()))
                }
            }
        }

        Ok(())
    }
}

