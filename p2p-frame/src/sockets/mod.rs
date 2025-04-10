pub mod tcp;
// pub mod udp;
// mod net_listener;
mod types;
mod net_manager;
mod quic;
mod net_util;
mod get_if_addrs;

use rustls::pki_types::ServerName;
// pub use net_listener::*;
pub use types::*;
pub use net_manager::*;
pub use quic::*;

pub fn validate_server_name(server_name: String) -> String {
    match ServerName::try_from(server_name.as_str()) {
        Ok(_) => server_name,
        Err(_) => format!("p2p.{}.com", server_name)
    }
}

pub fn parse_server_name(server_name: &str) -> &str {
    if server_name.starts_with("p2p.") && server_name.ends_with(".com"){
        server_name.trim_start_matches("p2p.").trim_end_matches(".com")
    } else {
        server_name
    }
}
