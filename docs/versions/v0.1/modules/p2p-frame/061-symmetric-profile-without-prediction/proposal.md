---
task_manifest: task.yaml
status: approved
---

# p2p-frame Proposal: Keep SymmetricLike Profile Without Prediction Hint

Risk profile: not-created (replace with ./risk-profile.yaml only after high-risk confirmation)

## Workflow Tier Judgment
- Proposed tier: standard
- Final tier: standard
- Tier rationale / triggered boundaries: bounded single-project bugfix in p2p-frame NAT traversal. It is not `trivial` because it changes runtime traversal behavior across three integrated layers (QUIC listener, UdpTunnelNetwork trait, SN client probe reporting) and touches the crate-internal `UdpTunnelNetwork` trait contract (all implementers are in-repo). No high-risk trigger: no wire/protocol format change (NatProfile is already exchanged and versioned), no persistent data, no security boundary change, no dependency/build or rollout impact, no cross-project or architectural boundary change.
- Proposal and tier confirmation: confirmed by the user without revisions; no unresolved questions were listed.

## Background and Goal

When the SN-driven NAT probe observes a symmetric mapping whose external ports are not arithmetically predictable (real-auth SN reproduction: samples 40000/40003/40006 classify SymmetricLike; samples 40000/40003/40007 also classify SymmetricLike because ports differ), the client still ends up reporting `Unknown`:

- `QuicTunnelListener::predict_traversal_endpoints` (p2p-frame/src/networks/quic/listener.rs:736-762) couples observation/classification with prediction-candidate generation. For `SymmetricLike` it collects only `profile.predicted_ports(...)`; with an unusable/absent prediction hint the list is empty and the function returns `Err(NotFound, "listener NAT prediction is unavailable")`.
- The SN client probe entry (`SNService::probe_endpoints`, p2p-frame/src/sn/client/sn_service.rs:1421-1466) maps every error to `NatProfile::unknown()`, discarding the valid `SymmetricLike` classification.
- Downstream, `select_connect_plan` (p2p-frame/src/tunnel/nat_connect_plan.rs:169) treats `Unknown` as legacy-plan input, so the intended `BoundedBestEffort` strategy for "symmetric but unpredictable" peers is unreachable through this probe path and connections fall back to legacy.

Goal: separate "obtain the mapping profile" from "generate prediction candidates" so a valid classification is preserved even when no prediction hint is available.

## Scope
### In scope
- Add a profile-only probe API: `UdpTunnelNetwork::probe_nat_profile(...)` returning `P2pResult<NatProfile>`, implemented by `QuicTunnelNetwork` and backed by a split-out `QuicTunnelListener::probe_nat_profile` that performs the existing probe/classification logic and errors only on real probe failures (closed, timeout, send failure, no observed endpoint).
- Keep `predict_traversal_endpoints` behavior for its rendezvous caller: it now builds on `probe_nat_profile` and still returns `Err(NotFound, "listener NAT prediction is unavailable")` when no prediction candidates exist.
- SN client `probe_endpoints` switches to the profile-only API, so `SymmetricLike` with no usable prediction hint is reported instead of `Unknown`.
- Add a required trait-method stub (`NotSupport`) to the three in-repo test mocks of `UdpTunnelNetwork`.
- Regression tests in the listener-level suite reproducing the issue: unpredictable symmetric ports (40000/40003/40007) yield `Ok(SymmetricLike)` with no hint from the profile API while the prediction API still errors; predictable ports (40000/40003/40006) keep yielding predicted candidates.

### Out of scope
- No change to the NAT probe wire protocol, `NatProfile` encoding, or SN-side scheduler/plan logic (`nat_connect_plan.rs` already handles `SymmetricLike` without a hint via `BoundedBestEffort`).
- No change to rendezvous prediction semantics (`CandidateMode::Predicted` still requires candidates; failures fall back as today).
- No new public API surface outside the crate-internal trait method.

### Boundary with neighboring modules
- `sn-miner` SN service is untouched: it already accepts fresh `SymmetricLike` probe results (nat_probe_scheduler accepts any non-Unknown fresh profile).
- Tunnel manager rendezvous flow is untouched; its prediction path keeps today's error semantics.

## Requirement Review
The requested outcome (keep valid classification without prediction hints) is reasonable and matches the existing design intent: `NatProfile` already models `SymmetricLike` with an optional hint, and the connect planner already has the `BoundedBestEffort` branch for unpredictable symmetric peers. The only mismatch is the client probe entry funneling through the prediction-candidate API. Chosen direction: split the listener logic into profile observation vs. candidate generation and point the SN client at the former. Tradeoff accepted: the rendezvous flow keeps requiring prediction candidates and may still fail over to fallbacks when a re-probe loses predictability; that is today's behavior and is not part of this fix.

## Proposal Items
| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-001 | CHG-symmetric-profile-without-prediction | Unpredictable symmetric mapping observations keep their `SymmetricLike` classification in the client probe path instead of degrading to `Unknown` | Applies to the SN-probe profile path; rendezvous prediction API keeps its existing candidate requirement and errors | Profile API returns classification without candidates; prediction API stays candidate-only | Listener-level test: 40000/40003/40007 → `probe_nat_profile` = Ok(SymmetricLike), hint None, prediction API still NotFound; 40000/40003/40006 → prediction endpoints generated; existing SN flow tests stay green | No wire protocol, scheduler, or connect-plan behavior change |

## Success Criteria
- Concrete user-visible or system-visible result: a peer behind an unpredictable symmetric NAT is classified `SymmetricLike` in its reported NAT profile, enabling the `BoundedBestEffort` connect strategy instead of unconditional legacy connect.
- Required evidence: targeted listener-level regression tests for both port patterns; `cargo test -p p2p-frame` targeted suites (listener/rendezvous prediction, udp tunnel network mocks, nat-type-aware flows) pass.
- Explicit non-goals: no protocol/version changes, no scheduler or plan changes, no rendezvous fallback behavior change.

## Risks
- `UdpTunnelNetwork` gains a required method; all current implementers are in this repository and will be updated in the same change.
- Probe failures that previously surfaced as the prediction `NotFound` error now surface only through the profile path; rendezvous semantics are preserved by keeping the candidate-generation error in `predict_traversal_endpoints`.
