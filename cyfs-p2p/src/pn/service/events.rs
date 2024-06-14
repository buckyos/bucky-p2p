use bucky_crypto::AesKey;
use crate::error::BdtResult;
use super::proxy::ProxyDeviceStub;

#[async_trait::async_trait]
pub trait ProxyServiceEvents: Send + Sync {
    async fn pre_create_tunnel(&self, mix_key: &AesKey, device_pair: &(ProxyDeviceStub, ProxyDeviceStub)) -> BdtResult<()>;
}
