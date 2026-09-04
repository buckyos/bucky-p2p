use super::*;

#[test]
fn probe_configuration_accepts_disabled_or_multiple_unique_ports_only() {
    assert!(validate_nat_probe_config(&[], None).is_ok());
    assert_eq!(
        validate_nat_probe_config(&[3000], Some(Ipv4Addr::new(198, 51, 100, 1)))
            .unwrap_err()
            .code(),
        P2pErrorCode::InvalidParam
    );
    assert_eq!(
        validate_nat_probe_config(&[3000, 3000], Some(Ipv4Addr::new(198, 51, 100, 1)))
            .unwrap_err()
            .code(),
        P2pErrorCode::InvalidParam
    );
    assert_eq!(
        validate_nat_probe_config(&[3000, 3001], None)
            .unwrap_err()
            .code(),
        P2pErrorCode::InvalidParam
    );
    assert!(
        validate_nat_probe_config(&[3000, 3001, 3002], Some(Ipv4Addr::new(198, 51, 100, 1)))
            .is_ok()
    );
}
