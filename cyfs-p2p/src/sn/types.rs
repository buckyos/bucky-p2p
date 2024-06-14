use bucky_error::BuckyErrorCode;
use bucky_objects::Endpoint;
use crate::protocol::*;

#[derive(Clone, Debug)]
pub struct PingSessionResp {
    pub from: Endpoint,
    pub err: BuckyErrorCode,
    pub endpoints: Vec<Endpoint>
}
