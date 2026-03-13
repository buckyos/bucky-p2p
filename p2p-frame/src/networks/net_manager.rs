use crate::endpoint::{Endpoint, Protocol};
use crate::error::{P2pError, P2pErrorCode, P2pResult, p2p_err};
use crate::executor::{Executor, SpawnHandle};
use crate::p2p_identity::{P2pId, P2pIdentityRef};
use crate::tls::ServerCertResolverRef;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::{Mutex as AsyncMutex, mpsc};

use super::{TunnelListenerInfo, TunnelListenerRef, TunnelNetworkRef};

pub struct TunnelAcceptor {
    rx: mpsc::UnboundedReceiver<P2pResult<super::TunnelRef>>,
}

impl TunnelAcceptor {
    pub async fn accept_tunnel(&mut self) -> P2pResult<super::TunnelRef> {
        match self.rx.recv().await {
            Some(result) => result,
            None => Err(p2p_err!(
                P2pErrorCode::Interrupted,
                "tunnel acceptor closed"
            )),
        }
    }
}

pub struct NetManager {
    cert_resolver: ServerCertResolverRef,
    tunnel_networks: HashMap<Protocol, TunnelNetworkRef>,
    listener_meta: Mutex<HashMap<Protocol, Vec<TunnelListenerInfo>>>,
    subscriptions: Mutex<HashMap<P2pId, mpsc::UnboundedSender<P2pResult<super::TunnelRef>>>>,
    listener_tasks: Mutex<Vec<SpawnHandle<()>>>,
    is_listening: AtomicBool,
}

pub type NetManagerRef = Arc<NetManager>;

impl NetManager {
    pub fn new(
        tunnel_networks: Vec<TunnelNetworkRef>,
        cert_resolver: ServerCertResolverRef,
    ) -> P2pResult<NetManagerRef> {
        let tunnel_networks = tunnel_networks
            .into_iter()
            .map(|network| (network.protocol(), network))
            .collect::<HashMap<Protocol, TunnelNetworkRef>>();
        Ok(Arc::new(Self {
            cert_resolver,
            tunnel_networks,
            listener_meta: Mutex::new(HashMap::new()),
            subscriptions: Mutex::new(HashMap::new()),
            listener_tasks: Mutex::new(Vec::new()),
            is_listening: AtomicBool::new(false),
        }))
    }

    pub async fn listen(
        self: &Arc<Self>,
        endpoints: &[Endpoint],
        port_mapping: Option<Vec<(Endpoint, u16)>>,
    ) -> P2pResult<()> {
        if self.is_listening.load(Ordering::SeqCst) {
            self.refresh_listener_meta();
            return Ok(());
        }

        let ep_len = endpoints.len();
        if ep_len == 0 {
            return Err(p2p_err!(P2pErrorCode::InvalidParam, "no endpoint"));
        }

        let mut port_mapping = port_mapping.unwrap_or_default();
        let mut ep_index = 0;
        let mut listeners = Vec::new();

        while ep_index < ep_len {
            let ep = &endpoints[ep_index];
            let ep_pair = if ep.is_mapped_wan() {
                let local_index = ep_index + 1;
                let ep_pair = if local_index == ep_len {
                    Err(P2pError::new(
                        P2pErrorCode::InvalidParam,
                        format!("mapped wan endpoint {} has no local endpoint", ep),
                    ))
                } else {
                    let local_ep = &endpoints[local_index];
                    if !(local_ep.is_same_ip_version(ep)
                        && local_ep.protocol() == ep.protocol()
                        && !local_ep.is_static_wan())
                    {
                        Err(P2pError::new(
                            P2pErrorCode::InvalidParam,
                            format!(
                                "mapped wan endpoint {} has invalid local endpoint {}",
                                ep, local_ep
                            ),
                        ))
                    } else {
                        Ok((*local_ep, Some(*ep)))
                    }
                };
                ep_index = local_index;
                ep_pair
            } else {
                Ok((*ep, None))
            };
            ep_index += 1;

            let (local, out) = ep_pair?;
            let mapping_port = take_mapping_port(&mut port_mapping, ep);
            let network = self
                .get_network(local.protocol())
                .map_err(|_| p2p_err!(P2pErrorCode::NotFound, "network not found: {}", local))?;
            let listener = network.listen(&local, out, mapping_port).await?;
            listeners.push(listener);
        }

        self.refresh_listener_meta();
        let mut tasks = Vec::new();
        for listener in listeners {
            tasks.push(self.spawn_listener_loop(listener));
        }
        self.listener_tasks.lock().unwrap().extend(tasks);
        self.is_listening.store(true, Ordering::SeqCst);
        Ok(())
    }

    pub fn get_network(&self, protocol: Protocol) -> P2pResult<TunnelNetworkRef> {
        self.tunnel_networks.get(&protocol).cloned().ok_or_else(|| {
            p2p_err!(
                P2pErrorCode::NotFound,
                "no network for protocol {:?}",
                protocol
            )
        })
    }

    pub fn get_listener(&self, protocol: Protocol) -> Vec<TunnelListenerRef> {
        self.tunnel_networks
            .get(&protocol)
            .map(|network| network.listeners())
            .unwrap_or_default()
    }

    pub fn listener_entries(&self) -> Vec<(Protocol, Vec<TunnelListenerRef>)> {
        self.tunnel_networks
            .iter()
            .map(|(protocol, network)| (*protocol, network.listeners()))
            .collect()
    }

    pub fn get_listener_info(&self, protocol: Protocol) -> Vec<TunnelListenerInfo> {
        self.listener_meta
            .lock()
            .unwrap()
            .get(&protocol)
            .cloned()
            .unwrap_or_default()
    }

    pub fn listener_info_entries(&self) -> Vec<(Protocol, Vec<TunnelListenerInfo>)> {
        self.listener_meta
            .lock()
            .unwrap()
            .iter()
            .map(|(protocol, listeners)| (*protocol, listeners.clone()))
            .collect()
    }

    pub fn protocols(&self) -> Vec<Protocol> {
        self.tunnel_networks.keys().copied().collect()
    }

    pub async fn add_listen_device(&self, device: P2pIdentityRef) -> P2pResult<()> {
        self.cert_resolver.add_server_identity(device).await
    }

    pub async fn remove_listen_device(&self, device_id: &str) -> P2pResult<()> {
        self.cert_resolver.remove_server_identity(device_id).await
    }

    pub async fn get_listen_device(&self, device_id: &str) -> Option<P2pIdentityRef> {
        self.cert_resolver.get_server_identity(device_id).await
    }

    pub fn register_tunnel_acceptor(&self, local_id: P2pId) -> P2pResult<TunnelAcceptor> {
        let (tx, rx) = mpsc::unbounded_channel();
        let mut subscriptions = self.subscriptions.lock().unwrap();
        if subscriptions.contains_key(&local_id) {
            return Err(p2p_err!(
                P2pErrorCode::AlreadyExists,
                "tunnel acceptor already exists for {}",
                local_id
            ));
        }
        subscriptions.insert(local_id, tx);
        Ok(TunnelAcceptor { rx })
    }

    pub fn unregister_tunnel_acceptor(&self, local_id: &P2pId) {
        self.subscriptions.lock().unwrap().remove(local_id);
    }

    fn refresh_listener_meta(&self) {
        let mut listener_meta = self.listener_meta.lock().unwrap();
        listener_meta.clear();
        for (protocol, network) in &self.tunnel_networks {
            listener_meta.insert(*protocol, network.listener_infos());
        }
    }

    fn spawn_listener_loop(self: &Arc<Self>, listener: TunnelListenerRef) -> SpawnHandle<()> {
        let manager = self.clone();
        Executor::spawn_with_handle(async move {
            loop {
                match listener.accept_tunnel().await {
                    Ok(tunnel) => manager.dispatch_tunnel(tunnel),
                    Err(err) => {
                        log::warn!("accept tunnel failed: {:?}", err);
                        break;
                    }
                }
            }
        })
        .unwrap()
    }

    fn dispatch_tunnel(&self, tunnel: super::TunnelRef) {
        let local_id = tunnel.local_id();
        let mut subscriptions = self.subscriptions.lock().unwrap();
        if let Some(subscriber) = subscriptions.get(&local_id) {
            if subscriber.send(Ok(tunnel)).is_err() {
                subscriptions.remove(&local_id);
            }
        }
    }
}

impl Drop for NetManager {
    fn drop(&mut self) {
        for task in self.listener_tasks.lock().unwrap().drain(..) {
            task.abort();
        }
    }
}

fn take_mapping_port(port_mapping: &mut Vec<(Endpoint, u16)>, src: &Endpoint) -> Option<u16> {
    let index = port_mapping.iter().position(|(ep, _)| ep == src)?;
    Some(port_mapping.remove(index).1)
}
