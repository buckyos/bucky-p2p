---
module: p2p-frame
version: v0.1
status: approved
approved_by: auto-pipeline
approved_at: 2026-06-25T12:06:32+08:00
approved_content_sha256: 5be0bb9a38c4f18a169cca5cc1d1a8d17e7ca3319697fcfdf017919f64e7d3d2
---

# p2p-frame 设计

> 该数据包为治理现有核心库定义设计基线。现有协议说明继续作为参考文档存在，并在此处建立索引。

## Design Scope
### 目标
- 让核心库边界足够明确，使未来工作可以按子模块拆分。
- 复用现有协议说明，而不是重复编写设计内容。
- 为 planning、testing 和 acceptance 定义具备阶段可执行性的子模块责任归属。
- 为本轮 `pn/service/pn_server.rs` 的用户流量统计与限速需求建立可执行的设计边界，并把 `sfo-io` 接入限定在 relay bridge 路径内，包括 source/target 双边独立统计视图与 source 单边限速的区分。
- 为本轮 proxy tunnel `stream` 路径的“可选 TLS-over-proxy 载荷加密、由使用者显式接口控制且由两端在 tunnel 外约定”需求建立可执行设计边界，并明确 `datagram` 路径继续保持明文兼容且忽略该 `stream` 加密模式，同时把加密接口限制在 PN 专有 API 内，而不是污染通用 `Tunnel` / `TunnelNetwork` trait。
- 为本轮 `PnTunnel` idle timeout 生命周期关闭需求建立可执行设计边界，明确 channel lease 计数、原子关闭、accept/open 失败和关闭后重新创建语义。
- 为本轮 `PnTunnel` 控制通道需求建立可执行设计边界：logical tunnel 打开时建立独立控制 channel，建立成功后才返回可用 tunnel，并在控制面断开、读写失败或对端 close 时关闭本地 tunnel。
- 为本轮 `tunnel/TunnelManager` 中“当远端当前通过 proxy tunnel 连通时，后台周期性重试 direct/reverse 升级并限制失败退避上限”的行为建立设计边界，避免连接长期粘连在代理路径上。
- 为本轮 `tunnel/TunnelManager` 中“新建 tunnel 的统一 register/publish 生命周期，以及 reverse tunnel 只延后 publish 时机而不改变 publish 规则”的行为建立可执行设计边界，收敛当前散落在多处的发布决策。
- 为本轮 `tunnel/TunnelManager` 中“reverse incoming 无同 `(remote_id, tunnel_id)` waiter 时直接关闭且不发布”的行为建立可执行设计边界，避免本地未等待的 reverse tunnel 通过订阅路径变成可用候选。
- 为本轮单 SN NAT 打洞优化建立可执行设计边界，包括 direct/reverse 统一 300ms 短延迟竞速、`SnCall` 本次反连候选、QUIC listener 同源 UDP punch burst、proxy 短窗口脱代理升级，以及按协议拆分的 endpoint 评分。
- 为本轮 endpoint area 语义变更建立可执行设计边界：`EndpointArea::Default` 重命名为 `ServerReflexive`，`Display`/`FromStr` 和 raw codec 使用 `S` 编码，SN 观察地址只有与节点自上报地址一致时才标记为 `Wan`，否则标记为 `ServerReflexive`，并移除 `is_sys_default()` 的公开判定入口。
- 为本轮 `ServerReflexive` QUIC NAT keepalive 需求建立可执行设计边界：UDP punch 只对 `EndpointArea::ServerReflexive` QUIC candidate 开启，QUIC tunnel 控制心跳发送间隔保持现有值，heartbeat timeout 调整为 30 秒。
- 为本轮 networks 基于 `sfo-reuseport` 的 listener 重构建立可执行设计边界：TCP listener 直接使用 `sfo_reuseport::TcpServer`，QUIC listener 直接使用 `sfo_reuseport::QuicServer::serve_socket(...)`、每 worker 一个 Quinn endpoint、`sfo_reuseport::QuicCidGenerator` worker shard 和 `sfo_reuseport::UdpSocket` Quinn helper 接口，外部可显式设置 `ServerRuntime`，且不新增 `NetworkServerRuntime`。
- 为本轮 `TunnelNetwork` listener 暴露模型变更建立可执行设计边界：`listen(...)` 接收入站 tunnel 回调并返回 `P2pResult<()>`，不再返回 `TunnelListenerRef`，公共 trait 移除 `listeners()`，`NetManager` 负责在回调中执行原有 incoming validator、订阅发布和 reject close 逻辑。
- 为本轮 `Tunnel` stream/datagram 入站 channel 暴露模型变更建立可执行设计边界：`listen_stream(...)` / `listen_datagram(...)` 接收入站 channel 回调，公共 trait 移除 `accept_stream()` / `accept_datagram()`，TCP/QUIC/PN tunnel 内部在入站处理路径中按 listen 规则触发回调，TTP、stream manager、datagram manager 和 PN server/client 不再通过公共 accept loop 消费 channel。
- 为本轮 `Tunnel` control stream API 建立可执行设计边界：公共 trait 只新增 `open_control_stream(...)` / `listen_control_stream(...)` 和 callback/stream 类型；内部 `control_stream` runtime 作为 `networks` 私有共享模块，通过现有 TCP/QUIC/PN 控制命令新增的单一 `Data` 命令承载内部多路复用 frame，单个 `Data` payload 最大 `64 KiB`，底层控制通道断开时关闭所有派生 control stream。
- 为本轮 SN control stream 信令建立可执行设计边界：SN report、call、called、response 或等价低频小消息通过公开 `Tunnel::open_control_stream(...)` / `listen_control_stream(...)` 交互；SN 不调用内部 `control_stream` runtime/frame，不把该路径扩展为大流量数据平面；控制通道不可用、远端未监听 SN purpose 或旧版本不支持时显式失败，不保留普通业务 stream fallback。
- 为本轮 `TtpNode` 主动建链需求建立可执行设计边界：新增 `TtpNode` 作为同时实现 `TtpPortListener` 和 `TtpConnector` 的 TTP 组合入口，复用 `TtpClient` 的 target tunnel 查找/主动建链/attach 逻辑与 `TtpServer` 的入站 tunnel subscriber 逻辑；`open_stream(...)` / `open_control_stream(...)` 在无匹配可用 tunnel 时主动建链，`open_datagram(...)` 第一版与 `TtpClient` 保持一致，也使用同一 `get_or_create_tunnel(...)` 路径。
- 为本轮 `TtpClient` 连接生命周期需求建立可执行设计边界：新增 `remove_server(...)` 从保持连接 target 集合中幂等删除指定 target；maintain loop 每轮从当前集合取快照，删除后的 target 不再被自动重建；`TtpClient` 为非保持 target 的 cached tunnel 记录最后使用时间和 active/pending lease 计数，默认 5 分钟 idle 后仅释放本地 cache 引用，保持 target 和仍有 active/pending channel 的 tunnel 不参与释放。
- 为本轮 SN server 连接验证器建立可执行设计边界：`sn/service` 暴露 validator 装配点和显式 allow-all 默认实现，SN server 在处理 report、call 或等价入站请求前用只包含 `client_id` 与 `client_cert` 的连接上下文校验客户端，reject 时短路当前请求而不改变 SN command 线协议或 `SnCallResp` 最终连通性语义。
- 为本轮 `SnServiceContractServer` 清理建立可执行设计边界：删除未完整接入 SN 主流程的 service contract/receipt 生产路径、公开导出和构造装配；保留 `sn/protocol` 中既有 receipt wire 兼容结构，以及 `sn/service` 的 report/call/called handler、peer manager、连接验证器和 SN control stream 信令，不引入新的计费、合约评估、配额或持久化账本替代方案。
- 为本轮多 PN server 建链需求建立可执行设计边界：PN client 侧必须由上层显式指定目标用户所在的 relay PN server，并在该 PN 上建立或复用 relay tunnel；PN server 默认 `PnConnectionValidator` / `PnServer::new(...)` 保持显式 allow-all 兼容行为；多 PN assigned target admission 必须通过显式 validator 或 policy 构造路径限制新建逻辑 `PnTunnel` 的 target 归属；旧默认 PN 选择、`latest tunnel` 选择和单 `proxy_client` fallback 不再作为多 PN 新路径兼容目标。
- 为本轮 bounded channel 容量配置化需求建立可执行设计边界：`P2pConfig` / `P2pStackConfig` 作为顶层配置入口提供按位置拆分的容量配置，各项默认 `1024`，调用方默认无需设置；`NetManager`、`TunnelManager`、TTP registry、QUIC listener connect queue 和 PN 相关内部队列只接收对应位置已解析容量，不在底层定义默认值；TCP/QUIC tunnel 的入站 stream/datagram 已改为回调交付，不再保留旧 accept queue 容量参数；所有生产路径 `tokio::sync::mpsc::unbounded_channel` 均替换为 bounded channel，并明确满载时的背压、错误或关闭语义。
- 为本轮后续 stack/NetManager/TTP/PN 容量配置清理建立可执行设计边界：删除 `ChannelCapacityConfig`、`P2pEnv` 容量快照、`P2pConfig` / `P2pStackConfig` 的容量 getter/setter 和继承逻辑；`NetManager` 不再保存或暴露 channel capacity；`TtpRuntime::new()` 不再接收无效容量参数，`TtpClient` / `TtpServer` 不再为了创建 TTP runtime 从 `NetManager` 读取容量；`PnClient` 不再提供 channel capacity 显式构造入口；保留 bounded channel 与 `DEFAULT_CHANNEL_CAPACITY == 1024`，由默认构造路径内部使用固定容量。

### 非目标
- 对 `p2p-frame/docs/` 下已存在的每个协议细节做完整重写
- 在 harness 启动改造阶段重组源码文件
- 引入多 SN fanout、跨 SN NAT 类型推断、完整 STUN/TURN 协议栈或二层虚拟局域网语义
- 引入可被上层消费的原生 UDP tunnel、UDP payload 业务协议、raw UDP 接收解析器或独立于 QUIC listener 端口的新 UDP 打洞 socket
- 保留 `Default` 作为 endpoint area 名称、继续接受 `D` 作为新语义编码，或把 SN 观察地址无条件提升为静态 `Wan`
- 新增 `NetworkServerRuntime`、socket factory trait 或其他通用运行时抽象来包裹 `sfo-reuseport`
- 因 `sfo-reuseport` 重构或 listener 回调化而改变 TCP/QUIC tunnel 线协议、TLS 身份校验、QUIC NAT punch 策略、heartbeat 语义或 tunnel publish 规则
- 因 `Tunnel` stream/datagram 回调化而改变 TCP/QUIC/PN/TTP 线协议、TLS 身份校验、PN proxy channel 协议、业务 payload 格式、vport/purpose 编解码或 tunnel publish 规则
- 将内部 `control_stream` runtime、frame、stream id、window 或 buffer 协议公开导出；让外部直接读写 raw tunnel 控制通道；把 control stream 扩展为普通业务大流量传输平面；因新增 `Data` 命令重写现有 TCP/QUIC/PN ready、heartbeat、close、claim/open 或 PN control open 逻辑
- 将 SN control stream 信令解释为新的 SN 大流量数据平面、通用消息总线、跨 SN 协调协议或对内部 `control_stream` frame/window 的公开依赖；改变 `SnCallResp` 只表示 SN 受理结果的语义
- 借 `TtpNode` 改变 `Tunnel` / `TunnelNetwork` 公共 trait、TCP/QUIC/PN/TTP wire、vport/purpose 编码、身份校验、tunnel publish 规则，或引入库内 target directory / 自动路由系统
- 借 `TtpClient` target 删除或 idle release 改变 `Tunnel` / `TunnelNetwork` 公共 trait、TCP/QUIC/PN/TTP wire、tunnel publish 规则、底层 tunnel close 语义或 `TtpServer` lookup-only 行为
- 将 SN server 连接验证器解释为新的认证协议、计费系统、限速器、NAT 类型推断、跨 SN 策略同步、请求语义审计器或最终连通性判定；validator 不修改 SN command payload、endpoint 分类、单 SN 边界或 control-stream-only 信令选择，不接收 command、tunnel id、reported peer、target peer、来源 endpoint 或其他报文载荷派生字段
- 将 `SnServiceContractServer` 清理解释为删除 SN server/client 基础能力、SN command 线协议、`SnCallResp` 受理语义、peer manager、endpoint 分类、连接验证器、SN control stream 信令或 `sn/protocol` receipt wire 兼容结构；本轮不引入新的计费、配额、合约评估、持久化账本或相邻模块兼容旁路。
- 将多 PN server 支持解释为库内 `peer_id -> PN` 全局目录、自动 PN server 发现/切换、用户迁移、重平衡、多副本在线策略、PN server 之间目录同步或 `A -> PN-A -> PN-B -> B` 跨 PN 二跳业务 bridge；本轮不允许多 PN 新路径依赖默认 allow-all 掩盖错误 PN，但必须保留 `PnServer::new(...)` 默认 allow-all 兼容路径。
- 为 bounded channel 引入独立配置模块、全局静态默认值或底层局部默认值
- 通过额外 unbounded buffer、后台无限 Vec 队列或隐藏转发任务绕开 bounded channel 容量限制
- 为 `ChannelCapacityConfig` 删除引入新的公开容量配置结构、环境变量、feature flag 或运行时覆盖 API

## Overall Approach
- 将 `p2p-frame` 视为 Tier 0 核心模块。
- 把本文件作为顶层设计索引。
- 将 `p2p-frame/docs/` 下现有协议说明视为传输、tunnel、PN、SN 和 TTP 行为的从属设计证据。
- 未来工作按直接子模块拆分，并赋予真实的责任边界与验证边界。

## Simplicity Check
- Smallest sufficient approach: PN 多 server 只增加显式 relay route、显式 assigned target admission 和本 relay 进程内 session registry；不引入库内目录、自动切换或跨 PN 二跳 bridge；`PnServer::new(...)` 默认 allow-all 仅作为未配置部署兼容路径保留，不参与多 PN 错误 PN 兜底。
- Existing components or patterns reused: 复用现有 `TtpClient` / `TtpServer`、TTP target 匹配、`TtpRuntime::attach_tunnel(...)`、`PROXY_SERVICE`、`ProxyOpenReq` / `ProxyOpenResp`、PN control channel、`TunnelManager` proxy path publish 和 `PnServer` relay bridge；`TtpClient` 生命周期清理复用现有 tunnel cache pruning 与 TTP open_* 入口，不新增底层 tunnel trait。
- New abstractions introduced: `TtpNode`、`PnProxyRouteResolver` 或等价 `PnClient` 内部 route 解析配置、`PnAssignedTargetPolicy` / `PnRelayAdmission`、`PnRelaySessionRegistry`。
- Why each new abstraction is necessary: `TtpNode` 提供同时监听和按需主动建链的 TTP 节点入口，避免改变 `TtpServer` lookup-only 语义；route 输入消除 latest/default PN 歧义；assigned target policy 是错误 PN 在打开目标流前失败的准入点；session registry 用于区分新建 logical tunnel 与既有 tunnel 后续双向 channel。

## Current Structure
当前 `p2p-frame` 是一个包含 `networks`、`tunnel`、`ttp`、`sn`、`pn`、`finder`、TLS/X509 和 stack 组装层的核心 crate。TTP 现有公开对象分为 `TtpClient` 和 `TtpServer`：`TtpClient` 会在缺失 target tunnel 时主动创建并 attach，`connect_server(...)` 会把 target 加入 `maintained_targets` 并由后台 maintain loop 重连，但当前没有删除保持 target 的公开入口，普通 target 创建出的 cache tunnel 也没有按 idle 生命周期释放；`TtpServer` 通过 `NetManager` incoming subscriber 记住已接收 tunnel，并在 open 路径上只查找既有 tunnel。PN 现有路径通过 `PnClient` / `PnServer` 在 `PROXY_SERVICE` 上使用 `ProxyOpenReq` / `ProxyOpenResp` 建立 relay bridge；当前实现中存在按 latest tunnel 或单 proxy client 隐式选择 PN 的倾向。`PnServer::new(...)` 的 allow-all 便捷构造符合未配置部署兼容目标，但不能作为多 PN assigned target 准入的替代路径。

## Invariants to Preserve
- PN `ProxyOpenReq` / `ProxyOpenResp` wire 格式、TLS-over-proxy 边界、source/target 统计口径和 source 单边限速语义不因多 PN server 改动而改变。
- 错误 PN、缺少 relay route 或显式 assigned target policy 拒绝时必须显式失败，不得隐式查询目录、切换 PN server 或跨 PN 二跳 bridge；未注入策略的 `PnServer::new(...)` 只表达默认 allow-all 兼容行为。
- `TtpNode` 不改变现有 `TtpServer` lookup-only open 语义；需要主动建链的调用方显式使用 `TtpNode`。
- TTP 主动建链必须继续经由 `NetManager` / `TunnelNetwork`，不得在 TTP 层新增 target directory、网络选择规则或 tunnel publish 旁路。
- `TtpClient` target 删除和 idle release 只管理 `TtpClient` 本地保持集合与 tunnel cache；不得解释为删除 peer、全局关闭底层 tunnel 或改变 `TtpServer` / `TtpNode` 的 cache 与 lookup 行为。
- 已建立 logical `PnTunnel` 的后续双向 stream/datagram channel 可以复用同一 relay session，不应被误判为新的 target assignment 错误。
- `p2p-frame` 不拥有用户到 PN server 的目录、迁移、重平衡、断线重连或多副本在线策略。

## Submodules
| Submodule | Type | Responsibility | Depends On | Exported Interface | Notes |
|-----------|------|----------------|------------|--------------------|-------|
| `networks` | technical | TCP/QUIC 监听器、endpoint、validator、QUIC listener 同源 UDP punch burst 和底层网络行为 | `identity_tls`, `tunnel_channel_callback`, `tunnel_control_stream` | tunnel transport implementations | 高风险 trigger surface |
| `tunnel_channel_callback` | shared | `Tunnel` stream/datagram listen 回调类型、公共 trait 签名、入站 channel 回调交付和关闭/背压语义 | `identity_tls` | `listen_stream`, `listen_datagram` callback contracts | 被 networks、ttp、pn、stream/datagram 消费 |
| `tunnel_control_stream` | shared | `Tunnel` control stream 公共方法的内部 runtime、多路复用 frame、stream id、buffer/window、64KiB frame 上限和底层控制通道关闭传播 | `identity_tls` | `open_control_stream`, `listen_control_stream` adapters | 被 networks、pn、sn 消费 |
| `tunnel` | business | tunnel 生命周期、连接选择、统一 register/publish 生命周期、proxy 回退与后续脱代理升级行为 | `networks`, `finder`, `tunnel_channel_callback`, `tunnel_control_stream` | tunnel candidate selection and publish | PN/SN 接入由 stack_runtime 组装，避免设计依赖环 |
| `ttp` | technical | tunnel 上的命令和流复用协议、TTP target tunnel 查找/主动建链节点、TTP client 本地连接生命周期 | `networks`, `tunnel_channel_callback`, `tunnel_control_stream` | TTP client/server/node stream APIs | 具体 tunnel attachment 由 stack/runtime 组装 |
| `sn` | business | 对端注册、信令和调用转发，SN control stream 信令，连接验证器，移除 contract/receipt 生产路径 | `ttp`, `identity_tls`, `tunnel_control_stream` | SN service/client APIs | 单 SN 边界 |
| `pn` | business | proxy-node 中继行为、多 PN 显式 relay 选择和 assigned target admission | `tunnel`, `ttp`, `tunnel_channel_callback`, `tunnel_control_stream` | PN client/server APIs | 直接子模块文档保留 |
| `finder` | technical | 设备与 outer device 查询缓存 | `identity_tls` | finder lookup APIs | 支撑 tunnel |
| `identity_tls` | technical | P2P 身份、TLS、X509 和密码学辅助逻辑 | not-applicable: identity/tls is a leaf support submodule | identity and TLS helpers | 安全敏感 |
| `stack_runtime` | assembly | 高层 stack 编排和运行时抽象 | `networks`, `tunnel`, `ttp`, `sn`, `pn`, `finder`, `identity_tls`, `channel_capacity_config` | stack config and assembly APIs | composition root |
| `channel_capacity_config` | shared | bounded channel 固定默认容量和构造路径传递；后续清理删除公开分位置容量配置结构 | not-applicable: fixed capacity is consumed by callers rather than depending on them | default capacity constant / constructor wiring | 被 stack、networks、tunnel、ttp、pn 消费 |

## Boundary Rationale
| Boundary | Classification | Why Separate | Shared Logic / Technical Area | Notes |
|----------|----------------|--------------|-------------------------------|-------|
| `pn` relay vs `tunnel` orchestration | business | PN 负责指定 relay 上的 proxy 建链和 admission；tunnel 负责候选选择、publish 和复用 | shared control/listen callback APIs | 防止把 PN 目录或二跳 bridge 扩散到 tunnel manager |
| `PnAssignedTargetPolicy` vs library directory | business | target assignment 来自库使用者，`p2p-frame` 只消费策略结果 | technical injection boundary in `pn/service` | 避免新增库内 `peer_id -> PN` schema |
| `tunnel_control_stream` | shared | TCP/QUIC/PN 都需要低频 control stream 语义 | shared internal runtime | 不公开 frame/runtime |
| `identity_tls` | technical | 身份与 TLS 贯穿多 transport 和 PN TLS-over-proxy | technical support | 安全边界单独建模 |

## Boundary Decision Matrix
| boundary | classification | business_responsibility | shared_logic_or_technical_area | decision |
|----------|----------------|-------------------------|--------------------------------|----------|
| PN multi-server route selection | business | 上层指定目标 PN，PN client 只按指定 relay 建链 | route input and existing `TtpConnector` target interface | keep in `pn`/stack boundary; no library-owned directory |
| PN assigned target admission | business | `PnServer` 判断本 server 可作为 target 的用户 | `PnAssignedTargetPolicy` injected into `pn/service` | split from traffic accounting; required constructor dependency |
| PN relay session registry | technical | 区分新建 logical tunnel 与既有 tunnel 后续 channel | in-process registry owned by `pn/service` | keep inside `pn/service`; not a cross-PN directory |
| TTP node active open | technical | `TtpNode` 同时承担 TTP 监听和按 target 主动建链 | shared helper inside `ttp` using `NetManager` and `TtpRuntime` | add a new node type; keep `TtpServer` lookup-only |
| TTP client connection lifecycle | technical | `TtpClient` distinguishes maintained target and one-shot target local lifecycle | `maintained_targets` plus local tunnel cache metadata inside `ttp` | add `remove_server(...)` and local idle release; keep wire/trait/server semantics unchanged |
| control stream runtime | shared | 为 tunnel users 提供低频 control stream | private frame/runtime in `networks` | shared by TCP/QUIC/PN, not public |
| channel capacity | shared | bounded queue capacity stays fixed after cleanup | fixed default and constructor wiring | shared support, no public config after cleanup |

## Dependency Graph
| Source | Depends On | Reason | Cycle Check |
|--------|------------|--------|-------------|
| `networks` | `identity_tls`, `tunnel_channel_callback`, `tunnel_control_stream` | transport handshake and callback/control stream adapters | acyclic |
| `tunnel` | `networks`, `finder`, `tunnel_channel_callback`, `tunnel_control_stream` | candidate orchestration and publish | acyclic |
| `ttp` | `networks`, `tunnel_channel_callback`, `tunnel_control_stream` | framed stream/datagram multiplexing on attached tunnel IO and active tunnel creation through `NetManager` | acyclic |
| `sn` | `ttp`, `identity_tls`, `tunnel_control_stream` | SN command/control signaling and validation | acyclic |
| `pn` | `tunnel`, `ttp`, `tunnel_channel_callback`, `tunnel_control_stream` | proxy relay over TTP and tunnel lifecycle | acyclic |
| `finder` | `identity_tls` | device identity lookup support | acyclic |
| `stack_runtime` | `networks`, `tunnel`, `ttp`, `sn`, `pn`, `finder`, `identity_tls`, `channel_capacity_config` | assembly root wires submodules together | acyclic |
| `networks` | `channel_capacity_config` | bounded queue capacity usage | acyclic |
| `tunnel` | `channel_capacity_config` | subscription queue capacity usage | acyclic |
| `ttp` | `channel_capacity_config` | registry queue capacity usage | acyclic |
| `pn` | `channel_capacity_config` | PN internal queue capacity usage | acyclic |

## Key Call Flows
| Flow | Caller | Callee / Submodule Path | Purpose | Failure Handling | Notes |
|------|--------|--------------------------|---------|------------------|-------|
| PN active open with specified relay | `TunnelManager` delegates to configured PN `TunnelNetworkRef` | `pn/client` route resolver -> `ttp/client` -> specified PN `PROXY_SERVICE` | 建立 `A -> PN-B -> B` proxy tunnel | missing route returns config/param error; relay unavailable returns connect/open error; no fallback to latest/default PN | route resolver is owned by the `PnClient` implementation behind the trait object |
| PN server assigned target admission | remote source peer | `pn/service` admission -> target stream factory | 在打开目标前拒绝错误 PN | non-assigned target returns `InvalidParam`/permission-equivalent result before `open_target_stream` | no library directory lookup |
| Existing PN session reverse channel | established peer on same tunnel | `pn/service` session registry -> target stream factory | 允许同一 logical tunnel 后续双向 channel | session miss falls back to new-tunnel admission; closed/expired session rejects or reopens through new flow | prevents rejecting legitimate reverse channel |
| SN control stream signaling | SN client/service | `sn` -> `Tunnel::open_control_stream` | 低频 SN report/call/called/response | control unavailable or purpose not listened returns explicit error; no stream fallback | preserves SN command semantics |
| TTP node active stream open | `TtpNode::open_stream` / `open_control_stream` | `ttp` target cache -> `NetManager` selected network -> `TtpRuntime::attach_tunnel` -> `Tunnel::open_*` | 复用或按需建立 target tunnel 后打开 stream/control stream | existing tunnel unavailable is retained out; create/open/attach errors return directly; purpose not listened remains tunnel open error | no new tunnel trait or wire behavior |
| TTP maintained target removal | caller using `TtpClient` | `TtpClient::remove_server(target)` -> `maintained_targets` | stop background keepalive | target missing idempotent success; other targets remain; no global peer/tunnel delete | maintain loop reads fresh cloned target set each tick |
| TTP non-maintained idle release | `TtpClient` idle sweeper / `open_*` paths | `ttp` cache metadata -> local cache prune | release local cache reference | active/pending lease > 0 retains cache; maintained target retains cache; release does not alter tunnel wire/publish | default idle timeout 5 minutes; tests may use shorter helper |

## Large Module Submodule Decision
| Submodule | Source Proposal | Decision | Design Packet | Reason |
|-----------|-----------------|----------|---------------|--------|
| `pn` | P-PN-MULTI-SERVER-ASSIGNED-TARGET-1 / `pn_multi_server_assigned_target` | existing | `docs/versions/v0.1/modules/p2p-frame/design/pn-server.md` | 多 PN assigned target 属于既有 PN relay/client 责任，不需要新的直接 submodule packet |
| `sn` | P-SN-CONTRACT-CLEANUP-1 / `remove_sn_service_contract_server` | existing | `design.md` and SN design references | 清理发生在既有 SN service/client 边界内 |
| `tunnel_control_stream` | P-TUNNEL-CONTROL-STREAM-API-1 / `tunnel_control_stream_api` | existing shared | `docs/versions/v0.1/modules/p2p-frame/design/tunnel-control-stream-api.md` | 已作为共享技术子模块建模 |
| `ttp` | P-TTP-NODE-ACTIVE-OPEN-1 / `ttp_node_active_open`; P-TTP-CLIENT-CONNECTION-LIFECYCLE-1 / `ttp_client_connection_lifecycle` | existing | `design.md` | `TtpNode` 与 `TtpClient` 生命周期管理都属于既有 TTP 封装责任，不需要新的直接 submodule packet |

## Trigger Matrix
| trigger_category | applies | evidence | design_coverage | required_checks | deferred_checks_and_reason |
|------------------|---------|----------|-----------------|-----------------|----------------------------|
| contract/protocol | yes | PN multi-server constrains `ProxyOpenReq` relay behavior without changing wire; control stream adds private control command data; `TtpNode` adds a TTP wrapper API; `ttp_client_connection_lifecycle` adds local client lifecycle without changing tunnel or TTP wire | this file, `design/pn-server.md`, control stream docs, `ttp_node_active_open` and `ttp_client_connection_lifecycle` mappings | testing must cover correct PN, wrong PN, TtpNode active open, TtpClient remove/idle lifecycle, and no wire/trait format change | owner: testing; risk: protocol/API regression; acceptance impact: verify no PN/TTP wire drift |
| data/schema | no | user-to-PN assignment remains library-user owned; relay session registry is transient in-process state | Data and State | not-applicable: no persistent repository schema | owner: none; risk: low; acceptance impact: confirm no owned directory schema |
| security/privacy/permission | yes | assigned target admission rejects non-assigned target before target stream open; SN validator uses normalized identity | `pn-server.md`, SN validator sections | tests must cover rejection before target open and identity normalization | owner: testing; risk: unauthorized relay target; acceptance impact: permission boundaries verified |
| runtime/integration | yes | PN route selection, server constructor migration, control streams, TtpNode active open, TtpClient maintained target removal / non-maintained idle release, bounded queues and listener runtime affect integration | Directly Mapped Change Items | integration must cover specified PN success, wrong PN failure, TtpNode create/reuse behavior, TtpClient cache lifecycle, and migrated callers | owner: testing; risk: workspace migration; acceptance impact: stack callers compile and run |
| build/dependency/config/deployment | yes | `PnServer` default constructor remains allow-all; explicit multi-PN deployments provide assigned target policy; `sfo-io` and `sfo-reuseport` affect build/deployment | Interfaces and Dependencies | compile checks and integration config compatibility required | owner: implementation/testing; risk: caller migration; acceptance impact: no hidden default config |
| ui/datamodel/workflow | no | crate has no UI and does not define external assignment datamodel | not-applicable | not-applicable: no UI/datamodel owned here | owner: none; risk: low; acceptance impact: none |
| harness/process | yes | new change ids must map to design and admission scope, including `ttp_node_active_open` and `ttp_client_connection_lifecycle` | Directly Mapped Change Items | doc-structure-check and stage-scope-check | owner: design; risk: admission mismatch; acceptance impact: change_id evidence exists |

## Implementation Order
| 阶段 | 目标 | 前置条件 | 输出 | 依赖 | 可并行 |
|------|------|----------|------|------|--------|
| 1 | 确认 proposal 范围和直接子模块 | 已批准 proposal | 稳定的模块拆分 | proposal | no |
| 2 | 更新 design 映射并自动确认 | 已批准 proposal 与 pipeline launch evidence | approved `design.md` | 阶段 1 | no |
| 3 | 在硬性准入检查下实现 TTP 生产代码 | 已批准 design | production code and admission evidence | 阶段 2 | no |
| 4 | 基于已交付代码生成 testing 覆盖 | implementation complete | `testing.md`、`testplan.yaml`、tests | 阶段 3 | no |
| 5 | 依据 proposal 审计证据链 | implementation/testing 证据已就绪 | acceptance report | 阶段 4 | no |

## Interfaces and Dependencies
| Interface | Consumer | Compatibility | Notes |
|-----------|----------|---------------|-------|
| `PnProxyRouteResolver` 或等价目标 PN route 输入 | `pn_multi_server_assigned_target`, `PnClient`, stack callers | new | 调用方必须在 `PnClient` 配置目标用户所在 relay PN 解析能力；缺失时失败 |
| `PnServer::new(ttp_server)` | existing callers, `cyfs-p2p-test`, `sn-miner-rust` | backward-compatible | 默认安装显式 allow-all `PnConnectionValidator`，保持未配置部署可连接 |
| `PnServer::new_with_connection_validator(...)` / `PnServer::new_with_options(...)` 或等价显式策略构造器 | `pn_multi_server_assigned_target`, deployments requiring assigned target admission | backward-compatible | 多 PN 部署通过显式 validator / policy 注入 assigned target 准入；默认构造不承担错误 PN 拒绝 |
| `PnAssignedTargetPolicy` / `PnRelayAdmission` | `pn_multi_server_assigned_target`, `pn/service` | new | 判断新建 logical tunnel target 是否属于本 PN |
| `Tunnel::open_control_stream` / `listen_control_stream` | `tunnel_control_stream_api`, `sn_control_stream_signaling`, `pn/client` | new | control stream public API; internal frame/runtime stays private |
| `Tunnel::listen_stream` / `listen_datagram` callbacks | `tunnel_stream_datagram_listen_callback`, `ttp`, `pn`, `stream`, `datagram` | migration-required | replaces public `accept_*` polling |
| `TtpNode::new(...)` / `TtpNodeRef` | `ttp_node_active_open`, TTP callers that need both listener and active-open behavior | new | combines `TtpServer` incoming subscriber behavior with `TtpClient` target active-open behavior |
| `TtpClient::remove_server(&TtpTarget)` | `ttp_client_connection_lifecycle`, TTP callers that no longer want a target maintained | new | idempotently removes an exact maintained target; does not delete peers, globally close tunnels, or alter `NetManager` candidates |
| `TtpClient` non-maintained idle cache release | `ttp_client_connection_lifecycle`, normal one-shot `open_*` callers | backward-compatible | releases only local cached tunnel entries after idle timeout and zero active/pending leases; maintained targets are retained |
| `SnServiceConfig::set_connection_validator(...)` 或等价 SN validator 装配 | `sn_server_connection_validator`, `sn/service` | backward-compatible | SN 默认 allow-all remains explicit for SN only |

### 公共接口摘要
- `p2p-frame` 暴露核心网络和 tunnel 栈，供 `cyfs-p2p` 与运行时二进制消费。
- 直接子模块契约必须持续与当前协议说明和公开 crate 导出保持一致。
- `Tunnel` 公共 trait 的入站 stream/datagram 消费入口为 `listen_stream(vports, callback)` 与 `listen_datagram(vports, callback)`；公共 trait 不再提供 `accept_stream()` 或 `accept_datagram()`。
- stream 回调输入为 `P2pResult<(TunnelPurpose, TunnelStreamRead, TunnelStreamWrite)>` 或等价命名类型；datagram 回调输入为 `P2pResult<(TunnelPurpose, TunnelDatagramRead)>` 或等价命名类型。回调类型必须可 `Clone + Send + Sync + 'static`，返回 `Send + 'static` 的异步 future，且不得把 TLS、PN 加密模式或线协议参数加入通用 trait。
- `Tunnel` 公共 trait 的 control stream 消费入口为 `listen_control_stream(purposes, callback)`，主动打开入口为 `open_control_stream(purpose)`；control stream callback 输入为 `P2pResult<(TunnelPurpose, TunnelStreamRead, TunnelStreamWrite)>` 或等价命名类型。除这些公共类型和 trait 方法外，`ControlStreamRuntime`、`ControlStreamFrame`、stream id、window、buffer、`MAX_CONTROL_DATA_FRAME_SIZE` 等内部实现细节必须保持 `pub(crate)` 或更窄可见性，不得从 `p2p-frame` crate 公共 API 导出。
- SN 信令消费 control stream 时只通过上述公共 `Tunnel` trait 方法进入。SN 子模块定义一个固定的 SN 信令 purpose 或等价 `TunnelPurpose` 常量，并在 tunnel attach 或 SN service 初始化时注册 `listen_control_stream(...)`；发送 report/call/called/response 等小消息时调用 `open_control_stream(...)`。purpose 不匹配、控制通道未 ready、远端未监听或旧版本返回 `PortNotListen` / `ListenerClosed` 时，按 `design/sn-control-stream-signaling.md` 中的失败规则返回错误，不回退到既有普通 stream 信令路径，不改变 SN 命令语义。
- `TtpNode` 属于 `ttp` 子模块的新公开节点类型。它实现 `TtpPortListener` 与 `TtpConnector`，构造参数与 `TtpServer::new(local_identity, net_manager)` 保持同类；内部保存 `local_identity`、`net_manager`、共享 `TtpRuntime` 和按 `remote_id` 记忆的 tunnel cache。`TtpNode` 注册与 `TtpServer` 等价的 incoming tunnel subscriber，入站 tunnel attach 后进入同一 cache；`open_stream(...)`、`open_control_stream(...)` 和第一版 `open_datagram(...)` 复用一个私有 `get_or_create_tunnel(target)` helper：先按 `remote_id`、endpoint 和 `is_tunnel_available` 查找，找不到时通过 `target.remote_ep.protocol()` 选择 network，并按 `target.local_ep` 是否存在调用 `create_tunnel_with_local_ep(...)` 或 `create_tunnel(...)`，成功后 attach 并缓存。`TtpServer` 的 `get_existing_tunnel(...)` 和 lookup-only 错误语义保持不变。
- `TtpClient::remove_server(&TtpTarget)` 是 `connect_server(...)` 的保持集合反向操作，按 `(remote_id, remote_ep)` 精确匹配并幂等删除 target。删除不存在的 target 返回成功；删除一个 target 不影响同 peer 的其他 target，不关闭所有 peer tunnel，不修改 `NetManager` candidate，也不改变 `TtpServer` lookup-only 行为。`configured_server_id()` 基于当前保持集合返回单一 server id；集合为空时返回 `Ok(None)`；maintain loop 每轮只克隆当前保持集合，删除后的 target 不会被下一轮重建。
- `TtpClient` tunnel cache 为每个 cached tunnel 记录 tunnel 引用、是否属于 maintained target、最近使用时间以及 active/pending lease 计数。`open_stream(...)` / `open_control_stream(...)` / `open_datagram(...)` 在发起底层 open 前增加 pending lease，成功后把返回的 read/write 或 datagram write 包装为 active lease guard，失败时释放 pending lease；guard drop 后更新最近使用时间并减少 active lease。idle release 只清理 non-maintained、lease 为 0、tunnel 仍可用且超过默认 5 分钟 idle timeout 的本地 cache entry；测试可以使用更短的 test-only timeout/helper。保持 target 不被 non-maintained idle release 清理，底层 tunnel wire、publish 和 close 语义不变。
- SN server 连接验证器接口属于 `sn/service`。设计使用一个可克隆、线程安全的 validator 对象或函数，输入为规范化后的客户端上下文，且该上下文只能包含 `client_id` 与 `client_cert`。`client_id` 来自 cmd tunnel 已认证 peer id；`client_cert` 来自当前 SN 请求携带的客户端证书或已缓存的同一客户端证书，且实现必须用 `cert_factory` 解析证书并确认解析出的 id 等于 `client_id` 后才能调用 validator。默认 `SnServer::new(...)` 或等价便捷构造器必须安装 `allow_all_sn_connection_validator()`；需要自定义策略的调用方使用 `SnServiceConfig::set_connection_validator(...)` 或等价显式构造器。
- `SnServiceContractServer` 清理属于 `sn/service` 和 `sn/client` 中未接入主流程的服务合约路径删除，不新增公开替代接口。实现应删除或停止导出 `client/contract.rs`、`service/receipt.rs`、`SnServiceContractServer` 及仅服务于该方向的后台任务、构造字段和存储/统计连接；`sn/protocol` 中的 `SnServiceReceipt`、`ReceiptWithSignature` 和 report/response receipt 字段保留以维持 wire 兼容。`SnServiceConfig::new(...)`、`create_sn_service(...)`、SN control purpose 注册、validator 装配和 report/call/called handler 保持可用。若下游启动二进制仅通过 `SnServiceConfig` / `create_sn_service` 使用 SN service，则只需随公开 API 删除编译适配；不得在 `cyfs-p2p-test`、`sn-miner-rust` 或 `cyfs-p2p` 中重建 contract server 兼容旁路。
- 多 PN server 支持属于 PN client 和 stack 组装边界。主动创建 proxy tunnel 时，外部调用方仍只能通过既有 `TunnelNetwork::create_tunnel_with_intent(...)` / `create_tunnel_with_local_ep_and_intent(...)` 建链接口进入 PN network，不得新增或改调 PN 专有建链入口；上层必须为实际的 `PnClient` 配置目标用户所在的 relay PN server id 解析能力。`PnClient` 在该既有接口实现内部解析 relay，并只能通过该指定 relay 打开 `PROXY_SERVICE`，不得使用 `TtpClient` 的 latest tunnel、单例 `proxy_client` 或隐式默认 PN 作为 fallback。`TunnelManager` 字段和构造参数保持 `TunnelNetworkRef` 边界，不持有、不调用 PN route resolver，只把 proxy 建链委托给已配置 PN network 的既有建链接口；`PnClient` 若无法解析目标 PN server，应返回配置/参数错误，不得自行查询目录或尝试其他 PN server。`PnServer::new(ttp_server)` 必须保留显式 allow-all 默认 validator；需要多 PN assigned target admission 的部署必须使用 `new_with_connection_validator(...)`、`new_with_options(...)` 或等价显式策略构造器。

### 外部依赖
- async/runtime crates
- crypto and TLS crates
- CYFS-adjacent consumers through `cyfs-p2p`
- `sfo-io` 提供的流量统计与限速实现，供 `pn/service` 在 bridge 数据路径中复用
- `rustls`、`tokio-rustls` 以及现有 `p2p-frame/src/tls/**` 组件，供 proxy tunnel 在成功建链后复用 TLS 握手与身份校验

### 运行约束
- 传输和运行时行为必须能够通过日志进行诊断。
- 影响协议的改动必须在代码改动开始前更新 design/testing 证据。
- `pn_server` 的统计与限速不得改变现有 `ProxyOpenReq` / `ProxyOpenResp` 握手顺序和结果码映射。
- 多 PN server 的主动建链必须显式携带目标 relay PN 选择；缺少 relay 选择、relay 不可连接、或指定 relay 的 assigned target admission 拒绝 `req.to` 时，应在打开目标侧 stream 前失败，不能回退到 latest tunnel、默认 `proxy_client`、库内目录查询或跨 PN 二跳 bridge。
- `TtpClient::remove_server(...)` 不等待并发中的 `connect_server(...)` 或 `open_*` 完成；同一 exact target 最终是否维护由最后一次 add/remove 写入决定。实现不得持有 `maintained_targets` 或 tunnel cache mutex 跨 await；创建或打开 tunnel 后，必须重新读取当前 target 状态再标记 cache entry 是否 maintained。
- `TtpClient` idle release 只移除本地 cache entry，不主动关闭底层 tunnel，不从 `TunnelManager` 或 `NetManager` 删除候选，不影响其他持有同一 `TunnelRef` 的 owner。
- `pn_server` 的统计主体必须分别锚定到 relay 已认证并规范化后的 source 身份与已打开的 target 身份；限速主体仍只使用 source 侧身份。
- proxy tunnel 的“是否加密”必须通过 PN 专有显式入口或显式的 client/stack 配置决定；通用 `TunnelNetwork` / `Tunnel` trait 不新增 TLS 参数。未显式配置的 `PnClient` 上，`create_tunnel*` 与 `open_*` 默认保持当前明文兼容语义；若调用方先显式设置 `PnClient::set_stream_security_mode(...)` 或 `P2pStackConfig::set_proxy_stream_encrypted(true)`，则该 `PnClient` 后续通过通用 trait 创建的 proxy tunnel，以及同一 listener 被动接受到的 `PnTunnel`，都会继承当时的 TLS 模式快照。
- 是否启用 TLS 由 proxy tunnel 两端在 tunnel 外预先约定；PN open 控制流不额外承载 TLS 模式协商。
- 若调用方显式选择加密，则失败路径必须直接失败，不允许静默回退到明文桥接。
- 本轮加密设计只覆盖 proxy `stream`；proxy `datagram` 不进入 TLS-over-proxy 范围，但必须忽略 `stream` 加密模式并保持当前明文兼容语义。
- `PnTunnel` idle close 必须按本地兜底生命周期关闭建模；远端关闭感知由控制通道承担，idle timeout 触发后 tunnel 状态进入 `Closed` 或等价错误终态，并复用普通 close 的本地效果。
- `PnTunnel` 控制通道必须参考 `TcpTunnel` 的 control connection ready gate 和 `QuicTunnel` 的 command stream/heartbeat 模型：active 创建 tunnel 时先打开 PN control channel，passive 侧注册并投递 tunnel 后返回 ready，active 侧收到 ready 成功后才把 tunnel 视为可用。
- PN control channel 必须与业务 stream/datagram channel 分离；业务 `ProxyOpenReq` / `ProxyOpenResp` 继续只负责单条业务 channel，不能兼任 tunnel 生命周期控制面。
- PN control channel 建立后必须启动控制接收循环和心跳循环；收到对端 close、控制 read EOF、decode 失败、写失败或 heartbeat timeout 时，必须进入与手动 close/idle close 共用的 `PnTunnel` 关闭路径。
- 本地 `PnTunnel::close()` 必须尽力通过控制通道发送 close 命令并关闭控制写端；发送失败不阻止本地关闭，但必须保证本地状态幂等收敛。
- `PnTunnel` 必须在统一状态临界区内维护 channel lease 计数、pending inbound queue 计数、idle deadline 和 close reason；本地 open、inbound 投递、channel drop 与 idle sweeper 都必须通过该状态执行竞态判定。
- `PnTunnel` 生命周期状态必须显式区分 `OpeningControl`、`Open`、`Closing`、`Closed` 或等价状态；只有控制通道 ready 后才能进入 `Open`，后续 local open 与 inbound channel 投递都必须检查该状态。
- `PnShared` 必须在 tunnel 关闭后移除 live registry 中的旧对象，确保后续同 `(remote_id, tunnel_id)` inbound `ProxyOpenReq` 不会投递到已关闭 tunnel，而是创建新的 passive tunnel。
- 当某个远端当前只有 proxy tunnel 可用时，`TunnelManager` 必须在后台继续尝试 direct 或 reverse 建链，而不是无限期停留在 proxy 路径。
- 上述脱代理尝试不得再次把“升级任务”回退成 proxy 建链；proxy 只作为对外可用的兜底连通性，而不是后台升级路径的成功条件。
- 持续失败的脱代理尝试可以延长重试间隔，但必须有上限；默认常规约束为初始 5 分钟、失败后指数退避、最大不超过 2 小时。本轮 NAT 打洞优化对新建 proxy candidate 额外引入短窗口升级调度：先按短间隔探测 direct/reverse，再回到有上限的指数退避。
- 对外可用的新 tunnel 必须先进入 `TunnelManager` 候选注册，再进入统一 publish 路径；除 `remote_id` 未知的临时 wrapper 外，不允许存在“只广播不登记”的长期语义分支。
- `TunnelManager` 的 publish 可见性决策必须收敛到单一生命周期模型：默认“register 后立即 publish”，唯一允许的延后场景是命中本地 `(remote_id, tunnel_id)` `reverse_waiter` 的 reverse tunnel。
- 上述延后 publish 只影响时机，不改变规则：reverse tunnel 一旦完成本地 waiter 交付，就必须通过与 direct/proxy 相同的 publish 入口变为可见候选。
- reverse incoming 必须先尝试消费同 `(remote_id, tunnel_id)` 的 pending reverse waiter。命中 waiter 时，incoming 分支只 notify waiter，不得立即 register；waiter 接收方确认拿到 tunnel 后，才负责 register 并通过统一 publish 入口发布。无 waiter 时必须直接关闭，不得 register、不得 publish、不得进入默认复用候选。
- reverse 建链若在等待期间超时、取消或失败，相关 waiter 必须被清理。清理后迟到的同 key reverse incoming 会因为无 waiter 而关闭；实现不得为此引入额外 reverse 过期表。
- `TunnelManager` 在默认复用已有 tunnel 时必须先从已发布候选中选择；同一可见性层级下，非 proxy candidate 优先于 proxy candidate，只有没有可用非 proxy candidate 时才返回 proxy。若存在多个同类候选，再按最近更新时间选择最新候选。
- direct 与 reverse 必须使用同一个 logical `tunnel_id` 进行 hedged 建链；reverse 的启动延迟统一为 300ms，而不是固定等待 direct 路径 2 秒。
- QUIC/UDP NAT 候选场景下，可以在 direct/reverse 建链窗口内发送少量 best-effort 原生 UDP punch 包；该机制必须使用与 QUIC listener 相同的本地 UDP socket/端口或该 socket 的 send-only clone，不得新建不同源端口的 UDP socket 伪装成同一路径打洞。
- UDP punch 只允许在 SN service 存在时面向 `EndpointArea::ServerReflexive` QUIC/UDP 候选启用；`Lan`、`Wan`、`Mapped`、TCP、IPv6、0 端口和 proxy fallback 路径不得因为该机制改变原有建链语义。映射端点在当前实现中按 WAN endpoint 处理，不触发 punch。
- UDP punch 必须通过本次连接的 `TunnelConnectIntent` 启用，默认不发送；`TunnelManager` 只有在存在 SN service 且候选符合策略时才为本次 candidate intent 开启该开关。该开关只控制这一次 QUIC 建链的 punch 调度，不改变 `TunnelNetwork` trait 参数或 tunnel 成功条件。
- UDP punch 包必须是本地实现私有的短探测载荷；载荷内容和长度都可以每包随机生成，长度范围限定为 `5..=30` 字节，不要求包含固定 magic、版本、`tunnel_id`、`candidate_id` 或方向标记，也不得被任一侧依赖为协议语义。payload 首字节必须清除 QUIC fixed bit `0x40`，避免随机探测包碰撞成符合 QUIC packet invariant 的输入。接收侧不解析、不确认、不投递给上层，QUIC listener 继续把这些非 QUIC datagram 作为无效输入丢弃。
- UDP punch 发送失败、被丢包或被远端忽略不得直接判定 tunnel 成败；真正的成功条件仍是后续 QUIC handshake 和 `TunnelManager` register/publish 生命周期完成。
- UDP punch burst 必须有严格上限：reverse burst 在启动后立即发送第一包，active burst 则从 `250ms` offset 才发送第一包；之后两者都固定每 `50ms` 一包，默认最晚到 `1s` 截止，并受 NAT hedged window 约束裁剪，避免在弱网或 proxy 脱代理重试中形成无限重发或发送风暴。
- QUIC tunnel 控制心跳发送间隔保持现有默认值；heartbeat timeout 调整为 30 秒。该调整只改变失活判定阈值，不新增上层业务心跳、raw UDP keepalive 协议或公共 `TunnelNetwork` 参数；心跳超时后的关闭仍走既有 QUIC tunnel close/error 路径。
- `SNClientService::call(...)` 必须支持调用方传入本次建链的 `reverse_endpoint_array`；若调用方不传，仍保持现有兼容语义。候选来源和去重由 `tunnel` / `sn/client` 设计补充文档定义。
- SN server 在解码 report、call 或等价入站 SN command 后、执行状态更新或转发前调用 validator。validator 返回 accept 时继续既有 handler；返回 reject 或 `PermissionDenied` 类错误时，server 短路当前请求，不更新注册表、不转发 call、不扩展观察候选。若当前请求需要响应，错误映射复用现有通用失败结果或 `PermissionDenied` / `InvalidParam` 等价错误，不新增 SN 线协议字段。
- validator 上下文的客户端身份必须来自 cmd tunnel 规范化 peer id，并且客户端证书必须由 `cert_factory` 解析后与该 `client_id` 一致；SN command payload 中客户端自填的 `from` / `to`、command code、tunnel id、endpoint 或其他请求字段不得进入 validator 上下文。
- SN report、call、called、response 等低频小消息应复用 `Tunnel` control stream，避免每次小消息都默认建立普通业务 stream。bootstrap、控制通道不可用、远端未监听 control purpose 或旧版本不支持时，当前 SN 命令通道创建/发送按既有错误模型失败，不回退到普通业务 stream。
- `TunnelNetwork::listen(...)` 的公共签名调整为接收 `IncomingTunnelCallback` 或等价类型：回调必须可在线程间安全克隆，输入为 `P2pResult<TunnelRef>`，返回异步 `()` 或等价 future。该回调是唯一的公共入站 tunnel 通知路径。
- `TunnelNetwork::listen(...)` 成功返回 `P2pResult<()>`，表示 listener 已绑定或已复用并会把后续入站 tunnel 投递给回调；失败仍按原有 bind/listen 错误返回。
- `TunnelNetwork` 公共 trait 移除 `listeners()`。`TcpTunnelListener`、`QuicTunnelListener` 和 `PnListener` 可继续作为各 network 内部实现对象存在，但不再由 `TunnelNetwork` trait 导出给上层。
- `NetManager::listen(...)` 必须为每个 network listen 调用注册一个回调，回调中调用 `dispatch_tunnel_result(...)` 或等价路径，保持原有 `spawn_listener_loop` 中的错误处理、incoming validator、订阅发布和 reject close 行为。
- `NetManager::get_listener(...)` 与 `listener_entries(...)` 不再属于公共 manager 能力；需要监听地址和映射端口的调用方继续使用 `get_listener_info(...)` 与 `listener_info_entries(...)`。
- proxy client 启动不得通过 `listeners().is_empty()` 判断是否已监听，应使用 `listener_infos().is_empty()` 或 PN client 内部幂等 listen 语义。
- `Tunnel::listen_stream(...)` 与 `Tunnel::listen_datagram(...)` 必须保存对应 vport/purpose listen 规则和回调，并返回 `P2pResult<()>`；重复 listen 默认替换同类回调和 listen 规则，旧回调不再接收后续 channel。
- `Tunnel` 入站处理路径收到 stream/datagram channel 后，必须先检查 tunnel 状态和 listen 规则。未 listen 或不匹配 purpose 时，必须按现有 open 失败语义拒绝或关闭 channel，不得静默丢弃成功状态。
- `Tunnel` 关闭后必须清空或禁用 stream/datagram 回调；关闭与入站投递并发时，迟到 channel 必须关闭、拒绝或返回错误，不得交付给已关闭 tunnel 的外部回调。
- 回调 future 的调度不得阻塞 transport control loop、QUIC endpoint accept loop、PN control loop 或 TTP frame dispatch。TCP/QUIC/PN tunnel 可在对应 tunnel runtime 中 `spawn` 回调任务，或通过已配置 bounded channel 转交给专门分发任务；如果内部队列满载，必须返回 `P2pErrorCode::OutOfLimit` 或等价错误并关闭迟到 channel。
- 对基于 `sfo-reuseport` worker 的 TCP/QUIC 入站路径，worker runtime 只负责把新 channel 交给 tunnel 内部 dispatch；外部回调 future 不得长期占用 worker socket accept/recv loop。具体实现可在 worker runtime spawn 轻量投递任务，但必须保持 bounded channel 背压和关闭后不回调语义。
- TTP runtime attach tunnel 时不再 spawn `accept_stream()` / `accept_datagram()` loop，而是注册 stream/datagram 回调；回调中按 registry 查找 listener 并转交给 TTP listener bounded 队列。stream manager、datagram manager 和 PN server/client 同样改为注册回调并在回调中执行原有 listener lookup、统计或 bridge 分发逻辑。
- `listen_control_stream(...)` 的重复注册、替换和关闭后注册语义必须与 `listen_stream(...)` 保持一致；`open_control_stream(...)` 只能在底层 tunnel 控制通道健康且 tunnel 未关闭时成功。control stream 的 purpose 过滤复用 `ListenVPortsRef`，未 listen 或 purpose 不匹配时通过内部 `OpenResp` 返回 `PortNotListen` / `ListenerClosed`。
- TCP/QUIC/PN 现有控制命令枚举各新增一个 `Data` 命令，其 body 只包含 `Vec<u8>` payload；transport 控制循环遇到 `Data` 只调用内部 `control_stream.on_data(payload)`，不得解析内部 frame。内部 `control_stream` frame 至少覆盖 `Open`、`OpenResp`、`Data`、`Fin`、`Reset` 和必要 window/credit；这些 frame 不对外公开。
- 单个外层控制命令 `Data` payload 最大 `64 KiB`；内部 control stream 写入更大用户 buffer 时必须切分成多个不超过上限的 `Data` 命令。接收侧发现 payload 超过 `64 KiB` 时必须按协议错误关闭底层 tunnel 或至少调用 `control_stream.close_all(...)` 并拒绝继续解析。
- 底层控制通道 EOF、decode 失败、write 失败、heartbeat timeout、收到 remote close、本地 `close()` 或 tunnel 状态进入 closing/closed/error 时，transport 必须调用 `control_stream.close_all(reason)`；该调用必须让所有 virtual stream 的 read/write/pending open 立即失败或 EOF，并释放 buffer。
- 为避免 control stream 数据阻塞内部 heartbeat/close，`control_stream` 发送侧必须分片并避免长期持有控制写锁；内部 close/heartbeat 等既有命令逻辑保持原有优先级和状态机，不因 control stream 子协议重写。
- `P2pConfig` 必须新增顶层 `ChannelCapacityConfig` 或等价配置快照，按队列用途至少区分 QUIC listener connect、TTP listener registry、TunnelManager subscription、NetManager incoming subscriber 和 PN 内部队列；每项默认值为 `1024`，调用方默认不需要设置，并可只覆盖单个位置。`create_p2p_env(...)` 必须把对应容量传入仍存在 bounded queue 的底层组件。
- `P2pEnv` 必须保存已解析的分位置 channel 容量配置，作为 stack 层继续传递的唯一来源；`P2pStackConfig` 必须从 `P2pEnv` 继承该配置快照，并在创建默认 `PnClient`、`SNClientService`、`TunnelManager` 及其他 stack 组装路径时传入对应位置容量。底层构造函数只接收 `usize` 容量或显式配置快照，不得在构造函数内部使用 `unwrap_or(1024)`、`const DEFAULT_*` 或其他默认兜底。
- 后续 `stack_channel_capacity_config_removal` 清理阶段必须删除上一轮公开 `ChannelCapacityConfig` 结构以及 `P2pEnv` / `P2pConfig` / `P2pStackConfig` 的容量访问、覆盖和继承逻辑；`NetManager::new(...)` / `new_with_incoming_tunnel_validator(...)` 不再接收容量参数，`NetManager` 不再保存 `channel_capacity` 字段或提供 `channel_capacity()`；`TtpRuntime::new()` 必须改为无参数，`TtpClient::new(...)` / `TtpServer::new(...)` 不再读取 `NetManager::channel_capacity()` 来创建 TTP runtime；`PnClient::new_with_channel_capacity(...)` 和 `PnClient::new_with_tls_material_and_channel_capacity(...)` 必须删除，默认 PN client 构造内部使用固定容量。该清理不得改变底层 bounded sender/receiver 类型，也不得新增替代公开容量配置 API。
- TCP 与 QUIC tunnel 的 inbound stream/datagram 不再通过旧 accepted queue 暴露给调用方；实现不得保留旧 `channel_capacity` 构造参数或为该路径新增隐藏 queue 来模拟旧轮询入口。远端或控制循环投递 inbound channel 时，必须按 listen 回调、关闭状态和协议拒绝路径收敛。
- `TunnelManager` 的 subscription receiver、`NetManager` incoming subscriber、TTP listener registry、QUIC listener connect request queue 和 PN service/test 内部 accept queue 必须分别使用顶层配置中对应位置的容量。对不允许阻塞的同步分发路径使用 `try_send` 并把满载转化为错误、关闭或移除订阅；对天然异步且调用方可等待的路径可以使用 `send(...).await`，但不得持有会造成死锁的 mutex guard 跨 await。
- `TtpRegistry::register(...)` 必须接收 capacity 参数或在 `TtpClient` / listener 构造时绑定容量；同一 purpose 重复注册语义保持不变。满载时，新的 stream/datagram 投递必须返回错误给 tunnel/ttp 分发路径，而不是丢弃成功状态。
- `TtpNode` 的 `get_or_create_tunnel(...)` 必须清理不可用 tunnel 并避免重复 attach 同一 tunnel；attach 失败时不得把该 tunnel 记入 cache。`open_stream(...)` 和 `open_control_stream(...)` 的 metadata 构造与 `TtpClient` 保持一致：`local_ep` 优先使用 tunnel local endpoint 后回退 target local endpoint，`remote_ep` 优先使用 tunnel remote endpoint 后回退 target remote endpoint。`open_datagram(...)` 第一版也使用该 helper，保持与 `TtpClient` 的 active-open 行为一致；若后续需要 lookup-only datagram 必须回 proposal/design。
- 测试替身和 `#[cfg(test)]` helper 可以使用显式小容量来验证满载行为；如果测试需要默认容量，必须从顶层配置或测试专用 helper 参数传入，不得在底层类型中新增默认值。
- `EndpointArea::ServerReflexive` 是 SN 观察到的 server-reflexive endpoint 标记，不代表系统默认绑定地址。`Endpoint` 的文本编码必须用 `S` 表示该 area，字符串解析只把 `S` 映射到 `ServerReflexive`；raw codec 继续复用原 area bit 位置，但语义名改为 `ServerReflexive`。
- `Endpoint::is_sys_default()` 不再属于公开接口；实现阶段应删除该方法并修正调用点。若发现下游依赖该方法，应退回 design 明确兼容策略，而不是保留旧 system-default 语义。
- SN 服务端扩展观察 endpoint 时必须比较 SN 观察到的 socket address 与节点自上报 endpoint 集合：协议、IP 和端口均匹配时可标记为 `Wan`；否则标记为 `ServerReflexive`。`Mapped` 仍表示明确映射端口构造出的 WAN 类候选，不与 `ServerReflexive` 合并。
- tunnel/NAT 候选消费端必须把 `ServerReflexive` 作为非静态 WAN 来源处理；它可参与反连候选和 NAT 打洞策略，但不得满足 `is_static_wan()` 的 `Wan` / `Mapped` 判定。
- endpoint 评分必须按协议和端点来源记录历史结果；TCP 失败不得降低同一远端 QUIC/UDP 候选的打洞优先级。
- 本轮保持单 SN 信令模型。SN 服务端只转发单 SN 观察到的公网端点、客户端上报的映射端口和本次 `SnCall` 候选，不承担多 SN 汇总或最终连通性判定。

## Implementation Layout
```text
p2p-frame/src
├── networks/
├── tunnel/
├── ttp/
├── sn/
├── pn/
├── finder/
├── tls/
├── x509/
├── datagram/
├── dht/
├── stack.rs
└── p2p_identity.rs
```

| 路径 | 类型 | 职责 | 备注 |
|------|------|------|------|
| `p2p-frame/src/networks/` | dir | 传输层 | 高风险 trigger surface |
| `p2p-frame/src/tunnel/` | dir | tunnel 编排 | 高风险 trigger surface |
| `p2p-frame/src/ttp/` | dir | tunnel transport protocol | 高风险 trigger surface |
| `p2p-frame/src/sn/` | dir | SN 服务逻辑 | 高风险 trigger surface |
| `p2p-frame/src/pn/` | dir | PN 中继逻辑 | 高风险 trigger surface |
| `p2p-frame/src/pn/client/` | dir | PN client、listener 和 tunnel 行为 | 与 PN 参考说明配套 |
| `p2p-frame/src/pn/service/` | dir | relay 侧 PN server、校验和桥接 | 由 `pn_server` 补充文档建立索引 |
| `p2p-frame/src/finder/` | dir | 设备查询辅助逻辑 | 对邻接模块敏感 |
| `p2p-frame/src/tls/` | dir | TLS/密码学辅助逻辑 | 安全敏感 |
| `p2p-frame/src/x509.rs` 和 `p2p-frame/src/x509/` | file/dir | X509 支持 | 受 feature gate 控制且安全敏感 |
| `p2p-frame/src/stack.rs` | file | stack 组装与顶层配置 | `P2pConfig` / `P2pStackConfig` 持有 bounded channel 容量默认值和覆盖入口，并向 env、network、manager、PN client 等底层路径传递 |
| `p2p-frame/docs/*.md` | docs | 协议/设计参考 | 在下方建立索引 |

## Document Index
| 文档 | 主题 | 范围 |
|------|------|------|
| `design.md` | 模块概览和任务拆分 | 完整模块 |
| `docs/versions/v0.1/modules/p2p-frame/design/tunnel-publish-lifecycle.md` | `TunnelManager` 新 tunnel 注册/发布生命周期与 reverse 延后 publish 规则补充 | `tunnel` |
| `docs/versions/v0.1/modules/p2p-frame/design/tunnel-nat-traversal.md` | 单 SN NAT 打洞优化、direct/reverse 竞速、QUIC listener 同源 UDP punch、候选刷新、proxy 短窗口脱代理、endpoint 评分和 `ServerReflexive` endpoint area 分类补充 | `endpoint`、`tunnel`、`networks/quic`、`sn/client`、必要 `sn/service` |
| `docs/versions/v0.1/modules/p2p-frame/design/pn-proxy-encryption.md` | PN client / tunnel 侧可选端到端载荷加密、显式接口和 TLS 叠加设计补充 | `pn/client` |
| `docs/versions/v0.1/modules/p2p-frame/design/pn-tunnel-idle-close.md` | `PnTunnel` idle timeout 生命周期关闭、channel lease 计数和关闭后重新创建设计补充 | `pn/client` |
| `docs/versions/v0.1/modules/p2p-frame/design/pn-tunnel-control-channel.md` | `PnTunnel` 控制通道建立、ready gate、heartbeat、远端关闭感知和关闭状态机收敛设计补充 | `pn/client`、`pn/protocol`、必要 `pn/service` |
| `docs/versions/v0.1/modules/p2p-frame/design/pn-server.md` | relay 侧 PN server、`sfo-io` 流量统计与限速设计补充 | `pn/service` |
| `docs/versions/v0.1/modules/p2p-frame/design/sfo-reuseport-listeners.md` | TCP/QUIC listener 基于 `sfo-reuseport` 重构、`ServerRuntime` 注入和 Quinn `AsyncUdpSocket` 适配设计补充 | `networks/tcp`、`networks/quic`、`stack_runtime` |
| `docs/versions/v0.1/modules/p2p-frame/design/tunnel-channel-listen-callback.md` | `Tunnel` stream/datagram listen 回调化、公共 `accept_*` 移除、回调运行时与 manager 迁移设计补充 | `networks`、`ttp`、`stream`、`datagram`、`pn/client`、`pn/service` |
| `docs/versions/v0.1/modules/p2p-frame/design/tunnel-control-stream-api.md` | `Tunnel` control stream API、内部 control stream runtime、`Data` 控制命令承载、64KiB 上限和控制通道关闭传播设计补充 | `networks/tunnel`、`networks/tcp`、`networks/quic`、`pn/client` |
| `docs/versions/v0.1/modules/p2p-frame/design/sn-control-stream-signaling.md` | SN report/call/called/response 低频小消息使用 `Tunnel` control stream、purpose 与失败边界设计补充 | `sn/client`、`sn/service`、`networks/tunnel` |
| `p2p-frame/docs/tunnel_design.md` | tunnel 概念 | tunnel |
| `p2p-frame/docs/tunnel_command_protocol_design.md` | tunnel 命令协议 | tunnel/ttp |
| `p2p-frame/docs/tcp_tunnel_protocol_design.md` | TCP tunnel 协议 | networks/tunnel |
| `p2p-frame/docs/quic_tunnel_design.md` | QUIC tunnel 协议 | networks |
| `p2p-frame/docs/sn_design.md` | SN 行为 | sn |
| `p2p-frame/docs/pn_design.md` | PN 协议以及 client/server 参考说明 | pn |
| `p2p-frame/docs/ttp_module_design.md` | TTP 模块行为 | ttp |

## 当前改动直接映射
| Proposal 条目 | 设计对象 | 代码路径/接口 | 风险/回滚备注 |
|---------------|----------|---------------|----------------|
| relay 侧 `pn_server` 在成功握手后的双向字节桥接路径上增加按用户统计的流量计量 | relay bridge 上的 source/target 双边独立统计视图 | `p2p-frame/src/pn/service/pn_server.rs`、`PnTrafficManager`、统计查询接口 | 若实现阶段无法在不改变握手与 bridge 契约的前提下同时暴露 source/target 视图，应先退回 design 重新澄清统计模型，而不是把 target 统计挤进 source 视图。 |
| relay 侧 `pn_server` 在成功握手后的双向字节桥接路径上增加按用户生效的限速能力 | 仅 source 侧生效的用户级限速，与 target 统计视图解耦 | `p2p-frame/src/pn/service/pn_server.rs`、限速配置/查询接口 | 若实现阶段发现 target 统计可见性会迫使限速扩展成双边模型，应先退回 proposal，而不是静默扩大限速范围。 |
| `PnTunnel` idle timeout 生命周期关闭 | `PnTunnel` 本地状态机、channel lease 计数、idle sweeper 和关闭后重新创建 | `p2p-frame/src/pn/client/pn_tunnel.rs`、`p2p-frame/src/pn/client/pn_client.rs`、`p2p-frame/src/pn/client/pn_listener.rs` | 若实现阶段无法可靠追踪已返回给上层的 channel 生命周期，应先退回 design 重新划分 lease wrapper，而不是只统计 inbound queue。 |
| `PnTunnel` tunnel 级控制通道与远端关闭感知 | control channel ready gate、控制接收循环、heartbeat、close 命令和统一关闭状态机 | `p2p-frame/src/pn/protocol.rs`、`p2p-frame/src/pn/client/pn_client.rs`、`p2p-frame/src/pn/client/pn_listener.rs`、`p2p-frame/src/pn/client/pn_tunnel.rs`、必要 `p2p-frame/src/pn/service/pn_server.rs` bridge 路径 | 若实现阶段发现现有 `ProxyOpenReq` 无法无歧义承载 control channel，应扩展 PN protocol 的 control open 命令或 kind，而不是复用业务 stream/datagram kind 造成兼容歧义。 |
| 多 PN server 显式 relay 选择与 assigned target admission | PN client 侧在既有 `TunnelNetwork::create_tunnel_with_intent(...)` 实现内部通过自身配置的 resolver 按指定 relay PN 打开 proxy tunnel；PN server 默认构造保留 allow-all；显式策略构造用 assigned target admission 限制新建逻辑 tunnel target；relay session registry 允许同一逻辑 tunnel 后续双向 channel | `p2p-frame/src/pn/client/mod.rs`、`p2p-frame/src/pn/client/pn_client.rs`、`p2p-frame/src/pn/client/pn_listener.rs`、`p2p-frame/src/pn/client/pn_tunnel.rs`、`p2p-frame/src/pn/service/pn_server.rs`、`p2p-frame/src/ttp/client.rs`、`p2p-frame/src/ttp/server.rs`、`p2p-frame/src/tunnel/tunnel_manager.rs`、必要 `p2p-frame/src/stack.rs` | 若实现阶段需要库内目录、自动 PN 切换、跨 PN 二跳 bridge、让 `TunnelManager` 持有 PN route resolver，或用默认 allow-all 代替显式 assigned target 策略，应退回 proposal/design；若只是 route selector 形态不清，应退回 design。 |
| `TtpNode` 主动建链 | 新增 `TtpNode`，复用 `TtpClient` target tunnel 主动建链 helper 与 `TtpServer` incoming subscriber 逻辑；`open_stream(...)` / `open_control_stream(...)` / 第一版 `open_datagram(...)` 在无匹配可用 tunnel 时主动建链、attach、缓存并打开对应 channel | `p2p-frame/src/ttp/client.rs`、`p2p-frame/src/ttp/node.rs`、`p2p-frame/src/ttp/server.rs`、`p2p-frame/src/ttp/mod.rs`、必要 `p2p-frame/src/ttp/tests.rs` | 若实现阶段需要改变 `Tunnel` / `TunnelNetwork` trait、wire 协议、target directory、tunnel publish 或 `TtpServer` lookup-only 行为，应退回 proposal/design；若只是 helper 复用形态不清，可在 `ttp` 内部调整。 |
| `TtpClient` 连接生命周期 | 新增 `remove_server(&TtpTarget)` 精确删除保持连接 target；maintain loop 只基于当前 target 快照重连；为非保持 target cache tunnel 记录 idle 时间和 active/pending lease，超过默认 5 分钟且 lease 为 0 时仅释放本地 cache 引用 | `p2p-frame/src/ttp/client.rs`、必要 `p2p-frame/src/ttp/tests.rs` | 若实现阶段需要改变 `Tunnel` / `TunnelNetwork` trait、wire 协议、target matching、底层 tunnel close、tunnel publish、`NetManager` 候选删除或 `TtpServer` lookup-only 行为，应退回 proposal/design；若只是 lease wrapper 形态不清，应退回 design。 |
| 核心库的长期模块边界 | `TunnelManager` 的统一 register/publish 生命周期 | `p2p-frame/src/tunnel/tunnel_manager.rs` | 收敛 publish 逻辑时，优先保持 reverse waiter、候选复用和 proxy 升级语义不变；若实现阶段发现现有测试/运行时依赖旧的分散式时序，则先回滚到文档阶段补充约束。 |
| reverse incoming 无 waiter 关闭 | `TunnelManager` 的 incoming reverse waiter 判定与 close 分支 | `p2p-frame/src/tunnel/tunnel_manager.rs` | 若实现阶段发现无 waiter reverse 仍可能被合法接收，应退回 design 明确协议来源，而不是继续 publish 未等待的 reverse tunnel。 |
| 单 SN NAT 打洞优化 | direct/reverse 统一 300ms 竞速、本次反连候选、QUIC listener 同源 UDP punch、proxy 短窗口脱代理、按协议隔离 endpoint 评分 | `p2p-frame/src/tunnel/tunnel_manager.rs`、`p2p-frame/src/networks/quic/listener.rs`、`p2p-frame/src/networks/quic/network.rs`、`p2p-frame/src/sn/client/sn_service.rs`、必要 `p2p-frame/src/sn/service/service.rs` | 若实现阶段需要多 SN fanout、改变 `SnCallResp` 语义、解析 raw UDP 业务包、改变 `TunnelNetwork` trait 或引入 STUN/TURN，应退回 proposal；若只是候选结构、punch 调度或 socket clone 细节不清，应退回 design。 |
| `ServerReflexive` QUIC NAT keepalive | UDP punch candidate policy 收窄为 `EndpointArea::ServerReflexive` QUIC endpoint；QUIC tunnel heartbeat interval 保持现有值，heartbeat timeout 调整为 30 秒 | `p2p-frame/src/tunnel/tunnel_manager.rs`、`p2p-frame/src/networks/quic/tunnel.rs`、必要 `p2p-frame/src/networks/quic/listener.rs` 测试 | 若实现阶段需要改变 heartbeat interval、增加 raw UDP keepalive、让远端解析 punch payload 或修改公共 `TunnelNetwork` trait，应退回 proposal。 |
| 多个已有 tunnel candidate 的默认复用选择 | published 优先，非 proxy 优先于 proxy，同类候选内选择最新 | `p2p-frame/src/tunnel/tunnel_manager.rs` | 若实现阶段发现 proxy 仍可能覆盖已发布 direct/passive candidate，应优先修正 `get_tunnel()` 选择策略，而不是让后台脱代理升级成为唯一恢复路径。 |
| `EndpointArea::ServerReflexive` endpoint area 语义 | endpoint enum 命名、`S` 文本/codec 编码、SN 观察地址 `Wan` / `ServerReflexive` 分类、删除 `is_sys_default()` | `p2p-frame/src/endpoint.rs`、`p2p-frame/src/sn/service/service.rs`、必要 `p2p-frame/src/sn/**`、`p2p-frame/src/tunnel/**` | 若实现阶段发现必须继续兼容 `D` 或 `is_sys_default()`，应退回 design 明确兼容策略；若需要 STUN/TURN 或跨 SN NAT 类型推断，应退回 proposal。 |
| TCP listener 基于 `sfo-reuseport` 重构 | `TcpTunnelListener` 由本地 `TcpListener::accept()` 循环改为 `sfo_reuseport::TcpServer` handler 接收入站 `TcpStream`；stream 继续进入现有 TLS accept、control/data 分流、registry 和 publish 流程；`ServerRuntime` 从 stack/network 构造传入，未设置时默认创建。 | `p2p-frame/src/stack.rs`、`p2p-frame/src/networks/tcp/listener.rs`、`p2p-frame/src/networks/tcp/connection.rs`、`p2p-frame/src/networks/tcp/network.rs`、`docs/versions/v0.1/modules/p2p-frame/design/sfo-reuseport-listeners.md` | 若实现阶段需要改变 TCP tunnel 线协议、TLS 身份校验或公共 `TunnelNetwork` trait，应退回 proposal；若只是 close/task 语义不清，应退回 design。 |
| QUIC listener 基于 `sfo-reuseport` 重构 | `QuicTunnelListener` 使用 `sfo_reuseport::QuicServer::serve_socket(...)` 接收每个 worker 的 UDP socket，内部 Quinn `AsyncUdpSocket` 适配器直接包裹 `sfo_reuseport::UdpSocket` 并创建 per-worker `quinn::Endpoint::new_with_abstract_socket(...)`；worker endpoint 使用 `sfo_reuseport::QuicCidGenerator` 生成匹配 worker shard 的 CID；主动 connect 从 worker endpoint 集合选择一个 endpoint；同源 UDP punch 使用首个可用 worker socket。 | `p2p-frame/src/stack.rs`、`p2p-frame/src/networks/quic/listener.rs`、`p2p-frame/src/networks/quic/network.rs`、`docs/versions/v0.1/modules/p2p-frame/design/sfo-reuseport-listeners.md` | 若实现阶段需要新增 raw UDP tunnel、解析 punch payload、改变 QUIC NAT punch policy 或 heartbeat 语义，应退回 proposal；若只是 waker/backpressure/close 细节不清，应退回 design。 |
| `TunnelNetwork` listener 回调化 | 公共 `TunnelNetwork::listen(...)` 接收入站 tunnel 回调并返回 `P2pResult<()>`；公共 trait 移除 `listeners()`；`NetManager` 把回调接入原有 dispatch、validator、订阅发布和 reject close；TCP/QUIC/PN listener 对象降为 network 内部实现细节。 | `p2p-frame/src/networks/network.rs`、`p2p-frame/src/networks/net_manager.rs`、`p2p-frame/src/networks/tcp/network.rs`、`p2p-frame/src/networks/quic/network.rs`、`p2p-frame/src/pn/client/pn_client.rs`、必要调用点 | 若实现阶段发现仍有外部调用方必须直接持有 `TunnelListenerRef`，应退回 design 明确替代接口；不得保留 `listeners()` 作为兼容旁路。 |
| `Tunnel` stream/datagram listen 回调化 | 公共 `Tunnel` trait 移除 `accept_stream()` / `accept_datagram()`；`listen_stream(...)` / `listen_datagram(...)` 接收 stream/datagram 入站回调；TCP/QUIC/PN tunnel 在内部入站处理路径中触发回调；TTP、stream manager、datagram manager 和 PN server/client 迁移到回调模型。 | `p2p-frame/src/networks/tunnel.rs`、`p2p-frame/src/networks/tcp/tunnel.rs`、`p2p-frame/src/networks/quic/tunnel.rs`、`p2p-frame/src/pn/client/pn_tunnel.rs`、`p2p-frame/src/ttp/runtime.rs`、`p2p-frame/src/stream/stream_manager.rs`、`p2p-frame/src/datagram/datagram_manager.rs`、`p2p-frame/src/pn/service/pn_server.rs`、测试替身 | 若实现阶段需要保留公共 `accept_*`、改变线协议、改变 vport/purpose 编码或让回调阻塞 worker/control loop，应退回 design；不得以内部公共队列绕回旧轮询语义。 |
| `Tunnel` control stream API | 公共 `Tunnel` trait 新增 `open_control_stream(...)` / `listen_control_stream(...)`；内部 control stream runtime 通过 TCP/QUIC/PN 控制命令新增的 `Data` 命令承载私有多路复用 frame，单个 `Data` payload 最大 `64 KiB`，底层控制通道关闭时关闭所有 virtual control stream。 | `p2p-frame/src/networks/tunnel.rs`、`p2p-frame/src/networks/control_stream.rs`、`p2p-frame/src/networks/tcp/protocol.rs`、`p2p-frame/src/networks/tcp/tunnel.rs`、`p2p-frame/src/networks/quic/tunnel.rs`、`p2p-frame/src/pn/protocol.rs`、`p2p-frame/src/pn/client/pn_tunnel.rs`、`docs/versions/v0.1/modules/p2p-frame/design/tunnel-control-stream-api.md`、相关 tests | 若实现阶段需要公开内部 frame/runtime、让外部直接读写 raw 控制通道、把 control stream 扩展为大流量业务传输、或重写现有控制通道 ready/heartbeat/close/open 逻辑，应退回 proposal；若只是 frame/window/buffer 细节不清，应退回 design。 |
| SN control stream 信令 | SN report/call/called/response 或等价低频小消息通过固定 SN control purpose 使用 `Tunnel::open_control_stream(...)` / `listen_control_stream(...)`，不默认建立普通业务 `open_stream()`；bootstrap、控制通道不可用、远端未监听或旧版本不支持时当前 SN 命令通道创建/发送失败，不回退到普通 stream。 | `p2p-frame/src/sn/client/sn_service.rs`、`p2p-frame/src/sn/service/service.rs`、必要 `p2p-frame/src/sn/**`、`p2p-frame/src/networks/tunnel.rs`、`docs/versions/v0.1/modules/p2p-frame/design/sn-control-stream-signaling.md`、相关 tests | 若实现阶段需要公开内部 control stream 协议、改变 SN 命令语义、引入 SN 大流量数据平面、改变单 SN 边界或保留普通 stream fallback，应退回 proposal/design；若只是 purpose 常量或错误码不清，应退回 design。 |
| SN server 连接验证器 | `sn/service` 增加 validator type/helper 和显式自定义构造路径；默认构造安装 allow-all validator；report、call 或等价入站 SN command 在 handler 状态更新或转发前执行 validator，validator 输入只包含 `client_id` 与 `client_cert`，reject 时短路当前请求 | `p2p-frame/src/sn/service/service.rs`、必要 `p2p-frame/src/sn/service/**`、必要 tests | 若实现阶段需要新增 SN wire 字段、改变 `SnCallResp` 最终连通性语义、把 validator 扩展为认证/计费/限速/NAT 推断系统、向 validator 暴露 command/tunnel id/报文 from/to/endpoint 字段，或无法从已认证连接上下文与客户端证书构造 validator 输入，应退回 proposal/design；默认 allow-all 必须保持现有兼容行为。 |
| SN service contract/receipt 清理 | 删除未完整接入主流程的 `SnServiceContractServer`、`client/contract.rs`、`service/receipt.rs` 及相关构造装配和公开导出；保留 `sn/protocol` receipt wire 兼容结构；SN 基础 report/call/called、peer manager、连接验证器和 control-stream-only 信令路径保留。 | `p2p-frame/src/sn/service/service.rs`、`p2p-frame/src/sn/service/mod.rs`、`p2p-frame/src/sn/service/receipt.rs`、`p2p-frame/src/sn/client/contract.rs`、`p2p-frame/src/sn/client/mod.rs`、必要下游启动调用点 | 若实现阶段发现 contract/receipt 服务逻辑仍是 SN report/call/called 或 validator 的必需依赖，应退回 design 明确依赖拆分；不得通过删除 SN 基础能力、改变 SN wire 语义或在相邻模块重建兼容旁路来完成清理。 |
| bounded channel 容量配置化 | 顶层配置按位置默认 `1024`，用户默认无需设置，外部可只覆盖某一位置；生产路径 `unbounded_channel` 替换为 bounded channel；底层构造只接收对应位置已解析容量，不定义默认值；满载时按路径返回错误、关闭迟到 channel/tunnel 或移除订阅。 | `p2p-frame/src/stack.rs`、`p2p-frame/src/networks/net_manager.rs`、`p2p-frame/src/tunnel/tunnel_manager.rs`、`p2p-frame/src/networks/tcp/tunnel.rs`、`p2p-frame/src/networks/tcp/listener.rs`、`p2p-frame/src/networks/quic/tunnel.rs`、`p2p-frame/src/networks/quic/listener.rs`、`p2p-frame/src/networks/quic/network.rs`、`p2p-frame/src/ttp/registry.rs`、`p2p-frame/src/ttp/listener.rs`、`p2p-frame/src/pn/**` 中现有 mpsc queue 使用点 | 若某条路径需要无限缓存才能保持语义，应退回 proposal；若只是满载错误码、同步/异步发送边界或构造传参不清，应退回 design，而不是保留 unbounded channel。 |

## Directly Mapped Change Items
| change_id | proposal_id | Design Coverage | Scope Paths | Risk / Rollback Notes |
|-----------|-------------|-----------------|-------------|-----------------------|
| endpoint_area_server_reflexive | P-ENDPOINT-AREA-1 | 将 `EndpointArea::Default` 重命名为 `ServerReflexive`；`Display`/`FromStr` 使用 `S`，raw codec 保持 area bit 位置但更新语义；SN 观察地址与节点自上报 endpoint 完全一致时标记 `Wan`，否则标记 `ServerReflexive`；删除 `is_sys_default()` 并保持 `is_static_wan()` 只覆盖 `Wan` / `Mapped`。 | `p2p-frame/src/endpoint.rs`、`p2p-frame/src/sn/service/service.rs`、必要 `p2p-frame/src/sn/**`、`p2p-frame/src/tunnel/**`、`docs/versions/v0.1/modules/p2p-frame/design/tunnel-nat-traversal.md` | 该变更影响公开枚举和文本编码；若下游依赖 `D` 或旧 method，回滚应恢复旧 enum/codec 并退回 proposal 重新定义兼容窗口。 |
| server_reflexive_quic_nat_keepalive | P-QUIC-SR-NAT-KEEPALIVE-1 | UDP punch 只由 `EndpointArea::ServerReflexive` QUIC candidate 在 SN service 存在且满足非 LAN IPv4、非 0 端口基本条件时开启；`Lan`、`Wan`、`Mapped`、TCP、IPv6、0 端口和默认 intent 路径不触发 punch；QUIC tunnel heartbeat interval 保持现有值，heartbeat timeout 调整为 30 秒。 | `p2p-frame/src/tunnel/tunnel_manager.rs`、`p2p-frame/src/networks/quic/tunnel.rs`、必要 `p2p-frame/src/networks/quic/listener.rs` 测试 | 该变更收窄 punch 触发面并放宽 QUIC heartbeat timeout；若回滚，应恢复旧 candidate policy 和旧 timeout 常量，但不得重新引入 raw UDP 协议或公共 trait 参数。 |
| reverse_timeout_close_late_tunnel | P-REV-TIMEOUT-1 | `TunnelManager` 对 incoming reverse 先消费同 `(remote_id, tunnel_id)` pending waiter；命中 waiter 时只 notify，后续由 reverse open 接收方 register 并 publish；无 waiter 时关闭 tunnel 并跳过 register/publish。 | `p2p-frame/src/tunnel/tunnel_manager.rs`、`docs/versions/v0.1/modules/p2p-frame/design/tunnel-publish-lifecycle.md` | 该变更收窄 reverse incoming 可见性；若回滚，应恢复无 waiter reverse publish，但不得影响正常 waiter 命中后 publish 或非 reverse incoming publish。 |
| networks_sfo_reuseport_tcp_listener | P-SFO-TCP-LISTENER-1 | TCP listener 使用 `sfo_reuseport::TcpServer` 注册服务，handler 接收 `sfo_reuseport::TcpStream` 后调用现有 TLS accept 与 TCP control/data 分流；`TcpTunnelListener` 保存 `TcpServer` handle 并在 close 时停止服务；`P2pStackConfig` 可显式注入 `sfo_reuseport::ServerRuntime`，默认路径自动创建 runtime。 | `p2p-frame/src/stack.rs`、`p2p-frame/src/networks/tcp/listener.rs`、`p2p-frame/src/networks/tcp/connection.rs`、`p2p-frame/src/networks/tcp/network.rs`、`docs/versions/v0.1/modules/p2p-frame/design/sfo-reuseport-listeners.md` | 回滚时恢复本地 `TcpListener` bind/accept 路径和旧 `reuse_address` 设置，但不得改变 TCP tunnel 线协议或 publish 语义。 |
| networks_sfo_reuseport_quic_listener_socket | P-SFO-QUIC-LISTENER-1 | QUIC listener 使用 `sfo_reuseport::QuicServer::serve_socket(...)` 注册服务；每个 `(UdpSocket, worker_id)` 回调创建一个 Quinn endpoint，`AsyncUdpSocket::poll_recv()` / `try_send()` / `UdpPoller` 分别委托给 `UdpSocket::poll_recv_from_vectored(...)`、`try_send_to(...)` 和 `poll_send_ready(...)`；endpoint CID generator 使用 `QuicCidGenerator::for_worker(worker_id)`；所有 endpoint accept 结果汇入同一 listener 队列；主动 connect 可选择任一 endpoint；UDP punch 保存第一个 worker socket；`QuicTunnelListener` close 同时关闭 `QuicServer` 和所有 Quinn endpoint。 | `p2p-frame/src/stack.rs`、`p2p-frame/src/networks/quic/listener.rs`、`p2p-frame/src/networks/quic/network.rs`、`docs/versions/v0.1/modules/p2p-frame/design/sfo-reuseport-listeners.md` | 回滚时恢复直接 UDP socket + `quinn::Endpoint::new(...)` 路径，但不得引入独立 punch socket、raw UDP tunnel 或改变 NAT punch/heartbeat 语义。 |
| tunnel_network_listen_callback | P-TUNNEL-NETWORK-CALLBACK-1 | `TunnelNetwork::listen(...)` 由返回 `TunnelListenerRef` 改为接收 `IncomingTunnelCallback` 并返回 `P2pResult<()>`；TCP/QUIC/PN network 保存内部 listener 并在 accept 到新 tunnel 后调用回调；`NetManager` 注册回调并复用原有 dispatch/validator/publish 逻辑；公共 trait 删除 `listeners()`。 | `p2p-frame/src/networks/network.rs`、`p2p-frame/src/networks/net_manager.rs`、`p2p-frame/src/networks/tcp/network.rs`、`p2p-frame/src/networks/quic/network.rs`、`p2p-frame/src/pn/client/pn_client.rs`、`p2p-frame/src/stack.rs`、相关 tests | 回滚时恢复 `listen(...) -> P2pResult<TunnelListenerRef>` 和 `listeners()`，但必须同步回滚 NetManager/stack 调用点；不得改变 tunnel publish 或线协议语义。 |
| tunnel_stream_datagram_listen_callback | P-TUNNEL-CHANNEL-CALLBACK-1 | `Tunnel` trait 删除 `accept_stream()` / `accept_datagram()`；新增或调整 `IncomingStreamCallback`、`IncomingDatagramCallback` 等等价回调类型；`listen_stream(...)` / `listen_datagram(...)` 保存 listen 规则和回调；TCP/QUIC/PN tunnel 入站 stream/datagram dispatch 按状态、listen 规则和 bounded capacity 触发回调；TTP、stream manager、datagram manager 和 PN server/client 迁移到回调消费模型。 | `p2p-frame/src/networks/tunnel.rs`、`p2p-frame/src/networks/tcp/tunnel.rs`、`p2p-frame/src/networks/quic/tunnel.rs`、`p2p-frame/src/pn/client/pn_tunnel.rs`、`p2p-frame/src/ttp/runtime.rs`、`p2p-frame/src/stream/stream_manager.rs`、`p2p-frame/src/datagram/datagram_manager.rs`、`p2p-frame/src/pn/service/pn_server.rs`、`docs/versions/v0.1/modules/p2p-frame/design/tunnel-channel-listen-callback.md`、相关 tests | 回滚时恢复公共 `accept_*` 和旧 accept loops，并同步回滚 TTP/manager 调用点；不得留下既有回调又有公共 accept 的双入口状态。 |
| tunnel_control_stream_api | P-TUNNEL-CONTROL-STREAM-API-1 | `Tunnel` trait 新增 `open_control_stream(...)` / `listen_control_stream(...)` 和 control stream callback 类型；新增 `networks` 私有 control stream runtime 负责 frame、stream id、buffer/window、open/listen 和关闭传播；TCP/QUIC/PN 控制命令各新增单一 `Data` 命令承载 runtime 私有 frame，`Data` payload 最大 `64 KiB`；底层控制通道断开时关闭所有派生 control stream。 | `p2p-frame/src/networks/tunnel.rs`、`p2p-frame/src/networks/control_stream.rs`、`p2p-frame/src/networks/tcp/protocol.rs`、`p2p-frame/src/networks/tcp/tunnel.rs`、`p2p-frame/src/networks/quic/tunnel.rs`、`p2p-frame/src/pn/protocol.rs`、`p2p-frame/src/pn/client/pn_tunnel.rs`、`docs/versions/v0.1/modules/p2p-frame/design/tunnel-control-stream-api.md`、相关 tests | 回滚时删除公共 `open_control_stream` / `listen_control_stream`、内部 runtime 和 TCP/QUIC/PN `Data` 命令；不得留下公开 frame/runtime 类型或让 control stream 数据继续占用内部控制命令通道。 |
| sn_control_stream_signaling | P-SN-CONTROL-STREAM-1 | SN report、call、called、response 或等价低频小消息使用固定 SN control purpose 调用 `Tunnel::open_control_stream(...)` / `listen_control_stream(...)`；不默认建立普通业务 `open_stream()`；bootstrap、控制通道不可用、远端未监听或旧版本不支持时当前 SN 命令通道创建/发送失败，不回退到普通 stream。 | `p2p-frame/src/sn/client/sn_service.rs`、`p2p-frame/src/sn/service/service.rs`、必要 `p2p-frame/src/sn/**`、`p2p-frame/src/networks/tunnel.rs`、`docs/versions/v0.1/modules/p2p-frame/design/sn-control-stream-signaling.md`、相关 tests | 回滚时恢复 SN 默认普通 stream 信令路径并删除 SN control purpose 注册/发送逻辑；不得删除通用 `Tunnel` control stream API，也不得改变 SN 命令、单 SN、endpoint 分类或 `SnCallResp` 语义。 |
| sn_server_connection_validator | P-SN-SERVER-CONNECTION-VALIDATOR-1 | `sn/service` 提供连接验证器装配点、默认 allow-all helper 和自定义 validator 构造路径；validator 以只含 `client_id` 与 `client_cert` 的规范化客户端上下文作为输入；SN server 在 report、call 或等价入站请求进入 handler 前解析客户端证书并确认其 id 等于 cmd tunnel peer id，随后执行 validator，reject 时短路当前请求。 | `p2p-frame/src/sn/service/service.rs`、必要 `p2p-frame/src/sn/service/**`、相关 tests | 回滚时移除自定义 validator 构造路径并恢复默认无准入短路行为；不得改变 SN command 线协议、`SnCallResp` 语义、endpoint 分类、单 SN 边界或 SN control stream 信令选择；不得把 command、tunnel id、reported peer、target peer、来源 endpoint 或其他报文载荷派生字段重新加入 validator 上下文。 |
| remove_sn_service_contract_server | P-SN-CONTRACT-CLEANUP-1 | 删除 `SnServiceContractServer` 相关服务合约/回执生产路径和公开导出；`sn/service` 不再构造或启动 contract server，`sn/client` 不再导出 contract helper；`sn/protocol` receipt wire 兼容结构保留；SN report/call/called、peer manager、连接验证器和 control-stream-only 信令继续按现有设计工作。 | `p2p-frame/src/sn/service/service.rs`、`p2p-frame/src/sn/service/mod.rs`、`p2p-frame/src/sn/service/receipt.rs`、`p2p-frame/src/sn/client/contract.rs`、`p2p-frame/src/sn/client/mod.rs`、必要下游启动调用点和 tests | 回滚时恢复 contract/receipt 文件、公开导出和 service 构造装配；不得借回滚改变 SN command 线协议、`SnCallResp` 语义、validator 上下文、endpoint 分类或 control stream 信令选择。 |
| pn_multi_server_assigned_target | P-PN-MULTI-SERVER-ASSIGNED-TARGET-1 | PN client active open 必须在既有 `TunnelNetwork::create_tunnel_with_intent(...)` 实现内部使用自身 resolver 得到上层指定的 relay PN server id/route，并通过既有 `TtpConnector::open_stream(...)` target 接口连接该 relay 的 `PROXY_SERVICE`；`TunnelManager` 不持有 PN route resolver，stack 不通过 latest tunnel 隐式选择 PN；`PnServer::new(...)` 默认安装显式 allow-all validator；显式 assigned target validator / policy 路径拒绝非本 PN assigned target 的新建 logical tunnel；成功建立后记录 relay session，后续相同 `(tunnel_id, endpoint pair)` 的双向 channel 可复用该 session 而不重新按 target 分配拒绝。 | `p2p-frame/src/pn/client/mod.rs`、`p2p-frame/src/pn/client/pn_client.rs`、`p2p-frame/src/pn/client/pn_listener.rs`、`p2p-frame/src/pn/client/pn_tunnel.rs`、`p2p-frame/src/pn/service/pn_server.rs`、`p2p-frame/src/ttp/client.rs`、`p2p-frame/src/ttp/server.rs`、`p2p-frame/src/tunnel/tunnel_manager.rs`、必要 `p2p-frame/src/stack.rs`、`docs/versions/v0.1/modules/p2p-frame/design/pn-server.md` | 回滚时恢复 explicit route / admission 前的 implicit proxy client 选择；不得删除默认 allow-all PN server 构造，也不得让默认 allow-all 掩盖显式策略下的错误 PN 失败。 |
| ttp_node_active_open | P-TTP-NODE-ACTIVE-OPEN-1 | 新增 `TtpNode` 类型和 `TtpNodeRef`；它实现 `TtpPortListener` / `TtpConnector`，注册 incoming tunnel subscriber 并将入站 tunnel attach/cache；`open_stream(...)`、`open_control_stream(...)` 和第一版 `open_datagram(...)` 通过共享 `get_or_create_tunnel(target)` 复用已有可用 tunnel或按 target endpoint 主动创建、attach、缓存 tunnel 后打开对应 channel；`TtpServer` lookup-only open 行为保持不变。 | `p2p-frame/src/ttp/client.rs`、`p2p-frame/src/ttp/node.rs`、`p2p-frame/src/ttp/server.rs`、`p2p-frame/src/ttp/mod.rs`、`p2p-frame/src/ttp/tests.rs` | 回滚时删除 `TtpNode` 导出和测试，保留 `TtpClient` / `TtpServer` 现有行为；不得改变 `Tunnel` / `TunnelNetwork` trait、wire 协议、target matching 或 `TtpServer` lookup-only 语义。 |
| ttp_client_connection_lifecycle | P-TTP-CLIENT-CONNECTION-LIFECYCLE-1 | `TtpClient::remove_server(&TtpTarget)` 按 `(remote_id, remote_ep)` 精确幂等删除 maintained target；maintain loop 每轮读取当前集合快照，删除后的 target 不再自动重建；`TtpClient` cache 记录 maintained 标记、last_used 和 active/pending lease，`open_*` 期间持有 lease，non-maintained 且 lease 为 0 的 cache entry 超过默认 5 分钟 idle timeout 后只释放本地引用；maintained target 和 active/pending target 均保留。 | `p2p-frame/src/ttp/client.rs`、`p2p-frame/src/ttp/tests.rs` | 回滚时删除 `remove_server` 和 lease/idle metadata，恢复简单 `HashMap<P2pId, TunnelRef>`；不得改变 traits、wire、target matching、底层 close 或 `TtpServer` lookup-only 行为。 |
| bounded_channel_capacity_config | P-BOUNDED-CHANNELS-1 | 在 `P2pConfig` / `P2pStackConfig` 顶层提供分位置 channel 容量配置，每项默认 `1024`，用户默认无需设置，并通过 `P2pEnv` 和各构造函数把对应位置容量传入底层；所有生产路径 `mpsc::unbounded_channel`、`UnboundedSender`、`UnboundedReceiver` 替换为 bounded `mpsc::channel`、`Sender`、`Receiver`；同步分发路径使用 `try_send` 并将满载转成错误、关闭或订阅清理，异步可背压路径使用 `send(...).await`。 | `p2p-frame/src/stack.rs`、`p2p-frame/src/networks/**`、`p2p-frame/src/tunnel/tunnel_manager.rs`、`p2p-frame/src/ttp/**`、`p2p-frame/src/pn/**` 中现有 unbounded mpsc 使用点和对应测试替身 | 回滚时恢复 unbounded mpsc 类型和构造传参，但不得留下半迁移容量 API；若保留顶层配置，应退回 proposal/design 明确兼容目标。 |
| stack_channel_capacity_config_removal | P-STACK-CHANNEL-CAPACITY-REMOVAL-1 | 删除 `ChannelCapacityConfig` 结构和 stack 层容量 getter/setter；`P2pEnv` 不再保存容量快照，`P2pStackConfig` 不再从 env 继承或覆盖容量；`NetManager` 不再保存或暴露容量；`QuicTunnelNetwork::new(...)` 不接收容量参数；`TtpRuntime::new()` 改为无参数，`TtpClient` / `TtpServer` 不再为 TTP runtime 读取 `NetManager` 容量；`PnClient` 删除显式容量构造入口，stack 默认 proxy client 不再传入容量。 | `p2p-frame/src/stack.rs`、`p2p-frame/src/networks/net_manager.rs`、`p2p-frame/src/networks/quic/network.rs`、`p2p-frame/src/ttp/runtime.rs`、`p2p-frame/src/ttp/client.rs`、`p2p-frame/src/ttp/server.rs`、`p2p-frame/src/pn/client/pn_client.rs`，以及仅当编译需要时调整直接依赖被删除容量 API 的调用点 | 回滚时恢复 `ChannelCapacityConfig`、env/config 字段、setter/getter、`NetManager` 容量字段/构造参数、`QuicTunnelNetwork::new(..., capacity)`、`TtpRuntime::new(capacity)` 和 PN 显式容量构造入口；不得借回滚恢复 unbounded channel 或改变底层队列满载语义。 |

## Key Decisions
| Decision | Chosen | Alternatives Considered | Rejection Reason |
|----------|--------|-------------------------|------------------|
| PN relay selection | caller-supplied relay PN route | library-owned peer-to-PN directory or latest tunnel fallback | directory/switching is out of scope; latest fallback hides wrong PN errors |
| PN target admission | default allow-all plus explicit assigned target policy constructor | making assigned target policy mandatory for every `PnServer` | mandatory policy breaks existing unconfigured deployments; explicit policy still protects multi-PN deployments |
| Existing PN reverse channel handling | in-process relay session registry | re-run assigned target check on every channel | would reject legitimate reverse channel on established logical tunnel |
| Multi PN bridge shape | single relay `A -> PN-B -> B` | cross PN `A -> PN-A -> PN-B -> B` bridge | expands protocol/runtime complexity beyond proposal |
| Control stream implementation | private shared runtime with public `Tunnel` methods only | expose internal frame/runtime | public subprotocol would freeze internals and couple callers |
| TTP active-open surface | new `TtpNode` type | change `TtpServer` to create missing tunnels | mutating `TtpServer` would break lookup-only server semantics and old callers that depend on `NotFound` |
| TTP client idle release scope | local cache release after active/pending leases reach zero and idle timeout elapses | close every matching underlying tunnel or add a public tunnel idle API | global close could affect other owners and public idle API would expand the tunnel trait beyond the proposal |

## Data and State
| Data or State | Owner Submodule | Access For Others | State Transitions |
|---------------|-----------------|-------------------|-------------------|
| PN relay route input | `pn` / stack caller boundary | configured on `PnClient`; no repository-owned peer directory storage | absent -> param/config error; present -> open specified relay; relay unavailable -> open failure |
| assigned target policy | `pn` | injected into explicit `PnServer` validator / policy path; only admission calls it | default constructor -> allow-all; explicit policy present -> accept/reject per target; policy update semantics owned by caller |
| relay session registry | `pn` | internal to `pn/service`; no cross-PN access | absent -> new-tunnel admission; registered after success; removed on bridge/control close, timeout or error |
| tunnel candidate registry | `tunnel` | accessed through `TunnelManager` APIs | pending -> published/rejected/closed; reverse waiter hit delays publish; no waiter closes |
| control stream runtime state | `tunnel_control_stream` | transport adapters call public/private runtime methods | open/listen -> data/fin/reset -> closed/error on transport close |
| TTP node tunnel cache | `ttp` | internal to `TtpNode`; other modules access it only through `TtpConnector` / `TtpPortListener` | absent -> create/attach/cache; available -> reuse; unavailable -> retain cleanup then recreate; attach/open failure -> no cache insert or error return |
| TTP client maintained targets | `ttp` | internal to `TtpClient`; modified by `connect_server(...)` / `remove_server(...)` | absent -> added by connect; present -> idempotent connect; present -> removed by remove; removed -> skipped by maintain loop |
| TTP client tunnel cache entries | `ttp` | internal to `TtpClient`; accessed through `TtpConnector` / `TtpPortListener` | absent -> create/attach/cache; non-maintained idle with zero leases -> local cache release; maintained target -> retained/recreated by maintain loop; active/pending leases -> retained |
| channel capacity default | `channel_capacity_config` | construction paths consume fixed default | fixed `1024`; no public override after cleanup |

## Testability
- Isolation seams per submodule: TTP active open and TTP client lifecycle can be tested with fake `NetManager` / tunnel network, in-memory tunnel handles, and test-only short idle deadlines; PN server admission can be tested with fake assigned target policy and fake target stream factory; PN client relay route can be tested with fake TTP connector target behavior; relay session registry can be tested without real network.
- Replaceable external boundaries: `sfo-io` accounting/limiting remains behind PN service adapter; `sfo-reuseport` listener behavior remains behind network listener tests; SN validator uses injected validator.
- How error/boundary cases will be triggered: TTP missing tunnel by starting with an empty node cache, tunnel reuse by pre-remembering an available tunnel, maintained target removal by deleting a target before a maintain tick, non-maintained idle release by advancing a test-only short idle timeout, active/pending retention by holding stream/control/datagram guards, attach/open failure by fake tunnel/network errors, wrong PN by policy reject before target open, missing route by `PnClient` configuration/parameter error, existing reverse channel by pre-registered session, no session by registry miss, full queues by small bounded capacity test config where applicable.
- Untriggerable failure paths and their alternative verification: real multi-server deployment churn and directory migration remain library-user responsibility; verify by absence of repository-owned directory/schema and by explicit route-only APIs.

## Risks and Rollback
- 协议或传输改动可能破坏所有下游 crate。
- 面向运行时或密码学的回归需要比孤立工具改动更强的回滚姿态。
- `Tunnel` stream/datagram 回调化影响公共 API 和所有 tunnel 消费者；回滚必须成组恢复 trait 签名、TCP/QUIC/PN tunnel 实现、TTP/stream/datagram/PN 调用点和测试替身，避免出现回调与公共 accept 双入口并存。
- `Tunnel` control stream API 会在现有控制命令通道上承载外部控制数据；回滚必须成组删除 trait 方法、callback 类型、内部 runtime、TCP/QUIC/PN `Data` 命令和测试覆盖，避免出现公开 API 仍存在但某个 transport 无法承载或控制通道继续解析未知 `Data` 的半迁移状态。
- SN control stream 信令依赖通用 `Tunnel` control stream API；回滚 SN 迁移时只恢复 SN 默认普通 stream 信令选择，不应删除通用 control stream API 或改变 TCP/QUIC/PN control stream adapter；本轮实现不得同时保留普通 stream fallback。
- `TtpNode` 复用现有 TTP runtime 和 target tunnel 建链路径；若实现导致 `TtpServer` lookup-only 语义变化、重复 attach、错误复用 target tunnel 或绕过 `NetManager`，必须回滚 `TtpNode` 新增导出和实现，而不是改变底层 tunnel trait 或协议。
- `TtpClient` 生命周期管理有并发风险：maintain loop 快照、remove_server、open_* 和 idle sweep 可能交错。实现必须通过同一 mutex 保护 maintained target 与 cache metadata 的判定，或保证跨锁顺序不会在 await 时持有 mutex；若无法证明 active/pending 计数正确，应退回 design，而不是强制释放 cache。
- SN server 连接验证器位于 `sn/service` 入站 handler 前；若 `client_id`、`client_cert` 来源不清、证书解析 id 与 cmd tunnel peer id 一致性不清，或 reject 错误映射不清，应退回 design，而不是在实现中直接信任客户端 payload 字段。回滚必须恢复默认所有客户端可连接行为。
- `SnServiceContractServer` 清理会删除未完整接入主流程的 service contract/receipt 文件和导出；若下游仍依赖这些公开项，必须退回 proposal/design 明确兼容窗口，而不是在相邻模块重建旁路。回滚必须成组恢复文件、导出和 service 构造装配，但不得改变 SN 基础命令、protocol receipt wire 兼容结构、validator 或 control stream 信令。
- 多 PN server 支持容易混淆默认 allow-all 兼容路径与显式 assigned target 策略路径；实现必须让旧默认调用点继续可用，同时让多 PN 策略调用点显式拒绝错误 PN。若实现阶段发现某个路径仍必须隐式选择 PN server，应退回 proposal/design 重新定义迁移窗口，而不是保留 latest tunnel fallback。
- bounded channel 会把历史积压转为背压或满载错误；回滚必须成组恢复 sender/receiver 类型、构造传参和满载错误映射，避免出现 sender bounded、receiver unbounded 或容量配置无法生效的半迁移状态。
- `ChannelCapacityConfig` 删除是公开 stack 配置 API 清理；若下游仍依赖该 API，应退回 proposal/design 明确兼容窗口，而不是在实现中保留半公开容量结构。
- 回滚应优先撤销具体实现改动，同时保留已批准的 proposal/design/testing 证据，为下一次尝试复用。

## Approval Record
- approver: user
- approval_date: 2026-06-14T00:00:00+08:00
- user_statement: 确认，实现吧，自动处理后续步骤
