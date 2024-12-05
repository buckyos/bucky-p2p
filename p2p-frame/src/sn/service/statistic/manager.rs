
use std::{sync::{RwLock, Arc}, collections::{BTreeMap, }, time::Duration};
use std::net::SocketAddr;
use bucky_time::bucky_time_now;

use once_cell::sync::OnceCell;
use crate::executor::Executor;
use crate::p2p_identity::P2pId;
use crate::types::Timestamp;
use super::super::storage::SqliteStorage;

use super::{PeerStatus, };

const CACHE_MAX_TIMEOUT: Duration = Duration::from_secs(5);

struct StatisticImpl {
}

struct StatisticManagerImpl {
    storage: Option<Arc<SqliteStorage>>,
    last_cache_timestamp: Timestamp,
    statistics: BTreeMap<String, PeerStatus>,
}

#[derive(Clone)]
pub struct StatisticManager(Arc<RwLock<StatisticManagerImpl>>);

impl StatisticManager {
    fn new() -> Self {
        let ret = Self(Arc::new(RwLock::new(StatisticManagerImpl{
            storage: None,
            last_cache_timestamp: bucky_time_now(),
            statistics: BTreeMap::new(),
        })));

        let arc_ret = ret.clone();
        let _ = Executor::spawn(async move {
            let mut storage = SqliteStorage::new();
            match storage.init("sn-statistic").await {
                Ok(_) => {
                    arc_ret.0.write().unwrap()
                        .storage = Some(Arc::new(storage));
                }
                Err(err) => {
                    error!("failed to init statistic-db with err = {}", err);
                }
            }
        });

        ret

    }
}

impl StatisticManager {
    pub fn get_instance() -> &'static Self {
        static INSTANCE: OnceCell<StatisticManager> = OnceCell::new();
        INSTANCE.get_or_init(|| Self::new())
    }

    pub fn get_peer_status(&self, id: P2pId, now: Timestamp) -> PeerStatus {
        self.0.write().unwrap()
            .statistics
            .entry(id.to_string())
            .or_insert(PeerStatus::with_peer(id, now))
            .clone()
    }

    pub fn get_endpoint_status(&self, endpoint: SocketAddr) -> PeerStatus {
        self.0.write().unwrap()
            .statistics
            .entry(endpoint.to_string())
            .or_insert(PeerStatus::with_endpoint(endpoint))
            .clone()
    }
}

impl StatisticManager {
    pub fn on_time_escape(&self, now: Timestamp) {
        let (all, storage) = {
            let manager = &mut *self.0.write().unwrap();

            if now > manager.last_cache_timestamp &&
               now - manager.last_cache_timestamp >= CACHE_MAX_TIMEOUT.as_micros() as u64 {
                let all: Vec<PeerStatus> = manager.statistics.values().cloned().collect();
                manager.last_cache_timestamp = now;
                (Some(all), manager.storage.clone())
            } else {
                (None, None)
            }
        };

        if let Some(storage) = storage {
            let storage = unsafe {&mut *(Arc::as_ptr(&storage) as *mut SqliteStorage)};
            let _ = Executor::spawn(async move {
                if let Some(all) = all {
                    for a in all.iter() {
                        a.storage(storage).await;
                    }
                }

            });
        }
    }
}
