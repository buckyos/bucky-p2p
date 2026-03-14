use crate::endpoint::Endpoint;
use crate::networks::{TunnelDatagramRead, TunnelPurpose};
use crate::p2p_identity::P2pId;

#[derive(Clone, Debug)]
pub struct TtpTarget {
    pub local_ep: Option<Endpoint>,
    pub remote_ep: Endpoint,
    pub remote_id: P2pId,
    pub remote_name: Option<String>,
}

#[derive(Clone, Debug)]
pub struct TtpStreamMeta {
    pub local_ep: Option<Endpoint>,
    pub remote_ep: Option<Endpoint>,
    pub local_id: P2pId,
    pub remote_id: P2pId,
    pub remote_name: Option<String>,
    pub purpose: TunnelPurpose,
}

#[derive(Clone, Debug)]
pub struct TtpDatagramMeta {
    pub local_ep: Option<Endpoint>,
    pub remote_ep: Option<Endpoint>,
    pub local_id: P2pId,
    pub remote_id: P2pId,
    pub remote_name: Option<String>,
    pub purpose: TunnelPurpose,
}

pub struct TtpDatagram {
    pub meta: TtpDatagramMeta,
    pub read: TunnelDatagramRead,
}
