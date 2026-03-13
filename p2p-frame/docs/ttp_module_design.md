# Ttp 客户端/服务器通信模块设计

## 目标

在 `p2p-frame` 现有 `TunnelNetwork` / `Tunnel` / `NetManager` 之上，定义一层更稳定的客户端/服务器通信抽象，下文简称 `Ttp`，用于统一表达：

- 客户端主动向指定远端地址打开某个 `vport` 的 `stream` / `datagram`
- 客户端在自己已建立的传输通道上监听指定 `vport` 的入站 `stream` / `datagram`
- 服务端在已有 tunnel 上向指定远端打开某个 `vport` 的 `stream` / `datagram`
- 服务端监听指定 `vport` 的入站 `stream` / `datagram`

该模块只负责传输，不关心任何上层业务协议。

## 设计原则

- `Ttp` 层基于 `NetManager` 创建和接收入站 `Tunnel`
- `Ttp` 层暴露 `stream` 和 `datagram` 两类基础通道语义，不直接暴露底层网络协议细节
- 地址统一使用 `Endpoint`
- 建立连接时显式携带 `remote_id`，用于身份校验、连接复用和缓存键
- 客户端和服务端共享一套底层 tunnel 分发能力，但对外职责分离
- 上层只依赖传输接口，不直接操作 `Tunnel` 和 `NetManager`
- 只有客户端可以主动创建到服务端的 tunnel，服务端不能主动向客户端建 tunnel

## 现有基础能力

当前仓库已经具备实现 `Ttp` 层所需的核心基础：

- `NetManager` 负责：
  - 根据 `Protocol` 选择 `TunnelNetwork`
  - 调用 `TunnelNetwork::create_tunnel()` 创建出站 tunnel
  - 调用 `register_tunnel_acceptor(local_id)` 接收入站 tunnel
  - 统一维护监听端点信息
- `Tunnel` 负责：
  - `open_stream(vport)` 主动打开指定端口的流
  - `accept_stream()` 接收对端主动打开的流
  - `open_datagram(vport)` 主动打开指定端口的 datagram 发送端
  - `accept_datagram()` 接收对端主动打开的 datagram 入站数据
  - 维护 tunnel 状态与生命周期

因此，新的 `Ttp` 层不重新发明连接管理，而是对 `NetManager + Tunnel` 做一层面向业务的包装。

额外约束：

- 客户端负责主动建 tunnel
- 服务端只接收入站 tunnel
- 服务端虽然可以在已有 tunnel 上主动打开 `stream/datagram`，但不能在 tunnel 不存在时反向创建新 tunnel

## 模块作用

`Ttp` 模块位于网络层和业务协议层之间，主要承担以下职责：

- 将“建立/复用 tunnel”封装成“打开 stream/datagram”能力
- 将“tunnel 上的入站 stream/datagram”重新组织为“按 vport 监听”能力
- 为客户端和服务端统一返回 stream/datagram 元信息
- 屏蔽底层 tunnel 来源、监听循环、分发队列等细节
- 提供稳定的生命周期和并发语义

当前代码中，`SN` 和 `PN` 都已经直接使用这层抽象：

- `SN` 客户端通过 `TtpClient` 打开 `SN_CMD_VPORT`
- `SN` 服务端通过 `TtpServer` 监听 `SN_CMD_VPORT`
- `PN` 客户端通过共享 `TtpClient` 打开并监听 `PN_PROXY_VPORT`
- relay 侧 `PN` 通过 `TtpServer` 向目标 peer 直接打开 `PN_PROXY_VPORT`

## 核心模型

### 目标地址

```rust
pub struct TtpTarget {
    pub local_ep: Option<Endpoint>,
    pub remote_ep: Endpoint,
    pub remote_id: P2pId,
    pub remote_name: Option<String>,
}
```

说明：

- `remote_ep` 是实际连接目标地址
- `remote_id` 是远端身份标识，不与地址混用
- `remote_name` 用于需要名称的握手场景
- `local_ep` 为可选项，用于需要固定本地出口地址的场景

### 入站/出站 stream 元信息

```rust
pub struct TtpStreamMeta {
    pub local_ep: Option<Endpoint>,
    pub remote_ep: Option<Endpoint>,
    pub local_id: P2pId,
    pub remote_id: P2pId,
    pub remote_name: Option<String>,
    pub vport: u16,
}

pub struct TtpStream {
    pub meta: TtpStreamMeta,
    pub read: TunnelStreamRead,
    pub write: TunnelStreamWrite,
}
```

说明：

- `TtpStream` 是 `Ttp` 层唯一对上暴露的数据流对象
- `meta` 中保留地址、身份和端口信息，便于上层做路由和审计
- `local_ep` / `remote_ep` 使用 `Option`，兼容底层 tunnel 尚不能稳定提供端点的场景

### 入站/出站 datagram 元信息

```rust
pub struct TtpDatagramMeta {
    pub local_ep: Option<Endpoint>,
    pub remote_ep: Option<Endpoint>,
    pub local_id: P2pId,
    pub remote_id: P2pId,
    pub remote_name: Option<String>,
    pub vport: u16,
}

pub struct TtpDatagram {
    pub meta: TtpDatagramMeta,
    pub read: TunnelDatagramRead,
}

pub struct TtpDatagramWriter {
    pub meta: TtpDatagramMeta,
    pub write: TunnelDatagramWrite,
}
```

说明：

- 出站 `open_datagram()` 返回 `TtpDatagramWriter`
- 入站 `accept()` 返回 `TtpDatagram`
- `datagram` 语义直接沿用底层 `Tunnel` 当前抽象：主动侧拿到发送端，接收侧拿到入站可读对象

## 对外接口

### stream 监听器接口

```rust
#[async_trait::async_trait]
pub trait TtpListener: Send + Sync + 'static {
    async fn accept(&self) -> P2pResult<TtpStream>;
}
```

语义：

- 一个 `TtpListener` 对应一个固定 `vport`
- `accept()` 返回下一个到达该 `vport` 的入站流
- 若监听关闭，则返回 `Interrupted` 或等价错误

### datagram 监听器接口

```rust
#[async_trait::async_trait]
pub trait TtpDatagramListener: Send + Sync + 'static {
    async fn accept(&self) -> P2pResult<TtpDatagram>;
}
```

语义：

- 一个 `TtpDatagramListener` 对应一个固定 `vport`
- `accept()` 返回下一个到达该 `vport` 的入站 datagram
- 若监听关闭，则返回 `Interrupted` 或等价错误

### 公共端口监听接口

```rust
#[async_trait::async_trait]
pub trait TtpPortListener: Send + Sync + 'static {
    async fn listen_stream(&self, vport: u16) -> P2pResult<Arc<dyn TtpListener>>;
    async fn unlisten_stream(&self, vport: u16) -> P2pResult<()>;

    async fn listen_datagram(&self, vport: u16) -> P2pResult<Arc<dyn TtpDatagramListener>>;
    async fn unlisten_datagram(&self, vport: u16) -> P2pResult<()>;
}
```

语义：

- `listen_stream(vport)` 为本端注册一个 `vport` 监听器
- 同一个对象上，同一个 `vport` 只允许注册一次
- `unlisten_stream(vport)` 注销监听，并使后续 `accept()` 尽快结束
- `listen_datagram(vport)` / `unlisten_datagram(vport)` 语义与 `stream` 相同，但作用于 datagram 通道

### 主动打开接口

```rust
#[async_trait::async_trait]
pub trait TtpConnector: Send + Sync + 'static {
    async fn open_stream(
        &self,
        target: &TtpTarget,
        vport: u16,
    ) -> P2pResult<TtpStream>;

    async fn open_datagram(
        &self,
        target: &TtpTarget,
        vport: u16,
    ) -> P2pResult<TtpDatagramWriter>;
}
```

语义：

- `open_stream()` 主动向指定 `remote_ep + remote_id` 打开一个流
- `open_datagram()` 主动向指定 `remote_ep + remote_id` 打开一个 datagram 发送端
- 若已有可复用的健康 tunnel，可直接复用
- 若不存在可用 tunnel，则通过 `NetManager` 创建新的 tunnel

### 客户端对象

```rust
pub struct TtpClient { ... }
```

语义：

- `TtpClient` 同时实现 `TtpConnector` 与 `TtpPortListener`
- 客户端既支持主动打开 `stream/datagram`，也支持监听入站 `stream/datagram`
- 客户端监听范围是所有“由本客户端管理的已建立 tunnel”

### 服务端对象

```rust
pub struct TtpServer { ... }
```

语义：

- `TtpServer` 同时实现 `TtpConnector` 与 `TtpPortListener`
- 服务端既支持监听本地端口，也支持在“已存在的客户端入站 tunnel”上主动打开 `stream/datagram`
- 所有入站 tunnel 均来自 `NetManager.register_tunnel_acceptor(local_id)`
- 服务端 `listen_stream(vport)` / `listen_datagram(vport)` 监听的是“所有入站 tunnel 上到达本地该端口的通道”
- 服务端 `open_stream()` / `open_datagram()` 只允许复用现有 tunnel；若目标 tunnel 不存在，则返回错误，而不是主动建 tunnel

## 推荐实现结构

建议在 `p2p-frame/src/ttp/` 下拆分为以下模块：

- `mod.rs`
- `types.rs`
- `listener.rs`
- `registry.rs`
- `runtime.rs`
- `client.rs`
- `server.rs`

### `types.rs`

定义：

- `TtpTarget`
- `TtpStreamMeta`
- `TtpStream`
- `TtpDatagramMeta`
- `TtpDatagram`
- `TtpDatagramWriter`

### `listener.rs`

定义：

- `TtpListener` trait
- `TtpDatagramListener` trait
- 基于异步队列的默认 `TtpListener` 实现

建议：

- `stream` 和 `datagram` 分别使用独立 listener 实现与独立队列

### `registry.rs`

负责：

- 维护 `vport -> listener queue`
- 分别维护 `stream_vport -> listener queue` 与 `datagram_vport -> listener queue`
- 检查重复注册
- 处理 listener 注销与关闭广播

### `runtime.rs`

负责：

- 接管一个 tunnel 的 `accept_stream()` / `accept_datagram()` 循环
- 将入站对象转换为 `TtpStream` 或 `TtpDatagram`
- 按 `vport` 分发到对应 `stream/datagram listener`
- 管理已接管 tunnel 集合，避免重复启动 accept loop

### `client.rs`

负责：

- `open_stream()` / `open_datagram()` 的 tunnel 选择、创建、复用
- 客户端已建立 tunnel 的注册与回收
- 对外实现 `TtpClient`

### `server.rs`

负责：

- 从 `NetManager` 注册本地 `TunnelAcceptor`
- 接收入站 tunnel 并交给 `TtpRuntime`
- 支持服务端在已有入站 tunnel 上主动打开 `stream/datagram`
- 对外实现 `TtpServer`

## 内部原理

### 一、主动打开 stream/datagram

`TtpConnector::open_stream()` / `open_datagram()` 的内部流程建议如下：

1. 读取 `target.remote_ep.protocol()`
2. 通过 `NetManager::get_network(protocol)` 找到对应 `TunnelNetwork`
3. 优先从客户端内部 tunnel 池中查找可复用 tunnel
4. 若未命中：
   - 若 `target.local_ep.is_some()`，调用 `create_tunnel_with_local_ep()`
   - 否则调用 `create_tunnel()`
5. 将新 tunnel 注册到 `TtpRuntime`
6. 根据调用类型：
   - `stream` 调用 `tunnel.open_stream(vport)`
   - `datagram` 调用 `tunnel.open_datagram(vport)`
7. 返回 `TtpStream` 或 `TtpDatagramWriter`

对应特点：

- 连接建立与通道打开对上层表现为一次调用
- tunnel 复用逻辑留在`Ttp` 层内部
- 上层不需要感知 `TunnelRef`

对于客户端与服务端，这里的语义不同：

- 客户端可以在未命中 tunnel 池时，通过 `NetManager` 创建新 tunnel，再打开通道
- 服务端不允许创建新 tunnel，只能在已有入站 tunnel 上打开通道
- 因此服务端实现 `open_stream()` / `open_datagram()` 时，只做 tunnel 查找与复用；若未命中则返回 `NotFound`、`Interrupted` 或等价错误

### 二、客户端监听 stream/datagram

客户端并不是只会主动打开通道。对于已经由客户端创建或接管的 tunnel，对端也可能在这些 tunnel 上向本端打开新的 `vport` stream/datagram。

因此客户端实现也需要：

1. 支持 `listen_stream(vport)` 和 `listen_datagram(vport)`
2. 在每个已管理 tunnel 上运行 `accept_stream()` 和 `accept_datagram()` 循环
3. 将收到的入站对象按 `vport` 投递到对应 listener 队列

这里的监听范围是：

- 监听所有“已由当前客户端建立或接管”的 tunnel 上的指定 `vport`
- 不按某个特定远端单独建 listener
- 通过返回元信息中的 `remote_id` 和 `remote_ep` 区分来源

### 三、服务端接收 stream/datagram

服务端侧内部流程建议如下：

1. 在初始化时通过 `NetManager::register_tunnel_acceptor(local_id)` 注册 tunnel 接收器
2. 启动后台循环，不断 `accept_tunnel()`
3. 每接收到一个新 tunnel，就将其交给 `TtpRuntime`
4. `TtpRuntime` 对该 tunnel 持续执行 `accept_stream()` 和 `accept_datagram()`
5. 将收到的对象按 `vport` 分发到对应监听器

服务端接收入站 tunnel 依赖底层 `NetManager` 和各 `TunnelNetwork`。服务端主动打开通道时，只能复用这些已经接入运行时的 tunnel，不能通过 `NetManager` 反向创建到客户端的新 tunnel。

### 四、统一分发器

客户端和服务端虽然入口不同，但底层分发逻辑可以复用同一套运行时：

- 输入：`TunnelRef`
- 行为：为 tunnel 启动唯一的 `accept_stream()` / `accept_datagram()` 循环
- 输出：按 `vport` 投递 `TtpStream` / `TtpDatagram`

这意味着真正需要共享的是：

- listener 注册表
- tunnel 接管状态
- stream/datagram 分发循环
- tunnel 关闭清理逻辑

而客户端和服务端仅在“tunnel 从哪里来”这一点上不同：

- 客户端 tunnel 来自主动创建/复用
- 服务端 tunnel 来自 `NetManager` 入站 acceptor

也就是说：

- 客户端具备“建 tunnel + 用 tunnel”的完整能力
- 服务端只具备“收 tunnel + 用 tunnel”的能力

## 关键内部组件

### Tunnel 池

客户端需要一个 tunnel 池，建议键为：

- `remote_id`
- `remote_ep`
- `local_ep` 可选参与

作用：

- 复用已有健康连接
- 减少重复握手
- 将“打开多个 `vport` 的 stream/datagram”复用到同一个底层 tunnel 上

服务端也需要维护一个 tunnel 映射，但用途不同：

- 它不用于主动建 tunnel
- 只用于记录当前已经接入的入站 tunnel
- 供服务端 `open_stream()` / `open_datagram()` 在已有 tunnel 上反向开通道时查找目标连接

### Tunnel 接管表

运行时需要维护一个“已接管 tunnel 集合”，防止以下问题：

- 同一个 tunnel 被重复启动多个 `accept_stream()` 循环
- 同一个 tunnel 被重复启动多个 `accept_datagram()` 循环
- 多个循环竞争同一入站对象，造成数据丢失或乱序

建议按 `Arc::as_ptr(&tunnel)` 或等价稳定标识做去重。

### VPort 监听注册表

建议使用：

- `HashMap<u16, StreamListenerState>`
- `HashMap<u16, DatagramListenerState>`

`ListenerState` 至少包含：

- 入站队列 sender
- 关闭标志
- 监听器生命周期控制对象

要求：

- 同一 `vport` 不允许重复监听
- 注销后新到达的流直接丢弃或记录日志
- 已阻塞的 `accept()` 能够尽快结束

## 生命周期

### 客户端生命周期

- 创建 `TtpClient`
- 按需 `listen_stream(vport)` / `listen_datagram(vport)`
- 按需 `open_stream(target, vport)` / `open_datagram(target, vport)`
- `open_*()` 时自动建立或复用 tunnel
- tunnel 关闭后自动从池中清理

### 服务端生命周期

- 创建 `TtpServer`
- 注册一个或多个 `stream/datagram vport` 监听器
- 启动入站 tunnel 接收循环
- 新入站 tunnel 自动纳入运行时分发
- 若需要主动向客户端回发 `stream/datagram`，则从已有入站 tunnel 中查找并复用
- 停止时取消 accept 任务并关闭所有 listener

## 并发语义

### listener 注册

- 对同一 `vport` 的重复 `listen_stream()` / `listen_datagram()` 返回 `AlreadyExists`
- `unlisten_stream()` / `unlisten_datagram()` 与 `accept()` 并发时，`accept()` 应尽快返回中断错误

### tunnel 接管

- 同一个 tunnel 只允许一个 stream accept 循环和一个 datagram accept 循环
- 多个打开流请求可以并发复用同一个 tunnel
- 多个打开 datagram 请求也可以并发复用同一个 tunnel
- tunnel 关闭时，应通知运行时移除该 tunnel

### 队列背压

可以有两种实现策略：

- 无界队列：实现简单，但需要防止监听者长期不消费
- 有界队列：可控制内存，但需要定义队列满时的丢弃或错误策略

第一阶段建议优先使用无界队列，以降低实现复杂度。

## 错误语义

`Ttp` 层建议统一暴露以下错误类别：

- `NotFound`
  - 协议对应的 `TunnelNetwork` 不存在
  - 指定目标不可达
- `ConnectFailed`
  - 建立 tunnel 失败
- `PortNotListen`
  - 远端未监听指定 `vport`
- `AlreadyExists`
  - 本地重复监听同一 `vport`
- `NotFound`
  - 服务端尝试向某个目标打开通道，但当前不存在可复用的入站 tunnel
- `Interrupted`
  - listener 被关闭
  - acceptor 被移除
  - tunnel 已关闭
- `Timeout`
  - 建链或打开通道超时

`Ttp` 层不引入独有错误码，直接复用现有 `P2pErrorCode`。

## 与现有模块的关系

### 与 `NetManager` 的关系

`NetManager` 是`Ttp` 层的唯一 tunnel 来源：

- 客户端通过它获取 `TunnelNetwork` 并创建出站 tunnel
- 服务端通过它注册 `TunnelAcceptor` 并接收入站 tunnel

约束：

- 只有客户端会调用 `TunnelNetwork::create_tunnel()` 或 `create_tunnel_with_local_ep()` 主动建 tunnel
- 服务端不会通过 `NetManager` 主动向客户端建 tunnel
- 服务端只消费 `NetManager.register_tunnel_acceptor(local_id)` 接收到的入站 tunnel

`Ttp` 层不直接操作具体的 `TcpTunnelNetwork`、`QuicTunnelNetwork` 实现。

### 与 `Tunnel` 的关系

`Ttp` 层将 `Tunnel` 视为底层连接载体：

- `Tunnel::open_stream(vport)` 对应客户端主动打开流
- `Tunnel::accept_stream()` 对应运行时接收入站流
- `Tunnel::open_datagram(vport)` 对应主动打开 datagram 发送端
- `Tunnel::accept_datagram()` 对应运行时接收入站 datagram

`Ttp` 层不改变 `Tunnel` 的协议行为，只做组织和复用。

## 为什么需要单独的`Ttp` 层

如果上层协议直接依赖 `NetManager` 和 `Tunnel`，通常会产生以下问题：

- 每个协议都重复实现 tunnel 复用逻辑
- 每个协议都重复实现 `vport` 监听注册表
- 每个协议都重复实现 tunnel 上的 stream/datagram accept 循环和分发任务
- 客户端侧“已建立 tunnel 上的被动入站流”容易被忽略
- datagram 的打开与接收语义容易在各协议中分散实现

独立`Ttp` 层后，可以将这些通用逻辑集中到一处：

- 减少重复实现
- 统一错误和并发语义
- 让上层协议只面向 `open_*()` / `listen_*()` 编程
- 显式固化“客户端建 tunnel、服务端复用入站 tunnel”的方向性约束

## 后续扩展点

该设计当前已覆盖 `stream` 和 `datagram`，后续可以继续扩展：

- 在客户端 tunnel 池中加入更细粒度的健康度与替换策略
- 引入 listener 队列容量控制
- 增加传输统计信息，如：
  - 当前活跃 tunnel 数
  - 每个 `vport` 入站队列深度
  - 打开 stream/datagram 成功率和失败原因

## 小结

`Ttp` 模块的本质是：

- 以 `NetManager` 为 tunnel 入口
- 以 `Tunnel` 为底层连接载体
- 以 `vport` 为上层可见的监听与路由维度
- 对外提供客户端/服务端统一且稳定的 `stream/datagram` 传输接口

这样可以把“建 tunnel、复用 tunnel、从 tunnel 接受 stream/datagram、按端口分发通道”从业务协议中剥离出来，形成可独立演进的公共`Ttp` 层。
