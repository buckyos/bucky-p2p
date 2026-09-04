---
task_manifest: task.yaml
status: approved
---

# Fallback Rendezvous Failures to Legacy SnCall Proposal

Risk profile: ./risk-profile.yaml

## Workflow Tier Judgment
- Proposed tier: high-risk
- Final tier: high-risk
- Tier rationale / triggered boundaries: The change alters the production tunnel-establishment failure sequence in `TunnelManager`. It must preserve lookup reuse, the absolute connection deadline, legacy direct/reverse action semantics, and PN as the final fallback. This is a material runtime-integration and compatibility boundary in the core `p2p-frame` networking path.
- Proposal and tier confirmation: Confirmed by the user with automatic downstream completion authorized.

## Background and Goal
When NAT-aware planning selects a rendezvous plan, any final rendezvous request or target-action failure currently jumps directly to `open_proxy_path`. That skips the approved legacy `SnCall` fallback even though the same lookup endpoints, serving SN, and NAT traversal context are still available. If PN is absent while legacy direct/reverse establishment remains viable, the connection fails unnecessarily.

Restore the intended order: rendezvous first, then the real legacy `SnCall` path on any final rendezvous failure, and PN only if the legacy caller action also fails.

## Scope

### In scope
- Change the NAT-aware rendezvous error branch to invoke `open_nat_aware_tunnel_legacy` instead of invoking `open_proxy_path` directly.
- Reuse the already-resolved remote endpoints, remote identity/name, serving SN id, NAT traversal context, and caller fallback action; do not perform a second peer/SN query.
- Keep rendezvous and the subsequent legacy attempt under the existing absolute total deadline so fallback does not renew the connection budget.
- Preserve legacy behavior in which `SnCall` coordinates concurrently with the caller action and a failed caller action falls back to PN.
- Replace the unit expectation that currently locks in rendezvous-to-PN behavior with regression coverage proving rendezvous failure enters legacy first, while PN remains the final fallback after legacy direct failure.

### Out of scope
- Do not change rendezvous plan selection, retry/error classification inside `open_rendezvous_tunnel`, NAT candidate generation, endpoint eligibility, or prediction algorithms.
- Do not change SN/PN wire formats, authentication, public APIs, or proxy-upgrade behavior.
- Do not add a second SN query or refresh NAT profiles during fallback.
- Do not claim public-NAT, deployed multi-SN, or real-router traversal evidence from local tests.

### Boundary with neighboring modules
`p2p-frame/src/tunnel/tunnel_manager.rs` owns the rendezvous/legacy/PN orchestration. Existing `SNClientService` and PN implementations remain unchanged. Regression tests stay in the existing NAT-aware tunnel-manager test module and exercise the production orchestration method with controlled network doubles.

## Requirement Review
The requested fallback order is reasonable and matches task 034's approved behavior. The safest implementation is a localized orchestration change: retain inputs needed by both branches, pass them to the legacy path only after rendezvous has returned a final error, and let the existing legacy function own the direct-to-PN transition. The key tradeoff is latency: a failed rendezvous may consume part of the shared budget before legacy begins, but renewing the timeout would amplify the caller-visible deadline and is therefore excluded.

## Proposal Items
| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-001 | CHG-fallback-rendezvous-to-legacy | After any final rendezvous request/action error, run the real legacy `SnCall` flow with the existing lookup/context; only a failed legacy caller action may enter PN. | Keep the current plan selection, rendezvous-internal retry policy, and one-query contract; keep all stages under the existing absolute deadline. | Legacy gets only the time remaining after rendezvous, preserving bounded latency even if that reduces its opportunity to complete. | Regression tests observe a legacy caller action after rendezvous failure, prove a viable legacy direct path succeeds without PN, and prove legacy direct failure reaches PN only afterward; targeted x509-gated checks pass. | No protocol, NAT-policy, retry-classification, query-refresh, or PN implementation changes. |

## Success Criteria
- Concrete user-visible or system-visible result: A selected rendezvous path that ultimately fails no longer skips a still-viable legacy `SnCall`; PN remains the last fallback rather than the immediate next step.
- Required evidence: A red/green regression bound to `CHG-fallback-rendezvous-to-legacy`; assertions for legacy-before-PN ordering and the no-PN/legacy-available success case; targeted `p2p-frame` tests with the required `x509` feature; bounded-deadline and query-reuse review evidence.
- Explicit non-goals: No wire migration, no second peer query, no change to plan selection or candidate policy, and no claim of real public NAT traversal.

## Risks
- Ownership: rendezvous currently consumes endpoint/name values, so retaining them for fallback must avoid changing their contents or introducing stale substitutes.
- Deadline: calling legacy outside the existing timeout would renew the budget and violate the bounded connection contract.
- Ordering: direct PN invocation must remain solely inside the legacy action-failure branch for this flow; otherwise a test may pass without proving legacy ran first.
- Regression coverage: a mock that only asserts the final tunnel form cannot distinguish rendezvous-to-PN from rendezvous-to-legacy-to-PN; tests must observe the legacy action/call path explicitly.

## Approval Record
- approver: user
- approval_date: 2026-09-04
- user_statement: `确认，自动完成`
