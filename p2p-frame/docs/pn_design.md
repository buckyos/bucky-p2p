# PN 设计文档

## 目标

`PN` 模块用于提供一条基于 relay 的代理 tunnel 路径。

它的职责是：

- 在两个 peer 之间通过 relay 建立代理通信路径
- 对上层暴露标准 `TunnelNetwork` / `TunnelListener` / `Tunnel` 接口
- 在直连、反连不可用时作为备用通信路径
- 复用现有 `TTP` / `NetManager` 能力，不再维护独立的数据连接体系

在新的 tunnel 元数据约束下：

- `PnTunnel` 也实现统一的 `tunnel_id()` 接口
- `PnTunnel` 也实现统一的 `candidate_id()` 接口
- `PnTunnel.form()` 固定为 `Proxy`
- `PnTunnel.is_reverse()` 固定为 `false`

## 当前结论

本次设计收敛后的关键点如下：

- 对外只有一种 `PnTunnel`，不再区分 `PnOutgoingTunnel` / `PnIncomingTunnel`
- relay 建链不再使用 `ProxyOpenNotify`
- 建链只保留一组命令：`ProxyOpenReq` / `ProxyOpenResp`
- `kind` 和 `vport` 直接并入 `ProxyOpenReq`
- 建链采用一次握手：成功即得到可用通道，失败则直接返回错误
- `datagram` 语义也直接跑在底层 `stream` 通道上
- relay 只负责转发建链请求、回传结果、桥接两端 channel stream

## 对外接口

当前 PN 对外统一暴露三类对象：

- `PnClient`：实现 `TunnelNetwork`
- `PnListener`：实现 `TunnelListener`
- `PnTunnel`：实现 `Tunnel`

上层使用方式保持不变：

- 通过 `PnClient.create_tunnel()` 获取指向远端 peer 的代理 tunnel 句柄
- 通过 `PnListener.accept_tunnel()` 获取一条入站代理 tunnel 句柄
- 通过标准 `Tunnel` 的 `open_stream` / `accept_stream` / `open_datagram` / `accept_datagram` 使用 PN

这里的变化只在内部实现语义：

- `PnTunnel` 是统一类型
- 主动打开和被动接收只是内部状态差异
- 不再把方向差异暴露到类型层面

## 角色与拓扑

设：

- `A`：发起方
- `S`：relay/server
- `B`：目标方

稳定前提：

- `A` 维持到 `S` 的 tunnel
- `B` 维持到 `S` 的 tunnel

因此建立 `A -> B` 的 `PnTunnel` 时，不需要额外通知 B 再反向补一条独立 data connection。

正确模型是：

1. `A` 新建一条到 `S` 的 channel stream
2. `A` 在这条 stream 上发送 `ProxyOpenReq`
3. `S` 在到 `B` 的既有 tunnel 上再打开一条 channel stream
4. `S` 将 `ProxyOpenReq` 转发给 `B`
5. `B` 校验请求后返回 `ProxyOpenResp`
6. `S` 将响应转发给 `A`
7. 若成功，`S` 直接桥接 A/B 两条 channel stream

自此代理通道建立完成。

## 整体结构

PN 可以拆成三层：

### 1. relay stream 层

PN 使用固定 `vport` 在 relay 上承载 channel stream：

- `PN_PROXY_VPORT = 0xfff1`

该 stream 由 `TTP` 提供：

- `A` 侧通过 `TtpClient.open_stream(..., PN_PROXY_VPORT)` 打到 `S`
- `S` 侧通过 `TtpServer` 接收来自 `A` 的 stream
- `S` 再通过 `TtpServer.open_stream_by_id(..., PN_PROXY_VPORT)` 在到 `B` 的既有 tunnel 上打开一条新 stream
- `B` 侧通过 `TtpClient.listen_stream(PN_PROXY_VPORT)` 收到 relay 转发过来的 stream

### 2. 建链命令层

当前建链命令只保留：

- `ProxyOpenReq`
- `ProxyOpenResp`

它们完成两件事：

- 声明要打开什么代理通道
- 返回目标侧是否接受该通道

relay 不需要再维护单独的通知命令。

### 3. 数据透传层

当 `ProxyOpenResp` 成功后：

- relay 将 A/B 两条 channel stream 直接桥接
- 后续所有业务字节流都透传
- relay 不再解析业务数据

## 目录结构

- `pn/client/pn_client.rs`
  - `PnClient`
- `pn/client/pn_listener.rs`
  - `PnListener`
- `pn/client/pn_tunnel.rs`
  - `PnTunnel`
- `pn/protocol.rs`
  - PN 常量与命令定义
- `pn/service/pn_server.rs`
  - relay 侧 `PnServer`

## 协议定义

定义位于 `p2p-frame/src/pn/protocol.rs`。

### 命令

- `ProxyOpenReq`
- `ProxyOpenResp`

### 请求

建议收敛为：

```rust
pub enum PnChannelKind {
    Stream = 1,
    Datagram = 2,
}

pub struct ProxyOpenReq {
    pub tunnel_id: TunnelId,
    pub from: P2pId,
    pub to: P2pId,
    pub kind: PnChannelKind,
    pub vport: u16,
}
```

字段含义：

- `tunnel_id`：本次代理建链的唯一标识
- `from`：发起方 peer id
- `to`：目标方 peer id
- `kind`：请求打开的通道类型
- `vport`：目标业务端口

说明：

- `A -> S` 时，`S` 不能盲信请求里的 `from`
- `S` 应以底层 tunnel/stream 的认证身份为准，必要时重写 `from`
- `S -> B` 时，转发的是经 `S` 校正后的 `ProxyOpenReq`

### 响应

```rust
pub struct ProxyOpenResp {
    pub tunnel_id: TunnelId,
    pub result: u8,
}
```

其中 `result` 复用 `TunnelCommandResult` 语义。

### 已删除的旧命令

以下命令不再属于当前设计：

- `ProxyOpenNotify`
- `ProxyOpenReady`
- `ProxyChannelOpen`
- `ProxyChannelOpenResp`

原因：

- 不再需要 relay 先通知 B 再让 B 回连一条 data connection
- `kind/vport` 已并入 `ProxyOpenReq`
- 建链结果改为一次握手完成，不再做二段 channel open 协商

## 关键对象

### `PnClient`

`PnClient` 是 PN 的核心入口。

它负责：

- 复用 `TtpClient` 当前已经缓存的 relay tunnel
- 建立到 relay 的 `PN_PROXY_VPORT` stream
- 对上层实现 `TunnelNetwork`
- 持有共享的 `TtpClient`，同时用于主动打开和被动监听 `PN_PROXY_VPORT`

`PnClient.create_tunnel()` 返回一个逻辑上的 `PnTunnel`：

- 它知道目标 peer 是谁
- 它带有一个逻辑 `tunnel_id`
- 它还带有一个具体 `candidate_id`
- 尚未绑定具体代理 channel
- 具体 channel 在 `open_stream()` / `open_datagram()` 时建立

### `PnListener`

`PnListener` 基于 `TtpClient.listen_stream(PN_PROXY_VPORT)` 工作。

它接收到 relay 转发来的新 stream 后：

1. 读取首个 `ProxyOpenReq`
2. 根据请求中的 `from` / `to` / `kind` / `vport` 构造一个被动状态的 `PnTunnel`
3. 将这条 `PnTunnel` 交给上层

也就是说：

- 被动侧不再先收一条通知命令
- 收到的第一个命令本身就是完整建链请求

### `PnTunnel`

`PnTunnel` 是无方向区分的统一 tunnel 类型。

它可以处于两类内部状态：

- 主动状态：由 `PnClient.create_tunnel()` 创建，用于向远端发起代理建链
- 被动状态：由 `PnListener.accept_tunnel()` 创建，已经持有 relay 转发过来的 channel stream 和 `ProxyOpenReq`

其行为约定如下：

- `open_stream(vport)`
  - 新建一条到 relay 的 channel stream
  - 发送 `ProxyOpenReq { tunnel_id, from, to, kind=Stream, vport }`
  - 等待 `ProxyOpenResp`
  - 成功后直接返回 `(read, write)`
- `accept_stream()`
  - 基于收到的 `ProxyOpenReq` 进行本地校验
  - 返回 `ProxyOpenResp`
  - 成功后把当前 stream 作为 `(read, write)` 交给上层
- `open_datagram(vport)`
  - 发送 `ProxyOpenReq { tunnel_id, kind=Datagram, vport }`
  - 底层仍建立并桥接 stream
  - 成功后只返回 `write`
- `accept_datagram()`
  - 基于收到的 `ProxyOpenReq { kind=Datagram }` 完成校验
  - 底层仍使用当前 stream
  - 成功后只返回 `read`

### `PnServer`

`PnServer` 运行在 relay 侧。

它负责：

1. 接收来自 `A` 的 `PN_PROXY_VPORT` stream
2. 读取首个命令 `ProxyOpenReq`
3. 在到 `B` 的既有 tunnel 上打开新的 `PN_PROXY_VPORT` stream
4. 将 `ProxyOpenReq` 转发给 `B`
5. 读取 `B` 返回的 `ProxyOpenResp`
6. 将 `ProxyOpenResp` 转发给 `A`
7. 若结果成功，则桥接两端 stream

relay 不负责：

- 判断 `vport` 是否监听
- 决定最终是 `stream` 还是 `datagram`
- 解析后续业务 payload

这些都交给目标端 `B` 自己处理。

## 建链流程

### 流程一：A 主动打开到 B 的代理通道

假设 `A` 需要通过 relay 联系 `B`。

1. 上层通过 `PnClient.create_tunnel()` 得到 `PnTunnel(A->B)`
2. 上层调用 `open_stream(vport)` 或 `open_datagram(vport)`
3. `PnTunnel` 新建一条 `A -> S` 的 `PN_PROXY_VPORT` stream
4. `PnTunnel` 发送 `ProxyOpenReq { tunnel_id, from, to, kind, vport }`
5. `S` 读取请求后，在到 `B` 的既有 tunnel 上打开一条新的 `S -> B` channel stream
6. `S` 将 `ProxyOpenReq` 转发给 `B`
7. `B` 校验请求，返回 `ProxyOpenResp { tunnel_id, result }`
8. `S` 将该响应转发给 `A`
9. 若 `result == Success`，`S` 开始桥接 A/B 两条 stream
10. `A` 收到成功响应后，代理通道建立完成

这个流程里，建链成功本身就意味着通道已可用，不再存在后续独立的 channel-open 阶段。

### 流程二：B 接收入站 PN tunnel

1. `S` 在到 `B` 的既有 tunnel 上打开 `PN_PROXY_VPORT` stream
2. `S` 将 `ProxyOpenReq` 写入这条 stream
3. `B` 的 `PnListener` 通过 `TtpClient.listen_stream(PN_PROXY_VPORT)` 接收到这条 stream
4. `PnListener` 读取完整 `ProxyOpenReq`
5. `PnListener` 构造一个被动状态的 `PnTunnel`
6. 上层通过 `accept_tunnel()` 得到这条 `PnTunnel`
7. 当上层调用 `accept_stream()` 或 `accept_datagram()` 时，`PnTunnel` 执行本地校验并写回 `ProxyOpenResp`
8. 若校验通过，该通道建立完成并交给上层

这里的关键点是：

- 入站 `PnTunnel` 一开始就已经拿到了完整建链请求
- 不需要额外的通知命令
- 不需要第二阶段的 channel-open 命令

### 流程三：relay 配对与桥接

relay 侧逻辑被收敛为一个简单模型：

1. 接收 `A -> S` channel stream
2. 读取 `ProxyOpenReq`
3. 新建 `S -> B` channel stream
4. 转发 `ProxyOpenReq`
5. 接收 `B -> S` 的 `ProxyOpenResp`
6. 转发 `ProxyOpenResp` 给 `A`
7. 若成功，则桥接两条 stream

因此 relay 真正需要理解的只有一组建链命令：

- `ProxyOpenReq`
- `ProxyOpenResp`

## stream / datagram 语义

### stream

`stream` 语义保持直观：

- `open_stream(vport)` 成功，表示远端已经确认该 `vport` 可接受 stream 通道
- `accept_stream()` 成功，表示本地已经确认该请求合法并接受该 stream

### datagram

当前 `PnTunnel` 上的 `datagram` 通道也直接使用底层 `stream` 来承载。

也就是说：

- `open_datagram(vport)` 本质上仍打开并桥接一条 stream
- `accept_datagram()` 本质上也是消费同一条 stream
- 只是接口层面：
  - 主动侧只返回 `write`
  - 被动侧只返回 `read`

这样做的直接含义是：

- PN 不提供真正的消息边界语义
- 若上层需要 datagram-style framing，必须自行封包
- relay 完全不关心是 `stream` 还是 `datagram` 业务语义，只透传字节流

## 通道校验语义

来自 `proxy_tunnel_design.md` 的有效约束保留如下：

- `open_stream(vport)` 成功，表示远端确认该 `vport` 已监听
- `open_datagram(vport)` 成功，表示远端确认该 `vport` 已监听
- 错误端口在 `open_*` 阶段立即返回 `PortNotListen`
- `accept_stream()` / `accept_datagram()` 之前必须已经先调用对应的 `listen_*()`
- 若 `listen_*()` 注入与 `accept_*()` 存在初始化竞态，`PnTunnel` 仅在首次读取对应 kind 的监听状态时做一次短暂异步等待；`stream` 与 `datagram` 状态彼此独立
- 若首次等待后仍未先 `listen_*()` 就调用 `accept_*()`，本地失败；目标侧看到的握手结果为 `ListenerClosed`
- 失败的代理通道不应被发布为可用 tunnel

目标侧 `B` 在返回 `ProxyOpenResp` 前需要完成：

- `to == self.local_id()` 校验
- `kind` 合法性校验
- 对应 `vport` 是否已监听校验
- tunnel/provider 是否仍可用校验

## 错误语义

PN 当前主要暴露：

- `NotFound`
  - 共享 `TtpClient` 中没有可复用的 relay tunnel
  - relay 没有到目标 peer 的可复用 tunnel
- `ConnectFailed`
  - relay 无法打开到目标 peer 的 channel stream
- `PortNotListen`
  - 目标侧未监听请求的 `vport`
- `Timeout`
  - 等待 `ProxyOpenResp` 超时
- `InvalidParam`
  - 非法 `kind`
  - 非法 `vport`
  - 请求字段不合法
- `InvalidData`
  - 收到未知或不匹配的 command
- `Interrupted`
  - listener 结束
  - tunnel/provider 已关闭
  - 建链中途链路断开
- `IoError`
  - 底层 stream 读写失败

## 状态与超时

虽然协议变成一次握手，但 relay 运行时仍需要维护最小状态：

- A 侧入站 channel stream
- B 侧出站 channel stream
- 当前 `tunnel_id` 对应的一次建链上下文

典型清理场景：

- B 未及时返回 `ProxyOpenResp`
- A 在等待期间断开
- S 到 B 的 channel stream 打开失败
- B 返回失败结果

建议保留明确的建链超时，例如 `500~1500ms`。

## 与 SN / TTP 的关系

### 与 SN 的关系

当前 PN 仍依赖 SN：

- `PnClient` 自身不再直接依赖 `SNClientService`
- 但当前默认 relay tunnel 仍通常由 SN 路径先通过共享 `TtpClient` 建立并缓存

因此当前 PN 仍建立在“active SN 同时充当 relay”的假设上。

### 与 TTP 的关系

当前 PN 的底层传输完全建立在 `TTP` 之上：

- client 侧通过共享 `TtpClient` 打开/监听 `PN_PROXY_VPORT`
- relay 侧通过 `TtpServer` 接收和打开 `PN_PROXY_VPORT`
- PN 自己不再维护独立的“通知后再回连”数据连接模型

从职责上看：

- `TTP` 负责把 stream 能力建立在现有 tunnel 上
- `PN` 负责在 relay 上完成 A/B 配对与桥接

## 与统一 tunnel 元数据的对齐

- `TunnelNetwork::create_tunnel_with_intent(...)` 传入的 `intent.tunnel_id` 会成为新建 `PnTunnel` 的逻辑 ID
- `TunnelNetwork::create_tunnel_with_intent(...)` 传入的 `intent.candidate_id` 会成为新建 `PnTunnel` 的具体候选 ID
- 若上层未显式指定 `tunnel_id`，`PnClient` 会在本地生成一个新的逻辑 ID
- 若上层未显式指定 `candidate_id`，`PnClient` 当前默认使用一个与 `tunnel_id` 对齐的本地候选 ID
- 被动侧 `PnListener.accept_tunnel()` 返回的 `PnTunnel.tunnel_id()` 直接取自收到的 `ProxyOpenReq.tunnel_id`
- 被动侧 `PnListener.accept_tunnel()` 返回的 `PnTunnel.candidate_id()` 当前默认与 `tunnel_id` 对齐
- 当前 PN 不区分普通直连/反连语义，因此 `is_reverse()` 始终为 `false`

这里的语义约束是：

- PN 目前不承担“同一个逻辑 `tunnel_id` 下多 candidate 竞争”的 transport 级并发建链问题
- 因此 `candidate_id` 主要用于对齐统一 `Tunnel` trait，而不是像 TCP/QUIC 那样参与底层握手路由

## 当前实现边界

### 1. 仍依赖 active SN

PN 还没有独立的 relay 发现与维护机制，仍依赖共享 `TtpClient` 中已经存在的 relay tunnel。

### 2. relay 必须持有到目标 peer 的既有 tunnel

因为 relay 需要直接在到 `B` 的既有 tunnel 上开一条新的 `PN_PROXY_VPORT` channel stream。

### 3. datagram 只是接口语义，不是真正底层 datagram

当前 `open_datagram()` / `accept_datagram()` 只是以 stream 作为承载，并在接口层裁剪返回值。

### 4. 被动侧实例通常仍是一条待消费通道

虽然对外统一为 `PnTunnel`，但由 `PnListener.accept_tunnel()` 返回的实例通常仍对应一条已到达的入站代理请求，最终会被一次 `accept_*()` 消费。

## 历史方案说明

以下设计已不再作为当前方案的一部分：

- `ProxyOpenNotify`
- `ProxyOpenReady`
- `ProxyChannelOpen` / `ProxyChannelOpenResp`
- `PN2D` 同端口分流前导方案
- “B 先收到通知，再自行补建 data connection” 的旧模型

如果旧文档与本文冲突，以本文为准。

## 小结

当前 PN 设计可以概括为：

- 用 `PN_PROXY_VPORT` 承载 PN 的 channel stream
- 只保留 `ProxyOpenReq` / `ProxyOpenResp` 两个建链命令
- `kind` / `vport` 并入 `ProxyOpenReq`
- `A -> S -> B` 一次握手成功后立即 bridge
- `PnTunnel` 对外不再区分入站/出站类型
- `datagram` 语义也统一承载在 stream 上
- relay 只负责转发建链请求、转发结果、桥接字节流
- 运行时仍依赖 SN 作为 relay 发现来源
