use std::net::Shutdown;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use crate::error::BdtResult;
use crate::tunnel::TunnelStream;

pub struct StreamGuard {
    stream: Box<dyn TunnelStream>
}

impl StreamGuard {
    pub(crate) fn new(stream: Box<dyn TunnelStream>) -> Self {
        Self {
            stream
        }
    }
}

impl Deref for StreamGuard {
    type Target = Box<dyn TunnelStream>;

    fn deref(&self) -> &Self::Target {
        &self.stream
    }
}

impl DerefMut for StreamGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.stream
    }
}
