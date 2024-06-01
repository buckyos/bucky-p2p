use crate::error::BdtResult;
use crate::stream::Stream;
use crate::tunnel::TunnelGuard;

pub struct TcpStream {
    tunnel: TunnelGuard
}

impl TcpStream {
    pub fn new(tunnel: TunnelGuard) -> Self {
        Self {
            tunnel,
        }
    }
}

#[async_trait::async_trait]
impl Stream for TcpStream {
    async fn write(&self, buf: &[u8]) -> BdtResult<usize> {
        todo!()
    }

    async fn read(&self, buf: &mut [u8]) -> BdtResult<usize> {
        todo!()
    }
}
