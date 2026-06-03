use crate::error::{P2pErrorCode, P2pResult};
use std::future::Future;
use std::marker::PhantomData;
use std::sync::Mutex;
use tokio::runtime::{Handle, RuntimeFlavor};
use tokio::task::JoinHandle;

pub struct Executor;

pub struct SpawnHandle<Output> {
    task: Mutex<Option<JoinHandle<Output>>>,
    _output: PhantomData<Output>,
}

impl<Output> SpawnHandle<Output> {
    pub fn abort(&self) {
        if let Some(task) = self.task.lock().unwrap().take() {
            task.abort();
        }
    }
}

impl Executor {
    pub fn spawn_with_handle<Fut>(future: Fut) -> P2pResult<SpawnHandle<Fut::Output>>
    where
        Fut: Future + Send + 'static,
        Fut::Output: Send + 'static,
    {
        let task = tokio::spawn(future);
        Ok(SpawnHandle {
            task: Mutex::new(Some(task)),
            _output: PhantomData,
        })
    }

    pub fn spawn<Fut>(future: Fut) -> P2pResult<()>
    where
        Fut: Future<Output = ()> + Send + 'static,
    {
        tokio::spawn(future);
        Ok(())
    }

    pub fn spawn_ok<Fut>(future: Fut)
    where
        Fut: Future<Output = ()> + Send + 'static,
    {
        tokio::spawn(future);
    }
}
