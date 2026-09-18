//! Platform-aware local address selection for SN reporting.
//!
//! The client reports local addresses to SN so that peers can attempt direct
//! connections. Reporting every address an interface happens to carry wastes
//! the endpoint budget and, worse, advertises addresses the kernel already
//! deprecated or that never passed duplicate address detection. This module
//! turns kernel/adapter state into a platform-neutral [`AddressMeta`] and keeps
//! only addresses that are still usable, ordered so stable preferred addresses
//! come first.
//!
//! Nothing here mutates the kernel address table: the module only reads
//! interface metadata (`/proc/net/if_inet6` on Linux, `GetAdaptersAddresses` on
//! Windows) and never deletes or re-configures an address.

use std::net::IpAddr;

/// Upper bound on how many local addresses are handed to the report assembly.
pub(crate) const MAX_LOCAL_IP_COUNT: usize = 32;

/// Normalized duplicate-address-detection state of one local address.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DadState {
    Preferred,
    Deprecated,
    Tentative,
    /// Linux `IFA_F_DADFAILED` and Windows `NldsDuplicate`: DAD failed, so the
    /// address is not usable as a source.
    DadFailed,
    /// The platform could not provide DAD state, either because it has no such
    /// API or because the address was absent from the platform snapshot.
    Unknown,
}

/// Normalized interface-address origin used only for ranking.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PrefixSource {
    Manual,
    LinkLayer,
    Random,
    Other,
    Unknown,
}

/// Platform-neutral view of one candidate local address.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AddressMeta {
    pub(crate) addr: IpAddr,
    pub(crate) dad_state: DadState,
    pub(crate) temporary: bool,
    pub(crate) prefix_source: PrefixSource,
}

impl AddressMeta {
    /// Metadata for an address whose platform state is unknown. Unknown
    /// addresses stay reportable so platforms without a metadata API keep the
    /// existing behavior.
    pub(crate) fn unknown(addr: IpAddr) -> Self {
        Self {
            addr,
            dad_state: DadState::Unknown,
            temporary: false,
            prefix_source: PrefixSource::Unknown,
        }
    }
}

/// Whether an address is still usable as a reported source.
pub(crate) fn address_is_usable(meta: &AddressMeta) -> bool {
    matches!(meta.dad_state, DadState::Preferred | DadState::Unknown)
}

fn address_score(meta: &AddressMeta) -> i64 {
    let mut score = 0;
    if meta.dad_state == DadState::Preferred {
        score += 100;
    }
    if !meta.temporary {
        score += 30;
    }
    if meta.prefix_source != PrefixSource::Random {
        score += 20;
    }
    score
}

/// Select the addresses that should be reported.
///
/// Unusable addresses are dropped, the rest are ranked by stability, and the
/// result is truncated to [`MAX_LOCAL_IP_COUNT`]. `sort_by` is stable, so
/// entries with equal scores keep the enumeration order of the base provider;
/// that also keeps platforms whose metadata is entirely unknown in their
/// current order.
pub(crate) fn select_local_ips(metas: &[AddressMeta]) -> Vec<IpAddr> {
    let mut selected: Vec<(i64, usize, IpAddr)> = metas
        .iter()
        .enumerate()
        .filter(|(_, meta)| address_is_usable(meta))
        .map(|(index, meta)| (address_score(meta), index, meta.addr))
        .collect();
    selected.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    selected.truncate(MAX_LOCAL_IP_COUNT);
    selected.into_iter().map(|(_, _, addr)| addr).collect()
}

// Windows `NL_DAD_STATE` and `NL_SUFFIX_ORIGIN` discriminants (`nldef.h`).
// They are kept as plain numbers so the mapping stays testable on every host.
const NLDS_INVALID: u32 = 0;
const NLDS_TENTATIVE: u32 = 1;
const NLDS_DUPLICATE: u32 = 2;
const NLDS_DEPRECATED: u32 = 3;
const NLDS_PREFERRED: u32 = 4;

const NLSO_MANUAL: u32 = 1;
const NLSO_WELL_KNOWN: u32 = 2;
const NLSO_DHCP: u32 = 3;
const NLSO_LINK_LAYER: u32 = 4;
const NLSO_RANDOM: u32 = 5;

/// Map a Windows DAD state to the normalized state. `None` means the state is
/// invalid and the address must not be reported.
pub(crate) fn windows_dad_state(code: u32) -> Option<DadState> {
    match code {
        NLDS_INVALID => None,
        NLDS_TENTATIVE => Some(DadState::Tentative),
        NLDS_DUPLICATE => Some(DadState::DadFailed),
        NLDS_DEPRECATED => Some(DadState::Deprecated),
        NLDS_PREFERRED => Some(DadState::Preferred),
        _ => Some(DadState::Unknown),
    }
}

/// Map a Windows suffix origin to the normalized prefix source.
pub(crate) fn windows_prefix_source(code: u32) -> PrefixSource {
    match code {
        NLSO_MANUAL => PrefixSource::Manual,
        NLSO_LINK_LAYER => PrefixSource::LinkLayer,
        NLSO_RANDOM => PrefixSource::Random,
        NLSO_WELL_KNOWN | NLSO_DHCP => PrefixSource::Other,
        _ => PrefixSource::Unknown,
    }
}

/// Build the normalized metadata of one Windows unicast address.
///
/// Returns `None` when the address must not be reported: the owning adapter is
/// not up, or the platform reports an invalid DAD state.
pub(crate) fn windows_meta(
    addr: IpAddr,
    dad_state_code: u32,
    suffix_origin_code: u32,
    adapter_up: bool,
) -> Option<AddressMeta> {
    if !adapter_up {
        return None;
    }
    let dad_state = windows_dad_state(dad_state_code)?;
    let prefix_source = windows_prefix_source(suffix_origin_code);
    Some(AddressMeta {
        addr,
        dad_state,
        temporary: prefix_source == PrefixSource::Random,
        prefix_source,
    })
}

/// Linux `/proc/net/if_inet6` parsing and kernel flag decoding.
#[cfg(target_os = "linux")]
pub(crate) mod linux {
    use super::{AddressMeta, DadState, PrefixSource};
    use std::net::{IpAddr, Ipv6Addr};

    // Kernel `IFA_F_*` bits from `include/uapi/linux/if_addr.h`. The flags
    // column of `/proc/net/if_inet6` is the raw 32-bit `ifa_flags` value, so the
    // higher bits (`0x100` mngtmpaddr, `0x800` stable privacy) appear as three
    // hex digits and the field must be parsed as `u32`.
    pub(crate) const IFA_F_TEMPORARY: u32 = 0x01;
    pub(crate) const IFA_F_DADFAILED: u32 = 0x08;
    pub(crate) const IFA_F_DEPRECATED: u32 = 0x20;
    pub(crate) const IFA_F_TENTATIVE: u32 = 0x40;
    pub(crate) const IFA_F_STABLE_PRIVACY: u32 = 0x800;

    /// One parsed `/proc/net/if_inet6` row.
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub(crate) struct ProcNetIfInet6Entry {
        pub(crate) addr: IpAddr,
        pub(crate) ifname: String,
        pub(crate) prefix_len: u8,
        pub(crate) scope: u8,
        pub(crate) flags: u32,
    }

    /// Decode the DAD state from the kernel flag bits.
    pub(crate) fn dad_state_from_flags(flags: u32) -> DadState {
        if flags & IFA_F_DADFAILED != 0 {
            DadState::DadFailed
        } else if flags & IFA_F_TENTATIVE != 0 {
            DadState::Tentative
        } else if flags & IFA_F_DEPRECATED != 0 {
            DadState::Deprecated
        } else {
            DadState::Preferred
        }
    }

    /// Build normalized metadata from the kernel flag bits.
    pub(crate) fn meta_from_flags(addr: IpAddr, flags: u32) -> AddressMeta {
        AddressMeta {
            addr,
            dad_state: dad_state_from_flags(flags),
            temporary: flags & IFA_F_TEMPORARY != 0,
            prefix_source: if flags & IFA_F_STABLE_PRIVACY != 0 {
                PrefixSource::Random
            } else {
                PrefixSource::Unknown
            },
        }
    }

    /// Parse the text form of `/proc/net/if_inet6`.
    ///
    /// Rows that do not match the six-column layout or carry non-hex fields are
    /// skipped instead of failing the whole snapshot, so a future kernel change
    /// degrades to "unknown metadata" rather than to "no addresses".
    pub(crate) fn parse_proc_net_if_inet6(text: &str) -> Vec<ProcNetIfInet6Entry> {
        let mut entries = Vec::new();
        for line in text.lines() {
            let mut fields = line.split_whitespace();
            let (Some(address), Some(_ifindex), Some(prefix_len), Some(scope), Some(flags), Some(ifname)) = (
                fields.next(),
                fields.next(),
                fields.next(),
                fields.next(),
                fields.next(),
                fields.next(),
            ) else {
                continue;
            };
            let (Ok(address), Ok(prefix_len), Ok(scope), Ok(flags)) = (
                u128::from_str_radix(address, 16),
                u8::from_str_radix(prefix_len, 16),
                u8::from_str_radix(scope, 16),
                u32::from_str_radix(flags, 16),
            ) else {
                continue;
            };
            entries.push(ProcNetIfInet6Entry {
                addr: IpAddr::V6(Ipv6Addr::from(address)),
                ifname: ifname.to_string(),
                prefix_len,
                scope,
                flags,
            });
        }
        entries
    }

    /// Find the snapshot row of one enumerated address.
    ///
    /// The interface name is matched first because the base provider already
    /// filtered by name; an address-only match is accepted as a fallback so a
    /// name skew degrades to "known flags" instead of "unknown metadata".
    pub(crate) fn find_entry<'a>(
        entries: &'a [ProcNetIfInet6Entry],
        ifname: &str,
        addr: IpAddr,
    ) -> Option<&'a ProcNetIfInet6Entry> {
        entries
            .iter()
            .find(|entry| entry.addr == addr && entry.ifname == ifname)
            .or_else(|| entries.iter().find(|entry| entry.addr == addr))
    }
}

/// Attach platform metadata to the addresses kept by the base provider.
///
/// The returned metadata list follows the input order and only omits an address
/// when the platform explicitly says it must not be reported.
#[cfg(target_os = "linux")]
pub(crate) fn platform_annotate(entries: &[(String, IpAddr)]) -> Vec<AddressMeta> {
    let snapshot = std::fs::read_to_string("/proc/net/if_inet6")
        .ok()
        .map(|text| linux::parse_proc_net_if_inet6(&text))
        .unwrap_or_default();
    entries
        .iter()
        .map(|(ifname, addr)| match linux::find_entry(&snapshot, ifname, *addr) {
            Some(entry) => linux::meta_from_flags(*addr, entry.flags),
            None => AddressMeta::unknown(*addr),
        })
        .collect()
}

/// Attach platform metadata to the addresses kept by the base provider.
///
/// The returned metadata list follows the input order and only omits an address
/// when the platform explicitly says it must not be reported.
#[cfg(target_os = "windows")]
pub(crate) fn platform_annotate(entries: &[(String, IpAddr)]) -> Vec<AddressMeta> {
    let snapshot = windows::adapter_metadata();
    entries
        .iter()
        .filter_map(|(_, addr)| match snapshot.iter().find(|(candidate, _)| candidate == addr) {
            Some((_, Some(meta))) => Some(meta.clone()),
            Some((_, None)) => None,
            None => Some(AddressMeta::unknown(*addr)),
        })
        .collect()
}

/// Attach platform metadata to the addresses kept by the base provider.
///
/// Platforms without a duplicate-address-detection API keep every address the
/// base provider enumerated and keep its order, so their reported set is
/// unchanged. The metadata gap is documented in `docs/modules/p2p-frame.md`.
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub(crate) fn platform_annotate(entries: &[(String, IpAddr)]) -> Vec<AddressMeta> {
    entries
        .iter()
        .map(|(_, addr)| AddressMeta::unknown(*addr))
        .collect()
}

/// Windows adapter snapshot built from `GetAdaptersAddresses`.
#[cfg(target_os = "windows")]
mod windows {
    use super::{windows_meta, AddressMeta};
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
    use winapi::shared::ifdef::IfOperStatusUp;
    use winapi::shared::minwindef::ULONG;
    use winapi::shared::winerror::{ERROR_BUFFER_OVERFLOW, NO_ERROR};
    use winapi::shared::ws2def::{AF_INET, AF_INET6, AF_UNSPEC, SOCKADDR, SOCKADDR_IN};
    use winapi::shared::ws2ipdef::SOCKADDR_IN6;
    use winapi::um::iphlpapi::GetAdaptersAddresses;
    use winapi::um::iptypes::{
        GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_MULTICAST,
        IP_ADAPTER_ADDRESSES_LH,
    };

    const INITIAL_BUFFER_SIZE: ULONG = 16 * 1024;
    const MAX_BUFFER_SIZE: ULONG = 1024 * 1024;

    /// Collect one entry per unicast address, `None` when the address must be
    /// dropped (adapter down or invalid DAD state).
    pub(super) fn adapter_metadata() -> Vec<(IpAddr, Option<AddressMeta>)> {
        let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
        let mut size = INITIAL_BUFFER_SIZE;
        loop {
            let mut buffer = vec![0u8; size as usize];
            let result = unsafe {
                GetAdaptersAddresses(
                    AF_UNSPEC as ULONG,
                    flags,
                    std::ptr::null_mut(),
                    buffer.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH,
                    &mut size,
                )
            };
            if result == ERROR_BUFFER_OVERFLOW {
                if size > MAX_BUFFER_SIZE {
                    return Vec::new();
                }
                continue;
            }
            if result != NO_ERROR {
                return Vec::new();
            }
            return unsafe { collect(&buffer) };
        }
    }

    unsafe fn collect(buffer: &[u8]) -> Vec<(IpAddr, Option<AddressMeta>)> {
        let mut result = Vec::new();
        let mut adapter = buffer.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
        while !adapter.is_null() {
            let adapter_ref = unsafe { &*adapter };
            let adapter_up = adapter_ref.OperStatus == IfOperStatusUp;
            let mut unicast = adapter_ref.FirstUnicastAddress;
            while !unicast.is_null() {
                let entry = unsafe { &*unicast };
                let length = entry.Address.iSockaddrLength;
                if let Some(addr) = unsafe { sockaddr_ip(entry.Address.lpSockaddr, length) } {
                    let meta = windows_meta(addr, entry.DadState, entry.SuffixOrigin, adapter_up);
                    result.push((addr, meta));
                }
                unicast = entry.Next;
            }
            adapter = adapter_ref.Next;
        }
        result
    }

    unsafe fn sockaddr_ip(sockaddr: *const SOCKADDR, length: i32) -> Option<IpAddr> {
        if sockaddr.is_null() || length <= 0 {
            return None;
        }
        let family = unsafe { (*sockaddr).sa_family };
        if family == AF_INET6 as u16 && length as usize >= std::mem::size_of::<SOCKADDR_IN6>() {
            let v6 = unsafe { &*(sockaddr as *const SOCKADDR_IN6) };
            let octets = unsafe { *v6.sin6_addr.u.Byte() };
            return Some(IpAddr::V6(Ipv6Addr::from(octets)));
        }
        if family == AF_INET as u16 && length as usize >= std::mem::size_of::<SOCKADDR_IN>() {
            let v4 = unsafe { &*(sockaddr as *const SOCKADDR_IN) };
            let octets = unsafe { *v4.sin_addr.S_un.S_addr() }.to_ne_bytes();
            return Some(IpAddr::V4(Ipv4Addr::from(octets)));
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv6Addr};

    fn v6(text: &str) -> IpAddr {
        IpAddr::V6(text.parse::<Ipv6Addr>().unwrap())
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn parse_proc_net_if_inet6_reads_kernel_rows() {
        // Rows copied from `company-wgr` (/proc/net/if_inet6) with `ip -6 addr`
        // confirmation: 0xa2 is `PERMANENT|DEPRECATED|NODAD`, 0x82 is
        // `PERMANENT|NODAD`.
        let text = "\
240e03bc308f24c0c7f743da01d1d30a 03 40 00 a2     eth1
240e03bc308f24c00000000000000077 03 80 00 82     eth1
00000000000000000000000000000001 01 80 10 80       lo
garbage
";
        let entries = linux::parse_proc_net_if_inet6(text);
        assert_eq!(entries.len(), 3);
        assert_eq!(
            entries[0],
            linux::ProcNetIfInet6Entry {
                addr: v6("240e:3bc:308f:24c0:c7f7:43da:1d1:d30a"),
                ifname: "eth1".to_string(),
                prefix_len: 0x40,
                scope: 0x00,
                flags: 0xa2,
            }
        );
        assert_eq!(linux::dad_state_from_flags(entries[0].flags), DadState::Deprecated);
        assert_eq!(linux::dad_state_from_flags(entries[1].flags), DadState::Preferred);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_flags_map_to_dad_states_and_prefix_source() {
        assert_eq!(linux::dad_state_from_flags(0x20), DadState::Deprecated);
        assert_eq!(linux::dad_state_from_flags(0x40), DadState::Tentative);
        assert_eq!(linux::dad_state_from_flags(0x08), DadState::DadFailed);
        assert_eq!(linux::dad_state_from_flags(0x82), DadState::Preferred);
        let stable = linux::meta_from_flags(v6("240e:3bc:308f:24c0::77"), 0x800);
        assert_eq!(stable.prefix_source, PrefixSource::Random);
        let temporary = linux::meta_from_flags(v6("240e:3bc:308f:24c0::78"), 0x01);
        assert!(temporary.temporary);
    }

    #[test]
    fn windows_metadata_filters_failed_and_down_adapters() {
        let addr = v6("240e:3bc:308f:24c0::77");
        assert_eq!(windows_meta(addr, 4, 1, true).unwrap().dad_state, DadState::Preferred);
        assert_eq!(windows_meta(addr, 2, 1, true).unwrap().dad_state, DadState::DadFailed);
        assert_eq!(windows_meta(addr, 3, 1, true).unwrap().dad_state, DadState::Deprecated);
        assert_eq!(windows_meta(addr, 1, 1, true).unwrap().dad_state, DadState::Tentative);
        assert!(windows_meta(addr, 0, 1, true).is_none());
        assert!(windows_meta(addr, 4, 1, false).is_none());
        let random = windows_meta(addr, 4, 5, true).unwrap();
        assert!(random.temporary);
        assert_eq!(random.prefix_source, PrefixSource::Random);
    }

    #[test]
    fn select_local_ips_keeps_subset_and_prefers_stable_addresses() {
        let deprecated = v6("240e:3bc:308f:24c0:c7f7:43da:1d1:d30a");
        let preferred = v6("240e:3bc:308f:24c0::77");
        let unknown = v6("fdc8:b144:c39b::77");
        let metas = vec![
            linux_like(deprecated, DadState::Deprecated, false),
            AddressMeta::unknown(unknown),
            linux_like(preferred, DadState::Preferred, false),
        ];
        let selected = select_local_ips(&metas);
        assert_eq!(selected, vec![preferred, unknown]);
        assert!(!selected.contains(&deprecated));
    }

    #[test]
    fn select_local_ips_keeps_unknown_metadata_in_input_order() {
        let first = v6("240e:3bc:308f:24c0::77");
        let second = v6("fdc8:b144:c39b::77");
        let selected = select_local_ips(&[AddressMeta::unknown(first), AddressMeta::unknown(second)]);
        assert_eq!(selected, vec![first, second]);
    }

    #[test]
    fn select_local_ips_keeps_empty_input_empty() {
        assert!(select_local_ips(&[]).is_empty());
    }

    #[test]
    fn select_local_ips_drops_every_unusable_state() {
        let addr = v6("240e:3bc:308f:24c0::77");
        for dad_state in [
            DadState::Deprecated,
            DadState::Tentative,
            DadState::DadFailed,
        ] {
            let selected = select_local_ips(&[linux_like(addr, dad_state, false)]);
            assert!(selected.is_empty(), "{dad_state:?} must not be reported");
        }
    }

    #[test]
    fn select_local_ips_prefers_stable_over_temporary_random() {
        let temporary_random = v6("240e:3bc:308f:24c0:1111:2222:3333:4444");
        let stable = v6("240e:3bc:308f:24c0::77");
        let metas = vec![
            AddressMeta {
                addr: temporary_random,
                dad_state: DadState::Preferred,
                temporary: true,
                prefix_source: PrefixSource::Random,
            },
            linux_like(stable, DadState::Preferred, false),
        ];
        assert_eq!(select_local_ips(&metas), vec![stable, temporary_random]);
    }

    #[test]
    fn select_local_ips_caps_at_max_local_ip_count() {
        let metas: Vec<AddressMeta> = (1..=(MAX_LOCAL_IP_COUNT + 5))
            .map(|index| AddressMeta::unknown(v6(&format!("240e:3bc:308f:24c0::{index:x}"))))
            .collect();
        let selected = select_local_ips(&metas);
        let expected: Vec<IpAddr> = (1..=MAX_LOCAL_IP_COUNT)
            .map(|index| v6(&format!("240e:3bc:308f:24c0::{index:x}")))
            .collect();
        assert_eq!(selected.len(), MAX_LOCAL_IP_COUNT);
        assert_eq!(selected, expected);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn parse_proc_net_if_inet6_skips_malformed_rows_and_reads_wide_flags() {
        let text = "\
240e03bc308f24c0c7f743da01d1d30a 03 40 00 a2     eth1
short line
240e03bc308f24c00000000000000077 03 80 00 zz     eth1
240e03bc308f24c0103105a33ece0306 03 80 00 100     eth1
";
        let entries = linux::parse_proc_net_if_inet6(text);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].flags, 0xa2);
        assert_eq!(entries[1].flags, 0x100);
        // 0x100 is mngtmpaddr: a three digit flag value must not break parsing
        // and must not be mistaken for deprecation.
        assert_eq!(linux::dad_state_from_flags(entries[1].flags), DadState::Preferred);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn live_host_snapshot_rows_filter_deprecated_and_keep_preferred() {
        // Rows captured on `company-wgr`; `ip -6 addr show eth1` confirmed the
        // 0xa2 rows as `deprecated, preferred_lft 0sec`.
        let text = "\
240e03bc308f24c0c7f743da01d1d30a 03 40 00 a2     eth1
240e03bc308f24c00000000000000077 03 80 00 82     eth1
fdc8b144c39b0000d27ce2e8997f1138 03 40 00 a2     eth1
fdc8b144c39b00000000000000000077 03 80 00 82     eth1
";
        let metas: Vec<AddressMeta> = linux::parse_proc_net_if_inet6(text)
            .iter()
            .map(|entry| linux::meta_from_flags(entry.addr, entry.flags))
            .collect();
        assert_eq!(
            select_local_ips(&metas),
            vec![
                v6("240e:3bc:308f:24c0::77"),
                v6("fdc8:b144:c39b::77")
            ]
        );
    }

    fn linux_like(addr: IpAddr, dad_state: DadState, temporary: bool) -> AddressMeta {
        AddressMeta {
            addr,
            dad_state,
            temporary,
            prefix_source: PrefixSource::Unknown,
        }
    }
}
