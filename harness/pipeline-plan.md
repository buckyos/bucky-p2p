# 流水线计划

## 触发条件
- 已批准的 proposal：`docs/versions/v0.1/modules/p2p-frame/proposal.md`
- 用户启动确认：已于 `2026-06-02T11:14:58+08:00` 确认进入自动后续阶段，处理 `Tunnel` stream/datagram 入站 channel 回调化。
- 当前 `change_id`：
  - `tunnel_stream_datagram_listen_callback`

## 验收基线
- 最终验收以 `docs/versions/v0.1/modules/p2p-frame/proposal.md` 中 `tunnel_stream_datagram_listen_callback` 的批准内容为准。
- 本流水线不得改变 TCP/QUIC/PN/TTP 线协议、TLS 身份校验、PN proxy channel 协议、业务 payload 格式、vport/purpose 编解码、tunnel publish 规则或 `TunnelNetwork` listener 回调语义。
- 公共 `Tunnel` trait 必须移除 `accept_stream()` 与 `accept_datagram()`；入站 stream/datagram channel 必须通过 `listen_stream(...)` / `listen_datagram(...)` 注册的回调交付。

## 阶段图
| 任务 ID | 阶段 | 职责 | 范围 | 依赖项 | 输出 | 完成条件 | 状态 |
|---------|------|------|------|--------|------|----------|------|
| P-TUNNEL-CHANNEL-1 | proposal | 定义 `Tunnel` stream/datagram 回调化需求 | `proposal.md` | 用户确认 | approved proposal | `tunnel_stream_datagram_listen_callback` 已进入 Proposal Items 且 proposal approved | confirmed |
| PLAN-TUNNEL-CHANNEL-1 | planning | 创建自动流水线阶段图、依赖、输出和退回路由 | `harness/pipeline-plan.md` | approved proposal + 用户启动 | pipeline plan | 计划覆盖 design、implementation、testing、acceptance | complete |
| D-TUNNEL-CHANNEL-1 | design | 将 proposal 转为 `Tunnel` trait 签名、回调运行时、错误语义和调用点迁移设计 | `design.md`、必要长期边界同步 | PLAN-TUNNEL-CHANNEL-1 | approved design | Directly Mapped Change Items 包含 `tunnel_stream_datagram_listen_callback` | complete |
| I-TUNNEL-CHANNEL-1 | implementation | 在已批准 proposal/design 边界内修改生产代码 | production code | D-TUNNEL-CHANNEL-1 + admission passed | production code | 公共 `Tunnel` trait 无 `accept_*`，生产调用点迁移到 listen 回调 | complete |
| T-TUNNEL-CHANNEL-1 | testing | 基于 proposal、design 和实现补充测试设计与测试实现 | tests、`testing.md`、`testplan.yaml` | I-TUNNEL-CHANNEL-1 | testing artifacts + tests | testplan 映射 `tunnel_stream_datagram_listen_callback`，相关入口可运行 | complete |
| A-TUNNEL-CHANNEL-1 | acceptance | 审计 proposal/design/testing/implementation/验证证据一致性 | acceptance report | T-TUNNEL-CHANNEL-1 | acceptance report | 无 blocking finding | complete |

## 子任务
| 任务 ID | 阶段 | 职责 | 子模块 | 父任务 | 输出 | 完成条件 | 状态 |
|---------|------|------|--------|--------|------|----------|------|
| D-TUNNEL-CHANNEL-1.1 | design | 定义 `Tunnel` trait 回调类型、签名、重复 listen、关闭后 listen 和回调错误语义 | `networks` | D-TUNNEL-CHANNEL-1 | design coverage | 公共接口和错误语义完整 | complete |
| D-TUNNEL-CHANNEL-1.2 | design | 定义 TCP/QUIC/PN tunnel 入站 channel 到回调的运行时归属和背压策略 | `networks/tcp`、`networks/quic`、`pn/client` | D-TUNNEL-CHANNEL-1 | design coverage | `sfo-reuseport` worker runtime 调度边界明确 | complete |
| D-TUNNEL-CHANNEL-1.3 | design | 定义 TTP、stream manager、datagram manager 和 PN server/client 调用点迁移 | `ttp`、`stream`、`datagram`、`pn` | D-TUNNEL-CHANNEL-1 | design coverage | 无公共 `accept_*` 轮询循环残留 | complete |
| I-TUNNEL-CHANNEL-1.1 | implementation | 修改公共 `Tunnel` trait 与通用类型导出 | `p2p-frame/src/networks/tunnel.rs` | I-TUNNEL-CHANNEL-1 | production code | trait 签名与 design 一致 | complete |
| I-TUNNEL-CHANNEL-1.2 | implementation | 修改 TCP/QUIC/PN tunnel 实现和内部投递路径 | `p2p-frame/src/networks/**`、`p2p-frame/src/pn/**` | I-TUNNEL-CHANNEL-1 | production code | 入站 channel 经回调交付 | complete |
| I-TUNNEL-CHANNEL-1.3 | implementation | 迁移 TTP、stream manager、datagram manager 和生产调用点 | `p2p-frame/src/ttp/**`、`p2p-frame/src/stream/**`、`p2p-frame/src/datagram/**` | I-TUNNEL-CHANNEL-1 | production code | 生产代码无公共 `accept_*` 调用 | complete |
| T-TUNNEL-CHANNEL-1.1 | testing | 补充 trait 编译、入站回调、未 listen、关闭后不回调和 manager 迁移覆盖 | unit/integration | T-TUNNEL-CHANNEL-1 | tests + testing docs | `test-run.py p2p-frame unit` 可达 | complete |
| A-TUNNEL-CHANNEL-1.1 | acceptance | 生成并执行最终验收审计 | review report | A-TUNNEL-CHANNEL-1 | acceptance report | admission/test evidence 通过或明确退回 | complete |

## 退回规则
- 如果 proposal 不能支撑公共 `Tunnel` trait 移除 `accept_*`，退回 proposal。
- 如果设计无法明确回调类型、运行时归属、重复注册、关闭后注册、回调错误或背压语义，退回 design。
- 如果 implementation admission 失败，退回 checker 指向的文档阶段。
- 如果实现需要改变线协议、payload 格式、身份校验、vport/purpose 编解码、PN proxy channel 协议或 publish 规则，退回 proposal/design。
- 如果测试缺少 `tunnel_stream_datagram_listen_callback` 的直接映射或统一入口不可达，退回 testing。
- 如果 acceptance 发现文档、实现或测试不一致，按问题归属退回 design、implementation 或 testing；同一非需求问题超过 5 次仍未解决时停止并报告。

## 退出条件
- [x] proposal 为 approved
- [x] design 为 approved
- [x] testing 为 approved
- [x] implementation admission 通过 `tunnel_stream_datagram_listen_callback`
- [x] 公共 `Tunnel` trait 无 `accept_stream()` / `accept_datagram()`
- [x] TCP/QUIC/PN tunnel 入站 stream/datagram 通过 listen 回调交付
- [x] TTP、stream manager、datagram manager 和 PN server/client 已迁移
- [x] 关闭后不继续触发 stream/datagram 回调
- [x] 已基于 approved proposal 通过最终验收
