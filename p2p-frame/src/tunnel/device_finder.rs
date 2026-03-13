use crate::error::{P2pErrorCode, P2pResult, p2p_err};
use crate::p2p_identity::{
    P2pId, P2pIdentityCertCacheRef, P2pIdentityCertFactoryRef, P2pIdentityCertRef,
};
use crate::sn::client::SNClientServiceRef;
use std::sync::Arc;
use std::time::Duration;

#[async_trait::async_trait]
pub trait DeviceFinder: 'static + Send + Sync {
    async fn get_identity_cert(&self, device_id: &P2pId) -> P2pResult<P2pIdentityCertRef>;
}

pub type DeviceFinderRef = Arc<dyn DeviceFinder>;

pub struct DefaultDeviceFinder {
    cert_cache: P2pIdentityCertCacheRef,
    sn_service: SNClientServiceRef,
    cert_factory: P2pIdentityCertFactoryRef,
    query_cache: mini_moka::sync::Cache<P2pId, u64>,
}

impl DefaultDeviceFinder {
    pub fn new(
        sn_service: SNClientServiceRef,
        cert_factory: P2pIdentityCertFactoryRef,
        cert_cache: P2pIdentityCertCacheRef,
        interval: Duration,
    ) -> Arc<Self> {
        Arc::new(Self {
            cert_cache,
            sn_service,
            cert_factory,
            query_cache: mini_moka::sync::Cache::builder()
                .time_to_live(interval)
                .build(),
        })
    }
}

#[async_trait::async_trait]
impl DeviceFinder for DefaultDeviceFinder {
    async fn get_identity_cert(&self, device_id: &P2pId) -> P2pResult<P2pIdentityCertRef> {
        if let Some(device) = self.cert_cache.get(device_id).await {
            return Ok(device);
        }

        if self.query_cache.contains_key(device_id) {
            return Err(p2p_err!(P2pErrorCode::NotFound, "device not found"));
        }

        let resp = self.sn_service.query(device_id).await?;
        log::info!("query device {} resp {:?}", device_id, resp);
        let peer_info = resp
            .peer_info
            .ok_or_else(|| p2p_err!(P2pErrorCode::NotFound, "device not found"))?;
        let mut device = self.cert_factory.create(&peer_info)?;
        if !resp.end_point_array.is_empty() {
            let mut eps = device.endpoints();
            for wan_ep in &resp.end_point_array {
                let has = eps
                    .iter()
                    .any(|ep| ep.protocol() == wan_ep.protocol() && ep.addr() == wan_ep.addr());
                if !has {
                    eps.push(*wan_ep);
                }
            }

            device = device.update_endpoints(eps);
        }
        self.cert_cache.add(device_id, &device).await;
        Ok(device)
    }
}
