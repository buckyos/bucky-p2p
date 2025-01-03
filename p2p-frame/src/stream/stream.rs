use std::ops::{Deref, DerefMut};
use crate::tunnel::TunnelStream;

pub struct StreamGuard {
    stream: TunnelStream
}

impl StreamGuard {
    pub fn new(stream: TunnelStream) -> Self {
        Self {
            stream
        }
    }
}

impl Deref for StreamGuard {
    type Target = TunnelStream;

    fn deref(&self) -> &Self::Target {
        &self.stream
    }
}

impl DerefMut for StreamGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.stream
    }
}
