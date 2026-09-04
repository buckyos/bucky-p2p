# Lightweight Acceptance Report

- Status: complete

## Object and Scope
- Task manifest: task.yaml
- Workflow tier: standard
- Change record: docs/changes/043-direct-cache-failure-invalidation.md

## Delivery Summary
- Outcome: a cached Direct connection entry now has a real invalidation mechanism: after `DIRECT_CACHE_MAX_FAILURES = 2` consecutive dial failures (per-endpoint `fail_count`, reset on the next success), the entry is removed from the connection-info cache, receives no `+10,000` cache-preferred or `+2,000` last-success bonus in `preferred_direct_endpoints`, and `open_known_tunnel_with_options` stops issuing the dedicated single-endpoint preflight, falling straight through to the full concurrent candidate matrix. `P2pConnectionInfoCache` gains a backward-compatible defaulted `remove`, and `DefaultP2pConnectionInfoCache` actually deletes the entry.
- Handoff: verified with three new in-crate regression tests plus the pre-existing cache-preference and endpoint-score tests, the full feature-gated lib suite (432/432), the loopback real-socket strategy matrix (6/6), and a clean workspace compile. Residual follow-ups are tooling (the unified runner still lacks `--features x509` wiring) and evidence breadth (mock/loopback suites are not public-NAT or multi-host evidence).

## Proposal Consistency
| change_id | Requirement or Boundary | Proposal Source | Delivery Evidence | Finding | Status |
|-----------|-------------------------|-----------------|-------------------|---------|--------|
| stale_direct_cache_invalidation | Two consecutive dial failures since last success degrade/delete the cached Direct entry; degraded opens skip the single-endpoint preflight and enter the full candidate matrix | proposal.md P-DCACHE-1 | `open_known_tunnel_with_options` removes degraded entries before the preflight and falls through to `preferred_direct_endpoints`; `degraded_cached_direct_entry_is_removed_when_single_endpoint_fails` and `stale_cache_direct_skips_preflight_after_two_failed_opens` assert cache removal and dial counts (3 instead of 4) | matches | pass |
| stale_direct_cache_scoring_demotion | No `+10,000` cache-preferred or `+2,000` last-success bonus once the failure count reaches the threshold; static-WAN bonus and failure penalty unchanged | proposal.md P-DCACHE-2 | `preferred_direct_endpoints` gates both bonuses on the degraded predicate; `degraded_cached_direct_entry_skips_preflight_and_loses_preference` asserts the working endpoint outranks a stale endpoint after one success plus two failures | matches | pass |
| stale_direct_cache_regression_tests | Regression tests prove preflight skip, scoring demotion, real cache deletion, and existing tunnel/cache cases stay green | proposal.md P-DCACHE-3 | Three new in-crate tests plus pre-existing `conn_info_cache_direct_preferred_on_reconnect` and `endpoint_score_isolated_by_protocol` pass; full lib suite 432/432, loopback real-socket flow 6/6, workspace check clean | matches | pass |

## Independent Defect Discovery
| Category | Evidence Inspected | Adversarial Check | Finding or Not-Applicable Reason | Status |
|----------|--------------------|-------------------|----------------------------------|--------|
| behavior-and-logic | degraded predicate, preflight branch, scoring branch, success reset of `fail_count`, `MockDialNetwork.dial_count`, error propagation | challenge N=1 vs N=2, `saturating_add` overflow, reset-on-success recovery, one open contributing both a preflight failure and a matrix failure, and the state-lock/cache-lock ordering | no defect found: the threshold is per-endpoint consecutive-dial semantics since the last success; preflight+matrix double counting is the existing dial pattern now bounded to the stale endpoint and is asserted by the repeated-open test | pass |
| boundaries-and-failure-paths | only-stale-endpoint opens (no SN/proxy), defaulted third-party `remove`, cache re-add after other-endpoint success, old-port recovery, all-path failure | test what happens when a custom cache leaves the entry (scoring still demotes and the preflight is still skipped), when the old port recovers (success resets the score and re-adds the entry), and when every path fails (error returns and the Default cache is empty) | no defect found: behavior does not depend on whether `remove` actually deletes, so third-party caches inherit the fix; Default cache deletes, and matrix membership preserves recovery | pass |
| regression-and-side-effects | full lib suite, loopback real-socket matrix, workspace consumers, `P2pConnectionInfoCache` implementors, `open_direct_path` semantics | search other cache readers and implementors (ConnectionInfoRecorder compiles unchanged via the default `remove`), verify no wire/config/CLI change, no timing dependence, and no change to reverse/proxy cache lifecycle | no regression: 432/432 lib, 6/6 real-socket flow, workspace check clean; only pre-existing fixture warnings and the known unified-runner x509 feature gap remain | pass |

## Verification
- Targeted check: `cargo test -p p2p-frame --features x509 --lib` (432/432), `cargo test -p p2p-frame --features "x509 test-real-socket-matrix" --test real_p2p_tunnel_flow` (6/6), `cargo check --workspace` (clean), plus the three new regression tests and the pre-existing cache/score tests
- Result: passed
- Exception reason: not applicable; the feature-gated commands above are the executable evidence, and the pre-existing unified-runner x509 wiring gap is recorded in the change record

## Findings
| ID | Severity | Evidence | Problem | Blocking |
|----|----------|----------|---------|----------|
| F-1 | none | demotion/preflight tests | no defect: threshold, removal, and scoring demotion match the approved contract, and the preflight+matrix double dial is intentionally asserted | no |
| F-2 | low | repeated-open test | the chosen N=2 threshold still costs up to two failing dials on the first opens after NAT change before demotion; recorded as an explicit tradeoff in proposal and change record | no |
| F-3 | low | mock/loopback evidence | loopback and mock suites are not public-NAT or multi-host evidence, and the unified runner lacks x509 feature wiring (pre-existing tooling drift) | no |

## Conclusion
- Accepted / rejected / needs changes: accepted
- Reason: delivery satisfies the approved requirement: stale Direct cache entries are degraded and removed after two consecutive dial failures, lose the never-decaying preference bonuses, and no longer force a dedicated full-timeout preflight once degraded, with regression coverage in lib (432/432), the loopback real-socket matrix (6/6), and a clean workspace compile.
