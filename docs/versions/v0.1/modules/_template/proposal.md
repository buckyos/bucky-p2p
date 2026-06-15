---
module: example-module
submodule:
version: v0.1
status: draft
approved_by:
approved_at:
approved_content_sha256:
---

# [Module Name] Proposal

## Background and Goal
<!-- What problem is being solved and why now -->

## Scope
### In scope
### Out of scope
### Boundary with neighboring modules

## Assumptions and Ambiguities
- Assumptions:
- Open ambiguities:
- Decision needed before approval:

## Constraints
- Allowed libraries/components:
- Disallowed approaches:
- System constraints:

## Requirement Challenge
| question | evaluation | risk_or_tradeoff | decision |
|----------|------------|------------------|----------|
| Is the stated requirement reasonable for the user's goal? | answer with evidence, not yes/no only | name concrete risk or none with reason | keep / revise / reject with reason |
| Is there a simpler or safer approach? | compare at least one alternative | name tradeoff | chosen approach |
| Is scope ambiguous? | list ambiguity or none with evidence | acceptance impact | clarify before approval / proceed |

## Large Module Submodule Decision
<!-- If this module is a large package/crate/service with several independent submodules, decide whether this proposal creates a new direct submodule or belongs inside an existing submodule. New independent features should use a submodule packet under this module directory, for example `<submodule>/proposal.md` and `<submodule>/design.md`; optional testing artifacts may live there after implementation. Do not put independent submodule docs under `design/<submodule>/` or `testing/<submodule>/`. Keep each human-authored proposal doc under 1000 lines; split oversized docs by submodule, responsibility, or requirement boundary. -->

| Submodule | New or Existing | Responsibility | Proposal Packet | Reason |
|-----------|-----------------|----------------|-----------------|--------|
| example | new / existing / none | | `<submodule>/proposal.md` | |

## Trigger Matrix
| trigger_category | applies | evidence | required_checks | deferred_checks_and_reason |
|------------------|---------|----------|-----------------|----------------------------|
| contract/protocol | yes/no | concrete doc/code evidence | required checks if yes | owner/risk/acceptance impact if deferred |
| data/schema | yes/no | concrete doc/code evidence | required checks if yes | owner/risk/acceptance impact if deferred |
| security/privacy/permission | yes/no | concrete doc/code evidence | required checks if yes | owner/risk/acceptance impact if deferred |
| runtime/integration | yes/no | concrete doc/code evidence | required checks if yes | owner/risk/acceptance impact if deferred |
| build/dependency/config/deployment | yes/no | concrete doc/code evidence | required checks if yes | owner/risk/acceptance impact if deferred |
| ui/datamodel/workflow | yes/no | concrete doc/code evidence | required checks if yes | owner/risk/acceptance impact if deferred |
| harness/process | yes/no | concrete doc/code evidence | required checks if yes | owner/risk/acceptance impact if deferred |

## High-Level Outcomes
<!-- Business outcomes only; detailed acceptance rules and expected results are generated after implementation and testing. -->

## Proposal Items
| proposal_id | change_id | Outcome | Scope Boundary | Success Evidence | Explicit Non-Goal |
|-------------|-----------|---------|----------------|------------------|-------------------|
| P-001 | CHG-example | | | | |

## Success Criteria
- Concrete user-visible or system-visible result:
- Required evidence:
- Explicit non-goals:

## Risks
<!-- High-risk changes, shared contracts, security or migration impact -->

## Downstream Follow-Up
| follow_up_id | Owning Stage | Reason | Triggering Proposal Item | Blocking |
|--------------|--------------|--------|--------------------------|----------|
| FU-001 | design/testing/acceptance | | P-001 | yes/no |

## Proposal Guardrails
- Proposal-stage tasks modify only `proposal.md` unless the user explicitly requests a multi-stage update.
- If this proposal change requires design, implementation, testing, or acceptance updates, record the needed follow-up instead of editing downstream artifacts by default.
- If this is a large module with many independent submodules, classify whether the requested feature is a new direct submodule before design or testing starts.
- Put the split submodule's proposal and design files in a submodule directory under this module packet; optional post-implementation testing artifacts may live there when generated. Do not use `design/<submodule>/` or `testing/<submodule>/` for independent submodule docs.
- Keep human-authored proposal docs under 1000 lines where practical; if a doc would exceed 1000 lines, split it and update the relevant document index.
- If the request has multiple plausible meanings, record the ambiguity instead of silently choosing one.
- Proposal approval should not depend on chat-only context; task-critical assumptions belong in this file.
- Keep implementation strategy out of proposal except where a constraint is part of the requirement.
- Every implementation-ready requirement must have a stable `change_id`.
- A broad module-level statement is not enough for implementation admission; the relevant `change_id` must name the concrete behavior, contract, or implementation unit being admitted.

## Approval Record
<!-- Fill only when the user explicitly approves this document. Agents MUST NOT fill this section or set `status: approved` on their own initiative. `approver` must match front matter `approved_by`; `user_statement` must quote the user's approval instruction verbatim. The same edit must record front matter `approved_content_sha256` (generate via `schema-check.py --print-approval-hash <this-file>`); any later content edit invalidates the approval until re-approved. Auto-pipeline approvals use front matter plus `harness/pipeline-plan.md` launch evidence instead of this section. -->
- approver:
- approval_date:
- user_statement: ""
