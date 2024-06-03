use std::net::Shutdown;
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
pub trait StreamWriter: 'static + Send + Sync {
    async fn write(&mut self, buf: &[u8]) -> BdtResult<usize>;
    async fn write_all(&mut self, buf: &[u8]) -> BdtResult<()>;
    async fn flush(&mut self) -> BdtResult<()>;
}

pub struct StreamWriterGuard {
    writer: Box<dyn StreamWriter>,
}

impl StreamWriterGuard {
    pub(crate) fn new(writer: Box<dyn StreamWriter>) -> Self {
        Self {
            writer
        }
    }
}

impl Deref for StreamWriterGuard {
    type Target = dyn StreamWriter;

    fn deref(&self) -> &Self::Target {
        self.writer.as_ref()
    }
}

impl DerefMut for StreamWriterGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.writer.as_mut()
    }
}

#[async_trait::async_trait]
pub trait StreamReader: 'static + Send + Sync {
    async fn read(&mut self, buf: &mut [u8]) -> BdtResult<usize>;
    async fn read_exact(&mut self, buf: &mut [u8]) -> BdtResult<()>;
}

#[async_trait::async_trait]
pub trait Stream: 'static + Send + Sync {
    async fn write(&mut self, buf: &[u8]) -> BdtResult<usize>;
    async fn write_all(&mut self, buf: &[u8]) -> BdtResult<()>;
    async fn flush(&mut self) -> BdtResult<()>;
    async fn read(&mut self, buf: &mut [u8]) -> BdtResult<usize>;
    async fn read_exact(&mut self, buf: &mut [u8]) -> BdtResult<()>;
    async fn shutdown(&self, how: Shutdown) -> BdtResult<()>;
}
