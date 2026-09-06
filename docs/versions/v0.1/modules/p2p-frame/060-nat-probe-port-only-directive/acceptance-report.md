# NAT Probe Port-Only Directive Acceptance Report

## Findings

| ID | Severity | Owning Stage | Correctness Category | Evidence | Problem | Blocking |
|----|----------|--------------|----------------------|----------|---------|----------|
| F-060-A2-000 | none | none | overall | Approved P-001; current wire, scheduler, service, client, and miner implementations; focused boundary and lifecycle tests; `20260905T023743Z-p2p-frame+060-nat-probe-port-only-directive-all.json` | Independent post-repair falsification found no remaining requirement, design, implementation, or testing defect in the approved scope. | no |

## Prior Finding Closure

| Prior ID | Closure Evidence | Status |
|----------|------------------|--------|
| F-060-A1-001 | `handle_report_sn` captures `ProbeTransition` and `scheduler.ports()` under one scheduler lock, then applies the transition after releasing it, so the immediate directive and retained response ports come from one configuration snapshot. | closed |
| F-060-A1-002 | All three asynchronous `ActiveSN` mutation sites use `update_active_sn_if_owner` keyed by `(sn_peer_id, conn_id)`; a mutation-sensitive test proves stale connection A cannot overwrite replacement B while current B can update every snapshot field. | closed |
| F-060-A1-003 | Decoder tests feed old endpoint-bearing payloads under the reused report/directive magics and verify fail-closed behavior; both port-vector extension paths also cover `MAX_NAT_PROBE_ENDPOINTS + 1`. | closed |

## Object and Scope

- Task manifest: `task.yaml`
- Review date: 2026-09-05
- In-scope implementation: port-only report and directive wire fields, bounded port scheduling, wildcard reflector binding without advertised-IP derivation, client reconstruction from the exact active authenticated QUIC SN IPv4, connection-owned snapshot refresh, and the affected `sn-miner` assembly
- Review mode: independent second-pass reviewer inspected the current proposal, plan, implementation, callers, tests, and fresh unified test artifact after the first acceptance findings were repaired

## Requirement Coverage

| change_id | Requirement or Boundary | Source | Implementation Evidence | Finding | Status |
|-----------|-------------------------|--------|-------------------------|---------|--------|
| CHG-nat-probe-port-only-directive | Send only bounded unique non-zero probe ports; bind reflectors on wildcard IPv4 without a published address; reconstruct WAN QUIC endpoints from the selected authenticated SN IPv4; retain exact-connection ownership, security checks, lifecycle invalidation, and fallback; intentionally provide no old-wire compatibility | approved `proposal.md` P-001 and `pipeline/plan.md` Acceptance Baseline | `sn/protocol/sn.rs::NatProbeDirective` and `ReportSnResp`; `NatProbeScheduler::{set_ports,ports}`; `SnService::handle_report_sn` and reflector startup; `sn/client/sn_service.rs::expand_nat_probe_ports` and `update_active_sn_if_owner`; focused wire/client/service/scheduler tests, real listener flow, strategy matrix, and `sn-miner` real-process test | No approved behavior is missing; the full endpoint remains an internal client/tunnel snapshot and no server-supplied probe IP remains on the wire. | pass |

## Independent Defect Discovery

| Category | Applicable Scope | Evidence Inspected | Adversarial Check | Finding or Not-Applicable Reason | Status |
|----------|------------------|--------------------|-------------------|----------------------------------|--------|
| requirement-and-behavior | Port-only wire and active authenticated SN IPv4 ownership | P-001, plan baseline, report/directive codecs, service report path, client active-publication and real report/query/call flow | Challenged whether either extension still carried an IP, whether the client selected an identity/response IP instead of the active route, and whether later prediction diverged from immediate probing | Both wire paths contain only ports; immediate and retained snapshots are reconstructed from the selected authenticated QUIC SN endpoint and later tunnel prediction consumes that snapshot. | pass |
| logic-and-control-flow | Issuance eligibility, validation order, reconstruction, and later consumption | scheduler directive branch, client directive validation/expansion, `get_nat_probe_snapshot_for_sn`, tunnel prediction caller | Tried invalid capability/version/correlation/deadline/replay inputs, unusable active endpoints, insufficient reconstructed endpoints, and missing signer state | Invalid work fails before probing, valid ports expand deterministically to WAN QUIC endpoints, and unusable snapshots retain the existing Unknown/fallback behavior. | pass |
| boundary-and-input | Port count/value/uniqueness, wire shape, and active endpoint address family | protocol helpers and codec tests; client rejection tests; service and miner configuration tests | Exercised empty, one, zero, duplicate, over-eight, old same-magic endpoint payload, TCP, IPv6, unspecified, multicast, and broadcast inputs | Every malformed or unsupported set fails closed; valid bounded ports round-trip and reconstruct without accepting a server-selected address. | pass |
| state-and-data-integrity | Scheduler generation/profile/in-flight state and exact ActiveSN ownership | `NatProbeScheduler::set_ports`; `handle_report_sn`; `update_active_sn_if_owner`; scheduler and client mutation tests | Changed configuration during report assembly and completed stale connection A after replacement B had all snapshot fields populated | Directive and response ports are one locked scheduler snapshot; a stale `(sn_peer_id, conn_id)` cannot overwrite replacement endpoint, profile, signer, generation, or request state. | pass |
| error-handling-and-recovery | Malformed wire, invalid targets, reflector startup failure, probe failure, and fallback | codec fail-closed paths, client rejection paths, atomic wildcard bind test, probe result/fallback flow | Injected old wire, invalid ports, unusable IPs, a later bind collision, and failed/unknown probing | Invalid extensions become empty/no directive, no invalid target is probed, partial reflector startup spawns no tasks, and failures retain the existing Unknown and traversal fallback semantics. | pass |
| resource-lifetime-and-cleanup | Bounded reflectors, sockets, spawned tasks, signed-probe budgets, and shutdown | maximum port validation; reflector bind/start/stop/drop code; existing PNAT limits; lifecycle tests | Checked partial bind, normal stop/drop, timeout capacity release, and whether port-only reconstruction introduced unbounded targets or work | At most eight targets are accepted; all sockets bind before spawn; startup failure releases sockets; stop/drop drains tasks; existing request-rate and signature-concurrency bounds remain intact. | pass |
| concurrency-and-ordering | Configuration snapshot consistency, stale asynchronous completions, and scheduler capacity | one-lock report snapshot; three guarded ActiveSN writeback sites; scheduler timeout/capacity tests | Raced configuration refresh with response assembly, old connection completion with replacement publication, and in-flight timeout with new issuance | One scheduler lock prevents split snapshots, exact connection ownership prevents stale mutation, and timeout cleanup releases bounded issuance capacity without changing generation authority. | pass |
| interface-and-compatibility | Public Rust fields/methods, same-magic wire break, downstream consumers, and deployment boundary | exported structs and setters; negative API fixture; consumer closure; all-target compile; old-wire tests | Searched for old symbols/consumers and attempted to decode endpoint-bearing payloads as the new format | The direct breaking migration is complete and matches the explicit no-compatibility decision; old payloads fail closed and mixed-version/rolling deployment remains intentionally unsupported. | pass |
| security-and-capacity | Destination trust binding, response authentication, replay/deadline handling, and bounded load | active authenticated endpoint capture; signer verification; source/token checks; fixed port and PNAT budgets | Tried server-directed alternate IP, wrong signer/source/token, replay/expired work, and oversized target sets | The destination IP cannot be supplied by the directive, signed-probe validation is unchanged, and no input can exceed the existing bounded target and signature budgets. | pass |
| test-adequacy | Normal, boundary, negative, error, lifecycle, concurrency, compatibility, and cross-module behavior | focused wire/client/scheduler/service tests; wildcard bind rollback; `sn-miner` real process; real report/query/call and six-branch strategy matrix; fresh 10-step artifact | Inspected mutation sensitivity and wire bytes rather than accepting green status alone; specifically replayed stale owner writes, old same-magic payloads, and over-limit vectors | Registered tests expose all task-specific failure hypotheses and the fresh unified run completed every step with exit code zero. Loopback evidence does not claim public-NAT or mixed-version deployment coverage. | pass |

## Document Consistency

| Document | Source | Implementation Consistency | Finding | Status |
|----------|--------|----------------------------|---------|--------|
| proposal | `proposal.md` P-001 | Current code sends only ports, derives the target IP from the active authenticated SN connection, retains wildcard binding, and implements every named non-goal including the direct compatibility break. | No proposal-to-code mismatch found. | pass |
| design | `pipeline/plan.md` | Wire ownership, dependency migration, scheduler state, exact ActiveSN ownership, wildcard startup, failure flows, and unchanged tunnel boundary match the current implementation. | The post-repair atomic snapshot and stale-owner rules now satisfy the plan. | pass |
| testing | `testplan.yaml`, test sources, and `20260905T023743Z-p2p-frame+060-nat-probe-port-only-directive-all.json` | All registered API, consumer, compile, unit, integration, real-process, real-listener, and strategy-matrix steps ran against the current repair set and exited zero. | Evidence is fresh and includes regressions for all three prior acceptance findings. | pass |

## Result Summary

- Overall result: accepted
- Outcome: SN report responses and directives now publish only reflector ports, while the client reconstructs probe destinations from the exact active authenticated QUIC SN IPv4 and preserves the same internal endpoint snapshot for later traversal prediction.
- Blocking issues: none; the atomic server snapshot, stale ActiveSN write ownership, and old-wire/maximum-boundary test gaps are closed.
- Residual validation boundary: real-socket evidence is local loopback and does not claim public-NAT, cross-host firewall, or mixed-version deployment proof; mixed-version compatibility is explicitly out of scope.
- Next action: record accepted pipeline runtime state, complete lifecycle checks, and remove unfinished-task bookkeeping.

## Conclusion

- Accepted / rejected / needs changes: accepted
- Reason: current source, exact-owner concurrency regressions, malformed and old-wire codec tests, wildcard startup coverage, real listener/process flows, consumer closure, and the fresh unified artifact agree with approved P-001, and independent second-pass review found no remaining blocking defect.
