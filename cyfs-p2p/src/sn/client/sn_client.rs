use std::sync::{Arc, RwLock};
use std::time::Duration;
use callback_result::{CallbackWaiter, WaiterError};
use cyfs_base::{bucky_time_now, BuckyError, BuckyErrorCode, Device, DeviceId, Endpoint, NamedObject, RawEncodeWithContext, SizedOwnedData};
use crate::error::{bdt_err, BdtError, BdtErrorCode, BdtResult, into_bdt_err};
use crate::history::keystore;
use crate::history::keystore::Keystore;
use crate::protocol::{Exchange, MTU_LARGE, PackageBox, PackageBoxEncodeContext, SnCall, SnPing};
use crate::protocol::v0::{SnCalledResp, SnPingResp};
use crate::sockets::DataSender;
use super::super::types::PingSessionResp;
use crate::sockets::udp::UDPSocket;
use crate::types::{LocalDeviceRef, TempSeqGenerator};

#[derive(Clone)]
pub enum SNResp {
    Ping(SnPingResp),
    Call(SnCalledResp),
}

impl TryInto<SnPingResp> for SNResp {
    type Error = BdtError;

    fn try_into(self) -> Result<SnPingResp, Self::Error> {
        match self {
            SNResp::Ping(resp) => {
                Ok(resp)
            }
            _ => {
                Err(bdt_err!(BdtErrorCode::InvalidData, "SNResp::Call can't convert to PingSessionResp"))
            }
        }
    }
}

impl TryInto<SnCalledResp> for SNResp {
    type Error = BdtError;

    fn try_into(self) -> Result<SnCalledResp, Self::Error> {
        match self {
            SNResp::Call(resp) => {
                Ok(resp)
            }
            _ => {
                Err(bdt_err!(BdtErrorCode::InvalidData, "SNResp::Ping can't convert to SnCalledResp"))
            }
        }
    }
}

pub type SNRespWaiter = CallbackWaiter<String, SNResp>;
pub type SNRespWaiterRef = Arc<SNRespWaiter>;

pub trait SNRespWaiterEx {
    fn get_ping_id(seq: u32) -> String {
        format!("ping_{}", seq)
    }

    fn get_call_id(seq: u32) -> String {
        format!("call_{}", seq)
    }
}

impl SNRespWaiterEx for SNRespWaiter {}

pub struct SNClientState {

}
pub struct SNClient {
    local_device: LocalDeviceRef,
    sn_id: DeviceId,
    sn: Device,
    sn_ep: Endpoint,
    gen_seq: Arc<TempSeqGenerator>,
    data_sender: Box<dyn DataSender>,
    resp_waiter: SNRespWaiterRef,
    with_device: bool,
    key_store: Arc<Keystore>,
    ping_timeout: Duration,
    call_timeout: Duration,
}

impl std::fmt::Display for SNClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SNClient{{sn:{}, local:{}}}", self.sn_id, self.local_device.device_id())
    }
}

impl SNClient {
    pub fn new(
        local_device: LocalDeviceRef,
        sn_id: DeviceId,
        sn: Device,
        sn_ep: Endpoint,
        gen_seq: Arc<TempSeqGenerator>,
        data_sender: Box<dyn DataSender>,
        resp_waiter: SNRespWaiterRef,
        with_device: bool,
        key_store: Arc<Keystore>,
        ping_timeout: Duration,
        call_timeout: Duration,) -> Self {
        SNClient {
            local_device,
            sn_id,
            sn,
            sn_ep,
            gen_seq,
            data_sender,
            resp_waiter,
            with_device,
            key_store,
            ping_timeout,
            call_timeout,
        }
    }

    fn map_ret(ret: Result<SNResp, WaiterError>) -> BdtResult<SNResp> {
        let ret = ret.map_err(|_|bdt_err!(BdtErrorCode::Timeout, "ping timeout"))?;
        Ok(ret)
    }

    pub fn data_sender(&self) -> &Box<dyn DataSender> {
        &self.data_sender
    }

    pub async fn ping(&self) -> BdtResult<SnPingResp> {
        let seq = self.gen_seq.generate();
        let ping_req = SnPing {
            protocol_version: 0,
            stack_version: 0,
            seq: seq.clone(),
            sn_peer_id: self.sn_id.clone(),
            from_peer_id: Some(self.local_device.device_id().clone()),
            peer_info: if self.with_device { Some(self.local_device.device().clone())} else {None},
            send_time: bucky_time_now(),
            contract_id: None,
            receipt: None,
        };

        let key_stub = self.key_store.create_key(self.local_device.device_id(), self.sn.desc(), true);

        info!("{} send sn ping, seq={:?} key={}", self, seq, key_stub.key);
        let mut pkg_box = PackageBox::encrypt_box(
            self.local_device.device_id().clone(),
            self.sn_id.clone(),
            key_stub.key.clone());

        if let keystore::EncryptedKey::Unconfirmed(key_encrypted) = key_stub.encrypted {
            let mut exchg = Exchange::from((&ping_req,
                                            self.local_device.device().clone(),
                                            key_encrypted,
                                            key_stub.key.mix_key.clone()));
            let _ = exchg.sign(&self.key_store.signer(self.local_device.device_id()).unwrap()).await;
            pkg_box.push(exchg);
        }
        pkg_box.push(ping_req);

        let result_future = self.resp_waiter.create_timeout_result_future(SNRespWaiter::get_ping_id(seq.value()), self.ping_timeout);
        self.data_sender.send_pkg_box(&pkg_box).await?;
        let ret = result_future.await;
        Self::map_ret(ret)?.try_into()
    }

    pub async fn call(&self,
                      reverse_endpoints: Option<&[Endpoint]>,
                      remote: &DeviceId,
                      payload_pkg: &PackageBox) -> BdtResult<SnCalledResp> {
        let seq = self.gen_seq.generate();
        let mut call = SnCall {
            protocol_version: 0,
            stack_version: 0,
            seq,
            sn_peer_id: self.sn_id.clone(),
            to_peer_id: remote.clone(),
            from_peer_id: self.local_device.device_id().clone(),
            reverse_endpoint_array: reverse_endpoints.map(|ep_list| Vec::from(ep_list)),
            active_pn_list: None,
            peer_info: Some(self.local_device.device().clone()),
            send_time: 0,
            payload: SizedOwnedData::from(vec![]),
            is_always_call: false,
        };

        let mut context = PackageBoxEncodeContext::from(&call);
        let mut buf = vec![0u8;MTU_LARGE];
        let len = {
            let data = payload_pkg.raw_tail_encode_with_context(buf.as_mut(), &mut context, &None).map_err(into_bdt_err!(BdtErrorCode::RawCodecError))?;
            data.len()
        };
        buf.truncate(len);
        call.payload = SizedOwnedData::from(buf);

        let key_stub = self.key_store.create_key(self.local_device.device_id(), self.sn.desc(), true);
        info!("{} send sn call, seq={:?} key={}", self, seq, key_stub.key);
        let mut packages = PackageBox::encrypt_box(self.local_device.device_id().clone(), self.sn_id.clone(), key_stub.key.clone());
        if let keystore::EncryptedKey::Unconfirmed(encrypted) = &key_stub.encrypted {
            let mut exchange = Exchange::from((&call, encrypted.clone(), key_stub.key.mix_key.clone()));
            let _ = exchange.sign(&self.key_store.signer(self.local_device.device_id()).unwrap()).await.unwrap();
            packages.push(exchange);
        }
        packages.push(call);

        let result_future = self.resp_waiter.create_timeout_result_future(SNRespWaiter::get_call_id(seq.value()), self.call_timeout);
        self.data_sender.send_pkg_box(&packages).await?;
        Self::map_ret(result_future.await)?.try_into()
    }
}
