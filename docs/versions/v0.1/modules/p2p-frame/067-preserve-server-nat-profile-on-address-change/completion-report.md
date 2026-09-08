# Completion Report: 067-preserve-server-nat-profile-on-address-change

## Object and Scope

- Task manifest: task.yaml
- Workflow tier: standard
- Change record: docs/changes/067-preserve-server-nat-profile-on-address-change.md

## Delivery Summary

- Outcome: 服务端观察到客户端外网地址变化时，不再先清空旧 NAT 画像、也不下发 `profile_update=Some(None)` 的失效通知；`NatProbeScheduler` 在 `observe_capable_report` 与 `observe_control` 的地址变化分支保留当前 profile，仍推进 registration generation 并让 capable report 继续发出 external_address directive。同一 `ReportSn.net_profile` 携带的旧画像因此只会保留或后续更新，不会覆盖一个即将生效的失效通知。
- Handoff: 生产改动集中在 `p2p-frame/src/sn/service/nat_probe_scheduler.rs` 的地址变化重建逻辑；`handle_report_sn` 合并逻辑保持不变。测试覆盖地址变化报告保留旧画像、control 路径地址变化保留画像、以及 directive 结果到达后替换为新画像。

## Proposal Consistency

| change_id | requirement_or_boundary | proposal_source | delivery_evidence | finding | status |
|-----------|-------------------------|-----------------|-------------------|---------|--------|
| CHG-preserve-nat-profile-on-address-change | 外网地址变化时 `observe_capable_report`/`observe_control` 不清除/不发布失效，保留当前画像；directive 仍由 capable report 经 pending_trigger 发出 | proposal.md | `observe_capable_report` 与 `observe_control` 在 had_registration 时从旧状态拷贝 `profile`，且仅在新注册时写 `profile_update=Some(None)`；地址变化分支保留 pending_trigger | 新增 `nat_probe_scheduler_external_address_report_keeps_old_profile_until_new_result` 证明旧画像保留、directive 仍发出、新结果到达后替换；control 测试改断言保留画像 | pass |

## Independent Defect Discovery

| category | evidence_inspected | adversarial_check | finding_or_not_applicable_reason | status |
|----------|--------------------|------------------|----------------------------------|--------|
| behavior-and-logic | 地址变化重建分支、profile_update 合并、current_profile 过滤新鲜度、directive in_flight 生命周期 | 检查地址变化分支是否意外清除 in_flight 或丢失 control_supported；核对 `observe_reported_profile` 在地址变化后不会被 `Some(None)` 覆盖；验证 directive 结果到达后新画像替换 | 地址变化仍清 in_flight（generation 已更新，旧请求合法失效），control_supported 沿用当前 report 值；保留旧画像只影响发布时序，不影响 registration generation 或 directive 周期 | pass |
| boundaries-and-failure-paths | 首次注册 vs 地址变化、TCP ineligible、端口配置变更 `set_ports`、directive timeout、capability lost | 确认首次注册仍会 `Some(None)` 清空；TCP/端口配置/超时失效路径未被改动；地址变化期间 profile TTL 由 `current_profile` 新鲜度压制 | 端口配置变更仍由 `set_ports` 统一失效，`expire_due`/`finish_expired` 超时路径仍清 profile；本改动不扩大失效面 | pass |
| regression-and-side-effects | 调度器 14 项单测、全量 lib 498 项、`nat_probe_logging_contract` 3 项 | 检查日志断言是否还依赖 `reason=external_address_changed` 的 profile_invalidated 事件；核对 NAT 日志契约未因删日志而失败 | `nat_probe_logging_contract` 只要求存在 `nat_probe_profile_invalidated` 事件（由 timeout 等路径提供），不要求 external_address 清空日志，全部通过；未发现本次改动引入回归 | pass |

## Verification

- Targeted check:
  - `cargo test -p p2p-frame --features x509 nat_probe_scheduler --lib`：14 项通过
  - `cargo test -p p2p-frame --features x509 --lib`：498 项通过
  - `cargo test -p p2p-frame --test nat_probe_logging_contract`：3 项通过
- Result: pass
- Exception reason: n/a

## Findings

| id | severity | evidence | problem | blocking |
|----|----------|----------|---------|----------|
| 067-F-001 | none | 全量 lib 运行中未出现与本次改动相关的失败；498 项全绿 | 未发现新增缺陷 | no |
| 067-F-002 | none | 地址变化到新值上报之间，`current_profile` 会继续返回旧画像 | 这是提案确认的取舍，上界为客户端周期 2h 或 directive 约 30s 窗口；后续若网络切换场景需要更激进失效可再独立评估 | no |

## Conclusion

- Accepted / rejected / needs changes: accepted
- Reason: 已按提案修复地址变化时“先清空后被同一 report 恢复”的振荡，调度器定向测试、全量 lib 与日志契约全部通过；仅保留提案确认的“旧画像在窗口内可继续查询”取舍，无阻塞项。
