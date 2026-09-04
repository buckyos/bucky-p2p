use crate::error::{P2pErrorCode, P2pResult, p2p_err};
use crate::nat_type::NatProfile;
use crate::p2p_identity::{
    P2pId, P2pIdentityCertCacheRef, P2pIdentityCertFactoryRef, P2pIdentityCertRef,
};
use crate::sn::client::SNClientServiceRef;
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone)]
pub struct PeerLookupInfo {
    pub identity_cert: P2pIdentityCertRef,
    pub sn_peer_id: Option<P2pId>,
    pub local_net_profile: Option<NatProfile>,
    pub remote_net_profile: Option<NatProfile>,
}

#[async_trait::async_trait]
pub trait DeviceFinder: 'static + Send + Sync {
    async fn get_identity_cert(&self, device_id: &P2pId) -> P2pResult<P2pIdentityCertRef>;

    async fn get_peer_info(&self, device_id: &P2pId) -> P2pResult<PeerLookupInfo> {
        Ok(PeerLookupInfo {
            identity_cert: self.get_identity_cert(device_id).await?,
            sn_peer_id: None,
            local_net_profile: None,
            remote_net_profile: None,
        })
    }
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

    async fn get_peer_info(&self, device_id: &P2pId) -> P2pResult<PeerLookupInfo> {
        let cached = self.cert_cache.get(device_id).await;
        if self.query_cache.contains_key(device_id) {
            return cached
                .map(|identity_cert| PeerLookupInfo {
                    identity_cert,
                    sn_peer_id: None,
                    local_net_profile: None,
                    remote_net_profile: None,
                })
                .ok_or_else(|| p2p_err!(P2pErrorCode::NotFound, "device not found"));
        }

        let query = match self.sn_service.query_with_context(device_id).await {
            Ok(query) => query,
            Err(err) => {
                if let Some(identity_cert) = cached {
                    return Ok(PeerLookupInfo {
                        identity_cert,
                        sn_peer_id: None,
                        local_net_profile: None,
                        remote_net_profile: None,
                    });
                }
                return Err(err);
            }
        };
        log::info!("query device {} resp {:?}", device_id, query.response);
        let Some(peer_info) = query.response.peer_info.as_ref() else {
            self.query_cache.insert(device_id.clone(), 0);
            return cached
                .map(|identity_cert| PeerLookupInfo {
                    identity_cert,
                    sn_peer_id: None,
                    local_net_profile: None,
                    remote_net_profile: None,
                })
                .ok_or_else(|| p2p_err!(P2pErrorCode::NotFound, "device not found"));
        };

        let mut identity_cert = self.cert_factory.create(peer_info)?;
        if !query.response.end_point_array.is_empty() {
            let mut endpoints = identity_cert.endpoints();
            for endpoint in query.response.end_point_array.iter().copied() {
                if !endpoints.iter().any(|existing| {
                    existing.protocol() == endpoint.protocol() && existing.addr() == endpoint.addr()
                }) {
                    endpoints.push(endpoint);
                }
            }
            identity_cert = identity_cert.update_endpoints(endpoints);
        }
        self.cert_cache.add(device_id, &identity_cert).await;

        Ok(PeerLookupInfo {
            identity_cert,
            sn_peer_id: Some(query.sn_peer_id),
            local_net_profile: Some(query.local_net_profile),
            remote_net_profile: query.response.net_profile,
        })
    }
}
