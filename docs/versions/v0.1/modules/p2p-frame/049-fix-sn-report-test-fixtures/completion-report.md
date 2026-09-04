# Lightweight Acceptance Report

## Object and Scope
- Task manifest: task.yaml
- Workflow tier: trivial
- Change record: not-applicable

## Delivery Summary
- Outcome: Repaired the detached SN service test fixtures so report responses can publish a valid local identity certificate, and replaced the online/probe test's scheduler-dependent count assertion with an explicit in-flight result-report barrier.
- Handoff: The four reported regressions pass. Production `SnService`, signed PNAT validation, report wire data, NAT probe scheduling, and active-SN publication order were not changed.

## Proposal Consistency
| change_id | Requirement or Boundary | Proposal Source | Delivery Evidence | Finding | Status |
|-----------|-------------------------|-----------------|-------------------|---------|--------|
| CHG-fix-sn-report-test-fixtures | Restore the four named tests through production-faithful local identity setup and deterministic timing evidence without altering runtime behavior. | `proposal.md` Scope and Proposal Item P-001 | Test-only section of `p2p-frame/src/sn/service/service.rs`; `p2p-frame/tests/nat_type_aware/sn_profile_flow_tests.rs`; four exact passing regressions; 46 passing service tests; 6 passing profile-flow tests. | Delivery stays inside the approved test-fixture boundary and preserves the production identity requirement and online flow. | pass |

## Independent Defect Discovery
| Category | Evidence Inspected | Adversarial Check | Finding or Not-Applicable Reason | Status |
|----------|--------------------|-------------------|----------------------------------|--------|
| behavior-and-logic | Baseline diff for both task paths; `handle_report_sn` validation/identity order; `SNClientService` active publication, directive execution, and failed follow-up handling. | Checked whether the repair weakened missing-identity handling, bypassed authenticated peer checks, or merely removed the online-order assertion. | No production branch changed. Test identities now return their encoded certificate, and the async test observes active state both while the follow-up is blocked and after its failure warning is emitted. | pass |
| boundaries-and-failure-paths | `test_sn_service`, directory fixture, rejecting report tests, `Notify` permit semantics, timeout and malformed-response path. | Challenged identity mismatch/rejection ordering, notification-before-wait races, handler blocking, missing follow-up reports, and post-failure active-state loss. | Rejection occurs before the local identity read and remains covered. `Notify` retains a permit across the small started/release scheduling window; bounded timeouts fail rather than hang; the final active-state assertion occurs after correlated failure processing. | pass |
| regression-and-side-effects | Exact baseline diff, all 46 `SnService` tests, all 6 `sn_profile_flow_tests`, and five repeated runs of the formerly racy test. | Searched for production edits, extra changed paths, altered owner/directory behavior, repeat report races, stale blocked tasks, and formatting damage. | Only approved test code differs from the task baseline. Expanded and repeated tests pass. `git diff --check` passes; the only whole-file rustfmt complaint is an unrelated import layout already present in the baseline. | pass |

## Verification
- Targeted check: `cargo test -p p2p-frame --features x509 --lib 'sn::service::service::tests::protocol_version_query_tests::authenticated_report_updates_local_query_version' -- --exact`; `cargo test -p p2p-frame --features x509 --lib 'sn::service::service::tests::sn_service_default_validator_allows_report' -- --exact`; `cargo test -p p2p-frame --features x509 --lib 'sn::service::service::tests::sn_report_updates_local_detail_without_publishing_route' -- --exact`; `cargo test -p p2p-frame --features x509 --lib 'sn::tests::sn_profile_flow_tests::initial_probe_and_result_report_failure_do_not_gate_online' -- --exact`; `cargo test -p p2p-frame --features x509 --lib 'sn::service::service::tests'`; `cargo test -p p2p-frame --features x509 --lib 'sn::tests::sn_profile_flow_tests'`; five repeated exact runs of the preceding `initial_probe_and_result_report_failure_do_not_gate_online` command; `rustfmt --edition 2024 --check p2p-frame/tests/nat_type_aware/sn_profile_flow_tests.rs`; `git diff --check -- p2p-frame/src/sn/service/service.rs p2p-frame/tests/nat_type_aware/sn_profile_flow_tests.rs`.
- Result: passed
- Evidence summary: Reported tests 4/4, service module 46/46, profile-flow module 6/6, repeated race regression 5/5, and scoped formatting/diff checks passed.
- Exception reason: Whole-file rustfmt for `service.rs` reports a pre-existing import-layout difference reproduced against the captured baseline; it was intentionally not reformatted because that line is unrelated to this task.

## Findings
| ID | Severity | Evidence | Problem | Blocking |
|----|----------|----------|---------|----------|
| F-049-1 | none | Final baseline diff, expanded test results, repeated timing regression, and post-warning active-state assertion | No remaining defect found. During review, the active-state assertion was moved after the failure warning so it proves state retention after failure processing rather than only before handler completion. | no |

## Conclusion
- Accepted / rejected / needs changes: accepted
- Reason: All approved requirements are implemented in test-only code, the four failures and affected suites pass, repeated execution no longer depends on scheduler ordering, and the independent review found no remaining production, boundary, or regression issue.
