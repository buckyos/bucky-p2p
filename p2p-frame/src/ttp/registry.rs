use crate::error::{P2pErrorCode, P2pResult, p2p_err};
use crate::networks::{ListenVPorts, ListenVPortsRef, TunnelPurpose};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tokio::sync::mpsc;

pub(crate) struct TtpQueueRegistry<T> {
    listeners: RwLock<HashMap<TunnelPurpose, Arc<mpsc::UnboundedSender<P2pResult<T>>>>>,
}

impl<T> TtpQueueRegistry<T> {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            listeners: RwLock::new(HashMap::new()),
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
    ) -> P2pResult<mpsc::UnboundedReceiver<P2pResult<T>>> {
        let (tx, rx) = mpsc::unbounded_channel();
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

    pub(crate) fn deliver(&self, purpose: &TunnelPurpose, item: P2pResult<T>) {
        let sender = self.listeners.read().unwrap().get(purpose).cloned();
        if let Some(sender) = sender {
            if sender.send(item).is_err() {
                self.listeners.write().unwrap().remove(purpose);
            }
        }
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
