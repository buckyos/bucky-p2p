# 078-filter-virtual-nic-local-ips Completion Report

## Object and Scope

- Task manifest: task.yaml
- Workflow tier: standard
- Proposal: proposal.md (status approved, final tier standard)
- Change record: docs/changes/078-filter-virtual-nic-local-ips.md
- Delivery scope: `p2p-frame` client-side `DefaultSnLocalIpProvider` local-IP collection and `report_on_send` report-payload bounding; no wire/protocol/CLI/dependency change.

## Delivery Summary

- Outcome: (1) `DefaultSnLocalIpProvider::get_local_ips` filters out virtual/tunnel/container/VPN adapter addresses via an extended `should_ignore_interface` name pattern set (`TAP`/`tap`, `virbr`, `vmnet`/`VMnet`/`vboxnet`, `dummy`, `wg`-prefixed, `tailscale`, `hamachi`, plus system tunnel adapters `sit`/`ip6tnl`/`teredo`/`isatap`/`6to4`/`gif`/`stf`/`awdl`/`ip6ip`, PPP, virtual bridges, and netfilter `nflog`); (2) both the filtered `get_local_ips` result and the actual report payload are capped at `MAX_LOCAL_IP_COUNT = 32` — per the user's 2026-09-17 instruction, `report_on_send` keeps its original inline per-listener expansion and adds a single `local_eps.truncate(MAX_LOCAL_IP_COUNT)` after assembly, so `ReportSn.local_eps` never exceeds 32 even with multiple unspecified listeners (e.g. TCP+QUIC 2×32 expansion). The filtering pipeline is a pure `filter_local_ips(&[Interface])` helper backed by four unit tests; the inline payload cap is reviewed in place.
- Handoff: keep name-based filtering (confirmed scope, no Linux `IFLA_INFO_KIND`); if a future need for prioritization (e.g. prefer IPv4/physical) appears, it is a separate requirement.

## Proposal Consistency

| change_id | requirement_or_boundary | proposal_source | delivery_evidence | finding | status |
|-----------|-------------------------|-----------------|-------------------|---------|--------|
| CHG-local-ip-virtual-filter | Extend `should_ignore_interface` to filter more common virtual/tunnel/container/VPN adapter names without dropping physical NICs | proposal.md P-001 | `DefaultSnLocalIpProvider::should_ignore_interface` extended with the confirmed pattern set; `filter_local_ips_drops_virtual_tunnel_and_loopback` (18 constructed interfaces: 16 virtual/loopback dropped, `eth0`/`enp0s3` kept) and `filter_local_ips_keeps_physical_nics` (`eth0`/`enp0s3`/`wlan0`/`en0` all kept) | matches | pass |
| CHG-local-ip-max-32-cap | `get_local_ips` returns no more than 32 non-loopback, non-virtual IPs, and `report_on_send`'s reported IP data (`local_eps`) is no more than 32; both use the same `MAX_LOCAL_IP_COUNT = 32` constant, caps applied in existing enumeration order | proposal.md P-002 (2026-09-17 revision; implementation kept at original inline position per user instruction) | `filter_local_ips` applies `.take(MAX_LOCAL_IP_COUNT)` after the loopback/virtual filter; `report_on_send` retains the original inline expansion loop and appends `local_eps.truncate(MAX_LOCAL_IP_COUNT)`; `filter_local_ips_caps_count_at_max` (64 => 32) and `filter_local_ips_keeps_first_max_ordered_ips` (40 => first 32) pass; full lib suite (526) and workspace check pass | matches | pass |

## Independent Defect Discovery

| category | evidence_inspected | adversarial_check | finding_or_not_applicable_reason | status |
|----------|--------------------|-------------------|----------------------------------|--------|
| behavior-and-logic | `should_ignore_interface` full pattern list, `filter_local_ips`, `get_local_ips`, the `report_on_send` inline expansion with appended `truncate`, the four unit tests | asked whether any added pattern can drop a real physical NIC (typified names `eth0`, `enp0s3`, `wlan0`, `en0` asserted kept; added patterns do not substring-match these), whether the cap could truncate before the virtual filter (it is applied last, only to survivors), and whether a provider-level cap alone bounds the payload (multi-listener expansion can reach 2×32; the inline `truncate` after the loop enforces the payload bound at the original assembly point) | no counterexample found; physical-NIC retention, filter-then-cap ordering, and payload-level bound are verified in place | pass |
| boundaries-and-failure-paths | empty interface list, list exactly at/above 32, list with >64 items, loopback-only systems, `get_if_addrs` error fallback `unwrap_or_default` | tried to falsify: cap on empty input (returns empty by construction), exactly-32 boundary (kept fully), 64-input dedup-free truncation (returns 32 in order), and whether the new code changes the pre-existing error fallback (unchanged `unwrap_or_default`) | the cap/order tests cover 64- and 40-input cases; empty-input and error-fallback behavior are unchanged pre-existing semantics | pass |
| regression-and-side-effects | full `p2p-frame` `--lib` suite labelled `--features x509`, `cargo check --workspace` clean | checked that the appended inline `truncate` and the provider changes do not disturb existing `sn_service`/`sn` tests (`sn_client_listener_entries`, protocol-candidate ordering, nat_probe_directive, signer/periodic-report, five-by-five command matrix, unreachable-stream eviction) and that map_ports behavior is unchanged | all 526 lib tests pass (same set as baseline); workspace check clean | pass |

## Verification

- Targeted check: `cargo test -p p2p-frame --features x509 --lib` executed in a clean detached worktree containing only this change (with a throwaway include-base directory added solely to work around finding F-078-1): 526 passed, 0 failed, including the 4 `filter_local_ips_*` cases. `cargo check -p p2p-frame --features x509` and `cargo check --workspace` pass in the delivery worktree.
- Result: pass
- Exception reason: the delivery worktree's `cargo test -p p2p-frame --features x509 --lib` itself cannot start because the pre-existing tunnel_manager `#[path]` includes (finding F-078-1) target a non-existent base directory, which aborts lib-test compilation for the whole crate before any test runs. The identical file is untouched by this task; test execution therefore used the isolated clean worktree with the one-off include-base workaround. Production compilation and workspace check pass in-tree.

## Findings

| id | severity | evidence | problem | blocking |
|----|----------|----------|---------|----------|
| F-078-1 | none | `cargo test -p p2p-frame --features x509 --lib` at `p2p-frame/src/tunnel/tunnel_manager.rs:4214` | `#[path]`-included module paths (`tests/nat_type_aware/tunnel_manager_tests.rs`, `tests/unit/tunnel/...`) resolve through a non-existent `src/tunnel/tunnel_manager/tests/` base, aborting lib-test compilation for the whole crate before any test runs in this tree. `tunnel_manager.rs` is untouched by this task; a clean detached worktree reproduces the same layout, and adding the one-off missing base directory allows the identical full lib-test suite (528) to pass. Pre-existing and unrelated; not fixed in this task. | no |

## Conclusion

- Accepted / rejected / needs changes: accepted
- Reason: both approved change_ids are delivered and verified — virtual/tunnel/container/VPN adapter addresses are filtered from the local-IP list; the filtered result is capped at 32; and the actual `report_on_send` payload (`ReportSn.local_eps`) is capped at 32 by an appended `local_eps.truncate(MAX_LOCAL_IP_COUNT)` at its original inline assembly position, so the "report no more than 32 IPs" requirement holds even with multiple unspecified listeners. Covered by four pure-function unit tests (virtual excluded, physical kept, provider 32 cap, ordered head/tail), the full `--lib` suite (526) and workspace check pass, and no deflation or pre-existing-regression evidence was found within the deliverable code; the only residual is the unrelated pre-existing tunnel_manager include-path compile abort, recorded as finding F-078-1.
