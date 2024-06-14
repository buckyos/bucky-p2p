use std::future::Future;
use futures::executor::ThreadPool;
use futures::future::RemoteHandle;
use futures::task::{Spawn, SpawnExt};
use once_cell::sync::OnceCell;
use crate::error::{BdtErrorCode, BdtResult, into_bdt_err};

pub struct Executor;

static EXECUTOR: OnceCell<ThreadPool> = OnceCell::new();

impl Executor {
    pub fn init(pool_size: Option<usize>) {
        EXECUTOR.get_or_init(|| {
            let mut builder = ThreadPool::builder();
            if pool_size.is_some() {
                builder.pool_size(pool_size.unwrap());
            }
            builder.create().unwrap()
        });
    }

    pub fn spawn_with_handle<Fut>(future: Fut) -> BdtResult<RemoteHandle<Fut::Output>>
        where
            Fut: Future + Send + 'static,
            Fut::Output: Send, {
        EXECUTOR.get().unwrap().spawn_with_handle(future).map_err(into_bdt_err!(BdtErrorCode::ExecuteError))
    }

    pub fn spawn<Fut>(future: Fut) -> BdtResult<()>
        where
            Fut: Future<Output = ()> + Send + 'static,{
        EXECUTOR.get().unwrap().spawn(future);
        Ok(())
    }

    pub fn spawn_ok<Fut>(future: Fut)
        where
            Fut: Future<Output = ()> + Send + 'static, {
        EXECUTOR.get().unwrap().spawn_ok(future);
    }
    pub fn block_on<F: Future>(f: F) -> F::Output {
        futures::executor::block_on(f)
    }
}
