use log::*;
use std::{
    sync::RwLock,
    collections::BTreeSet
};
use std::sync::Arc;
use cyfs_base::*;
use crate::{
    protocol::{*, v0::*},
};
use crate::finder::DeviceCache;
use crate::receive_processor::{ReceiveProcessor, ReceiveProcessorRef, RespSender};
use crate::sockets::DataSender;
use crate::types::MixAesKey;

struct Proxies {
    active_proxies: BTreeSet<DeviceId>,
    passive_proxies: BTreeSet<DeviceId>,
    dump_proxies: BTreeSet<DeviceId>,
}

impl Proxies {
    fn new() -> Self {
        Self {
            active_proxies: Default::default(),
            passive_proxies: Default::default(),
            dump_proxies: Default::default(),
        }
    }
}

pub struct ProxyManager {
    device_cache: Arc<DeviceCache>,
    proxies: RwLock<Proxies>
}


impl std::fmt::Display for ProxyManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ProxyManager")
    }
}

impl ProxyManager {
    pub(crate) fn new(device_cache: Arc<DeviceCache>,) -> Self {
        Self {
            device_cache,
            proxies: RwLock::new(Proxies::new()),
        }
    }

    pub fn register_pkg_processor(self: &Arc<Self>, processor: &mut ReceiveProcessor) {
        let this = self.clone();
        processor.add_package_box_processor(PackageCmdCode::AckProxy,  move|resp_sender: &'static mut RespSender,
                                                                            pkg: DynamicPackage| {
            let this = this.clone();
            async move {
                Ok(())
            }
        });
    }

    pub fn add_active_proxy(&self, proxy: &Device) {
        let proxy_id = proxy.desc().device_id();
        info!("{} add active proxy {}", self, proxy_id);
        self.device_cache.add_static(&proxy_id, proxy);
        let _ = self.proxies.write().unwrap().active_proxies.insert(proxy_id);
    }

    pub fn remove_active_proxy(&self, proxy: &DeviceId) -> bool {
        self.proxies.write().unwrap().active_proxies.remove(proxy)
    }

    pub fn active_proxies(&self) -> Vec<DeviceId> {
        self.proxies.read().unwrap().active_proxies.iter().cloned().collect()
    }

     pub fn add_passive_proxy(&self, proxy: &Device) {
        let proxy_id = proxy.desc().device_id();
        info!("{} add passive proxy {}", self, proxy_id);
        self.device_cache.add_static(&proxy_id, proxy);
        let mut proxies = self.proxies.write().unwrap();
        let _ = proxies.passive_proxies.insert(proxy_id.clone());
        let _ = proxies.active_proxies.insert(proxy_id);
    }

    pub fn remove_passive_proxy(&self, proxy: &DeviceId) -> bool {
        let mut proxies = self.proxies.write().unwrap();
        let _ = proxies.active_proxies.remove(proxy);
        proxies.passive_proxies.remove(proxy)
    }

    pub fn passive_proxies(&self) -> Vec<DeviceId> {
        self.proxies.read().unwrap().passive_proxies.iter().cloned().collect()
    }

    pub fn add_dump_proxy(&self, proxy: &Device) {
        let proxy_id = proxy.desc().device_id();
        info!("{} add dump proxy {}", self, proxy_id);
        self.device_cache.add_static(&proxy_id, proxy);
        let _ = self.proxies.write().unwrap().dump_proxies.insert(proxy_id);
    }

    pub fn remove_dump_proxy(&self, proxy: &DeviceId) -> bool {
        self.proxies.write().unwrap().dump_proxies.remove(proxy)
    }

    pub fn dump_proxies(&self) -> Vec<DeviceId> {
        self.proxies.read().unwrap().dump_proxies.iter().cloned().collect()
    }

    // fn on_ack_proxy_pkg(&self,
    //                     resp_sender: &mut Box<dyn RespSender>,
    //                     remote_device_id: &DeviceId,
    //                     key: &MixAesKey,
    //                     first_package: &DynamicPackage) -> BuckyResult<()> {
    //     if first_package.cmd_code() == PackageCmdCode::AckProxy {
    //         let ack_proxy: &AckProxy = first_package.as_ref();
    //         trace!("{} got {:?} from {}", self, ack_proxy, package_box.as_ref().remote());
    //         let stack = Stack::from(&self.stack);
    //         if let Some(tunnel) = stack.tunnel_manager().container_of(&ack_proxy.to_peer_id) {
    //             let _ = tunnel.on_package(ack_proxy, package_box.as_ref().remote())?;
    //             Ok(())
    //         } else {
    //             let err = BuckyError::new(BuckyErrorCode::NotFound, "tunnel not exists");
    //             debug!("{} ignore {:?} from {} for {}", self, ack_proxy, package_box.as_ref().remote(), err);
    //             Err(err)
    //         }
    //     } else {
    //         let err = BuckyError::new(BuckyErrorCode::InvalidInput, format!("package box with first package {:?}", first_package.cmd_code()));
    //         debug!("{} ignore package from {} for {}", self, package_box.as_ref().remote(), err);
    //         Err(err)
    //     }
    // }
}
