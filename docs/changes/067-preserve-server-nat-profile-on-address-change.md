# 地址变更后服务端保留旧 NAT 画像，不再先清空再被同一 report 恢复

- Status: complete
- Owner module: p2p-frame
- Task manifest: `docs/versions/v0.1/modules/p2p-frame/067-preserve-server-nat-profile-on-address-change/task.yaml`
- Approved proposal: `docs/versions/v0.1/modules/p2p-frame/067-preserve-server-nat-profile-on-address-change/proposal.md`
- Affected paths: `p2p-frame/src/sn/service/nat_probe_scheduler.rs`, `p2p-frame/tests/unit/sn_tests/service/service/nat_probe_scheduler_tests.rs`
- Explicit tier override: none
- Expanded high-risk packet: none

## Approach

`handle_report_sn` 在同一请求内先调用 `observe_capable_report` 再合并 `ReportSn.net_profile`。当客户端外网地址变化而本地画像尚未过期时，`observe_capable_report` 会把 `profile_update` 置为 `Some(None)` 并重建 profile 状态，随后 `observe_reported_profile` 又接受旧画像，覆盖失效通知，形成“先清空后立即恢复”的振荡。

修复只在 `NatProbeScheduler` 的地址变更分支做最小改动：

- `observe_capable_report`：had_registration 且地址变化时保留现有 profile，不再写 `profile_update = Some(None)`；仍推进 registration generation 并保持 `pending_trigger=ExternalAddress`，让同一 capable report 继续下发 directive。
- `observe_control`：地址变化时同样保留现有 profile，不再写失效 transition；保持“不立即在此下发 directive、依赖后续 report”的既有行为。

同一条 `ReportSn.net_profile` 携带的旧画像因此只能作为“当前画像”被保留或后续更新，不会覆盖一个即将生效的失效通知。端口配置变更等其它清除路径维持现状。

## Risk Screen

- Public contract, protocol, or CLI change: no
- Persistent data, schema, or migration change: no
- Security, privacy, or trust-boundary change: no
- Concurrency, lifecycle, or runtime integration change: yes，地址变更后 NAT 画像发布时序变化，窗口内旧画像可继续被 query/download 使用，直到客户端上报新值
- Material dependency/build graph, supply-chain trust, produced artifact, production default/feature rollout, release/deployment, compatibility, or rollback impact: no
- Material UI, accessibility, localization, or navigation workflow change: no
- Harness rule, checker, or test-infrastructure change: no
- Cross-project or architectural boundary change: no

## Verification

- Targeted check:
  - `cargo test -p p2p-frame --features x509 nat_probe_scheduler --lib`：14 项通过，含新增 `nat_probe_scheduler_external_address_report_keeps_old_profile_until_new_result` 与调整后的地址变化/control 保留画像测试。
  - `cargo test -p p2p-frame --features x509 --lib`：498 项通过。
  - `cargo test -p p2p-frame --test nat_probe_logging_contract`：3 项通过。
- Result: pass
- Residual risk or follow-up: 地址变化到客户端新值上报之间，query/detail 可能继续使用基于旧地址的 NAT 画像；周期/directive 上界为 2h/约 30s 窗口。
