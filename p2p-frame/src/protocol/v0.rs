use super::sn::*;
use crate::endpoint::Endpoint;
use crate::error::P2pErrorCode;
use crate::p2p_identity::{EncodedP2pIdentityCert, P2pId};
use crate::types::{Sequence, SessionId, Timestamp, TunnelId};
use bucky_raw_codec::{RawDecode, RawEncode};

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

#[derive(Copy, Clone, Debug, RawEncode, RawDecode, Eq, PartialEq, Hash)]
pub enum TunnelType {
    Datagram,
    Stream,
}

#[derive(Clone, Debug, RawEncode, RawDecode)]
pub struct SynSession {
    pub tunnel_type: TunnelType,
    pub tunnel_id: TunnelId,
    pub to_vport: u16,
    pub session_id: SessionId,
    pub payload: Vec<u8>,
}

impl std::fmt::Display for SynSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "SynSession:{{session_id:{:?},to_vport:{}}}",
            self.session_id, self.to_vport
        )
    }
}

pub const TCP_ACK_CONNECTION_RESULT_OK: u8 = 0;
pub const TCP_ACK_CONNECTION_RESULT_REFUSED: u8 = 1;

#[derive(Clone, Debug, RawEncode, RawDecode)]
pub struct AckSession {
    pub result: u8,
}

#[derive(Clone, Debug, RawEncode, RawDecode)]
pub struct SynReverseSession {
    pub tunnel_type: TunnelType,
    pub tunnel_id: TunnelId,
    pub session_id: SessionId,
    pub vport: u16,
    pub result: u8,
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, RawEncode, RawDecode)]
pub struct AckReverseSession {
    pub result: u8,
}

#[derive(Clone, Debug, RawEncode, RawDecode)]
pub struct SynDatagram {
    pub tunnel_id: TunnelId,
    pub to_vport: u16,
    pub session_id: SessionId,
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, RawEncode, RawDecode)]
pub struct AckDatagram {
    pub result: u8,
}

#[derive(Clone, Debug, RawEncode, RawDecode)]
pub struct SynReverseDatagram {
    pub tunnel_id: TunnelId,
    pub to_vport: u16,
    pub session_id: SessionId,
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, RawEncode, RawDecode)]
pub struct AckReverseDatagram {
    pub result: u8,
}

#[derive(Clone, Debug, RawEncode, RawDecode)]
pub struct SynClose {
    pub reason: u8,
    pub tunnel_id: TunnelId,
}

#[derive(Clone, Debug, RawEncode, RawDecode)]
pub struct AckClose {
    pub tunnel_id: TunnelId,
}

#[derive(Debug, Clone, RawEncode, RawDecode)]
pub struct SnCallResp {
    //sn call的响应包
    pub seq: Sequence,                                //序列事情
    pub sn_peer_id: P2pId,                            //sn设备id
    pub result: u8,                                   //
    pub to_peer_info: Option<EncodedP2pIdentityCert>, //
}

#[derive(Clone, Debug, RawEncode, RawDecode)]
pub struct SnCalled {
    pub seq: Sequence,
    pub sn_peer_id: P2pId,
    pub to_peer_id: P2pId,
    pub reverse_endpoint_array: Vec<Endpoint>,
    pub active_pn_list: Vec<P2pId>,
    pub peer_info: EncodedP2pIdentityCert,
    pub tunnel_id: TunnelId,
    pub call_send_time: Timestamp,
    pub call_type: TunnelType,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, RawEncode, RawDecode)]
pub struct SnCalledResp {
    //sn called的应答报文
    pub seq: Sequence,     //序列号
    pub sn_peer_id: P2pId, //sn的设备id
    pub result: u8,        //
}

#[derive(Debug, Clone, RawEncode, RawDecode)]
pub struct SnPingResp {
    //SN Server收到来自device的SNPing包时，返回device的外网地址
    pub seq: Sequence,                             //包序列包
    pub sn_peer_id: P2pId,                         //sn的设备id
    pub result: u8,                                //是否接受device的接入
    pub peer_info: Option<EncodedP2pIdentityCert>, //sn的设备信息
    pub end_point_array: Vec<Endpoint>,            //外网地址列表
    pub receipt: Option<SnServiceReceipt>,         //返回sn的一些连接信息，如当前连接的peer数量
}

#[derive(Debug, Clone, RawEncode, RawDecode)]
pub struct AckProxy {
    pub seq: Sequence,
    pub to_peer_id: P2pId,
    pub proxy_endpoint: Option<Endpoint>,
    pub err: Option<P2pErrorCode>,
}
