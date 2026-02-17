use crate::endpoint::{Endpoint, Protocol};
use crate::error::{P2pError, P2pErrorCode, P2pResult, p2p_err};
use crate::finder::DeviceCache;
use crate::p2p_connection::{
    P2pConnection, P2pConnectionEventListener, P2pConnectionInfoCacheRef, P2pListenerRef,
};
use crate::p2p_identity::{P2pId, P2pIdentityCertFactoryRef, P2pIdentityRef};
use crate::p2p_network::P2pNetworkRef;
use crate::sockets::tcp::{TCPListener, TCPListenerRef};
use crate::sockets::{QuicListener, QuicListenerRef};
use crate::tls::ServerCertResolverRef;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub struct NetManager {
    connection_info_cache: P2pConnectionInfoCacheRef,
    cert_resolver: ServerCertResolverRef,
    p2p_networks: HashMap<Protocol, P2pNetworkRef>,
    port_mapping: Mutex<Vec<(Endpoint, u16)>>,
    listener: Mutex<HashMap<P2pId, Arc<dyn P2pConnectionEventListener>>>,
}
pub type NetManagerRef = Arc<NetManager>;

impl NetManager {
    pub fn new(
        p2p_networks: Vec<P2pNetworkRef>,
        cert_resolver: ServerCertResolverRef,
        connection_info_cache: P2pConnectionInfoCacheRef,
    ) -> P2pResult<Self> {
        let mut p2p_networks = p2p_networks
            .into_iter()
            .map(|network| (network.protocol(), network))
            .collect::<HashMap<Protocol, P2pNetworkRef>>();
        Ok(Self {
            connection_info_cache,
            cert_resolver,
            p2p_networks,
            port_mapping: Mutex::new(vec![]),
            listener: Mutex::new(HashMap::new()),
        })
    }

    pub async fn listen(
        self: &Arc<Self>,
        endpoints: &[Endpoint],
        port_mapping: Option<Vec<(Endpoint, u16)>>,
    ) -> P2pResult<()> {
        let ep_len = endpoints.len();
        if ep_len == 0 {
            let err = p2p_err!(P2pErrorCode::InvalidParam, "no endpoint");
            warn!("NetListener bind failed for {}", err);
            return Err(err);
        }

        let mut port_mapping = port_mapping.unwrap_or(vec![]);
        *self.port_mapping.lock().unwrap() = port_mapping.clone();

        let mut ep_index = 0;

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

            if ep_pair.is_err() {
                let err = ep_pair.unwrap_err();
                warn!("NetListener bind on {:?} failed for {:?}", ep, err);
                continue;
            }

            let p2p_network = match self.p2p_networks.get(&ep.protocol()) {
                Some(network) => network,
                None => {
                    let err = p2p_err!(
                        P2pErrorCode::InvalidParam,
                        "no network for protocol {:?}",
                        ep.protocol()
                    );
                    warn!("NetListener bind on {:?} failed for {:?}", ep, err);
                    continue;
                }
            };

            let (local, out) = ep_pair.unwrap();

            let mapping_port = {
                let mut found_index = None;
                for (index, (src_ep, _)) in port_mapping.iter().enumerate() {
                    if *src_ep == *ep {
                        found_index = Some(index);
                        break;
                    }
                }
                found_index.map(|index| {
                    let (_, dst_port) = port_mapping.remove(index);
                    dst_port
                })
            };

            let this = self.clone();
            let p2p_listener = match p2p_network
                .listen(
                    &local,
                    out,
                    mapping_port,
                    Arc::new(move |conn: P2pConnection| {
                        let this = this.clone();
                        async move {
                            let listener = {
                                let event_listeners = this.listener.lock().unwrap();
                                if let Some(listener) = event_listeners.get(&conn.local_id()) {
                                    listener.clone()
                                } else {
                                    return Ok(());
                                }
                            };
                            listener.on_new_connection(conn).await?;
                            Ok(())
                        }
                    }),
                )
                .await
            {
                Ok(listener) => listener,
                Err(e) => {
                    log::error!("NetListener bind on {:?} failed for {:?}", ep, e);
                    continue;
                }
            };
        }

        Ok(())
    }

    pub fn set_connection_event_listener(
        &self,
        local_id: P2pId,
        listener: impl P2pConnectionEventListener,
    ) {
        let event_listener = Arc::new(listener);
        self.listener
            .lock()
            .unwrap()
            .insert(local_id, event_listener);
    }

    pub fn remove_connection_event_listener(&self, local_id: &P2pId) {
        self.listener.lock().unwrap().remove(local_id);
    }

    pub fn get_connection_info_cache(&self) -> &P2pConnectionInfoCacheRef {
        &self.connection_info_cache
    }

    // pub fn get_udp_socket(&self, ep: &Endpoint) -> Option<Arc<UDPSocket>> {
    //     self.net_listener.udp_of(ep).map(|udp| udp.socket().as_ref().unwrap().clone())
    // }

    pub async fn add_listen_device(&self, device: P2pIdentityRef) -> P2pResult<()> {
        log::info!("add_listen_device {:?}", device.get_id());
        self.cert_resolver.add_server_identity(device).await
    }

    pub async fn remove_listen_device(&self, device_id: &str) -> P2pResult<()> {
        log::info!("remove_listen_device {:?}", device_id);
        self.cert_resolver.remove_server_identity(device_id).await
    }

    pub async fn get_listen_device(&self, device_id: &str) -> Option<P2pIdentityRef> {
        self.cert_resolver.get_server_identity(device_id).await
    }

    pub fn get_listener(&self, protocol: Protocol) -> Vec<P2pListenerRef> {
        self.p2p_networks
            .get(&protocol)
            .map(|network| network.listeners())
            .unwrap_or(vec![])
    }

    pub fn get_network(&self, protocol: Protocol) -> P2pResult<P2pNetworkRef> {
        if let Some(network) = self.p2p_networks.get(&protocol) {
            Ok(network.clone())
        } else {
            Err(p2p_err!(
                P2pErrorCode::NotFound,
                "no network for protocol {:?}",
                protocol
            ))
        }
    }

    pub fn get_networks(&self) -> Vec<P2pNetworkRef> {
        self.p2p_networks.values().cloned().collect()
    }

    pub fn port_mapping(&self) -> Vec<(Endpoint, u16)> {
        self.port_mapping.lock().unwrap().clone()
    }
}
