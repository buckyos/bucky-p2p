# 流水线计划

## 触发条件
- 已批准的 proposal：`docs/versions/v0.1/modules/p2p-frame/proposal.md`
- 用户启动确认：已于 `2026-05-13` 确认 `server_reflexive_quic_nat_keepalive` 并要求按自动 pipeline 规则处理
- 当前 `change_id`：`server_reflexive_quic_nat_keepalive`

## 验收基线
- 最终验收以下列文档为准：
  - `docs/versions/v0.1/modules/p2p-frame/proposal.md`

## 阶段图
| 任务 ID | 阶段 | 职责 | 范围 | 父任务 | 依赖项 | 输出 | 完成条件 |
|---------|------|------|------|--------|--------|------|----------|
| P-SR-NAT-1 | planning | 为 `ServerReflexive` QUIC NAT keepalive 创建阶段图、依赖、输出和退回路由 | `p2p-frame` / `tunnel` / `networks/quic` / `endpoint` | root | 用户确认已批准 proposal 并启动自动流水线 | `harness/pipeline-plan.md` | 本计划覆盖 design、testing、implementation、acceptance 任务 |
| D-SR-NAT-1 | design | 把已批准 proposal 转成 UDP punch candidate policy 与 QUIC heartbeat timeout 的可执行设计 | `design.md`、`design/tunnel-nat-traversal.md`、必要长期边界文档 | root | proposal approved | design 制品 | design approved 且覆盖 `server_reflexive_quic_nat_keepalive` |
| T-SR-NAT-1 | testing | 把设计映射为 unit 与 integration 验证面 | `testing.md`、`testing/tunnel-nat-traversal.md`、`testplan.yaml` | root | D-SR-NAT-1 approved | testing 制品 | testing approved，且验证入口覆盖 punch policy 与 heartbeat timeout |
| I-SR-NAT-1 | implementation | 在已批准 proposal/design/testing 边界内实现策略和测试 | `p2p-frame/src/tunnel/**`、`p2p-frame/src/networks/quic/**`、必要测试 | root | proposal/design/testing 均 approved，implementation admission 通过 | code + tests | 实现完成并提供 testing 要求的验证证据 |
| A-SR-NAT-1 | acceptance | 审计 proposal、design、testing、implementation 和验证证据是否一致 | `p2p-frame` NAT keepalive 交付 | root | implementation 证据已就绪 | acceptance report | acceptance 通过或明确退回责任阶段 |

## 子模块任务
| 任务 ID | 阶段 | 职责 | 子模块 | 父任务 | 依赖项 | 输出 | 完成条件 |
|---------|------|------|--------|--------|--------|------|----------|
| D-SR-NAT-1.1 | design | 定义 `TunnelManager` 只为 `EndpointArea::ServerReflexive` QUIC candidate 开启 UDP punch 的边界 | `tunnel` | D-SR-NAT-1 | proposal approved | `design.md`、`design/tunnel-nat-traversal.md` | candidate policy 明确排除 `Lan`、`Wan`、`Mapped`、TCP、IPv6 和 0 端口 |
| D-SR-NAT-1.2 | design | 定义 `networks/quic` 保持 heartbeat interval 不变且 timeout 为 30 秒的控制面策略 | `networks/quic` | D-SR-NAT-1 | proposal approved | `design.md`、`design/tunnel-nat-traversal.md` | heartbeat interval/timeout 边界明确，关闭路径不变 |
| T-SR-NAT-1.1 | testing | 补充 punch candidate policy 的 unit 验证 | `tunnel` unit | T-SR-NAT-1 | D-SR-NAT-1 approved | `testing.md`、`testing/tunnel-nat-traversal.md`、`testplan.yaml` | 已声明 `ServerReflexive` only 断言和反例 |
| T-SR-NAT-1.2 | testing | 补充 QUIC heartbeat interval/timeout 的 unit 验证 | `networks/quic` unit | T-SR-NAT-1 | D-SR-NAT-1 approved | `testing.md`、`testing/tunnel-nat-traversal.md`、`testplan.yaml` | 已声明 interval 保持 5s、timeout 为 30s 的断言 |
| I-SR-NAT-1.1 | implementation | 调整 UDP punch candidate policy 和现有测试 | `p2p-frame/src/tunnel/tunnel_manager.rs` | I-SR-NAT-1 | T-SR-NAT-1 approved | code + unit tests | `ServerReflexive` QUIC candidate 开启 punch，其他 endpoint 不开启 |
| I-SR-NAT-1.2 | implementation | 调整 QUIC heartbeat timeout 并补充测试可见性 | `p2p-frame/src/networks/quic/tunnel.rs` | I-SR-NAT-1 | T-SR-NAT-1 approved | code + unit tests | interval 保持 5s，timeout 为 30s |

## 退回规则
- 如果 proposal 无法支撑 `ServerReflexive` only punch 或 30 秒 heartbeat timeout：
  - 退回 proposal 澄清任务
- 如果 candidate policy 或 heartbeat timeout 的接口边界不明确：
  - 退回 design 任务
- 如果缺少 punch policy、heartbeat timeout 或 workspace 兼容验证覆盖：
  - 退回 testing 任务
- 如果 implementation admission 未通过，或实现需要新增 raw UDP 协议、公共 trait 参数或改变 heartbeat interval：
  - 退回对应前置阶段
- 如果验收发现证据链不一致：
  - 按问题归属退回 proposal、design、testing 或 implementation

## 退出条件
- [x] 所有阻塞问题已关闭
- [x] proposal、design、testing 均为 approved
- [x] UDP punch 只对 `EndpointArea::ServerReflexive` QUIC candidate 开启
- [x] `Lan`、`Wan`、`Mapped`、TCP、IPv6、0 端口和默认 intent 路径不启用 punch
- [x] QUIC heartbeat interval 保持现有 5 秒
- [x] QUIC heartbeat timeout 调整为 30 秒
- [x] 必需 unit/integration 证据存在
- [x] 已基于 `proposal.md` 通过最终验收
