# Lightweight Acceptance Report

## Object and Scope

- Task manifest: task.yaml
- Workflow tier: trivial
- Change record: not-applicable

## Delivery Summary

- Outcome: 修复了默认 `x509` 下 `sn_protocol_real_network_quic_matrix` 对 loopback Query endpoint 的过期非空断言；默认 QUIC 现在明确验证 endpoint 被过滤，`test-real-socket-matrix` 下仍验证 QUIC loopback endpoint 被实际返回，TCP 在两个 feature 边界继续验证有效 endpoint。
- Handoff: 用户报告的 panic 已精确复现并转绿；任务只修改现有 integration test 的一个断言块，生产 SN、endpoint、协议和 tunnel 实现均未产生任务新增差异。

## Proposal Consistency

| change_id | Requirement or Boundary | Proposal Source | Delivery Evidence | Finding | Status |
|-----------|-------------------------|-----------------|-------------------|---------|--------|
| CHG-align-sn-real-network-query-loopback | 默认 QUIC 验证 loopback observation 被过滤，opt-in QUIC 保留有效 loopback endpoint 正向证据；TCP 两侧保留有效 endpoint，并继续覆盖 identity、协议版本和 unknown peer。 | `proposal.md` Scope、P-058-QUERY、Success Criteria 及 post-confirmation evidence refinement | `p2p-frame/tests/sn_protocol_real_network.rs::exercise_query` 按 TCP/QUIC 分支断言，并仅为 QUIC 区分 feature；默认与 opt-in 完整 integration target 各 3/3 通过。 | 交付与批准目标和安全边界一致；确认后发现的 TCP 既有语义已在同一范围内细化，未扩展 scope 或修改 tier。 | pass |

## Independent Defect Discovery

| Category | Evidence Inspected | Adversarial Check | Finding or Not-Applicable Reason | Status |
|----------|--------------------|-------------------|----------------------------------|--------|
| behavior-and-logic | `exercise_query`、`run_real_network_matrix`、`SnService::get_peer_observed_ep`、`sanitize_reported_endpoints` 和 `handle_query_sn` 当前分支。 | 检查是否把默认 QUIC 空数组误当成 peer 不存在，是否把 feature 条件错误套到 TCP，是否用 SN transport 代替未经验证的 target transport，以及正向断言是否只检查非空而不检查协议/地址/端口。 | Query 仍先解码并验证 target identity，再按已由 `assert_registered_on_selected_transport` 绑定的矩阵 transport 分支；默认 QUIC 明确为空，opt-in QUIC 要求 QUIC+loopback+非零端口，TCP 两侧要求 TCP+非零端口。未发现错误分支或弱化断言。 | pass |
| boundaries-and-failure-paths | 默认/opt-in feature、TCP/QUIC 四个组合，以及同函数中的 known/unknown peer 和协议版本断言。 | 尝试找出 feature 下零测试、Ext transport 静默通过、unknown peer 被新分支影响、默认路径重新接受 loopback 或 opt-in 路径不再证明 endpoint 的情况。 | 两次完整 integration target 都实际执行 inventory、TCP 和 QUIC 共 3 个测试；`Protocol::Ext` 显式 unreachable；known peer identity/version 与 unknown peer empty/None 断言保持不变。未发现边界或失败路径缺口。 | pass |
| regression-and-side-effects | 任务开始基线中的 untracked 测试副本、精确 task-only no-index diff、当前脏工作区、两次完整测试结果和 scoped whitespace check。 | 检查是否覆盖用户已有未提交内容、触碰生产文件、改变 TCP 行为、删除 matrix 正向覆盖、引入无关格式化或依赖已有任务文档替代当前运行证据。 | task-only diff 只有 `exercise_query` 的一个断言块；默认与 opt-in TCP 均通过，opt-in QUIC 正向覆盖仍执行；scoped diff check 无问题，其他脏工作区内容保持不变。 | pass |

## Verification

- Targeted check: red: `cargo test -p p2p-frame --features x509 --test sn_protocol_real_network sn_protocol_real_network_quic_matrix -- --exact --test-threads=1`；green: `cargo test -p p2p-frame --features x509 --test sn_protocol_real_network -- --test-threads=1`；green opt-in: `cargo test -p p2p-frame --features "x509,test-real-socket-matrix" --test sn_protocol_real_network -- --test-threads=1`；scope: baseline-to-current `git diff --no-index --check` for `p2p-frame/tests/sn_protocol_real_network.rs`。
- Result: passed
- Exception reason: not-applicable

## Findings

| ID | Severity | Evidence | Problem | Blocking |
|----|----------|----------|---------|----------|
| F-058-1 | none | 精确 red、两侧 3/3 green、生产调用链和 task-only baseline diff | 独立比例复核未发现剩余缺陷；修复没有放宽生产 loopback 策略，也没有弱化 TCP 或 opt-in QUIC 正向覆盖。 | no |

## Conclusion

- Accepted / rejected / needs changes: accepted
- Reason: 实现完整满足批准的 Query 测试契约修复；默认安全边界与 opt-in 真实 socket 正向证据均有实际运行覆盖，任务范围内无生产行为变化或阻塞发现。
