---
module: p2p-frame
version: v0.1
status: approved
approved_by: auto-pipeline
approved_at: 2026-06-02T11:14:58+08:00
---

# p2p-frame 设计

> 该数据包为治理现有核心库定义设计基线。现有协议说明继续作为参考文档存在，并在此处建立索引。

## 设计范围
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
- 为本轮 bounded channel 容量配置化需求建立可执行设计边界：`P2pConfig` / `P2pStackConfig` 作为顶层配置入口提供按位置拆分的容量配置，各项默认 `1024`，调用方默认无需设置；`NetManager`、`TunnelManager`、TTP registry、QUIC listener connect queue 和 PN 相关内部队列只接收对应位置已解析容量，不在底层定义默认值；TCP/QUIC tunnel 的入站 stream/datagram 已改为回调交付，不再保留旧 accept queue 容量参数；所有生产路径 `tokio::sync::mpsc::unbounded_channel` 均替换为 bounded channel，并明确满载时的背压、错误或关闭语义。

### 非目标
- 对 `p2p-frame/docs/` 下已存在的每个协议细节做完整重写
- 在 harness 启动改造阶段重组源码文件
- 引入多 SN fanout、跨 SN NAT 类型推断、完整 STUN/TURN 协议栈或二层虚拟局域网语义
- 引入可被上层消费的原生 UDP tunnel、UDP payload 业务协议、raw UDP 接收解析器或独立于 QUIC listener 端口的新 UDP 打洞 socket
- 保留 `Default` 作为 endpoint area 名称、继续接受 `D` 作为新语义编码，或把 SN 观察地址无条件提升为静态 `Wan`
- 新增 `NetworkServerRuntime`、socket factory trait 或其他通用运行时抽象来包裹 `sfo-reuseport`
- 因 `sfo-reuseport` 重构或 listener 回调化而改变 TCP/QUIC tunnel 线协议、TLS 身份校验、QUIC NAT punch 策略、heartbeat 语义或 tunnel publish 规则
- 因 `Tunnel` stream/datagram 回调化而改变 TCP/QUIC/PN/TTP 线协议、TLS 身份校验、PN proxy channel 协议、业务 payload 格式、vport/purpose 编解码或 tunnel publish 规则
- 为 bounded channel 引入独立配置模块、全局静态默认值或底层局部默认值
- 通过额外 unbounded buffer、后台无限 Vec 队列或隐藏转发任务绕开 bounded channel 容量限制

## 总体方案
- 将 `p2p-frame` 视为 Tier 0 核心模块。
- 把本文件作为顶层设计索引。
- 将 `p2p-frame/docs/` 下现有协议说明视为传输、tunnel、PN、SN 和 TTP 行为的从属设计证据。
- 未来工作按直接子模块拆分，并赋予真实的责任边界与验证边界。

## 模块拆分
| 子模块 | 类型 | 职责 | 输入 | 输出 | 依赖 | 是否独立文档 |
|--------|------|------|------|------|------|----------------|
| `networks` | core transport | TCP/QUIC 监听器、endpoint、validator、QUIC listener 同源 UDP punch burst 和底层网络行为 | sockets、runtime、TLS | 传输事件和 tunnel plumbing | runtime、TLS | no |
| `tunnel_channel_callback` | public tunnel API | `Tunnel` stream/datagram listen 回调类型、公共 trait 签名、入站 channel 回调交付和关闭/背压语义 | tunnel inbound channel、listen vports、callbacks | stream/datagram callback invocation | `networks`、`ttp`、`pn`、`stream`、`datagram` | no |
| `tunnel` | orchestration | tunnel 生命周期、连接选择、统一 register/publish 生命周期、proxy 回退与后续脱代理升级行为 | 传输事件、身份、发现能力 | active/passive/proxy tunnel 状态 | `networks`、`finder`、`pn`、`sn` | yes |
| `ttp` | protocol | tunnel 上的命令和流复用协议 | tunnel IO | 带帧的命令/流行为 | `tunnel` | no |
| `sn` | service | 对端注册、信令和调用转发 | tunnel/ttp、身份 | SN 服务行为 | `ttp`、`p2p_identity` | no |
| `pn` | service | proxy-node 中继行为 | tunnel/ttp | 基于 relay 的连通性 | `tunnel`、`ttp` | yes |
| `finder` | support | 设备与 outer device 查询缓存 | endpoints、设备元数据 | 发现辅助能力 | `stack` | no |
| `identity_tls` | support | P2P 身份、TLS、X509 和密码学辅助逻辑 | keys、certs、握手元数据 | 已认证连接 | `tls`、`x509`、`p2p_identity` | no |
| `stack_runtime` | assembly | 高层 stack 编排和运行时抽象 | 所有下层 | 端到端 P2P 栈 | 几乎全部子模块 | no |
| `channel_capacity_config` | shared runtime config | bounded channel 分位置容量配置、顶层默认值和构造路径传递 | `P2pConfig` / `P2pStackConfig` | 按用途拆分的已解析 `usize` 容量 | `stack_runtime`、`networks`、`tunnel`、`ttp`、`pn` | no |

## 实现顺序
| 阶段 | 目标 | 前置条件 | 输出 | 依赖 | 可并行 |
|------|------|----------|------|------|--------|
| 1 | 确认 proposal 范围和直接子模块 | 已批准 proposal | 稳定的模块拆分 | proposal | no |
| 2 | 为每个直接子模块定义或更新 testing 覆盖 | 已批准的 design 拆分 | `testing.md`、`testplan.yaml` | 阶段 1 | limited |
| 3 | 在硬性准入检查下实现子模块改动 | 已批准 testing | 代码与测试 | 阶段 1-2 | yes |
| 4 | 依据 proposal 审计证据链 | implementation 证据已就绪 | acceptance report | 阶段 3 | no |

## 接口与依赖
### 公共接口摘要
- `p2p-frame` 暴露核心网络和 tunnel 栈，供 `cyfs-p2p` 与运行时二进制消费。
- 直接子模块契约必须持续与当前协议说明和公开 crate 导出保持一致。
- `Tunnel` 公共 trait 的入站 stream/datagram 消费入口为 `listen_stream(vports, callback)` 与 `listen_datagram(vports, callback)`；公共 trait 不再提供 `accept_stream()` 或 `accept_datagram()`。
- stream 回调输入为 `P2pResult<(TunnelPurpose, TunnelStreamRead, TunnelStreamWrite)>` 或等价命名类型；datagram 回调输入为 `P2pResult<(TunnelPurpose, TunnelDatagramRead)>` 或等价命名类型。回调类型必须可 `Clone + Send + Sync + 'static`，返回 `Send + 'static` 的异步 future，且不得把 TLS、PN 加密模式或线协议参数加入通用 trait。

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
- `P2pConfig` 必须新增顶层 `ChannelCapacityConfig` 或等价配置快照，按队列用途至少区分 QUIC listener connect、TTP listener registry、TunnelManager subscription、NetManager incoming subscriber 和 PN 内部队列；每项默认值为 `1024`，调用方默认不需要设置，并可只覆盖单个位置。`create_p2p_env(...)` 必须把对应容量传入仍存在 bounded queue 的底层组件。
- `P2pEnv` 必须保存已解析的分位置 channel 容量配置，作为 stack 层继续传递的唯一来源；`P2pStackConfig` 必须从 `P2pEnv` 继承该配置快照，并在创建默认 `PnClient`、`SNClientService`、`TunnelManager` 及其他 stack 组装路径时传入对应位置容量。底层构造函数只接收 `usize` 容量或显式配置快照，不得在构造函数内部使用 `unwrap_or(1024)`、`const DEFAULT_*` 或其他默认兜底。
- TCP 与 QUIC tunnel 的 inbound stream/datagram 不再通过旧 accepted queue 暴露给调用方；实现不得保留旧 `channel_capacity` 构造参数或为该路径新增隐藏 queue 来模拟旧轮询入口。远端或控制循环投递 inbound channel 时，必须按 listen 回调、关闭状态和协议拒绝路径收敛。
- `TunnelManager` 的 subscription receiver、`NetManager` incoming subscriber、TTP listener registry、QUIC listener connect request queue 和 PN service/test 内部 accept queue 必须分别使用顶层配置中对应位置的容量。对不允许阻塞的同步分发路径使用 `try_send` 并把满载转化为错误、关闭或移除订阅；对天然异步且调用方可等待的路径可以使用 `send(...).await`，但不得持有会造成死锁的 mutex guard 跨 await。
- `TtpRegistry::register(...)` 必须接收 capacity 参数或在 `TtpClient` / listener 构造时绑定容量；同一 purpose 重复注册语义保持不变。满载时，新的 stream/datagram 投递必须返回错误给 tunnel/ttp 分发路径，而不是丢弃成功状态。
- 测试替身和 `#[cfg(test)]` helper 可以使用显式小容量来验证满载行为；如果测试需要默认容量，必须从顶层配置或测试专用 helper 参数传入，不得在底层类型中新增默认值。
- `EndpointArea::ServerReflexive` 是 SN 观察到的 server-reflexive endpoint 标记，不代表系统默认绑定地址。`Endpoint` 的文本编码必须用 `S` 表示该 area，字符串解析只把 `S` 映射到 `ServerReflexive`；raw codec 继续复用原 area bit 位置，但语义名改为 `ServerReflexive`。
- `Endpoint::is_sys_default()` 不再属于公开接口；实现阶段应删除该方法并修正调用点。若发现下游依赖该方法，应退回 design 明确兼容策略，而不是保留旧 system-default 语义。
- SN 服务端扩展观察 endpoint 时必须比较 SN 观察到的 socket address 与节点自上报 endpoint 集合：协议、IP 和端口均匹配时可标记为 `Wan`；否则标记为 `ServerReflexive`。`Mapped` 仍表示明确映射端口构造出的 WAN 类候选，不与 `ServerReflexive` 合并。
- tunnel/NAT 候选消费端必须把 `ServerReflexive` 作为非静态 WAN 来源处理；它可参与反连候选和 NAT 打洞策略，但不得满足 `is_static_wan()` 的 `Wan` / `Mapped` 判定。
- endpoint 评分必须按协议和端点来源记录历史结果；TCP 失败不得降低同一远端 QUIC/UDP 候选的打洞优先级。
- 本轮保持单 SN 信令模型。SN 服务端只转发单 SN 观察到的公网端点、客户端上报的映射端口和本次 `SnCall` 候选，不承担多 SN 汇总或最终连通性判定。

## 实现布局
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

## 文档索引
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
| bounded_channel_capacity_config | P-BOUNDED-CHANNELS-1 | 在 `P2pConfig` / `P2pStackConfig` 顶层提供分位置 channel 容量配置，每项默认 `1024`，用户默认无需设置，并通过 `P2pEnv` 和各构造函数把对应位置容量传入底层；所有生产路径 `mpsc::unbounded_channel`、`UnboundedSender`、`UnboundedReceiver` 替换为 bounded `mpsc::channel`、`Sender`、`Receiver`；同步分发路径使用 `try_send` 并将满载转成错误、关闭或订阅清理，异步可背压路径使用 `send(...).await`。 | `p2p-frame/src/stack.rs`、`p2p-frame/src/networks/**`、`p2p-frame/src/tunnel/tunnel_manager.rs`、`p2p-frame/src/ttp/**`、`p2p-frame/src/pn/**` 中现有 unbounded mpsc 使用点和对应测试替身 | 回滚时恢复 unbounded mpsc 类型和构造传参，但不得留下半迁移容量 API；若保留顶层配置，应退回 proposal/design 明确兼容目标。 |

## 风险与回滚
- 协议或传输改动可能破坏所有下游 crate。
- 面向运行时或密码学的回归需要比孤立工具改动更强的回滚姿态。
- `Tunnel` stream/datagram 回调化影响公共 API 和所有 tunnel 消费者；回滚必须成组恢复 trait 签名、TCP/QUIC/PN tunnel 实现、TTP/stream/datagram/PN 调用点和测试替身，避免出现回调与公共 accept 双入口并存。
- bounded channel 会把历史积压转为背压或满载错误；回滚必须成组恢复 sender/receiver 类型、构造传参和满载错误映射，避免出现 sender bounded、receiver unbounded 或容量配置无法生效的半迁移状态。
- 回滚应优先撤销具体实现改动，同时保留已批准的 proposal/design/testing 证据，为下一次尝试复用。
