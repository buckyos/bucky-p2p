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

#[test]
fn loaded_serving_identity_normalizes_configured_addresses_to_wildcards() {
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "sn-miner-wildcard-identity-{}-{suffix}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let base = dir.join("serving-sn");
    let private_key = PrivateKey::generate_rsa(1024).unwrap();
    let device = Device::new(
        None,
        UniqueId::default(),
        vec![
            Endpoint::from((
                Protocol::Udp,
                SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(198, 51, 100, 9), 3456)),
            )),
            Endpoint::from((
                Protocol::Udp,
                SocketAddr::V6(SocketAddrV6::new(
                    "2001:db8::9".parse().unwrap(),
                    3456,
                    0,
                    0,
                )),
            )),
        ],
        vec![],
        vec![],
        private_key.public(),
        Area::default(),
        DeviceCategory::Server,
    )
    .build();
    device
        .encode_to_file(base.with_extension("desc").as_path(), true)
        .unwrap();
    private_key
        .encode_to_file(base.with_extension("sec").as_path(), true)
        .unwrap();

    let (mut loaded, _) = load_device_info(&base).unwrap();
    let endpoints = loaded.mut_connect_info().mut_endpoints().clone();
    assert_eq!(endpoints.len(), 2);
    assert!(endpoints.iter().all(|endpoint| endpoint.addr().ip().is_unspecified()));

    std::fs::remove_dir_all(dir).unwrap();
}
