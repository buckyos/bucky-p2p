# PN Proxy TLS-over-Proxy 设计补充

本补充文档聚焦于 `pn/client` 中的 proxy tunnel `stream` 可选 TLS 载荷加密设计。它与 [docs/versions/v0.1/modules/p2p-frame/design/pn-server.md](/mnt/f/work/p2p/docs/versions/v0.1/modules/p2p-frame/design/pn-server.md) 配套：端点侧负责显式选择是否在已建立的 proxy stream 上再叠一层 TLS，TLS 模式由两端在 tunnel 外预先约定，relay 侧只负责桥接 TLS 握手字节和后续密文，并继续执行统计/限速。

## 范围

### 范围内
- 为 proxy tunnel `stream` 定义显式的“明文 / TLS-over-proxy”选择接口
- 保持通用 `TunnelNetwork` / `Tunnel` trait 兼容的前提下，为 `PnClient` / `PnTunnel` 增加 PN 专有安全接口
- 定义由 tunnel 外约定 TLS 模式时，`PnClient` / `PnTunnel` 如何在本地执行一致的 stream 包装
- 定义 active/passive 两侧在 proxy stream 建立成功后如何叠加 TLS client/server 握手
- 约束“调用方未开启 TLS”和“显式开启但 TLS 建立失败”这两条路径的行为
- 明确同一 `PnTunnel` 上的 `datagram` channel 不承载 TLS 语义，并在 `TlsRequired` 下继续保持明文兼容

### 范围外
- 修改非 `pn` tunnel 的 open/listen trait 契约
- 把是否启用 TLS 做成全局隐式开关
- 让 relay 成为 TLS 终止点或 payload 解密点
- 为 proxy `datagram` 在本轮同时引入 TLS 等价的保密语义

## 目标与原则

- 加密是可选能力，由调用方显式选择。
- 是否启用 TLS 由 proxy tunnel 两端在 tunnel 外预先约定。
- 兼容路径必须稳定：现有 `TunnelNetwork::create_tunnel*` 与 `Tunnel::open_stream()` 调用点不需要立刻改造。
- 一旦调用方显式选择 TLS，TLS 建立失败就直接失败，不允许静默降级到明文。
- relay 仍可做 admission、计量和限速，但在 TLS 模式下默认只看到控制元数据、TLS 握手字节和密文长度/时序。

## 接口落点

### 保持兼容的通用接口

- `TunnelNetwork::create_tunnel*` 继续返回通用 `TunnelRef`，默认创建“明文模式”的 `PnTunnel`。
- `Tunnel::open_stream()` 在 `PnTunnel` 上继续保持当前语义，不新增额外参数。
- `Tunnel::open_datagram()` / `accept_datagram()` 继续保持当前明文行为，本轮不提供 TLS 版本。

### 新增的 PN 专有接口

- `PnClient` 新增显式构造入口，用于创建带安全策略的 active proxy tunnel。
- `PnClient` 额外暴露显式的 client 级安全模式配置，用于影响同一 `PnClient` 后续通过通用 trait 创建的 active tunnel，以及该 client 上 listener 被动接受到的 tunnel。
- `PnTunnel` 内部持有创建或接受当时固定下来的安全策略，并为诊断提供只读查询接口。

建议的数据结构如下：

```rust
pub enum PnProxyStreamSecurityMode {
    Disabled,
    TlsRequired,
}

pub struct PnTunnelOptions {
    pub stream_security_mode: PnProxyStreamSecurityMode,
}

impl Default for PnTunnelOptions {
    fn default() -> Self {
        Self {
            stream_security_mode: PnProxyStreamSecurityMode::Disabled,
        }
    }
}
```

建议的新增入口如下：

```rust
impl PnClient {
    pub async fn create_tunnel_with_options(
        &self,
        local_identity: &P2pIdentityRef,
        remote: &Endpoint,
        remote_id: &P2pId,
        remote_name: Option<String>,
        intent: TunnelConnectIntent,
        options: PnTunnelOptions,
    ) -> P2pResult<Arc<PnTunnel>>;

    pub fn set_stream_security_mode(&self, mode: PnProxyStreamSecurityMode);
    pub fn stream_security_mode(&self) -> PnProxyStreamSecurityMode;
}

impl PnTunnel {
    pub fn stream_security_mode(&self) -> PnProxyStreamSecurityMode;
}
```

当前实现提供两条显式控制路径：

- 调用方可通过 `create_tunnel_with_options(...)` 为单条 active tunnel 选择 `stream_security_mode`。
- 调用方也可通过 `set_stream_security_mode(...)` 为某个 `PnClient` 设置 client 级默认模式；`P2pStackConfig::set_proxy_stream_encrypted(true)` 创建默认 proxy client 时，会走这条路径。

该设计把“是否启用 TLS”的显式控制限定在 PN 专有 API 或显式 stack 配置，而不是扩展整个 `TunnelNetwork` / `Tunnel` trait。这样通用 tunnel 管理器与相邻模块仍保持既有签名；需要 proxy payload 保密性的调用点可以显式选择单 tunnel 模式，也可以显式配置整个 `PnClient` / stack 的默认行为。

## 外部约定模型

- `ProxyOpenReq` / `ProxyOpenResp` 保持当前 open-result 语义，不额外承载 TLS 模式字段。
- 是否启用 TLS 由 tunnel 外的配置、调用约定或双方共享构造参数决定。
- active/passive 两侧必须通过同一套外部策略得到一致的 `PnProxyStreamSecurityMode`，以约束 proxy `stream` 的行为。
- 接收侧不会从 `ProxyOpenReq` 中协商 TLS 模式；入站 channel 会投递到同一 `(remote_id, tunnel_id)` 的已有 `PnTunnel`，包括本端主动创建的 active tunnel 和此前被动接受的 passive tunnel，并继续使用该 tunnel 已固化的 TLS 模式。若必须创建新的 passive tunnel，`PnListener.accept_tunnel()` 会把 listener 所属 `PnClient` 在创建该逻辑 tunnel 时的 `stream_security_mode` 快照固化到返回的 `PnTunnel` 中。
- 明确双向 open 语义：`A` 建立 `PnTunnel(A->B)` 后，`B` 使用对应逻辑 tunnel 反向 `open_stream()` 时，`A` 端应在原来的 active `PnTunnel(A->B)` 上通过 `accept_stream()` 接收该 stream；该路径不得要求 `A` 再通过 `accept_tunnel()` 获取新的 passive tunnel。
- 若一侧按明文处理、另一侧按 TLS 处理，结果会在 TLS 握手或后续 I/O 上直接失败；系统不提供额外的协议级协商或明文回退。

## TLS 叠加时序

### Active 侧

1. `PnTunnel.open_stream()` 按既有逻辑完成 `ProxyOpenReq` / `ProxyOpenResp` 握手。
2. 若 `stream_security_mode == Disabled`，直接返回底层 proxy stream。
3. 若 `stream_security_mode == TlsRequired`，则在成功得到底层 proxy stream 后，立即以 TLS client 身份在该 stream 上发起握手。
4. 只有 TLS client 握手成功，才向调用方返回可用 stream。

### Passive 侧

1. `PnListener.accept_tunnel()` 继续返回 `PnTunnel`，不新增额外 listener trait 参数；如果没有同一 `(from, tunnel_id)` 的已有 tunnel，返回对象持有的是 listener 所属 `PnClient` 在创建该逻辑 tunnel 时的 TLS 模式快照。
2. 同一 `(from, tunnel_id)` 的后续 `ProxyOpenReq` 不再次创建 `PnTunnel`，而是进入已有 tunnel 的待接收队列；该已有 tunnel 可以是本端主动创建的 active tunnel，也可以是此前被动创建的 passive tunnel。
3. 当已有 tunnel 是本端主动创建的 active tunnel 时，远端反向 `open_stream()` 仍表现为该 active tunnel 上的 `accept_stream()` 事件。
4. `PnTunnel.accept_stream()` 从 stream 队列取出下一条 `ProxyOpenReq`，在返回成功的 `ProxyOpenResp` 后，根据本地 `stream_security_mode` 决定是否进入 TLS server 握手。
5. 对于 `Disabled`，按当前语义直接返回底层 stream。
6. 对于 `TlsRequired`，在同一条 proxy stream 上等待并完成 TLS server 握手。
7. 只有 TLS server 握手成功，才向调用方返回可用 stream。

因此，TLS 建立发生在 proxy stream 已建成之后，而不是替换 PN open 控制流本身。

## TLS 身份与校验

- active 侧必须把 TLS server 端身份校验绑定到预期的 `remote_id`，不能只验证“这是某个合法证书”。
- passive 侧继续使用本节点已有的身份材料作为 TLS server 证书来源。
- 优先复用现有 `p2p-frame/src/tls/**`、`P2pIdentity` 和对应 verifier / signer 组件，不额外设计一套新的 proxy 专用密码学协议。
- relay 不参与证书签发、证书选择或握手校验，只转发字节。

## relay 可见边界

- relay 不感知 TLS 模式开关本身，也不负责比较两端配置是否一致。
- 一旦两端在 open 成功后进入 TLS 握手，relay 看到的是 TLS ClientHello、ServerHello 和后续 TLS record 密文。
- relay 仍然可以看到 `from`、`to`、`kind`、`purpose` 以及密文长度/时序，但看不到业务明文。

## datagram 处理

- 本轮不为 `datagram` 提供 TLS-over-proxy 语义。
- `PnTunnelOptions.stream_security_mode` 只约束 `stream` channel；`Tunnel::open_datagram()` / `accept_datagram()` 必须忽略该模式并继续返回当前明文 datagram 行为。
- 若同一 tunnel 选择了 `TlsRequired`，`datagram` 仍然走既有 PN datagram open/accept 控制流，不做 TLS 握手，也不因为该模式返回 `NotSupport`。
- 若未来需要 datagram 保密语义，应通过新的 proposal 单独设计。

## 兼容性与默认行为

- 未显式配置的 `PnClient` 上，现有只使用通用 `TunnelNetwork` / `Tunnel` trait 的调用点，默认保持明文 proxy tunnel。
- 显式调用 `create_tunnel_with_options(...)` 并选择 `TlsRequired` 时，会在单条 active tunnel 成功建链后进入 TLS 握手。
- 显式调用 `set_stream_security_mode(...)`，或通过 `P2pStackConfig::set_proxy_stream_encrypted(true)` 构造默认 proxy client 后，该 `PnClient` 后续通过通用 `create_tunnel_with_intent(...)` 创建的 proxy tunnel，以及被动接受到的 `PnTunnel`，都会继承该模式快照。
- 即使显式调用 PN 专有 API 并选择 `TlsRequired`，同一 tunnel 上的 `datagram` 调用点仍保持当前明文兼容语义。
- 由于安全策略绑定在 `PnTunnel` 创建时，现有 `TunnelManager`、`NetManager` 和泛型调用方不需要同步扩展 trait 签名。

## 失败模型

- 调用方选择 `Disabled`：不触发 TLS 握手，行为与当前实现一致。
- 调用方选择 `TlsRequired` 但远端按明文处理或未准备好执行 TLS：当前 channel 在 TLS 握手或后续 I/O 上失败，不建立可用 stream。
- `ProxyOpenResp` 成功但后续 TLS 握手失败：当前 channel 失败并关闭，不向调用方暴露明文底层 stream。
- 已建立 TLS-over-proxy stream 在后续读写中出现 TLS 告警或证书校验失败：按 TLS 流语义关闭当前 channel。
- `datagram` 调用点无论 `stream_security_mode` 为何，都不因 TLS 模式本身新增失败路径；其失败语义继续由既有 datagram open/accept 条件决定。

## 建议实现布局

| 路径 | 职责 |
|------|------|
| `p2p-frame/src/pn/client/pn_client.rs` | 新增显式 `create_tunnel_with_options` 入口 |
| `p2p-frame/src/pn/client/pn_tunnel.rs` | 保存 tunnel 安全策略，并在 open/accept 成功后装配 TLS client/server wrapper |
| `p2p-frame/src/tls/**` | 复用现有 TLS provider、证书解析、签名和 verifier 逻辑 |

## 风险

- 若 TLS server 身份没有绑定到预期 `remote_id`，relay 仍可能插入中间 TLS 终止点。
- 若 passive 侧在 TLS server 握手完成前就提前暴露底层 stream，可能形成明文窗口。
- 当前实现同时支持单 tunnel 显式输入和 client 级显式默认模式；若调用方把 client 级模式当成“一次性只影响某条 tunnel 的临时开关”，会把后续通过同一 `PnClient` 创建或接受的 tunnel 一并切到该模式。
- 若两端没有共享同一份外部策略，而是一端启用 TLS、另一端保持明文，错误会推迟到 TLS 握手阶段暴露。
- datagram 继续维持明文兼容，意味着本轮保密能力只覆盖 proxy stream；若实现上错误复用 `stream` 模式判断，会把现有 datagram 调用点回归成非预期拒绝。
