#[path = "real_p2p_tunnel_flow/collision_cross_sn.rs"]
mod collision_cross_sn;
#[path = "real_p2p_tunnel_flow/fallback.rs"]
mod fallback;
#[path = "real_p2p_tunnel_flow/fixture.rs"]
mod fixture;
// The matrix test drives the opt-in test-real-socket-matrix seam (loopback
// WAN/Mapped rendezvous eligibility). Without the feature it cannot reach the
// production rendezvous branch, so the module is compiled only with the seam.
#[cfg(feature = "test-real-socket-matrix")]
#[path = "real_p2p_tunnel_flow/strategy_matrix.rs"]
mod strategy_matrix;
