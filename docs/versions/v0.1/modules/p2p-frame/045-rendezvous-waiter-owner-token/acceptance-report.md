# Rendezvous Waiter Owner Token Acceptance Report

Risk profile: ./risk-profile.yaml

## Findings
| ID | Severity | Owning Stage | Correctness Category | Evidence | Problem | Blocking |
|----|----------|--------------|----------------------|----------|---------|----------|
| F-045-001 closed | none | none | test-adequacy | `rendezvous_waiter_owner_duplicate_notify_completes_incumbent_action` now exercises duplicate `on_sn_rendezvous`; the final task artifact explicitly runs direction-aware, wrong-direction, and reverse-registration Drop cases | First acceptance found that helper-only duplicate coverage would not detect a call-site order regression and that the broad DV filter omitted ordinary waiter compatibility cases. Testing added the actual call-site case and three explicit steps, then regenerated all task evidence. | no |

## Object and Scope
- Task manifest: task.yaml
- Module: p2p-frame
- Version: v0.1
- Task name: 045-rendezvous-waiter-owner-token
- change_id values reviewed: rendezvous_waiter_owner_token_lifecycle, rendezvous_waiter_collision_regression_tests
- Review date: 2026-09-03
- In-scope implementation: tokenized incoming waiter entries, atomic rendezvous owner/waiter publication and replacement, token-aware lifecycle cleanup, and focused regressions
- Review mode: independent falsification after testing return F-045-001; conclusion selected after fresh implementation, call-site, test, and artifact review

## Requirement Coverage
| change_id | Requirement or Boundary | Source | Implementation Evidence | Finding | Status |
|-----------|-------------------------|--------|-------------------------|---------|--------|
| rendezvous_waiter_owner_token_lifecycle | Bind waiter cleanup to owner/registration generation and atomically publish the rendezvous owner with its waiter without changing tuple delivery or public contracts | `proposal.md` P-RWOT-1 | `IncomingWaiterEntry`; token-bearing `ReverseWaitRegistration` and `IncomingPlanWaitRegistration`; `install_rendezvous_owner`; `complete_rendezvous_owner`; `cancel_rendezvous_owner`; `remove_incoming_waiter_from_state`; updated `open_rendezvous_tunnel` and `on_sn_rendezvous` callers | Duplicate/conflict/yield contenders do not publish or tuple-clean; replacement removes only the displaced token, publishes the new owner and waiter under one lock, and stale cleanup cannot remove the new entry. | pass |
| rendezvous_waiter_collision_regression_tests | Reproduce duplicate incumbent loss and displaced-owner deletion of a replacement waiter, including the real inbound notification call site | `proposal.md` P-RWOT-2 | three `rendezvous_waiter_owner_` tests plus explicit direction-aware, wrong-direction, reverse registration Drop, and full rendezvous lifecycle runner steps | The helper and actual `on_sn_rendezvous(WaitIncoming)` paths preserve the incumbent; stable-order replacement survives displaced abort and stale registration Drop; compatibility paths remain green. | pass |

## Independent Defect Discovery
| Category | Applicable Scope | Evidence Inspected | Adversarial Check | Finding or Not-Applicable Reason | Status |
|----------|------------------|--------------------|-------------------|----------------------------------|--------|
| requirement-and-behavior | approved duplicate, replacement, stale cleanup, compatibility, and non-goal boundaries | proposal items, pipeline acceptance baseline, implementation deltas, three owner tests | searched for missing owner paths, unintended wire/NAT changes, and a helper-only false positive | All approved behavior is implemented; the actual duplicate notification path now proves incumbent completion, and no wire, NAT plan, endpoint, prediction, or fallback behavior changed. | pass |
| logic-and-control-flow | duplicate, conflict, yield, replacement, attach, complete, cancel, and incoming branches | `install_rendezvous_owner`, both production callers, `attach_rendezvous_task`, completion/cancel helpers | challenged whether any losing contender can publish or later tuple-clean, and whether a winner publishes before state visibility | Collision decisions occur before contender publication; replacement waiter removal/publication and owner insertion share one state lock; yielded/failed contenders never remove the incumbent tuple. | pass |
| boundary-and-input | same tuple and generation, different seq replacement, both directions, missing owner, and wrong direction | duplicate/replacement tests, existing collision/handoff/attach tests, explicit direction tests | exercised exact duplicate AlreadyExists, deterministic inbound replacement, stale token, non-reverse WaitIncoming delivery, and wrong-direction input | Exact duplicate preserves the incumbent token; different-seq inbound replacement preserves the new token; wrong direction does not consume the active plan waiter. | pass |
| state-and-data-integrity | in-memory owner map and pending waiter map | tokenized map entry, owner token reuse, compare-and-remove helper, state assertions in regressions | searched for partial owner/waiter state, stale mutation, double consumption, and replacement corruption | Owner and rendezvous waiter share the same opaque token; old removal, new waiter publication, and owner replacement are one atomic state transition; stale tokens are no-ops. | pass |
| error-handling-and-recovery | AlreadyExists, Conflict, yielded collision, spawn/attach failure, timeout, and cancellation | error branches, RAII registration, task artifact rendezvous set | tested whether an error path removes the incumbent or leaves an unowned notifier, and whether later cancellation affects a replacement | Duplicate and rejected contenders do not mutate the current entry; spawn/attach/timeout/cancel paths remove only their matching generation and retain bounded completion behavior. | pass |
| resource-lifetime-and-cleanup | notifier, owner task, completion sender, RAII guard, and incoming tunnel lifetime | `abort_rendezvous_owner`, both registration Drop implementations, completion/cancel, incoming consume | inspected success, duplicate, displacement, stale Drop, timeout, and manager cleanup for leaks or cross-generation deletion | Current notifier is consumed once; displaced work is cancelled after unlocking; ordinary and rendezvous registrations use token-aware cleanup; no new unbounded resource was introduced. | pass |
| concurrency-and-ordering | shared `ManagerState` publication/consumption under concurrent rendezvous and incoming delivery | mutex critical sections, task start notification, replacement test, call-site duplicate test | searched for TOCTOU, lost wakeup, nested state-lock deadlock, reverse lock order, and incoming-vs-cleanup races | Owner decision plus waiter publication is atomic; incoming and cleanup serialize on the same mutex; cancel/abort occurs after unlock; no nested reverse lock order or stale cross-generation removal remains. | pass |
| interface-and-compatibility | private callers, tuple key semantics, ordinary reverse/NAT consumers, SN wire/public API | consumer call sites, task plan API impact, explicit compatibility runner steps | checked whether tokenization changes tuple lookup, direction behavior, public symbols, wire fields, or ordinary RAII semantics | Incoming tunnels still consume the current `(remote_id, tunnel_id, direction)` entry; public and wire contracts are unchanged; active/wrong-direction and ordinary reverse Drop cases pass. | pass |
| security-and-capacity | local token allocation in the existing bounded lifecycle maps | risk profile, waiter entry allocation, unchanged authentication path | checked for trust-boundary exposure, predictable external identity, amplification, or new unbounded state | The token is process-local ownership identity and never crosses the wire or authorization boundary; entry count and existing lifecycle bounds are unchanged. | not-applicable |
| test-adequacy | red-green defect exposure, lifecycle branches, compatibility, and runnable task evidence | testplan, exact test bodies, final unified artifact, F-045-001 return | asked whether tests would fail if call-site pre-insertion/tuple cleanup returned, whether replacement abort could delete the winner, and whether ordinary tokenized consumers actually run | Actual `on_sn_rendezvous` duplicate order, helper duplicate, stable replacement, full rendezvous lifecycle, active/wrong direction, and reverse Drop are directly asserted; all five declared steps ran successfully. | pass |

## Document Consistency
| Document | Source | Implementation Consistency | Finding | Status |
|----------|--------|----------------------------|---------|--------|
| proposal | `proposal.md` | Implementation and tests satisfy both change items and preserve every non-goal | No requirement contradiction or uncovered approved outcome remains. | pass |
| design | `pipeline/plan.md` | Tokenized entry, shared owner token, one-lock replacement, tuple consume, failure flows, and rejected alternatives match current code | No design-to-code mismatch found; optional waiter/direction mismatch is unreachable in both private production callers and not an approved external contract. | pass |
| testing | `testplan.yaml` | All five declared commands are task-scoped and match the implemented test names and compatibility claims | Final artifact binds both change IDs, contains 3 focused owner tests, 32 rendezvous tests, and three explicit compatibility passes; no `cyfs-p2p-test` evidence is used. | pass |

## Result Summary
- Overall result: accepted
- Outcome: Duplicate/colliding inbound rendezvous can no longer overwrite or delete the incumbent waiter, and displaced/stale owner cleanup cannot remove a replacement owner's waiter.
- What was verified: atomic owner/waiter installation, exact duplicate handling through the real notification call site, stable-order replacement, stale registration/complete/cancel/abort protection, incoming tuple consumption, direction isolation, ordinary reverse registration cleanup, and existing rendezvous lifecycle behavior.
- Evidence used: `.harness/test-results/test-runs/20260903T091437Z-p2p-frame+045-rendezvous-waiter-owner-token-all.json`, implementation/testing stage-scope evidence, captured dirty-file baselines, current code/tests, and the F-045-001 acceptance return.
- Residual validation boundary: local controlled runtime tests do not prove public-NAT, multi-SN deployment, or every scheduler interleaving; the change is private in-process lifecycle behavior and makes no such claim.
- Blocking issues: none
- Next action: validate complete pipeline state and remove task 045 from unfinished-task bookkeeping.

## Conclusion
- Accepted / rejected / needs changes: accepted
- Reason: Both approved change IDs have matching production behavior and runnable regression evidence, the independent testing gap was closed and re-reviewed, all applicable defect-discovery categories pass, and no blocking finding remains.
