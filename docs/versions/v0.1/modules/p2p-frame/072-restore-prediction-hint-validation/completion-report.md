# Completion Report: 072-restore-prediction-hint-validation

## Object and Scope

- Task manifest: task.yaml
- Workflow tier: standard
- Change record: docs/changes/072-restore-prediction-hint-validation.md

## Delivery Summary

- Outcome: `NatProfile::usable_prediction_hint()` 已恢复 hint 合法性校验：fresh SymmetricLike profile 只有在 `hint.is_usable_with(&hint.last_observed)` 成立时才返回可预测 hint。`select_connect_plan()` 对携带无效 hint 的双对称 profile 生成 `BoundedBestEffort`/`Base` 计划，不再错误进入 `Predicted` 并丢失 `Base` 尝试。
- Handoff: 交付改动仅 `p2p-frame/src/nat_type.rs` 的过滤逻辑，以及 `p2p-frame/tests/unit/nat_type/tests.rs` 的无效 hint 单测与 `p2p-frame/tests/unit/tunnel/nat_connect_plan/tests.rs` 的策略回归测试；wire、结构、freshness、实时预测调用链均未改变。

## Proposal Consistency

| change_id | requirement_or_boundary | proposal_source | delivery_evidence | finding | status |
|-----------|-------------------------|-----------------|-------------------|---------|--------|
| CHG-usable-prediction-hint-validity | `usable_prediction_hint()` 对新鲜 SymmetricLike profile 仍校验 hint 合法性；无效 hint 返回 `None`，连接计划回退 `Base` | proposal.md P-001 Scope / Success Criteria | `usable_prediction_hint()` 以 `hint.last_observed` 作 base 调用 `is_usable_with`；`fresh_symmetric_profile_rejects_invalid_hints` 覆盖样本不足/delta 为 0/奇偶不一致；`symmetric_profiles_with_invalid_hints_keep_base_plan` 断言计划为 Base | 交付符合已批准范围 | pass |

## Independent Defect Discovery

| category | evidence_inspected | adversarial_check | finding_or_not_applicable_reason | status |
|----------|--------------------|------------------|----------------------------------|--------|
| behavior-and-logic | `usable_prediction_hint()` 的 SymmetricLike/freshness 判定、`is_usable_with()` 全部 gate，以及 `select_connect_plan()` 的 `caller_predictable`/`callee_predictable` 计算 | 反向构造 `sample_count < 2`、`port_delta == 0`、奇偶不一致的 fresh SymmetricLike hint，确认全部返回 `None`；再让双对称 profile 携带无效 hint，检查计划是否仍进入 Predicted | 三类无效 hint 均被过滤；双对称无效 hint 计划稳定为 `BoundedBestEffort` 与 `Base`，无 Predicted 路径 | pass |
| boundaries-and-failure-paths | `prediction_or_base()`、`NatProfile::unknown()`、freshness 边界、直接构造 profile | 检查无效 hint 是否会被 freshness 或 observation 条件之外的路径绕过 | `usable_prediction_hint()` 是唯一读取点，所有绕过都要经过同一 filter | pass |
| regression-and-side-effects | `NatPredictionHint::from_observations()` 生成的合法 hint、既有 `prediction_hint_requires_consistent_delta_and_obeys_bounds`、`ordered_matrix_selects_one_connector_and_matching_peer_action` | 确认合法 hint 的 can_use/predicted_ports 行为不变；全量 lib 无本次改动相关失败 | 合法 hint 路径不变；全量首轮仅两个既有 TCP 真实 socket 时序用例失败，单独重跑通过 | pass |

## Verification

- Targeted check:
  - `cargo test -p p2p-frame --features x509 fresh_symmetric_profile_rejects_invalid_hints --lib`：通过
  - `cargo test -p p2p-frame --features x509 symmetric_profiles_with_invalid_hints_keep_base_plan --lib`：通过
  - `cargo test -p p2p-frame --features x509 nat_type::tests --lib`：4/4 通过
  - `cargo test -p p2p-frame --features x509 tunnel::nat_connect_plan::tests --lib`：6/6 通过
  - `cargo test -p p2p-frame --features x509 --lib`：507 项中 505 通过；2 项 TCP 真实 socket 用例单独重跑均通过
- Result: pass
- Exception reason: 全量首轮的 2 个失败用例（`tcp_data_control_direction_priority_prefers_passive_peer_creation_and_reuses_it`、`tcp_reverse_data_first_claim_pn_proxy_stream_uses_real_reverse_tcp_target`）均为既有 TCP listener/真实 socket 在并行全量下的端口占用或时序抖动，与本次 `usable_prediction_hint()` 逻辑无交集；两个用例单独运行均通过。

## Findings

| id | severity | evidence | problem | blocking |
|----|----------|----------|---------|----------|
| 072-F-001 | none | 定向单测与策略回归；全量非相关失败用例单独复跑通过 | 未发现阻塞性生产缺陷 | no |

## Conclusion

- Accepted / rejected / needs changes: accepted
- Reason: `usable_prediction_hint()` 已恢复 hint 自洽校验，`select_connect_plan()` 对无效 hint 回退 `Base`；新增 NatType 单测与策略回归均通过，既有合法 hint 行为不受影响。
