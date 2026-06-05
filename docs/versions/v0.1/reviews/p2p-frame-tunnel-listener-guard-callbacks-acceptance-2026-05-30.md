# p2p-frame Tunnel Listener Guard Callbacks Acceptance Report

## Findings
| ID | Severity | Stage | Evidence | Issue | Blocking |
|----|----------|-------|----------|-------|----------|
| F-1 | Low | governance | `stage-scope-check.py --stage proposal/design/implementation/testing` | Stage scope checks report pre-existing cross-stage dirty worktree files. The reviewed `tunnel_listener_guard_callbacks` evidence is mapped and tests pass, but the global worktree prevents a clean single-stage scope result. | no |

## Scope
- Module: `p2p-frame`
- Version: `v0.1`
- Review date: `2026-05-30`
- Change ID: `tunnel_listener_guard_callbacks`
- In scope: `TunnelListener` guard callback API, listener callback registry, TCP/QUIC/PN listener migration, `NetManager` guard ownership, unit/integration evidence.
- Out of scope: TCP/QUIC/PN wire protocol changes, incoming validator changes, `IncomingTunnelCallback` subscription key semantics, `TunnelManager` register/publish lifecycle changes.

## Inputs
- `docs/versions/v0.1/modules/p2p-frame/proposal.md`
- `docs/versions/v0.1/modules/p2p-frame/design.md`
- `docs/versions/v0.1/modules/p2p-frame/testing.md`
- `docs/versions/v0.1/modules/p2p-frame/testplan.yaml`
- `docs/versions/v0.1/modules/p2p-frame/acceptance.md`
- `docs/modules/p2p-frame.md`
- Implementation: `p2p-frame/src/networks/listener.rs`, `p2p-frame/src/networks/net_manager.rs`, TCP/QUIC/PN listener call sites, test fake listeners, `p2p-frame/Cargo.toml`, `Cargo.lock`
- Test evidence listed below

## Evidence
- `uv run --active python ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame`: passed
- `uv run --active python ./harness/scripts/admission-check.py --version v0.1 --module p2p-frame --change-id tunnel_listener_guard_callbacks`: passed
- `cargo check -p p2p-frame`: passed
- `cargo test -p p2p-frame`: passed, 104 tests
- `python3 ./harness/scripts/test-run.py p2p-frame unit`: passed, 104 tests
- `python3 ./harness/scripts/test-run.py p2p-frame integration`: passed, workspace test entry
- `python3 ./harness/scripts/test-run.py all all`: passed
- `./test-run.sh all all`: passed; emitted non-blocking shell warning `OSTYPE: parameter not set` while continuing to success
- `git diff --check`: passed
- `rg "accept_tunnel\\(|pending_accept: Mutex<Option<TunnelListenerCallback>>|TunnelListenerCallback = Box" -n p2p-frame/src`: no old `TunnelListener` API implementation/call sites found; remaining `accept_tunnel()` occurrences are `TunnelSubscription` test/consumer API.

## Consistency
- Proposal and design: consistent. Both define guard-owned permanent callback registration, guard-drop deletion, RCU/copy-on-write callback snapshots, and no-callback incoming tunnel drop.
- Design and implementation: consistent. `TunnelListener::watch_tunnels(...)` returns `TunnelListenerGuard`; `TunnelListenerCallbackRegistry` uses `ArcSwap<Vec<_>>` with copy-on-write updates; `NetManager` stores listener guards and no longer re-arms after each tunnel.
- Testing and testplan: consistent. `tunnel_listener_guard_callbacks` is mapped to unit and integration coverage.
- Implementation and tests: consistent. New unit tests cover repeated dispatch while guard is alive, guard-drop deletion, and no-callback false dispatch. Compile/test coverage migrates TCP/QUIC/PN listeners and fake listeners.

## Conclusion
- Passed.
- The approved proposal behavior is implemented and verified. No blocking document, implementation, or test evidence mismatch was found for `tunnel_listener_guard_callbacks`.

## Return Routing
- Proposal: none
- Design: none
- Implementation: none
- Testing: none

## Residual Risk
- The worktree contains substantial pre-existing unrelated dirty files, so stage-scope checks cannot be used as clean evidence for this single change.
- `TunnelListenerCallbackRegistry` intentionally does not cancel callback futures already taken from a snapshot before guard drop; this matches approved semantics.
