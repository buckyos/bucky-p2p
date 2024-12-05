use crate::endpoint::Endpoint;
use crate::error::P2pErrorCode;

#[derive(Clone, Debug)]
pub struct PingSessionResp {
    pub from: Endpoint,
    pub err: P2pErrorCode,
    pub endpoints: Vec<Endpoint>
}
