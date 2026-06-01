use crate::error::{P2pErrorCode, P2pResult, p2p_err};
use crate::networks::{ListenVPorts, ListenVPortsRef, TunnelPurpose};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tokio::sync::mpsc;

pub(crate) struct TtpQueueRegistry<T> {
    listeners: RwLock<HashMap<TunnelPurpose, Arc<mpsc::Sender<P2pResult<T>>>>>,
    channel_capacity: usize,
}

impl<T> TtpQueueRegistry<T> {
    pub(crate) fn new(channel_capacity: usize) -> Arc<Self> {
        Arc::new(Self {
            listeners: RwLock::new(HashMap::new()),
            channel_capacity,
        })
    }

    pub(crate) fn as_listen_vports_ref(self: &Arc<Self>) -> ListenVPortsRef
    where
        T: Send + 'static,
    {
        self.clone()
    }

    pub(crate) fn register(
        &self,
        purpose: TunnelPurpose,
    ) -> P2pResult<mpsc::Receiver<P2pResult<T>>> {
        let (tx, rx) = mpsc::channel(self.channel_capacity);
        let mut listeners = self.listeners.write().unwrap();
        if listeners.contains_key(&purpose) {
            return Err(p2p_err!(
                P2pErrorCode::AlreadyExists,
                "ttp purpose {} already listening",
                purpose
            ));
        }
        listeners.insert(purpose, Arc::new(tx));
        Ok(rx)
    }

    pub(crate) fn remove(&self, purpose: &TunnelPurpose) {
        self.listeners.write().unwrap().remove(purpose);
    }

    pub(crate) fn deliver(&self, purpose: &TunnelPurpose, item: P2pResult<T>) -> P2pResult<()> {
        let Some(sender) = self.listeners.read().unwrap().get(purpose).cloned() else {
            return Err(p2p_err!(
                P2pErrorCode::NotFound,
                "ttp purpose {} not listening",
                purpose
            ));
        };
        sender.try_send(item).map_err(|err| {
            self.listeners.write().unwrap().remove(purpose);
            p2p_err!(
                P2pErrorCode::OutOfLimit,
                "ttp purpose {} listener queue full or closed: {}",
                purpose,
                err
            )
        })
    }
}

impl<T> ListenVPorts for TtpQueueRegistry<T>
where
    T: Send + 'static,
{
    fn is_listen(&self, purpose: &TunnelPurpose) -> bool {
        self.listeners.read().unwrap().contains_key(purpose)
    }
}
