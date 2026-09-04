---
task_manifest: task.yaml
status: approved
---

# P2P Frame SN Report Test Fixture Repair Proposal

Risk profile: not-created

## Workflow Tier Judgment
- Proposed tier: trivial
- Final tier: trivial
- Tier rationale / triggered boundaries: The failures are confined to stale test identity setup and a racy test timing assertion in `p2p-frame`; the repair does not change production behavior, public protocol, security validation, dependencies, or runtime lifecycle semantics, and focused regression commands are available.
- Proposal and tier confirmation: User confirmed the displayed proposal and proposed `trivial` tier on 2026-09-04.

## Background and Goal
Four SN tests fail after the signed PNAT/report-response work. Three detached `SnService` fixtures do not install the local SN identity now required to populate `ReportSnResp.peer_info`. The online-before-probe test assumes a scheduler-dependent ordering between `wait_online` polling and the follow-up report, so the follow-up can legitimately be observed first.

## Scope
### In scope
- Give report-handling unit-test services a valid local test identity/certificate matching the production `SnServer` setup contract.
- Replace the scheduler-sensitive follow-up-report count assertion with explicit test synchronization that holds the failing result report in flight while online state is asserted.
- Re-run the four reported regressions and the narrow affected SN test groups with the required `x509` feature.

### Out of scope
- No relaxation of signed PNAT signer/certificate validation.
- No change to `ReportSnResp` wire fields, SN online publication order, NAT probe scheduling, or production `SnService`/`SnServer` behavior.
- No unrelated cleanup of the existing dirty worktree or prior task artifacts.

### Boundary with neighboring modules
The change remains in `p2p-frame` test fixtures and tests. Existing production code in the same source file is treated as pre-existing user work and is not altered.

## Requirement Review
The requested failures share fixture assumptions introduced by recent SN identity and NAT-probe behavior. Repairing those assumptions is preferable to weakening the production identity requirement or adding runtime delays/retries. A deterministic synchronization barrier makes the online/non-gating behavior directly observable without depending on executor timing.

## Proposal Items
| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-001 | CHG-fix-sn-report-test-fixtures | Restore the four named SN tests with production-faithful identity setup and deterministic online/follow-up-report ordering evidence. | Test-only fixture and synchronization edits in `p2p-frame`. | Adds a small explicit coordination primitive to one async test in exchange for eliminating timing dependence. | Each named test passes; focused affected suites pass with `--features x509`; fresh defect review finds no production or neighboring-test regression. | Changing signed PNAT trust, SN online semantics, or unrelated tests. |

## Success Criteria
- Concrete user-visible or system-visible result: All four reported tests pass reliably without changing production SN behavior.
- Required evidence: Red evidence is the supplied failure output plus source-level reproduction mapping; green evidence is focused Cargo test execution with `--features x509`, followed by the trivial-tier completion check and independent proportional defect review.
- Explicit non-goals: Broad workspace refactoring, protocol compatibility changes, public-NAT validation, and modification or cleanup of unrelated dirty files.

## Risks
The main risk is accidentally hiding a real production identity or ordering bug. The implementation therefore keeps the identity requirement intact, mirrors the production identity setup in fixtures, and makes the non-gating assertion stronger through an in-flight follow-up report rather than removing coverage.
