# 流水线计划

## 触发条件
- 已批准的 proposal：`docs/versions/v0.1/modules/p2p-frame/proposal.md`
- 用户启动确认：已于 `2026-05-13` 确认 `endpoint_area_server_reflexive` 并要求按自动 pipeline 规则处理
- 当前 `change_id`：`endpoint_area_server_reflexive`

## 验收基线
- 最终验收以下列文档为准：
  - `docs/versions/v0.1/modules/p2p-frame/proposal.md`

## 阶段图
| 任务 ID | 阶段 | 职责 | 范围 | 父任务 | 依赖项 | 输出 | 完成条件 |
|---------|------|------|------|--------|--------|------|----------|
| P-AREA-1 | planning | 为 endpoint area `ServerReflexive` 语义创建阶段图、依赖、输出和退回路由 | `p2p-frame` / `endpoint` / `sn/service` / `sn/client` / `tunnel` | root | 用户确认已批准 proposal 并启动自动流水线 | `harness/pipeline-plan.md` | 本计划覆盖 design、testing、implementation、acceptance 任务 |
| D-AREA-1 | design | 把已批准 proposal 转成 endpoint area 枚举、编码、SN 观察地址分类和兼容边界设计 | `docs/versions/v0.1/modules/p2p-frame/design.md`、`design/tunnel-nat-traversal.md`、必要长期边界文档 | root | proposal approved | design 制品 | design approved 且覆盖 `endpoint_area_server_reflexive` |
| T-AREA-1 | testing | 把 `ServerReflexive` 设计映射为 unit、DV 和 integration 验证面 | `testing.md`、`testing/tunnel-nat-traversal.md`、`testplan.yaml` | root | D-AREA-1 approved | testing 制品 | testing approved，且验证入口覆盖 enum/codec/SN 分类和兼容性 |
| I-AREA-1 | implementation | 在已批准 proposal/design/testing 边界内实现 `ServerReflexive` 命名、`S` 编码、SN 分类逻辑和测试 | `p2p-frame/src/endpoint.rs`、`p2p-frame/src/sn/service/service.rs`、必要 `p2p-frame/src/sn/**` 与测试 | root | proposal/design/testing 均 approved，implementation admission 通过 | code + tests | 实现完成并提供 testing 要求的验证证据 |
| A-AREA-1 | acceptance | 审计 proposal、design、testing、implementation 和验证证据是否一致 | `p2p-frame` endpoint area 交付 | root | implementation 证据已就绪 | acceptance report | acceptance 通过或明确退回责任阶段 |

## 子模块任务
| 任务 ID | 阶段 | 职责 | 子模块 | 父任务 | 依赖项 | 输出 | 完成条件 |
|---------|------|------|--------|--------|--------|------|----------|
| D-AREA-1.1 | design | 定义 `EndpointArea::ServerReflexive`、`Display`/`FromStr`/raw codec 的 `S` 编码，以及移除 `is_sys_default()` 的公开接口边界 | `endpoint` | D-AREA-1 | proposal approved | `design.md`、`design/tunnel-nat-traversal.md` | endpoint area 命名和 codec 兼容边界明确 |
| D-AREA-1.2 | design | 定义 SN 观察地址与节点自上报地址一致/不一致时的 `Wan` / `ServerReflexive` 分类规则 | `sn/service`、`sn/client` | D-AREA-1 | proposal approved | `design/tunnel-nat-traversal.md` | SN 观察地址不再无条件标记为 `Wan` |
| D-AREA-1.3 | design | 定义 tunnel/NAT 候选消费 `ServerReflexive` 时与 `Wan`、`Mapped` 的边界 | `tunnel` | D-AREA-1 | proposal approved | `design/tunnel-nat-traversal.md` | NAT 候选分类不把 SN 反射地址静默等同于自声明公网地址 |
| T-AREA-1.1 | testing | 补充 endpoint enum、`S` 文本编解码、raw codec 和 `is_sys_default()` 移除验证 | `endpoint` unit | T-AREA-1 | D-AREA-1 approved | `testing.md`、`testing/tunnel-nat-traversal.md`、`testplan.yaml` | 已声明 unit 断言 |
| T-AREA-1.2 | testing | 补充 SN 观察地址一致标记 `Wan`、不一致标记 `ServerReflexive` 的验证 | `sn/service` unit | T-AREA-1 | D-AREA-1 approved | `testing/tunnel-nat-traversal.md`、`testplan.yaml` | 已声明 unit 断言 |
| T-AREA-1.3 | testing | 补充下游兼容性验证，确认新编码和公开枚举变更不会破坏 workspace 编译测试边界 | downstream integration | T-AREA-1 | D-AREA-1 approved | `testing.md`、`testplan.yaml` | DV/integration 入口声明兼容性关注点 |
| I-AREA-1.1 | implementation | 重命名 endpoint area、更新 `S` 编解码并移除 `is_sys_default()` | `p2p-frame/src/endpoint.rs` | I-AREA-1 | T-AREA-1 approved | code + unit tests | endpoint 编解码与公开接口符合 design/testing |
| I-AREA-1.2 | implementation | 修改 SN 观察地址分类逻辑，只有观察地址与节点自上报地址一致才设置 `Wan`，否则设置 `ServerReflexive` | `p2p-frame/src/sn/service/service.rs`、必要 `p2p-frame/src/sn/**` | I-AREA-1 | T-AREA-1 approved | code + unit tests | SN call/called/report 相关候选 area 符合 proposal |
| I-AREA-1.3 | implementation | 调整受 endpoint area 影响的 NAT 候选策略和现有测试引用 | `p2p-frame/src/tunnel/**`、必要测试 | I-AREA-1 | T-AREA-1 approved | code + unit tests | `ServerReflexive` 不被误判为静态 `Wan` 或 `Mapped` |

## 退回规则
- 如果 proposal 无法支撑 `ServerReflexive` 命名、`S` 编码或 SN 观察地址分类：
  - 退回 proposal 澄清任务
- 如果 endpoint codec、SN 分类或 NAT 候选消费边界不明确：
  - 退回 design 任务
- 如果缺少 enum/codec、SN 一致/不一致分类、公开接口移除或下游兼容验证覆盖：
  - 退回 testing 任务
- 如果 implementation admission 未通过，或实现需要引入 STUN/TURN、多 SN NAT 类型推断、新 endpoint area 或公开 trait 参数：
  - 退回对应前置阶段
- 如果验收发现证据链不一致：
  - 按问题归属退回 proposal、design、testing 或 implementation

## 退出条件
- [ ] 所有阻塞问题已关闭
- [ ] proposal、design、testing 均为 approved
- [ ] `EndpointArea::Default` 已替换为 `EndpointArea::ServerReflexive`
- [ ] `Display`、`FromStr` 和 raw codec 使用 `S` 表示 `ServerReflexive`
- [ ] `is_sys_default()` 不再作为公开方法存在
- [ ] SN 观察地址与节点自上报地址一致时标记 `Wan`
- [ ] SN 观察地址与节点自上报地址不一致时标记 `ServerReflexive`
- [ ] 必需 unit/DV/integration 证据存在
- [ ] 已基于 `proposal.md` 通过最终验收
