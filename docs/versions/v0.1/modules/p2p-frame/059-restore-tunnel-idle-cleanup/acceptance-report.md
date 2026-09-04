# Restore Tunnel Idle Cleanup Acceptance Report

## Findings

| ID | Severity | Owning Stage | Correctness Category | Evidence | Problem | Blocking |
|----|----------|--------------|----------------------|----------|---------|----------|
| F-059-A3-000 | none | none | overall | Approved P-001; concrete TCP/QUIC/PN lifecycle implementations; manager exact-candidate cleanup; focused and real-loopback tests; `20260904T122905Z-p2p-frame+059-restore-tunnel-idle-cleanup-all.json` | Independent post-testing falsification found no remaining requirement, design, implementation, or testing defect in the approved scope. | no |

## Prior Finding Closure

| Prior ID | Closure Evidence | Status |
|----------|------------------|--------|
| F-059-D1-001 | The rejected manager-facing `ActivityTrackedTunnel` design was removed. `TunnelActivity` is owned by concrete TCP/QUIC tunnels, PN reuses its existing channel lifecycle, and `TunnelManager` stores and returns the original `Arc<dyn Tunnel>`. | closed |
| F-059-T1-001 | TCP and QUIC real loopback tests now assert stream, datagram, and control handles keep both opened and accepted tunnels active until final drop; transport open futures also prove pending retention and cancellation release. Shared state-machine and control prepare-before-ACK tests cover strict timeout and atomic ordering. | closed |
| F-059-A2-001 | This report was regenerated from the current implementation and current evidence; all obsolete wrapper, stable-wrapper identity, old test-count, and old artifact claims were removed. | closed |

## Object and Scope

- Task manifest: `task.yaml`
- Review date: 2026-09-04
- In-scope implementation: tunnel-owned business activity for TCP/QUIC/PN, incoming control prepare ordering, the additive `Tunnel::try_retire_idle` contract, exact TunnelManager cleanup, focused regressions, and TCP/QUIC real loopback lifecycle assertions
- Review mode: independent reviewer inspected current primary source and tests separately from implementation; the acceptance owner integrated that review only after the final clean task run

## Requirement Coverage

| change_id | Requirement or Boundary | Source | Implementation Evidence | Finding | Status |
|-----------|-------------------------|--------|-------------------------|---------|--------|
| CHG-restore-tunnel-idle-cleanup | Restore strict business-idle cleanup while pending or live channel work prevents retirement; cover incoming/outgoing stream, datagram, and control ownership; atomically arbitrate activity versus cleanup; preserve original Arc identity, exact candidate topology, proxy reconciliation, compatibility, and lock-free close | approved `proposal.md` P-001 and `pipeline/plan.md` Acceptance Baseline | `networks/tunnel.rs::TunnelActivity`; TCP/QUIC `begin_pending` and promotion paths; control `listen_with_prepare`; PN lifecycle counters and `zero_since`; `TunnelManager::cleanup_closed_tunnels`; focused manager tests and real TCP/QUIC loopback assertions | No approved behavior is missing and no rejected wrapper or out-of-scope wire/default-policy change remains. | pass |

## Independent Defect Discovery

| Category | Applicable Scope | Evidence Inspected | Adversarial Check | Finding or Not-Applicable Reason | Status |
|----------|------------------|--------------------|-------------------|----------------------------------|--------|
| requirement-and-behavior | Historical idle predicate adapted to current multi-channel tunnels | P-001, plan baseline, `P2pStackConfig::idle_timeout` call site, three built-in Tunnel implementations, manager cleanup | Challenged whether manager still inferred activity, heartbeat counted as work, custom tunnels were forcibly retired, or independent PN/TTP policies were replaced | Concrete tunnels decide business idleness; manager supplies timeout and performs topology cleanup; default trait method opts custom implementations out; neighboring policies remain unchanged. | pass |
| logic-and-control-flow | Strict timeout, pending-to-active promotion, retired rejection, and cleanup selection | `TunnelActivity::try_retire_idle`, `PendingTunnelActivity::promote`, PN `try_retire_idle`, manager retain closure | Tried exact timeout equality, one-nanosecond-over, pending opens, partial stream-half release, stale promotion, and unavailable candidates | Retirement uses strict `elapsed > timeout`; any pending/work count retains the tunnel; promotion and retirement share one lock and have one winner. | pass |
| boundary-and-input | Zero timeout, stream pairs, one-sided datagrams, listener rejection, and custom Tunnel defaults | activity boundary tests, control rejection test, TCP/QUIC loopback tests, manager opt-out test | Exercised equality, zero timeout, one remaining half, no listener, retired prepare, and default hook behavior | Applicable boundaries reject early cleanup and expose no successful channel after retirement. | pass |
| state-and-data-integrity | Live/pending/active/retired transitions, PN generation, exact candidate identity, proxy state | shared activity state, PN lifecycle lock, original manager `TunnelRef`, exact candidate and proxy cleanup tests | Looked for split wrapper identity, tuple-only removal, stale generation mutation, count underflow, and orphan proxy tracking | One concrete Tunnel owns the lifecycle; original Arc identity is retained; exact removal and existing `tunnels -> state` reconciliation preserve remaining topology. | pass |
| error-handling-and-recovery | Open failure/cancellation, prepare rejection, collision, and transport close failure | RAII drops, `Retired` control response, interrupted promotion, manager close warning, injected close-error test | Cancelled a pending open, forced retirement before promotion, rejected incoming prepare, and returned `IoError` from close | Pending/work ownership is released on failure; late success is rejected; removal remains committed and close errors are logged after locks are released. | pass |
| resource-lifetime-and-cleanup | Read/write handle leases, pending guards, sockets, manager entries, and close lifetime | tracked IO wrappers, Drop implementations, TCP/QUIC/PN channel wrappers, cleanup loop | Dropped one half then the final half, aborted open futures, retired with no work, and re-entered manager locks during close | Each delivered handle or PN shared lease keeps the tunnel active for its real lifetime; final release restarts the idle interval; close runs lock-free. | pass |
| concurrency-and-ordering | Activity versus cleanup race and incoming ACK visibility | shared mutex transitions, TCP/QUIC incoming paths, control prepare-before-ACK test, manager lock scopes | Raced idle retirement with begin/promote, checked success ACK ordering, and attempted manager lock acquisition from close | Atomic tunnel-local state gives one winner; incoming success is not exposed without ownership; no reverse manager-lock acquisition was found. | pass |
| interface-and-compatibility | Public Tunnel trait, custom implementations, stack callers, wire behavior, and Arc identity | additive default method, all-target x509 compile, manager return/publication paths, full manager suite | Searched for required downstream method implementations, wrapper leakage, signature/export changes, and protocol-codec changes | The default method is source-compatible; built-ins opt in; no manager wrapper, wire change, timeout default change, or caller migration is required. | pass |
| security-and-capacity | Remote open rejection and per-channel state cost | control/open validation order, fixed counters and RAII handles, unchanged codecs and auth paths | Checked whether rejected opens could expose channels or add unbounded queues/retries | Retirement rejection is fail-closed and adds only bounded per-live-operation ownership; no trust boundary, parser, retry source, or unbounded collection changed. | pass |
| test-adequacy | Normal, boundary, negative, error, lifecycle, concurrency, compatibility, and cross-module behavior | focused activity/control/PN/manager tests, TCP/QUIC loopback tests, full 89-test manager suite, full 479-test library suite, `20260904T122905Z-p2p-frame+059-restore-tunnel-idle-cleanup-all.json` | Mapped each failure hypothesis to assertions and challenged the lack of a full transport/type/direction Cartesian matrix | Shared primitive tests plus transport entry-point inspection, real pending opens, and all stream/datagram/control opened+accepted active-handle tests expose the relevant defects. Passive pending and datagram/control cancellation permutations remain useful future hardening, not a blocker. | pass |

## Document Consistency

| Document | Source | Implementation Consistency | Finding | Status |
|----------|--------|----------------------------|---------|--------|
| proposal | `proposal.md` P-001 | Current code restores the strict idle behavior through tunnel-owned lifecycle state and preserves every named non-goal. | No proposal-to-code mismatch found. | pass |
| design | `pipeline/plan.md` | Concrete ownership, default hook, incoming ordering, original Arc identity, state transitions, lock order, and failure flows match current code. | The earlier wrapper design is absent from both current plan and implementation. | pass |
| testing | `testplan.yaml`, test sources, and `20260904T122905Z-p2p-frame+059-restore-tunnel-idle-cleanup-all.json` | Registered commands cover compile compatibility, deterministic lifecycle/order checks, PN integration, manager topology, real TCP/QUIC loopback ownership, and the complete crate-local library suite. | Evidence is current and the one parallel-port collision was excluded from final proof. | pass |

## Result Summary

- Overall result: accepted
- Outcome: `idle_timeout` again retires a connected manager candidate only after its concrete Tunnel atomically confirms no pending or live business channel remains beyond the strict timeout.
- Blocking issues: none; the tunnel-ownership redesign, built-in lifecycle-test gap, and stale-report mismatch are closed.
- Residual validation boundary: local tests do not claim public-NAT, deployed multi-SN, real-router traversal, exact housekeeping wall-clock timing, or exhaustive scheduler interleavings; passive-pending and datagram/control-cancellation permutations are not individually enumerated.
- Next action: record accepted pipeline runtime state and remove unfinished-task bookkeeping.

## Conclusion

- Accepted / rejected / needs changes: accepted
- Reason: current source, focused lifecycle/concurrency tests, real TCP/QUIC loopback coverage, full manager compatibility tests, and the clean task-scoped artifact agree with approved P-001, and independent review found no remaining blocking defect.
