# Completion Report: 036-fix-test-lifecycle-readiness

## Object and Scope

- Task manifest: task.yaml
- Workflow tier: standard
- Change record: docs/changes/chg-036-quic-ttp-readiness.md

## Delivery Summary

- Outcome: TTP 测试 readiness 观察改为只读 bucket 扫描，`Connecting` tunnel 不再被观察器
  prune；QUIC owner-lifecycle 回归改为 1 秒有界轮询验证 `AbortOnDropTask` 对 worker task
  的 abort。三个用户点名失败与 PN 并发压力用例均已通过。
- Handoff: 当前工作区仍按原状态保留所有未提交改动；交付只涉及上述测试/只读观察边界，
  未改变 production TTP cache、QUIC connect/punch 状态机或任何 wire/CLI 契约。

## Proposal Consistency

| change_id | requirement_or_boundary | proposal_source | delivery_evidence | finding | status |
|-----------|-------------------------|-----------------|-------------------|---------|--------|
| CHG-036-quic-owner-abort | owner future 被 abort 后 worker-runtime task 必须被 abort，测试在有界时间观察 | proposal.md CHG-036-QUIC 行 | `udp_punch_quic_nat_connect_worker_task_is_aborted_with_owner_future` 通过，1 秒有界轮询替代固定 20 次让步 | 放宽的等待窗口使 cancellation 后的 task drop 可稳定观察到 | pass |
| CHG-036-ttp-readiness | 测试 readiness 观察不能删除 Connecting tunnel，PN 只发一次 proxy request | proposal.md CHG-036-TTP 行 | `has_cached_tunnel_in_multi` 只读扫描 + TTP/PN 精确与并发压力用例通过 | Connecting 项保留，状态提升后观察器返回 true，PN 未因观察超时 | pass |

## Independent Defect Discovery

| category | evidence_inspected | adversarial_check | finding_or_not_applicable_reason | status |
|----------|--------------------|-------------------|----------------------------------|--------|
| behavior-and-logic | `TtpServer::has_cached_tunnel_for_test`、`RuntimeCore::get_existing_tunnel`、`find_existing_tunnel_in_multi` 调用链 | 判断观察器是否在 Connecting 窗口仍触发 retain/remove；实际 diff 显示已切换到无 retain 的 `has_cached_tunnel_in_multi` | 强制观察器必须对所有非 Connected 项返回 false 但不删除；TTP 回归测试通过该断言 | pass |
| boundaries-and-failure-paths | `AbortOnDropTask::join` 与 `Drop` abort 路径、QUIC owner 测试 | 用 1 秒超时验证 owner 取消后 worker `DropFlag` 必然置位；若生产 wrapper 不 abort，测试会超时而不是误通过 | 检查 owner.await 返回 Cancelled 后仍等待 worker 资源释放，证明取消传播不止于 owner future | pass |
| regression-and-side-effects | PN reverse TCP first-claim 精确用例、4 路并发压力、相关 TTP server 与 QUIC 模块共 19 个用例 | 并发运行 TTP server、QUIC 和 PN reverse 用例，确认只读观察不会因跨测试资源压力导致 readiness 超时或缓存丢失 | 19/19 通过；无新增 production prune、状态提升或 proxy 语义变化 | pass |

## Verification

- Targeted check: `cargo test -p p2p-frame --features x509 --lib -- ttp::tests::server_ udp_punch_quic_nat reverse_tcp_proxy_tests -- --nocapture`
- Result: pass
- Exception reason: n/a

## Findings

| id | severity | evidence | problem | blocking |
|----|----------|----------|---------|----------|
| 036-F-001 | none | 重复相关 19 个用例全部通过且 diff 仅覆盖测试/只读边界 | 未发现阻断性缺陷；残留观察窗口依赖 1 秒有界轮询，不影响生产契约 | no |

## Conclusion

- Accepted / rejected / needs changes: accepted
- Reason: 三个点名失败和 PN 并发压力均已由针对性命令验证通过；独立缺陷发现覆盖逻辑、边界和回归三类，未发现生产行为回归。
