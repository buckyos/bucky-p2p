# Design Document Rules

## Goal
- Define the minimum structure and approval requirements for `design.md` and `design/`.

## Scope
- `docs/versions/<version>/modules/<module>/design.md`
- `docs/versions/<version>/modules/<module>/design/`
- `docs/versions/<version>/modules/<module>/<submodule>/design.md` when a large module is split into direct submodule packets
- required long-lived boundary sync in `docs/modules/<module>.md`

## Required Metadata
- `module`
- `version`
- `status`
- `approved_by`
- `approved_at`

## Required Content
- submodule list and responsibilities, with submodule type (business / shared / technical / assembly)
- dependencies between submodules and affected external modules
- key call flows with failure handling for flows that cross submodule or module boundaries
- implementation order
- exported interfaces with named consumers and compatibility decisions
- acyclic module and submodule dependency graph with business -> shared/technical dependency direction
- business-logic submodule boundaries and shared implementation submodules when they affect external behavior or ownership
- boundary decision matrix with concrete business/shared/technical decisions
- single-owner data and state modeling with failure-inclusive state transitions
- rejected alternatives for boundary, technical-choice, and cross-module collaboration decisions
- testability seams reserved for post-implementation test design
- invariants to preserve when changing an existing module
- trigger matrix with design coverage for every triggered category
- document index
- direct change mapping for implementation-ready work
- stable `change_id` values matching proposal items
- simplicity check covering reused components and any new abstractions
- large-module submodule documentation decision when adding new features
- major risks and rollback notes
- approval record, filled only when the user explicitly approves the document

## Guardrails
- Design must implement approved proposal intent without silently changing scope.
- Design tasks deliver `status: draft`; agents MUST NOT set `status: approved` or fill `approved_by` / `approved_at` on their own initiative.
- Apply `status: approved` only on an explicit user approval instruction, and record `approver`, `approval_date`, and the verbatim `user_statement` in `## Approval Record` in the same edit; or via auto-pipeline auto-confirm backed by `harness/pipeline-plan.md` launch evidence.
- The same edit that applies an approval MUST record front matter `approved_content_sha256` (generate via `schema-check.py --print-approval-hash <doc>`); later content edits make the approval stale until re-approved.
- Design tasks are single-stage by default and MUST NOT edit `proposal.md`, `testing.md`, `testing/`, `testplan.yaml`, `acceptance.md`, code, or test code unless the user explicitly requested those additional stages.
- Design tasks must not modify testing strategy, acceptance criteria, or implementation code.
- Design MUST stay at module shape level: submodules, dependencies, key call flows, exported interfaces, and external module dependencies. Avoid low-level implementation detail unless it affects a public contract, cross-module dependency, or important control flow.
- Design MUST list direct submodules or explicitly say that none exist.
- Design MUST keep module and submodule dependencies acyclic. Circular dependencies between modules, between submodules, or between a module and one of its submodules are design failures and MUST be resolved before implementation.
- Design MUST split modules and submodules by business logic first. Different business responsibilities belong in different business submodules.
- If multiple business submodules share implementation logic, that common logic MUST be modeled as its own shared submodule instead of being duplicated or hidden inside one business submodule.
- Business, shared, and technical boundary choices MUST be recorded in `## Boundary Decision Matrix`; `doc-structure-check.py --docs design` MUST fail if decisions are missing, malformed, or placeholder-only.
- Technically distinct implementation areas inside a business module, such as HTTP interfaces, persistence/database access, external adapters, codecs, schedulers, or storage, MUST be modeled in `## Boundary Decision Matrix`; if not split into a dedicated submodule, the matrix MUST record the reason.
- Every submodule `Type` MUST be one of `business`, `shared`, `technical`, or `assembly`. Dependencies MUST flow business -> shared / technical: shared and technical submodules MUST NOT depend on business submodules, and nothing may depend on an assembly submodule; `doc-structure-check.py --docs design` MUST fail direction violations.
- A shared submodule MUST have at least 2 consumers visible in the dependency graph (record external module consumers as explicit edges) and one nameable responsibility. Implementation logic with a single consumer stays inside that consumer. Grab-bag shared submodules named `common`, `utils`, `misc`, `helpers`, or similar are design defects.
- Every exported interface MUST name at least one concrete consumer: an existing module or a `change_id` from the current version. Exports without a named consumer are speculative; remove them and record the removal in `## Simplicity Check`.
- Adding or changing an exported interface MUST record a compatibility decision: `new`, `backward-compatible`, `migration-required`, or `breaking`. A breaking or migration-required change MUST list every affected caller and its migration path.
- Every persistent datum or shared state MUST have exactly one owning submodule recorded in `## Data and State`; other submodules access it only through the owner's exported interface. Cross-submodule direct writes to the same data are boundary defects and return work to design.
- Entities with a lifecycle MUST document state transitions that include failure and interruption states, not only the normal flow.
- Key call flows that cross a submodule or module boundary MUST describe failure handling: who owns the failure or timeout, whether the call retries, whether it must be idempotent, and what state remains on partial completion. Happy-path-only call flow design is incomplete.
- Boundary split, technical submodule choice, and cross-module collaboration decisions MUST each record at least one seriously considered alternative and its rejection reason in `## Key Decisions`; a decision that genuinely has no viable alternative records `not-applicable: <concrete reason>`.
- Because tests are designed after implementation, `## Testability` MUST reserve the seams: each submodule's exported interface verifiable without the other business submodules, external service/system dependencies behind a replaceable boundary, and a stated way to trigger error and boundary cases — or an alternative verification when a failure path cannot be constructed in tests.
- Changes to an existing module MUST list the externally observable behavior and invariants that must not change in `## Invariants to Preserve`; acceptance treats that list as the regression boundary. Greenfield modules record `not-applicable: new module`.
- A small implementation submodule MAY be represented by a single file. A larger implementation submodule that contains internal sub-responsibilities MUST describe its visible responsibilities and external dependencies here and keep detailed internal layout out of `design.md` unless required for the planned change.
- For existing code, describe current structure before describing the change.
- When the target module is a large subproject package, crate, service, or similar module root that already contains logically independent submodules — operationally, 3 or more directories or files with distinct externally visible responsibilities — a new logically independent feature MUST be modeled as its own direct submodule unless the design explains why it belongs inside an existing submodule.
- That modeling decision MUST be recorded in `## Large Module Submodule Decision`; `doc-structure-check.py --docs design` MUST fail when the table is missing, malformed, or placeholder-only.
- For large-module changes, keep the large module's `design.md` as the module overview and document index. Put detailed submodule design in the submodule packet under the large module directory, such as `docs/versions/<version>/modules/<module>/<submodule>/design.md`, not under `design/<submodule>/`.
- Human-authored design docs MUST stay under 1000 lines each. Any design document that would exceed 1000 lines MUST be split by submodule, responsibility, or interface boundary and the document index MUST be updated.
- Do not introduce idealized architecture unless the proposal approved that shift.
- Prefer the simplest sufficient approach that satisfies the approved proposal and constraints.
- Do not add speculative features, extension points, configuration, or abstractions.
- New abstractions MUST either match an established local pattern or remove real duplicated complexity, and the reason MUST be recorded in `## Simplicity Check`.
- Design must identify every affected module for cross-module work.
- Design must not use broad change buckets as implementation admission evidence.
- `Scope Paths` entries in `## Directly Mapped Change Items` MUST be concrete repo-relative path prefixes or globs (backtick-wrapped) that cover exactly the implementation area the change is allowed to touch. They are mechanically enforced: `admission-check.py` records them in the admission stamp, `edit-guard.py` blocks edits outside them, and `stage-scope-check.py --stage implementation` fails diffs outside them. Over-broad entries such as `src` defeat the gate and are a design review finding.
- Design MUST fill `## Trigger Matrix` for every trigger category and map every triggered category to design coverage and required checks; `doc-structure-check.py --docs design` MUST fail when this evidence is missing.
- If a design task discovers proposal ambiguity, return work to proposal instead of repairing it in place.
- If a design change implies testing or acceptance updates, record the downstream follow-up unless the user explicitly requested cross-stage synchronization.
- Before design completion, run `uv run --active python ./harness/scripts/doc-structure-check.py --version <version> --module <module> --docs design`.
