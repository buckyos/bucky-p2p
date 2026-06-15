---
module: example-module
submodule:
version: v0.1
status: draft
approved_by:
approved_at:
approved_content_sha256:
---

# [Module Name] Design

> This file describes module shape, dependencies, key flows, exported interfaces, and external dependencies. Keep low-level implementation detail out unless it affects a public contract or important control flow.

## Design Scope
### Goals
### Non-goals

## Overall Approach
<!-- Main implementation path, layers, data flow, key interactions -->

## Simplicity Check
- Smallest sufficient approach:
- Existing components or patterns reused:
- New abstractions introduced:
- Why each new abstraction is necessary:

## Current Structure
<!-- For existing code, describe the current structure and constraints before describing the planned change -->

## Invariants to Preserve
<!-- For changes to an existing module: list the externally observable behavior and invariants that must NOT change (interface contracts, data formats, concurrency assumptions). Acceptance treats this list as the regression boundary. Greenfield modules record `not-applicable: new module`. -->
-

## Submodules
<!-- Type must be business, shared, technical, or assembly. Dependencies flow business -> shared / technical; nothing depends on assembly. A shared submodule needs at least 2 consumers and one nameable responsibility. If this module has no direct submodules, record one row with `not-applicable: <reason>` in the Submodule cell and explain in Responsibility. -->
| Submodule | Type | Responsibility | Depends On | Exported Interface | Notes |
|-----------|------|----------------|------------|--------------------|-------|
| example | business / shared / technical / assembly | | | | |

## Boundary Rationale
<!-- Split by business logic first. Put different business responsibilities in different business submodules. Put implementation logic shared by multiple business submodules in a shared submodule. Put technically distinct implementation areas, such as HTTP interfaces, persistence/database access, external adapters, codecs, schedulers, or storage, in dedicated submodules when they have clear responsibility boundaries. -->

| Boundary | Classification | Why Separate | Shared Logic / Technical Area | Notes |
|----------|----------------|--------------|-------------------------------|-------|
| | business / shared / technical | | | |

## Boundary Decision Matrix
| boundary | classification | business_responsibility | shared_logic_or_technical_area | decision |
|----------|----------------|-------------------------|--------------------------------|----------|
| example | business / shared / technical / none | concrete responsibility | concrete shared or technical area, or none with reason | split / keep together with reason |

## Dependency Graph
<!-- List module and submodule dependencies. The graph must be acyclic; no module or submodule may depend on something that depends back on it. -->

| Source | Depends On | Reason | Cycle Check |
|--------|------------|--------|-------------|
| | | | acyclic |

## Key Call Flows
<!-- Flows that cross a submodule or module boundary must describe failure handling: who owns the failure or timeout, retry behavior, idempotency, and the state left on partial completion. -->
| Flow | Caller | Callee / Submodule Path | Purpose | Failure Handling | Notes |
|------|--------|--------------------------|---------|------------------|-------|
| | | | | | |

## Large Module Submodule Decision
<!-- If this module is a large package/crate/service with several independent submodules, confirm whether the approved proposal creates a new direct submodule. Independent new features should use a submodule packet under this module directory, for example `<submodule>/proposal.md` and `<submodule>/design.md`; optional testing artifacts may live there after implementation. Do not put independent submodule design under `design/<submodule>/`. Keep each human-authored doc under 1000 lines; if a doc would exceed 1000 lines, split it by submodule, responsibility, or interface boundary. -->

| Submodule | Source Proposal | Decision | Design Packet | Reason |
|-----------|-----------------|----------|---------------|--------|
| example | P-001 / CHG-example | new / existing / none | `<submodule>/design.md` | concrete reason |

## Trigger Matrix
| trigger_category | applies | evidence | design_coverage | required_checks | deferred_checks_and_reason |
|------------------|---------|----------|-----------------|-----------------|----------------------------|
| contract/protocol | yes/no | concrete doc/code evidence | section or doc path | required checks if yes | owner/risk/acceptance impact if deferred |
| data/schema | yes/no | concrete doc/code evidence | section or doc path | required checks if yes | owner/risk/acceptance impact if deferred |
| security/privacy/permission | yes/no | concrete doc/code evidence | section or doc path | required checks if yes | owner/risk/acceptance impact if deferred |
| runtime/integration | yes/no | concrete doc/code evidence | section or doc path | required checks if yes | owner/risk/acceptance impact if deferred |
| build/dependency/config/deployment | yes/no | concrete doc/code evidence | section or doc path | required checks if yes | owner/risk/acceptance impact if deferred |
| ui/datamodel/workflow | yes/no | concrete doc/code evidence | section or doc path | required checks if yes | owner/risk/acceptance impact if deferred |
| harness/process | yes/no | concrete doc/code evidence | section or doc path | required checks if yes | owner/risk/acceptance impact if deferred |

## Directly Mapped Change Items
| change_id | proposal_id | Design Coverage | Scope Paths | Interface / Boundary Impact | Notes |
|-----------|-------------|-----------------|-------------|-----------------------------|-------|
| CHG-example | P-001 | this file section or `design/...` doc | `path/or/component` | none / describe impact | |

## Implementation Order
| Phase | Goal | Preconditions | Outputs | Depends On | Parallel |
|-------|------|---------------|---------|------------|----------|
| 1 | | | | | no |

## Key Decisions
<!-- Boundary split, technical submodule choice, and cross-module collaboration decisions must each record at least one seriously considered alternative and why it lost. `not-applicable` needs a concrete reason. -->
| Decision | Chosen | Alternatives Considered | Rejection Reason |
|----------|--------|-------------------------|------------------|
| example decision | chosen approach | rejected alternative, or `not-applicable: <reason>` | why the alternative loses |

## Data and State
<!-- Every persistent datum or shared state has exactly one owning submodule; other submodules access it only through the owner's exported interface. Entities with a lifecycle must list state transitions including failure/interrupted states. Stateless modules record one row with `not-applicable: <reason>` in the Data or State cell. -->
| Data or State | Owner Submodule | Access For Others | State Transitions |
|---------------|-----------------|-------------------|-------------------|
| example store | owning submodule | via owner exported interface | states incl. failure/interrupted, or `not-applicable: <reason>` |

## Testability
<!-- Tests are designed after implementation, so reserve the seams here. Each submodule's exported interface must be verifiable without the other business submodules. External service/system dependencies must pass through a replaceable boundary (technical submodule or injection point). State how error and boundary cases will be triggered; if a failure path cannot be constructed in tests, record the alternative verification. -->
- Isolation seams per submodule:
- Replaceable external boundaries:
- How error/boundary cases will be triggered:
- Untriggerable failure paths and their alternative verification:

## Interfaces and Dependencies
### Exported interfaces
<!-- Every exported interface must name at least one concrete consumer: an existing module or a change_id from this version. Exports without a named consumer are speculative; remove them and note the removal in ## Simplicity Check. Compatibility is one of new / backward-compatible / migration-required / breaking; breaking and migration-required entries must list affected callers and migration path in Notes. Modules that export nothing record one row with `not-applicable: <reason>`. -->
| Interface | Consumer | Compatibility | Notes |
|-----------|----------|---------------|-------|
| example API | consumer module or change_id | new / backward-compatible / migration-required / breaking | affected callers and migration path when breaking |

### External module dependencies
### External service or protocol constraints

## Document Index
| Document | Topic | Scope |
|----------|-------|-------|
| `design.md` | module overview | full module |

## Risks and Rollback
<!-- Implementation, migration, compatibility, rollback -->

## Design Guardrails
- Do not rewrite approved proposal intent in `design.md`.
- If this module has no direct submodules, say so explicitly.
- If this is a large module with many independent submodules, model each new independent feature as its own direct submodule unless this design explains why it belongs inside an existing submodule.
- Keep module and submodule dependencies acyclic. Resolve circular dependencies before implementation.
- Keep dependency direction business -> shared / technical: shared and technical submodules never depend on business submodules, and nothing depends on assembly.
- Split by business logic first: different business responsibilities go into different business submodules.
- Extract implementation logic shared by multiple business submodules into a shared submodule; a shared submodule needs at least 2 consumers and one nameable responsibility.
- Give every persistent datum or shared state exactly one owning submodule; other submodules go through the owner's exported interface.
- Use dedicated submodules for technically distinct implementation areas when they have clear boundaries, such as HTTP interfaces, database/persistence access, external adapters, codecs, schedulers, or storage.
- Describe only the implementation layout that affects ownership, exported interfaces, external dependencies, or key call flow.
- Keep detailed independent submodule documentation in a submodule packet under this module directory, such as `<submodule>/design.md`; do not accumulate all module documentation in one file or place independent submodule docs under `design/<submodule>/`.
- Keep human-authored docs under 1000 lines where practical; if a doc would exceed 1000 lines, split it and update the document index.
- For existing code, describe current structure first, then the change.
- Do not introduce idealized architecture that the proposal did not approve.
- Prefer the simplest design that satisfies the approved proposal and documented constraints.
- Do not add speculative extension points, configuration, or abstractions for single-use code.
- Every implementation-ready design item must carry the same `change_id` used in `proposal.md`.
- For multi-module or cross-boundary work, list each affected module and explain whether it needs separate implementation admission.

## Approval Record
<!-- Fill only when the user explicitly approves this document. Agents MUST NOT fill this section or set `status: approved` on their own initiative. `approver` must match front matter `approved_by`; `user_statement` must quote the user's approval instruction verbatim. The same edit must record front matter `approved_content_sha256` (generate via `schema-check.py --print-approval-hash <this-file>`); any later content edit invalidates the approval until re-approved. Auto-pipeline approvals use front matter plus `harness/pipeline-plan.md` launch evidence instead of this section. -->
- approver:
- approval_date:
- user_statement: ""
