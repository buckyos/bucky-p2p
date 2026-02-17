use crate::error::P2pResult;
use once_cell::sync::OnceCell;
use std::future::Future;
use tokio::runtime::Handle;
use tokio::task::JoinHandle;

pub struct Executor;
pub type SpawnHandle<Output> = JoinHandle<Output>;
static EXECUTOR: OnceCell<tokio::runtime::Runtime> = OnceCell::new();

impl Executor {
    pub fn init_new_multi_thread(pool_size: Option<usize>) {
        EXECUTOR.get_or_init(|| {
            let mut builder = tokio::runtime::Builder::new_multi_thread();
            if let Some(size) = pool_size {
                builder.worker_threads(size);
            }
            builder.enable_all().build().unwrap()
        });
    }

    pub fn init() {
        EXECUTOR.get_or_init(|| tokio::runtime::Runtime::new().unwrap());
    }

    pub fn spawn_with_handle<Fut>(future: Fut) -> P2pResult<JoinHandle<Fut::Output>>
    where
        Fut: Future + Send + 'static,
        Fut::Output: Send,
    {
        Ok(EXECUTOR.get().unwrap().spawn(future))
    }

    pub fn spawn<Fut>(future: Fut) -> P2pResult<()>
    where
        Fut: Future<Output = ()> + Send + 'static,
    {
        EXECUTOR.get().unwrap().spawn(future);
        Ok(())
    }

    pub fn spawn_ok<Fut>(future: Fut)
    where
        Fut: Future<Output = ()> + Send + 'static,
    {
        EXECUTOR.get().unwrap().spawn(future);
    }
    pub fn block_on<F: Future>(f: F) -> F::Output {
        if Handle::try_current().is_ok() {
            tokio::task::block_in_place(|| EXECUTOR.get().unwrap().block_on(f))
        } else {
            EXECUTOR.get().unwrap().block_on(f)
        }
    }
}
