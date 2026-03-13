use crate::error::{P2pErrorCode, P2pResult};
use crate::networks::{Tunnel, TunnelRef, TunnelStreamRead, TunnelStreamWrite};
use crate::runtime;
use std::sync::{Arc, Mutex, Weak};

use super::listener::{
    QueueTtpDatagramListener, QueueTtpListener, TtpDatagramListenerRef, TtpListenerRef,
};
use super::registry::TtpQueueRegistry;
use super::{TtpDatagram, TtpDatagramMeta, TtpStreamMeta};

pub(crate) struct TtpRuntime {
    stream_registry: Arc<TtpQueueRegistry<(TtpStreamMeta, TunnelStreamRead, TunnelStreamWrite)>>,
    datagram_registry: Arc<TtpQueueRegistry<TtpDatagram>>,
    attached_tunnels: Mutex<Vec<Weak<dyn Tunnel>>>,
}

impl TtpRuntime {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            stream_registry: TtpQueueRegistry::new(),
            datagram_registry: TtpQueueRegistry::new(),
            attached_tunnels: Mutex::new(Vec::new()),
        })
    }

    pub(crate) fn listen_stream(&self, vport: u16) -> P2pResult<TtpListenerRef> {
        let rx = self.stream_registry.register(vport)?;
        Ok(QueueTtpListener::new(rx))
    }

    pub(crate) fn unlisten_stream(&self, vport: u16) {
        self.stream_registry.remove(vport);
    }

    pub(crate) fn listen_datagram(&self, vport: u16) -> P2pResult<TtpDatagramListenerRef> {
        let rx = self.datagram_registry.register(vport)?;
        Ok(QueueTtpDatagramListener::new(rx))
    }

    pub(crate) fn unlisten_datagram(&self, vport: u16) {
        self.datagram_registry.remove(vport);
    }

    pub(crate) async fn attach_tunnel(self: &Arc<Self>, tunnel: TunnelRef) -> P2pResult<()> {
        if !self.try_mark_attached(&tunnel) {
            return Ok(());
        }

        match tunnel
            .listen_stream(self.stream_registry.as_listen_vports_ref())
            .await
        {
            Ok(()) => {}
            Err(err) if err.code() == P2pErrorCode::NotSupport => {}
            Err(err) => return Err(err),
        }

        match tunnel
            .listen_datagram(self.datagram_registry.as_listen_vports_ref())
            .await
        {
            Ok(()) => {}
            Err(err) if err.code() == P2pErrorCode::NotSupport => {}
            Err(err) => return Err(err),
        }

        self.spawn_stream_loop(tunnel.clone());
        self.spawn_datagram_loop(tunnel);
        Ok(())
    }

    fn try_mark_attached(&self, tunnel: &TunnelRef) -> bool {
        let mut attached = self.attached_tunnels.lock().unwrap();
        attached.retain(|weak| weak.strong_count() > 0);

        for existing in attached.iter() {
            if let Some(existing) = existing.upgrade() {
                if Arc::ptr_eq(&existing, tunnel) {
                    return false;
                }
            }
        }

        attached.push(Arc::downgrade(tunnel));
        true
    }

    fn spawn_stream_loop(self: &Arc<Self>, tunnel: TunnelRef) {
        let weak = Arc::downgrade(self);
        runtime::task::spawn(async move {
            loop {
                match tunnel.accept_stream().await {
                    Ok((vport, read, write)) => {
                        let Some(runtime) = weak.upgrade() else {
                            break;
                        };
                        runtime.stream_registry.deliver(
                            vport,
                            Ok((
                                TtpStreamMeta {
                                    local_ep: tunnel.local_ep(),
                                    remote_ep: tunnel.remote_ep(),
                                    local_id: tunnel.local_id(),
                                    remote_id: tunnel.remote_id(),
                                    remote_name: None,
                                    vport,
                                },
                                read,
                                write,
                            )),
                        );
                    }
                    Err(err) => {
                        if err.code() != P2pErrorCode::NotSupport {
                            log::debug!("ttp accept stream finished: {:?}", err);
                        }
                        break;
                    }
                }
            }
        });
    }

    fn spawn_datagram_loop(self: &Arc<Self>, tunnel: TunnelRef) {
        let weak = Arc::downgrade(self);
        runtime::task::spawn(async move {
            loop {
                match tunnel.accept_datagram().await {
                    Ok((vport, read)) => {
                        let Some(runtime) = weak.upgrade() else {
                            break;
                        };
                        runtime.datagram_registry.deliver(
                            vport,
                            Ok(TtpDatagram {
                                meta: TtpDatagramMeta {
                                    local_ep: tunnel.local_ep(),
                                    remote_ep: tunnel.remote_ep(),
                                    local_id: tunnel.local_id(),
                                    remote_id: tunnel.remote_id(),
                                    remote_name: None,
                                    vport,
                                },
                                read,
                            }),
                        );
                    }
                    Err(err) => {
                        if err.code() != P2pErrorCode::NotSupport {
                            log::debug!("ttp accept datagram finished: {:?}", err);
                        }
                        break;
                    }
                }
            }
        });
    }
}
