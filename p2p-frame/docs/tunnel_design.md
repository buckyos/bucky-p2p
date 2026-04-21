# Tunnel 重构设计

## 目标

重新定义 `Tunnel`，将它视为两个节点之间的直接连通状态，语义上类似 QUIC 的 `Connection`。

在一个 `Tunnel` 之下，可以：

- 打开双向 `stream` 通道
- 打开单向发送的 `datagram` 通道
- 接收对端发起的 `stream` 和 `datagram` 通道

约束：

- 对于 `remote_id` 已知且可判等的远端设备，允许同时存在多条成功候选 `Tunnel`
- `remote_id` 尚未知、或仍处于 proxy 过渡实现阶段时，允许短暂存在不进入主映射的临时 tunnel/wrapper
- `stream` 和 `datagram` 默认优先复用最近可用且已发布的 tunnel；若没有已发布候选，允许回退到隐藏的 reverse 候选
- `open_stream(vport)` / `open_datagram(vport)` 成功，表示远端已经确认该 `vport` 正在监听，而不仅仅是底层数据连接已建立

Tunnel 的建立形式分为三种：

- `Active`
- `Passive`
- `Proxy`

这里的 `TunnelForm` 只表达 tunnel 的建立路径来源：

- `Active`: 本端主动直连建立
- `Passive`: 本端被动接收入站连接建立
- `Proxy`: 通过 PN/代理链路建立

`TunnelForm` 不表达 channel 类型，也不表达某个具体 channel 是谁先打开的。

反连语义单独由 `Tunnel::is_reverse()` 表达：

- `form = Active, is_reverse = false`：普通主动直连
- `form = Active, is_reverse = true`：本端响应 SN called 主动回拨建立的反连 tunnel
- `form = Passive, is_reverse = false`：对端普通主动连接到本端
- `form = Passive, is_reverse = true`：对端以反连语义连到本端

面向传输层的 `TunnelNetwork` 负责：

- 监听入站 tunnel
- 创建出站主动 tunnel

`TunnelNetwork` 不再直接暴露 stream/datagram 级别的创建接口。

## 主要模型变化

### 旧模型

- `Tunnel` 以 session 为中心
- `TunnelConnection` 承载了大部分真实连接状态
- `StreamManager` 和 `DatagramManager` 各自维护独立的 tunnel 管理逻辑
- 旧的 `P2pNetwork` 同时混合了 listener 接口和 stream/datagram 专用 connect 接口
- 错误 `vport` 主要由上层 manager 在收到入站 channel 后再做路由判断

### 新模型

- `Tunnel` 以 connection 为中心
- `TunnelConnection` 从公共架构中移除
- `StreamManager` 和 `DatagramManager` 共享同一个 `TunnelManager`
- `TunnelNetwork` 只暴露 tunnel 级别的 listen/create 接口
- 同一个远端设备在任意时刻只对应一个主 tunnel
- `Tunnel` 自身负责在 channel 建立阶段校验远端 `vport` 是否可接受

## 核心 Trait

### `Tunnel` 接口

```rust
use crate::endpoint::{Endpoint, Protocol};
use crate::error::P2pResult;
use crate::p2p_identity::P2pId;
use crate::runtime;
use crate::types::{TunnelCandidateId, TunnelId};
use std::pin::Pin;
use std::sync::Arc;

pub type TunnelStreamRead = Pin<Box<dyn runtime::AsyncRead + Send + Unpin + 'static>>;
pub type TunnelStreamWrite = Pin<Box<dyn runtime::AsyncWrite + Send + Unpin + 'static>>;
pub type TunnelDatagramRead = Pin<Box<dyn runtime::AsyncRead + Send + Unpin + 'static>>;
pub type TunnelDatagramWrite = Pin<Box<dyn runtime::AsyncWrite + Send + Unpin + 'static>>;

pub trait ListenVPorts: Send + Sync + 'static {
    fn is_listen(&self, vport: u16) -> bool;
}

pub type ListenVPortsRef = Arc<dyn ListenVPorts>;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum TunnelForm {
    Active,
    Passive,
    Proxy,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum TunnelState {
    Connecting,
    Connected,
    Closed,
    Error,
}

#[async_trait::async_trait]
pub trait Tunnel: Send + Sync + 'static {
    fn tunnel_id(&self) -> TunnelId;
    fn candidate_id(&self) -> TunnelCandidateId;
    fn form(&self) -> TunnelForm;
    fn is_reverse(&self) -> bool;
    fn protocol(&self) -> Protocol;

    fn local_id(&self) -> P2pId;
    fn remote_id(&self) -> P2pId;

    fn local_ep(&self) -> Option<Endpoint>;
    fn remote_ep(&self) -> Option<Endpoint>;

    fn state(&self) -> TunnelState;
    fn is_closed(&self) -> bool;

    async fn close(&self) -> P2pResult<()>;

    async fn listen_stream(&self, vports: ListenVPortsRef) -> P2pResult<()>;
    async fn listen_datagram(&self, vports: ListenVPortsRef) -> P2pResult<()>;

    async fn open_stream(&self, vport: u16) -> P2pResult<(TunnelStreamRead, TunnelStreamWrite)>;
    async fn accept_stream(&self) -> P2pResult<(u16, TunnelStreamRead, TunnelStreamWrite)>;

    async fn open_datagram(&self, vport: u16) -> P2pResult<TunnelDatagramWrite>;
    async fn accept_datagram(&self) -> P2pResult<(u16, TunnelDatagramRead)>;
}
```

设计说明：

- `Tunnel` 是传输层之上唯一可见的连接级抽象
- `tunnel_id()` 暴露本次逻辑建链 ID；同一次 direct fan-out / hedged reverse 共用同一个 `tunnel_id`
- `candidate_id()` 暴露某条具体候选 tunnel 的实例 ID；同一个 `tunnel_id` 下可以同时存在多个不同 `candidate_id`
- `form()` 只表达本地主动/被动/代理方向，`is_reverse()` 单独表达是否属于反连语义
- `close()` 用于 tunnel 替换、空闲清理和显式关闭
- `listen_stream` / `listen_datagram` 用于向 tunnel 注入“远端当前监听端口查询能力”
- `Tunnel` 不直接拥有业务 listener，也不直接依赖 `StreamManager` / `DatagramManager` 具体类型
- `open_stream(vport)` / `open_datagram(vport)` 的成功条件是：远端已经确认该 `vport` 正在监听
- `accept_stream()` / `accept_datagram()` 只应返回已经通过 tunnel 内部 `vport` 校验的 channel
- 当前阶段 `datagram` 仍被建模为单向 channel：主动打开返回 `AsyncWrite`，被动接收返回一个单次入站可读单元
- 这里的 `datagram` 不是严格 message-bounded 的 UDP 式对象；如果后续需要真正的消息边界，需要单独定义新的 send/recv trait
- `vport` 属于 channel 打开/接收层，而不属于 tunnel 本身

### 监听端口注册表

`StreamManager` 和 `DatagramManager` 不直接实现 `ListenVPorts`。它们分别持有一个内部注册表，用于：

- 管理本地真实 listener 对象
- 对 tunnel 暴露统一的 `ListenVPortsRef`

建议的内部结构：

```rust
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

pub struct ListenVPortRegistry<L> {
    listeners: RwLock<HashMap<u16, Arc<L>>>,
}

impl<L> ListenVPortRegistry<L> {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            listeners: RwLock::new(HashMap::new()),
        })
    }

    pub fn as_listen_vports_ref(self: &Arc<Self>) -> ListenVPortsRef
    where
        L: Send + Sync + 'static,
    {
        self.clone()
    }

    pub fn contains(&self, vport: u16) -> bool {
        self.listeners.read().unwrap().contains_key(&vport)
    }

    pub fn insert(&self, vport: u16, listener: Arc<L>) -> Option<Arc<L>> {
        self.listeners.write().unwrap().insert(vport, listener)
    }

    pub fn get(&self, vport: u16) -> Option<Arc<L>> {
        self.listeners.read().unwrap().get(&vport).cloned()
    }

    pub fn remove(&self, vport: u16) -> Option<Arc<L>> {
        self.listeners.write().unwrap().remove(&vport)
    }
}

impl<L> ListenVPorts for ListenVPortRegistry<L>
where
    L: Send + Sync + 'static,
{
    fn is_listen(&self, vport: u16) -> bool {
        self.contains(vport)
    }
}
```

设计说明：

- `Tunnel` 只看到 `ListenVPorts`，不感知 manager 细节
- manager 仍可通过同一个 registry 直接取得真实 listener 对象并投递 channel
- 不要求把端口列表复制到每个 tunnel 内部；tunnel 只保留 `ListenVPortsRef`，实时查询 `is_listen(vport)`

### TunnelListener 与 TunnelNetwork

```rust
use crate::endpoint::{Endpoint, Protocol};
use crate::error::P2pResult;
use crate::p2p_identity::{P2pId, P2pIdentityRef};
use std::sync::Arc;

pub type TunnelRef = Arc<dyn Tunnel>;

#[async_trait::async_trait]
pub trait TunnelListener: Send + Sync + 'static {
    async fn accept_tunnel(&self) -> P2pResult<TunnelRef>;
}

pub type TunnelListenerRef = Arc<dyn TunnelListener>;

#[async_trait::async_trait]
pub trait TunnelNetwork: Send + Sync + 'static {
    fn protocol(&self) -> Protocol;
    fn is_udp(&self) -> bool;

    async fn listen(
        &self,
        local: &Endpoint,
        out: Option<Endpoint>,
        mapping_port: Option<u16>,
    ) -> P2pResult<TunnelListenerRef>;

    async fn close_all_listener(&self) -> P2pResult<()>;
    fn listeners(&self) -> Vec<TunnelListenerRef>;

    async fn create_tunnel(
        &self,
        local_identity: &P2pIdentityRef,
        remote: &Endpoint,
        remote_id: &P2pId,
        remote_name: Option<String>,
    ) -> P2pResult<TunnelRef>;

    async fn create_tunnel_with_intent(
        &self,
        local_identity: &P2pIdentityRef,
        remote: &Endpoint,
        remote_id: &P2pId,
        remote_name: Option<String>,
        intent: TunnelConnectIntent,
    ) -> P2pResult<TunnelRef>;

    async fn create_tunnel_with_local_ep(
        &self,
        local_identity: &P2pIdentityRef,
        local_ep: &Endpoint,
        remote: &Endpoint,
        remote_id: &P2pId,
        remote_name: Option<String>,
    ) -> P2pResult<TunnelRef>;

    async fn create_tunnel_with_local_ep_and_intent(
        &self,
        local_identity: &P2pIdentityRef,
        local_ep: &Endpoint,
        remote: &Endpoint,
        remote_id: &P2pId,
        remote_name: Option<String>,
        intent: TunnelConnectIntent,
    ) -> P2pResult<TunnelRef>;
}
```

设计说明：

- `listen` 返回 listener 对象，而不是事件回调
- `listen` 只绑定监听端口/endpoint，不绑定某个特定身份；同一个 listener/监听端口可以同时服务多个本地身份
- 本地身份由更上层先注册到共享 `ServerCertResolver`，入站握手时再根据 SNI/证书名解析出本次 tunnel 对应的 `local_id`
- `accept_tunnel` 与 `accept_stream` 统一采用阻塞式/异步 accept 风格
- 出站创建只保留 tunnel 级别语义
- reverse/proxy 不由 `TunnelNetwork` 直接创建，而是由更上层协调逻辑产出对应 tunnel form
- 由更上层创建出来的 reverse/proxy tunnel 仍然必须统一注册到 `TunnelManager`
- `TunnelListener` 不承担关闭、端口映射、本地 endpoint 查询等管理语义
- `TunnelConnectIntent` 由上层显式传入逻辑 `tunnel_id`、具体 `candidate_id` 和 `is_reverse`，transport 负责把这些信息随建链 `hello` 发给对端

### `TunnelManager`

```rust
use crate::endpoint::Endpoint;
use crate::error::P2pResult;
use crate::p2p_identity::{P2pId, P2pIdentityCertRef};
use std::sync::Arc;

pub type TunnelRef = Arc<dyn Tunnel>;

#[async_trait::async_trait]
pub trait TunnelSubscription: Send + Sync + 'static {
    async fn accept_tunnel(&self) -> P2pResult<TunnelRef>;
}

pub type TunnelSubscriptionRef = Arc<dyn TunnelSubscription>;

#[async_trait::async_trait]
pub trait TunnelManager: Send + Sync + 'static {
    fn subscribe(&self) -> TunnelSubscriptionRef;

    async fn open_tunnel(&self, remote: &P2pIdentityCertRef) -> P2pResult<TunnelRef>;

    async fn open_tunnel_from_id(&self, remote_id: &P2pId) -> P2pResult<TunnelRef>;

    async fn open_direct_tunnel(
        &self,
        remote_eps: Vec<Endpoint>,
        remote_id: Option<P2pId>,
    ) -> P2pResult<TunnelRef>;
}
```

设计说明：

- `TunnelManager` 对外核心语义是 `subscribe` + 按不同上下文打开 tunnel
- `TunnelManager` 对外按不同打开语义拆成多个函数，而不是把不同参数合并成一个枚举
- `open_tunnel`、`open_tunnel_from_id`、`open_direct_tunnel` 内部统一封装 tunnel 复用、直连、反连、代理连接等能力
- `TunnelManager` 不直接分发 `stream` 或 `datagram`
- `subscribe()` 在建立后应先回放当前已经可用的 tunnel，再继续接收后续新增或替换后的 tunnel
- 所有新变为可用的 tunnel 都应该广播给订阅者，包括入站 tunnel、新创建的出站 tunnel、以及替换旧主 tunnel 的新 tunnel
- `TunnelManager` 需要后台定时清理长期空闲的 tunnel；“空闲”定义为一段时间内没有活跃 channel，且没有新的 open/accept 活动
- 同一个后台 housekeeping 循环还需要负责“proxy 已连通后的脱代理升级”调度：当某个远端当前只有 proxy tunnel 可用时，继续周期性尝试 direct 或 reverse
- 一次逻辑建链（多 endpoint 并发、direct+reverse hedged）必须共享同一个 `tunnel_id`
- 同一次逻辑建链里的每条具体候选 tunnel 必须拥有不同的 `candidate_id`
- 反连 waiter 需要按 `(remote_id, tunnel_id)` 匹配，不能只按 `remote_id`
- `reverse` 候选是否 publish，不由 `is_reverse` 单独决定，而由本地是否仍持有对应 `(remote_id, tunnel_id)` 的 `reverse_waiter` 决定
- 脱代理升级的目标是把“当前仅依赖 proxy 的可用连接”切换回 direct/reverse；升级路径本身不得再次把 proxy 视为成功结果
- 持续失败的脱代理升级允许采用指数退避，但需要显式上限；当前约束为初始 5 分钟、失败后退避增长、最大不超过 2 小时

## Tunnel 候选与发布规则

- 对同一个 `remote_id`，`TunnelManager` 允许同时保存多条健康 tunnel 候选
- 非 `reverse` 候选只要建链成功就保留，并 publish 给订阅者
- 本地仍持有 `reverse_waiter` 时，命中的 `reverse` 候选先保留但不 publish
- 对同一个 `(remote_id, tunnel_id)`，一旦本地已经没有 waiter，后续成功的 `reverse` 候选也会正常 publish
- `TunnelManager::get_tunnel(remote_id)` 默认优先返回已 publish 的健康候选；若没有已 publish 候选，再回退到隐藏的 reverse 候选
- 候选是否被长期保留，不由建链阶段 winner 选择决定，而由后续实际使用和空闲清理自然收敛
- proxy tunnel 允许作为“立即可用”的兜底连通性被 publish，但对于已知 `remote_id` 的远端，不应成为长期唯一状态；manager 需要为其登记后续脱代理尝试
- 一旦 direct/reverse 脱代理成功，新得到的非 proxy tunnel 应像其他健康候选一样进入正常 publish/复用路径，并清除对应远端的 proxy 升级状态

## StreamManager 和 DatagramManager 如何感知新的 Channel

### 问题

如果某个 tunnel 已经存在，而远端节点在这个 tunnel 上新开了 `stream` 或发来了新的 `datagram`，那么 `StreamManager` 和 `DatagramManager` 必须能够感知到这些新 channel。

### 方案

不要让 `TunnelManager` 直接分发 stream/datagram 事件。

而是采用下面的方式：

1. `TunnelManager` 暴露 tunnel 订阅机制
2. `StreamManager` 订阅所有已存在和后续新出现的 tunnel
3. `DatagramManager` 也订阅所有已存在和后续新出现的 tunnel
4. 每个 manager 在自己收到的每个 tunnel 上运行独立的 accept loop
5. 在启动 accept loop 前，manager 先向 tunnel 注入对应的 `ListenVPortsRef`

### 流程

#### 入站 tunnel 流程

1. `TunnelNetwork::listen(...)` 创建一个 `TunnelListener`
2. accept loop 调用 `listener.accept_tunnel().await`
3. 接收到的 tunnel 交给 `TunnelManager` 内部注册逻辑
4. `TunnelManager` 按候选保存/发布规则决定是否保留并广播该 tunnel
5. `StreamManager` 收到 tunnel 后，先调用 `tunnel.listen_stream(stream_registry.as_listen_vports_ref()).await`
6. `DatagramManager` 收到 tunnel 后，先调用 `tunnel.listen_datagram(datagram_registry.as_listen_vports_ref()).await`
7. 两个 manager 分别启动自己的 `accept_stream()` / `accept_datagram()` 循环

#### 出站 tunnel 流程

1. `StreamManager` 或 `DatagramManager` 根据场景调用 `TunnelManager::open_tunnel(...)`、`TunnelManager::open_tunnel_from_id(...)` 或 `TunnelManager::open_direct_tunnel(...)`
2. `TunnelManager` 先检查该远端是否已有 tunnel；如果有则直接复用，否则根据需要执行 active、reverse、proxy 等建立流程
3. 当新建 tunnel 成功后，`TunnelManager` 会将其注册为候选；非 reverse 候选会广播，reverse 候选仅在本地仍存在对应 waiter 时暂不广播
4. 两个 manager 都开始向这个 tunnel 注入自己的 `ListenVPortsRef`，并监听该 tunnel 上后续来自远端的入站 channel

### 为什么不让 TunnelManager 直接分发 Channel

如果这么做，`TunnelManager` 就必须理解：

- `stream` 和 `datagram` 的区别
- `vport`
- listener 路由
- channel 投递语义

这样会让它重新退化成 session dispatcher，并重新引入当前代码中的耦合问题。

保持 `TunnelManager` 只处理 tunnel，才能更清晰地维持职责边界。

## 监听注入与竞态

由于 tunnel 可能先建立完成，而 `StreamManager` / `DatagramManager` 稍后才完成 `listen_*` 注入，因此协议需要显式处理 provider 尚未注入的窗口。

建议：

- 每个 tunnel 分别维护 `stream` / `datagram` 两套 `ListenVPortsRef`
- `listen_stream` / `listen_datagram` 被调用前，channel open 请求可以短暂等待 provider 就绪
- provider 长时间未注入时，按 tunnel 级超时失败
- provider 注入后，tunnel 通过 `is_listen(vport)` 实时查询，不缓存端口快照

这样可以避免“远端刚建好 tunnel 就马上发起 `open_stream(vport)`，而本端还没来得及注入 `ListenVPortsRef`”时发生误判。

## 推荐的 Manager 职责划分

### `TunnelManager`

- 对外提供 `subscribe`、`open_tunnel`、`open_tunnel_from_id`、`open_direct_tunnel`
- 内部按 `remote_id -> tunnel_id -> candidate_id` 维护候选 tunnel 映射
- 每个远端设备允许同时存在多个健康候选 tunnel
- 统一处理 active/reverse/proxy tunnel 建立路径
- 对新订阅者先回放当前可用 tunnel，再持续推送后续新 tunnel
- 将每一个新可用的非 reverse tunnel 发布给订阅者
- 定时清理长期空闲、closed、error 的 tunnel

### `StreamManager`

- 维护 `ListenVPortRegistry<StreamListener>`
- 订阅 tunnel 更新
- 在每个 tunnel 上先执行 `listen_stream(...)`
- 在每个 tunnel 上运行 `accept_stream()`
- 将 `(vport, read, write)` 路由给匹配的 stream listener
- 若兜底阶段仍发现 `vport` 没有 listener，立即关闭/释放该 channel，而不是缓存或静默保留
- 出站打开时根据场景调用 `open_tunnel(...)`、`open_tunnel_from_id(...)` 或 `open_direct_tunnel(...)`，然后执行 `.open_stream(vport)`

### `DatagramManager`

- 维护 `ListenVPortRegistry<DatagramListener>`
- 订阅 tunnel 更新
- 在每个 tunnel 上先执行 `listen_datagram(...)`
- 在每个 tunnel 上运行 `accept_datagram()`
- 将 `(vport, read)` 路由给匹配的 datagram listener
- 若兜底阶段仍发现 `vport` 没有 listener，立即关闭/释放该 channel，而不是缓存或静默保留
- 出站发送时根据场景调用 `open_tunnel(...)`、`open_tunnel_from_id(...)` 或 `open_direct_tunnel(...)`，然后执行 `.open_datagram(vport)`

## 统一错误语义

对外统一约定：

- 错误 `vport`：`P2pErrorCode::PortNotListen`
- provider 长时间未注入：`P2pErrorCode::Timeout`
- tunnel/控制面中断：`P2pErrorCode::Interrupted`

实现细节上允许不同 transport 保留自己的细分原因：

- TCP: `ClaimConnNackReason::VportNotFound`
- QUIC: `OpenChannelResp.result`
- Proxy: 轻量 `result` / `nack` 码

但这些细分原因向上层 API 暴露时，应统一映射成稳定的 `P2pErrorCode`。

## 协议方向

架构上需要明确拆分这两个概念：

- tunnel 建立形式：`Active | Passive | Proxy`
- tunnel 建立形式：`Active | Passive | Proxy`
- channel 类型：`Stream | Datagram`

当前的 `TunnelType` 不应该继续同时表达连接语义和 channel 语义。

迁移期间需要明确兼容策略：

- 线上的 `protocol::v0::TunnelType` 仍可能保留 `Stream | Datagram` 编码以兼容旧包格式
- 但在新架构语义中，它只应被视为过渡字段，而不是新的核心抽象
- reverse/SN call 需要逐步迁移为“请求建立 tunnel”而不是“请求建立某种 channel”
- 直连和反连共用同一个逻辑 `tunnel_id`；transport 建链 `hello` 必须显式携带 `tunnel_id`、`candidate_id` 与 `is_reverse`

在新接口下，还需要明确一个额外约束：

- `open_stream` / `open_datagram` 必须包含远端确认阶段
- 不允许仅建立底层数据连接便直接向上返回成功
- `accept_stream` / `accept_datagram` 不承担首个 `vport` 错误反馈职责，首个拒绝点应位于 tunnel 内部握手阶段

对于不同 transport：

- TCP 复用已有 control claim/ack/nack 机制，只需把 `vport` 校验真正接入 `Tunnel`
- QUIC 需要额外引入 tunnel 内部控制通道，解决 `stream` / `datagram` 的 open ack 问题
- Proxy 需要在 channel open 时引入轻量 request/ack

## 分阶段迁移计划

### 阶段 1

- 定义 `ListenVPorts`、`ListenVPortRegistry` 和新的 `Tunnel` trait
- 更新设计文档
- 保持旧实现不动，先建立统一语义

### 阶段 2

- 让 `StreamManager` 和 `DatagramManager` 切换到 `ListenVPortRegistry`
- 在收到新 tunnel 后先注入 `listen_stream(...)` / `listen_datagram(...)`
- 保持现有 accept loop 兜底逻辑，但不再依赖它做主路径错误反馈

### 阶段 3

- 接通 `TcpTunnel` 的 `listen_stream` / `listen_datagram`
- 将 `ClaimConnNackReason::VportNotFound` 真实接入处理链路
- 向上层统一映射为 `P2pErrorCode::PortNotListen`

### 阶段 4

- 实现 `QuicTunnel` 控制通道设计
- 为 `open_stream` / `open_datagram` 增加远端确认语义
- 将 QUIC 的入站 channel 先在 tunnel 内部完成 `vport` 校验，再进入 accept 队列

### 阶段 5

- 改造 proxy 创建流程，让它在 channel open 时携带显式 ack/nack
- 在必要的兼容阶段，允许 proxy 仍以内部分 channel wrapper 形式存在，但对外继续收敛到 tunnel 级抽象

### 阶段 6

- 替换旧的 session-centric tunnel 路径
- 删除 `TunnelConnection`
- 清理旧协议类型和兼容过渡代码
- 为 reverse/SN call 引入独立的 tunnel 建立语义，去掉旧 `TunnelType` 对 channel 类型的核心表达职责

## 实施建议

- 优先先落 `ListenVPorts` 和统一错误语义，再分别推进 TCP、QUIC、Proxy
- `vport` 保持在 channel 层
- 不要让 `TunnelManager` 感知业务 listener
- 入站和出站 tunnel 在注册之后一视同仁
- manager 先订阅 tunnel，再各自在 tunnel 上 accept 自己关心的 channel
- 对没有监听者的 `vport`，应在 tunnel 建立 channel 的握手阶段直接拒绝
- 明确 proxy endpoint 可能未知，`local_ep/remote_ep` 允许为 `None`
- `TunnelManager` 内部要有周期性空闲清理任务，避免远端 tunnel 映射无限增长
