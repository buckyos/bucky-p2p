#[test]
fn owner_targets_preserve_all_ordered_endpoints() {
    use crate::endpoint::{Endpoint, Protocol};
    use crate::sn::directory::{OwnerMember, OwnerMembership};
    let owner = P2pId::from(vec![111; 32]);
    let empty = P2pId::from(vec![112; 32]);
    let endpoints = vec![
        Endpoint::from((Protocol::Quic, "127.0.0.1:25201".parse().unwrap())),
        Endpoint::from((Protocol::Tcp, "127.0.0.1:25202".parse().unwrap())),
    ];
    let membership = OwnerMembership::with_members(
        vec![OwnerMember::with_endpoints(owner.clone(), endpoints.clone(), Some("owner".into())), OwnerMember::new(empty.clone())],
        2, std::time::Duration::from_secs(60),
    ).unwrap();
    let targets = owner_targets(&membership);
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[&owner].remote_ep, endpoints);
    assert_eq!(targets[&owner].remote_name.as_deref(), Some("owner"));
    assert!(!targets.contains_key(&empty));
}
