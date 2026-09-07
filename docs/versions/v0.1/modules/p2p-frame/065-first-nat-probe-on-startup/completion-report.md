# Completion Report: 065-first-nat-probe-on-startup

## Object and Scope

- Task manifest: task.yaml
- Workflow tier: standard
- Change record: docs/changes/065-first-nat-probe-on-startup.md

## Delivery Summary

- Outcome: 客户端 `SnService::start()`（p2p-frame/src/sn/client/sn_service.rs:1016）现在会设置 `first_report_pending`；`ping_proc` 首轮循环在已有 active SN 时跳过 600 秒 `latest_time` 刷新门控并立即 report，首测后再恢复正常周期。修改把“启动/首次 online 必测一次”从依赖随机路径变成启动标记保证。
- Handoff: 生产改动限 `p2p-frame/src/sn/client/sn_service.rs`；测试改动在 `p2p-frame/tests/unit/sn_tests/client/nat_probe_directive_tests.rs` 与 `p2p-frame/tests/nat_type_aware/sn_profile_flow_tests.rs`（后一处仅补充新状态字段初始化）。

## Proposal Consistency

| change_id | requirement_or_boundary | proposal_source | delivery_evidence | finding | status |
|-----------|-------------------------|-----------------|-------------------|---------|--------|
| CHG-startup-first-nat-probe | `SnService::start()` 后对当前 active SN 立即 report 并执行返回的 NAT probe directive，不受 600 秒 `latest_time` 门控影响；完成后恢复正常刷新节奏 | proposal.md | `first_report_pending` 标记 + `collect_due_active_sns(force_initial_report=true)` 强制选择最近 active SN，首测后 `latest_time` 回写为 now | 新增定向测试证明最近 active SN 在 force 下被选中；既有 NAT probe 流程与全量 lib 无回归 | pass |

## Independent Defect Discovery

| category | evidence_inspected | adversarial_check | finding_or_not_applicable_reason | status |
|----------|--------------------|------------------|----------------------------------|--------|
| behavior-and-logic | `start()` 设置标记、`ping_proc` 首轮清除并跳过 sleep、`collect_due_active_sns` 同时更新 `latest_time` | force=false 时最近 active 不被选择；force=true 时最近 active 被选择且 `latest_time` 更新；599s/600s/601s 边界断言 | 门控与首测选择逻辑清晰，未发现绕过或重复选择 | pass |
| boundaries-and-failure-paths | active 列表为空、已有 active、多 active、失败 report、无 directive 返回 | 首轮 force 仅执行一次；标记在锁内清除；活跃列表为空时维持原有立即候选 report；失败后不进入重试风暴 | 首测 report 若失败/无 directive，不会在当次启动自动重试，与提案的“失败 backoff/周期刷新语义不变”一致，非本任务缺陷 | pass |
| regression-and-side-effects | 既有 empty-start 首次 report、周期 directive、result-report failure、`SNServiceState` 构造点 | 新字段有初始值；`SNServiceState` 测试字面量补齐；全量 lib 套件 494 项 | `active_sn_profiles_are_kept_per_sn_id`、`sn_profile_flow_tests` 7 项、完整 lib 全部通过 | pass |

## Verification

- Targeted check: `cargo test -p p2p-frame --features x509 active_sn --lib`（9 项）；`cargo test -p p2p-frame --features x509 sn_profile_flow_tests --lib`（7 项）；`cargo test -p p2p-frame --features x509 --lib`（494 项）。
- Result: pass
- Exception reason: n/a

## Findings

| id | severity | evidence | problem | blocking |
|----|----------|----------|---------|----------|
| 065-F-001 | none | `collect_due_active_sns` 在 report 前更新 `latest_time` | 首测 report 若成功发起但失败，当次启动不会自动重试；沿用既有周期刷新语义，属于已知边界，不阻塞目标 | no |

## Conclusion

- Accepted / rejected / needs changes: accepted
- Reason: `start()` 启动标记在锁内设置并在首轮消费，已有 active SN 不再被 600 秒门控拖延；新增定向测试与完整 lib 套件均通过，未发现阻塞项。
