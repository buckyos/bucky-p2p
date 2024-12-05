use std::{
    sync::{Mutex},
    time::Duration,
    collections::{BTreeMap}
};
use mini_moka::sync::{Cache, CacheBuilder};
use crate::executor::Executor;
use crate::p2p_identity::{P2pId, P2pIdentityCertRef};
use super::outer_device_cache::*;

#[derive(Clone)]
pub struct DeviceCacheConfig {
    pub expire: Duration,
    pub capacity: usize
}

struct MemCaches {
    lru_caches: Cache<P2pId, P2pIdentityCertRef>,
    static_caches: BTreeMap<P2pId, P2pIdentityCertRef>
}

impl MemCaches {
    fn new(config: &DeviceCacheConfig) -> Self {
        Self {
            static_caches: BTreeMap::new(),
            lru_caches: CacheBuilder::new(config.capacity as u64).time_to_idle(config.expire).build()
        }
    }

    fn remove(&mut self, remote: &P2pId) {
        self.static_caches.remove(remote);
        self.lru_caches.invalidate(remote);
    }

    fn get(&mut self, remote: &P2pId) -> Option<P2pIdentityCertRef> {
        self.lru_caches.get(remote).or_else(|| self.static_caches.get(remote).map(|v| v.clone()))
    }
}

pub struct DeviceCache {
    outer: Option<Box<dyn OuterDeviceCache>>,
    //FIXME 先简单干一个
    cache: Mutex<MemCaches>,
}

impl DeviceCache {
    pub fn new(
        config: &DeviceCacheConfig,
        outer: Option<Box<dyn OuterDeviceCache>>,
    ) -> Self {
        Self {
            cache: Mutex::new(MemCaches::new(config)),
            outer,
        }
    }

    pub fn add_static(&self, id: &P2pId, device: &P2pIdentityCertRef) {
        let real_device_id = device.get_id();
        if *id != real_device_id {
            error!("add device but unmatch device_id! param_id={}, calc_id={}", id, real_device_id);
            // panic!("{}", msg);
            return;
        }

        {
            let mut cache = self.cache.lock().unwrap();
            cache.static_caches.insert(id.clone(), device.clone());
        }

        if let Some(outer) = &self.outer {
            let outer = outer.clone_cache();
            let id = id.to_owned();
            let device = device.to_owned();

            let _ = Executor::spawn(async move {
                outer.add(&id, device).await;
            });
        }
    }

    pub fn add(&self, id: &P2pId, device: &P2pIdentityCertRef) {
        let real_device_id = device.get_id();
        if *id != real_device_id {
            error!("add device but unmatch device_id! param_id={}, calc_id={}", id, real_device_id);
            // panic!("{}", msg);
            return;
        }

        {
            let cache = self.cache.lock().unwrap();
            cache.lru_caches.insert(id.clone(), device.clone());
        }

        if let Some(outer) = &self.outer {
            let outer = outer.clone_cache();
            let id = id.to_owned();
            let device = device.to_owned();

            let _ = Executor::spawn(async move {
                outer.add(&id, device).await;
            });
        }
    }

    pub async fn get(&self, id: &P2pId) -> Option<P2pIdentityCertRef> {
        let mem_cache = self.get_inner(id);
        if mem_cache.is_some() {
            mem_cache
        } else if let Some(outer) = &self.outer {
            outer.get(id).await
        } else {
            None
        }
    }

    pub fn get_inner(&self, id: &P2pId) -> Option<P2pIdentityCertRef> {
        self.cache.lock().unwrap().get(id)
    }

    pub fn remove_inner(&self, id: &P2pId) {
        self.cache.lock().unwrap().remove(id);
    }
}
