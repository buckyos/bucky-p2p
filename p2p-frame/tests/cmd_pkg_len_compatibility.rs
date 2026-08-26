use bucky_raw_codec::RawFixedBytes;
use p2p_frame::sn::types::{OwnerCmdPkgLen, SnCmdHeader, SnCmdPkgLen};
use sfo_cmd_server::{CmdHeader, CmdPkgLen};

const OWNER_MAX_PKG_LEN: u64 = 10 * 1024 * 1024;

fn assert_cmd_pkg_len<T: CmdPkgLen>() {}

#[test]
fn shared_aliases_expose_the_expected_fixed_width_contracts() {
    assert_cmd_pkg_len::<SnCmdPkgLen>();
    assert_cmd_pkg_len::<OwnerCmdPkgLen>();

    assert_eq!(SnCmdPkgLen::raw_bytes(), Some(2));
    assert_eq!(
        <SnCmdPkgLen as CmdPkgLen>::MAX_PKG_LEN,
        u16::MAX as u64
    );
    assert_eq!(OwnerCmdPkgLen::raw_bytes(), Some(3));
    assert_eq!(
        <OwnerCmdPkgLen as CmdPkgLen>::MAX_PKG_LEN,
        OWNER_MAX_PKG_LEN
    );
}

#[test]
fn owner_alias_accepts_the_exact_limit_and_rejects_the_next_byte() {
    let exact_limit = OwnerCmdPkgLen::try_from(OWNER_MAX_PKG_LEN as u32)
        .expect("the configured 10 MiB package limit must be representable");

    assert_eq!(exact_limit.get(), OWNER_MAX_PKG_LEN as u32);
    assert!(OwnerCmdPkgLen::try_from(OWNER_MAX_PKG_LEN as u32 + 1).is_err());
}

#[test]
fn command_headers_preserve_values_with_both_shared_aliases() {
    let sn_len = SnCmdPkgLen::try_from(u16::MAX).expect("u16::MAX is the SN alias limit");
    let sn_header: SnCmdHeader = CmdHeader::new(1, false, Some(7), 0x11, sn_len);

    assert_eq!(sn_header.pkg_len().get(), u16::MAX);
    assert_eq!(sn_header.version(), 1);
    assert_eq!(sn_header.seq(), Some(7));
    assert!(!sn_header.is_resp());
    assert_eq!(sn_header.cmd_code(), 0x11);

    let owner_len = OwnerCmdPkgLen::try_from(OWNER_MAX_PKG_LEN as u32)
        .expect("the owner alias accepts its exact package limit");
    let owner_header: CmdHeader<OwnerCmdPkgLen, u8> =
        CmdHeader::new(2, true, Some(9), 0x22, owner_len);

    assert_eq!(owner_header.pkg_len().get(), OWNER_MAX_PKG_LEN as u32);
    assert_eq!(owner_header.version(), 2);
    assert_eq!(owner_header.seq(), Some(9));
    assert!(owner_header.is_resp());
    assert_eq!(owner_header.cmd_code(), 0x22);
}
