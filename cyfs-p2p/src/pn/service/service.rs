// use log;
use std::{
    time::Duration,
};
use std::net::SocketAddr;
use std::sync::{Arc, Weak};
use bucky_crypto::{AesKey, PrivateKey};
use bucky_objects::{Device, NamedObject};
use crate::{
    history::keystore::{self, Keystore},
    protocol::*,
};
use crate::error::BdtResult;
use crate::executor::Executor;
use super::{
    command::*,
    proxy::{self, ProxyTunnelManager, ProxyDeviceStub},
    events::ProxyServiceEvents
};

pub struct Config {
    keystore: keystore::Config,
    tunnel: proxy::Config
}

impl Default for Config {
    fn default() -> Self {
        Self {
            keystore: keystore::Config {
                active_time: Duration::from_secs(300),
                key_expire: Duration::from_secs(120),
                capacity: 10000,
            },
            tunnel: proxy::Config {
                keepalive: Duration::from_secs(5 * 60)
            }
        }
    }
}


struct DefaultEvents {

}

#[async_trait::async_trait]
impl ProxyServiceEvents for DefaultEvents {
    async fn pre_create_tunnel(&self, _mix_key: &AesKey, _device_pair: &(ProxyDeviceStub, ProxyDeviceStub)) -> BdtResult<()> {
        Ok(())
    }
}

struct ServiceImpl {
    keystore: Keystore,
    command_tunnel: Option<CommandTunnel>,
    proxy_tunnels: ProxyTunnelManager,
    events: Box<dyn ProxyServiceEvents>
}

#[derive(Clone)]
pub struct Service(Arc<ServiceImpl>);

#[derive(Clone)]
pub(super) struct WeakService(Weak<ServiceImpl>);

impl From<&WeakService> for Service {
    fn from(w: &WeakService) -> Self {
        Self(w.0.upgrade().unwrap())
    }
}

impl std::fmt::Display for Service {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Service")
    }
}

impl Service {
    pub async fn start(
        local_device: Device,
        local_secret: PrivateKey,
        proxy_ports: Vec<(SocketAddr, Option<SocketAddr>)>,
        config: Option<Config>,
        events: Option<Box<dyn ProxyServiceEvents>>) -> BdtResult<Service> {
        info!("will start pn service device:{}", local_device.desc().device_id());

        let config = config.unwrap_or_default();

        let service = Self(Arc::new(ServiceImpl {
            keystore: Keystore::new(config.keystore.clone()),
            command_tunnel: None,
            proxy_tunnels: ProxyTunnelManager::open(config.tunnel.clone(), proxy_ports.as_slice())?,
            events: events.unwrap_or(Box::new(DefaultEvents {}))
        }));

        service.keystore().add_local_key(local_device.desc().device_id().clone(),
                                         local_secret,
                                         local_device.desc().clone());
        let command_port = local_device.connect_info().endpoints()[0];

        let service_impl = unsafe { &mut *(Arc::as_ptr(&service.0) as *mut ServiceImpl) };
        service_impl.command_tunnel = Some(CommandTunnel::open(service.to_weak(), command_port)?);

        Ok(service)
    }

    pub(super) fn command_tunnel(&self) -> &CommandTunnel {
        &self.0.command_tunnel.as_ref().unwrap()
    }

    pub(super) fn proxy_tunnels(&self) -> &ProxyTunnelManager {
        &self.0.proxy_tunnels
    }

    pub(super) fn keystore(&self) -> &Keystore {
        &self.0.keystore
    }

    fn events(&self) -> &Box<dyn ProxyServiceEvents> {
        &self.0.events
    }


    fn to_weak(&self) -> WeakService {
        WeakService(Arc::downgrade(&self.0))
    }

    pub(crate) fn on_sync_proxy_package(&self, syn_proxy: &SynProxy, context: (&PackageBox, &SocketAddr)) -> BdtResult<()> {
        //TODO: 认证是否授权pn服务
        let (in_box, from) = context;
        trace!("{} got {} from {:?}", self, syn_proxy, from);
        let service = self.clone();
        let syn_proxy = syn_proxy.clone();
        let from = *from;
        let key = in_box.key().clone();
        Executor::spawn(async move {
            let stub_pair = (ProxyDeviceStub {
                id: syn_proxy.from_peer_info.desc().device_id(),
                timestamp: syn_proxy.from_peer_info.body().as_ref().unwrap().update_time(),
            },
                             ProxyDeviceStub {
                                 id: syn_proxy.to_peer_id.clone(),
                                 timestamp: syn_proxy.to_peer_timestamp,
                             }
            );

            let filter_result = service.events().pre_create_tunnel(&syn_proxy.mix_key, &stub_pair).await;
            match filter_result {
                Ok(_) => {
                    let ret = service.proxy_tunnels().create_tunnel(&syn_proxy.mix_key, stub_pair);
                    let _ = service.command_tunnel().ack_proxy(ret, &syn_proxy, &from, &key);
                },
                Err(err) => {
                    let _ = service.command_tunnel().ack_proxy(Err(err), &syn_proxy, &from, &key);
                }
            }
        });

        Ok(())
    }
}
