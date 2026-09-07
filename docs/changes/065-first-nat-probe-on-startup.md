# 客户端启动/首次 online 强制立即执行 NAT 探测

- Status: complete
- Owner module: p2p-frame
- Task manifest: `docs/versions/v0.1/modules/p2p-frame/065-first-nat-probe-on-startup/task.yaml`
- Approved proposal: `docs/versions/v0.1/modules/p2p-frame/065-first-nat-probe-on-startup/proposal.md`
- Affected paths: `p2p-frame/src/sn/client/sn_service.rs`, `p2p-frame/tests/unit/sn_tests/client/nat_probe_directive_tests.rs`, `p2p-frame/tests/nat_type_aware/sn_profile_flow_tests.rs`
- Explicit tier override: none
- Expanded high-risk packet: none

## Approach

在 `SnService::start()` 设置客户端首次启动探测标记；`ping_proc` 首轮循环发现该标记时跳过 600 秒 `latest_time` 刷新门控并立即对当前 active SN 发 report，首测请求后清除标记。空 active 列表路径保持原有“立即候选 report”节奏；非空 active 路径因此不再等到 600 秒刷新点。首测启动的 active SN `latest_time` 同步更新到当前时间，之后恢复既有 600 秒周期与失败 backoff 语义。

## Risk Screen

- Public contract, protocol, or CLI change: no
- Persistent data, schema, or migration change: no
- Security, privacy, or trust-boundary change: no
- Concurrency, lifecycle, or runtime integration change: yes
- Material dependency/build graph, supply-chain trust, produced artifact, production default/feature rollout, release/deployment, compatibility, or rollback impact: no
- Material UI, accessibility, localization, or navigation workflow change: no
- Harness rule, checker, or test-infrastructure change: no
- Cross-project or architectural boundary change: no

`start()` 与 `ping_proc` 之间新增一个布尔首测标记；标记的写入/清除与 `latest_time` 更新同处 `SNServiceState` 写锁临界区，避免首测与周期刷新并发选择同一 active SN 重复 report。影响范围仅客户端启动/首次 online 时序，不改写服务端或 wire 契约。

## Verification

- Targeted check: `cargo test -p p2p-frame --features x509 active_sn --lib`（9 项，含新增 `active_sn_due_report_force_initial_probe_bypasses_refresh_gate` 与 `collect_due_active_sns_force_initial_reports_recent_active_sn`）；`cargo test -p p2p-frame --features x509 sn_profile_flow_tests --lib`（7 项 NAT probe/online 流程）；`cargo test -p p2p-frame --features x509 --lib`（494 项全量 lib）。
- Result: pass
- Residual risk or follow-up: 本任务不改 `wait_online()` 首次探测完成才 online 的语义；若服务端首报未携带 directive，仍按正常周期等待下一条周期性 directive，属于既有服务端/失败路径行为。
