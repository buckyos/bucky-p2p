use log::*;
use std::{
    time::Duration,
    collections::HashMap,
    sync::{Arc}
};
use std::pin::Pin;
use std::sync::Mutex;
use bucky_error::BuckyErrorCode;
use bucky_time::bucky_time_now;
use crate::{
    types::*,
    protocol::{Package, PackageBox}
};
use crate::protocol::MTU_LARGE;
use crate::sockets::DataSender;


struct PackageResendInfo {
    pkg: Arc<PackageBox>,
    sender: Arc<dyn DataSender>,
    interval: Duration,
    times: u8,
    last_time: Timestamp,
    nick_name: String,
}

#[callback_trait::callback_trait]
pub trait ResendCallbackTrait: Send + Sync + 'static {
    async fn on_callback(&self, pkg: Arc<PackageBox>, errno: BuckyErrorCode);
}

pub struct ResendQueue {
    default_interval: Duration,
    max_times: u8,

    packages: Mutex<HashMap<u32, PackageResendInfo>>,

    cb: Option<Pin<Box<dyn ResendCallbackTrait>>>,

}

impl ResendQueue {
    pub fn new(
        default_interval: Duration,
        max_times: u8) -> ResendQueue {
        ResendQueue {
            default_interval,
            max_times,
            packages: Mutex::new(Default::default()),
            cb: None,
        }
    }

    pub fn set_callback(&mut self, cb: impl ResendCallbackTrait) {
        self.cb = Some(Box::pin(cb));
    }

    pub async fn send(
        &self,
        sender: Arc<dyn DataSender>,
        pkg: Package,
        pkg_id: u32,
        pkg_nick_name: String) {
        let now = bucky_time_now();
        let to_send = {
            let mut packages = self.packages.lock().unwrap();
            if let Some(info) = packages.get_mut(&pkg_id) {
                if info.times > 1 {
                    // 短时间内多次send，在前次基础上适当多分配resend机会，避免因为前一次超时导致本次丢失
                    info.times >>= 1;
                    info.interval = info.interval / 2;
                }
                let pkg_box = PackageBox::from_package(sender.local_device_id().clone(), sender.remote_device_id().clone(), sender.key().clone(), pkg);
                info.sender = sender;
                info.pkg = Arc::new(pkg_box);
                info.nick_name = pkg_nick_name.clone();

                if now > info.last_time
                    && Duration::from_micros(now - info.last_time) > info.interval {
                    info.times += 1;
                    info.last_time = now;
                    Some((info.sender.clone(), info.pkg.clone()))
                } else {
                    None
                }
            } else {
                let pkg_box = Arc::new(PackageBox::from_package(sender.local_device_id().clone(), sender.remote_device_id().clone(), sender.key().clone(), pkg));
                packages.insert(pkg_id, PackageResendInfo {
                    pkg: pkg_box.clone(),
                    sender: sender.clone(),
                    interval: self.default_interval,
                    times: 1,
                    last_time: bucky_time_now(),
                    nick_name: pkg_nick_name.clone(),
                });
                Some((sender, pkg_box.clone()))
            }
        };

        if let Some((sender, pkg)) = to_send {
            match sender.send_pkg_box(pkg.as_ref()).await {
                Ok(_) => {
                    info!("{} send ok.", pkg_nick_name);
                },
                Err(e) => {
                    warn!("{} send failed, error: {}.", pkg_nick_name, e.to_string());
                }
            }
        }
    }

    pub async fn confirm_pkg(&self, pkg_id: u32) {
        if self.cb.is_none() {
            return;
        }

        let ret = {
            self.packages.lock().unwrap().remove(&pkg_id)
        };
        if let Some(will_remove) = ret {
            self.cb.as_ref().unwrap().on_callback(will_remove.pkg.clone(), BuckyErrorCode::Ok).await;
        }
    }

    pub async fn try_resend(&self, now: Timestamp) {
        let mut to_send = vec![];
        let mut will_remove = vec![];

        let mut remove_list = Vec::new();
        {
            let mut packages = self.packages.lock().unwrap();
            for (pkg_id, pkg_info) in packages.iter_mut() {
                if now > pkg_info.last_time
                    && Duration::from_micros(now - pkg_info.last_time) > pkg_info.interval {
                    pkg_info.times += 1;
                    pkg_info.interval = pkg_info.interval * 2;
                    pkg_info.last_time = now;
                    if pkg_info.times >= self.max_times {
                        will_remove.push(*pkg_id);
                    }

                    let pkg = pkg_info.pkg.clone();
                    let sender = pkg_info.sender.clone();
                    let nick_name = pkg_info.nick_name.clone();

                    to_send.push((pkg, sender, nick_name));
                }
            }

            for id in will_remove {
                let pkg = packages.remove(&id);
                remove_list.push(pkg);
            }
        }

        for pkg in remove_list {
            if let Some(p) = pkg {
                warn!("{} resend timeout, {} to: {}.", p.nick_name, p.sender.local().addr(), p.sender.remote().addr());
                self.cb.as_ref().unwrap().on_callback(p.pkg.clone(), BuckyErrorCode::Timeout).await;
            }
        }

        for (pkg, sender, nick_name) in to_send {
            match sender.send_pkg_box(pkg.as_ref()).await {
                Ok(_) => {
                    info!("{} send ok, {} to {}.", nick_name, sender.local(), sender.remote());
                },
                Err(e) => {
                    warn!("{} send failed, {} to {}, error: {}.", nick_name, sender.local(), sender.remote(), e.to_string());
                }
            }
        }
    }
}
