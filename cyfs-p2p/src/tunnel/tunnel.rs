use std::sync::{Arc, Mutex};
use std::time::Duration;
use callback_result::CallbackWaiter;
use cyfs_base::{bucky_time_now, BuckyError, BuckyErrorCode, BuckyResult, DeviceDesc};
use crate::protocol::{AckTunnel, DynamicPackage, PackageCmdCode, SynTunnel};
use crate::sockets::DataSender;
use crate::{LocalDeviceRef, TempSeq};
use crate::protocol::v0::PingTunnel;
use crate::tunnel::tunnel_connection::{TunnelConnectionKey, TunnelWaiter, TunnelWaiterRef};


pub enum TunnelStatus {
    Connecting,
    Active,
    Dead,
}

struct TunnelState {
    status: TunnelStatus,
}

pub struct Tunnel {
    sequence: TempSeq,
    data_sender: Arc<dyn DataSender>,
    resp_waiter: TunnelWaiterRef,
    state: Mutex<TunnelState>,
    protocol_version: u8,
    stack_version: u32,
    duration: Duration,
    local_device: LocalDeviceRef,
}

impl Tunnel {
    pub fn new(sequence: TempSeq,
               data_sender: Arc<dyn DataSender>,
               protocol_version: u8,
               stack_version: u32,
               local_device: LocalDeviceRef,) -> Self {
        Self {
            sequence,
            data_sender,
            resp_waiter: Arc::new(TunnelWaiter::new()),
            state: Mutex::new(TunnelState { status: TunnelStatus::Connecting }),
            protocol_version,
            stack_version,
            duration: Default::default(),
            local_device,
        }
    }

    pub fn get_sequence(&self) -> TempSeq {
        self.sequence
    }

    pub fn get_resp_waiter(&self) -> &TunnelWaiterRef {
        &self.resp_waiter
    }

    pub async fn connect(&self, extend_pkgs: Vec<DynamicPackage>) -> BuckyResult<()> {
        let syn = SynTunnel {
            protocol_version: self.protocol_version,
            stack_version: self.stack_version,
            to_device_id: self.data_sender.remote_device_id().clone(),
            sequence: self.sequence,
            from_device_desc: self.local_device.device().clone(),
            send_time: bucky_time_now(),
        };

        let result_future = self.resp_waiter.create_timeout_result_future(self.sequence, self.duration);
        let mut pkgs = vec![DynamicPackage::from(syn)];
        pkgs.extend(extend_pkgs);
        self.data_sender.send_dynamic_pkgs(pkgs).await?;
        let result = result_future.await.map_err(|_| BuckyError::from((BuckyErrorCode::Timeout, "syn timeout")))?;
        if result.cmd_code() != PackageCmdCode::AckTunnel {
            return Err(BuckyError::from((BuckyErrorCode::InvalidData, "invalid ack tunnel")));
        }

        let ack: &AckTunnel = result.as_ref();
        if BuckyErrorCode::from(ack.result as u16) != BuckyErrorCode::Ok {
            return Err(BuckyError::from((BuckyErrorCode::InvalidData, "ack tunnel failed")));
        }
        Ok(())
    }

    pub async fn ping(&self) -> BuckyResult<()> {
        let ping = PingTunnel {
            package_id: self.sequence.value(),
            send_time: bucky_time_now(),
            recv_data: 0,
        };

        let result_future = self.resp_waiter.create_timeout_result_future(self.sequence, self.duration);
        self.data_sender.send_dynamic_pkg(DynamicPackage::from(ping)).await?;
        let result = result_future.await.map_err(|_| BuckyError::from((BuckyErrorCode::Timeout, "ping timeout")))?;
        if result.cmd_code() != PackageCmdCode::PingTunnelResp {
            return Err(BuckyError::from((BuckyErrorCode::InvalidData, "invalid ping tunnel resp")));
        }
        Ok(())
    }
}
