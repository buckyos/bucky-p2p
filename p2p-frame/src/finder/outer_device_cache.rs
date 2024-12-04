use async_trait::async_trait;

use crate::error::BdtResult;
use crate::p2p_identity::{P2pId, P2pIdentityCertRef};

#[async_trait]
pub trait OuterDeviceCache: Sync + Send + 'static {
    // 添加一个device并保存
    async fn add(&self, device_id: &P2pId, device: P2pIdentityCertRef);

    // 直接在本地数据查询
    async fn get(&self, device_id: &P2pId) -> Option<P2pIdentityCertRef>;

    // flush device from memory cache
    async fn flush(&self, device_id: &P2pId);

    // 本地查询，查询不到则发起查找操作
    async fn search(&self, device_id: &P2pId) -> BdtResult<P2pIdentityCertRef>;

    // 校验device的owner签名是否有效
    async fn verfiy_owner(&self, device_id: &P2pId, device: Option<&P2pIdentityCertRef>) -> BdtResult<()>;

    fn clone_cache(&self) -> Box<dyn OuterDeviceCache>;
}
