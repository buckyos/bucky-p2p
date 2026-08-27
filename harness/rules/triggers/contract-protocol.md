# Contract And Protocol Trigger

## Required Coverage
- Design names every affected API, CLI, RPC, event, message, file format, extension point, exported function, or cross-module interface.
- Record compatibility as `new`, `backward-compatible`, `migration-required`, or `breaking`, with affected callers and a migration path where needed.
- Testing includes positive and negative contract checks plus a boundary-focused validation path.

## Reviewer Focus
- Check error semantics, idempotency, undocumented caller dependencies, versioning, migration closure, and cross-module traceability.
