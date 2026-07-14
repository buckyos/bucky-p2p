use crate::error::{P2pError, P2pErrorCode, P2pResult};
use crate::networks::{Tunnel, TunnelPurpose, TunnelRef, TunnelStreamRead, TunnelStreamWrite};
use std::sync::{Arc, Mutex, Weak};

use super::listener::{
    TtpIncomingControlStreamCallback, TtpIncomingDatagramCallback, TtpIncomingStream,
    TtpIncomingStreamCallback,
};
use super::registry::TtpQueueRegistry;
use super::{TtpDatagram, TtpDatagramMeta, TtpStreamMeta};

pub(crate) struct TtpRuntime {
    stream_registry: Arc<TtpQueueRegistry<TtpIncomingStream>>,
    control_stream_registry: Arc<TtpQueueRegistry<TtpIncomingStream>>,
    datagram_registry: Arc<TtpQueueRegistry<TtpDatagram>>,
    attached_tunnels: Mutex<Vec<Weak<dyn Tunnel>>>,
}

impl TtpRuntime {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            stream_registry: TtpQueueRegistry::new(),
            control_stream_registry: TtpQueueRegistry::new(),
            datagram_registry: TtpQueueRegistry::new(),
            attached_tunnels: Mutex::new(Vec::new()),
        })
    }

    pub(crate) fn listen_stream(
        &self,
        purpose: TunnelPurpose,
        callback: TtpIncomingStreamCallback,
    ) -> P2pResult<()> {
        log::debug!("ttp listen stream register purpose={}", purpose);
        self.stream_registry.register(purpose, callback)
    }

    pub(crate) fn unlisten_stream(&self, purpose: &TunnelPurpose) {
        log::debug!("ttp unlisten stream purpose={}", purpose);
        self.stream_registry.remove(purpose);
    }

    pub(crate) fn listen_control_stream(
        &self,
        purpose: TunnelPurpose,
        callback: TtpIncomingControlStreamCallback,
    ) -> P2pResult<()> {
        log::debug!("ttp listen control stream register purpose={}", purpose);
        self.control_stream_registry.register(purpose, callback)
    }

    pub(crate) fn unlisten_control_stream(&self, purpose: &TunnelPurpose) {
        log::debug!("ttp unlisten control stream purpose={}", purpose);
        self.control_stream_registry.remove(purpose);
    }

    pub(crate) fn listen_datagram(
        &self,
        purpose: TunnelPurpose,
        callback: TtpIncomingDatagramCallback,
    ) -> P2pResult<()> {
        log::debug!("ttp listen datagram register purpose={}", purpose);
        self.datagram_registry.register(purpose, callback)
    }

    pub(crate) fn unlisten_datagram(&self, purpose: &TunnelPurpose) {
        log::debug!("ttp unlisten datagram purpose={}", purpose);
        self.datagram_registry.remove(purpose);
    }

    pub(crate) async fn attach_tunnel(self: &Arc<Self>, tunnel: TunnelRef) -> P2pResult<()> {
        log::debug!(
            "ttp attach tunnel start local={} remote={} protocol={:?} tunnel_id={:?} candidate_id={:?}",
            tunnel.local_id(),
            tunnel.remote_id(),
            tunnel.protocol(),
            tunnel.tunnel_id(),
            tunnel.candidate_id()
        );
        if !self.try_mark_attached(&tunnel) {
            log::debug!(
                "ttp attach tunnel skip duplicated local={} remote={} protocol={:?} tunnel_id={:?} candidate_id={:?}",
                tunnel.local_id(),
                tunnel.remote_id(),
                tunnel.protocol(),
                tunnel.tunnel_id(),
                tunnel.candidate_id()
            );
            return Ok(());
        }

        let stream_tunnel = tunnel.clone();
        let stream_weak = Arc::downgrade(self);
        let stream_callback: crate::networks::IncomingStreamCallback = Arc::new(move |accepted| {
            let tunnel = stream_tunnel.clone();
            let weak = stream_weak.clone();
            Box::pin(async move {
                match accepted {
                    Ok((purpose, read, write)) => {
                        let Some(runtime) = weak.upgrade() else {
                            return;
                        };
                        if let Err(err) = runtime.stream_registry.deliver(
                            &purpose,
                            Ok((
                                TtpStreamMeta {
                                    local_ep: tunnel.local_ep(),
                                    remote_ep: tunnel.remote_ep(),
                                    local_id: tunnel.local_id(),
                                    remote_id: tunnel.remote_id(),
                                    remote_name: None,
                                    purpose: purpose.clone(),
                                },
                                read,
                                write,
                            )),
                        ) {
                            log::warn!("ttp stream deliver failed purpose={}: {:?}", purpose, err);
                        }
                    }
                    Err(err) => {
                        if err.code() != P2pErrorCode::NotSupport {
                            log::debug!("ttp stream callback finished: {:?}", err);
                        }
                    }
                }
            }) as crate::networks::IncomingStreamCallbackFuture
        });
        match tunnel
            .listen_stream(self.stream_registry.as_listen_vports_ref(), stream_callback)
            .await
        {
            Ok(()) => {
                log::debug!(
                    "ttp attach tunnel listen_stream ready local={} remote={} protocol={:?} tunnel_id={:?} candidate_id={:?}",
                    tunnel.local_id(),
                    tunnel.remote_id(),
                    tunnel.protocol(),
                    tunnel.tunnel_id(),
                    tunnel.candidate_id()
                );
            }
            Err(err) if err.code() == P2pErrorCode::NotSupport => {
                log::debug!(
                    "ttp attach tunnel listen_stream not supported local={} remote={} protocol={:?} tunnel_id={:?} candidate_id={:?}",
                    tunnel.local_id(),
                    tunnel.remote_id(),
                    tunnel.protocol(),
                    tunnel.tunnel_id(),
                    tunnel.candidate_id()
                );
            }
            Err(err) => return Err(Self::close_failed_attach(&tunnel, err)),
        }

        let control_stream_tunnel = tunnel.clone();
        let control_stream_weak = Arc::downgrade(self);
        let control_stream_callback: crate::networks::IncomingControlStreamCallback =
            Arc::new(move |accepted| {
                let tunnel = control_stream_tunnel.clone();
                let weak = control_stream_weak.clone();
                Box::pin(async move {
                    match accepted {
                        Ok((purpose, read, write)) => {
                            let Some(runtime) = weak.upgrade() else {
                                return;
                            };
                            if let Err(err) = runtime.control_stream_registry.deliver(
                                &purpose,
                                Ok((
                                    TtpStreamMeta {
                                        local_ep: tunnel.local_ep(),
                                        remote_ep: tunnel.remote_ep(),
                                        local_id: tunnel.local_id(),
                                        remote_id: tunnel.remote_id(),
                                        remote_name: None,
                                        purpose: purpose.clone(),
                                    },
                                    read,
                                    write,
                                )),
                            ) {
                                log::warn!(
                                    "ttp control stream deliver failed purpose={}: {:?}",
                                    purpose,
                                    err
                                );
                            }
                        }
                        Err(err) => {
                            if err.code() != P2pErrorCode::NotSupport {
                                log::debug!("ttp control stream callback finished: {:?}", err);
                            }
                        }
                    }
                }) as crate::networks::IncomingControlStreamCallbackFuture
            });
        match tunnel
            .listen_control_stream(
                self.control_stream_registry.as_listen_vports_ref(),
                control_stream_callback,
            )
            .await
        {
            Ok(()) => {
                log::debug!(
                    "ttp attach tunnel listen_control_stream ready local={} remote={} protocol={:?} tunnel_id={:?} candidate_id={:?}",
                    tunnel.local_id(),
                    tunnel.remote_id(),
                    tunnel.protocol(),
                    tunnel.tunnel_id(),
                    tunnel.candidate_id()
                );
            }
            Err(err) if err.code() == P2pErrorCode::NotSupport => {
                log::debug!(
                    "ttp attach tunnel listen_control_stream not supported local={} remote={} protocol={:?} tunnel_id={:?} candidate_id={:?}",
                    tunnel.local_id(),
                    tunnel.remote_id(),
                    tunnel.protocol(),
                    tunnel.tunnel_id(),
                    tunnel.candidate_id()
                );
            }
            Err(err) => return Err(Self::close_failed_attach(&tunnel, err)),
        }

        let datagram_tunnel = tunnel.clone();
        let datagram_weak = Arc::downgrade(self);
        let datagram_callback: crate::networks::IncomingDatagramCallback =
            Arc::new(move |accepted| {
                let tunnel = datagram_tunnel.clone();
                let weak = datagram_weak.clone();
                Box::pin(async move {
                    match accepted {
                        Ok((purpose, read)) => {
                            let Some(runtime) = weak.upgrade() else {
                                return;
                            };
                            if let Err(err) = runtime.datagram_registry.deliver(
                                &purpose,
                                Ok(TtpDatagram {
                                    meta: TtpDatagramMeta {
                                        local_ep: tunnel.local_ep(),
                                        remote_ep: tunnel.remote_ep(),
                                        local_id: tunnel.local_id(),
                                        remote_id: tunnel.remote_id(),
                                        remote_name: None,
                                        purpose: purpose.clone(),
                                    },
                                    read,
                                }),
                            ) {
                                log::warn!(
                                    "ttp datagram deliver failed purpose={}: {:?}",
                                    purpose,
                                    err
                                );
                            }
                        }
                        Err(err) => {
                            if err.code() != P2pErrorCode::NotSupport {
                                log::debug!("ttp datagram callback finished: {:?}", err);
                            }
                        }
                    }
                }) as crate::networks::IncomingDatagramCallbackFuture
            });
        match tunnel
            .listen_datagram(
                self.datagram_registry.as_listen_vports_ref(),
                datagram_callback,
            )
            .await
        {
            Ok(()) => {
                log::debug!(
                    "ttp attach tunnel listen_datagram ready local={} remote={} protocol={:?} tunnel_id={:?} candidate_id={:?}",
                    tunnel.local_id(),
                    tunnel.remote_id(),
                    tunnel.protocol(),
                    tunnel.tunnel_id(),
                    tunnel.candidate_id()
                );
            }
            Err(err) if err.code() == P2pErrorCode::NotSupport => {
                log::debug!(
                    "ttp attach tunnel listen_datagram not supported local={} remote={} protocol={:?} tunnel_id={:?} candidate_id={:?}",
                    tunnel.local_id(),
                    tunnel.remote_id(),
                    tunnel.protocol(),
                    tunnel.tunnel_id(),
                    tunnel.candidate_id()
                );
            }
            Err(err) => return Err(Self::close_failed_attach(&tunnel, err)),
        }
        log::debug!(
            "ttp attach tunnel completed local={} remote={} protocol={:?} tunnel_id={:?} candidate_id={:?}",
            tunnel.local_id(),
            tunnel.remote_id(),
            tunnel.protocol(),
            tunnel.tunnel_id(),
            tunnel.candidate_id()
        );
        Ok(())
    }

    fn close_failed_attach(tunnel: &TunnelRef, attach_err: P2pError) -> P2pError {
        if let Err(close_err) = tunnel.close() {
            log::warn!(
                "ttp close attach-failed tunnel failed local={} remote={} protocol={:?} tunnel_id={:?} candidate_id={:?} attach_err={:?} close_err={:?}",
                tunnel.local_id(),
                tunnel.remote_id(),
                tunnel.protocol(),
                tunnel.tunnel_id(),
                tunnel.candidate_id(),
                attach_err,
                close_err
            );
        }
        attach_err
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
}
