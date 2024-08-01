use bucky_crypto::PrivateKey;
use bucky_error::{BuckyError, BuckyErrorCode, BuckyResult};
use bucky_objects::{Area, Device, DeviceCategory, DeviceId, Endpoint, NamedObject, UniqueId};
use bucky_raw_codec::{RawConvertTo, RawDecode, RawDecodeWithContext, RawEncode, RawEncodePurpose, RawEncodeWithContext, RawFixedBytes, SizedOwnedData, SizeU16, TailedOwnedData};
use bucky_time::bucky_time_now;
use crate::error::{BdtErrorCode, BdtResult, into_bdt_err};
use super::common::*;
use super::sn::*;


// #[test]
// fn encode_protocol_session_data() {
//     use crate::interface::udp;

//     let data = "hello".as_bytes().to_vec();
//     let mut stream_ranges = Vec::new();
//     stream_ranges.push(StreamRange{
//         pos: rand::random::<u64>(),
//         length: rand::random::<u32>(),
//     });
//     stream_ranges.push(StreamRange{
//         pos: rand::random::<u64>(),
//         length: rand::random::<u32>(),
//     });

//     let src = SessionData {
//         stream_pos: rand::random::<u64>(),
//         ack_stream_pos: rand::random::<u64>(),
//         sack: None,
//         session_id: IncreaseId::default(),
//         send_time: bucky_time_now(),
//         syn_info: Some(SessionSynInfo{
//             sequence: TempSeq::from(rand::random::<u32>()),
//             from_session_id: IncreaseId::default(),
//             to_vport: rand::random::<u16>()
//         }),
//         to_session_id: Some(IncreaseId::default()),
//         id_part: Some(SessionDataPackageIdPart{
//             package_id: IncreaseId::default(),
//             total_recv: rand::random::<u64>()
//         }),
//         payload: TailedOwnedData::from(data),
//         flags: 0,
//     };

//     let mut buf = [0u8; udp::MTU];
//     let remain = src.raw_encode_with_context(
//         &mut buf,
//         &mut merge_context::OtherEncode::default(),
//         &None).unwrap();
//     let remain = remain.len();

//     let dec = &buf[..buf.len() - remain];
//     let (cmd, dec) = u8::raw_decode(dec).map(|(code, dec)| (PackageCmdCode::try_from(code).unwrap(), dec)).unwrap();
//     assert_eq!(cmd, PackageCmdCode::SessionData);
//     let (dst, _) = SessionData::raw_decode_with_context(&dec, &mut merge_context::OtherDecode::default()).unwrap();

//     assert_eq!(dst.stream_pos, src.stream_pos);
//     assert_eq!(dst.ack_stream_pos, src.ack_stream_pos);

//     let dst_sack = dst.sack.unwrap().to_hex().unwrap();
//     let src_sack = src.sack.unwrap().to_hex().unwrap();
//     assert_eq!(dst_sack, src_sack);

//     assert_eq!(dst.session_id, src.session_id);
//     assert_eq!(dst.send_time, src.send_time);

//     let dst_syn_info = dst.syn_info.unwrap().to_hex().unwrap();
//     let src_syn_info = src.syn_info.unwrap().to_hex().unwrap();
//     assert_eq!(dst_syn_info, src_syn_info);

//     assert_eq!(dst.to_session_id, src.to_session_id);

//     let dst_id_part = dst.id_part.unwrap().to_hex().unwrap();
//     let src_id_part = src.id_part.unwrap().to_hex().unwrap();
//     assert_eq!(dst_id_part, src_id_part);

//     let src_payload = src.payload.to_string();
//     let dst_payload = dst.payload.to_string();
//     assert_eq!(dst_payload, src_payload);

//     assert_eq!(dst.flags, src.flags);
// }

#[derive(Clone, Debug, RawEncode, RawDecode)]
pub struct SynStream {
    pub sequence: TempSeq,
    pub to_vport: u16,
    pub session_id: IncreaseId,
    pub payload: Vec<u8>,
}

impl std::fmt::Display for SynStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "TcpSynConnection:{{session_id:{:?},to_vport:{}}}",
            self.session_id, self.to_vport
        )
    }
}

pub const TCP_ACK_CONNECTION_RESULT_OK: u8 = 0;
pub const TCP_ACK_CONNECTION_RESULT_REFUSED: u8 = 1;

#[derive(Clone, Debug, RawEncode, RawDecode)]
pub struct AckStream {
    pub result: u8,
}

#[derive(Clone, Debug, RawEncode, RawDecode)]
pub struct SynReverseStream {
    pub sequence: TempSeq,
    pub session_id: IncreaseId,
    pub vport: u16,
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, RawEncode, RawDecode)]
pub struct AckReverseStream {
    pub result: u8,
}

#[derive(Clone, Debug, RawEncode, RawDecode)]
pub struct SynDatagram {
    pub sequence: TempSeq,
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, RawEncode, RawDecode)]
pub struct AckDatagram {
    pub result: u8,
}

#[derive(Clone, Debug, RawEncode, RawDecode)]
pub struct SynReverseDatagram {
    pub sequence: TempSeq,
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, RawEncode, RawDecode)]
pub struct AckReverseDatagram {
    pub result: u8,
}

#[derive(Clone, Debug, RawEncode, RawDecode)]
pub struct SynClose {
    pub sequence: TempSeq,
}

#[derive(Clone, Debug, RawEncode, RawDecode)]
pub struct AckClose {
    pub sequence: TempSeq
}

#[derive(Debug, Clone, RawEncode, RawDecode)]
pub struct SnCallResp {
    //sn call的响应包
    pub seq: TempSeq,                 //序列事情
    pub sn_peer_id: DeviceId,         //sn设备id
    pub result: u8,                   //
    pub to_peer_info: Option<Device>, //
}

#[derive(Clone, Debug, RawEncode, RawDecode)]
pub struct SnCalled {
    pub seq: TempSeq,
    pub sn_peer_id: DeviceId,
    pub to_peer_id: DeviceId,
    pub reverse_endpoint_array: Vec<Endpoint>,
    pub active_pn_list: Vec<DeviceId>,
    pub peer_info: Device,
    pub tunnel_id: TempSeq,
    pub call_send_time: Timestamp,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, RawEncode, RawDecode)]
pub struct SnCalledResp {
    //sn called的应答报文
    pub seq: TempSeq,         //序列号
    pub sn_peer_id: DeviceId, //sn的设备id
    pub result: u8,           //
}


#[derive(Debug, Clone, RawEncode, RawDecode)]
pub struct SnPingResp {
    //SN Server收到来自device的SNPing包时，返回device的外网地址
    pub seq: TempSeq,                      //包序列包
    pub sn_peer_id: DeviceId,              //sn的设备id
    pub result: u8,                        //是否接受device的接入
    pub peer_info: Option<Device>,         //sn的设备信息
    pub end_point_array: Vec<Endpoint>,    //外网地址列表
    pub receipt: Option<SnServiceReceipt>, //返回sn的一些连接信息，如当前连接的peer数量
}


#[derive(Debug, Clone, RawEncode, RawDecode)]
pub struct AckProxy {
    pub seq: TempSeq,
    pub to_peer_id: DeviceId,
    pub proxy_endpoint: Option<Endpoint>,
    pub err: Option<BdtErrorCode>,
}
