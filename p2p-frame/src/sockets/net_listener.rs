use std::collections::{BTreeSet};
use std::sync::{Arc};
use std::time::Duration;
use crate::endpoint::{Endpoint, Protocol};
use crate::error::{bdt_err, P2pError, P2pErrorCode, P2pResult};
use crate::executor::Executor;
use crate::finder::DeviceCache;
use crate::p2p_identity::{P2pIdentityCertFactoryRef};
use crate::tls::ServerCertResolverRef;
use super::tcp::{TCPListener, TcpListenerEventListener, TCPListenerRef};
use super::{QuicListener, QuicListenerEventListener, QuicListenerRef, UpdateOuterResult};

pub struct NetListener {
    udp: Vec<QuicListenerRef>,
    tcp: Vec<TCPListenerRef>,
}
pub type NetListenerRef = Arc<NetListener>;

impl NetListener {
    pub async fn open(
        device_cache: Arc<DeviceCache>,
        cert_resolver: ServerCertResolverRef,
        cert_factory: P2pIdentityCertFactoryRef,
        endpoints: &[Endpoint],
        port_mapping: Option<Vec<(Endpoint, u16)>>,
        tcp_accept_timout: Duration,
    ) -> P2pResult<Arc<Self>> {
        let ep_len = endpoints.len();
        if ep_len == 0 {
            let err = bdt_err!(P2pErrorCode::InvalidParam, "no endpoint");
            warn!("NetListener bind failed for {}", err);
            return Err(err);
        }

        let mut listener = NetListener {
            udp: vec![],
            tcp: vec![],
            // ip_set: BTreeSet::new(),
            // ep_set: BTreeSet::new(),
        };
        let mut port_mapping = port_mapping.unwrap_or(vec![]);

        let mut ep_index = 0;

        while ep_index < ep_len {
            let ep = &endpoints[ep_index];
            let ep_pair = if ep.is_mapped_wan() {
                let local_index = ep_index + 1;
                let ep_pair = if local_index == ep_len {
                    Err(P2pError::new(P2pErrorCode::InvalidParam, format!("mapped wan endpoint {} has no local endpoint", ep)))
                } else {
                    let local_ep = &endpoints[local_index];
                    if !(local_ep.is_same_ip_version(ep)
                        && local_ep.protocol() == ep.protocol()
                        && !local_ep.is_static_wan()) {
                        Err(P2pError::new(P2pErrorCode::InvalidParam, format!("mapped wan endpoint {} has invalid local endpoint {}", ep, local_ep)))
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

            let (local, out) = ep_pair.unwrap();

            let r = match ep.protocol() {
                Protocol::Bdt => {
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
                    let udp_listener = QuicListener::new(
                        device_cache.clone(),
                        cert_resolver.clone(),
                        cert_factory.clone(),
                        tcp_accept_timout);
                    let ret= udp_listener.bind(local.clone(), out, mapping_port).await;
                    listener.udp.push(udp_listener);
                    ret
                },
                Protocol::Tcp => {
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
                    let tcp_listener = TCPListener::new(device_cache.clone(), cert_resolver.clone(), cert_factory.clone(), tcp_accept_timout);
                    let ret = tcp_listener.bind(local, out, mapping_port).await;
                    listener.tcp.push(tcp_listener);
                    ret
                },
                Protocol::Unk(_) => {
                    panic!()
                },
                Protocol::Quic => {
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
                    let udp_listener = QuicListener::new(
                        device_cache.clone(),
                        cert_resolver.clone(),
                        cert_factory.clone(),
                        tcp_accept_timout);
                    let ret= udp_listener.bind(local.clone(), out, mapping_port).await;
                    listener.udp.push(udp_listener);
                    ret
                },
                Protocol::Kcp => {
                    panic!()
                }
            };

            if let Err(e) = r.as_ref() {
                warn!("NetListener{{local:{}}} bind on {:?} failed for {:?}", local, ep, e);
            } else {
                info!("NetListener{{local:{}}} bind on {:?} success", local, ep);
                // listener.ep_set.insert(*ep);
                // if listener.ip_set.insert(ep.addr().ip()) {
                //     info!("NetListener{{local:{}}} add local ip {:?}", local, ep.addr().ip());
                // }
            }
        }
        Ok(Arc::new(listener))
    }

    pub fn set_udp_listener_event_listener(&self, listener: Arc<dyn QuicListenerEventListener>) {
        for udp in self.udp.iter() {
            udp.set_listener(listener.clone());
        }
    }

    pub fn set_tcp_listener_event_listener(&self, listener: Arc<dyn TcpListenerEventListener>) {
        for tcp in self.tcp.iter() {
            tcp.set_listener(listener.clone());
        }
    }


    pub async fn reset(self: &Arc<Self>, endpoints: Option<&[Endpoint]>) -> Arc<Self> {
        if let Some(endpoints) = endpoints {
            let mut all_default = true;
            for ep in endpoints {
                if !ep.is_sys_default() {
                    all_default = false;
                    break;
                }
            }
            //TODO: 支持显式绑定本地ip的 reset
            if !all_default {
                error!("reset should be endpoint with default flag");
                return self.clone();
            }
        }


        fn local_of(former: Endpoint, endpoints: &Option<&[Endpoint]>) -> Option<Endpoint> {
            if let Some(endpoints) = endpoints {
                for ep in *endpoints {
                    if former.is_same_ip_version(ep)
                        && former.protocol() == ep.protocol()
                        && former.addr().port() == ep.addr().port() {
                        return Some(*ep);
                    }
                }
            }
            None
        }

        // let mut ip_set = BTreeSet::new();
        // let mut ep_set = BTreeSet::new();
        let _udp = Vec::from_iter(self.udp.iter().map(|udp| {
            if let Some(new_ep) = local_of(udp.local(), &endpoints) {
                // ep_set.insert(new_ep);
                // ip_set.insert(new_ep.addr().ip());
                Executor::block_on(udp.reset(&new_ep))
            } else {
                // ep_set.insert(new_ep);
                // ip_set.insert(new_ep.addr().ip());
                udp.clone()
            }
        }));

        let _tcp = Vec::from_iter(self.tcp.iter().map(|tcp| {
            if let Some(new_ep) = local_of(tcp.local(), &endpoints) {
                Executor::block_on(tcp.reset(&new_ep))
            } else {
                tcp.clone()
            }
            // ep_set.insert(new_ep);
            // ip_set.insert(new_ep.addr().ip());

        }));

        self.clone()
    }

    pub fn start(&self) {
        for i in self.udp() {
            i.start();
        }
        for l in &self.tcp {
            l.start();
        }
    }

    pub fn update_outer(&self, ep: &Endpoint, outer: &Endpoint) -> UpdateOuterResult {
        let outer = *outer;
        let mut reseult = UpdateOuterResult::None;
        if let Some(interface) = self.udp_of(ep) {
            let udp_result = interface.update_outer(&outer);
            if udp_result > reseult {
                reseult = udp_result;
            }
            if udp_result > UpdateOuterResult::None {
                if ep.addr().is_ipv6() {
                    for listener in self.tcp() {
                        if listener.local().addr().is_ipv6() {
                            let mut tcp_outer = outer;
                            tcp_outer.set_protocol(Protocol::Tcp);
                            tcp_outer.mut_addr().set_port(listener.local().addr().port());
                            listener.update_outer(&tcp_outer);
                        }
                    }
                } else {
                    for listener in self.tcp() {
                        if let Some(mapping_port) = listener.mapping_port() {
                            if listener.local().is_same_ip_addr(ep) {
                                let mut tcp_outer = outer;
                                tcp_outer.set_protocol(Protocol::Tcp);
                                tcp_outer.mut_addr().set_port(mapping_port);
                                listener.update_outer(&tcp_outer);
                            }
                        }
                    }
                }
            }
        }
        reseult
    }

    pub fn endpoints(&self) -> BTreeSet<Endpoint> {
        let mut ep_set = BTreeSet::new();
        for udp in self.udp() {
            if udp.local().addr().is_ipv4() {
                ep_set.insert(udp.local());
            }
            let outer = udp.outer();
            if outer.is_some() {
                ep_set.insert(outer.unwrap());
            }
        }
        for tcp in self.tcp() {
            if tcp.local().addr().is_ipv4() {
                ep_set.insert(tcp.local());
            }
            let outer = tcp.outer();
            if outer.is_some() {
                ep_set.insert(outer.unwrap());
            }
        }
        ep_set
    }


    pub fn udp_of(&self, ep: &Endpoint) -> Option<&QuicListenerRef> {
        for i in &self.udp {
            if i.local() == *ep {
                return Some(i);
            }
        }
        None
    }

    pub fn udp(&self) -> &Vec<QuicListenerRef> {
        &self.udp
    }

    pub fn tcp_of(&self, ep: &Endpoint) -> Option<&TCPListenerRef> {
        for i in &self.tcp {
            if i.local() == *ep {
                return Some(i);
            }
        }
        None
    }

    pub fn tcp(&self) -> &Vec<TCPListenerRef> {
        &self.tcp
    }
}
