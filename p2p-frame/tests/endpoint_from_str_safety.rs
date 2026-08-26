use std::panic::catch_unwind;
use std::str::FromStr;

use p2p_frame::endpoint::{Endpoint, EndpointArea, Protocol};
use p2p_frame::error::P2pErrorCode;

fn assert_invalid_input_without_panic(input: &str) {
    let parsed = catch_unwind(|| Endpoint::from_str(input))
        .unwrap_or_else(|_| panic!("Endpoint::from_str panicked for {input:?}"));
    assert_eq!(
        parsed.unwrap_err().code(),
        P2pErrorCode::InvalidInput,
        "unexpected error code for {input:?}"
    );
}

#[test]
fn endpoint_from_str_unit_records_pre_fix_red_behavior() {
    fn removed_fixed_slices(input: &str) {
        let _ = &input[0..1];
        let _ = &input[2..5];
    }

    for input in ["", "W", "W4", "W4q", "W4qi", "中4qic127.0.0.1:1"] {
        assert!(
            catch_unwind(|| removed_fixed_slices(input)).is_err(),
            "pre-fix fixed slices unexpectedly accepted {input:?}"
        );
    }
}

#[test]
fn endpoint_from_str_unit_rejects_short_and_utf8_inputs_without_panicking() {
    for input in [
        "",
        "W",
        "W4",
        "W4q",
        "W4qi",
        "中4qic127.0.0.1:1",
        "W中qic127.0.0.1:1",
        "W4中127.0.0.1:1",
        "W4q中127.0.0.1:1",
        "W4qi中127.0.0.1:1",
        "W4qic中",
    ] {
        assert_invalid_input_without_panic(input);
    }
}

#[test]
fn endpoint_from_str_unit_preserves_validation_and_success_branches() {
    for input in [
        "D4qic127.0.0.1:1",
        "W7qic127.0.0.1:1",
        "W4zzz127.0.0.1:1",
        "W4eaa127.0.0.1:1",
        "W4e07127.0.0.1:1",
        "W4e16127.0.0.1:1",
        "W4qic",
        "W4qicnot-an-address",
        "W6qic127.0.0.1:1",
        "W4qic[::1]:1",
    ] {
        assert_invalid_input_without_panic(input);
    }

    let cases = [
        ("W4tcp127.0.0.1:1", EndpointArea::Wan, Protocol::Tcp),
        ("M4qic127.0.0.1:2", EndpointArea::Mapped, Protocol::Quic),
        ("L4udp127.0.0.1:3", EndpointArea::Lan, Protocol::Quic),
        (
            "S4e08127.0.0.1:4",
            EndpointArea::ServerReflexive,
            Protocol::Ext(8),
        ),
        ("W4e15127.0.0.1:5", EndpointArea::Wan, Protocol::Ext(15)),
        ("L6tcp[::1]:6", EndpointArea::Lan, Protocol::Tcp),
    ];

    for (input, area, protocol) in cases {
        let endpoint = Endpoint::from_str(input).unwrap();
        assert_eq!(endpoint.get_area(), area, "unexpected area for {input}");
        assert_eq!(endpoint.protocol(), protocol, "unexpected protocol for {input}");
    }

    assert_eq!(
        Endpoint::from_str("L4udp127.0.0.1:3").unwrap(),
        Endpoint::from_str("L4qic127.0.0.1:3").unwrap()
    );
}

#[test]
fn endpoint_from_str_integration_preserves_public_fallible_contract() {
    let endpoint = "S6qic[2001:db8::1]:4040".parse::<Endpoint>().unwrap();
    assert_eq!(endpoint.get_area(), EndpointArea::ServerReflexive);
    assert_eq!(endpoint.protocol(), Protocol::Quic);
    assert!(endpoint.addr().is_ipv6());

    assert_invalid_input_without_panic("界");
    assert_invalid_input_without_panic("S6qic界");
}
