use std::sync::Arc;
use cyfs_base::{BuckyResult, DeviceId, Endpoint};
use crate::MixAesKey;
use crate::protocol::DynamicPackage;
use crate::sockets::{DataSender, SocketType};

pub struct RespSender {
    data_sender: Arc<dyn DataSender>,
    pkg_cache: Vec<DynamicPackage>,
}

impl RespSender {
    pub(super) fn new(data_sender: Arc<dyn DataSender>) -> Self {
        Self {
            data_sender,
            pkg_cache: vec![],
        }
    }

    pub async fn send_resp(&self, data: &[u8]) -> BuckyResult<()> {
        self.data_sender.send_resp(data).await
    }

    pub async fn send_dynamic_pkgs(&mut self, pkgs: Vec<DynamicPackage>) -> BuckyResult<()> {
        self.pkg_cache.extend(pkgs);
        Ok(())
    }

    pub async fn send_dynamic_pkg(&mut self, pkg: DynamicPackage) -> BuckyResult<()> {
        self.pkg_cache.push(pkg);
        Ok(())
    }

    pub(super) async fn send_cache(&mut self) -> BuckyResult<()> {
        if self.pkg_cache.is_empty() {
            return Ok(());
        }
        let pkgs = std::mem::replace(&mut self.pkg_cache, vec![]);
        self.data_sender.send_dynamic_pkgs(pkgs).await
    }

    pub fn clone_data_sender(&self) -> Arc<dyn DataSender> {
        self.data_sender.clone()
    }

    pub fn remote(&self) -> &Endpoint {
        self.data_sender.remote()
    }

    pub fn local(&self) -> &Endpoint {
        self.data_sender.local()
    }

    pub fn remote_device_id(&self) -> &DeviceId {
        self.data_sender.remote_device_id()
    }

    pub fn local_device_id(&self) -> &DeviceId {
        self.data_sender.local_device_id()
    }

    pub fn key(&self) -> &MixAesKey {
        self.data_sender.key()
    }

    pub fn socket_type(&self) -> SocketType {
        self.data_sender.socket_type()
    }
}
