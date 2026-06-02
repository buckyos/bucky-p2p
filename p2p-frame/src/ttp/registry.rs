use crate::error::{P2pErrorCode, P2pResult, p2p_err};
use crate::executor::Executor;
use crate::networks::{ListenVPorts, ListenVPortsRef, TunnelPurpose};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, RwLock};

type TtpQueueCallback<T> =
    Arc<dyn Fn(P2pResult<T>) -> Pin<Box<dyn Future<Output = ()> + Send + 'static>> + Send + Sync>;

pub(crate) struct TtpQueueRegistry<T> {
    listeners: RwLock<HashMap<TunnelPurpose, TtpQueueCallback<T>>>,
    _marker: std::marker::PhantomData<fn(T)>,
}

impl<T> TtpQueueRegistry<T> {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            listeners: RwLock::new(HashMap::new()),
            _marker: std::marker::PhantomData,
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
        callback: TtpQueueCallback<T>,
    ) -> P2pResult<()> {
        let mut listeners = self.listeners.write().unwrap();
        if listeners.contains_key(&purpose) {
            return Err(p2p_err!(
                P2pErrorCode::AlreadyExists,
                "ttp purpose {} already listening",
                purpose
            ));
        }
        listeners.insert(purpose, callback);
        Ok(())
    }

    pub(crate) fn remove(&self, purpose: &TunnelPurpose) {
        self.listeners.write().unwrap().remove(purpose);
    }

    pub(crate) fn deliver(&self, purpose: &TunnelPurpose, item: P2pResult<T>) -> P2pResult<()> {
        let Some(callback) = self.listeners.read().unwrap().get(purpose).cloned() else {
            return Err(p2p_err!(
                P2pErrorCode::NotFound,
                "ttp purpose {} not listening",
                purpose
            ));
        };
        Executor::spawn((callback)(item));
        Ok(())
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
