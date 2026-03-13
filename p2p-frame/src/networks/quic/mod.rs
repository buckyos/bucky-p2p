mod listener;
mod network;
mod tunnel;

pub use network::*;

#[derive(Debug, Eq, PartialEq, Clone, Copy)]
pub enum QuicCongestionAlgorithm {
    Bbr,
    Cubic,
    NewReno,
}
