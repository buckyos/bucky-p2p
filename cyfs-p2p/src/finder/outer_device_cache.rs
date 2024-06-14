use async_trait::async_trait;

use std::sync::Arc;
use bucky_objects::{AnyNamedObject, Device, DeviceId, ObjectId};
use crate::error::BdtResult;

#[async_trait]
pub trait OuterDeviceCache: Sync + Send + 'static {
    // 添加一个device并保存
    async fn add(&self, device_id: &DeviceId, device: Device);

    // 直接在本地数据查询
    async fn get(&self, device_id: &DeviceId) -> Option<Device>;

    // flush device from memory cache
    async fn flush(&self, device_id: &DeviceId);

    // 本地查询，查询不到则发起查找操作
    async fn search(&self, device_id: &DeviceId) -> BdtResult<Device>;

    // 校验device的owner签名是否有效
    async fn verfiy_owner(&self, device_id: &DeviceId, device: Option<&Device>) -> BdtResult<()>;

    // 有权对象的body签名自校验
    async fn verfiy_own_signs(&self, object_id: &ObjectId, object: &Arc<AnyNamedObject>) -> BdtResult<()>;

    fn clone_cache(&self) -> Box<dyn OuterDeviceCache>;
}
