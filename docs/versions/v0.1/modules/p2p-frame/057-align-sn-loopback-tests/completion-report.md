# Lightweight Acceptance Report

## Object and Scope

- Task manifest: task.yaml
- Workflow tier: trivial
- Change record: not-applicable

## Delivery Summary

- Outcome: 对齐了四个 SN 测试与当前 loopback endpoint 安全策略；默认 `x509` 构建验证 loopback 被过滤，显式 `test-real-socket-matrix` 构建继续验证 endpoint、serving-SN observation 和 rendezvous response-owner 行为。
- Handoff: 用户报告的默认 SN 测试失败已消除，matrix 下原正向/拒绝场景仍实际执行；生产 `SnService`、endpoint policy、协议和 tunnel 逻辑均未修改。

## Proposal Consistency

| change_id | Requirement or Boundary | Proposal Source | Delivery Evidence | Finding | Status |
|-----------|-------------------------|-----------------|-------------------|---------|--------|
| CHG-align-sn-loopback-tests | 默认构建断言 loopback endpoint 被过滤，matrix feature 保留 query endpoint 与 response-owner 场景，不放宽生产策略。 | `proposal.md` Scope、Proposal Item P-057-LOOPBACK 与 Success Criteria | `p2p-frame/src/sn/tests.rs` 的双 feature 断言；`p2p-frame/tests/tunnel_rendezvous/sn_same_sn_tests.rs` 的两个 feature gate；默认 SN 37/37、matrix same-SN 6/6 及 matrix query/call 2/2 通过。 | 实际交付仅校正测试编译边界，完整覆盖 approved requirement 和 non-goal。 | pass |

## Independent Defect Discovery

| Category | Evidence Inspected | Adversarial Check | Finding or Not-Applicable Reason | Status |
|----------|--------------------|-------------------|----------------------------------|--------|
| behavior-and-logic | 当前 `SnService::sanitize_reported_endpoints`、`SnService::get_peer_observed_ep`、两个 query 测试和两个 response-owner 测试。 | 检查是否把已注册 peer identity 与可发布 endpoint 错误绑定，是否将两个 ownership 场景从所有构建中删除，及 feature 正反分支是否与生产条件完全一致。 | 默认分支仍验证 identity 并明确要求 endpoint 为空；matrix 分支要求有效 QUIC endpoint；两个 ownership 场景只在能够产生 loopback observation 的 matrix 构建中执行，未丢失接受/拒绝语义。 | pass |
| boundaries-and-failure-paths | `sanitize_reported_endpoints` 对 loopback 的落空分支、`get_peer_observed_ep` 的 feature 条件、mixed-unowned prediction 的失败断言及当前测试基线差异。 | 尝试找出默认路径仍可能返回 loopback、matrix 路径只跳过而不验证、或未拥有 prediction 被错误接受的反例。 | 默认 query 两个用例均执行空数组断言；matrix query 两个用例均执行非零 QUIC endpoint 断言；matrix response-owner 正负场景均运行并通过，混合第三方 IP 仍被拒绝。 | pass |
| regression-and-side-effects | 任务基线与两个 task path 的逐文件 diff、默认 `sn::tests`、matrix `sn_same_sn_tests`、四条 matrix/default 精确回归和 scoped `git diff --check`。 | 检查是否覆盖已有脏工作区内容、修改生产文件、引入格式问题、降低默认 peer/query/call 覆盖或只让失败测试变成零执行。 | 基线差异只有两处条件断言和两个 feature attribute；默认模块仍执行 37 个测试，matrix 模块执行包含两个目标场景的 6 个测试，生产路径无任务新增差异。 | pass |

## Verification

- Targeted check: `cargo test -p p2p-frame --features x509 --lib sn::tests::sn_client_query_registered_peer_returns_full_info -- --exact --test-threads=1`; `cargo test -p p2p-frame --features x509 --lib sn::tests::sn_client_server_connection_covers_report_query_call_and_called_response -- --exact --test-threads=1`; `cargo test -p p2p-frame --features x509 --lib sn::tests -- --test-threads=1`; `cargo test -p p2p-frame --features x509,test-real-socket-matrix --lib sn::tests::sn_same_sn_tests -- --test-threads=1`; matrix feature 下两个 query/call 精确命令；`git diff --check -- p2p-frame/src/sn/tests.rs p2p-frame/tests/tunnel_rendezvous/sn_same_sn_tests.rs`。
- Result: passed
- Exception reason: not-applicable

## Findings

| ID | Severity | Evidence | Problem | Blocking |
|----|----------|----------|---------|----------|
| F-057-1 | none | 当前生产条件、任务基线差异、默认 37/37 与 matrix 6/6/2/2 运行结果 | 独立比例复核未发现剩余缺陷；测试没有通过放宽生产安全边界或永久删除 response-owner 覆盖来变绿。 | no |

## Conclusion

- Accepted / rejected / needs changes: accepted
- Reason: 交付完整满足批准的测试边界修复；默认与 opt-in feature 两侧均有实际断言和运行证据，任务范围内没有生产行为变化或未解决回归。
