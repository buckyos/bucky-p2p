use crate::protocol::*;
use cyfs_base::Endpoint;
use cyfs_base::BuckyErrorCode;

#[derive(Clone, Debug)]
pub struct PingSessionResp {
    pub from: Endpoint,
    pub err: BuckyErrorCode,
    pub endpoints: Vec<Endpoint>
}
