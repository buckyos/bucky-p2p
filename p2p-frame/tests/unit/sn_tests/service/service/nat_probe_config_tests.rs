use super::*;

#[test]
fn probe_configuration_accepts_disabled_or_multiple_unique_ports_only() {
    assert!(validate_nat_probe_config(&[]).is_ok());
    assert_eq!(
        validate_nat_probe_config(&[3000])
            .unwrap_err()
            .code(),
        P2pErrorCode::InvalidParam
    );
    assert_eq!(
        validate_nat_probe_config(&[3000, 3000])
            .unwrap_err()
            .code(),
        P2pErrorCode::InvalidParam
    );
    assert_eq!(
        validate_nat_probe_config(&[0, 3001])
            .unwrap_err()
            .code(),
        P2pErrorCode::InvalidParam
    );
    assert!(validate_nat_probe_config(&[3000, 3001, 3002]).is_ok());
}
