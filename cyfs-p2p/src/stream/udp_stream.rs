use std::net::Shutdown;
use crate::error::BdtResult;
use crate::stream::Stream;
use crate::tunnel::TunnelGuard;

pub struct UdpStream {
    tunnel: TunnelGuard
}

impl UdpStream {
    pub fn new(tunnel: TunnelGuard) -> Self {
        Self {
            tunnel,
        }
    }
}

#[async_trait::async_trait]
impl Stream for UdpStream {
    async fn write(&mut self, buf: &[u8]) -> BdtResult<usize> {
        todo!()
    }

    async fn write_all(&mut self, buf: &[u8]) -> BdtResult<()> {
        todo!()
    }

    async fn flush(&mut self) -> BdtResult<()> {
        todo!()
    }

    async fn read(&mut self, buf: &mut [u8]) -> BdtResult<usize> {
        todo!()
    }

    async fn read_exact(&mut self, buf: &mut [u8]) -> BdtResult<()> {
        todo!()
    }

    async fn shutdown(&self, how: Shutdown) -> BdtResult<()> {
        self.tunnel.shutdown(how).await
    }
}
