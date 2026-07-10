# Module Interface Export Rules

## Goal
- Keep Rust module root files focused on module wiring and public interface export.
- Prevent production implementation logic from accumulating in `mod.rs` files.

## Scope
- Rust production source files named `mod.rs`.
- Applies to new implementation, bugfix, optimization, and refactor work.

## Rule
- `mod.rs` MUST act as a module facade only.
- `mod.rs` MAY contain:
  - `mod` declarations
  - `pub mod` declarations
  - `pub use` re-exports
  - feature-gated module declarations or re-exports
  - small interface-only type aliases or constants needed to expose the module boundary
- `mod.rs` MUST NOT contain business logic, protocol logic, state-machine logic, IO logic, persistence logic, background task logic, request handling, or substantial helper functions.
- New implementation logic MUST be placed in responsibility-specific sibling files or submodules, then exported through `mod.rs`.
- Existing implementation logic in `mod.rs` MUST NOT be expanded. When touched for functional changes, move the affected logic into a responsibility-specific file unless the task explicitly records a temporary exception.

## Exception Rule
- A temporary exception is allowed only when moving the logic would exceed the approved task scope or create avoidable compatibility risk.
- The exception MUST be recorded in the active design or admission evidence with:
  - affected `mod.rs` path
  - reason the logic cannot be moved in the current task
  - follow-up path for extracting it

## Acceptance Guidance
- Implementation review MUST treat newly added non-interface logic in `mod.rs` as a finding.
- Design review SHOULD ask for a named sibling file or submodule for each new responsibility instead of accepting `mod.rs` as the implementation location.
