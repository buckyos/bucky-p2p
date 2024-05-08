use std::time::{Duration, SystemTime, UNIX_EPOCH};
use cyfs_base::{AesKey, Area, bucky_time_now, bucky_time_to_system_time, BuckyError, BuckyErrorCode, Device, DeviceCategory, DeviceDesc, DeviceId, Endpoint, MIN_BUCKY_TIME, NamedObject, ObjectId, PrivateKey, RawConvertTo, RawDecode, RawDecodeWithContext, RawEncode, RawEncodePurpose, RawEncodeWithContext, RawFixedBytes, Signature, SizedOwnedData, SizeU16, system_time_to_bucky_time, UniqueId};
use crate::protocol::{Exchange, merge_context, Package, PackageCmdCode};
use crate::protocol::common::context;
use crate::types::{TempSeq, Timestamp};


#[derive(Clone)]
pub struct SnCall {
    pub protocol_version: u8,
    pub stack_version: u32,
    pub seq: TempSeq,
    pub sn_peer_id: DeviceId,
    pub to_peer_id: DeviceId,
    pub from_peer_id: DeviceId,
    pub reverse_endpoint_array: Option<Vec<Endpoint>>,
    pub active_pn_list: Option<Vec<DeviceId>>,
    pub peer_info: Option<Device>,
    pub send_time: Timestamp,
    pub payload: SizedOwnedData<SizeU16>,
    pub is_always_call: bool,
}


impl std::fmt::Debug for SnCall {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "SnCall:{{seq:{:?}, sn_peer_id:{:?}, to_peer_id:{}, from_peer_id:{:?}, reverse_endpoint_array:{:?}, active_pn_list:{:?}, peer_info:{}, payload:{}}}",
            self.seq,
            self.sn_peer_id,
            self.to_peer_id,
            self.from_peer_id,
            self.reverse_endpoint_array,
            self.active_pn_list,
            self.peer_info.is_some(),
            self.payload.len()
        )
    }
}


impl Package for SnCall {
    fn version(&self) -> u8 {
        self.protocol_version
    }

    fn cmd_code() -> PackageCmdCode {
        PackageCmdCode::SnCall
    }
}




impl From<(&SnCall, Vec<u8>, AesKey)> for Exchange {
    fn from(context: (&SnCall, Vec<u8>, AesKey)) -> Self {
        let (sn_call, key_encrypted, mix_key) = context;

        Self {
            sequence: sn_call.seq.clone(),
            to_device_id: sn_call.sn_peer_id.clone(),
            send_time: sn_call.send_time,
            key_encrypted,
            sign: Signature::default(),
            from_device_desc: sn_call.peer_info.clone().unwrap(),
            mix_key
        }
    }
}

impl Into<merge_context::FixedValues> for &SnCall {
    fn into(self) -> merge_context::FixedValues {
        let mut v = merge_context::FixedValues::new();
        v.insert("sequence", self.seq)
            .insert("to_device_id", self.to_peer_id.clone())
            .insert("from_device_id", self.from_peer_id.clone())
            .insert("send_time", self.send_time);
        if let Some(dev) = self.peer_info.as_ref() {
            v.insert("device_desc", dev.clone());
        }
        v
    }
}

impl<Context: merge_context::Encode> RawEncodeWithContext<Context> for SnCall {
    fn raw_measure_with_context(
        &self,
        _merge_context: &mut Context,
        _purpose: &Option<RawEncodePurpose>,
    ) -> Result<usize, BuckyError> {
        unimplemented!()
    }

    fn raw_encode_with_context<'a>(
        &self,
        enc_buf: &'a mut [u8],
        merge_context: &mut Context,
        _purpose: &Option<RawEncodePurpose>,
    ) -> Result<&'a mut [u8], BuckyError> {
        let mut flags = context::FlagsCounter::new();
        let (mut context, buf) = context::Encode::<Self, Context>::new(enc_buf, merge_context)?;
        let buf = context.encode(buf, &self.protocol_version, context::FLAG_ALWAYS_ENCODE)?;
        let buf = context.encode(buf, &self.stack_version, context::FLAG_ALWAYS_ENCODE)?;
        let buf = context.check_encode(buf, "sequence", &self.seq, flags.next())?;
        let buf = context.check_encode(buf, "sn_device_id", &self.sn_peer_id, flags.next())?;
        let buf = context.check_encode(buf, "to_device_id", &self.to_peer_id, flags.next())?;
        let buf = context.check_encode(buf, "from_device_id", &self.from_peer_id, flags.next())?;
        let buf = context.encode(buf, &self.reverse_endpoint_array, flags.next())?;
        let buf = context.encode(buf, &self.active_pn_list, flags.next())?;
        let buf = context.check_option_encode(buf, "device_desc", &self.peer_info, flags.next())?;
        let buf = context.check_encode(buf, "send_time", &self.send_time, flags.next())?;
        let _buf = context.encode(buf, &self.payload, flags.next())?;
        context.set_flags({
            let f = flags.next();
            if self.is_always_call {
                f
            } else {
                0
            }
        });
        context.finish(enc_buf)
    }
}

impl<'de, Context: merge_context::Decode> RawDecodeWithContext<'de, &mut Context> for SnCall {
    fn raw_decode_with_context(
        buf: &'de [u8],
        merge_context: &mut Context,
    ) -> Result<(Self, &'de [u8]), BuckyError> {
        let mut flags = context::FlagsCounter::new();
        let (mut context, buf) = context::Decode::new(buf, merge_context)?;
        let (protocol_version, buf) = context.decode(buf, "SnCall.protocol_version", context::FLAG_ALWAYS_DECODE)?;
        let (stack_version, buf) = context.decode(buf, "SnCall.stack_version", context::FLAG_ALWAYS_DECODE)?;
        let (seq, buf) = context.check_decode(buf, "sequence", flags.next())?;
        let (sn_peer_id, buf) = context.check_decode(buf, "sn_device_id", flags.next())?;
        let (to_peer_id, buf) = context.check_decode(buf, "to_device_id", flags.next())?;
        let (from_peer_id, buf) = context.check_decode(buf, "from_device_id", flags.next())?;
        let (reverse_endpoint_array, buf) = context.decode(buf, "SnCall.reverse_endpoint_array", flags.next())?;
        let (active_pn_list, buf) = context.decode(buf, "SnCall.active_pn_list", flags.next())?;
        let (peer_info, buf) = context.check_option_decode(buf, "device_desc", flags.next())?;
        let (send_time, buf) = context.check_decode(buf, "send_time", flags.next())?;
        let (payload, buf) = context.decode(buf, "SnCall.payload", flags.next())?;
        let is_always_call = context.check_flags(flags.next());

        Ok((
            Self {
                protocol_version,
                stack_version,
                seq,
                to_peer_id,
                from_peer_id,
                sn_peer_id,
                reverse_endpoint_array,
                active_pn_list,
                peer_info,
                payload,
                send_time,
                is_always_call,
            },
            buf,
        ))
    }
}

#[test]
fn encode_protocol_sn_call() {
    let private_key = PrivateKey::generate_rsa(1024).unwrap();
    let from_device = Device::new(
        None,
        UniqueId::default(),
        vec![],
        vec![],
        vec![],
        private_key.public(),
        Area::default(),
        DeviceCategory::PC,
    )
        .build();

    let private_key = PrivateKey::generate_rsa(1024).unwrap();
    let to_device = Device::new(
        None,
        UniqueId::default(),
        vec![],
        vec![],
        vec![],
        private_key.public(),
        Area::default(),
        DeviceCategory::PC,
    )
        .build();

    let data = "hello".as_bytes().to_vec();
    let mut eps = Vec::new();
    eps.push(Endpoint::from_str("W4tcp10.10.10.10:8060").unwrap());
    eps.push(Endpoint::from_str("W4tcp10.10.10.11:8060").unwrap());
    let mut pn = Vec::new();
    pn.push(from_device.desc().device_id().clone());

    let src = SnCall {
        protocol_version: 0,
        stack_version: 0,
        seq: TempSeq::from(rand::random::<u32>()),
        sn_peer_id: to_device.desc().device_id(),
        to_peer_id: to_device.desc().device_id(),
        from_peer_id: from_device.desc().device_id(),
        reverse_endpoint_array: Some(eps),
        active_pn_list: Some(pn),
        peer_info: Some(from_device),
        send_time: bucky_time_now(),
        payload: SizedOwnedData::from(data),
        is_always_call: true,
    };

    let mut buf = [0u8; 4096];
    let remain = src
        .raw_encode_with_context(&mut buf, &mut merge_context::OtherEncode::default(), &None)
        .unwrap();
    let remain = remain.len();

    let dec = &buf[..buf.len() - remain];
    let (cmd, dec) = u8::raw_decode(dec)
        .map(|(code, dec)| (PackageCmdCode::try_from(code).unwrap(), dec))
        .unwrap();
    assert_eq!(cmd, PackageCmdCode::SnCall);
    let (dst, _) =
        SnCall::raw_decode_with_context(&dec, &mut merge_context::OtherDecode::default()).unwrap();

    assert_eq!(dst.seq, src.seq);
    assert_eq!(dst.sn_peer_id, src.sn_peer_id);
    assert_eq!(dst.to_peer_id, src.to_peer_id);
    assert_eq!(dst.from_peer_id, src.from_peer_id);
    assert_eq!(dst.reverse_endpoint_array, src.reverse_endpoint_array);

    let dst_pn = dst.active_pn_list.to_hex().unwrap();
    let src_pn = src.active_pn_list.to_hex().unwrap();
    assert_eq!(dst_pn, src_pn);

    let dst_peer_info = dst.peer_info.to_hex().unwrap();
    let src_peer_info = src.peer_info.to_hex().unwrap();
    assert_eq!(dst_peer_info, src_peer_info);

    assert_eq!(dst.send_time, src.send_time);

    let dst_payload = dst.payload.to_hex().unwrap();
    let src_payload = src.payload.to_hex().unwrap();
    assert_eq!(dst_payload, src_payload);

    assert_eq!(dst.is_always_call, src.is_always_call);
}

#[derive(Clone, Copy, PartialEq, PartialOrd, Ord, Eq, Debug)]
pub enum SnServiceGrade {
    None = 0,
    Discard = 1,
    Passable = 2,
    Normal = 3,
    Fine = 4,
    Wonderfull = 5,
}

impl SnServiceGrade {
    pub fn is_accept(&self) -> bool {
        *self >= SnServiceGrade::Passable
    }
    pub fn is_refuse(&self) -> bool {
        !self.is_accept()
    }
}

impl TryFrom<u8> for SnServiceGrade {
    type Error = BuckyError;
    fn try_from(v: u8) -> Result<Self, Self::Error> {
        match v {
            0 => Ok(Self::None),
            1 => Ok(Self::Discard),
            2 => Ok(Self::Passable),
            3 => Ok(Self::Normal),
            4 => Ok(Self::Fine),
            5 => Ok(Self::Wonderfull),
            _ => Err(BuckyError::new(
                BuckyErrorCode::InvalidParam,
                "invalid SnServiceGrade value",
            )),
        }
    }
}

impl RawFixedBytes for SnServiceGrade {
    fn raw_bytes() -> Option<usize> {
        Some(1)
    }
}

impl RawEncode for SnServiceGrade {
    fn raw_measure(&self, _purpose: &Option<RawEncodePurpose>) -> Result<usize, BuckyError> {
        Ok(Self::raw_bytes().unwrap())
    }

    fn raw_encode<'a>(
        &self,
        buf: &'a mut [u8],
        _purpose: &Option<RawEncodePurpose>,
    ) -> Result<&'a mut [u8], BuckyError> {
        let bytes = Self::raw_bytes().unwrap();
        if buf.len() < bytes {
            let msg = format!("not enough buffer for encode SnServiceGrade, except={}, got={}", bytes, buf.len());
            error!("{}", msg);

            return Err(BuckyError::new(
                BuckyErrorCode::OutOfLimit,
                msg,
            ));
        }
        buf[0] = (*self) as u8;
        Ok(&mut buf[1..])
    }
}

impl<'de> RawDecode<'de> for SnServiceGrade {
    fn raw_decode(buf: &'de [u8]) -> Result<(Self, &'de [u8]), BuckyError> {
        let bytes = Self::raw_bytes().unwrap();
        if buf.len() < bytes {
            let msg = format!("not enough buffer for decode SnServiceGrade, except={}, got={}", bytes, buf.len());
            error!("{}", msg);

            return Err(BuckyError::new(
                BuckyErrorCode::OutOfLimit,
                msg,
            ));
        }
        let v = Self::try_from(buf[0])?;
        Ok((v, &buf[Self::raw_bytes().unwrap()..]))
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum SnServiceReceiptVersion {
    Invalid = 0,
    Current = 1,
}

impl TryFrom<u8> for SnServiceReceiptVersion {
    type Error = BuckyError;
    fn try_from(v: u8) -> Result<Self, Self::Error> {
        match v {
            0 => Ok(Self::Invalid),
            1 => Ok(Self::Current),
            _ => Err(BuckyError::new(
                BuckyErrorCode::UnSupport,
                format!("unsupport version({})", v).as_str(),
            )),
        }
    }
}

impl RawFixedBytes for SnServiceReceiptVersion {
    fn raw_bytes() -> Option<usize> {
        Some(1)
    }
}

impl RawEncode for SnServiceReceiptVersion {
    fn raw_measure(&self, _purpose: &Option<RawEncodePurpose>) -> Result<usize, BuckyError> {
        Ok(Self::raw_bytes().unwrap())
    }

    fn raw_encode<'a>(
        &self,
        buf: &'a mut [u8],
        _purpose: &Option<RawEncodePurpose>,
    ) -> Result<&'a mut [u8], BuckyError> {
        let bytes = Self::raw_bytes().unwrap();
        if buf.len() < bytes {
            let msg = format!("not enough buffer for encode SnServiceReceiptVersion, except={}, got={}", bytes, buf.len());
            error!("{}", msg);

            return Err(BuckyError::new(
                BuckyErrorCode::OutOfLimit,
                msg,
            ));
        }
        buf[0] = (*self) as u8;
        Ok(&mut buf[1..])
    }
}

impl<'de> RawDecode<'de> for SnServiceReceiptVersion {
    fn raw_decode(buf: &'de [u8]) -> Result<(Self, &'de [u8]), BuckyError> {
        let bytes = Self::raw_bytes().unwrap();
        if buf.len() < bytes {
            let msg = format!("not enough buffer for decode SnServiceReceiptVersion, except={}, got={}", bytes, buf.len());
            error!("{}", msg);

            return Err(BuckyError::new(
                BuckyErrorCode::OutOfLimit,
                msg,
            ));
        }
        let v = Self::try_from(buf[0])?;
        Ok((v, &buf[Self::raw_bytes().unwrap()..]))
    }
}

struct SnServiceReceiptSignature {
    sn_peerid: DeviceId,
    receipt: SnServiceReceipt,
}

impl RawEncode for SnServiceReceiptSignature {
    fn raw_measure(&self, purpose: &Option<RawEncodePurpose>) -> Result<usize, BuckyError> {
        let len = self.sn_peerid.raw_measure(purpose)? + self.receipt.raw_measure(purpose)?;
        Ok(len)
    }

    fn raw_encode<'a>(
        &self,
        buf: &'a mut [u8],
        purpose: &Option<RawEncodePurpose>,
    ) -> Result<&'a mut [u8], BuckyError> {
        let buf = self.sn_peerid.raw_encode(buf, purpose)?;
        self.receipt.raw_encode(buf, purpose)
    }
}

#[derive(Copy, Clone, Debug)]
pub struct SnServiceReceipt {
    pub version: SnServiceReceiptVersion,
    pub grade: SnServiceGrade,
    pub rto: u16,
    pub duration: Duration,
    pub start_time: SystemTime,
    pub ping_count: u32,
    pub ping_resp_count: u32,
    pub called_count: u32,
    pub call_peer_count: u32,
    pub connect_peer_count: u32,
    pub call_delay: u16,
}

impl SnServiceReceipt {
    pub fn sign(
        &self,
        sn_peerid: &DeviceId,
        _private_key: &PrivateKey,
    ) -> Result<Signature, BuckyError> {
        let _sig_fields = SnServiceReceiptSignature {
            sn_peerid: sn_peerid.clone(),
            receipt: self.clone(),
        };
        //FIMXE: sign
        unimplemented!()
        // Authorized::sign(&sig_fields, private_key)
    }

    pub fn verify(
        &self,
        sn_peerid: &DeviceId,
        _sign: &Signature,
        _const_info: &DeviceDesc,
    ) -> bool {
        let _sig_fields = SnServiceReceiptSignature {
            sn_peerid: sn_peerid.clone(),
            receipt: self.clone(),
        };
        //FIMXE: verify
        unimplemented!()
        //Authorized::verify(&sig_fields, sign, const_info)
    }
}

impl Default for SnServiceReceipt {
    fn default() -> Self {
        SnServiceReceipt {
            version: SnServiceReceiptVersion::Invalid,
            grade: SnServiceGrade::None,
            rto: 0,
            duration: Duration::from_millis(0),
            start_time: UNIX_EPOCH,
            ping_count: 0,
            ping_resp_count: 0,
            called_count: 0,
            call_peer_count: 0,
            connect_peer_count: 0,
            call_delay: 0,
        }
    }
}

impl RawEncode for SnServiceReceipt {
    fn raw_measure(&self, purpose: &Option<RawEncodePurpose>) -> Result<usize, BuckyError> {
        let mut size = self.version.raw_measure(purpose)?;
        size += self.grade.raw_measure(purpose)?;
        size += self.rto.raw_measure(purpose)?;
        size += 0u32.raw_measure(purpose)?;
        size += 0u64.raw_measure(purpose)?;
        size += self.ping_count.raw_measure(purpose)?;
        size += self.ping_resp_count.raw_measure(purpose)?;
        size += self.called_count.raw_measure(purpose)?;
        size += self.call_peer_count.raw_measure(purpose)?;
        size += self.connect_peer_count.raw_measure(purpose)?;
        size += self.call_delay.raw_measure(purpose)?;
        Ok(size)
    }

    fn raw_encode<'a>(
        &self,
        buf: &'a mut [u8],
        purpose: &Option<RawEncodePurpose>,
    ) -> Result<&'a mut [u8], BuckyError> {
        let buf = self.version.raw_encode(buf, purpose)?;
        let buf = self.grade.raw_encode(buf, purpose)?;
        let buf = self.rto.raw_encode(buf, purpose)?;
        let buf = (self.duration.as_millis() as u32).raw_encode(buf, purpose)?;
        let buf = system_time_to_bucky_time(&self.start_time).raw_encode(buf, purpose)?;
        let buf = self.ping_count.raw_encode(buf, purpose)?;
        let buf = self.ping_resp_count.raw_encode(buf, purpose)?;
        let buf = self.called_count.raw_encode(buf, purpose)?;
        let buf = self.call_peer_count.raw_encode(buf, purpose)?;
        let buf = self.connect_peer_count.raw_encode(buf, purpose)?;
        let buf = self.call_delay.raw_encode(buf, purpose)?;
        Ok(buf)
    }
}

impl<'de> RawDecode<'de> for SnServiceReceipt {
    fn raw_decode(buf: &'de [u8]) -> Result<(Self, &'de [u8]), BuckyError> {
        let (version, buf) = SnServiceReceiptVersion::raw_decode(buf)?;
        let (grade, buf) = SnServiceGrade::raw_decode(buf)?;
        let (rto, buf) = u16::raw_decode(buf)?;
        let (duration, buf) = u32::raw_decode(buf)?;
        let duration = Duration::from_millis(duration as u64);
        let (timestamp, buf) = Timestamp::raw_decode(buf)?;
        if timestamp < MIN_BUCKY_TIME {
            return Err(BuckyError::new(
                BuckyErrorCode::InvalidData,
                "invalid timestamp",
            ));
        }
        let start_time = bucky_time_to_system_time(timestamp);
        let (ping_count, buf) = u32::raw_decode(buf)?;
        let (ping_resp_count, buf) = u32::raw_decode(buf)?;
        let (called_count, buf) = u32::raw_decode(buf)?;
        let (call_peer_count, buf) = u32::raw_decode(buf)?;
        let (connect_peer_count, buf) = u32::raw_decode(buf)?;
        let (call_delay, buf) = u16::raw_decode(buf)?;
        Ok((
            SnServiceReceipt {
                version,
                grade,
                rto,
                duration,
                start_time,
                ping_count,
                ping_resp_count,
                called_count,
                call_peer_count,
                connect_peer_count,
                call_delay,
            },
            buf,
        ))
    }
}

#[derive(Debug, Clone)]
pub struct ReceiptWithSignature(SnServiceReceipt, Signature);

impl ReceiptWithSignature {
    pub fn receipt(&self) -> &SnServiceReceipt {
        &self.0
    }

    pub fn signature(&self) -> &Signature {
        &self.1
    }
}

impl RawEncode for ReceiptWithSignature {
    fn raw_measure(&self, purpose: &Option<RawEncodePurpose>) -> Result<usize, BuckyError> {
        Ok(self.0.raw_measure(purpose)? + self.1.raw_measure(purpose)?)
    }

    fn raw_encode<'a>(
        &self,
        buf: &'a mut [u8],
        purpose: &Option<RawEncodePurpose>,
    ) -> Result<&'a mut [u8], BuckyError> {
        let buf = self.0.raw_encode(buf, purpose)?;
        let buf = self.1.raw_encode(buf, purpose)?;
        Ok(buf)
    }
}

impl<'de> RawDecode<'de> for ReceiptWithSignature {
    fn raw_decode(buf: &'de [u8]) -> Result<(Self, &'de [u8]), BuckyError> {
        let (receipt, buf) = RawDecode::raw_decode(buf)?;
        let (sig, buf) = RawDecode::raw_decode(buf)?;
        Ok((Self(receipt, sig), buf))
    }
}

impl From<(SnServiceReceipt, Signature)> for ReceiptWithSignature {
    fn from(v: (SnServiceReceipt, Signature)) -> Self {
        Self(v.0, v.1)
    }
}

#[test]
fn encode_protocol_sn_ping() {
    use crate::sockets::udp;

    let private_key = PrivateKey::generate_rsa(1024).unwrap();
    let from_device = Device::new(
        None,
        UniqueId::default(),
        vec![],
        vec![],
        vec![],
        private_key.public(),
        Area::default(),
        DeviceCategory::PC,
    )
        .build();

    let private_key = PrivateKey::generate_rsa(1024).unwrap();
    let to_device = Device::new(
        None,
        UniqueId::default(),
        vec![],
        vec![],
        vec![],
        private_key.public(),
        Area::default(),
        DeviceCategory::PC,
    )
        .build();

    let ssr = SnServiceReceipt {
        version: SnServiceReceiptVersion::Invalid,
        grade: SnServiceGrade::None,
        rto: rand::random::<u16>(),
        duration: std::time::Duration::from_millis(0),
        start_time: std::time::UNIX_EPOCH,
        ping_count: rand::random::<u32>(),
        ping_resp_count: rand::random::<u32>(),
        called_count: rand::random::<u32>(),
        call_peer_count: rand::random::<u32>(),
        connect_peer_count: rand::random::<u32>(),
        call_delay: rand::random::<u16>(),
    };

    let sig = Signature::default();
    let receipt = ReceiptWithSignature::from((ssr, sig));
    let src = SnPing {
        protocol_version: 0,
        stack_version: 0,
        seq: TempSeq::from(rand::random::<u32>()),
        sn_peer_id: to_device.desc().device_id(),
        from_peer_id: Some(from_device.desc().device_id()),
        peer_info: Some(to_device),
        send_time: bucky_time_now(),
        contract_id: Some(
            ObjectId::from_str("5aSixgLtjoYcAFH9isc6KCqDgKfTJ8jpgASAoiRz5NLk").unwrap(),
        ),
        receipt: Some(receipt),
    };

    let mut buf = [0u8; udp::MTU];
    let remain = src
        .raw_encode_with_context(&mut buf, &mut merge_context::OtherEncode::default(), &None)
        .unwrap();
    let remain = remain.len();

    let dec = &buf[..buf.len() - remain];
    let (cmd, dec) = u8::raw_decode(dec)
        .map(|(code, dec)| (PackageCmdCode::try_from(code).unwrap(), dec))
        .unwrap();
    assert_eq!(cmd, PackageCmdCode::SnPing);
    let (dst, _) =
        SnPing::raw_decode_with_context(&dec, &mut merge_context::OtherDecode::default()).unwrap();

    assert_eq!(dst.seq, src.seq);
    assert_eq!(dst.sn_peer_id, src.sn_peer_id);
    assert_eq!(dst.from_peer_id, src.from_peer_id);

    let dst_peer_info = dst.peer_info.to_hex().unwrap();
    let src_peer_info = src.peer_info.to_hex().unwrap();
    assert_eq!(dst_peer_info, src_peer_info);

    assert_eq!(dst.send_time, src.send_time);

    let dst_contract_id = dst.contract_id.to_hex().unwrap();
    let src_contract_id = src.contract_id.to_hex().unwrap();
    assert_eq!(dst_contract_id, src_contract_id);

    let dst_receipt = dst.receipt.unwrap().to_hex().unwrap();
    let src_receipt = src.receipt.unwrap().to_hex().unwrap();
    assert_eq!(dst_receipt, src_receipt);
}


#[derive(Debug, Clone)]
pub struct SnPing {
    pub protocol_version: u8,
    pub stack_version: u32,
    //ln与sn的keepalive包
    pub seq: TempSeq,                          //序列号
    pub sn_peer_id: DeviceId,                  //sn的设备id
    pub from_peer_id: Option<DeviceId>,        //发送者设备id
    pub peer_info: Option<Device>,             //发送者设备信息
    pub send_time: Timestamp,                  //发送时间
    pub contract_id: Option<ObjectId>,         //合约文件对象id
    pub receipt: Option<ReceiptWithSignature>, //客户端提供的服务清单
}

impl From<(&SnPing, Device, Vec<u8>, AesKey)> for Exchange {
    fn from(context: (&SnPing, Device, Vec<u8>, AesKey)) -> Self {
        let (sn_ping, local_device, key_encrypted, mix_key) = context;
        Self {
            sequence: sn_ping.seq.clone(),
            to_device_id: sn_ping.sn_peer_id.clone(),
            send_time: sn_ping.send_time,
            key_encrypted,
            sign: Signature::default(),
            from_device_desc: local_device,
            mix_key,
        }
    }
}


impl Into<merge_context::FixedValues> for &SnPing {
    fn into(self) -> merge_context::FixedValues {
        let mut v = merge_context::FixedValues::new();
        v.insert("sequence", self.seq)
            .insert("to_device_id", self.sn_peer_id.clone())
            .insert("from_device_id", self.from_peer_id.clone())
            .insert("device_desc", self.peer_info.clone())
            .insert("send_time", self.send_time);
        v
    }
}

impl Package for SnPing {
    fn version(&self) -> u8 {
        self.protocol_version
    }

    fn cmd_code() -> PackageCmdCode {
        PackageCmdCode::SnPing
    }
}

impl<Context: merge_context::Encode> RawEncodeWithContext<Context> for SnPing {
    fn raw_measure_with_context(
        &self,
        _merge_context: &mut Context,
        _purpose: &Option<RawEncodePurpose>,
    ) -> Result<usize, BuckyError> {
        unimplemented!()
    }

    fn raw_encode_with_context<'a>(
        &self,
        enc_buf: &'a mut [u8],
        merge_context: &mut Context,
        _purpose: &Option<RawEncodePurpose>,
    ) -> Result<&'a mut [u8], BuckyError> {
        let mut flags = context::FlagsCounter::new();
        let (mut context, buf) = context::Encode::<Self, Context>::new(enc_buf, merge_context)?;
        let buf = context.encode(buf, &self.protocol_version, context::FLAG_ALWAYS_ENCODE)?;
        let buf = context.encode(buf, &self.stack_version, context::FLAG_ALWAYS_ENCODE)?;
        let buf = context.check_encode(buf, "seq", &self.seq, flags.next())?;
        let buf = context.check_encode(buf, "to_device_id", &self.sn_peer_id, flags.next())?;
        let buf =
            context.check_option_encode(buf, "from_device_id", &self.from_peer_id, flags.next())?;
        let buf = context.check_option_encode(buf, "device_desc", &self.peer_info, flags.next())?;
        let buf = context.check_encode(buf, "send_time", &self.send_time, flags.next())?;
        let buf = context.option_encode(buf, &self.contract_id, flags.next())?;
        let _buf = context.option_encode(buf, &self.receipt, flags.next())?;
        context.finish(enc_buf)
    }
}

impl<'de, Context: merge_context::Decode> RawDecodeWithContext<'de, &mut Context> for SnPing {
    fn raw_decode_with_context(
        buf: &'de [u8],
        merge_context: &mut Context,
    ) -> Result<(Self, &'de [u8]), BuckyError> {
        let mut flags = context::FlagsCounter::new();
        let (mut context, buf) = context::Decode::new(buf, merge_context)?;
        let (protocol_version, buf) = context.decode(buf, "SnPing.protocol_version", context::FLAG_ALWAYS_DECODE)?;
        let (stack_version, buf) = context.decode(buf, "SnPing.stack_version", context::FLAG_ALWAYS_DECODE)?;
        let (seq, buf) = context.check_decode(buf, "seq", flags.next())?;
        let (sn_peer_id, buf) = context.check_decode(buf, "to_device_id", flags.next())?;
        let (from_peer_id, buf) =
            context.check_option_decode(buf, "from_device_id", flags.next())?;
        let (peer_info, buf) = context.check_option_decode(buf, "device_desc", flags.next())?;
        let (send_time, buf) = context.check_decode(buf, "send_time", flags.next())?;
        let (contract_id, buf) = context.option_decode(buf, flags.next())?;
        let (receipt, buf) = context.option_decode(buf, flags.next())?;

        Ok((
            Self {
                protocol_version,
                stack_version,
                seq,
                from_peer_id,
                sn_peer_id,
                peer_info,
                send_time,
                contract_id,
                receipt,
            },
            buf,
        ))
    }
}
