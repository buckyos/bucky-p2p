# QUIC Tunnel 设计

## 目标

为 `QuicTunnel` 引入统一的 tunnel 级 channel open 握手机制，使以下语义成立：

- `open_stream(vport)` 成功，表示远端确认该 `vport` 已监听
- `open_datagram(vport)` 成功，表示远端确认该 `vport` 已监听
- 错误端口在 `open_*` 阶段立即返回 `P2pErrorCode::PortNotListen`
- `accept_stream()` / `accept_datagram()` 只向上层返回已经通过 `vport` 校验的 channel
- `listen_*()` 必须先于对应的 `accept_*()`；若未先 `listen_*()`，本地 `accept_*()` 直接失败

## 背景

当前 `QuicTunnel` 的行为是：

- `open_stream(vport)`：打开一个 bi-stream，发送 `vport` 后立即返回
- `open_datagram(vport)`：打开一个 uni-stream，发送 `vport` 后立即返回
- 接收侧 `accept_stream()` / `accept_datagram()` 只负责读出 `vport`
- 是否真正有业务 listener 由 `StreamManager` / `DatagramManager` 事后判断

这种模型的问题是：

- 错误端口无法在 tunnel 层立即失败
- datagram 是单向写通道，天然拿不到对端确认
- 上层 manager 被迫承担 tunnel 协议错误反馈职责

## 依赖接口

`QuicTunnel` 实现新的 `Tunnel` 接口：

- `tunnel_id()`
- `candidate_id()`
- `is_reverse()`
- `listen_stream(vports: ListenVPortsRef)`
- `listen_datagram(vports: ListenVPortsRef)`
- `open_stream(vport)`
- `accept_stream()`
- `open_datagram(vport)`
- `accept_datagram()`

其中 `ListenVPortsRef` 由 `StreamManager` / `DatagramManager` 通过内部 `ListenVPortRegistry` 注入。

## 总体方案

在每个 `QuicTunnel` 内部维护：

- 一个建链 `hello`
- 一条长期命令 bi-stream
- 若干数据通道
- 两套 `ListenVPortsRef`
  - stream provider
  - datagram provider

命令流负责：

- 在 tunnel 建立阶段传递 `TunnelHello { tunnel_id, is_reverse }`
- 在 tunnel 建立阶段传递 `TunnelHello { tunnel_id, candidate_id, is_reverse }`
- 发送和接收统一 `TunnelCommand`
- 维护 `Ping` / `Pong` 心跳
- 回传 datagram channel open 结果
- 对 datagram 提供 ack 能力

数据流负责：

- 实际承载 stream/datagram 数据
- 在 open 阶段先承载 `TunnelCommand::OpenChannelReq`
- 对 `stream` 再额外承载 `TunnelCommand::OpenChannelResp`
- open 成功后再进入纯业务数据阶段

## 统一命令协议

`QuicTunnel` 不再单独定义一套 QUIC 私有命令枚举，而是直接采用统一规范：

- `p2p-frame/docs/tunnel_command_protocol_design.md`

也就是说，QUIC tunnel 会复用统一命令格式，但不同命令绑定到不同通道：

- command bi-stream：`TunnelCommand::Ping`、`TunnelCommand::Pong`，以及 datagram 场景下的 `TunnelCommand::OpenChannelResp`
- stream bi-stream：`TunnelCommand::OpenChannelReq` + `TunnelCommand::OpenChannelResp`
- datagram uni-stream：`TunnelCommand::OpenChannelReq`

QUIC 文档只定义这些命令如何在 QUIC 的 command/data stream 上绑定，不再重新定义另一套命令语义。

## 命令通道

每个 `QuicTunnel` 维护一条长期命令 bi-stream，要求：

- 由连接发起方主动 `open_bi()` 建立命令流
- 接收方通过 `accept_bi()` 接收该命令流
- 接收方只有在收到 `TunnelHello` 后才创建最终 `QuicTunnel`
- `stream` / `datagram` 的 open 结果都复用统一 `TunnelCommand::OpenChannelResp`
- `datagram` 的 open 结果通过这条命令流返回
- tunnel 活性通过这条命令流上的心跳维护

### 建链 Hello

QUIC 在命令流初始化后，必须先完成一次 tunnel 级 hello：

```rust
TunnelHello {
    tunnel_id,
    candidate_id,
    is_reverse,
}

TunnelHelloResp {
    tunnel_id,
    candidate_id,
}
```

约定：

- 只有连接发起方发送 `TunnelHello`
- 接收方读取 `TunnelHello` 后，回 `TunnelHelloResp`
- 接收方创建的 tunnel `form = Passive`
- 发起方创建的 tunnel `form = Active`
- `is_reverse` 不影响 `form`，只表达这条 tunnel 是否属于反连语义
- `TunnelHelloResp.tunnel_id` 与 `TunnelHelloResp.candidate_id` 都必须原样回显，便于发起方校验
- 同一个 `tunnel_id` 下允许存在多个不同 `candidate_id` 的成功候选 tunnel

### 命令通道心跳

QUIC command stream 必须支持统一的 `Ping` / `Pong`：

- 在一个 `heartbeat_interval` 内若没有任何命令面流量，主动发送 `Ping`
- 收到 `Ping` 后必须尽快返回 `Pong`
- 在一个 `heartbeat_timeout` 窗口内既没有收到任何命令，也没有收到匹配 `Pong` 时，命令面判定失活

命令面失活后的处理：

- `QuicTunnel` 进入 `Error` 或 `Closed`
- 所有 pending 的 `OpenChannelReq` 立即失败，返回 `Interrupted`
- 后续新的 `open_stream/open_datagram` 直接返回错误

## OpenChannel 命令

```rust
TunnelCommand::OpenChannelReq(TunnelOpenChannelReq {
    request_id,
    kind,
    vport,
})

TunnelCommand::OpenChannelResp(TunnelOpenChannelResp {
    request_id,
    result,
})
```

约定：

- `result` 直接使用统一的 `TunnelCommandResult`
- `PortNotListen` 是错误端口的统一返回
- QUIC 不再定义独立于统一规范之外的结果码
- `request_id` 仍然保留在命令体中用于匹配一次 open attempt，尤其是 datagram 场景下的“数据流发请求、命令流回结果”关联

## 数据通道首命令

每条新数据通道建立后，不再额外在流头裸写 `request_id`。统一改为先发送一个命令外壳承载的 `TunnelCommand::OpenChannelReq`：

- `stream`：bi-stream 上先发送 `TunnelCommand::OpenChannelReq`
- `datagram`：uni-stream 上先发送 `TunnelCommand::OpenChannelReq`

因此，数据通道的 open 握手也统一受 `TunnelCommandHeader + body` 的版本号、`command_id`、`data_len` 校验保护，而不是依赖单独的流头字段。

## 打开流程

### open_stream(vport)

1. 本端生成 `request_id`
2. 本端打开一个新的 bi-stream
3. 在该 bi-stream 上发送 `TunnelCommand::OpenChannelReq`
4. 在同一条 bi-stream 上等待对端返回 `TunnelCommand::OpenChannelResp`
5. 若 `TunnelCommand::OpenChannelResp.result = Success`，返回该 bi-stream 的 `(read, write)`
6. 若返回失败结果，当前 bi-stream 打开失败，关闭该 bi-stream 并返回错误

### open_datagram(vport)

1. 本端生成 `request_id`
2. 本端打开一个新的 uni-stream
3. 在该 uni-stream 上发送 `TunnelCommand::OpenChannelReq`
4. 通过长期命令流等待对应的 `TunnelCommand::OpenChannelResp`
5. 若 `TunnelCommand::OpenChannelResp.result = Success`，返回该 uni-stream writer
6. 若返回失败结果，当前 uni-stream 打开失败，关闭该 uni-stream 并返回错误

## 接收流程

`QuicTunnel` 内部维护两个后台任务：

- 命令流接收循环
- bi-stream 接收循环
- uni-stream 接收循环

### 数据流接收

- 收到新的 bi-stream 后，先在该 bi-stream 上读取 `TunnelCommand::OpenChannelReq`
- 收到新的 uni-stream 后，先在该 uni-stream 上读取 `TunnelCommand::OpenChannelReq`
- 只有当 open 成功后，才把对应 stream 投递给内部 accept 队列

### 命令流接收

- 收到 `TunnelCommand::Ping`：
  - 立即返回 `TunnelCommand::Pong`
- 收到 `TunnelCommand::Pong`：
  - 更新命令面存活时间与对应序号
- 收到 `TunnelCommand::OpenChannelReq`：
  - 这在新的 QUIC 绑定下属于协议错误，因为 open request 应通过数据通道发送
  - 当前 tunnel 应按协议错误收敛
- 收到 `TunnelHello` / `TunnelHelloResp`：
  - 这在建链完成后属于协议错误
  - 当前 tunnel 应按协议错误收敛
- 收到 `TunnelCommand::OpenChannelResp`：
  - 匹配本地 pending 的 datagram open request
  - `result = Success` 时唤醒打开方
  - `result != Success` 时令打开方失败返回对应错误

### bi-stream 接收

- 收到新的 bi-stream 后，先读取 `TunnelCommand::OpenChannelReq`
- 等待对应 `ListenVPortsRef` 注入完成
- 检查 `vport` 是否监听
- 若 `vport` 合法：
  - 在同一条 bi-stream 上返回 `TunnelCommand::OpenChannelResp { result: Success }`
  - 将该 bi-stream 投递到内部 `stream` accept 队列
- 若 `vport` 非法：
  - 在同一条 bi-stream 上返回失败结果
  - 不向上层 accept 队列投递

### uni-stream 接收

- 收到新的 uni-stream 后，先读取 `TunnelCommand::OpenChannelReq`
- 等待对应 `ListenVPortsRef` 注入完成
- 检查 `vport` 是否监听
- 若 `vport` 合法：
  - 通过长期命令流返回 `TunnelCommand::OpenChannelResp { result: Success }`
  - 将该 uni-stream 投递到内部 `datagram` accept 队列
- 若 `vport` 非法：
  - 通过长期命令流返回失败结果
  - 不向上层 accept 队列投递

## accept_stream / accept_datagram

`accept_stream()` / `accept_datagram()` 不再直接读取 QUIC socket，而是改为消费 tunnel 内部队列：

- `stream_rx`
- `datagram_rx`

这些队列只包含已经完成 `vport` 校验的入站 channel。

## listen_stream / listen_datagram

`QuicTunnel` 提供：

- `listen_stream(vports: ListenVPortsRef)`
- `listen_datagram(vports: ListenVPortsRef)`

实现要点：

- 保存 provider
- 唤醒等待 provider 的控制流程
- 允许重复注入，后一次覆盖前一次
- 不要求拷贝端口列表，实时调用 `is_listen(vport)`

## 状态与并发

建议维护以下内部状态：

- `stream_vports: RwLock<Option<ListenVPortsRef>>`
- `datagram_vports: RwLock<Option<ListenVPortsRef>>`
- `bi_accept_started: AtomicBool`
- `uni_accept_started: AtomicBool`
- `pending_open_requests: Mutex<HashMap<u64, oneshot::Sender<P2pResult<()>>>>`
- `next_ping_seq: AtomicU64`
- `next_request_id: AtomicU64`
- `last_cmd_recv_at: AtomicU64` 或等价时间戳
- `last_pong_at: AtomicU64` 或等价时间戳
- `accepted_stream_tx/rx`
- `accepted_datagram_tx/rx`

必须处理的竞态：

- tunnel 关闭时需要统一清理 pending 项

## listener 尚未注册时的行为

当前语义已收紧：

- `listen_stream()` / `listen_datagram()` 必须在对应 `accept_*()` 之前完成
- `accept_bi()` / `accept_uni()` 的接收循环在首次 `listen_stream()` / `listen_datagram()` 时才启动
- 若尚未调用对应 `listen_*()`，对端 `open_*()` 可能等待到超时，而不是立即收到 `ListenerClosed`
- 本地若未先 `listen_*()` 就调用 `accept_*()`，直接返回 `P2pErrorCode::Interrupted`

也就是说，QUIC tunnel 不再等待 provider 延迟注入，而是把“开始接收入站 channel”的时机完全交给 `listen_*()` 控制。

## 错误处理

- `vport` 未监听：`P2pErrorCode::PortNotListen`
- listener 未注册：本地 `accept_*()` 看到 `P2pErrorCode::Interrupted`，对端 `open_*()` 可能超时
- 命令流断开或心跳超时：`P2pErrorCode::Interrupted`
- 数据通道首命令非法、或 datagram 的控制面回执不匹配：`P2pErrorCode::InvalidData`

## 与统一命令规范的关系

本设计是统一命令规范在 QUIC transport 下的绑定文档：

- 公共命令集合和结果码以 `p2p-frame/docs/tunnel_command_protocol_design.md` 为准
- 本文只定义 QUIC 如何承载命令流、如何把命令流与数据流关联、以及如何在 command stream 上执行心跳
- 如果未来需要新增 tunnel 级命令，应先更新统一命令规范，再更新 QUIC 绑定

## 与上层 manager 的关系

- `StreamManager` 收到新的 `QuicTunnel` 后，先调用 `listen_stream(...)`
- `DatagramManager` 收到新的 `QuicTunnel` 后，先调用 `listen_datagram(...)`
- 上层 accept loop 只消费已经通过 tunnel 内部校验的 channel
- 若上层在兜底阶段仍发现没有 listener，应立即关闭该 channel 并记日志，但这不应成为主路径语义
- `TunnelManager` 可以通过 `tunnel_id()`、`candidate_id()` 和 `is_reverse()` 把 QUIC 入站 tunnel 与同一次逻辑建链、SN 反连 waiter 对齐
- 非 reverse 的成功 QUIC 候选会被 `TunnelManager` 保留并 publish；reverse 候选在本地仍存在对应 `reverse_waiter` 时暂不 publish，waiter 被消费后后续成功候选会正常 publish

## 测试建议

- 正常 `stream` round trip
- 正常 `datagram` round trip
- 错误 `stream vport` 立即失败
- 错误 `datagram vport` 立即失败
- `stream` 在同一条 bi-stream 上完成 req/resp 后再进入业务数据阶段
- `datagram` 通过 uni-stream 发 req、命令流回 resp 的关联正确性
- 未先 `listen_*()` 时，本地 `accept_*()` 立即失败，远端 open 收到 `ListenerClosed`
- tunnel 关闭时 pending open 全部失败
