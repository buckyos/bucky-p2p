# 客户端主导 NAT 探测周期、服务端只按地址变化下发指令

- Status: complete
- Owner module: p2p-frame
- Task manifest: `docs/versions/v0.1/modules/p2p-frame/066-client-owned-nat-probe/task.yaml`
- Approved proposal: `docs/versions/v0.1/modules/p2p-frame/066-client-owned-nat-probe/proposal.md`
- Affected paths: `p2p-frame/src/sn/client/sn_service.rs`, `p2p-frame/src/sn/service/nat_probe_scheduler.rs`, `p2p-frame/src/sn/service/service.rs`, `p2p-frame/tests/nat_type_aware/sn_profile_flow_tests.rs`, `p2p-frame/tests/nat_probe_logging_contract.rs`, `p2p-frame/tests/unit/sn_tests/service/service/nat_probe_scheduler_tests.rs`
- Explicit tier override: none
- Expanded high-risk packet: none

## Approach

把 NAT 探测节奏收归客户端：ActiveSN 增加客户端本地探测定时器，启动/上线后立即用已有 `nat_probe_endpoints` 与 `nat_probe_signer` 本地执行一次探测，完成后下一次为 `now + 2h`。周期探测在 600 秒 report 之前执行，并通过 `ReportSn.net_profile` 随本次上报携带最新画像；指令探测完成后先落盘本地画像与 `next_probe_at`，再回传结果，结果上报失败不再丢弃已测得画像。`ReportSn.net_profile` 由服务端接收并更新 `NatProbeScheduler` profile，query/detail 继续从 scheduler 返回。服务端调度器移除周期与纯 demand 主动下发，只在新 authority 建立或观察到外网地址变化时下发 directive。

## Risk Screen

- Public contract, protocol, or CLI change: no
- Persistent data, schema, or migration change: no
- Security, privacy, or trust-boundary change: no
- Concurrency, lifecycle, or runtime integration change: yes
- Material dependency/build graph, supply-chain trust, produced artifact, production default/feature rollout, release/deployment, compatibility, or rollback impact: no
- Material UI, accessibility, localization, or navigation workflow change: no
- Harness rule, checker, or test-infrastructure change: no
- Cross-project or architectural boundary change: no

客户端本地探测结果随普通 report 携带，并在本地持久保存；即使本次 report/结果上报失败，`ActiveSN.net_profile` 与 `next_probe_at` 也不会回退，下次 report 继续携带最新画像。客户端本地探测与服务端 directive 探测不同时由同一周期触发，避免同一时刻重复探测。

## P2 Defect Fix

修复新增启动测试的异步竞态与周期回填验证缺失：

- 启动测试不再以 `event=nat_probe_client_local_started` 作为等待完成信号，而是等 `ActiveSN.net_profile` 回填为非 Unknown、`next_probe_at` 已推进到未来后再做终态断言。
- 终态必须为 `local_completed`，不再允许 `local_failed` 通过，并断言启动阶段只启动一次本地探测。
- 新增测试 hook `force_active_sn_report_due_for_test`，只把报告周期置为到期而不修改 `next_probe_at`，证明 2 小时探测边界前的 report 周期不会重复探测；随后强制探测到期，证明下一轮本地探测会执行并重新排程 `next_probe_at`。

## Verification

- Targeted check:
  - `cargo test -p p2p-frame --features x509 active_sn --lib`：9 项通过
  - `cargo test -p p2p-frame --features x509 sn_profile_flow_tests --lib`：9 项通过，含新增 `startup_report_without_server_directive_runs_client_local_probe`、`periodic_local_probe_runs_before_report_and_preserves_local_profile_on_report_failure`；启动测试已按下述 P2 修复等待状态回填并验证调度边界
  - `cargo test -p p2p-frame --features x509 nat_probe_directive --lib`：6 项通过
  - `cargo test -p p2p-frame --features x509 nat_probe_scheduler --lib`：13 项通过；另有 `scheduler_publishes_fresh_client_profile_and_ignores_stale_or_unknown` 单独通过
  - `cargo test -p p2p-frame --test nat_probe_logging_contract`：3 项通过
- Result: pass
- Residual risk or follow-up: 端口配置变更不触发强制重测，保持 non-goal；`ReportSn.net_profile` 的陈旧/Unknown 画像会被服务端忽略，避免覆盖更新的 scheduler profile。
