# PN Tunnel Control Channel 设计补充

本补充文档定义 `PnTunnel` 的 tunnel 级控制通道。目标是在 logical tunnel 打开时建立一条独立控制 channel，使 PN proxy tunnel 具备与 `TcpTunnel` control connection、`QuicTunnel` command stream 相同类别的生命周期控制面：ready gate、heartbeat、对端关闭感知和本地关闭收敛。

## 范围

### 范围内
- active `PnTunnel` 创建时建立 control channel，并在 ready 成功后才返回可用 tunnel
- passive 侧收到 control open 后创建、注册并投递 `PnTunnel`，再返回 control ready
- 控制通道上的 `Ping` / `Pong` 心跳和 `Close` 命令
- 控制通道 EOF、decode 失败、写失败、heartbeat timeout 和对端 close 触发本地 tunnel 关闭
- 控制通道关闭、手动 close 与 idle close 共享同一 `PnTunnel` 关闭状态机
- 控制通道 close 后拒绝新的 local open 和 inbound business channel 投递

### 范围外
- 把 PN control channel 扩展成全局 lease heartbeat、relay session 存活模型或 PN 之外的通用传输控制协议
- 让 relay 解析控制通道 ready 后的控制命令；relay 只负责按现有 bridge 语义透传控制流字节
- 改变通用 `Tunnel` / `TunnelNetwork` trait 签名
- 改变 proxy `stream` / `datagram` 的业务 payload 加密策略

## 参考模型

`TcpTunnel` 的关键语义是：control connection 先完成 `ControlConnReady`，active 侧等待 ready 成功后才进入 connected；control read loop 退出或控制命令处理失败会关闭 tunnel。

`QuicTunnel` 的关键语义是：command stream 承载 `Ping` / `Pong`，heartbeat timeout 或 command stream 错误会让 tunnel 进入 `Error` / `Closed`，并唤醒 accept 等待者。

`PnTunnel` 应复用这两个模型的生命周期约束，但控制通道仍走 PN relay 的 `PN_PROXY_VPORT` stream，由 relay 桥接两端控制流，不要求 relay 理解 ready 之后的控制命令。

## 协议扩展

`p2p-frame/src/pn/protocol.rs` 需要新增 control open 与 control command 的命令定义。设计上推荐新增独立命令，而不是把业务 `PnChannelKind::Stream` 复用为 control，避免后续 channel 分发歧义。

```text
ProxyControlOpenReq {
    tunnel_id,
    from,
    to,
}

ProxyControlOpenResp {
    tunnel_id,
    result,
}

PnControlCmd:
    Ping { seq, send_time }
    Pong { seq, send_time }
    Close { reason }
```

`ProxyControlOpenReq/Resp` 只用于建立 control channel 的 ready gate。ready 成功后，relay 将 source 和 target 两条 control stream 直接桥接；后续 `PnControlCmd` 只在两端 `PnTunnel` 之间解析。

`ProxyOpenReq/Resp` 继续只用于业务 stream/datagram channel，不承担 tunnel 级生命周期控制。

## 建立流程

### Active

1. `PnClient::create_tunnel*` 生成 `tunnel_id` / `candidate_id`，但对象初始状态是 `OpeningControl`。
2. active 侧通过 relay 打开 `PN_PROXY_VPORT` stream，发送 `ProxyControlOpenReq`。
3. relay 基于已认证 source 身份规范化 `from`，再向 target 打开一条 `PN_PROXY_VPORT` stream 并转发 control open。
4. passive 侧创建 `PnTunnel`，保存 control stream，注册 live registry，并投递给 `PnListener.accept_tunnel()`。
5. passive 侧返回 `ProxyControlOpenResp(Success)` 后启动 control receive loop。
6. relay 回传 ready 并桥接两端 control stream。
7. active 侧收到 success 后注册 tunnel，切到 `Open`，启动 control receive loop 和 heartbeat loop，然后 `create_tunnel*` 返回。

若任一步失败，active 当前 `tunnel_id` 不得复用；错误按现有 `TunnelCommandResult` / `P2pErrorCode` 映射返回。

### Passive

passive tunnel 不应再依赖第一条业务 `ProxyOpenReq` 才创建。control open 是 logical tunnel 的创建入口：

1. 收到 control open 时，如果同 `(remote_id, tunnel_id)` 已存在 live open tunnel，应按冲突处理返回失败或关闭旧错误对象，具体策略由实现选择但必须保持幂等。
2. 若旧 weak 已失效或旧 tunnel 已关闭，应清理 registry 后创建新的 passive tunnel。
3. passive tunnel 初始 queued business channel 计数为 0；control channel 不计入 business active/pending/queued channel。
4. 只有注册成功且 listener 投递成功后，才能向 active 返回 ready success。

## 控制循环

每个 `PnTunnel` 持有 control write half，并启动 control read loop：

- 收到 `Ping`：立即返回 `Pong`。
- 收到匹配 `Pong`：刷新 heartbeat 状态。
- 收到 `Close`：调用统一 close path，reason 使用 remote close 或 interrupted。
- 读到 EOF、decode 失败或未知控制命令：调用统一 close path；decode/未知命令可映射为 protocol error。
- 发送控制命令失败：调用统一 close path。

Heartbeat 规则参考 `QuicTunnel`：

- 在 `heartbeat_interval` 内没有任何控制面入站或出站流量时，发送 `Ping`。
- 在 `heartbeat_timeout` 内没有收到任何控制面流量或匹配 `Pong` 时，关闭 tunnel。
- heartbeat 参数应复用现有 tunnel/transport 配置默认值；若第一版没有公共配置入口，必须使用内部常量并在 testing 中覆盖短 timeout 注入点。

## 状态机收敛

`PnTunnelLifecycleState` 需要在 idle close 已有计数模型上补充控制状态：

```text
OpeningControl
Open {
    control_ready,
    active_channels,
    pending_channels,
    queued_channels,
    zero_since,
}
Closing { reason }
Closed { reason, closed_at }
```

必须满足：

- `open_stream()` / `open_datagram()` 只能在 `Open` 状态登记 pending。
- inbound business `ProxyOpenReq` 只能投递到 `Open` tunnel；`OpeningControl`、`Closing`、`Closed` 都必须拒绝或触发重新创建路径。
- control close、manual close、idle close 都调用同一内部 close helper。
- close helper 负责清理 registry、drain business inbound queue、唤醒 `accept_*`、关闭 control write half，并保持幂等。
- control channel 本身不参与 idle channel 计数；否则空闲 tunnel 会因为控制通道常驻而永不 idle。

## Relay 边界

`PnServer` 只需要在 control open 阶段执行与 business open 一致的身份规范化、目标查找和 bridge 建立：

- source 身份必须来自 relay 已认证连接，不能信任报文 `from`。
- target 身份来自 `ProxyControlOpenReq.to` 并经 relay 打开目标侧 stream 成功确认。
- ready 成功后 relay 不解析 `PnControlCmd`，只统计/限速和桥接字节；TLS-over-proxy 的业务加密边界不受 control channel 影响。
- control channel 是否纳入 relay 统计/限速应与当前 bridge 字节口径一致；若要排除控制字节，必须在 testing 中单独声明。

## 兼容性

- 通用 `Tunnel` / `TunnelNetwork` trait 不新增参数。
- `create_tunnel*` 的语义从“创建本地 proxy tunnel 对象”收紧为“control channel ready 后返回可用 tunnel”；调用方获得更强可用性保证，但可能暴露新的建连失败。
- 已有 business `ProxyOpenReq/Resp` 兼容保留；新增 control open 命令需要版本内两端同时支持，不提供静默降级到无控制通道的模式。
- `tunnel_id` 与 `candidate_id` 在一个 `PnTunnel` 对象生命周期内保持稳定；control channel 断开后的对象不得重新打开。

## 风险

- 若 active 在 control ready 前返回 tunnel，上层仍可能继续遇到对端已关闭但本端未知的问题。
- 若 control close 与 idle close 没有共享状态锁，可能发生重复关闭、关闭后入队或 registry 旧对象残留。
- 若 relay 在 ready 后继续尝试解析控制命令，会把控制面与 relay bridge 耦合，破坏 PN relay 的透明转发边界。
- 若 heartbeat 过短，弱网下可能误关 tunnel；若过长，则对端关闭感知延迟过大。测试应使用可注入短 timeout，不把生产默认值改短。
