use crate::tunnel::{TunnelDatagramRecv, TunnelDatagramSend};

pub struct DatagramSendGuard {
    send: TunnelDatagramSend,
}


impl DatagramSendGuard {
    pub fn new(send: TunnelDatagramSend) -> Self {
        DatagramSendGuard {
            send,
        }
    }
}

impl std::ops::Deref for DatagramSendGuard {
    type Target = TunnelDatagramSend;

    fn deref(&self) -> &Self::Target {
        &self.send
    }
}

impl std::ops::DerefMut for DatagramSendGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.send
    }
}

pub struct DatagramRecvGuard {
    recv: TunnelDatagramRecv,
}

impl DatagramRecvGuard {
    pub fn new(recv: TunnelDatagramRecv) -> Self {
        DatagramRecvGuard {
            recv,
        }
    }
}

impl std::ops::Deref for DatagramRecvGuard {
    type Target = TunnelDatagramRecv;

    fn deref(&self) -> &Self::Target {
        &self.recv
    }
}

impl std::ops::DerefMut for DatagramRecvGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.recv
    }
}
