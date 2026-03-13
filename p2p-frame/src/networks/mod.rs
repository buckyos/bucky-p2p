mod command;
mod listener;
mod net_manager;
mod network;
mod quic;
mod tcp;
mod tunnel;
mod validator;

use crate::error::{P2pErrorCode, P2pResult, p2p_err};
pub use crate::tunnel::{
    DefaultDeviceFinder, DeviceFinder, DeviceFinderRef, TunnelManager, TunnelManagerRef,
    TunnelSubscription,
};
pub use command::*;
use futures::FutureExt;
pub use listener::*;
pub use net_manager::*;
pub use network::*;
pub use quic::*;
use rustls::pki_types::ServerName;
use std::fmt::Debug;
use std::future::Future;
pub use tcp::*;
pub use tunnel::*;
pub use validator::*;

pub fn validate_server_name(server_name: String) -> String {
    match ServerName::try_from(server_name.as_str()) {
        Ok(_) => server_name,
        Err(_) => format!("p2p.{}.com", server_name),
    }
}

pub fn parse_server_name(server_name: &str) -> &str {
    if server_name.starts_with("p2p.") && server_name.ends_with(".com") {
        server_name
            .trim_start_matches("p2p.")
            .trim_end_matches(".com")
    } else {
        server_name
    }
}

pub async fn select_successful<T, E: Debug, F>(futures: Vec<F>) -> P2pResult<T>
where
    F: Future<Output = Result<T, E>> + Unpin,
{
    let mut futures = futures.into_iter().map(FutureExt::fuse).collect::<Vec<_>>();

    while futures.len() > 0 {
        let select_all = futures::future::select_all(futures);
        match select_all.await {
            (Ok(result), _index, _remaining) => {
                return Ok(result);
            }
            (Err(e), _index, remaining) => {
                log::trace!("select failed {:?}", e);
                futures = remaining;
            }
        }
    }
    Err(p2p_err!(P2pErrorCode::ConnectFailed, "connect failed"))
}
