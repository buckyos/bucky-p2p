# Completion Report: 066-client-owned-nat-probe

## Object and Scope

- Task manifest: task.yaml
- Workflow tier: standard
- Change record: docs/changes/066-client-owned-nat-probe.md

## Delivery Summary

- Outcome: NAT 探测节奏改为客户端主导。客户端 ActiveSN 新增 `next_probe_at`，启动/上线时若服务端未下发 directive 则用本地 `nat_probe_endpoints` + `nat_probe_signer` 直接执行探测，并在完成后把下次探测按完成时间排到 2 小时后；周期探测先于 600 秒 report 执行，`ReportSn.net_profile` 随本次上报携带最新画像；指令/本地探测完成即写入本地 `net_profile` 并推进 `next_probe_at`，不依赖结果上报成功。服务端消费 `ReportSn.net_profile` 更新 scheduler profile，query/detail 继续从 scheduler 返回。
- Handoff: 生产改动在 `p2p-frame/src/sn/client/sn_service.rs`、`p2p-frame/src/sn/service/nat_probe_scheduler.rs` 与 `p2p-frame/src/sn/service/service.rs`，服务端 `handle_query_sn`/`handle_call_sn` 的 demand 挂载点已移除；测试覆盖客户端本地探测前置、report 失败保留本地画像、服务端地址变化-only 语义以及服务端接受客户端周期画像。

## Proposal Consistency

| change_id | requirement_or_boundary | proposal_source | delivery_evidence | finding | status |
|-----------|-------------------------|-----------------|-------------------|---------|--------|
| CHG-client-owned-nat-probe-schedule | 客户端启动/2 小时间隔，周期探测先于 report 且结果回传/本地并存，不依赖 report 成功或服务端 directive | proposal.md | `probe_local_if_due()` 在 `report()` 前执行并回填；`apply_completed_probe()` 与结果上报解耦 | 新增 `periodic_local_probe_runs_before_report_and_preserves_local_profile_on_report_failure` 证明 report 失败时本地画像与 `next_probe_at` 仍更新 | pass |
| CHG-server-address-change-only-directive | 服务端仅由新 authority/外网地址变化下发指令，移除周期与 demand 主动下发 | proposal.md | scheduler 仅 `pending_trigger` 驱动；`mark_demand`/`force_periodic_due`/`NAT_PROBE_FAILURE_BACKOFF` 已移除 | 调度器定向测试证明周期/reschedule 不再下发，地址变化仍 trigger=external_address | pass |

## Independent Defect Discovery

| category | evidence_inspected | adversarial_check | finding_or_not_applicable_reason | status |
|----------|--------------------|------------------|----------------------------------|--------|
| behavior-and-logic | `probe_local` 状态回填（非 Unknown profile + `next_probe_at` 推进）、调度边界（到期 report/到期 probe 分离）、scheduler 仅 online/external 下发 | 无 directive、服务端稳定报告、失败/超时、全局容量、地址变化；启动测试改为等待状态回填后按强制到期验证下一轮探测 | 未发现本地探测被 directive 缺失阻塞，也没有周期服务端指令残留；P2 指出的启动测试竞态已修复 | pass |
| boundaries-and-failure-paths | 空 active 启动、已有 active、无 signer、TCP only、服务端无 directive | signer 缺失返回 Unknown；服务端稳定上报不再触发指令；超时后不自动重试 | 与用户确认的职责边界一致；端口配置变更强制重测保持 non-goal | pass |
| regression-and-side-effects | ActiveSN 构造点、SNServiceState 测试字面量、调度器测试、logging contract | 全量 lib 唯一失败为既有 `sn_report_rejects_wrong_business_seq_and_sn_identity` AddrInUse 端口占用 flake，单独重跑通过 | 未发现本次改动导致的回归 | pass |

## Verification

- Targeted check: `sn_profile_flow_tests --lib` 9 项；`nat_probe_directive --lib` 6 项；`nat_probe_scheduler --lib` 13 项 + 新增 `scheduler_publishes_fresh_client_profile_and_ignores_stale_or_unknown` 单独通过；`nat_probe_logging_contract` 3 项；另 `cargo check --features x509 --lib` 通过。
- Result: pass
- P2 fix recheck: `cargo test -p p2p-frame --features x509 sn_profile_flow_tests --lib` reruns 9/9 green, and the startup test now waits for state backfill and verifies the next-round scheduling boundary.
- Exception reason: n/a

## Findings

| id | severity | evidence | problem | blocking |
|----|----------|----------|---------|----------|
| 066-F-001 | none | 全量 lib 并行/串行均出现过 `sn_report_rejects_wrong_business_seq_and_sn_identity` AddrInUse | 既有 SN 测试端口分配在部分运行环境下与残留监听/TIME_WAIT 冲突；单独重跑通过，与本次改动无关 | no |
| 066-F-002 | none | `probe_local` 仅在 ActiveSN 有可用 endpoints/signer 时执行 | 若客户端首次上报未带回 `nat_probe_ports`/SN peer_info，本地探测退化为 Unknown；沿用服务端必须提供端口和证书的前置条件 | no |
| 066-F-003 | fixed | review 指出周期探测仍在 600 秒 report 成功分支内、并先探测后推进 | 已改为 `probe_local_if_due()` 在 report 前用已保存快照执行，`next_probe_at` 在探测完成后按完成时间推进；报告失败不再阻塞本地探测 | no |
| 066-F-004 | fixed | review 指出结果上报失败会丢弃已测得画像并延后 2 小时再测 | 指令/本地探测完成即 `apply_completed_probe()` 落盘 `net_profile` 与 `next_probe_at`；`ReportSn.net_profile` 随后续 report 携带，服务端接收并更新 scheduler profile | no |
| 066-F-005 | fixed | P2 review 指出新增启动测试等到 `local_started` 后立即断言终态日志存在，存在异步竞态；允许 `local_failed` 通过，且未断言 `net_profile`、`next_probe_at` 或下一轮到期行为 | 启动测试改为等待 `ActiveSN.net_profile` 非 Unknown 且 `next_probe_at > now` 的状态回填，终态必须 `local_completed`；新增只到期 report 的测试 hook 验证 2 小时探测边界前不重复探测，再强制探测到期验证下一轮本地探测与重新排程 | no |

## Conclusion

- Accepted / rejected / needs changes: accepted
- Reason: 客户端研发周期与服务端地址变化驱动的指令边界已落地，且 review 指出的两处“依赖 report 成功”缺陷已修复；定向与回归测试覆盖 report 失败、本地画像保留和下一次随 report 上报；端口占用 flake 为既有测试环境问题。
- P2 fix scope: review 指出的新增启动测试异步竞态、允许 `local_failed` 通过、未验证 `net_profile`/`next_probe_at`/下一轮到期行为均已修复，启动和周期套件复跑全绿。
