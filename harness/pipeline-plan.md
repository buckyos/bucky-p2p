# 流水线计划

## 触发条件
- 已批准的 proposal：`docs/versions/v0.1/modules/p2p-frame/proposal.md`
- 用户启动确认：已于 `2026-06-04T16:26:18+08:00` 确认自动处理后续步骤，处理 `Tunnel` control stream API。
- 当前 `change_id`：
  - `tunnel_control_stream_api`

## 验收基线
- 最终验收以 `docs/versions/v0.1/modules/p2p-frame/proposal.md` 中 `tunnel_control_stream_api` 的批准内容为准。
- 公共能力只通过 `Tunnel::open_control_stream(...)` / `Tunnel::listen_control_stream(...)` 暴露。
- 内部 `control_stream` runtime、frame、stream id、window 和 buffer 协议不得公开导出。
- TCP/QUIC/PN 现有控制命令只新增单一 `Data` 命令承载内部 frame，单个 `Data` payload 最大 `64 KiB`。
- 底层控制通道断开时，所有派生 control stream、pending open 和 pending write 必须断开或失败。

## 阶段图
| 任务 ID | 阶段 | 职责 | 范围 | 依赖项 | 输出 | 完成条件 | 状态 |
|---------|------|------|------|--------|------|----------|------|
| P-TUNNEL-CONTROL-STREAM-1 | proposal | 定义 `Tunnel` control stream API 需求、边界、非目标和验收锚点 | `proposal.md` | 用户确认 | approved proposal | `tunnel_control_stream_api` 已进入 Proposal Items 且 proposal approved | confirmed |
| PLAN-TUNNEL-CONTROL-STREAM-1 | planning | 创建自动流水线阶段图、依赖、输出和退回路由 | `harness/pipeline-plan.md` | approved proposal + 用户启动 | pipeline plan | 计划覆盖 design、implementation、testing、acceptance | complete |
| D-TUNNEL-CONTROL-STREAM-1 | design | 将 proposal 转为公共 trait、内部 runtime、transport adapter、64KiB 上限和关闭传播设计 | `design.md`、`design/tunnel-control-stream-api.md`、必要长期边界同步 | PLAN-TUNNEL-CONTROL-STREAM-1 | approved design | Directly Mapped Change Items 包含 `tunnel_control_stream_api` | complete |
| I-TUNNEL-CONTROL-STREAM-1 | implementation | 在已批准 proposal/design 边界内修改生产代码 | production code | D-TUNNEL-CONTROL-STREAM-1 + admission passed | production code | 公共 trait、内部 runtime 和 TCP/QUIC/PN `Data` adapter 实现完成 | complete |
| T-TUNNEL-CONTROL-STREAM-1 | testing | 基于 proposal、design 和实现补充测试设计与测试实现 | tests、`testing.md`、`testing/`、`testplan.yaml` | I-TUNNEL-CONTROL-STREAM-1 | testing artifacts + tests | testplan 映射 `tunnel_control_stream_api`，相关入口可运行 | complete |
| A-TUNNEL-CONTROL-STREAM-1 | acceptance | 审计 proposal/design/testing/implementation/验证证据一致性 | acceptance report | T-TUNNEL-CONTROL-STREAM-1 | acceptance report | 无 blocking finding | complete |

## 子任务
| 任务 ID | 阶段 | 职责 | 子模块 | 父任务 | 输出 | 完成条件 | 状态 |
|---------|------|------|--------|--------|------|----------|------|
| D-TUNNEL-CONTROL-STREAM-1.1 | design | 定义 `Tunnel` trait control stream 类型、签名和外部可见性边界 | `networks/tunnel` | D-TUNNEL-CONTROL-STREAM-1 | design coverage | 公共接口和内部不可导出边界完整 | complete |
| D-TUNNEL-CONTROL-STREAM-1.2 | design | 定义内部 `control_stream` runtime、frame、64KiB 上限、buffer/window 和关闭传播 | `networks/control_stream` | D-TUNNEL-CONTROL-STREAM-1 | design supplement | frame、上限和 close_all 语义完整 | complete |
| D-TUNNEL-CONTROL-STREAM-1.3 | design | 定义 TCP/QUIC/PN 控制命令 `Data` adapter 边界 | `networks/tcp`、`networks/quic`、`pn/client` | D-TUNNEL-CONTROL-STREAM-1 | design coverage | transport 只包装/拆出 payload，不解析内部 frame | complete |
| I-TUNNEL-CONTROL-STREAM-1.1 | implementation | 修改公共 `Tunnel` trait 与通用 callback 类型 | `p2p-frame/src/networks/tunnel.rs` | I-TUNNEL-CONTROL-STREAM-1 | production code | trait 签名与 design 一致 | complete |
| I-TUNNEL-CONTROL-STREAM-1.2 | implementation | 新增内部 `control_stream` runtime | `p2p-frame/src/networks/control_stream.rs` | I-TUNNEL-CONTROL-STREAM-1 | production code | open/listen、frame、64KiB、close_all 实现完成 | complete |
| I-TUNNEL-CONTROL-STREAM-1.3 | implementation | 接入 TCP/QUIC/PN `Data` 控制命令和关闭传播 | `p2p-frame/src/networks/tcp/**`、`p2p-frame/src/networks/quic/**`、`p2p-frame/src/pn/**` | I-TUNNEL-CONTROL-STREAM-1 | production code | 三个 tunnel 实现均可承载 control stream | complete |
| T-TUNNEL-CONTROL-STREAM-1.1 | testing | 补充内部 runtime 和 mock tunnel 行为测试 | unit | T-TUNNEL-CONTROL-STREAM-1 | tests | open/listen、purpose、64KiB、close_all 覆盖 | complete |
| T-TUNNEL-CONTROL-STREAM-1.2 | testing | 补充 TCP/QUIC/PN adapter 编译或行为覆盖 | unit/integration | T-TUNNEL-CONTROL-STREAM-1 | tests | 三个 transport 的 trait 实现和 `Data` 命令接入可验证 | complete |
| A-TUNNEL-CONTROL-STREAM-1.1 | acceptance | 生成并执行最终验收审计 | review report | A-TUNNEL-CONTROL-STREAM-1 | acceptance report | admission/test evidence 通过或明确退回 | complete |

## 退回规则
- 如果 proposal 不能支撑公开 `Tunnel` control stream API，退回 proposal。
- 如果设计无法明确内部 runtime 不公开、`Data` 命令职责、64KiB 上限、关闭传播或 transport adapter 边界，退回 design。
- 如果 implementation admission 失败，退回 checker 指向的文档阶段。
- 如果实现需要公开内部 frame/runtime、让外部直接读写 raw 控制通道、重写现有 ready/heartbeat/close/open 逻辑或把 control stream 扩成大流量业务通道，退回 proposal/design。
- 如果测试缺少 `tunnel_control_stream_api` 的直接映射或统一入口不可达，退回 testing。
- 如果 acceptance 发现文档、实现或测试不一致，按问题归属退回 design、implementation 或 testing；同一非需求问题超过 5 次仍未解决时停止并报告。

## 退出条件
- [x] proposal 为 approved
- [x] design 为 approved
- [x] testing 为 approved
- [x] implementation admission 通过 `tunnel_control_stream_api`
- [x] 公共 `Tunnel` trait 提供 `open_control_stream(...)` / `listen_control_stream(...)`
- [x] 内部 `control_stream` runtime/frame 未公开导出
- [x] TCP/QUIC/PN 只新增 `Data` 控制命令承载内部 frame
- [x] `Data` payload 最大 `64 KiB`，发送切分和接收超限拒绝均有实现
- [x] 底层控制通道断开时所有派生 control stream 断开
- [x] 已基于 approved proposal 通过最终验收

## 验证证据
- `cargo check -p p2p-frame` 已通过。
- `cargo test -p p2p-frame control_stream -- --nocapture` 已通过，执行 `networks::control_stream` 3 个单元测试。
- `python3 ./harness/scripts/test-run.py p2p-frame unit` 已通过，执行 110 个 lib unit tests，doc tests 0 个。
- `cargo test -p p2p-frame --features x509 control_stream -- --nocapture` 已通过，执行 6 个 control stream 相关测试，包含 TCP/QUIC/PN tunnel 建立后的双向收发。
- `cargo check -p p2p-frame --features x509` 已通过。
- 既有测试中已移除对已删除 `Executor::init()` / `Executor::init_new_multi_thread(...)` API 的调用；停用的 `async_std_executor.rs` 旧实现保持不变。
