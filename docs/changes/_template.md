# [Change Title]

- Status: active / complete / upgraded
- Owner module:
- Task manifest:
- Approved proposal:
- Affected paths:
- Explicit tier override: none
- Expanded high-risk packet: none / existing task packet

## Approach

<!-- Keep this short. Record only the implementation choice, compatibility constraint, or assumption that another contributor would need. -->

## Risk Screen

- Public contract, protocol, or CLI change: no
- Persistent data, schema, or migration change: no
- Security, privacy, or trust-boundary change: no
- Concurrency, lifecycle, or runtime integration change: no
- Material dependency/build graph, supply-chain trust, produced artifact, production default/feature rollout, release/deployment, compatibility, or rollback impact: no
- Material UI, accessibility, localization, or navigation workflow change: no
- Harness rule, checker, or test-infrastructure change: no
- Cross-project or architectural boundary change: no

If any answer becomes `yes`, record the evidence. When it changes the confirmed requirement, scope, or acceptance boundary, return the existing packet's `proposal.md` to draft, recommend the appropriate tier, and obtain proposal/tier reconfirmation before further project mutation. When it is newly discovered risk inside the unchanged confirmed scope, keep the user-selected tier and record the residual risk instead of silently upgrading. If the user reconfirms `high-risk`, set `Status: upgraded`, update the existing `task.yaml`, add the risk profile and downstream lifecycle artifacts to the same packet, record that expansion above, and continue from the earliest responsible stage.

Answer `yes` only for confirmed material consequences. A documentation/configuration change or matching path alone is not sufficient; documentation-only and configuration-only corrections remain lower-tier when they do not change governed intent or runtime behavior.

If an explicit current-user lower-tier override applies, record the user's instruction in `Explicit tier override`, keep the selected tier, and describe the known risk under `Residual risk or follow-up`.

## Verification

- Targeted check:
- Result:
- Residual risk or follow-up:
