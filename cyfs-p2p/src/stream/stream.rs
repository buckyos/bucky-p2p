use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use crate::error::BdtResult;

pub struct StreamGuard {
    stream: Arc<dyn Stream>
}

impl StreamGuard {
    pub(crate) fn new(stream: Arc<dyn Stream>) -> Self {
        Self {
            stream
        }
    }
}

impl Deref for StreamGuard {
    type Target = Arc<dyn Stream>;

    fn deref(&self) -> &Self::Target {
        &self.stream
    }
}

#[async_trait::async_trait]
pub trait Stream: 'static + Send + Sync {
    async fn write(&self, buf: &[u8]) -> BdtResult<usize>;
    async fn read(&self, buf: &mut [u8]) -> BdtResult<usize>;
}
