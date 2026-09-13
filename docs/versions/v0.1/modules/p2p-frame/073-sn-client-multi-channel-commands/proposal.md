---
task_manifest: task.yaml
status: approved
approved_by: user
approved_at: 2026-09-13T13:48:00+08:00
approved_content_sha256: 1ddec21aaa5275cb08973ea0f70de33debb2e918142de5debb5b117fb1216c1e
---

# P2P Frame Proposal

Risk profile: ./risk-profile.yaml

## Workflow Tier Judgment

- Proposed tier: high-risk
- Final tier: high-risk
- Tier rationale / triggered boundaries: this changes SN command dispatch across multiple authenticated command streams multiplexed over TTP tunnels, adds creation-on-demand behavior under the existing command-stream cap, changes active-SN identity/lifecycle matching, removes the public `ActiveSN.conn_id` field, and makes QA timeout close the exact used command stream without closing the bearer TTP tunnel. These are material runtime concurrency, connection lifecycle, authentication binding, public-source compatibility, and migration risks.
- Proposal and tier confirmation: approved by the user on 2026-09-13 with instruction to execute the high-risk flow automatically.

## Background and Goal

SN commands currently pin requests to the single `ActiveSN.conn_id`, which identifies one command stream opened through TTP control-stream multiplexing. A slow `ReportSn`, `SnCall`, `SnQuery`, or `SnTunnelRendezvous` therefore holds that command-stream guard while waiting for its response, so later commands using the same SN are serialized behind it. `SnCalledResp` is also sent through that pinned command stream. Although the command-client pool supports multiple command streams and can create one on demand, the SN call sites bypass that scheduling by requesting the exact pinned stream.

`ActiveSN.conn_id` mixes SN identity state with transport state. Once command dispatch is keyed by authenticated SN peer identity and classified command streams, the active-SN record should not own a pinned stream handle. A transport-level stream handle still exists for each request and for timeout cleanup, but it belongs to the command client/request context, not to `ActiveSN`.

The goal is to let the complete SN client/server command surface use multiple command streams. SN-client-initiated commands may use any idle authenticated command stream to the serving SN; when none exists and the configured command-stream limit is not reached, the client opens a new command stream over an available TTP tunnel (creating the TTP bearer only when no bearer exists). At the configured limit, the client waits for an available command stream rather than creating unbounded connections. Server-initiated commands distribute across command streams already created by the client.

This is supported by the current implementation: `SnClientTunnelFactory::open_cmd_tunnel` calls `TtpClient::open_control_stream`; `TtpClient` first reuses or creates a bearer TTP tunnel, then opens a new multiplexed control stream on that bearer. Repeated calls can therefore create independent SN command streams over the same TTP tunnel, and the `sn_tunnel_count` setting caps these command streams rather than the number of bearer TTP tunnels.

Therefore, the core fix is localized to the SN command layer in `p2p-frame`: stop selecting requests by a pinned `ActiveSN.conn_id` and use the existing classified command-stream pool instead. No source change is expected in `p2p-frame/src/ttp/**` or in the `sfo-cmd-server` dependency. The current dependency already exposes `get_send_by_classified(...)` for idle-or-create pool selection, and the selected guard exposes `set_disable()` so QA timeout cleanup can close only that command stream.

## Scope

### In scope

- Route every SN-client-initiated command through the existing classified command-stream pool using a serving-SN classification rather than pinning requests to `ActiveSN.conn_id`. This includes `ReportSn`, `SnCall`, `SnQuery`, `SnTunnelRendezvous`, and `SnCalledResp`.
- Remove `ActiveSN.conn_id`. Bind command streams to the authenticated SN peer identity rather than treating a stream handle as active-SN identity state.
- Reuse idle authenticated command streams over an available TTP tunnel before opening another command stream; create a new TTP bearer only when no usable bearer exists. Multiple command streams over one bearer must be able to run concurrently.
- Consume the existing `sfo-cmd-server` classified client APIs; do not modify TTP transport behavior or the dependency version.
- Keep QA response frames on the same command stream as their request because the command runtime correlates `(command stream, command, seq)`; do not change the wire format to move responses onto another stream.
- Allow server-initiated `SnCalled` and `SnTunnelRendezvousNotify` commands to arrive on any authenticated command stream belonging to that SN peer.
- Allow server-initiated `SnTunnelRendezvousNotify` QA to select among the target client's existing command streams instead of repeatedly choosing one pinned stream; the server consumes command streams created by the client and does not create client command streams itself.
- Close the exact command stream used by a QA request when that request times out. This applies to client-initiated `ReportSn`, `SnCall`, `SnQuery`, and `SnTunnelRendezvous`, and server-initiated `SnTunnelRendezvousNotify`. Do not close the bearer TTP tunnel merely because one QA times out; other command streams over that TTP tunnel must remain usable.
- Preserve or adapt response validation so business `seq`, command code, command-stream identity, and SN peer identity remain mandatory.
- Keep failure and timeout paths bounded: failed command streams are not reused, timed-out QA command streams are closed before reuse, pending requests remain individually cancellable, and active-SN state is not polluted by late or mismatched responses.
- Closing or timing out one command stream removes only that stream; active-SN identity remains valid while another healthy stream exists.
- Cover all SN command flows, concurrent QA dispatch, command-stream creation on demand, the configured command-stream cap, timeout and command-stream closure, bearer-tunnel survival, transport failure, cancellation, mismatched responses, and rendezvous notification authentication.

### Out of scope

- Do not remove the existing command-stream cap (currently configured as `sn_tunnel_count`) or make command-stream creation unbounded.
- Do not close the bearer TTP tunnel merely because a QA over one of its command streams times out.
- Do not change SN command wire formats, command versions, business codecs, or authentication.
- Do not remove transport-level stream identity from QA correlation or timeout cleanup; remove only the persistent `ActiveSN.conn_id` field.
- Do not replace the command QA runtime with an application-level pending map.
- Do not change SN-to-peer payload tunnels, rendezvous action strategy, NAT probe semantics, or PN behavior.
- Do not provide mixed-version compatibility claims beyond the existing protocol boundary.
- Do not keep a timed-out QA command stream alive for reuse.
- Do not modify `p2p-frame/src/ttp/**` or change/patch the `sfo-cmd-server` dependency.

### Boundary with neighboring modules

- `p2p-frame/src/sn/client` owns command dispatch and active-SN lifecycle changes.
- TTP owns bearer-tunnel multiplexing and is consumed unchanged; SN owns each command stream and closes only the timed-out command stream on QA timeout.
- `p2p-frame/src/sn/service` owns server-side selection among command streams already accepted from a client and SN-peer lifecycle matching.
- `sfo-cmd-server` remains responsible for classified command-stream pooling, on-demand command-stream creation, QA correlation, and command-stream guard release through its current API; this task consumes it but does not modify it.
- The command client remains the owner of transport stream handles; `ActiveSN` remains the owner of authenticated peer/profile lifecycle state.
- `sn-miner-rust` and other consumers continue to configure the maximum SN command-stream count through `P2pStackConfig::sn_tunnel_count`.

## Requirement Review

The request is reasonable. The existing command runtime already supports classified command streams with reuse and bounded on-demand creation; the current problem is that SN client call sites select the exact `ActiveSN.conn_id`. Using a classification keyed to the serving SN avoids one slow command monopolizing that SN's command traffic while retaining authentication and configured capacity limits. On timeout, the correct cleanup unit is that command stream, not the TTP bearer tunnel. Existing public APIs are sufficient, so no TTP or `sfo-cmd-server` source/version change is required.

The main tradeoff is that active-SN state can no longer treat one pinned command stream as the only valid transport. Lifecycle updates and inbound-command acceptance must be keyed by authenticated SN peer identity, with per-request command-stream and sequence validation retained. Removing `ActiveSN.conn_id` is a source-breaking public API change, so repository consumers and tests must be migrated and closed.

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-001 | CHG-sn-client-classified-command-channel | Route all SN-client-initiated commands through classified command streams | Reuse an idle authenticated command stream; open a new command stream only when no idle stream is available and the configured cap is not reached; create a TTP bearer only when none exists; wait when at the cap; no TTP or sfo-cmd-server source change | Adds more simultaneous command streams over TTP multiplexing | Regression proves a slow QA does not block another command, the pool opens a new command stream instead of waiting for the pinned stream, and two streams can use the same TTP bearer concurrently | Unbounded stream creation |
| P-002 | CHG-sn-command-active-identity-lifecycle | Key active-SN acceptance and lifecycle updates to authenticated SN peer identity | Per-request command code, business sequence, SN identity, and transport-failure cleanup remain mandatory; late or mismatched responses are rejected; QA responses stay on their request stream | Relaxes the old single-conn-id binding while preserving authenticated identity binding | Tests prove commands/responses/notifications on any valid SN command stream are accepted only after identity and sequence validation, and mismatched/late messages do not pollute state | Removing response validation or stream cleanup |
| P-003 | CHG-sn-server-command-stream-selection | Distribute server-initiated QA across the target client's existing command streams | Do not create command streams from the server; selection must not repeatedly pin all server QA to one client command stream | Requires server-side selection state without changing wire correlation | Tests prove two concurrent server QA notifications use distinct available client command streams when possible | Changing response correlation or server-initiated stream creation |
| P-004 | CHG-qa-timeout-command-stream-close | Close the exact command stream used by a timed-out QA request | Timeout closes that command stream before reuse; the bearer TTP tunnel remains available; late responses cannot complete or restore state; other healthy command streams remain available; use the existing classified guard's `set_disable()` | A transient timeout now sacrifices the command stream to avoid a stale or monopolized channel | Tests prove each QA timeout disables/removes its command stream, preserves the bearer tunnel, cleans associated state, leaves other streams usable, and rejects a late response | Keeping timed-out command streams alive, closing the bearer tunnel for one QA timeout, or adding a new dependency API |
| P-005 | CHG-remove-active-sn-conn-id | Remove `ActiveSN.conn_id` and key active-SN state by authenticated peer identity | Per-request transport handles remain internal; response/timeout cleanup still uses the selected stream identity; a stream failure does not invalidate an active SN that has another healthy stream | Removes single-stream ownership and changes the public `ActiveSN` shape | Repository consumer migration plus regressions prove active-SN identity, multi-stream dispatch, timeout cleanup, and stale-response rejection without `conn_id` | Keeping a persistent pinned stream handle in active-SN state |

## Success Criteria

- Concrete user-visible or system-visible result: a long-running SN QA no longer prevents another SN command from using another idle or newly created command stream; concurrent commands can run over distinct streams multiplexed by the same TTP bearer; when a QA times out, its stream is closed while the bearer remains usable; active-SN state no longer exposes a pinned stream handle; and server-initiated QA is not repeatedly funneled into one client stream.
- Required evidence: targeted all-command dispatch tests, concurrent QA tests, same-bearer multi-stream tests, command-stream creation tests, cap/timeout/command-stream-closure tests, bearer-tunnel survival tests, transport-failure tests, response-correlation tests, rendezvous notification tests, repository consumer migration closure, and relevant `p2p-frame` task tests.
- Explicit non-goals: no wire-format change, no unbounded connections, no application-level QA waiters, and no change to rendezvous action semantics.

## Risks

- Concurrent use of multiple command streams can expose stale active-SN state, duplicated reports, or races in lifecycle updates.
- Relaxing `conn_id` matching without retaining peer identity and per-request validation could accept a response or notification from the wrong authenticated peer.
- On-demand command-stream creation can increase SN load if the cap, cancellation, and failure cleanup are incorrect.
- Timeout cleanup must close the exact command stream and cannot inadvertently terminate the TTP bearer tunnel or other command streams.
- A transport failure on one channel must not incorrectly invalidate a healthy channel or leak state.
- Timeout cleanup must not delete a newer active-SN registration or another healthy connection's state.
- Removing `ActiveSN.conn_id` must not weaken stale-response rejection or accidentally delete an active SN that still has healthy command streams.

## Approval Record

- approver: user
- approval_date: 2026-09-13
- user_statement: 确认该提案与 high-risk 分级，按 high-risk 流程自动执行到验证与收尾
