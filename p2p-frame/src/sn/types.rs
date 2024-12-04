use crate::endpoint::Endpoint;
use crate::error::BdtErrorCode;

#[derive(Clone, Debug)]
pub struct PingSessionResp {
    pub from: Endpoint,
    pub err: BdtErrorCode,
    pub endpoints: Vec<Endpoint>
}
