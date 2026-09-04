use super::*;

#[test]
fn nat_probe_port_parser_preserves_disabled_default_and_rejects_invalid_sets() {
    assert!(parse_nat_probe_ports(None).unwrap().is_empty());
    assert!(
        parse_nat_probe_ports(Some(&String::new()))
            .unwrap()
            .is_empty()
    );
    assert!(parse_nat_probe_ports(Some(&"3000".to_owned())).is_err());
    assert!(parse_nat_probe_ports(Some(&"3000,3000".to_owned())).is_err());
    assert!(parse_nat_probe_ports(Some(&"0,3001".to_owned())).is_err());
    assert!(parse_nat_probe_ports(Some(&"bad,3001".to_owned())).is_err());
    assert_eq!(
        parse_nat_probe_ports(Some(&"3000, 3001,3002".to_owned())).unwrap(),
        vec![3000, 3001, 3002]
    );
}
