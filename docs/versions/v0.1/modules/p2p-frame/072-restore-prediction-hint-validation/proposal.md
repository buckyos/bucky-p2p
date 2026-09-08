---
task_manifest: task.yaml
status: approved
---

# Restore hint validity check in usable_prediction_hint Proposal

Risk profile: not-created

## Workflow Tier Judgment
- Proposed tier: standard
- Final tier: standard
- Tier rationale / triggered boundaries: 改动集中在 `NatProfile::usable_prediction_hint` 的合法性判定及其单元/连接计划回归测试；不改公开结构字段、wire 编码、并发/生命周期、依赖、部署或安全面；但会改变生产预测策略选择行为（无效 hint 从可预测恢复为不可预测），需要新增策略回归测试，不满足 trivial，按 bounded single-project bugfix 归 standard。
- Proposal and tier confirmation: 用户于 2026-09-08 明确要求"按建议修复"；该建议即本提案范围（恢复 `hint.is_usable_with(&hint.last_observed)` 校验并补充无效 hint 策略回归测试），视为已确认 standard 执行。

## Background and Goal
071 移除 `NatProfile.observed_endpoint` 后，`p2p-frame/src/nat_type.rs` 的 `usable_prediction_hint()` 只检查 SymmetricLike 分类与 freshness，不再校验 hint 本身是否合法。SN 上报/存储路径只检查 freshness，不过滤 `sample_count < 2`、`port_delta == 0`、端口奇偶关系不一致等无效 hint；`select_connect_plan()` 因此可能为无效 hint 选择 `CandidateMode::Predicted`，实时预测失败后进入代理或报错，丢失原有 `Base` 尝试。

目标：恢复 hint 合法性校验，使执行者只接受与 `NatPredictionHint` 自描述一致的 hint；同时加入无效 hint 的策略回归测试，防止 071 删除校验的同类回归再次出现。

## Scope
### In scope
- `p2p-frame/src/nat_type.rs` `usable_prediction_hint()`：在 freshness 与 SymmetricLike 判定之后，用 `hint.is_usable_with(&hint.last_observed)` 作为唯一 hint 合法性门槛。该一次性校验覆盖 `sample_count >= 2`、`port_delta != 0`、first/last/base 同 IPv4、总增量一致与奇偶关系一致。
- 回归测试：
  - `p2p-frame/tests/unit/nat_type/tests.rs` 新增无效 hint（样本不足、delta 为 0、奇偶不一致）单元测试，证明 fresh SymmetricLike profile 也不返回这些 hint。
  - `p2p-frame/tests/unit/tunnel/nat_connect_plan/tests.rs` 新增策略回归：对称 profile 携带无效 hint 时 `select_connect_plan()` 选择 `BoundedBestEffort` 与 `Base` 候选，不再进入 `Predicted`。

### Out of scope
- 不改变 `NatPredictionHint` 结构、`from_observations` 的生成逻辑或预测数学。
- 不改变 `select_connect_plan()` 的其它策略分支、wire/协议、NAT probe 调度、TTL 或 freshness 本身。
- 不引入公网/多机端到端环境；本地单元与策略回归即为验收范围。

### Boundary with neighboring modules
`usable_prediction_hint()` 是 `select_connect_plan()` 与策略矩阵判断可预测性的唯一读取点；修复该判定即可保证无效 hint 不会进入 `Predicted` 连接计划。SN 上报路径无需额外修改，因为 hint 是否合法已在消费侧由 `usable_prediction_hint()` fail-closed。

## Proposal Items
| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-001 | CHG-usable-prediction-hint-validity | `usable_prediction_hint()` 对新鲜 SymmetricLike profile 仍校验 hint 合法性；无效 hint 返回 `None`，连接计划回退 `Base` | `nat_type.rs`、`nat_type/tests.rs`、`nat_connect_plan/tests.rs` | 以 hint 自身 `last_observed` 为 base 做一次性校验；正常 `from_observations` 生成的 hint 不受影响 | 新增无效 hint 单测与策略回归通过；`cargo test -p p2p-frame --features x509` 定向组通过 | 不改 hint 结构、不上层过滤、不加实时探测 |

## Success Criteria
- 系统可见结果：任何 fresh SymmetricLike profile，只要 hint 不自洽（样本不足、delta 为 0、奇偶/增量不符），`usable_prediction_hint()` 返回 `None`。
- 系统可见结果：`select_connect_plan()` 对携带无效 hint 的双对称 profile 生成 `BoundedBestEffort`/`Base` 计划，不再把 `Base` 尝试换成 `Predicted`。
- 所需证据：
  - `cargo test -p p2p-frame --features x509 usable_prediction_hint` / `nat_connect_plan` 定向测试通过；
  - `cargo test -p p2p-frame --features x509 --lib` 通过（无回归）。
- 非目标：不声明公网 NAT、部署或跨进程证据；不改变正常合法 hint 的预测行为。

## Risks
- 行为收紧：外部/SN 上报的非法 hint 在消费侧将无法再进入 Predicted，计划回退 Base 或失败关闭；这是恢复 071 之前既有语义的预期收紧。
- 兼容影响：`NatProfile` 结构/wire 不受影响；只有"非法但曾被误判为可预测"的运行时 profile 行为变化。

## Approval Record
- approver: user
- approval_date: 2026-09-08
- user_statement: 按建议修复；保留 `hint.is_usable_with(&hint.last_observed)` 校验并补充无效 hint 的策略回归测试。
