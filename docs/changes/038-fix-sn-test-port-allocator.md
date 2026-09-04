# Fix SN test port allocator to avoid the OS ephemeral range

- Status: complete
- Owner module: p2p-frame
- Task manifest: docs/versions/v0.1/modules/p2p-frame/038-fix-sn-test-port-allocator/task.yaml
- Approved proposal: docs/versions/v0.1/modules/p2p-frame/038-fix-sn-test-port-allocator/proposal.md
- Affected paths:
  - p2p-frame/src/sn/tests.rs
  - docs/changes/038-fix-sn-test-port-allocator.md
- Explicit tier override: none
- Expanded high-risk packet: none

## Approach

`next_port()` used to fetch-and-add from 42000, so under a full parallel suite the
SN fixed-port range drifted into the OS ephemeral port range and collided with
tests that obtain ports via `bind(0)` (observed `AddrInUse` in
`sn_profile_flow_tests::tcp_only_registration_never_receives_or_executes_probe`).
The allocator now hands out ports from `[TEST_PORT_LOW, TEST_PORT_HIGH]` where
`TEST_PORT_LOW = 25025` and `TEST_PORT_HIGH` is `min(43100, ephemeral_start - 1)`
on Linux (ephemeral start read from `/proc/sys/net/ipv4/ip_local_port_range`),
falling back to 43100 on other platforms (below macOS/Windows ephemeral starts).
Allocation is monotonic decreasing with compare-exchange and wraps only through
the reserved range; a regression test samples 64 ports and asserts uniqueness
plus `< ephemeral_start` on Linux. Existing `SETUP_MAX_RETRY` /
`is_addr_bind_conflict` bounded retries are unchanged and remain the fallback
for external-process or exotic-environment conflicts.

## Risk Screen

- Public contract, protocol, or CLI change: no (test allocator only)
- Persistent data, schema, or migration change: no
- Security, privacy, or trust-boundary change: no
- Concurrency, lifecycle, or runtime integration change: no (production scheduling/runtime unchanged)
- Material dependency/build graph, supply-chain trust, produced artifact, production default/feature rollout, release/deployment, compatibility, or rollback impact: no
- Material UI, accessibility, localization, or navigation workflow change: no
- Harness rule, checker, or test-infrastructure change: no
- Cross-project or architectural boundary change: no

Platform note: on Linux the reserved range is derived from the live ephemeral
start; if a host configures ephemeral_start <= 25025 the assert in `next_port`
fails loudly instead of silently overlapping.

## Verification

- Targeted check: new range regression test + `sn_profile_flow` group at 16 threads + full `--features x509 --lib` at default and 4 threads
- Result: pass
  - `sn::tests::next_port_stays_outside_os_ephemeral_range`: pass
  - `sn_profile_flow` group (6 tests, 16 threads): 3/3 passes
  - Full lib suite default threads: 425/425 twice
  - Full lib suite `--test-threads=4`: 425/425 once
- Residual risk or follow-up: exotic OS ephemeral configurations below 25025 are not covered by this allocator and would trip the range assert; existing bounded bind-conflict retries remain the fallback.
