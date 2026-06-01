# 流水线计划

## 触发条件
- 已批准的 proposal：`docs/versions/v0.1/modules/p2p-frame/proposal.md`
- 用户启动确认：已于 `2026-06-01T11:09:20+08:00` 确认进入自动后续阶段，处理 `p2p-frame` 内部 `unbounded_channel` 到 bounded channel 的配置化改造。
- 当前 `change_id`：
  - `bounded_channel_capacity_config`

## 验收基线
- 最终验收以 `docs/versions/v0.1/modules/p2p-frame/proposal.md` 中 `bounded_channel_capacity_config` 的批准内容为准。
- 本流水线不得改变 TCP/QUIC/PN/TTP 线协议、身份校验、tunnel publish 规则或业务 payload 格式。
- channel 容量默认值只能位于顶层配置，默认 `1024`；底层组件只接收已解析容量。

## 阶段图
| 任务 ID | 阶段 | 职责 | 范围 | 依赖项 | 输出 | 完成条件 | 状态 |
|---------|------|------|------|--------|------|----------|------|
| P-BOUNDED-1 | proposal | 定义 bounded channel 容量配置需求 | `proposal.md` | 用户确认 | approved proposal | `bounded_channel_capacity_config` 已进入 Proposal Items | confirmed |
| PLAN-BOUNDED-1 | planning | 创建自动流水线阶段图、依赖、输出和退回路由 | `harness/pipeline-plan.md` | approved proposal + 用户启动 | pipeline plan | 计划覆盖 design、implementation、testing、acceptance | complete |
| D-BOUNDED-1 | design | 将 proposal 转为顶层配置、容量传递、满载语义和代码路径设计 | `design.md`、必要长期边界同步 | PLAN-BOUNDED-1 | approved design | Directly Mapped Change Items 包含 `bounded_channel_capacity_config` | complete |
| I-BOUNDED-1 | implementation | 在已批准 proposal/design 边界内替换生产路径 unbounded channel | production code | D-BOUNDED-1 + admission passed | production code | 生产路径不再使用 `mpsc::unbounded_channel` / `UnboundedSender` / `UnboundedReceiver` | complete |
| T-BOUNDED-1 | testing | 基于 proposal、design 和实现补充测试设计与测试实现 | tests、`testing.md`、`testplan.yaml` | I-BOUNDED-1 | testing artifacts + tests | testplan 映射 `bounded_channel_capacity_config`，相关入口可运行 | complete |
| A-BOUNDED-1 | acceptance | 审计 proposal/design/testing/implementation/验证证据一致性 | acceptance report | T-BOUNDED-1 | acceptance report | 无 blocking finding | complete |

## 子任务
| 任务 ID | 阶段 | 职责 | 子模块 | 父任务 | 输出 | 完成条件 | 状态 |
|---------|------|------|--------|--------|------|----------|------|
| D-BOUNDED-1.1 | design | 定义 `P2pConfig` / `P2pStackConfig` 顶层容量配置、默认值和传递边界 | `stack` | D-BOUNDED-1 | design coverage | 默认 `1024` 只在顶层配置出现 | complete |
| D-BOUNDED-1.2 | design | 定义 TCP/QUIC listener 和 tunnel accepted queue 的 bounded channel 语义 | `networks/tcp`、`networks/quic` | D-BOUNDED-1 | design coverage | 满载时返回错误或关闭迟到输入 | complete |
| D-BOUNDED-1.3 | design | 定义 `NetManager`、`TunnelManager`、TTP registry 和 PN 测试替身容量传递 | `networks`、`tunnel`、`ttp`、`pn` | D-BOUNDED-1 | design coverage | 底层无局部默认容量 | complete |
| I-BOUNDED-1.1 | implementation | 增加顶层容量配置 API 并传入 network/manager/client 构造路径 | `stack`、constructors | I-BOUNDED-1 | production code | 默认和自定义容量可下发 | complete |
| I-BOUNDED-1.2 | implementation | 替换生产路径 `unbounded_channel` 类型和发送语义 | `p2p-frame/src/**` | I-BOUNDED-1 | production code | 搜索确认生产路径无 unbounded mpsc | complete |
| T-BOUNDED-1.1 | testing | 补充默认容量、自定义容量和满载行为测试 | unit | T-BOUNDED-1 | tests + testing docs | `test-run.py p2p-frame unit` 可达 | complete |
| A-BOUNDED-1.1 | acceptance | 生成并执行最终验收审计 | review report | A-BOUNDED-1 | acceptance report | admission/test evidence 通过或明确退回 | complete |

## 退回规则
- 如果 proposal 不能支撑某个底层队列的有界化语义，退回 proposal。
- 如果设计无法明确满载时是背压、错误、关闭还是丢弃已关闭 listener 迟到事件，退回 design。
- 如果 implementation admission 失败，退回 checker 指向的文档阶段。
- 如果实现需要改变线协议、payload 格式、身份校验或 publish 规则，退回 proposal/design。
- 如果测试缺少 `bounded_channel_capacity_config` 的直接映射或统一入口不可达，退回 testing。
- 如果 acceptance 发现文档、实现或测试不一致，按问题归属退回 design、implementation 或 testing；同一非需求问题超过 5 次仍未解决时停止并报告。

## 退出条件
- [x] proposal、design、testing 均为 approved
- [x] implementation admission 通过 `bounded_channel_capacity_config`
- [x] 顶层配置默认容量为 `1024`，外部可覆盖
- [x] 底层组件不定义局部默认 channel 容量
- [x] 生产路径无 `mpsc::unbounded_channel`、`UnboundedSender`、`UnboundedReceiver`
- [x] 满载行为有测试或明确验证证据
- [x] 已基于 approved proposal 通过最终验收
