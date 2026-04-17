# TCP Tunnel 协议设计

> 注：本文描述 `p2p-frame/src/networks/tcp/` 当前采用的 TCP tunnel 线协议与实现约束；若与更上层的 tunnel 管理策略相关，以上层约束为准。

## 背景

`p2p-frame/src/networks/tcp/` 当前已经把 TCP 传输拆成两层：

- 一条长期存在的 control connection 负责握手、心跳与控制命令
- 一组独立的 TCP/TLS data connection 承载 `stream` / `datagram` 的业务数据

这已经说明，在 TCP 传输下，`Tunnel` 不是“一条物理连接上复用多个 channel 的 framing connection”，而是一个逻辑对象：

- 它拥有唯一的 tunnel 级控制面
- 它维护若干可建立、可绑定、可回收、可复用的 data connection
- 上层只感知一个 `TcpTunnel`

这套协议最初的重构动机，来自更早期实现中依赖句柄 drop 与 `EnterIdle` 回收连接的方式。

这个方案有几个核心问题：

1. control 与 data 是两条独立连接，没有跨连接顺序保证
2. 一侧收到“对端已 idle”时，不能证明对端最后的数据已经全部到达并被消费
3. 同一条 data connection 被多次复用后，旧控制消息可能误作用到新一轮 channel
4. 如果双边都拥有复用权，同一条 idle connection 会出现争抢
5. 现有文档只覆盖 data connection reuse，不能单独定义完整的 TCP tunnel 生命周期
6. tunnel 建立、control 连接冲突、data connection 注册、channel 绑定与连接回收在语义上彼此关联，拆开描述容易留下协议空洞

因此，不能只保留一份“data connection 复用子协议”文档，而需要一份完整的 `tcp tunnel` 协议设计，统一描述：

- tunnel 如何建立
- control connection 如何成为唯一的一致性基础
- data connection 如何建立并注册到 tunnel
- channel 如何绑定到某条 data connection 的某一轮租约
- 一轮 channel 何时才算真正结束并可安全回收
- control/data 乱序、旧消息、并发 claim、失败退役如何收敛

本文将这些问题统一纳入 `TCP Tunnel` 协议本身，其中 data connection reuse 是 tunnel 协议中的一个子协议，而不是全文唯一主题。

## 目标

- 定义完整、可独立实现的 TCP tunnel 协议，不依赖其他旧文档
- 保留 `TcpTunnel` 的逻辑对象语义：一条 control connection + 一组 data connection
- 明确 tunnel 建立完成、data connection 注册完成、channel 绑定完成分别意味着什么
- 保留 TCP data connection 复用能力
- data connection 在完成建立握手后继续传裸数据，不在业务数据流里插入控制帧
- data connection 在进入对称复用态后，双边都拥有复用权
- 保证旧命令、跨连接先后不一致、竞争 claim 不会破坏一致性
- 将“channel 完成”和“connection 可复用”定义成可验证状态，而不是仅靠本地 drop 句柄推断
- 让后续 `protocol.rs`、`network.rs`、`listener.rs`、`tunnel.rs` 可以直接按本文重写

## 非目标

- 不改成“一条 TCP 物理连接上承载多个 frame channel”的总线式协议
- 不改成 data 通道全量 frame 化
- 不依赖底层 TCP/TLS 半关闭作为复用边界
- 不要求一条 data connection 同时承载多个 channel
- 不把未注册连接上的 claim 当作正常乱序场景恢复；这类情况按协议错误处理
- 不尝试在复用失败时做复杂自动恢复；优先保证正确性，失败时允许直接 retire 连接
- 不定义 `Proxy` tunnel 形态；本文只覆盖 `Active` 与 `Passive`
- 不展开 `TunnelManager`、`StreamManager`、`DatagramManager` 的内部路由与订阅实现

## 与新 Tunnel 接口的对齐

在新的 tunnel 抽象下，TCP transport 需要对齐如下上层语义：

- `Tunnel` 需要实现 `listen_stream(ListenVPortsRef)` 与 `listen_datagram(ListenVPortsRef)`
- `StreamManager` / `DatagramManager` 在收到新的 `TcpTunnel` 后，会先调用对应的 `listen_*` 接口，再启动各自的 accept loop
- `ListenVPortsRef` 由上层内部的 `ListenVPortRegistry` 提供，`TcpTunnel` 只能通过 `is_listen(vport)` 判断远端某个 `vport` 当前是否正在监听
- 若未先调用 `listen_*` 就直接调用对应的 `accept_*`，本地应立即返回中断错误；对端 claim/open 则应得到 `ListenerClosed`
- `open_stream(vport)` / `open_datagram(vport)` 成功，不再只表示 claim 成功，而表示远端已经确认该 `vport` 可接受
- 错误 `vport` 的主路径拒绝点下沉到 `TcpTunnel` 的 claim 处理阶段，而不是由上层 manager 在 `accept_stream()` / `accept_datagram()` 后静默丢弃
- TCP 的 control connection 命令面需要对齐统一规范 `p2p-frame/docs/tunnel_command_protocol_design.md`
- 其中 `Ping/Pong` 直接是统一命令的 TCP binding，`ClaimConnReq/Ack(result)` 则是 `TunnelOpenChannelReq/Resp` 在 TCP reuse 模型下的扩展 binding

因此，本文后续出现的：

- `vport` 校验
- `ListenerClosed`
- `AcceptQueueFull`
- `VportNotFound`

都应理解为 `TcpTunnel` 在已经持有 `ListenVPortsRef` 的前提下，对当前 claim / channel open 请求做出的 tunnel 级判断。

## 协议总览

TCP tunnel 协议由四个串行子阶段组成：

1. control connection 建立与 tunnel 注册
2. data connection 建立与注册
3. channel claim / bind / transfer
4. channel drain / reuse / retire

只有先满足第 1 步的全局建立完成条件（active 侧收到 `ControlConnReady(Success)`，passive 侧至少进入 `PassiveReady`），才允许进入第 2 步；只有先完成第 2 步，才允许进入第 3 步；只有第 4 步完成，某条 data connection 才允许重新回到 `Idle`。

## 术语

### `TcpTunnel`

设备 A 与设备 B 之间的一个逻辑 TCP tunnel 实例，上层感知为一个 `Tunnel`。

在 TCP 传输下，本文中的 `TcpTunnel` 是一个逻辑对象：它由一条 control connection 与其管理的一组 data connection 组成，而不是“一条物理连接上同时承载多个 channel”的 framing 连接。

同一对设备之间是否只保留一个健康的 `TcpTunnel`，由更上层的 tunnel 管理策略决定，不由 `TcpTunnelNetwork` / `TcpTunnelListener` 在传输层内部强制裁决。

### `Control Connection`

TCP tunnel 的唯一控制连接。它负责：

- tunnel 建立与确认
- tunnel 心跳
- data connection 的反向建立请求
- channel claim / ack / nack
- `WriteFin` / `ReadDone`
- tunnel 关闭后的一致性收敛

单个 `TcpTunnel` 实例在任意时刻必须只有一条 control connection。

### `Data Connection`

实际承载业务字节流的 TCP/TLS 连接。一个 data connection 在生命周期内可以多次被不同 channel 复用，但任意时刻只服务一个 channel。

### `Registered Data Connection`

已经完成以下条件的 data connection：

- 双方完成 TCP/TLS 建立
- 双方通过 `TcpConnectionHello(role = Data)` 确认该连接属于某个 `Tunnel`
- 被动接收侧已经把该连接登记到本地 `conn_id -> connection` 表，并已发送 `DataConnReady`
- 创建方已经在该 data connection 上收到 `DataConnReady(Success)`

registered 只表示这条连接已经完成注册握手，不表示双方已经对称地拥有 claim 权。

为避免把“某一侧已经完成本地登记”与“双方都确认注册完成”混为一体，本文固定区分两层语义：

- `locally registered`：被动接收侧已经完成本地 `conn_id -> connection` 登记，并已发送 `DataConnReady(Success)`
- `registered data connection`：创建方已经收到匹配的 `DataConnReady(Success)`，且双方都已进入各自等待首轮 claim 的本地状态

除非特别说明，后文的 `registered data connection` 默认指第二层，也就是双方已经完成这一轮注册握手的全局条件。

### `Channel`

一次逻辑业务通道：

- `stream` 是双向通道
- `datagram` 在当前接口下是单向发送 / 单向接收通道

这里的 `datagram` 仅表示 channel API 语义，不表示底层 data connection 具备消息边界；在 TCP 下，它仍然通过裸字节流承载，并沿用与 `stream` 相同的租约、drain 与回收规则。

一个 channel 只绑定一条 data connection 的某一轮租约。

### Active Tunnel / Passive Tunnel

- control connection 主动建立方视角下，本地 tunnel form 为 `Active`
- control connection 被动接受方视角下，本地 tunnel form 为 `Passive`

另外，TCP tunnel 还单独携带 `is_reverse`：

- `form = Active, is_reverse = false`：普通主动直连
- `form = Active, is_reverse = true`：本端响应 SN called 主动回拨建立的反连 tunnel
- `form = Passive, is_reverse = false`：对端普通主动连入
- `form = Passive, is_reverse = true`：对端以反连语义连入

`form` 只描述 tunnel 的建立方向，不表示是否属于反连语义；一旦 tunnel 建立完成，双方都可以在协议允许的时机创建 data connection、发起 claim、接收对端 claim。

### `Lease`

同一条 data connection 在某一轮被某个 channel 占用的生命周期。lease 的唯一主键为 `(conn_id, lease_seq)`。

### `Retired`

本地已确认某条 data connection 不再可复用。进入 `Retired` 后：

- 必须立即关闭对应物理连接
- 清理该连接相关的 pending 状态
- 之后不得再回到其他状态

## Tunnel 建立协议

本章节定义 control connection 的建立、注册、确认与失败处理。本文中的单个 TCP tunnel 实例只有在满足本章定义的全局建立完成条件后，才视为“tunnel 已建立”。

### 协议边界

- 本章节负责 control connection 建立与 tunnel 注册
- 本章节不负责 data connection 建立
- 本章节不负责 channel 绑定
- 本章节结束条件是：双方就同一个 `tunnel_id` 完成该 tunnel 实例的 control connection 建立，active 侧进入 `Connected`，passive 侧至少进入 `PassiveReady`

### control 连接首命令

所有新的 TCP/TLS 物理连接在完成 TLS 后，都必须先发送一个统一命令外壳承载的 `TcpConnectionHello`，用来让接收端区分这是 control connection 还是 data connection。

也就是说，fresh connection 的第一条协议消息固定为：

```text
[TunnelCommandHeader(command_id = TcpConnectionHello::COMMAND_ID)]
[TcpConnectionHello body]
```

`TcpConnectionHello` 是 TCP tunnel 在统一命令框架下的首命令，而不是裸结构体首帧。

```rust
enum TcpConnectionRole {
    Control,
    Data,
}

struct TcpConnectionHello {
    role: TcpConnectionRole,
    tunnel_id: TunnelId,
    candidate_id: TunnelCandidateId,
    is_reverse: bool,
    conn_id: Option<u64>,
    open_request_id: Option<u64>,
}
```

字段约束：

- 当 `role = Control` 时：
  - `candidate_id` 表示这条具体 candidate tunnel 实例
  - `is_reverse` 表示这条 control connection 是否属于反连语义
  - `conn_id = None`
  - `open_request_id = None`
- 当 `role = Data` 时：
  - `candidate_id` 必须与所属 control connection 一致
  - `is_reverse = false`
  - `conn_id = Some(u64)`
  - `open_request_id` 可为 `None` 或 `Some(request_id)`

### control 建连确认消息

control connection 自身在 hello 之后，还必须在同一条 control connection 上返回一次统一命令外壳承载的显式确认，表示 tunnel 注册是否成功。

```rust
struct ControlConnReady {
    tunnel_id: TunnelId,
    candidate_id: TunnelCandidateId,
    result: u8,
}

enum ControlConnReadyResult {
    Success = 0,
    TunnelConflictLost = 1,
    ProtocolError = 2,
    InternalError = 3,
}
```

语义：

- `Success`：接收端已经接受这条 control connection，并把它注册为本地该 `TcpTunnel` 实例的控制连接
- `TunnelConflictLost`：当前 control connection 被更上层的 tunnel 唯一性 / 冲突策略拒绝，接收端不会使用这条连接继续建立 tunnel
- `ProtocolError`：hello 字段或上下文非法
- `InternalError`：接收端发生内部错误，当前连接不得继续使用
- 返回 `ControlConnReady` 时，`tunnel_id` 与 `candidate_id` 必须原样回显本条 control connection 上先前 `TcpConnectionHello(role = Control)` 里的值
- 主动建立方收到 `ControlConnReady` 时，必须校验它来自当前这条等待中的 control connection，且 `(tunnel_id, candidate_id)` 与本端刚刚发送的 hello 完全一致；否则当前 candidate tunnel 必须按 `ProtocolError` 失败并关闭该 control connection
- 一条 control connection 在建连阶段只允许出现一次 `ControlConnReady`；重复或矛盾的 ready 都属于 `ProtocolError`
- `TcpConnectionHello` 与 `ControlConnReady` 都必须遵循统一命令外壳的校验规则：`version`、`command_id`、`data_len` 必须与实际命令体一致

### `tunnel_id`

`tunnel_id` 是 control connection 代表的逻辑 TCP tunnel ID，当前实现类型固定为 `u32`。

本文固定采用以下分区规则，所有实现都必须一致：

- 若 `local_id.as_slice()` 的字节字典序小于 `remote_id.as_slice()`，则本端创建的 `tunnel_id` 最高位恒为 `0`
- 若 `local_id.as_slice()` 的字节字典序大于 `remote_id.as_slice()`，则本端创建的 `tunnel_id` 最高位恒为 `1`
- 其余 31 位由 tunnel 创建方在本地按单调递增序列分配
- `tunnel_id = 0` 视为无效值，不参与正常建连
- `tunnel_id` 在单个远端对生命周期内必须视为一次性 ID；一旦分配，无论后续成功、失败还是冲突失败，都不得复用

这样可以让双方在并发建 tunnel 时，对“某个 `tunnel_id` 是哪一侧创建的”得出一致结论，并避免双边本地计数器从相同初值开始时发生碰撞。

更上层在一次逻辑建链里（多 endpoint 并发、direct+reverse hedged）必须复用同一个 `tunnel_id`；transport 首个 `hello` 必须显式携带这个 `tunnel_id`，这样接收侧才能把多条候选连接归并到同一次建链。

### `candidate_id`

`candidate_id` 是单次逻辑 `tunnel_id` 下某条具体 candidate tunnel 的实例标识。

- 同一个 `tunnel_id` 下允许存在多个不同的 `candidate_id`
- 同一个 `(local_id, remote_id, tunnel_id, candidate_id)` 才唯一标识一条具体 TCP tunnel candidate
- 非 reverse 候选全部保留并 publish；reverse 候选在本地仍存在对应 `reverse_waiter` 时暂不 publish，waiter 被消费后后续成功候选会正常 publish

### 上层唯一性与冲突裁决

是否允许任意两个设备之间同时存在多个健康 `TcpTunnel`，由更上层管理策略决定；TCP transport 层本身只按 `(local_id, remote_id, tunnel_id, candidate_id)` 区分 tunnel candidate 实例。

因此：

- `TcpTunnelNetwork::create_tunnel()` 每次都创建新的 candidate tunnel，不复用已有 tunnel
- `TcpTunnelListener` 对合法的 control hello 默认接受，不在 transport 层内部做“同对端只保留一个 tunnel”的裁决
- 若更上层需要“同一远端对只保留一个健康 tunnel”，应由更上层在 candidate tunnel 之上做去重、关闭与切换
- `ControlConnReady(TunnelConflictLost)` 仍保留为合法线协议结果，供更上层策略在需要时使用；但当前 transport 默认路径不依赖它做本地冲突裁决

若更上层启用了唯一性策略，则具体胜负规则、旧 tunnel 关闭时机、是否允许在线切换，都属于 transport 之外的编排逻辑；本文不在 `network.rs` / `listener.rs` 中定义该策略。

### 正常时序：主动建立 tunnel

1. A 需要与 B 建立 TCP tunnel，分配新的 `tunnel_id`
2. A 与 B 建立 TCP/TLS control connection
3. A 在 control connection 上发送 `TcpConnectionHello { role: Control, tunnel_id, candidate_id, is_reverse, conn_id: None, open_request_id: None }`
4. B 收到 hello 后：
   - 校验字段组合合法
   - 若接受该 control connection，则创建本地 `Passive` tunnel 对象并注册该 control connection
5. B 在同一条 control connection 上回复 `ControlConnReady { tunnel_id, candidate_id, Success }`
6. B 在发送 `ControlConnReady(Success)` 后，视为本地已经接受该 control connection，并将 tunnel 标记为 `PassiveReady`
7. 在 active 侧收到 `ControlConnReady(Success)` 之前，B 不得主动发送除必要响应外的后续 tunnel 控制命令
8. A 收到 `ControlConnReady(Success)` 后，将本地 tunnel 状态视为 `Connected`；此时该 tunnel 已满足全局建立完成条件
9. B 后续在同一 tunnel 上观察到来自 A 的任一合法后续 tunnel-scoped 消息（例如新的 control 命令，或引用该 `tunnel_id` 的合法 data connection hello）后，才可把本地状态从 `PassiveReady` 提升为 `Connected`

### 失败处理

如果 control 建连阶段出现以下情况，当前 candidate tunnel 必须直接失败：

- `TcpConnectionHello(role = Control)` 字段非法
- 未收到 `ControlConnReady`
- 收到 `ControlConnReady(Failed...)`
- control connection 在建连完成前关闭
- 被更上层唯一性策略显式拒绝（若启用）

建议：

- `ControlConnReady` 等待超时默认取 `connect_timeout`
- 若 active 侧在超时时间内未收到 `ControlConnReady(Success)`，则本次 tunnel 建立失败
- 失败的 `tunnel_id` 不得复用

### tunnel 建立完成条件

一条 TCP tunnel 只有同时满足以下条件，才视为“建立完成”：

- control connection 已完成 TLS 建立
- active 侧已经发送 `TcpConnectionHello(role = Control)`
- passive 侧已经接受这条 control connection，并返回 `ControlConnReady(Success)`
- active 侧已经收到 `ControlConnReady(Success)`

这里的“tunnel 建立完成”是全局条件，不等同于 passive 侧已经完成本地接受：passive 侧可以先完成本地注册，但在 active 侧收到 `ControlConnReady(Success)` 之前，仍不得据此主动启动后续 tunnel 控制流量。

这里进一步约定：`Connected` 与“全局建立完成”不是完全同义的本地事件。

- 对 active 侧，收到 `ControlConnReady(Success)` 就是本地进入 `Connected` 的判定点
- 对 passive 侧，发送 `ControlConnReady(Success)` 后只进入 `PassiveReady`；只有后续观察到来自 active 侧的合法 tunnel-scoped 消息，证明对端已经越过建连闸门后，才进入本地 `Connected`

在此之前：

- 不得发送 `Ping/Pong`
- 不得发送 `OpenDataConnReq`
- 不得把任何 data connection 注册到该 `tunnel_id`

### tunnel 状态

本文对 tunnel 级状态定义以下最小集合：

```text
Connecting
  -> PassiveReady   (仅被动侧在发出 `ControlConnReady(Success)` 后)
  -> Connected
  -> Closed
  -> Error

PassiveReady
  -> Connected      (观察到 active 侧后续合法 tunnel-scoped 消息)
  -> Closed
  -> Error

Connected
  -> Closed
  -> Error
```

其中：

- `Connecting`：control connection 建立中，尚未完成 `ControlConnReady(Success)`
- `PassiveReady`：仅 passive 侧可见；本地已经接受 control connection 并发出 `ControlConnReady(Success)`，但尚未观察到 active 侧已经越过建连闸门的后续 tunnel-scoped 证据，因此仍不得主动启动后续 tunnel 控制流量
- `Connected`：tunnel 已全局建立完成，可进入后续 data/claim 流程
- `Closed`：本地主动关闭或远端正常关闭
- `Error`：超时、协议错误、I/O 错误、控制面一致性丢失

## Tunnel 心跳协议

一旦 TCP tunnel 进入 `Connected`，control connection 上必须维护固定的存活判定机制。

```rust
struct PingCmd {
    seq: u64,
    send_time: Timestamp,
}

struct PongCmd {
    seq: u64,
    send_time: Timestamp,
}
```

语义：

- `PingCmd` 用于探测对端 control connection 存活
- `PongCmd` 必须原样回显 `seq` 与 `send_time`
- tunnel 在 `Connected` 后，所有复用相关的一致性都依赖 control connection 存活

协议要求：

- 双方在 `Connected` 后都必须持续维护 control connection 的存活判定
- 若在一个 `heartbeat_timeout` 窗口内，既未收到任何 control 命令，也未收到匹配的 `PongCmd`，则必须把 control connection 判定为失活

默认发送策略：

- active 侧在本地进入 `Connected` 后，若没有更高优先级的后续 tunnel-scoped 消息需要立即发送，应尽快发送一次 `PingCmd`；这也为 passive 侧从 `PassiveReady` 提升到本地 `Connected` 提供确定性触发点
- 当在一个 `heartbeat_interval` 内未收到任何 control 命令时，主动发送 `PingCmd`

## 标识与约束

### `conn_id`

物理 data connection 的唯一 ID。要求在单个 `Tunnel` 内唯一即可，类型为 `u64`。

本文固定采用以下分区规则，所有实现都必须一致：

- control 连接主动建立方创建的 data connection，`conn_id` 最高位恒为 `0`
- control 连接被动接受方创建的 data connection，`conn_id` 最高位恒为 `1`
- 其余位由 data connection 的创建方在该 `Tunnel` 内按单调递增计数分配
- `conn_id` 只用于 data connection，control connection 不参与这个编号空间
- `conn_id` 在单个 `Tunnel` 生命周期内必须视为一次性 ID；一旦分配，无论后续注册成功、失败、claim 成功、claim 失败、连接关闭还是进入 `Retired`，都不得复用

只要双方都知道 control connection 的主被动方向，就必须能仅凭 `conn_id` 的分区规则，对“该 data connection 的创建方是谁”得出完全一致的结论；首轮 claim 权也完全由这个结论决定。

### `lease_seq`

同一条 `conn_id` 被第几次租用。

- 每个 `conn_id` 在本地维护一个已提交的 `lease_seq`
- 发起新一轮 claim 时，使用 `next_lease_seq = committed_lease_seq + 1`
- 只有 claim 成功并实际进入 `Bound(new channel)` 后，才把本地 `committed_lease_seq` 更新为 `next_lease_seq`
- 若 claim 超时、冲突失败、收到拒绝、或本轮绑定过程失败，则 `committed_lease_seq` 保持不变
- 所有和复用相关的控制命令都必须带上 `(conn_id, lease_seq)`
- 对 `WriteFin`、`ReadDone`、`ClaimConnAck(result != Success)` 这类非 claim 请求消息，只要 `lease_seq` 不匹配当前租约，就必须按 stale 或矛盾消息规则处理；不得影响当前租约

例子：

- `conn_id = 42, lease_seq = 1` 被 `channel_id = 1001` 使用
- 回收到 idle 后再次复用
- 新一轮变成 `conn_id = 42, lease_seq = 2`
- 这时旧的 `lease_seq = 1` 命令如果迟到，不能再影响当前连接状态

### `claim_nonce`

双边同时争抢同一条 idle connection 时使用的随机数。类型固定为 `u64`。

- `claim_nonce` 只在“双方都对同一个 `(conn_id, lease_seq)` 发起 claim”时参与决策
- 它的目的不是表达连接优先级，而是为一次并发冲突提供稳定、可复现的胜负结果

### `channel_id`

逻辑 channel 的唯一 ID。类型固定为 `u64`，并只要求在单个 `Tunnel` 内唯一。

生成规则固定与 `conn_id` 保持一致：

- control 连接主动建立方生成的 `channel_id` 最高位恒为 `0`
- control 连接被动接受方生成的 `channel_id` 最高位恒为 `1`
- 其余位在本端该 `Tunnel` 内按单调递增计数分配

### `request_id`

通过 control 请求对端反向创建 data connection 时使用的一次性 ID。类型固定为 `u64`。

- `request_id` 只要求在单个 `Tunnel` 生命周期内唯一
- 一旦分配，无论后续成功、失败、超时还是取消，都不得复用
- `request_id` 只用于把 control 面上的一次反向建连请求，与 data 面上晚些真正到达的一条新 data connection 关联起来

### lease_seq 的提交规则

每个 `conn_id` 独立维护：

- 初次建立连接并完成注册后，已提交值设为 `0`
- 每次发起 claim 时，本地计算候选值 `next_lease_seq = committed_lease_seq + 1`
- 每次成功 claim 并绑定给新 channel 时，才把已提交值更新为该 `next_lease_seq`
- claim 失败、超时、冲突失败、收到拒绝时，已提交值保持不变
- `lease_seq` 的提交必须与本地状态迁移到 `Bound` 视为同一个原子动作

### 任意时刻的唯一性

为避免把“物理连接生命周期”与“某一轮租约生命周期”混成一层，本文约定：

- `Connecting` / `Registering` / `FirstClaimPending` / `Idle` / `Retired` 是 `conn_id` 级状态
- `Claiming` / `Bound` / `Draining` 是“连接当前正携带某一轮租约”的状态；一旦连接进入这些状态，本地必须同时持有唯一的当前租约上下文 `(conn_id, lease_seq, channel_id)`
- `lease_seq` 只用于描述某条已注册 data connection 的某一轮候选租约或已提交租约，不单独脱离 `conn_id` 存在

任意时刻：

- 一个 `conn_id` 最多只能有一个当前租约上下文
- 同一个 `conn_id` 不允许在同一个 `lease_seq` 下同时服务多个 channel
- 若本地已经为某个 `conn_id` 进入 `Claiming/Bound/Draining`，则不得再为该 `conn_id` 并行创建第二个候选或活动租约

其中 `Retired` 的处理语义必须明确为：本地一旦将连接判定为 `Retired`，就立即关闭对应的物理 data connection，并且后续不得再复用、不得再回到其他状态。

### claim 前置条件

`ClaimConnReq` 只能针对已经完成注册的 data connection 发送。

- 未知 `conn_id` 的 claim 不是正常乱序，而是 `ProtocolError`
- 未注册 `conn_id` 的 claim 不是正常乱序，而是 `ProtocolError`
- `DataConnReady(Success)` 到达前，创建方不得把连接视为可发起 claim 的 `FirstClaimPending`
- 被动接收侧在本地注册并发送 `DataConnReady` 后，可以把连接视为 `FirstClaimPending`，但该状态下只允许接收首轮 claim，不允许主动发起 claim
- 只有连接已经至少完成过一轮成功绑定，并从 `Draining` 安全回到 `Idle` 后，双方才都可以对该连接主动发起后续 claim

### Claim 时的 `lease_seq` 判定规则

对每个已注册 `conn_id`，本地先计算：

- `committed_lease_seq`：当前已提交租约
- `expected_next_lease_seq = committed_lease_seq + 1`

发起方规则：

- 连接处于 `FirstClaimPending` 时，只允许创建方发起首轮 claim，且 `lease_seq` 必须为 `1`
- 连接处于 `Idle` 时，双方都只能发起 `lease_seq = expected_next_lease_seq` 的 claim
- 发起方不得跳号发送未来 lease，也不得重发一个已经提交过的旧 lease 作为新 claim

接收 `ClaimConnReq` 时，按以下顺序判定：

1. 若 `conn_id` 未知或未注册：`ProtocolError`
2. 若连接已 `Retired`：返回 `ClaimConnAck { result = Retired }`
3. 先做 `lease_seq` 与当前连接状态的可受理性判定；这一步先于 `vport/listener/accept queue` 校验，用来尽早过滤 stale / future / not-idle claim：
   - 若本地状态为 `FirstClaimPending`：
     - 只有首轮 `lease_seq = 1` 是合法候选值
     - 若该请求来自首轮创建方，则按首轮 claim 正常处理
     - 若该请求来自被动接收侧自己不应拥有的首轮主动 claim，统一返回 `ClaimConnAck { result = ProtocolError }`，并保持当前连接状态不变
     - 若 `lease_seq != 1`，返回 `ClaimConnAck { result = LeaseMismatch }`
   - 若本地状态为 `Idle`：
     - 只有 `lease_seq = expected_next_lease_seq` 才能进入正常 claim / 冲突仲裁流程
     - 若 `lease_seq < expected_next_lease_seq`，说明是旧 claim，返回 `ClaimConnAck { result = LeaseMismatch }`
     - 若 `lease_seq > expected_next_lease_seq`，说明是跳号或未来 claim，返回 `ClaimConnAck { result = LeaseMismatch }`
   - 若本地状态为 `Claiming`：
     - 若 `lease_seq = expected_next_lease_seq`，且命中同一 `(conn_id, lease_seq)`，进入 `claim_nonce` 冲突仲裁
     - 其余 `lease_seq` 按 `LeaseMismatch` 处理
   - 若本地状态为 `Bound` 或 `Draining`：
     - 对当前连接的任何新 `ClaimConnReq` 都返回 `ClaimConnAck { result = NotIdle }`
4. 只有当前 3 步通过后，才继续校验本地目标接收条件（`vport`、`ListenVPortsRef`、accept queue）
5. 只有请求同时通过“状态/lease 可受理性”与“本地目标接收条件”校验，且本地也正处于同一 `(conn_id, lease_seq)` 的 `Claiming` 时，才允许进入 `claim_nonce` 冲突仲裁

这里约定：

- `NotIdle` 表示连接当前不在可接受新 claim 的窗口，即使 `lease_seq` 数值本身看起来合法也不能绑定
- `LeaseMismatch` 表示请求里的 `lease_seq` 不是当前状态下唯一允许的那个值
- 首轮 claim 的特殊性只存在于 `FirstClaimPending -> lease_seq = 1` 这一轮；一旦连接成功经历过一轮完整生命周期，后续都按普通 `Idle -> expected_next_lease_seq` 规则处理

## 设计原则

### 原则 1：tunnel 建立、data 建连与复用阶段分离

- control connection 先建立并确认 tunnel
- data connection 在进入业务传输前，允许承载极少量建连握手帧：`TcpConnectionHello(role = Data)` 与 `DataConnReady`
- 只有完成注册后，这条 data connection 才进入“后续只传业务数据”的阶段
- 所有 channel 绑定、复用、完成、失败、退役都通过 control 通道传递

### 原则 2：channel 完成不等于连接可复用

一轮 channel 结束至少分两层：

1. 逻辑层结束：双方都不再使用这个 channel
2. 复用层结束：双方都确认该轮数据已经完全 drain，旧字节不会污染下一轮

只有第 2 层完成后，连接才可回 idle pool。

### 原则 3：失败时优先 retire，而不是强行复用

当存在以下情况时，连接直接退役：

- 本地 read 已提前释放，但还没读满对端声明的最终字节数
- control 状态与 data 状态不一致
- claim 冲突后的本地状态无法确定
- 连接读写错误
- control connection 关闭
- 建连 / 注册阶段出现协议错误

### 原则 4：TCP tunnel 只有一个控制面真相源

- 所有影响租约与回收的一致性判断，都必须以 control connection 为唯一控制面真相源
- 一旦 control connection 断开，所有后续仍想依赖控制面推进的 data connection，都不得再回到可复用态
- data connection 本身不承载“下一轮 claim / 当前轮回收”控制命令

## Data Connection 建立与注册协议

本章节只负责 data connection 的建立、注册、失败处理，以及将连接推进到首轮可绑定态 `FirstClaimPending`。

### 协议边界

- 本章节不负责 channel 绑定
- 本章节不负责 `WriteFin` / `ReadDone`
- 本章节结束条件是：被动接收侧完成本地注册并发送 `DataConnReady(Success)`，创建方收到匹配的 `DataConnReady(Success)`，且双方都进入各自等待首轮 claim 的本地状态 `FirstClaimPending`
- 只有本章节成功完成后，才允许由连接创建方发起首轮 `ClaimConnReq`

### 建连消息

并不是所有 data connection 的建立都需要先走 control 命令。

- 若本端可以直接发起到对端的 data connection（例如本端选择主动直连，或已判断本地与远端处于同一局域网），则直接建立 data connection，不发送 `OpenDataConnReq`
- 只有在需要请求对端负责创建一条反向 data connection 时，才通过 control 通道发送建连请求：

```rust
struct OpenDataConnReq {
    request_id: u64,
}
```

真正的 data connection 自身在建立后，使用统一命令外壳承载的首命令完成注册握手：

```rust
// role 固定为 Data
struct TcpConnectionHello {
    role: TcpConnectionRole,
    tunnel_id: TunnelId,
    candidate_id: TunnelCandidateId,
    conn_id: Option<u64>,
    open_request_id: Option<u64>,
}

struct DataConnReady {
    conn_id: u64,
    candidate_id: TunnelCandidateId,
    result: u8,
}
```

`DataConnReady.result` 定义为稳定枚举：

```rust
enum DataConnReadyResult {
    Success = 0,
    TunnelNotFound = 1,
    ConnIdConflict = 2,
    ProtocolError = 3,
    InternalError = 4,
}
```

### 语义

- `OpenDataConnReq.request_id` 在单个 `Tunnel` 生命周期内必须视为一次性 ID；一旦分配，无论后续成功、失败、超时还是取消，都不得复用
- `OpenDataConnReq.request_id` 只用于“请求对端创建一条反向 data connection”，不适用于本端可直接发起的新建流程，也不携带 channel 语义
- 若该 data connection 是为了响应某个 `OpenDataConnReq(request_id)` 而创建，则创建方必须在 `TcpConnectionHello.open_request_id` 中回填该 `request_id`
- `TcpConnectionHello(role = Data)` 用于让接收端识别“这条 data connection 属于哪个 `(tunnel_id, candidate_id)`、它的 `conn_id` 是多少、是否对应某个反向建连请求”
- 对 data connection 而言，`conn_id` 为必填字段；若 `role = Data` 但 `conn_id = None`，则为 `ProtocolError`
- `DataConnReady(Success)` 表示接收端已经成功完成本地注册
- `DataConnReady(Failed...)` 表示这条 data connection 不得进入 claim 阶段，发送后应关闭该连接；即使失败，`DataConnReady.conn_id` 也必须回显该连接在 `TcpConnectionHello` 中携带的 `conn_id`
- `OpenDataConnReq` 没有单独的正向 `Ack`；请求方是否成功，最终只以匹配的反向 data connection 是否真正到达并完成注册为准
- 返回 `DataConnReady` 时，`conn_id` 与 `candidate_id` 都必须原样回显本条 data connection 上先前 `TcpConnectionHello(role = Data)` 里的值
- 创建方收到 `DataConnReady` 时，必须校验它来自当前这条等待中的 data connection，且 `conn_id` 与 `candidate_id` 都与本端刚刚发送的 hello 完全一致；否则当前 data connection 必须按 `ProtocolError` 关闭
- 一条新的 data connection 在注册阶段只允许出现一次 `DataConnReady`；重复或矛盾的 ready 都属于 `ProtocolError`
- `TcpConnectionHello` 与 `DataConnReady` 都必须遵循统一命令外壳的校验规则：`version`、`command_id`、`data_len` 必须与实际命令体一致
- data connection 只在注册阶段发送这两条命令；一旦 `DataConnReady(Success)` 完成，该物理连接后续立即切回裸业务字节流，不再继续发送统一命令外壳消息

关于“直接建连”与“反向建连”的选择：

- 当本端具备直接拨号条件时，应优先直接建立 data connection
- 只有本端无法直接建立、而需要由对端反向连接回来时，才发送 `OpenDataConnReq`
- 若已判断本地与远端位于同一局域网，则双方在各自需要新建 data connection 时都应直接连接，不走 `OpenDataConnReq`

关于 `open_request_id` 的约束：

- 本地主动直接新建的 data connection，`open_request_id = None`
- 同一局域网场景下任意一侧直接新建的 data connection，`open_request_id = None`
- 响应 `OpenDataConnReq(request_id)` 创建的反向 data connection，`open_request_id = Some(request_id)`
- 请求方应维护 `pending_open_request[request_id]`
- 即使对应等待后来超时、取消或失败，该 `request_id` 也只能结束生命周期，不能重新分配给新的 `OpenDataConnReq`
- 请求方在发送 `OpenDataConnReq` 后，即进入“等待匹配的 `TcpConnectionHello { role: Data, open_request_id: Some(request_id) }` 真正到达”的阶段
- `pending_open_request[request_id]` 的等待超时建议单独命名为 `open_request_arrival_timeout`，默认取 `connect_timeout`
- 若等待方超时或上层取消，则必须移除对应 `pending_open_request[request_id]`
- 若请求方收到带 `Some(request_id)` 的 `TcpConnectionHello(role = Data)`，且本地仍有对应 pending request，则可将这条新连接与该请求关联并唤醒等待者
- 若本地已没有对应 pending request（例如等待方已超时或上层已取消），仍可继续按普通 data connection 完成注册并纳入连接池；此时只是不再把它视为某个仍在等待中的反向建连结果
- 但实现必须限制这类“晚到连接”在单个 `Tunnel` 下的积压数量；若本地 idle pool / 未 claim 连接预算已满，则应直接关闭该连接并标记为 `Retired`
- 若这类晚到连接最终完成 `DataConnReady(Success)`，则它后续应与普通新建 data connection 完全一致：立即进入 `FirstClaimPending`，并仍由该 `conn_id` 的创建方拥有首轮 claim 权；协议不额外引入“冷却态”

### `FirstClaimPending`

对初次建立的 data connection，首轮 `lease_seq = 1` 使用以下非对称规则：

- 只有 `conn_id` 创建方可以主动发起首轮 `ClaimConnReq`
- 被动接收侧在首轮绑定完成前，不得主动 claim 这条连接
- 被动接收侧在本地注册并发送 `DataConnReady` 后，进入 `FirstClaimPending`
- 创建方收到 `DataConnReady(Success)` 后，也将该连接视为 `FirstClaimPending`
- `FirstClaimPending` 不是稳定驻留态；若连接在该状态下长期未等到合法的首轮 `ClaimConnReq`，则应按超时失败处理并直接 `Retired`
- 只有至少成功完成一轮绑定，并在后续 drain 完成后回到 `Idle`，这条连接才进入双方对称可复用态
- 双方都应在各自本地把连接迁移到 `FirstClaimPending` 的同一时刻启动 `first_claim_timeout`；该定时器是纯本地超时，不要求双边同步，只要求任一侧超时后本地都不得再复用这条连接

### 正常时序 1：本地主动新建 data connection

1. A 需要一条新的 data connection，分配新的 `conn_id`
2. 若 A 当前可直接连接到 B（包括已判断双方在同一局域网内），则 A 直接与 B 完成 TCP/TLS 建立
3. A 在该 data connection 上发送 `TcpConnectionHello { role: Data, tunnel_id, candidate_id, is_reverse: false, conn_id: Some(conn_id), open_request_id: None }`
4. B 收到 hello 后：
   - 校验 `tunnel_id`
   - 校验 `conn_id`
   - 将该连接注册到本地 `registered_conn_by_id[conn_id]`
5. B 在同一条 data connection 上回复 `DataConnReady { conn_id, Success }`
6. B 在发送 `DataConnReady(Success)` 后，将该连接视为 `FirstClaimPending`，但只能等待 A 的首轮 claim
7. A 收到 `DataConnReady(Success)` 后，才把该连接视为 `FirstClaimPending`，并由 A 拥有首轮主动 claim 权

### 正常时序 2：通过 control 请求对端反向新建 data connection

1. A 判断自己当前不能直接建立到 B 的 data connection，因此通过 control 发送 `OpenDataConnReq(request_id)`
2. B 收到该请求后，若本地允许处理这次反向建连，则作为 data connection 创建方分配新的 `conn_id`
3. B 主动新建一条指向 A 的 data connection
4. B 在该 data connection 上发送 `TcpConnectionHello { role: Data, tunnel_id, candidate_id, is_reverse: false, conn_id: Some(conn_id), open_request_id: Some(request_id) }`
5. A 收到 hello 后：
   - 用 `open_request_id` 关联本地 pending 的 `OpenDataConnReq(request_id)`（若该请求仍存在）
   - 完成本地注册
   - 在这条 data connection 上回复 `DataConnReady { conn_id, Success }`
6. A 在发送 `DataConnReady(Success)` 后，将该连接视为 `FirstClaimPending`，但只能等待 B 的首轮 claim
7. B 收到 `DataConnReady(Success)` 后，才把这条连接视为 `FirstClaimPending`，并由 B 拥有首轮主动 claim 权

### 失败处理

若在建连 / 注册阶段出现以下情况，当前 data connection 必须直接关闭，并且不得进入 claim 阶段：

- `Tunnel` 不存在
- `conn_id` 与本地已注册连接冲突
- `TcpConnectionHello(role = Data)` 字段非法
- `DataConnReady` 返回失败
- 等待 `DataConnReady` 超时

说明：

- `DataConnReady` 只在 data connection 自身上传递，不通过 control 通道回传
- 首轮 claim 权只属于 data connection 创建方；被动接收侧即使已经完成本地注册，也必须等待创建方发起首轮 claim
- `open_request_id` 只用于把“control 面上的反向建连请求”与“data 面上实际到达的新连接”关联起来，不参与后续 claim / reuse / drain 的一致性主键
- 同一局域网场景下，双方各自需要新建 data connection 时都直接建连，因此不会产生 `open_request_id`
- 若反向建连请求在对端无法执行，则不额外定义独立的 control 面失败回执；请求方通过 `open_request_arrival_timeout` 超时感知失败
- 这样可避免“接收端已经注册，但发送端还不知道是否可以 claim”的跨连接同步空窗

## 协议消息

除 data connection 在 `DataConnReady(Success)` 之后承载的裸业务字节流外，本文所有 TCP tunnel 协议消息都通过统一命令外壳编码。

- fresh control/data connection 的首命令都是 `TcpConnectionHello`
- control connection 建连确认使用 `ControlConnReady`
- data connection 注册确认使用 `DataConnReady`
- 下面列出的其余消息都通过 control 通道发送

### 绑定 channel 到 data connection

```rust
enum TcpChannelKind {
    Stream,
    Datagram,
}

struct ClaimConnReq {
    channel_id: u64,
    kind: TcpChannelKind,
    vport: u16,
    conn_id: u64,
    lease_seq: u64,
    claim_nonce: u64,
}

struct ClaimConnAck {
    channel_id: u64,
    conn_id: u64,
    lease_seq: u64,
    result: u8,
}
```

语义：

- `ClaimConnReq` 表示“我希望把这条已注册 data connection 的这一轮租约绑定到某个 channel 上”
- `kind` 与 `vport` 由本次 claim 一并声明，不再拆成独立的 `OpenChannelReq/OpenChannelAck`
- `ClaimConnAck.result = 0` 表示对端接受该绑定
- `ClaimConnAck.result != 0` 表示对端拒绝该绑定，非零值表示失败原因

`ClaimConnAck.result` 不应作为自由 `u8` 使用，而应定义为固定的线协议枚举；下面的取值集合为本文协议的规范组成部分：

```rust
enum ClaimConnAckResult {
    Success = 0,
    VportNotFound = 1,
    ListenerClosed = 2,
    NotIdle = 3,
    LeaseMismatch = 4,
    ConflictLost = 5,
    Retired = 6,
    ProtocolError = 7,
    AcceptQueueFull = 8,
}
```

说明：

- `VportNotFound`：目标 `vport` 当前不在对应的 `ListenVPortsRef` 中
- `ListenerClosed`：目标监听已关闭，无法接收该 `kind`
- `NotIdle`：当前连接不处于可接受新 claim 的窗口，例如已 `Bound`、`Draining`，或本地状态不允许复用
- `LeaseMismatch`：收到的 `lease_seq` 不是当前状态下唯一允许的候选值；对普通复用阶段通常应等于 `committed_lease_seq + 1`
- `ConflictLost`：发生并发 claim，且发送方在仲裁中失败
- `Retired`：连接已被本地判定为不可复用；一旦进入该状态，本地必须立即关闭对应 data connection
- `ProtocolError`：消息组合与当前状态矛盾，例如 claim 命中未知或未注册 `conn_id`
- `AcceptQueueFull`：被动接收侧对应 `kind` 的 accept 队列已满，当前无法再接收新的 inbound channel

### channel 层写半关闭

```rust
struct WriteFin {
    channel_id: u64,
    conn_id: u64,
    lease_seq: u64,
    final_tx_bytes: u64,
}
```

语义：

- 本端在当前 `(conn_id, lease_seq)` 这轮 channel 中不再写数据
- `final_tx_bytes` 表示本端在本轮 channel / 本轮 lease 里，已经成功写入 data connection 的总字节数
- `tx_bytes` 的统计口径固定为：当前 `DataConnEntry` 持有的底层 data connection writer 上，`write()` / `poll_write()` 已返回成功的字节数之和
- 任何停留在上层 `BufWriter`、用户态聚合缓冲、或其他尚未真正下推到底层 data connection writer 的字节，都不得提前计入 `tx_bytes`
- `(conn_id, lease_seq)` 是 `WriteFin` 的匹配主键；`channel_id` 只作为这一轮租约绑定关系的一致性断言，不参与跨轮复用匹配
- 若接收侧已知当前 `(conn_id, lease_seq)` 对应的 `channel_id`，而消息里的 `channel_id` 与本地记录不一致，则说明同一租约的控制面身份已分叉，当前连接必须 `Retired`
- 该命令不是“连接可以回收”的直接信号，只是“本端写侧完成”的事实声明

### drain 完成确认

```rust
struct ReadDone {
    channel_id: u64,
    conn_id: u64,
    lease_seq: u64,
    final_rx_bytes: u64,
}
```

语义：

- 本端已经完整读到了对端在本轮 channel 中承诺发送的全部字节
- `final_rx_bytes` 表示本端在本轮 channel / 本轮 lease 中已经实际读到的总字节数
- `rx_bytes` 的统计口径固定为：当前 `DataConnEntry` 持有的底层 data connection reader 上，已经成功读出并交付给本轮 channel 或内部 drain 观察者的字节数之和
- `final_rx_bytes` 应等于对端之前发来的 `final_tx_bytes`
- `(conn_id, lease_seq)` 是 `ReadDone` 的匹配主键；`channel_id` 仍只用于校验这一轮租约的 channel 身份一致性
- 若消息命中了本地当前 `(conn_id, lease_seq)`，但 `channel_id` 与本地记录不一致，则按协议矛盾处理，当前连接必须 `Retired`
- `ReadDone` 是连接回收的关键确认

## `ClaimConnReq` 的确定性处理顺序

为避免不同实现因为校验顺序不同而得到不同结果，接收侧在处理单个 `ClaimConnReq` 时应按以下顺序判定：

1. 先校验 `conn_id` 是否已知且已注册；未知或未注册直接按 `ProtocolError` 处理
2. 再校验连接是否已 `Retired`；若是则返回 `ClaimConnAck { result = Retired }`
3. 再校验 `lease_seq` 与连接状态是否允许这次 claim：
   - 若该 claim 在当前状态下已可确定为旧 lease、未来 lease、首轮非法发起方或 `NotIdle`，则在这一步直接返回 `ClaimConnAck { result = LeaseMismatch }`、`ClaimConnAck { result = ProtocolError }` 或 `ClaimConnAck { result = NotIdle }`
   - 只有当前 `(conn_id, lease_seq)` 在本地仍然是一个合法候选值时，才继续后续校验
4. 再校验目标接收条件：
   - 当前 TCP transport 层至少要先校验该 `kind` 的入站接收路径是否仍可用
   - 若对应 `kind` 的接收路径已关闭，返回 `ClaimConnAck { result = ListenerClosed }`
   - 若对应 `kind` 的 `ListenVPortsRef` 尚未注入完成，可仅在首次读取该 `kind` 的监听状态时短暂异步等待；`stream` 与 `datagram` 两种状态彼此独立，超时后应按本地实现约定失败
   - 若对应 `kind` 的 `ListenVPortsRef` 已注入，但 `is_listen(vport)` 为假，返回 `ClaimConnAck { result = VportNotFound }`
   - 若对应 `kind` 的入站待交付队列已满，返回 `ClaimConnAck { result = AcceptQueueFull }`
5. 只有当请求本身已经通过上述非竞争性校验，且本地也正处于同一 `(conn_id, lease_seq)` 的 `Claiming` 时，才允许进入 `claim_nonce` 冲突仲裁
6. 只有在本地已经决定接受该 claim，且已原子地完成“提交 `committed_lease_seq`、切换到 `Bound`、把 inbound channel 放入本地待接收队列或等价的待交付槽位”后，才允许返回 `ClaimConnAck`

补充约束：

- `ClaimConnAck(result = Success)` 不能早于本地 `Bound` 提交，也不能早于本地 inbound channel 已经具备可交付性
- 若接收侧在准备成功返回的最后阶段发现本地 accept 队列已关闭或已满，则本轮 claim 必须回退为失败，并返回 `ClaimConnAck { result = AcceptQueueFull }` 或 `ClaimConnAck { result = ListenerClosed }`；不得先返回成功再在本地丢弃 channel
- 若接收侧已经完成本地 `Bound` 提交，但 `ClaimConnAck` 未能经 control 通道成功发出（包括发送报错、control connection 同时断开、或发送结果已不可确认），则当前 `(conn_id, lease_seq)` 必须立即 `Retired`；不得尝试保留“仅本地成功”的半提交 channel

## `ClaimConnAck` 的状态处理

`ClaimConnAck` 只应由当前本地仍在等待的 `(conn_id, lease_seq, channel_id)` claim 消费。

`channel_id` 在单个 `Tunnel` 生命周期内应视为一次性 ID：

- 一次 opening attempt 一旦分配了 `channel_id`，无论后续是成功、失败、冲突失败、超时还是取消，都不得把该 `channel_id` 复用于另一轮新的 claim
- 若上层在 claim 失败后要重试其他连接，必须分配新的 `channel_id`
- 这样可保证迟到的 `ClaimConnAck` 总能按“旧 opening attempt”安全丢弃，而不会误命中新一轮打开流程

处理 `ClaimConnAck`：

- 若本地当前正处于匹配的 `Claiming(conn_id, lease_seq, channel_id)`：
  - 若 `result = Success`：
    - 提交 `committed_lease_seq = lease_seq`
    - 首轮 claim 成功时（创建方本地等待 `Ack` 的路径）：`Claiming -> Bound`
    - 后续复用 claim 成功时：`Claiming -> Bound`
    - 唤醒对应 opening channel，开始 data 面传输
  - 若 `result = ConflictLost`：
    - 这条 `Ack` 只用于结束本地 opening attempt；连接状态必须保持为冲突仲裁路径已经提交的结果
    - `committed_lease_seq` 不得因为这条失败结果回退
    - 对应 opening channel 返回失败，并按冲突失败语义重试其他连接或新建 data connection
  - 若 `result != Success` 且 `result != ConflictLost`：
    - 本轮 claim 失败，`committed_lease_seq` 保持不变
    - 首轮 claim 失败时：本地状态回到 `FirstClaimPending`
    - 后续复用 claim 失败时：本地状态回到 `Idle`
    - 对应 opening channel 返回失败，并按 `result` 决定是否重试其他连接或直接上报错误
- 若本地已因为冲突失败、超时、取消或本地错误而放弃这次 claim：
  - 该 `ClaimConnAck` 视为迟到消息，直接丢弃
  - 不得重新把连接切回 `Bound`
- 若 `conn_id`、`lease_seq`、`channel_id` 任一不匹配当前等待项：
  - 按旧消息或矛盾消息处理；通常直接丢弃，必要时 `Retired`

- 对 `ConflictLost` 的特殊规则：
  - 如果本地是冲突失败方，则这条 `ClaimConnAck { result = ConflictLost }` 只是完成本地 opening channel 的失败收尾
  - 连接本身可能已经因为接受了对端 claim 而进入 `Bound` 服务对端 channel，此时不得因为收到该失败结果再把连接回退到 `Idle` 或 `FirstClaimPending`

实现约束：

- 本地对外发送 `ClaimConnReq` 后，必须为该 `(conn_id, lease_seq, channel_id)` 建立唯一等待项
- 一旦等待项被成功消费、超时取消或因冲突失败而撤销，后续同 key 的 `ClaimConnAck` 都只能按迟到消息处理
- `ClaimConnAck.result` 只影响“本地发起的这次 claim 是否成功”，不能覆盖已经由其他路径提交的连接状态

## `ProtocolError` 的处置层级

`ProtocolError` 是一个上下文相关的协议错误类别，不表示所有错误都必须升级成同一强度的失败。实现应优先选择“最小但足够收敛”的失败域。

### claim 级拒绝

以下情况优先表现为“拒绝当前这次 claim”，而不是直接打断整个 tunnel：

- `ClaimConnReq` 命中未知或未注册 `conn_id`
- `FirstClaimPending` 阶段由不应拥有首轮主动权的一侧发起 claim
- 其他仍可明确归因于“当前 opening attempt 非法”，且本地 control connection 与 data connection 整体状态仍然自洽的请求错误

这类情况应优先返回 `ClaimConnAck { result = ProtocolError }`；除非实现随后又发现更强的不一致证据，否则不必仅因这一次 claim 级错误就关闭 control connection 或整个 tunnel。

### data connection 级致命错误

以下情况属于“当前 data connection 的协议身份或租约身份已经无法自洽”：

- `DataConnReady` 与同一物理连接上先前 `TcpConnectionHello(role = Data)` 的 `conn_id` 不一致
- 同一 `(conn_id, lease_seq)` 命中当前租约，但 `channel_id`、`final_tx_bytes` 或 `final_rx_bytes` 与本地已记录事实矛盾
- 其他已经证明“这条 data connection 的当前租约事实分叉”，且无法仅靠拒绝单条 claim 收敛的情况

这类情况必须把当前 data connection 立即标记为 `Retired` 并关闭物理连接；必要时清理该连接相关的所有 pending 状态，但不要求连带关闭健康的 control connection。

### control connection / tunnel 级致命错误

以下情况属于 control 面自身已经失去一致性：

- `ControlConnReady` 与同一物理连接上先前 `TcpConnectionHello(role = Control)` 的 `tunnel_id` 不一致
- control 建连阶段收到重复或矛盾的 `ControlConnReady`
- 已建立 tunnel 的 control connection 本身出现无法恢复的协议矛盾，导致后续任何 claim / drain 判定都失去可信基础

这类情况必须关闭当前 control connection；若 tunnel 尚处于 `Connecting`，则 candidate tunnel 建立失败；若 tunnel 已处于 `Connected`，则应进入 `Error`，并按“control connection 断开”章节收敛所有 data connection。

## channel 失败后的处理

这里不再设计额外的 `AbortChannel` / `RetireConn` 控制命令。

原因是：

- 一旦当前 channel 已经失败，这条 data connection 原则上就不应再参与复用
- 继续通过 control 通道补发“失败通知”并不能恢复一致性，反而会引入新的状态分支和迟到消息处理
- 对失败连接来说，最简单且最稳妥的策略就是直接关闭物理连接，并在本地把它标记为 `Retired`

因此本文采用下面的规则：

- channel 失败时，直接关闭对应 data connection
- 本地立即将该 `(conn_id, lease_seq)` 标记为 `Retired`
- 对端不依赖额外控制命令获知失败，而是通过 data connection 的读写错误、EOF、claim 超时或后续状态超时感知异常
- 一旦任一侧确认当前连接失败，本地都不得再把该连接放回 idle pool

`Retired` 仍然是本地连接状态的一部分，但不是一个需要上线路传输的协议命令；它的实际处理动作就是“立刻关闭连接并停止复用”。

## 正常时序

本节只描述成功路径示例；若其与前文的确定性处理顺序、状态机或错误处理规则发生阅读上的简化差异，均以前文规范性规则为准。

### 1. 复用一条已注册的 idle connection

1. A 选择一条已经 registered 且处于 `Idle` 的 `conn_id`
2. A 读取本地已提交的 `committed_lease_seq`，计算 `next_lease_seq = committed_lease_seq + 1`
3. A 将该连接状态从 `Idle -> Claiming(next_lease_seq, claim_nonce, channel_id)`
4. A 发送 `ClaimConnReq(channel_id, kind, vport, conn_id, next_lease_seq, claim_nonce)`
5. B 收到请求后：
   - 校验 `conn_id` 已注册
   - 校验 `kind` / `vport` 可接受
   - 若该连接在本地也处于 `Idle` 且没有本地更强 claim，则接受
   - 将本地对应连接切换到 `Bound(channel_id, next_lease_seq)`，并提交 `committed_lease_seq = next_lease_seq`
   - 返回 `ClaimConnAck`
6. B 只有在 claim 成功后，才允许把该 channel 投递给 `accept_stream()` / `accept_datagram()`
7. A 收到 `ClaimConnAck` 后，也将本地连接切换到 `Bound(channel_id, next_lease_seq)`，并提交 `committed_lease_seq = next_lease_seq`
8. 双方开始通过这条 data connection 传输本轮 channel 的裸数据

### 2. 新建 data connection 后再绑定 channel

1. A 没有可用 idle connection，于是按“数据连接建立协议”新建一条 data connection
2. 该连接在双方都完成注册，并且 A 收到 `DataConnReady(Success)`
3. 该连接此时在双方本地都处于 `FirstClaimPending`，且 `committed_lease_seq = 0`
4. 由于 A 是该 `conn_id` 的创建方，因此只有 A 可以发起首轮 `ClaimConnReq(channel_id, kind, vport, conn_id, lease_seq = 1, claim_nonce)`
5. B 校验通过后完成绑定并返回 `ClaimConnAck`
6. A 收到 `ClaimConnAck` 后，提交 `lease_seq = 1` 并进入 `Bound`

该流程与复用连接的差别只在于连接来源不同，绑定协议保持一致。

## 双边同时拥有复用权时的 claim 冲突

### 冲突场景

对于同一条已经至少完成过一轮绑定并回到 `Idle` 的 `conn_id`：

- A 认为它可用于自己的新 channel
- B 也认为它可用于自己的新 channel
- 双方几乎同时对同一连接发出 `ClaimConnReq`

如果不做仲裁，会出现：

- 两端都认为自己已经占有该连接
- 但双方期望绑定到不同的 `channel_id`
- data connection 的语义立即分叉

### 决策规则

当本地对 `(conn_id, lease_seq)` 已处于 `Claiming`，且又收到了对端对同一 `(conn_id, lease_seq)` 的 `ClaimConnReq` 时：

- 只有当这条对端 `ClaimConnReq` 已经通过前文规定的全部非竞争性校验（`conn_id` 已知且已注册、连接未 `Retired`、`lease_seq` 可受理、`vport/listener/accept queue` 可接受）后，才允许进入本节的 `claim_nonce` 冲突仲裁
- 若该请求在本地先被判定为 `VportNotFound`、`ListenerClosed`、`AcceptQueueFull`、`LeaseMismatch`、`NotIdle`、`ProtocolError` 或 `Retired`，则必须优先返回对应的 `ClaimConnAck { result = ... }`；这类请求不进入 `claim_nonce` 比较，也不得因为“本地在竞争中失败”而被强行接受

1. 先确认比较范围只限于同一个 `(conn_id, lease_seq)`
   - 如果 `conn_id` 不同，则不是同一条物理连接，不构成冲突
   - 如果 `lease_seq` 不同，则说明至少一方基于旧状态或未来状态操作，不进入 nonce 比较，而是按 stale / invalid claim 处理
2. 若 `(conn_id, lease_seq)` 相同，则进入 claim 冲突仲裁
   - 比较 `claim_nonce_local` 与 `claim_nonce_remote`
   - `claim_nonce` 较大的一方胜出
3. 如果 `claim_nonce` 恰好相同，则使用固定节点序兜底
   - 直接比较双方 `P2pId.as_slice()` 的字节字典序
   - 字节序较大的一方胜出
   - 规则必须全局固定，所有实现都不得改成其他排序方式
   - 这里的比较对象固定为 `P2pId.as_slice()` 返回的原始字节序列本身；不得先转十六进制字符串、不得做大小端转换、也不得做额外归一化
4. 胜出方继续保留这次 claim
   - 本地状态保持 `Claiming`
   - 对自己此前收到的对端 `ClaimConnReq` 返回 `ClaimConnAck { result = ConflictLost }`
   - 等待对端对本地这次 claim 返回 `ClaimConnAck`
5. 失败方必须立即放弃本地这次 claim
   - 不再把当前 `(conn_id, lease_seq)` 绑定到本地正在打开的 `channel_id`
   - 在“对端这次 `ClaimConnReq` 已通过本地全部非竞争性校验”的前提下，必须接受对端这次 claim，把当前连接绑定到对端 `channel_id`
   - 在上述前提成立时，对端发来的这次 `ClaimConnReq` 应返回 `ClaimConnAck`
   - 本地 opening channel 等待自己此前发出的 `ClaimConnReq` 收到 `ClaimConnAck { result = ConflictLost }` 后，再返回失败并重新选择其他连接，或新建 data connection
   - 若等待这条失败结果超时，则当前 opening channel 按超时失败；连接本身继续服务已接受的对端 channel，不因为本地这个 opening channel 失败而自动 `Retired`

为保证该决策逻辑在双边完全对称时也能收敛，建议附加以下实现约束：

- 每次发起 claim 时都重新生成新的 `claim_nonce`，不能复用上一次 claim 的 nonce
- 同一个 `(conn_id, lease_seq)` 在本地最多只允许存在一个进行中的 claim
- 一旦本地判定自己输了冲突，就不能继续等待这条连接的 `Ack`
- 如果本地已经根据胜负结果完成状态回退，但稍后收到了迟到的旧 `Ack`，仍要按 `(conn_id, lease_seq)` 与本地状态二次校验，必要时直接丢弃

## channel 层半关闭与回收

### 本地写侧完成

当本地上层不再写当前 channel 时：

1. 记录当前 `tx_bytes`
2. 发送 `WriteFin(final_tx_bytes = tx_bytes)`
3. 将本地 channel 状态置为 `LocalWriteClosed`

这里的关键点是：

- `WriteFin` 只声明“我不会再写更多字节”
- 不能直接推导“连接已可复用”
- 因为 control 命令可能早于最后一段 data 到达

### 收到对端 WriteFin

收到 `WriteFin(peer_final_tx_bytes)` 后：

1. 记录对端本轮承诺发送的总字节数
2. 本地继续读 data connection
3. 只有当 `rx_bytes == peer_final_tx_bytes` 时，才能认为对端写半关闭真正完成

若出现 `rx_bytes > peer_final_tx_bytes`：

- 这是协议错误
- 当前连接必须 `Retired`

### 发送 ReadDone

当本地确认 `rx_bytes == peer_final_tx_bytes` 时：

1. 发送 `ReadDone(final_rx_bytes = rx_bytes)`
2. 将本地状态置为 `PeerWriteDrained`

### `Bound -> Draining` 的进入条件

`Draining` 表示：当前 `(conn_id, lease_seq)` 已经不再是“纯业务传输阶段”，而是进入“等待双向 `WriteFin/ReadDone` 事实补齐并决定是否可回收”的阶段。

进入 `Draining` 后，当前 lease 仍然可能继续承载已经在途的剩余业务字节，但协议语义上它已经进入回收判定窗口：实现不得再把它视为“尚未开始收尾的纯 `Bound` 传输态”，也不得再为这条连接发起新的 claim。

本地应在首次满足以下任一条件时，将连接从 `Bound` 迁移到 `Draining`：

1. 本地已发送 `WriteFin`
2. 本地已收到对端 `WriteFin`
3. 本地已经为当前 `(conn_id, lease_seq)` 建立了 `pending_fin_by_conn_lease` 或 `pending_read_done_by_conn_lease`

补充约束：

- `Bound -> Draining` 只表示“本轮开始进入回收判定窗口”，不表示已经满足回收条件
- 一旦进入 `Draining`，实现就必须开始按连接级事实跟踪 `local_write_fin_sent`、`peer_write_fin_received`、`local_read_done_sent`、`peer_read_done_received`、`peer_final_tx_bytes` 与 pending 表
- 若连接是因为早到的 `WriteFin` / `ReadDone` 事实被推进到 `Draining`，而本地数据面仍有尚未读完的在途字节，这属于预期内行为；实现应继续完成本轮数据消费与事实补齐，而不是把这种“先收尾信号、后到尾数据”的情况视为额外异常
- 若连接在 `Bound` 阶段就发生错误、协议矛盾或超时，也可以不经过 `Draining` 直接 `Retired`

### 连接何时可回 idle pool

一条 `(conn_id, lease_seq)` 只有满足以下条件时，才允许从 `Draining -> Idle`：

1. 本地已发送 `WriteFin`
2. 已收到对端 `WriteFin`
3. 本地 `rx_bytes == peer_final_tx_bytes`
4. 本地已发送 `ReadDone`
5. 已收到对端 `ReadDone`
6. 本地 read/write 句柄都已释放
7. 当前连接未被标记为 `Retired`

这里的“本地 read/write 句柄都已释放”只指对上层暴露的 channel 端点；用于 datagram 隐藏方向校验的内部 drain observer、计数器与 pending 状态不计入该条件，可以在上层句柄释放后继续存在，直到本轮完成、连接关闭或连接进入 `Retired`。

换句话说，连接能否复用不再由“双方句柄都 drop 了”决定，而由“双向写侧完成 + 双向 drain 完成 + 本地句柄释放”共同决定。

## 提前 drop 的处理

### 本地 read 已 drop 后收到对端 WriteFin

如果本地 read 已经释放，而此后才收到对端 `WriteFin(peer_final_tx_bytes)`，需要分两种情况处理。

#### 情况 1：`rx_bytes == peer_final_tx_bytes`

这说明虽然本地 read 已经 drop，但在 drop 之前实际上已经把对端本轮数据完整读完了。

此时可以认为：

- 对端写侧已经完成
- 本地读侧已经完成 drain
- 当前 data connection 仍然是有效的

后续只要其余复用条件也满足，例如：

- 本地已经发送 `WriteFin`
- 已收到对端 `ReadDone`
- 本地 write 也已经释放

则该连接仍然可以进入 idle pool。

#### 情况 2：`rx_bytes != peer_final_tx_bytes`

这说明本地 read drop 时，本轮 drain 结果与对端声明的最终发送字节数不一致。

此时已经没有办法再证明这一轮数据边界是完整的，因此必须：

- 放弃复用当前连接
- 直接关闭当前 data connection
- 本地直接将该连接标记为 `Retired`

### 本地 write 已 drop，但还未发送 WriteFin

本地 write 释放前应尽量完成 flush，并在控制面显式发送 `WriteFin`。如果因为异常未能发送：

- 当前 channel 视为失败
- 该连接默认 `Retired`

## 乱序与旧消息处理

### 跨连接先后不一致

control 与 data 是两条独立连接，因此仍然可能出现以下先后关系：

- `WriteFin` 先到，但最后的数据还在路上
- `ReadDone` 先到，但本地还未完成对应的 drain 判断
- 旧 `lease_seq` 的命令在新一轮租约之后迟到

接收端必须维护最小必要挂起表：

- `registered_conn_by_id[conn_id]`
- `pending_fin_by_conn_lease[(conn_id, lease_seq)]`
- `pending_read_done_by_conn_lease[(conn_id, lease_seq)]`

处理原则分三类：

1. 预期内的跨连接先后不一致
   - 例如 `WriteFin` 先到但最后数据未到，或 `ReadDone` 先到但本地尚未完成对应状态迁移
   - 这类情况不立即判错，而是进入对应 pending 表等待后续状态补齐
   - 如果在超时时间内仍未补齐，再将当前连接标记为 `Retired`
2. 明确的旧消息
   - 如果消息中的 `lease_seq < committed_lease_seq`，直接按 stale 消息丢弃
   - 这类消息不需要把连接 `Retired`
3. 明确矛盾的消息
   - 如果消息与当前已提交状态不可能同时成立，例如连接已 `Retired`、已绑定到另一个 lease，或 claim 命中未知 / 未注册 `conn_id`
   - 这类情况视为状态不一致，按 `ProtocolError` 或本地 `Retired` 处理

### 旧消息

所有与 channel 结束或连接回收有关的控制消息，都必须以 `(conn_id, lease_seq)` 作为匹配主键。

接收端处理规则：

- 若 `conn_id` 未知或未注册，则这不是正常乱序，按 `ProtocolError` 处理
- 若 `lease_seq` 小于当前租约，直接丢弃为 stale 消息
- 若 `lease_seq` 大于当前已提交租约，需要结合当前状态判断：
  - 若本地正处于本轮合法的 `Claiming` / `Bound` / `Draining` 迁移窗口，则按当前状态校验
  - 对 `WriteFin` / `ReadDone` 而言，若该 `lease_seq` 恰好命中本地当前正在迁移的同一租约，则允许把它作为当前租约的早到控制事实记录到 pending 表，而不是直接视为 future lease
  - 若本地状态为 `FirstClaimPending`、`Idle` 或 `Retired`，却收到 `lease_seq > committed_lease_seq` 的 `WriteFin` / `ReadDone`，则该消息不可能命中本地已知的当前租约或迁移中租约；对已知连接必须直接 `Retired`
  - 若本地已存在同一连接的更明确提交状态且二者矛盾，则直接 `Retired`
- 对 `WriteFin` / `ReadDone` 而言，`channel_id` 不是跨轮匹配主键；它只用于校验“本地已记录的当前租约 channel 身份”是否与消息一致
- 若 `(conn_id, lease_seq)` 命中当前租约，但 `channel_id` 与本地记录不一致，则这是同一租约内的身份冲突，当前连接必须 `Retired`

补充约束：

- 对 `ClaimConnReq` 不建议静默丢弃；只要本地仍能确定该连接状态，优先返回 `ClaimConnAck`
- 只有对已经完成生命周期、且 `lease_seq < committed_lease_seq` 的明显旧 claim，才允许作为 stale 直接丢弃

### 重复消息与幂等

为了避免 control 通道重试、对端重复发送、或本地并发处理导致实现分叉，以下规则固定为协议语义：

- 重复 `ClaimConnReq`：
  - 若请求与本地当前正在处理或刚完成处理的同一请求完全一致，即 `(conn_id, lease_seq, channel_id, claim_nonce, kind, vport)` 全部一致，则按幂等请求处理
  - 若本地对该请求此前已经返回过 `ClaimConnAck`，且 control connection 仍健康并且本地仍可安全发送，则必须重复返回同一 `ClaimConnAck`；只有在实现已无法安全重发（例如 control connection 已关闭、发送结果已不可确认、或对应处理记录只剩终态摘要）时，才允许静默丢弃；但不得改为返回其他结果
  - 若本地对该请求此前已经返回过 `ClaimConnAck { result = X }`，且 control connection 仍健康并且本地仍可安全发送，则必须重复返回同一 `ClaimConnAck { result = X }`；只有在实现已无法安全重发时，才允许静默丢弃；但不得改为返回其他 `result`
  - 若 `(conn_id, lease_seq)` 命中当前或最近一次处理记录，但 `channel_id`、`claim_nonce`、`kind` 或 `vport` 中任一字段与已记录事实不一致，则这不是幂等重发，而是矛盾消息；应按当前状态返回适当的 `ClaimConnAck { result = ... }`，必要时直接 `Retired`
- 重复 `WriteFin`：若 `(conn_id, lease_seq, channel_id, final_tx_bytes)` 与本地已记录事实完全一致，则按幂等消息忽略；若 key 命中但 `final_tx_bytes` 不一致，则当前连接必须 `Retired`
- 重复 `ReadDone`：若 `(conn_id, lease_seq, channel_id, final_rx_bytes)` 与本地已记录事实完全一致，则按幂等消息忽略；若 key 命中但 `final_rx_bytes` 不一致，则当前连接必须 `Retired`
- 重复 `ClaimConnAck`：只允许由当前仍存在的唯一等待项消费；若等待项已结束，则按 late message 丢弃；不得再次推进、回退或覆盖已经提交的连接状态
- 同一个 `(conn_id, lease_seq)` 在任一时刻都只允许存在唯一的当前租约上下文与唯一的本地等待项；实现不得通过“多消费者竞争同一控制消息”来决定结果

## 对 stream 与 datagram 的影响

### `stream`

- 双向都有写入能力
- 因此双方都可能发送 `WriteFin`
- 回收连接时必须等待双向 `ReadDone`

### `datagram`

当前接口模型是：

- 主动方 `open_datagram()` 得到写端
- 被动方 `accept_datagram()` 得到读端

因此在单轮 datagram lease 中，方向约束必须固定为：

- `open_datagram()` 一侧是唯一允许向对端发送非零业务字节的一侧；它的隐藏读侧必须保持“本轮实际收到业务字节数为 `0`”
- `accept_datagram()` 一侧是唯一允许从对端接收非零业务字节的一侧；它的隐藏写侧必须保持“本轮实际发送业务字节数为 `0`”
- 任一隐藏方向只要观察到非零业务字节，或收到与“该隐藏方向应为零字节”矛盾的 `WriteFin.final_tx_bytes`，当前连接都必须直接 `Retired`

但 data connection 本身不区分 `stream` / `datagram`，同一条 idle data connection 可以被下一轮任意类型的 channel 复用。

因此，`datagram` 在 TCP 下只是单向 API 视图，不是单向物理传输；它是否允许复用，仍然取决于底层是否像 `stream` 一样完成了双向的 `WriteFin/ReadDone` 校验。

因此在复用协议上，`datagram` 与 `stream` 保持同样的回收条件：

- 双方都需要完成本端 `WriteFin`
- 双方都需要收到对端 `WriteFin`
- 双方都需要在本地 drain 完对端承诺发送的字节后发送 `ReadDone`
- 连接只有在双向 `WriteFin + ReadDone` 全部完成后，才可回 idle pool
- 当前接口仍然可以保持单向收发视图；缺失的一侧只是不暴露给上层，不影响底层连接复用协议按双向 drain 执行

实现约束（规范要求）：

- 对于上层未暴露写端的一侧，底层必须把本轮 `tx_bytes` 固定维护为 `0`，并在该 datagram channel 进入 `Bound` 后主动发送一次 `WriteFin(final_tx_bytes = 0)`；不能等待不存在的上层 writer drop 来触发
- 对于上层未暴露读端的一侧，底层必须仍然保留一个内部 drain 观察者，持续跟踪本轮是否收到来自对端的字节、是否收到对端 `WriteFin`，以及最终 `peer_final_tx_bytes`
- 这个内部 drain 观察者必须在该 datagram channel 进入 `Bound` 时开始工作，并且只有在本轮已成功发送 `ReadDone`、连接进入 `Retired`、物理连接关闭、或 control connection 关闭后该连接已被明确标记为“本轮后不可复用”时，才允许停止
- 对隐藏读侧来说，“没有对上层暴露 reader”不等于“不再读取 data connection”；底层仍必须继续消费并统计属于当前 `(conn_id, lease_seq)` 的字节，以便判断是否满足 `peer_final_tx_bytes` 与零字节约束
- 当上层未暴露读端的一侧满足“本轮实际收到字节数始终为 `0`，且对端 `WriteFin.final_tx_bytes = 0`”时，底层才允许发送 `ReadDone(final_rx_bytes = 0)`
- 若上层未暴露读端的一侧在本轮观察到任意一个来自对端的业务字节，或收到的 `WriteFin.final_tx_bytes != 0`，则说明对端向一个本轮不应承载业务数据的隐藏方向发送了数据；当前连接必须直接 `Retired`
- 对隐藏写侧来说，自动发送的 `WriteFin(0)` 在每轮 datagram lease 中最多只能发送一次；发送后仍需继续等待对端 `WriteFin/ReadDone` 与本地隐藏读侧的 drain 结果，不能因为“本端无 writer”而提前判定连接可复用
- 换句话说，`WriteFin(0)` / `ReadDone(0)` 不是基于接口假设直接写死，而是要由底层显式维护零字节事实并完成校验后才能发送
- 只要任一实现无法持续维护上述隐藏方向事实，这条 datagram data connection 就不得进入复用路径，而应在本轮结束后直接关闭

## 状态机

### DataConnEntry 状态

这里的状态机描述的是每个 `conn_id` 对应的唯一 `DataConnEntry`。

- 当状态处于 `Connecting/Registering/FirstClaimPending/Idle/Retired` 时，表示该连接当前没有一个已绑定的活动租约
- 当状态处于 `Claiming/Bound/Draining` 时，`DataConnEntry` 内必须同时持有唯一的当前租约上下文 `(lease_seq, channel_id, kind, byte counters, fin/read_done facts, pending timers ...)`
- 因此 `Claiming/Bound/Draining` 不是脱离连接独立存在的“第二条状态机”，而是“带当前租约上下文的连接状态”

```text
Connecting
  -> Registering
  -> Retired

Registering
  -> FirstClaimPending
  -> Retired

FirstClaimPending
  -> Claiming    (仅创建方可发起首轮 claim)
  -> Bound       (被动接收侧接受创建方首轮 claim)
  -> Retired

Idle
  -> Claiming(lease_seq, claim_nonce, channel_id)
  -> Bound(lease_seq, channel_id)   (接收对端 claim 并成功接受)
  -> Retired

Claiming
  -> Bound(lease_seq, channel_id)
  -> FirstClaimPending  (首轮 claim 失败且未提交 lease)
  -> Idle               (后续 claim 失败且未提交 lease)
  -> Retired

Bound
  -> Draining
  -> Retired

Draining
  -> Idle        (双向 WriteFin + ReadDone 完成)
  -> Retired
```

进入 `Retired` 后没有后续迁移；实现应立刻关闭底层 data connection，并清理与该连接相关的本地状态。

### channel 状态

这里描述的是“本地一次 channel 打开 / 服务过程”的状态，而不是 `DataConnEntry` 的唯一权威状态。

- `DataConnEntry` 是否进入 `Bound` / `Draining`，仍以连接级状态机为准
- 本地 opening attempt 可以因为冲突失败、超时或取消而进入 `Aborted`，同时同一条 data connection 可能已经提交给对端 channel 使用；此时不得让 opening attempt 的失败回滚连接级已提交状态
- 实现上应把“连接状态提交”和“本地 opening attempt 收尾”视为两个并行但不同 owner 的状态对象

```text
Opening
  -> Claiming
  -> Aborted

Claiming
  -> Bound
  -> Aborted

Bound
  -> LocalWriteClosed
  -> PeerWriteClosedKnown
  -> Draining
  -> Aborted

Draining
  -> Completed
  -> Aborted
```

这里的 `LocalWriteClosed` / `PeerWriteClosedKnown` 更适合作为 channel 在 `Bound/Draining` 期间持有的局部事实标志，而不是必须单独拆成互斥的大状态枚举；实现只要保证这些事实能正确驱动 `WriteFin` / `ReadDone` 的发送与回收判定即可。

## 错误处理与超时

### 推荐超时基线

为避免同一实现里各处各自选择不同超时，建议把下面几类超时收敛到少量基础配置上：

- `connect_timeout`：control connection 与 data connection 的 TCP/TLS 建连超时
- `heartbeat_interval`：control connection 心跳周期
- `heartbeat_timeout`：control connection 失活判定超时

推荐默认值：

- `ControlConnReady` 等待超时：`connect_timeout`
- `DataConnReady` 等待超时：`connect_timeout`
- `FirstClaimPending` 等待首轮 claim 的超时：`heartbeat_timeout`
- `ClaimConnReq -> ClaimConnAck` 等待超时：`heartbeat_timeout`
- `ConflictLost` 场景下等待对端补发 `ClaimConnAck { result = ConflictLost }` 的超时：`heartbeat_timeout`
- 收到对端 `WriteFin` 后等待本地 drain 完成的“无进展超时”：`max(2 * heartbeat_timeout, 2s)`
- `pending_fin_by_conn_lease` / `pending_read_done_by_conn_lease` 的“无进展超时”：`max(2 * heartbeat_timeout, 2s)`
- `pending_open_request[request_id]` 的 `open_request_arrival_timeout`：`connect_timeout`

如果后续实现维护了 RTT 估计值，也可以把上述控制面超时进一步收敛为：

- `max(4 * srtt, heartbeat_timeout_floor)`

但在没有稳定 RTT 估计前，优先复用现有 `connect_timeout` / `heartbeat_timeout` 配置，避免引入新的独立调参项。

### 定时器归属与取消规则

- `ControlConnReady` 超时由 control connection 主动建立方启动；收到 `ControlConnReady` 或连接关闭后立即取消
- `DataConnReady` 超时由 data connection 创建方启动；收到 `DataConnReady` 或连接关闭后立即取消
- `pending_open_request[request_id]` 的 `open_request_arrival_timeout` 由发送 `OpenDataConnReq` 的请求方启动；收到匹配的 `TcpConnectionHello(role = Data)`、本地取消、或 tunnel 关闭后立即取消
- `first_claim_timeout` 由双方在各自本地把连接迁移到 `FirstClaimPending` 时独立启动；首轮合法 claim 成功、连接进入 `Retired`、或物理连接关闭后立即取消；该定时器不要求双边同步触发
- `ClaimConnReq` 超时由 claim 发起方启动；收到匹配的 `ClaimConnAck`、本地取消、或连接进入 `Retired` 后立即取消
- `ConflictLost` 失败方等待迟到 `ClaimConnAck { result = ConflictLost }` 的超时，仍归本地 opening channel 所有；该超时只结束 opening channel，不得回滚已被对端占用的连接状态
- drain 超时在“收到对端 `WriteFin` 但尚未满足 `rx_bytes == peer_final_tx_bytes`”时启动；每当 `rx_bytes` 继续前进时都应刷新该定时器；一旦 drain 完成、连接关闭、或连接进入 `Retired`，立即取消
- pending 表超时由连接级状态机统一维护；每当对应 pending 项收到能推进状态的新事实时都应刷新定时器；一旦对应 `(conn_id, lease_seq)` 进入下一稳定状态，必须同步移除 pending 项并取消定时器

### ControlConnReady 超时

如果 control connection 在超时时间内未收到对端返回的 `ControlConnReady`：

- 当前 tunnel 建立失败
- 直接关闭该 control connection
- 当前 `tunnel_id` 不得视为已连接 tunnel

### DataConnReady 超时

如果 data connection 在超时时间内未收到对端返回的 `DataConnReady`：

- 当前 data connection 建立失败
- 直接关闭该连接
- 该连接不得进入 `Idle`，也不得参与 claim

### 反向 data connection 到达超时

如果请求方发送 `OpenDataConnReq(request_id)` 后，在 `open_request_arrival_timeout` 内仍未等到匹配的 `TcpConnectionHello { role: Data, open_request_id: Some(request_id) }`：

- 当前这次“请求对端反向建连”的等待失败
- 必须移除对应 `pending_open_request[request_id]`
- 这次失败不产生任何已注册 data connection，也不提交任何 `lease_seq`
- 若对应 data connection 之后迟到到达，仍按前文“晚到连接”规则处理：可继续注册为普通新连接，或在预算不足时直接 `Retired`

### `FirstClaimPending` 超时

如果一条已注册 data connection 进入 `FirstClaimPending` 后，在 `first_claim_timeout` 内仍未等到创建方发起的合法首轮 `ClaimConnReq`：

- 说明这条“已建立但尚未首轮绑定”的连接已经失去继续保留的价值
- 本地应直接关闭对应 data connection，并将其标记为 `Retired`
- 不得把这条连接放入 `Idle`，也不得把它提升为双方对称可复用态
- 该超时不提交任何新的 `lease_seq`

### Claim 超时

如果 `ClaimConnReq` 在超时时间内未收到 `ClaimConnAck`：

- 当前 channel 打开失败
- 默认将当前连接直接标记为 `Retired`
- 若只是 claim 超时且本地尚未提交新租约，则 `committed_lease_seq` 保持不变
- 上层可以重试并重新选择连接

唯一例外只发生在并发 claim 冲突中的失败方：

- 失败方在接受对端 claim 并返回 `ClaimConnAck(result = Success)` 后，仍需等待自己此前发出的 `ClaimConnReq` 收到 `ClaimConnAck { result = ConflictLost }`
- 若该失败结果在超时时间内未到，则当前 opening channel 按超时失败；连接本身继续服务已接受的对端 channel，不因为这个超时自动 `Retired`
- 若该 `ClaimConnAck { result = ConflictLost }` 之后迟到到达，也只用于结束本地这次 opening channel 的等待；不能把已经服务对端 channel 的连接重新回退或标记为 `Retired`

设计理由：

- 在这个例外场景里，本地虽然“自己的 opening attempt 失败”，但连接本身已经接受了对端 claim，并且当前租约已被提交为对端 channel 服务
- 此时若仅因为本地没有及时收到那条补发的 `ClaimConnAck { result = ConflictLost }` 就把连接 `Retired`，会无端中止一个已经成功建立的对端 channel
- 因此这里超时只终止“本地失败 opening attempt 的收尾等待”，不回滚已经提交给对端 channel 的连接状态

### WriteFin 后长时间未 drain

如果一侧已经收到对端 `WriteFin`，但长时间没有达到 `rx_bytes == peer_final_tx_bytes`：

- 说明 data 面可能异常或上层已提前放弃读取
- 这里的“长时间”指 `rx_bytes` 在一个 drain 无进展超时窗口内没有继续前进，而不是本轮 drain 的绝对总时长
- 连接应 `Retired`

### pending 表超时

如果 `pending_fin_by_conn_lease[(conn_id, lease_seq)]` 或 `pending_read_done_by_conn_lease[(conn_id, lease_seq)]` 长时间未被后续状态补齐：

- 说明 control 面与 data 面的跨连接时序已经超出可恢复窗口
- 这里的“长时间”同样按无进展超时计算；只要 pending 项持续得到能推进本轮状态的新事实，就应持续续期
- 当前 `(conn_id, lease_seq)` 应直接 `Retired`
- 清理对应 pending 项，避免旧消息在更晚时间继续污染新状态

### control connection 断开

control connection 是复用协议的一致性基础。一旦 control connection 断开：

- tunnel 状态立即进入 `Closed` 或 `Error`
- 所有 `Idle` data connection 立即 `Retired`
- 所有 `Claiming` data connection 立即结束当前 opening attempt，并将连接标记为 `Retired`
- 所有 `Bound/Draining` data connection 立即标记为“本轮之后不可复用”
- 对已经交付给上层的 `Bound/Draining` channel，允许继续沿现有 data connection 做 best-effort 数据收发，直到本轮 channel 自然结束、底层 I/O 出错、或本地上层释放句柄
- 但这类连接后续不得再尝试回到 `Idle`，也不得继续等待补齐 `WriteFin/ReadDone` 来恢复复用；一旦当前 channel 结束，应直接关闭物理 data connection
- 所有依赖 control 通道继续推进的 pending claim / pending drain / pending open-request 定时器，都应立即按 tunnel 关闭语义结束，而不是等待 control 恢复

## 与当前实现的关系

### 当前实现中的关键差异

`p2p-frame/src/networks/tcp/` 当前实现已经覆盖了本文大部分核心协议元素，包括 `ControlConnReady`、`DataConnReady`、`ClaimConnReq/Ack(result)`、`WriteFin`、`ReadDone`、`conn_id`、`lease_seq` 与 `claim_nonce`。

当前仍需特别注意的实现约束主要是：

- `TcpTunnelNetwork` / `TcpTunnelListener` 不负责 tunnel 级唯一性与复用；每次 `create_tunnel()` 都创建新的 tunnel，由更上层决定是否去重
- `tunnel_id` 当前实现为 `u32`，并使用最高位区分创建侧，而不是 `u64`
- 本文中凡是涉及“同远端对只保留一个健康 tunnel”的内容，都应视为更上层编排策略，而不是 TCP transport 内部强制行为

### 当前实现已完成的高层收敛

- `TcpDataBind` 已被新的 tunnel/data 注册与 claim 协议替代
- `EnterIdle` 语义已被 `WriteFin/ReadDone + Draining -> Idle` 替代
- `active_channels` 已从“按 channel_id 管理连接”收敛为“按 `(conn_id, lease_seq)` 管理租约上下文”
- idle pool 已基于统一 `DataConnEntry` 模型，而不是按 `stream/datagram` 分离的浅层连接列表

## 实现映射说明

本节记录 `p2p-frame/src/networks/tcp/` 与本文协议的主要对应关系，不再重复定义前文已经给出的协议语义。

### `protocol.rs`

- `TcpConnectionHello` 定义为：
  - `role: Control | Data`
  - `tunnel_id`
  - `conn_id: Option<u64>`
  - `open_request_id: Option<u64>`
- `TcpConnectionHello`、`ControlConnReady`、`DataConnReady` 与各个 control 命令体都实现统一命令外壳编解码
- control connection 使用 `ControlConnReady`
- data connection 使用 `DataConnReady`
- Rust 实现内部仍保留 `TcpControlCmd` 作为本地分派载体，但它不再是线协议上的直接编码格式；线上仍按具体命令体各自的 `command_id` 编码
- control 命令体包括：
  - `OpenDataConnReq`
  - `ClaimConnReq`
  - `ClaimConnAck`
  - `WriteFin`
  - `ReadDone`
  - `Ping`
  - `Pong`
- 使用 `TcpChannelKind`
- 不再依赖 `TcpDataBind`

### `network.rs`

- control connection 建立逻辑对应 tunnel 建立协议的 active 侧实现
- active 侧建立 control connection 后等待 `ControlConnReady`
- 维护 `tunnel_id` 分配器
- 不在 `TcpTunnelNetwork` 内部实现 tunnel 去重或复用；每次调用都创建新的 tunnel

### `listener.rs`

- data/control 接入后，先按 `TcpConnectionHello.role` 做分流
- control 接入后，先完成 tunnel 注册，再返回 `ControlConnReady`
- data connection 接入后，先按 `conn_id` 完成本地注册，再在该 data connection 上回复 `DataConnReady`
- 不再依赖 `TcpDataBind` 直接决定 channel 归属
- 只有收到后续 control 通道上的 `ClaimConnReq` 并绑定成功后，才把连接投递给 `accept_stream()` / `accept_datagram()`
- inbound channel 的待接收队列采用有界队列或等价的有界待交付槽位；若本地无法再接收新的 inbound channel，应在 claim 阶段直接返回 `ClaimConnAck { result = AcceptQueueFull }` 或 `ClaimConnAck { result = ListenerClosed }`，而不是先返回成功再在本地静默丢弃
- `vport` 的实际监听存在性由 `TcpTunnel` 通过 `listen_stream(ListenVPortsRef)` / `listen_datagram(ListenVPortsRef)` 注入的 provider 判断，而不是等上层 manager 在接收分发时再决定
- 对 `datagram` 的入站绑定，listener/accept 路径还需负责启动隐藏读侧 drain 观察者，并在未暴露写端的一侧触发自动 `WriteFin(0)`；不能只完成 channel 投递而遗漏底层零字节校验

### `tunnel.rs`

- `active_channels` 已从“按 `channel_id` 管理单次使用连接”改为“channel 与 `(conn_id, lease_seq)` 绑定”
- idle pool 改为存储带 `conn_id`、`lease_seq` 的统一 data connection 对象，不再按 `stream/datagram` 分池
- 每条 data connection 的长期状态收敛为唯一 `DataConnEntry`（或等价对象），并以 `conn_id` 为主键统一管理：物理连接句柄、当前连接状态、`committed_lease_seq`、当前/待提交 claim、每轮 byte 计数、pending 表、定时器、是否 retired，都归属于这个对象，而不是分散挂在 `channel_id` 或 pool 容器上
- tunnel 建立流程包含 `ControlConnReady` 等待逻辑
- 新建 data connection 包含 `DataConnReady` 等待逻辑
- `FirstClaimPending` 具有独立超时；超时后直接关闭该 data connection 并标记为 `Retired`
- 支持 `pending_open_request[request_id]` 与晚到反向 data connection 的预算控制；等待超时后清理 pending 项，但晚到连接仍可按预算规则注册或直接 `Retired`
- 每轮租约维护以下计数与状态：
  - `tx_bytes`
  - `rx_bytes`
  - `peer_final_tx_bytes`
  - `local_write_fin_sent`
  - `peer_write_fin_received`
  - `local_read_done_sent`
  - `peer_read_done_received`
  - `retired`
- `datagram` 包含隐藏方向状态：自动 `WriteFin(0)` 发送标记、隐藏读侧 drain 观察者、零字节校验结果；这些事实也归属于 `DataConnEntry`

## 迁移验证与测试

下面这些测试项用于验证迁移后的实现是否真正满足前文协议约束。

### tunnel 建立路径

- 主动侧建立 control connection 后，收到 `ControlConnReady(Success)` 才进入 `Connected`
- 被动侧在发送 `ControlConnReady(Success)` 后只进入 `PassiveReady`；active 侧收到该消息前，双方都还不应把 tunnel 当作已全局 `Connected`
- 被动侧只有在后续观察到来自 active 侧的合法 tunnel-scoped 消息后，才进入本地 `Connected`
- `TcpConnectionHello(role = Control)` 字段非法时，control connection 被拒绝
- 双边同时主动建立 control connection 时，若更上层未启用唯一性策略，则双方可各自建立不同 `tunnel_id` 的 tunnel 实例
- 若更上层启用唯一性策略，则 transport 层允许使用 `TunnelConflictLost` 作为拒绝结果
- control connection 建立失败后，当前 `tunnel_id` 不会被复用

### 正常路径

- 本地新建 data connection，收到 `DataConnReady(Success)` 后进入 `FirstClaimPending`，并只能由创建方发起首轮 claim
- 通过 `OpenDataConnReq` 请求对端反向建连成功
- 已判断双方位于同一局域网时，A 需要新 data connection 与 B 需要新 data connection 这两种情况都走直接建连，且 `open_request_id = None`
- 多个并发 `OpenDataConnReq` 可通过 `TcpConnectionHello.open_request_id` 正确关联到各自到达的新 data connection
- `OpenDataConnReq` 发出后等待反向 data connection 到达超时，会清理 `pending_open_request[request_id]`
- 复用 idle connection 后绑定 channel 成功
- 首轮 `ClaimConnAck` 只将创建方从 `Claiming` 推进到 `Bound`，不会错误把被动侧也当作“本地发起成功”处理
- `stream` 在双向 `WriteFin + ReadDone` 完成后回到 idle pool
- `datagram` 在双向 `WriteFin + ReadDone` 完成后回到 idle pool
- `datagram` 的隐藏写侧会自动发送 `WriteFin(0)`，隐藏读侧在确认全程零字节后自动发送 `ReadDone(0)`
- `ControlConnReady`、`DataConnReady`、claim、drain、pending 表超时在成功路径上都会被正确取消，不遗留悬挂定时器

### 竞争路径

- 双边同时对同一个 `(conn_id, lease_seq)` 发起 `ClaimConnReq`
- 比较 `claim_nonce` 后只有一方成功
- `claim_nonce` 相等时，双方都按 `P2pId.as_slice()` 的字节字典序得到同一个赢家
- 同一个 `ClaimConnReq` 被重复发送时，接收侧按幂等请求处理，不会因为重复投递而改变既有决定
- 失败方接受对端 channel，本地原 channel 能重新选择其他连接或新建连接
- 双边同时 claim，但其中一侧对对端请求的 `vport/listener/accept queue` 校验失败时，应优先返回对应的 `ClaimConnAck { result = ... }`，而不是进入 `ConflictLost/Success` 路径
- 冲突失败方在连接已接受对端 claim 后，再迟到收到自己那次 `ClaimConnAck { result = ConflictLost }`，不会把连接错误回退到 `Idle`
- claim 失败后若上层重试，会分配新的 `channel_id`，迟到的旧 `ClaimConnAck` 不会误命中新一轮 opening attempt

### 乱序路径

- `WriteFin` 先到，最后数据后到
- `ReadDone` 先到，本地稍后完成对应 drain 状态迁移

### 异常路径

- 未收到 `ControlConnReady` 即超时，tunnel 建立失败
- 未收到 `DataConnReady` 即超时，连接被关闭
- 未知或未注册 `conn_id` 的 `ClaimConnReq` 返回 `ProtocolError`
- 被动接收侧 accept 队列已满时，`ClaimConnReq` 返回 `AcceptQueueFull`
- `(conn_id, lease_seq)` 相同但 `channel_id`、`claim_nonce`、`kind` 或 `vport` 不一致的重复 `ClaimConnReq` 会被当作矛盾消息处理，并返回适当的 `ClaimConnAck { result = ... }` 或直接导致连接 `Retired`
- 被动接收侧在 `FirstClaimPending` 阶段主动发起首轮 claim，被本地拒绝或按协议错误处理
- `OpenDataConnReq` 的等待方已经超时或取消，但对应 data connection 稍后到达；该连接仍可完成注册并作为普通新连接使用
- 晚到的反向 data connection 到达时，若本地 idle pool / 未 claim 连接预算已满，则该连接被直接关闭并 `Retired`
- 本地 claim 已超时或已撤销后，迟到的 `ClaimConnAck` 被安全丢弃，不改变已收敛的连接状态
- 非冲突例外场景下，`ClaimConnReq` 超时默认导致当前连接 `Retired`
- `datagram` 的隐藏读侧观察到任意非零业务字节，或收到对端 `WriteFin.final_tx_bytes != 0`，连接被 `Retired`
- `pending_fin_by_conn_lease` 或 `pending_read_done_by_conn_lease` 超时后，连接被 `Retired`，且 pending 项被清理
- 本地 read 提前 drop 后：若后续确认 `rx_bytes == peer_final_tx_bytes` 且其余回收条件满足，则连接仍可复用；否则连接被 `Retired`
- 本地 write 异常退出且未成功发送 `WriteFin`，连接被 retire
- 旧 `lease_seq` 的命令迟到并被忽略
- control connection 断开后所有 data connection 都不再复用

## 取舍总结

这套方案的优点：

- 补齐了 TCP tunnel 从建立到回收的完整闭环
- 保留 data connection 复用收益
- 不把 data channel 改成 frame 协议，业务数据路径在注册完成后仍保持裸流
- 建连协议、注册协议、claim 协议、drain 协议边界清晰，减少跨连接同步歧义
- 能覆盖双边复用权、claim 竞争、旧消息迟到、回收安全性、control connection 唯一性等关键问题

这套方案的代价：

- control 协议复杂度明显上升
- tunnel 与 data connection 在进入裸流前都需要额外的显式 ready 确认
- 需要维护 tunnel 级、连接级与 opening attempt 级多层状态
- 需要引入 `tunnel_id`、`conn_id + lease_seq`、`channel_id`、`request_id` 作为新的核心一致性键

如果后续实现时发现复杂度过高，可以退回到更保守的折中方案：

- 仅保留单边复用权，减少 claim 冲突处理
- 或保留预建连池，但不复用已经使用过的 data connection

但若最终目标明确要求“TCP 下仍保留逻辑 tunnel，并且双边都能安全复用同一批 data connection”，则本文给出的“control connection 建立 + data 注册 + `conn_id + lease_seq + claim + WriteFin/ReadDone`”是最小闭合设计。
