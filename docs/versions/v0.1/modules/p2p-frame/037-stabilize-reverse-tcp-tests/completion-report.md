# Completion Report: 037-stabilize-reverse-tcp-tests

## Object and Scope

- Task manifest: task.yaml
- Workflow tier: standard
- Change record: docs/changes/037-stabilize-reverse-tcp-tests.md

## Delivery Summary

- Outcome: 两个点名 reverse TCP 失败已修复。端口侧用测试专用 `TestTcpPortGuard` 消除 reserve-drop 窗口；PN 侧把 source FakeTunnel 的 attach 移到 B control tunnel 变为 cache-ready 之后，避免 `remember_tunnel_in_multi` 的 production prune 在 B 仍 `Connecting` 时删除其缓存项，同时增加 `#[cfg(test)]` 缓存快照与 accept progress 用于失败定位。生产 TTP cache、TCP 状态机、PN 请求语义均未改变。
- Handoff: 完整工作区既有未提交改动保持原样；本任务只交付上述测试面调整。

## Proposal Consistency

| change_id | requirement_or_boundary | proposal_source | delivery_evidence | finding | status |
|-----------|-------------------------|-----------------|-------------------|---------|--------|
| reverse_tcp_test_port_guard | endpoint 从选定到网络 bind 完成期间必须由 guard 持有，消除并发抢占窗口 | proposal.md P-037-PORT 行 | `TestTcpPortGuard`（Linux `SO_REUSEADDR`，非 listen socket）+ 两个 reverse 测试接线；反向组 20 用例 3 次通过、完整套件多次通过 | guard 仅在传入 socket bind 后 drop；无 bind 重试/吞错 | pass |
| pn_cache_readiness_deterministic_sync | 唯一 proxy request 前等到 B control tunnel cache-ready；超时必须输出可定位快照 | proposal.md P-037-PN 行 | 事件/低频率等待 + `TtpCacheTunnelSnapshot`/`accept_progress_for_test`；source FakeTunnel 在 cache-ready 之后才 attach | 先前必现的 7 测试 8 线程组合 5/5 通过，快照不再为空 | pass |

## Independent Defect Discovery

| category | evidence_inspected | adversarial_check | finding_or_not_applicable_reason | status |
|----------|--------------------|------------------|----------------------------------|--------|
| behavior-and-logic | `remember_tunnel_in_multi` prune 时序、`accept_incoming_tunnel` 阶段计数、测试顺序 | 假设缓存空 = 未 attach，用 accept progress 证伪；确认 prune 删除 Connecting 中 B 条目，随后按证据重排测试 | 根因不在 5 秒 deadline，而在 source FakeTunnel 的 remember prune；重排后无需延长 bound | pass |
| boundaries-and-failure-paths | guard 生命周期、监听/连接共存语义、失败日志 | 本地 socket2 实验验证 bound+bound、connected+listener 组合；guard 不吞 `AddrInUse` | 端口窗口消除；若 bind 失败仍会显式失败 | pass |
| regression-and-side-effects | PN 精确/并发压力、reverse 组、ttp 模块、完整 lib 套件 | 并发重复运行（7 测试 5 次、reverse 组 3 次、完整套件多轮）对比修复前后 | 点名两个用例稳定通过；未发现生产语义变化 | pass |

## Verification

- Targeted check: 7-test 8-thread 复现集 5/5；`reverse_data_first_claim` 组 3/3（20 用例）；PN exact + parallel-pressure；`ttp::` 22；full `--features x509 --lib --test-threads=4` 424/424；full default 一轮 424/424。
- Result: pass
- Exception reason: 一轮 full default 曾出现与任务无关的 `sn_profile_flow_tests` 稀有 `AddrInUse`（`sn/tests.rs` `NEXT_PORT` 非预留范围），随后整轮复跑 424/424；按规则不并入本任务，见 Findings。

## Findings

| id | severity | evidence | problem | blocking |
|----|----------|----------|---------|----------|
| 037-F-001 | none | 修复前的必现 7 测试组合与快照 `accept_progress: 4`/空缓存，修复后 5 次全过 | 已定位并修复；测试顺序重排消除了生产 prune 导致的 cache 丢失 | no |
| 037-F-002 | none | full default 首轮 `sn_profile_flow_tests::tcp_only_registration_never_receives_or_executes_probe` `AddrInUse`（同一轮第二次执行即通过） | SN 侧固定/顺序端口不与 OS 预留，独立于本任务范围，属于未完成 task 032 `sn_test_bind_conflict_recovery`（P-032-2）；建议后续完成 032 或 sibling | no |
| 037-F-003 | none | 424 全量多轮 + 定向组通过 | 未发现阻塞性缺陷 | no |

## Conclusion

- Accepted / rejected / needs changes: accepted
- Reason: 两个点名失败各有确定 red → green 证据，定向与全量验证通过；独立缺陷发现未发现生产行为回归；SN 端口残留已如实记录并归属 task 032。
