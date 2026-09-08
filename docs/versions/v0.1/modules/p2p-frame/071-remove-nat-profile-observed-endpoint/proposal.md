---
task_manifest: task.yaml
status: approved
---

# 移除 NatProfile.observed_endpoint Proposal

Risk profile: ./risk-profile.yaml

## Workflow Tier Judgment
- Proposed tier: high-risk
- Final tier: high-risk
- Tier rationale / triggered boundaries: 删除 `NatProfile.observed_endpoint` 会改变公开 Rust 结构的字段集合，并同时改变该结构在 SN report/query/inter-SN/call/called 路径上的 `RawEncode/RawDecode` wire 形态；prediction、freshness、connect plan 与 fallback 都有消费者。这属于实质性公共契约/协议与兼容性影响，按 `task-entry-gate-rules.md` 默认归为 high-risk。
- Proposal and tier confirmation: 用户于 2026-09-08 确认 high-risk、按删除方向执行，并明确选择不需要旧 wire 兼容、不新增替代字段。

## Background and Goal
用户结论：`NatProfile.observed_endpoint` 是提前缓存的探测映射，既不应作为预测端口计算起点，也不应作为 freshness 或策略判断依据；实时探测本身会产生当前映射，不需要 profile 长期保存该字段。用户明确要求删除该字段。

目标：从 `NatProfile` 中删除 `observed_endpoint`，让 profile 只表达分类、时效与（如有）预测 hint；公开结构、wire 编码、freshness 判断和预测调用链同步调整，并移除不再允许的“缓存 profile 展开预测候选”路径。

## Scope
### In scope
- 删除 `p2p-frame/src/nat_type.rs` 中 `NatProfile.observed_endpoint` 字段及相关构造/默认值。
- 调整 `NatProfile::from_observations`：仍然基于全部观测完成 `NonSymmetricLike`/`SymmetricLike` 分类与 `NatPredictionHint` 生成，但不再向 profile 保存 observed endpoint。
- 调整 freshness 语义：`is_fresh` 不再要求 `observed_endpoint.is_some()`，仅以 `version`、`observation != Unknown`、`observed_at`/`valid_until` 时间窗口为准。
- 调整预测锚点：live probe 生成的预测候选以同一轮 `NatPredictionHint.last_observed` 为 base；`predict_traversal_endpoints` 现场探测后直接展开预测端点。
- 移除不实时依赖：`nat_candidates(Predicted)` 不再使用查询阶段缓存下来的 remote `NatProfile`/hint展开候选；需要预测时必须走 live `predict_traversal_endpoints`。
- 同步更新所有生产消费者、公开 API 测试、wire round-trip 测试、scheduler/peer manager/inter-SN profile 测试与 real-socket 策略矩阵中对该字段的构造和断言。

### Out of scope
- 不改变 NAT 探测协议、反射器端口向量、探测 socket 生命周期、TTL 常量或探测周期策略。
- 不改变 `NatPredictionHint.first_observed/last_observed/delta/parity` 的预测数学本身。
- 不修改 peer_info/endpoint_array 的既有 S 端点来源与刷新机制；`NonSymmetricLike` 直连仍使用 endpoint_array 中可用的 S/LAN 候选。
- 不引入新的公网/多机 E2E 环境。

### Boundary with neighboring modules
`NatProfile` wire 是所有 SN profile 交换的公共载体。删除字段必须配套处理旧客户端/旧服务端对同一 profile blob 的兼容解码，并明确是否保留“读取并丢弃旧 observed_endpoint”的兼容 shim，还是直接声明本次 wire break。

## Requirement Review
需求合理性：用户主张预测必须实时探测，profile 中保存旧 observed_endpoint 作为预测起点不可靠；同时 34p 日志中 `16643` 与 `17076` 并存进一步说明缓存的 profile endpoint 和实时探测端点是两个不同通道。删除该字段可以强制预测候选从实时探测生成，方向合理。

材料风险：
- `NatProfile` 是公开导出的 wire-encoded 类型，字段删除是公共契约变化。
- `is_fresh` 语义变化会影响 SN 是否返回 profile、TunnelManager 是否进入 NAT-aware 计划。
- `NatPredictionHint.last_observed` 仍携带上一次探测锚点；若 fallback 仍拿缓存 hint 展开，不能真正消除“非实时预测”，因此本提案把 fallback 预测改为必须 live probe。

## Proposal Items
| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-001 | CHG-remove-observed-endpoint | 从 `NatProfile` 删除 `observed_endpoint` 字段并调整 freshness，使 profile 不保存长期公网映射 | `nat_type.rs` 及所有 `NatProfile` 构造/解码/断言调用方 | 不再有 profile 级公网端点；直连端点完全由 endpoint_array/SN 观察提供 | 编译通过；wire round-trip 测试覆盖新 profile；无任何代码引用 `NatProfile.observed_endpoint` | 不新增 endpoint 字段替代 |
| P-002 | CHG-live-prediction-only | 预测端口候选只能由 live probe 产生；fallback 的 `nat_candidates(Predicted)` 不再以缓存 profile/hint 展开 | `tunnel_manager.rs`/`quic/listener.rs` 预测调用链及策略测试 | 需要预测的 fallback 必须先 live probe；失败则明确 fail-closed | 新增定向测试证明 fallback 不直接按缓存 hint 展开预测候选 | 不改预测数学或 socket 所有权 |

## Success Criteria
- 系统可见结果：`NatProfile` 结构中不存在 `observed_endpoint`，`mapping_at`/`is_fresh` 不再依赖该字段；过期判断仅由 `observed_at` + `valid_until` 决定。
- 系统可见结果：任何预测端口候选都来自一次实时 `probe_nat_profile`/`predict_traversal_endpoints`；SN 查询缓存 profile 不参与预测候选展开。
- 所需证据：
  - `cargo test -p p2p-frame --features x509 --lib` 通过；
  - 相关策略/SN profile wire/rendezvous 测试通过（`nat_type_aware_tests`、`nat_probe_directive`、`nat_connect_plan`、rendezvous prediction）；
  - repo-wide `rg "observed_endpoint"` 无生产/测试残留（除已存在的 SN observed endpoint 候选逻辑字段名）。
- 非目标：不声明旧 profile wire 双向兼容通过，除非用户在本提案确认中明确选择兼容解码方案。

## Risks
1. wire/API 破坏：删除字段使新旧 `NatProfile` blob 不兼容，release/rollback 期间 profile 消费可能 fail-closed 为 `Unknown`。若保留兼容读取则应明确写明，否则按 wire break 执行。
2. fallback 行为变化：要求 `Predicted` fallback 必须 live probe 会延长失败回退时间并可能更频繁触发探测；需在实现/测试阶段验证 deadline 与 `NAT_PROBE_TARGET_TIMEOUT`/`RENDEZVOUS_PREDICTION_TTL` 交互。
3. `NonSymmetricLike` 直连：删除 profile endpoint 后，直连只能依赖 endpoint_array/SN 观察，端点的刷新错误将直接表现为 34p 式 `16643` 失败；这属于 endpoint 数据面问题，本任务不纳入修复。

## Confirmed Decisions
- 不保留旧 wire 兼容，按 wire break 直接删除字段。
- 不为 live probe 的当前映射新增任何字段；预测调用内部使用同一轮 `NatPredictionHint.last_observed` 作为锚点。
