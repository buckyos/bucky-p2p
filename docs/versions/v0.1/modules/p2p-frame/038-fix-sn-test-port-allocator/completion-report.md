# Completion Report: 038-fix-sn-test-port-allocator

## Object and Scope

- Task manifest: task.yaml
- Workflow tier: standard
- Change record: docs/changes/038-fix-sn-test-port-allocator.md

## Delivery Summary

- Outcome: `sn/tests.rs::next_port()` 不再进入 OS 临时端口区间。Linux 动态读取 `ip_local_port_range` 的起点，在 `25025..min(43100, ephemeral_start-1)` 内单调分配；其它平台回退到 43100 上限（低于 macOS/Windows 临时端口起点）。增加回归断言，连续 64 个端口唯一且低于 ephemeral start。完整并发套件中的 SN 固定端口 `AddrInUse` 得到结构性消除，同时保留既有 bounded retry 兜底。
- Handoff: 只修改 `p2p-frame/src/sn/tests.rs` 测试资源分配函数与回归断言；生产代码与 037 的 reverse 测试改动均保持原样。

## Proposal Consistency

| change_id | requirement_or_boundary | proposal_source | delivery_evidence | finding | status |
|-----------|-------------------------|-----------------|-------------------|---------|--------|
| sn_test_port_allocator_ephemeral_safe_range | SN 测试端口恒低于 OS 临时端口起点并高于既有固定端口区间；完整并发套件不再出现 SN `AddrInUse` | proposal.md P-038-PORT 行 | `next_port` 区间分配 + 64 端口回归断言；`sn_profile_flow` 3 轮、完整 lib 套件默认 2 轮 + 4 线程 1 轮全过 | 回归断言实测当前 ephemeral start 43255，分配端口全部低于该值 | pass |

## Independent Defect Discovery

| category | evidence_inspected | adversarial_check | finding_or_not_applicable_reason | status |
|----------|--------------------|------------------|----------------------------------|--------|
| behavior-and-logic | `next_port` CAS 分配、回绕边界、测试端口区间与既有固定端口（20001-25024）重叠检查 | 模拟连续分配与 wrap；断言 64 端口唯一且在区间内 | 修复前 42000 递增会漂入临时区间；修复后区间上限由 ephemeral start 动态决定 | pass |
| boundaries-and-failure-paths | `/proc` 读取失败、非 Linux、ephemeral_start<=25025 配置 | 读取错误回退 43100；极端配置下断言 loud-fail，不静默覆盖 | 异常配置可被回归断言直接发现，不吞错误 | pass |
| regression-and-side-effects | `sn_profile_flow` 6 用例、完整 425 用例默认并发与 4 线程 | 多轮全量并检查 reverse/PN 既有用例 | 425/425 多轮通过，无新增回归；037 的 guard/readiness 修复保持通过 | pass |

## Verification

- Targeted check: `next_port_stays_outside_os_ephemeral_range`；`sn_profile_flow`（16 线程）×3；`cargo test -p p2p-frame --features x509 --lib` 默认 ×2、`--test-threads=4` ×1
- Result: pass
- Exception reason: n/a

## Findings

| id | severity | evidence | problem | blocking |
|----|----------|----------|---------|----------|
| 038-F-001 | none | 425/425 多轮 + 区间断言 | 未发现阻塞性缺陷；极端 OS 临时端口配置（起点低于 25025）会触发 loud-fail，属文档化边界 | no |

## Conclusion

- Accepted / rejected / needs changes: accepted
- Reason: 端口区间回归证明分配器结构上避开临时端口区间；SN 组与完整套件多轮通过；独立缺陷发现无生产行为回归。
