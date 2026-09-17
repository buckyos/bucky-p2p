# Filter virtual NIC addresses and cap local IP/report payload count to 32

- Status: complete
- Owner module: p2p-frame
- Task manifest: docs/versions/v0.1/modules/p2p-frame/078-filter-virtual-nic-local-ips/task.yaml
- Approved proposal: docs/versions/v0.1/modules/p2p-frame/078-filter-virtual-nic-local-ips/proposal.md
- Affected paths:
  - p2p-frame/src/sn/client/sn_service.rs
- Explicit tier override: none
- Expanded high-risk packet: none

## Approach

`DefaultSnLocalIpProvider::get_local_ips` collects non-loopback local IPs for SN
reporting. Two changes in `p2p-frame/src/sn/client/sn_service.rs`:

1. Extend `should_ignore_interface` with more virtual/tunnel/VPN adapter name
   patterns (`TAP`/`tap`, `virbr`, `vmnet`/`VMnet`/`vboxnet`, `dummy`, `wg`,
   `tailscale`, `hamachi`, plus system tunnel adapters such as `sit`/`ip6tnl`/
   `teredo`/`isatap`/`gif`/`stf`/`awdl`, virtual bridges `bridge`/`br-`, and
   netfilter `nflog`).
2. Cap the reported IP data at `MAX_LOCAL_IP_COUNT = 32`: the filtered
   `get_local_ips` result is truncated to 32, and `report_on_send` additionally
   caps the actual report payload — the original per-listener endpoint
   expansion is kept at its existing inline position in `report_on_send` and a
   single `local_eps.truncate(MAX_LOCAL_IP_COUNT)` is appended, per the user's
   instruction not to extract a separate helper function. This keeps
   `ReportSn.local_eps` bounded even with multiple unspecified listeners (e.g.
   TCP + QUIC expanding 2×32 endpoints), while SN reporting still works.

The filtering pipeline is extracted into a pure `filter_local_ips(&[Interface])`
helper so unit tests can exercise virtual-NIC filtering, physical-NIC
retention, and the 32-IP cap without touching real network interfaces.
`report_on_send` keeps its original inline expansion and adds only the
`truncate` cap. Name-based matching (confirmed scope) is kept; no Linux netlink
`IFLA_INFO_KIND` or new dependency.

## Risk Screen

- Public contract, protocol, or CLI change: no
- Persistent data, schema, or migration change: no
- Security, privacy, or trust-boundary change: no
- Concurrency, lifecycle, or runtime integration change: no
- Material dependency/build graph, supply-chain trust, produced artifact, production default/feature rollout, release/deployment, compatibility, or rollback impact: no
- Material UI, accessibility, localization, or navigation workflow change: no
- Harness rule, checker, or test-infrastructure change: no
- Cross-project or architectural boundary change: no

## Verification

- Targeted check: `cargo test -p p2p-frame --features x509 --lib` (clean detached worktree with one-off include-base workaround for the pre-existing tunnel_manager include issue; 526 passed) and `cargo check --workspace` (clean)
- Result: pass
- Residual risk or follow-up: name matching may misjudge an unknown/odd-named physical NIC; covered by unit tests asserting common physical NIC names are retained. The report-payload cap keeps the first 32 expanded endpoints in enumeration order and may drop later protocols' endpoints when multiple unspecified listeners coexist; prioritized endpoint selection is a separate requirement.
