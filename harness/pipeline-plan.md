# 流水线计划

## 触发条件
- 已批准的 proposal：`docs/versions/v0.1/modules/p2p-frame/proposal.md`
- 用户启动确认：已于 `2026-05-10` 确认 `PnTunnel` 控制通道需求，进入自动流水线
- 用户设计审批：已于 `2026-05-10` 确认 `PnTunnel` 控制通道设计可按 `TcpTunnel` / `QuicTunnel` 参考模型推进

## 验收基线
- 最终验收以下列文档为准：
  - `docs/versions/v0.1/modules/p2p-frame/proposal.md`

## 阶段图
| 任务 ID | 阶段 | 职责 | 范围 | 父任务 | 依赖项 | 输出 | 完成条件 |
|---------|------|------|------|--------|--------|------|----------|
| P-1 | planning | 为 `PnTunnel` 控制通道创建阶段图、依赖、输出和退回路由 | `p2p-frame` / `pn/client` / `pn/protocol` / 必要 `pn/service` | root | 用户确认已批准 proposal 并启动自动流水线 | `harness/pipeline-plan.md` | 本计划覆盖 design、testing、implementation、acceptance 任务 |
| D-1 | design | 定义 `PnTunnel` control channel ready gate、控制命令、heartbeat、remote close 感知和统一 close 状态机 | `docs/versions/v0.1/modules/p2p-frame/design.md`、`design/pn-tunnel-control-channel.md`、必要长期边界文档 | root | proposal approved | `design.md`、`design/pn-tunnel-control-channel.md`、`docs/modules/p2p-frame.md` | design approved 且覆盖 proposal 控制通道锚点 |
| T-1 | testing | 把 `PnTunnel` 控制通道设计映射为 unit、DV 和 integration 验证面 | `testing.md`、`testing/pn-tunnel-control-channel.md`、`testplan.yaml` | root | D-1 approved | testing 制品 | testing approved，且验证入口覆盖 control ready、remote close、heartbeat 和 close 幂等 |
| I-1 | implementation | 在已批准 proposal/design/testing 边界内实现 PN control channel 协议、建立流程、状态机和测试 | `p2p-frame/src/pn/protocol.rs`、`p2p-frame/src/pn/client/**`、必要 `p2p-frame/src/pn/service/pn_server.rs` 和测试 | root | proposal/design/testing 均 approved，implementation admission 通过 | code + tests | 实现完成并提供 testing 要求的验证证据 |
| A-1 | acceptance | 审计 proposal、design、testing、implementation 和验证证据是否一致 | `p2p-frame` 本轮控制通道交付 | root | implementation 证据已就绪 | acceptance report | acceptance 通过或明确退回责任阶段 |

## 子模块任务
| 任务 ID | 阶段 | 职责 | 子模块 | 父任务 | 依赖项 | 输出 | 完成条件 |
|---------|------|------|--------|--------|--------|------|----------|
| D-1.1 | design | 定义 PN control open/ready 协议、`Ping` / `Pong` / `Close` 控制命令和兼容边界 | `pn/protocol` | D-1 | proposal approved | `design/pn-tunnel-control-channel.md` | control 与业务 `ProxyOpenReq` 分离 |
| D-1.2 | design | 定义 active/passive `PnTunnel` control ready gate、控制循环、heartbeat 和统一 close path | `pn/client` | D-1 | proposal approved | `design/pn-tunnel-control-channel.md` | open/accept/inbound/idle 与 control close 共享状态机 |
| D-1.3 | design | 定义 relay 对 control open 的身份规范化、目标打开和 ready 后透明桥接边界 | `pn/service` | D-1 | proposal approved | `design/pn-tunnel-control-channel.md` | relay 不解析 ready 后控制命令 |
| T-1.1 | testing | 补充 control channel ready gate、ready 失败、ready 前不可用和 protocol command 分离验证 | `pn/protocol`、`pn/client` unit | T-1 | D-1 approved | `testing/pn-tunnel-control-channel.md`、`testplan.yaml` | 已声明 unit 断言 |
| T-1.2 | testing | 补充 remote close、control EOF、控制写失败、heartbeat timeout、manual close 通知和 close 幂等验证 | `pn/client` unit | T-1 | D-1 approved | `testing/pn-tunnel-control-channel.md`、`testplan.yaml` | 已声明 unit 断言 |
| T-1.3 | testing | 补充默认构造路径、all-in-one 场景和 workspace 兼容性验证 | `pn/service` + 下游 | T-1 | D-1 approved | `testing.md`、`testplan.yaml` | DV/integration 入口声明兼容性关注点 |
| I-1.1 | implementation | 增加 PN control open/ready 和 control command wire 类型 | `p2p-frame/src/pn/protocol.rs` | I-1 | T-1 approved | code + unit tests | 协议类型可编码/解码且不复用业务 kind |
| I-1.2 | implementation | 实现 active/passive control channel 建立、ready gate、read/heartbeat loop、manual close 通知与统一关闭状态机 | `p2p-frame/src/pn/client/**` | I-1 | T-1 approved | code + unit tests | 控制通道断开能关闭本地 tunnel |
| I-1.3 | implementation | 实现 relay control open 转发和 ready 后透明 bridge | `p2p-frame/src/pn/service/pn_server.rs` | I-1 | T-1 approved | code + unit tests | relay 不解析 ready 后控制命令 |

## 退回规则
- 如果 proposal 无法支撑 `PnTunnel` tunnel 级控制通道、remote close 感知或 ready gate：
  - 退回 proposal 澄清任务
- 如果 control open/ready 协议、控制命令、heartbeat、relay bridge 或状态机边界不明确：
  - 退回 design 任务
- 如果缺少 ready gate、remote close、control EOF、写失败、heartbeat timeout、manual close、close 幂等或下游兼容验证覆盖：
  - 退回 testing 任务
- 如果 implementation admission 未通过，或实现需要扩大到全局 relay session / 通用 PN 外控制协议：
  - 退回对应前置阶段
- 如果验收发现证据链不一致：
  - 按问题归属退回 proposal、design、testing 或 implementation

## 退出条件
- [ ] 所有阻塞问题已关闭
- [ ] proposal、design、testing 均为 approved
- [ ] `PnTunnel` 打开时建立 tunnel 级控制通道，并在 ready 成功后才返回可用 tunnel
- [ ] 控制通道 EOF、decode 失败、写失败、heartbeat timeout 或对端 close 能关闭本地 tunnel
- [ ] 控制通道关闭、manual close 与 idle close 共享幂等关闭路径
- [ ] close 后同一 `(remote_id, tunnel_id)` inbound open 不投递到旧对象
- [ ] 必需 unit/DV/integration 证据存在
- [ ] 已基于 `proposal.md` 通过最终验收
