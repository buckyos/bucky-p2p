---
module: p2p-frame
task_name: 001-control-stream-runtime-fixes
submodule: 001-control-stream-runtime-fixes
version: v0.1
status: draft
approved_by:
approved_at:
approved_content_sha256:
---

# Control Stream Runtime Fixes Proposal

## Background and Goal

本任务是已批准 `tunnel_control_stream_api` 的 sibling fix packet。当前 `ControlStreamRuntime` 已能完成基本 open/listen 与双向收发，但控制接收循环会同步等待入站 callback，逐流满载错误会向上关闭整个 Tunnel，stream id 缺少可用性校验，shutdown 未保证已有数据先发送完成，关闭事件在队列满载时可能丢失，读取余量使用 `Vec::drain(..len)` 反复搬移数据。

目标是在不改变公共 `Tunnel::open_control_stream(...)` / `listen_control_stream(...)` API、既有多路复用 wire frame 集合和 TCP/QUIC/PN transport 边界的前提下，使 callback 调度、逐流隔离、stream id 准入、写入/关闭顺序、Tunnel 关闭传播和读取缓存行为可预测。

## Scope

### In scope

- 入站 virtual control stream 建立成功后，在独立 runtime task 中调用 callback，控制命令接收循环不得等待 callback future 完成。
- 单个 virtual stream 的 inbound buffer 满载时，丢弃无法接纳的当前数据帧并只终止或 Reset 该 virtual stream；不得把逐流满载传播为整个 Tunnel 的控制协议错误，不得影响其他 virtual stream。
- 入站 `Open` 必须校验 stream id 的发起方奇偶归属、当前可用性和重复/冲突状态；不可用 id 不得覆盖已有 stream 或 pending open。
- `poll_shutdown()` 必须先等待已接受的 pending data write 完成，再发送 `Fin`，保证同一 virtual stream 上 `Data` 先于 `Fin`。
- virtual stream 的终止事件必须按该流已接受数据之后的顺序交付；队列满载时不得静默丢失 Reset/终止状态，也不得因此关闭整个 Tunnel。
- 优化 `ControlStreamRead` 的剩余数据消费，避免每次小读取都从 `Vec` 头部 `drain` 并搬移未消费数据。
- 保持并修正既有 Tunnel 主导的关闭传播：底层控制通道/Tunnel 关闭后，已返回 writer 和 pending write 必须失败，不依赖外部主动 drop 或 shutdown。
- 为上述并发、隔离、顺序、关闭传播和缓存行为补充后续 design 与 post-implementation testing 映射。

### Out of scope

- 改为公开或直接交付底层 raw control transport stream。
- 删除既有 virtual control stream 多路复用模型，或改变公共 `Tunnel` control stream API。
- 因调用方主动 drop `ControlStreamRead` / `ControlStreamWrite` 而新增 wire-level Reset 保证；调用方仍按当前约定由 Tunnel 生命周期统一收敛，不在本任务增加主动 drop 协议。
- 允许逐流 buffer 满载静默形成可继续读取但中间缺字节的正常 stream；溢出流必须进入明确终止状态。
- 把 control stream 扩展为大流量业务数据平面。
- 修改 TCP/QUIC/PN 既有 heartbeat、ready、close、claim/open 或普通 stream/datagram 状态机。
- 在本 proposal 阶段修改生产代码、测试代码、design 或 testing artifact。

### Boundary with neighboring modules

- `p2p-frame/src/networks/control_stream.rs` 继续拥有私有 frame、virtual stream 状态与本地 buffer；transport adapter 只负责包装/拆出外层 `Data.payload` 和 Tunnel 关闭通知。
- TCP、QUIC、PN 不解析内部 `ControlStreamFrame`，也不为逐流满载增加 transport 级关闭策略。
- `stream_manager`、TTP 和 SN 调用方继续只依赖公共 callback/read/write 类型，不依赖内部 stream id、Reset 或 buffer 实现。

## Requirement Review

- 将 callback future 从控制接收循环中解耦是必要修正，否则 callback 等待该 stream 数据时会阻塞产生这些数据的同一接收循环。独立 task 会使 callback 完成顺序不再等同于 `Open` 到达顺序，但控制帧处理能够继续前进。
- 单流满载不应关闭整个 Tunnel。由于公共对象声明为可靠字节流，静默丢帧后继续返回正常数据会制造不可检测的数据损坏；因此选择“丢弃溢出帧并只终止/Reset 该流”，而不是让缺字节的流继续运行。
- 外部不主动 drop/close 可以作为调用约定，因此本任务不新增 drop-triggered wire Reset；但这不能替代既有 Tunnel 关闭传播。EOF、heartbeat timeout、本地 close 等仍可能在 writer 存活时发生，writer/pending write 必须观察共享关闭状态。
- Reset/终止需要保持逐流顺序，不能越过已接受 Data；同时不能依赖可能失败的单次 `try_send`。具体采用保留终止状态、队列预留、receiver close 或其他机制由 design 决定。
- stream id 校验不仅检查奇偶，还必须避免重复 Open 覆盖现有映射；具体错误响应或逐流 Reset 由 design 在不关闭健康流的前提下定义。
- 读取优化应保持现有 `AsyncRead`、EOF 与错误表面，可采用消费 offset、`Bytes`/`VecDeque` 或等价无头部搬移结构，具体结构留给 design。

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-CSRF-1 | control_stream_callback_dispatch | 入站成功 stream 的 callback 在独立 runtime task 中执行，控制接收循环不等待 callback 完成 | 保持 callback 公共签名与 OpenResp 语义 | callback 间完成顺序不保证与 Open 顺序一致 | 测试覆盖 callback 等待入站数据时控制循环仍能处理 Data，且慢 callback 不阻塞第二条流和 Tunnel 控制消息 | 不改变上层 callback API |
| P-CSRF-2 | control_stream_per_stream_overflow | inbound buffer 满载只丢弃溢出帧并终止/Reset 对应 virtual stream，其他 stream 与 Tunnel 保持可用 | 不静默维持缺字节的正常流；不把满载升级为 transport error | 满载流会明确失败，不能继续收发 | 测试以一个不消费流触发满载，同时证明另一流仍收发且 Tunnel 未关闭 | 不提供大流量公平调度或无限缓存 |
| P-CSRF-3 | control_stream_id_validation | 校验入站 stream id 的奇偶归属、重复和冲突状态，不得覆盖现有/pending stream | 内部协议仍使用现有 `u32 stream_id` | 非法对端请求会被拒绝或逐流终止 | 测试覆盖错误奇偶、重复 Open、与活动/pending id 冲突且原流不被替换 | 不改变公开 API 或 frame 字段类型 |
| P-CSRF-4 | control_stream_ordered_shutdown | shutdown 先完成已接受 pending Data，再发送 Fin | 保持现有分片上限和 AsyncWrite API | shutdown 可能等待底层发送背压解除 | 可控 sender 测试证明 wire 顺序恒为 Data 后 Fin，并覆盖发送错误 | 不保证调用方主动 drop 自动发送 Fin/Reset |
| P-CSRF-5 | control_stream_terminal_delivery | Reset/终止状态在已接受 Data 后可靠交付，队列满载不得丢失终态或关闭 Tunnel | 终态必须逐流隔离 | receiver 可能先消费已接受 Data 再观察终态 | 测试覆盖满队列、Tunnel close 和逐流 Reset 的最终可观察错误/EOF以及其他流存活 | 不要求终态越过已接受数据抢占交付 |
| P-CSRF-6 | control_stream_tunnel_close_writes | 已返回 writer 与 pending write 观察共享 runtime/Tunnel closed 状态并失败 | 不新增外部主动 drop wire Reset | writer 增加共享生命周期检查或可取消发送状态 | 测试覆盖 close_all 后旧 writer、pending write 和新 write 均失败 | 不由外部主动 drop 驱动对端清理 |
| P-CSRF-7 | control_stream_read_buffer_performance | 消费分片余量时避免反复从 Vec 头部搬移数据 | 保持字节顺序、EOF、Reset 和 AsyncRead 行为 | 需要额外 offset/容器状态 | 小 buffer 分段读取大 frame 的正确性测试及代码审查确认无头部 drain 搬移 | 不扩展 frame/queue 容量 |

## Success Criteria

- Concrete user-visible or system-visible result: 慢 callback、单流不消费、非法/重复 stream id、并发 shutdown 和 Tunnel 关闭不会阻塞或破坏其他健康 control stream；读取仍保持正确字节顺序且不反复搬移余量。
- Required evidence: 后续 design 为每个 change_id 定义状态转换、任务调度、逐流终态、Scope Paths 和验证入口；post-implementation testing 覆盖 callback 死锁回归、逐流满载隔离、ID 冲突、Data/Fin 顺序、满队列终态、Tunnel close writer 失败和小 buffer 读取；目标 unit、TCP/QUIC/PN control stream 回归及统一测试入口通过。
- Explicit non-goals: 不删除 virtual stream 多路复用，不公开 raw 控制通道，不增加主动 drop wire Reset，不把 control stream 变成大流量数据面，不改相邻 transport 生命周期协议。

## Risks

- callback 改为独立 task 后，如果 task spawn 失败或 runtime 正在退出，必须在 design 中定义 stream 的本地终止路径，不能留下已向对端确认成功但无人消费的 stream。
- 逐流满载若只丢帧而不终止，会破坏可靠流语义；若错误地继续向上返回 `OutOfLimit`，仍会关闭整个 Tunnel。
- writer 共享关闭状态与底层 send future 存在竞态；design 必须明确关闭发生在 send 前、send pending 中和 send 完成后的结果。
- Reset/终止排队若简单增加额外无界队列，会绕过 bounded-buffer 约束；不得以 unbounded channel 修复终态丢失。
- stream id 校验错误可能拒绝合法的双向并发 open；奇偶所有权必须以 `is_initiator` 的对端视角定义并覆盖双向测试。

## Approval Record

- approver:
- approval_date:
- user_statement: ""
