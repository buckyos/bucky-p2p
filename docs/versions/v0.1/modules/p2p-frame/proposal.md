---
module: p2p-frame
version: v0.1
status: approved
approved_by: auto-pipeline
approved_at: 2026-06-04T16:26:18+08:00
---

# p2p-frame 提案

## 背景与目标
- `p2p-frame` 是整个工作区的核心传输和 tunnel 库。
- 这个数据包当前的直接目标，是为未来核心网络栈的改动建立一个严格、可评审的基线，确保协议、传输和运行时改动无法绕过 proposal、design、testing 和 acceptance。
- 当前待落地的直接需求，是在 relay 侧 `pn/service/pn_server.rs` 增加用户流量统计与限速能力，并让经过 proxy server 的 proxy tunnel 支持由使用者显式控制的可选端到端载荷加密；其中 `stream` 路径可显式启用 TLS-over-proxy，而 `datagram` 路径继续保持明文兼容并忽略该加密模式，要求继续复用 `sfo-io` 中已有的统计/限速实现，同时避免在 `p2p-frame` 内重复实现一套新的字节整形逻辑或把业务明文暴露给 relay。本轮新增澄清是：relay 侧统计不再只提供单边 `from` 视图，而是要求 source 和 target 都能独立查询属于自己的桥接流量统计数据；限速仍只按 source 侧用户生效，不因 target 侧统计可见性而扩展成双边限速。
- 本轮新增需求是为 `PnTunnel` 定义本地 idle 生命周期关闭语义：当 tunnel 上无 active、pending 或 queued channel 且持续达到配置的 idle timeout（默认 30 分钟）时，本端必须按与普通 tunnel close 一致的路径原子关闭该 `PnTunnel`，让该对象上的 `accept_*` 等待者出错、后续 `open_*` 被拒绝；若之后又收到同一 `(remote_id, tunnel_id)` 的 inbound open，本端必须按现有 listener 流程重新创建新的 passive `PnTunnel`。
- 本轮新增需求是为 `PnTunnel` 在 logical tunnel 打开时建立一条控制通道，使其具备与 `TcpTunnel` / `QuicTunnel` 同类的对端关闭感知能力；当对端关闭或控制通道断开时，本端不能继续误认为该 tunnel 可用，而必须让本地 `PnTunnel` 进入关闭或错误终态，并唤醒相关 open/accept 等待路径。
- 本轮新增需求是优化单 SN 场景下的 NAT 打洞成功率：在不引入多 SN、不重写 SN/PN 协议、不取消 proxy 兜底的前提下，让 tunnel 建立流程能够使用更实时的候选端点、更适合 QUIC/UDP NAT 打洞的 direct/reverse 竞速窗口、更快的 proxy 脱代理重试，以及更细粒度的 endpoint 评分和刷新策略。本轮新增澄清是：QUIC listener 的同源 UDP punch 不再限定为 2-4 个短包，而是改为在单次 candidate intent 内按固定 50ms 间隔发送，默认持续到 1 秒截止；其中 active punch 从 `250ms` offset 才开始发送，reverse punch 必须立即开始发送，并继续受 SN 存在性、同源 socket 和单次连接开关约束。
- 本轮新增需求是收敛 endpoint area 语义：`EndpointArea::Default` 不再表示 system default，而应重命名为 `ServerReflexive`，用于标识 SN 从连接来源观察到的节点外网地址；SN 观察地址只有与节点自己上报的地址相同时才能升级为 `Wan`，否则必须保持为 `ServerReflexive`，从而区分节点自声明公网地址与 SN 侧反射地址。
- 本轮新增需求是进一步收窄 QUIC NAT 打洞辅助的触发条件：同源 UDP punch 只应面向 `EndpointArea::ServerReflexive` 的 QUIC endpoint 发起，不再面向普通 `Lan`、`Wan`、`Mapped` 或仅凭公网 IP 判断的 endpoint 发起；同时，现有 QUIC tunnel 控制心跳发送间隔保持不变，但心跳超时阈值应调整为 30 秒，降低弱网或调度抖动下的误关闭概率。
- 本轮新增需求是收紧 reverse tunnel 入站可见性语义：reverse incoming tunnel 只有命中本地正在等待的同 `(remote_id, tunnel_id)` reverse waiter 时才可被接收；如果没有 waiter，说明本地并未等待或已经放弃该 reverse 结果，必须直接关闭，不得作为普通 tunnel 向上发布。
- 本轮新增需求是将 `p2p-frame/src/networks/**` 的 TCP 与 QUIC listener 实现基于 `sfo-reuseport` 重构：TCP listener 必须直接使用 `sfo_reuseport::TcpServer` 接收入站连接，QUIC listener 必须直接使用 `sfo_reuseport::QuicServer::serve_socket(...)` 取得每个 worker 的 `sfo_reuseport::UdpSocket`，并为每个 worker socket 创建一个 `quinn::Endpoint::new_with_abstract_socket(...)`；`ServerRuntime` 必须允许由外部显式设置，同时保持默认构造路径可用。
- 本轮新增需求是调整 `TunnelNetwork` 的入站 tunnel 暴露模型：`TunnelNetwork` 不再向外导出 `TunnelListener` 对象，不再提供 `listeners()` 查询方法；`listen(...)` 必须由调用方传入接收新 `Tunnel` 的异步回调函数，返回值改为 `P2pResult<()>`，新进入的 tunnel 通过该回调通知外部。
- 本轮新增需求是调整通用 `Tunnel` 的 stream/datagram 入站 channel 暴露模型：`Tunnel` trait 不再提供 `accept_stream()` 与 `accept_datagram()` 轮询式接口；`listen_stream(...)` 与 `listen_datagram(...)` 必须由调用方传入接收新 stream/datagram channel 的异步回调函数，Tunnel 内部在对应 listener 或 `sfo-reuseport` worker runtime 的入站处理路径中监听新 channel 并触发回调。
- 本轮新增需求是为通用 `Tunnel` 提供低频外部控制数据通道能力：调用方可通过 `open_control_stream(...)` / `listen_control_stream(...)` 在现有 tunnel 控制命令通道上复用一组内部多路复用的 virtual control stream；具体实现必须作为 `Tunnel` 内部共享模块，不向外暴露 `control_stream` runtime、frame 或子协议类型。现有 TCP/QUIC/PN tunnel 控制命令只新增一个 `Data` 命令承载内部 control stream frame，`Data` payload 最大 `64 KiB`，底层控制通道断开时所有派生 control stream 必须断开。
- 本轮新增需求是清理 `p2p-frame` 内部所有 `tokio::sync::mpsc::unbounded_channel` 使用点，改为容量受限的 bounded channel；容量必须由外部配置向下传入，最上层配置提供按队列用途或位置拆分的容量配置，每个配置项默认值为 `1024`，调用方默认不需要显式设置；底层组件只接收对应位置已解析后的容量而不自行定义或兜底默认值。
- 本轮后续清理需求是删除 `p2p-frame/src/stack.rs` 中公开的 `ChannelCapacityConfig` 以及 `P2pConfig` / `P2pStackConfig` / `P2pEnv` 上围绕该结构的容量覆盖、继承和访问逻辑；现有 bounded channel 仍保留容量上限，内部默认统一使用 `DEFAULT_CHANNEL_CAPACITY == 1024`，调用方不再通过 stack 顶层配置覆盖队列容量。

## 范围
### 范围内
- 核心库的长期模块边界
- 对 `p2p-frame/docs/` 下现有协议说明建立设计索引
- 为 unit、DV 和 integration 定义明确的测试面
- 为未来所有 `p2p-frame` 工作定义硬性的 implementation admission 规则
- relay 侧 `pn_server` 在成功握手后的双向字节桥接路径上增加按用户统计的流量计量
- relay 侧 `pn_server` 为 source 与 target 两端分别保留各自可查询的用户流量统计视图
- relay 侧 `pn_server` 在成功握手后的双向字节桥接路径上增加按用户生效的限速能力
- 为 `pn_server` 明确 `sfo-io` 的接入边界、配置输入和统计口径，保证流量统计与限速共用同一套底层实现
- 记录 relay 侧按已认证 peer 身份归属流量和限速决策的要求，避免信任未经 relay 规范化的报文字段
- 明确 source 侧统计主体使用规范化后的已认证 `req.from`，target 侧统计主体使用目标侧已打开流对应的 `req.to`，且两者各自独立查询、互不覆盖
- 为 proxy tunnel 的 `stream` 路径定义“经由 relay 传输但不向 relay 暴露业务明文”的可选 TLS 载荷加密能力
- 明确 proxy tunnel 加密与已认证 peer 身份、relay bridge、用户统计和限速之间的边界
- 定义 proxy tunnel 的显式策略输入和调用接口，使使用者可以自行选择启用或关闭加密
- 约束“未开启加密”和“外部约定启用加密但 TLS 建立失败”时的行为，避免把明文 fallback 伪装成具备 confidentiality 的安全通道
- 明确 proxy tunnel 的 `datagram` 路径不承载 TLS-over-proxy 语义，即使同一 tunnel 选择了 `stream` 加密模式也继续按明文 datagram 行为工作，而不是返回 `NotSupport`
- 为 `PnTunnel` 增加本地 idle timeout 生命周期语义：在 channel 计数为 0 且持续超过可配置阈值后，将 tunnel 原子切换到 `Closed` 或等价错误终态
- idle close 必须复用普通 tunnel close 的本地效果，包括唤醒并失败 `accept_stream()` / `accept_datagram()`、拒绝后续 `open_stream()` / `open_datagram()`、清理待接收队列和保持 close 幂等
- idle 判断的 channel 计数必须覆盖已返回给上层的 stream/datagram、正在 open/accept 握手中的 channel，以及已进入 inbound queue 但尚未被消费的 channel
- 关闭后的同一 `(remote_id, tunnel_id)` 不应继续投递到已关闭对象；后续 inbound `ProxyOpenReq` 必须创建新的 passive `PnTunnel`
- 为 `PnTunnel` 增加 tunnel 级控制通道：logical tunnel 打开时必须完成控制通道建立或等价的 ready 握手，后续 stream/datagram channel 不得在缺少控制面生命周期依据的情况下被认为属于可用 tunnel
- 控制通道必须能让本端感知对端关闭、控制面读写失败或控制通道断开，并按普通 tunnel close 的本地效果关闭当前 `PnTunnel`
- `PnTunnel` 控制通道关闭和 idle timeout 关闭必须共享同一关闭状态机，避免同一对象被重复关闭、重新打开，或在 close 后继续接收新的 channel
- 单 SN 场景下的 NAT 打洞优化，包括 direct/reverse 统一短延迟竞速、`SnCall` 携带本次反连候选端点、proxy 后短窗口脱代理升级、endpoint 评分按协议隔离，以及 tunnel 建立前组合 SN 观察端点或本地映射候选
- endpoint area 语义更新：将 `Default` 改名为 `ServerReflexive`，将 SN 观察到但未与节点自上报地址一致的外网地址标记为 `ServerReflexive`，只有一致时才标记为 `Wan`
- QUIC NAT punch 策略收窄：只有目标 endpoint 的 area 是 `ServerReflexive` 时，`TunnelManager` 才能为本次 QUIC candidate intent 开启同源 UDP punch
- QUIC tunnel 控制心跳超时策略：保持现有心跳发送间隔不变，将失活判定超时调整为 30 秒
- NAT 打洞优化必须优先覆盖 QUIC/UDP tunnel；TCP 直连仍可保留现有静态 WAN 或明确映射端口路径，但不得把 TCP 失败扩散为 QUIC/UDP 候选降权依据
- proxy 仍是最终兜底连通性；优化目标是更快从 proxy 升级为 direct/reverse，而不是移除 proxy 或让 proxy 参与后台升级成功判定
- `TunnelManager` 复用已有 tunnel 时，若同一远端同时存在多个可用候选，必须优先返回非 proxy tunnel；proxy 只在没有可用非 proxy candidate 时作为兜底复用路径
- reverse incoming tunnel 必须命中同 `(remote_id, tunnel_id)` 的本地 reverse waiter；无 waiter 时必须关闭，不得 register、不得 publish、不得进入 `get_tunnel()` 默认复用候选
- TCP listener 必须基于 `sfo_reuseport::TcpServer` 注册服务，并把入站 `sfo_reuseport::TcpStream` 接入现有 TLS accept、control/data connection 分流和 tunnel publish 流程
- QUIC listener 必须基于 `sfo_reuseport::QuicServer::serve_socket(...)` 注册服务，并实现一个仅属于 `networks/quic` 内部的 Quinn `AsyncUdpSocket` 适配器，把每个 worker 回调得到的 `sfo_reuseport::UdpSocket` 直接交给对应的 Quinn endpoint
- 每个 QUIC worker socket 必须对应一个 Quinn endpoint；endpoint accept loop 在 `serve_socket` 回调 future 中运行，所有 accepted QUIC tunnel 汇入同一 `QuicTunnelListener` accept 队列
- QUIC listener 必须使用 `sfo_reuseport::QuicCidGenerator` 或等价内部适配，把 Quinn endpoint 生成的 connection ID 前 2 字节设置为对应 worker shard，确保后续 QUIC packet 稳定回到同一 worker endpoint
- QUIC 主动 connect 可在同一 listener 的 worker endpoint 集合中选择任一 endpoint 发起；同源 UDP punch 必须使用 `serve_socket` 回调取得的任一 listener socket，优先保存第一个可用 socket，不得退回独立 UDP socket或破坏既有同源端口语义
- `ServerRuntime` 必须能由 `p2p-frame` 外部通过明确配置入口设置；未设置时 `p2p-frame` 仍负责创建默认 `sfo_reuseport::ServerRuntime`
- `TunnelNetwork::listen(...)` 必须接收一个可克隆、线程安全的入站 tunnel 回调；TCP、QUIC 与 PN network 在 listener 内部 accept 到新 tunnel 后直接调用该回调，调用方不再通过返回的 `TunnelListener` 对象自行启动 accept loop
- `TunnelNetwork::listen(...)` 成功只表示 listener 已注册并开始向回调投递后续入站 tunnel；返回值不得携带 `TunnelListenerRef`
- `TunnelNetwork` 公共 trait 不再包含 `listeners()`；调用方若只需要监听元数据，继续通过 `listener_infos()` 获取
- `Tunnel::listen_stream(...)` 必须接收一个可克隆、线程安全的入站 stream 回调；Tunnel 内部接收到符合 listen 规则的新 stream channel 后直接调用该回调，调用方不再通过 `accept_stream()` 自行轮询
- `Tunnel::listen_datagram(...)` 必须接收一个可克隆、线程安全的入站 datagram 回调；Tunnel 内部接收到符合 listen 规则的新 datagram channel 后直接调用该回调，调用方不再通过 `accept_datagram()` 自行轮询
- `Tunnel` 公共 trait 必须移除 `accept_stream()` 与 `accept_datagram()`；TCP、QUIC、PN tunnel 以及 TTP/stream/datagram manager 调用点必须改为基于 listen 回调分发入站 channel
- stream/datagram 回调化必须保持现有 vport/purpose listen 过滤语义、open 失败语义、tunnel close/error 传播语义和 bounded channel 容量约束；若设计阶段需要保留内部队列，也只能作为 tunnel 内部实现细节，不得重新暴露公共 accept 轮询入口
- `Tunnel` 公共 trait 必须新增 `open_control_stream(purpose)` 与 `listen_control_stream(purposes, callback)`，用于低频外部控制数据流；返回给调用方的仍是 `TunnelStreamRead` / `TunnelStreamWrite`，不得暴露内部 `control_stream` runtime、frame enum、stream id 或 window 协议。
- control stream 必须复用每个 tunnel 已有的控制命令通道承载外部控制数据：TCP/QUIC/PN 各自仅新增一个控制命令 `Data { payload }` 或等价命令，payload 内部由 `Tunnel` 内部 `control_stream` 模块解析；现有控制通道的 ready、heartbeat、close、open response 等已有命令逻辑不得被重写或替换。
- 单个控制命令 `Data` 的 payload 上限必须是 `64 KiB`；调用方写入更大 buffer 时由内部 control stream 模块切分，接收侧遇到超过上限的 `Data` 必须按协议错误关闭相关 tunnel 或至少关闭所有派生 control stream，不得继续解析。
- 底层 tunnel 控制通道断开、decode 失败、write 失败、heartbeat timeout、收到 remote close、本地 close 或 tunnel 进入 closing/closed/error 时，所有基于该控制通道派生的 control stream 都必须断开，pending read/write/open 必须返回 EOF 或 `Interrupted` 类错误，后续 `open_control_stream` 必须立即失败。
- control stream 是低频控制扩展，不得承载普通业务大流量，不得改变现有 `open_stream` / `listen_stream`、`open_datagram` / `listen_datagram` 行为、线协议载荷格式、TLS 身份校验、PN proxy 业务 channel 协议、vport/purpose 编解码或 tunnel publish 规则。
- `p2p-frame` 内部事件、accept、listener、stream/datagram 和 tunnel 订阅队列必须由 unbounded channel 改为 bounded channel，避免无上限内存增长
- bounded channel 容量必须从顶层配置入口向下传递；顶层配置必须按不同队列用途或位置提供可独立覆盖的容量项，各项默认值均为 `1024`，调用方不设置时使用默认容量即可启动
- 不同位置的 channel 容量不得强制共用同一个配置值；至少应能区分 TTP listener registry、TunnelManager subscription、NetManager incoming subscriber、PN 内部队列和 QUIC listener connect/punch 相关内部队列等仍存在 bounded queue 的类别；TCP/QUIC tunnel stream/datagram 入站回调路径不再保留旧 accept queue 容量参数
- 底层 network、tunnel、PN、TTP 等组件不得定义自己的 channel 容量默认值；缺少容量时必须由构造路径上传入，而不是在底层 silently fallback
- 当 bounded channel 满载时，设计阶段必须为各路径明确背压、拒绝、关闭或错误传播语义，且不得通过重新引入 unbounded buffer 绕开容量限制
- 删除 stack 层容量配置 API 后，仍存在的 bounded channel 必须继续使用容量上限，不得退回 unbounded channel；默认容量保持 `1024`，但不再要求调用方可按队列位置覆盖容量

### 范围外
- 重写当前协议实现
- 改变当前工作区成员布局
- 用新副本替换现有协议说明
- 为非 `pn/service` 子模块引入同一轮的统一流量整形改造
- 在 `p2p-frame` 内重新实现一套独立于 `sfo-io` 的统计器、令牌桶或限速器
- 因为需要 target 侧统计可见性，就把用户级限速从 source 侧扩展为对 target 侧也生效的双边整形
- 让 relay/proxy server 持有、派生或恢复用于解密业务载荷的明文密钥
- 为 `pn` 之外的 active/reverse/direct tunnel 一次性引入同级别的业务载荷加密改造
- 为 proxy tunnel 的 `datagram` 路径在本轮同时引入 TLS 等价的载荷加密
- 借本轮需求重写整套 PN 协议
- 为 `PnTunnel` 引入全局租约心跳、跨 relay 长期 session 存活模型，或把控制通道扩展成通用 PN 外的传输控制协议
- 因 idle timeout 关闭本端 `PnTunnel` 时强制中断已经交给上层并仍在活动的 channel；这些 channel 必须先通过正常生命周期让计数归零，idle 才能触发
- 引入多 SN fanout、跨 SN 协调或依赖多个 SN 观察结果推断 NAT 类型
- 重写 SN `ReportSn` / `SnCall` / `SnCalled` 的基础命令协议，或改变 `SnCallResp` 仅表示 SN 受理结果而非最终连通性结果的语义
- 引入完整 STUN/TURN 协议栈、外部第三方 NAT 探测服务，或把 PN relay 替换为 TURN 等价服务
- 将 NAT 打洞优化扩展成二层广播域、L2 bridge、虚拟局域网自动发现或跨网段服务发现能力
- 为本轮同时设计双边 NAT 类型数据库、长期全局路径质量服务或跨进程持久化的连接质量画像
- 保留 `Default` 作为 endpoint area 的公开语义，或继续用 `D` 作为 `Display`/`FromStr` 的 area 标记
- 对非 `ServerReflexive` endpoint 发起 UDP punch，包括仅因地址是非 LAN IPv4 就发起 punch
- 为 `ServerReflexive` tunnel 引入新的上层业务心跳、raw UDP keepalive 协议、额外心跳发送频率或要求远端解析 punch payload
- 改变 QUIC tunnel 现有心跳发送间隔；本轮只允许调整心跳超时阈值
- 改变 direct、proxy、普通 incoming tunnel 的 register/publish 规则；本轮只收窄 reverse incoming 无 waiter 的行为
- 引入 reverse tunnel 过期表或跨进程状态；本轮只以当前 pending reverse waiter 作为接收入站 reverse 的依据
- 新增 `NetworkServerRuntime`、socket factory trait 或其他包裹 `sfo-reuseport` 的通用运行时抽象；本轮必须直接使用 `sfo_reuseport::ServerRuntime`、`TcpServer` 和 `QuicServer`
- 将 QUIC listener 重构为独立 raw UDP 业务协议，或要求上层解析 UDP punch payload
- 改变 TCP/QUIC tunnel 线协议、TLS 身份校验语义或 tunnel candidate publish 规则；除本轮明确批准的 `listen` 回调化、返回值改为 `P2pResult<()>` 和移除 `listeners()` 外，不再扩大 `TunnelNetwork` 公共 trait 变更
- 借 `Tunnel` stream/datagram 回调化改变 TCP/QUIC/PN/TTP 线协议、TLS 身份校验、PN proxy channel 协议、业务 payload 格式、vport/purpose 编码或 tunnel publish 规则
- 保留 `accept_stream()` / `accept_datagram()` 作为公共兼容旁路，或要求调用方同时注册回调又轮询 accept 队列
- 将内部 `control_stream` runtime、frame、stream id、window 或 buffer 类型作为公开 API 暴露给 `p2p-frame` 外部调用方；外部只能通过 `Tunnel` trait 的 `open_control_stream` / `listen_control_stream` 使用该能力
- 使用现有 tunnel 控制通道直接传输未经封装和限长的外部 byte stream，或让外部调用方直接读写内部控制通道 raw stream
- 借 control stream 引入新的业务数据平面、大流量传输替代、公开 raw control frame 协议、全局 tunnel session 协议或 PN 之外的新 relay 语义
- 因新增控制命令 `Data` 而重写现有 TCP/QUIC/PN 控制通道 ready、heartbeat、close、claim/open、PN control open 或业务 stream/datagram open 逻辑
- 因引入 `sfo-reuseport` 而移除现有 QUIC NAT punch 的 `ServerReflexive` 准入条件、50ms cadence、active/reverse 起发时机或 1 秒默认截止
- 在底层组件中散落硬编码 channel 容量默认值，或保留 `unbounded_channel` 作为容量限制的旁路
- 借本轮 bounded channel 改造改变 TCP/QUIC/PN/TTP 线协议、身份校验、tunnel publish 规则或业务 payload 格式
- 删除 `ChannelCapacityConfig` 时改变 bounded channel sender/receiver 类型、满载错误语义、TCP/QUIC/PN/TTP 线协议、身份校验、tunnel publish 规则或业务 payload 格式

### 与相邻模块的边界
- `cyfs-p2p` 可以适配 `p2p-frame`，但不拥有 `p2p-frame` 的协议语义。
- `cyfs-p2p-test` 提供运行时验证场景，但不能替代 `p2p-frame` 的 testing 设计。
- `sn-miner-rust` 和 `desc-tool` 消费相邻能力，它们是 integration 邻居，而不是核心栈的所有者。
- `pn_server` 的流量统计与限速属于 `p2p-frame/src/pn/service/**` 的 relay 责任，不下沉给 `cyfs-p2p` 做语义分叉。
- `sfo-io` 负责提供被调用的统计/限速实现；`p2p-frame` 只负责在 relay bridge 生命周期中正确装配、调用和暴露所需观测点。
- source 与 target 的独立统计视图都属于 `p2p-frame/src/pn/service/**` 的 relay 责任边界；下游适配层只能消费查询结果，不能自行推导另一套统计口径。
- proxy tunnel 的端到端加密语义属于 `p2p-frame` 的 PN/tunnel 责任边界；`cyfs-p2p` 只能消费或配置该能力，不能在适配层静默定义另一套不受 relay 约束的 proxy 加密语义。
- proxy tunnel 的 `datagram` 明文兼容语义同样属于 `p2p-frame` 的 PN/tunnel 责任边界；`cyfs-p2p` 不得通过适配层把 `stream` 加密模式扩展成 `datagram` 拒绝或隐式加密语义。
- NAT 打洞优化属于 `p2p-frame/src/tunnel/**`、`p2p-frame/src/sn/client/**` 和必要的 SN 服务端候选转发边界；`cyfs-p2p` 可以暴露配置或消费行为，但不得在适配层分叉 tunnel 建立策略。
- SN 服务端在本轮只承担单 SN 的观察端点、上报候选和 call/called 转发职责；它不负责判定最终连通性，也不负责跨多个 SN 汇总 NAT 类型。
- SN 观察地址分类属于 `p2p-frame` 的 SN/tunnel endpoint 语义边界；下游适配层不得把 `ServerReflexive` 与节点自声明 `Wan` 静默合并成同一类地址。
- `sfo-reuseport` 负责 listener socket 绑定、reuse-port worker 分发、`TcpServer`/`QuicServer` 服务注册、`serve_socket` worker socket 回调、`UdpSocket` Quinn helper 接口和 QUIC worker-shard CID 生成辅助；`p2p-frame` 负责把这些 socket/stream/packet 接入已有 tunnel、TLS、QUIC 和 NAT punch 语义，不得把 `p2p-frame` 的协议语义下沉到 `sfo-reuseport`。
- 外部设置 `ServerRuntime` 是 `p2p-frame` 的配置责任边界；`cyfs-p2p` 可透传或组合该配置，但不得在适配层另行定义 listener 分发策略。
- `Tunnel` stream/datagram 回调模型属于 `p2p-frame` 的公共 tunnel API 责任边界；`cyfs-p2p` 可以适配该 API，但不得在适配层保留另一套基于公共 `accept_*` 的 tunnel channel 暴露语义。
- `Tunnel` control stream API 属于 `p2p-frame` 的公共 tunnel API 责任边界；`cyfs-p2p` 可以调用 `open_control_stream` / `listen_control_stream`，但不得依赖或重新定义 `p2p-frame` 内部 control stream frame、stream id、window 和 buffer 协议。
- bounded channel 容量属于 `p2p-frame` 顶层运行时配置责任边界；`cyfs-p2p` 可以透传或组合该配置，但不得在适配层为 `p2p-frame` 底层队列另行定义一套默认值。顶层配置必须提供完整默认配置，因此普通调用方不需要为了启用 bounded channel 而显式填写任一容量项。
- `ChannelCapacityConfig` 清理后，容量不再是 `p2p-frame` 对相邻模块暴露的配置责任；相邻模块不得依赖 stack 顶层容量覆盖 API。

## 约束
- 允许使用的库/组件：
  - 现有工作区 crate 和当前协议说明
  - 基于 cargo 的验证命令
  - `sfo-io` 中已存在的流量统计与限速实现
- 禁止采用的方式：
  - 协议或运行时改动绕过阶段审批
  - 在不更新 testing 和 trigger-rule 覆盖的情况下静默改变传输行为
  - 在 `pn_server` 中复制或旁路 `sfo-io` 的统计/限速逻辑，导致两套行为源
  - 通过未认证的 `from`/`to` 报文字段归属用户流量或决定限速对象
  - 以“经过 relay 后仍由 relay 可见明文”的方式宣称 proxy tunnel 已具备端到端加密
  - 把是否加密变成隐式默认行为，导致调用方无法明确控制 proxy tunnel 的保密语义
  - 在两端已显式约定启用加密的情况下静默降级到明文 proxy tunnel
  - 通过增加多 SN 依赖来解决本轮单 SN 打洞问题
  - 将 SN 观察端点或本地映射端口提升为跨 SN NAT 类型推断依据
  - 因 TCP direct 失败而全局惩罚同一远端的 QUIC/UDP 打洞候选
  - 为规避 bounded channel 满载而在任一路径保留或重新引入 `unbounded_channel`
  - 为兼容旧调用方而在公共 `Tunnel` trait 中保留 `accept_stream()` 或 `accept_datagram()`
  - 公开导出内部 `control_stream` runtime/frame 类型，或让外部直接读写现有 tunnel raw 控制通道
- 系统约束：
  - 保持当前以 tokio 为优先的运行时策略
  - 保持混合 edition 的工作区布局
  - 保持当前协议说明中记录的兼容性预期
  - 保持现有 crate 边界，不因为本次需求把 `pn_server` 的 relay 语义迁移到其他 crate
  - bounded channel 各位置默认容量只能出现在顶层配置定义中，默认值均为 `1024`；底层构造函数、listener、tunnel、registry 和测试替身必须接收对应位置的显式容量或配置快照，不得自行选择默认值
  - 顶层配置必须能在用户不传任何 channel 容量参数时构造完整默认配置；显式配置时，调用方可以只覆盖某一类队列容量，不应被迫同时覆盖所有队列容量
  - channel 容量配置必须覆盖所有由当前 unbounded channel 承载且仍保留为 queue 的内部队列；设计阶段若某个路径已改为直接回调交付，不得继续保留旧 queue 容量参数，仍需明确关闭、拒绝或错误语义
  - 后续 `stack_channel_capacity_config_removal` 清理获批后，以上顶层分位置容量覆盖要求不再作为公开配置义务；保留的 bounded queue 使用固定默认容量 `1024`，并继续禁止生产路径 `unbounded_channel`
  - 流量统计口径必须与 relay 实际成功转发的字节数一致；仅进入用户态缓冲但未成功写出的字节不得提前计入
  - source 与 target 两个统计视图都必须以各自“成功写到对端”的字节为准；不得把同一批字节重复累计到同一用户视图，也不得因为双边都可见而丢失任何一侧的记账
  - 限速应作用于 relay 成功握手后的桥接数据路径，不改变握手前 `ProxyOpenReq`/`ProxyOpenResp` 的控制流时序
  - target 侧新增统计可见性不得改变当前限速主体；若未来需要 target 侧限速或双边配额，必须单独通过新的 proposal 扩展
  - proxy tunnel 的加密边界必须位于 relay 无需解密业务载荷也能继续完成转发、统计和限速的层次
  - 若 relay 继续承担统计或限速，其默认口径必须以实际成功转发的密文字节为准，除非后续设计另行定义并证明明文口径
  - 加密模式下的身份认证仍必须锚定到底层 relay 已认证并规范化后的 peer 身份，而不是依赖未认证的业务载荷字段
- 使用者必须能通过显式接口或配置为单条 proxy tunnel、单个调用点或明确的构造路径选择“加密”或“不加密”，而不是依赖全局隐式副作用
- TLS 是否启用由 proxy tunnel 两端在 tunnel 外通过配置、调用约定或同一构造路径显式决定；relay 不负责为此增加额外协商语义
- `stream` 加密模式只约束 `stream` channel；`datagram` channel 在本轮必须忽略该模式并保持当前明文兼容语义，不能因为 `TlsRequired` 而被本地或对端 open/accept 路径拒绝
- `PnTunnel` 控制通道是 logical tunnel 生命周期的一部分；tunnel 打开时必须建立控制面，并在控制面关闭、读写失败或对端显式关闭时把本地 tunnel 收敛到 `Closed` 或等价错误终态
- `PnTunnel` idle close 仍是本地兜底生命周期管理；即使没有远端主动关闭，本端也必须能在 channel 计数归零并超过 idle timeout 后独立释放本地对象
- `PnTunnel` 的 idle 关闭判定必须在统一状态临界区内完成，避免 channel 计数归零、超时扫描、inbound open 投递和本地 open 之间出现误关闭或向已关闭对象投递的竞态
- `PnTunnel` 控制通道与 channel open/accept、inbound 投递、idle sweeper 的关闭判定必须通过同一状态临界区协调，避免控制面断开后仍创建新 channel 或把迟到 channel 投递给已关闭对象
- `tunnel_id` 与 `candidate_id` 在 `PnTunnel` 生命周期内保持稳定；idle close 不得在同一对象上更换身份，也不得把已关闭对象重新打开
- NAT 打洞路径必须保持同一次逻辑建链共享同一个 `tunnel_id`；direct 与 reverse 竞速只能改变候选时机，不能破坏 candidate 注册、reverse waiter 和 publish 生命周期。
- reverse incoming 的接收判定必须只依赖当前 pending reverse waiter；无同 `(remote_id, tunnel_id)` waiter 时必须关闭该 tunnel，且不得阻止非 reverse incoming tunnel 的普通处理。
- QUIC/UDP NAT 打洞场景下，reverse 不应被固定为 direct 失败后的长延迟补救；具体延迟与触发条件必须由 design 明确，并可由 unit 测试验证。
- QUIC/UDP NAT 打洞场景下，同源 UDP punch 必须在本次连接开始后按固定 50ms cadence 发送，默认持续到 1 秒截止；active path 只能从 `250ms` offset 开始发首包，reverse path 必须从 `0ms` 立即发首包。若本次 NAT hedged window 更短，则只能在更短窗口内裁剪，禁止无限重发或跨 candidate 共享重试状态。
- QUIC/UDP NAT 打洞场景下，同源 UDP punch 的候选准入必须以 endpoint area 为准：只有 `EndpointArea::ServerReflexive` 且满足 QUIC、非 LAN IPv4、非 0 端口等基本发送条件时才可开启；`Wan`、`Mapped`、`Lan` 和未标记为 `ServerReflexive` 的公网 endpoint 不得触发 punch。
- QUIC tunnel 控制心跳必须保持现有发送间隔不变，仅将 heartbeat timeout 调整为 30 秒；该策略不得新增 `TunnelNetwork` NAT 专用参数，也不得要求下游调用方显式传入 NAT 类型。
- `SnCall` 携带的反连候选必须来自本次可解释的本地 listener、SN 观察端点或映射端口集合，且必须避免重复候选。
- SN 服务端生成或扩展观察端点时，若观察到的外网地址与节点自上报地址相同，才允许把该 endpoint 标记为 `Wan`；若不同，必须标记为 `ServerReflexive`。
- `EndpointArea::ServerReflexive` 的文本编码必须使用 `S`，`Display`、`FromStr` 和 raw codec 的 area 语义必须同步；原 `is_sys_default()` system-default 语义不再保留为公开判定入口。
- proxy 脱代理升级必须在 proxy 连通后进入短窗口重试，再回到有上限的指数退避；后台升级路径不得把再次建立 proxy 视为升级成功。
- `ServerRuntime` 注入必须是显式配置能力；默认路径仍必须在不要求调用方传入 runtime 的情况下启动 TCP/QUIC listener。
- TCP listener 的 `close()` 必须能停止本 listener 关联的 `TcpServer` 服务或至少停止其继续向当前 listener 投递入站 tunnel；不得只关闭上层接收队列而让旧服务继续生成可见 tunnel。
- QUIC listener 的 `close()` 必须关闭 `QuicServer`、所有 worker Quinn endpoint 和 punch 发送句柄；关闭后不得继续向已关闭 listener 投递 packet 或 tunnel。
- listener 关闭后不得继续调用已注册的入站 tunnel 回调；如果关闭与入站 accept 并发，迟到 tunnel 必须被关闭或丢弃，不得发布给已关闭 listener 的外部回调。
- tunnel 关闭后不得继续调用已注册的入站 stream/datagram 回调；如果关闭与入站 channel 投递并发，迟到 channel 必须按设计关闭、拒绝或返回错误，不得交付给已关闭 tunnel 的外部回调。
- stream/datagram 回调必须在与对应 tunnel 入站处理相同的运行时上下文中调度；对基于 `sfo-reuseport` worker 的 TCP/QUIC 入站路径，设计阶段必须明确回调 future 在 worker runtime 中执行、转发或 spawn 的所有权与背压语义。
- `listen_stream(...)` / `listen_datagram(...)` 的重复注册、替换、关闭后注册、回调返回错误或 panic/abort 处理必须由 design 明确，implementation 不得临时选择与现有 close/error 语义矛盾的行为。
- Quinn `AsyncUdpSocket` 适配器必须只属于 `p2p-frame/src/networks/quic/**` 内部实现；它不得成为公共 `TunnelNetwork` trait 的新要求，也不得暴露 raw UDP 业务接口。
- Quinn `AsyncUdpSocket::try_send()` 必须使用其 worker `sfo_reuseport::UdpSocket::try_send_to(...)` 发送 QUIC packet；同源 UDP punch 必须使用同一 listener 的任一 `serve_socket` worker socket 来源。
- Quinn `AsyncUdpSocket::poll_recv()` 必须只从其 worker `sfo_reuseport::UdpSocket` 接收 QUIC packet；UDP punch 私有短载荷仍不得被接收侧解析或传递给上层业务。
- `sfo-reuseport` 的 worker 分发不得改变现有 TLS 身份校验、QUIC connection accept、TCP control/data connection 分流、`tunnel_id`/`candidate_id` 和 reverse waiter 语义。
- `Tunnel` control stream 必须作为 tunnel 内部共享实现模块复用在 TCP/QUIC/PN 上；该模块只通过 transport 注入的 `send Data payload` 适配器与现有控制命令通道交互，transport 侧不得解析内部 control stream 子协议。
- `Data` 控制命令 payload 上限固定为 `64 KiB`，该上限必须由发送侧切分和接收侧校验共同保证；任何绕过该上限的 frame 都是协议错误。
- 控制命令通道生命周期严格支配所有派生 control stream；底层控制通道关闭或错误后，所有 virtual control stream、pending open、pending write 和 listen 交付必须收敛，不得留下仍可读写的半开对象。

## 高层结果
- 未来的 `p2p-frame` 改动必须通过显式模块数据包进入流程。
- 协议和运行时高风险改动必须触发更强的评审和验证。
- 核心栈工作的 acceptance 必须把结果回溯到 proposal 意图。
- relay 侧 `pn_server` 能按 source 与 target 两端各自的用户维度统计上下行传输字节，并把两侧统计结果都接到 `sfo-io` 的实现上。
- relay 侧 `pn_server` 能按已认证用户维度执行限速，并保持成功握手后的透明字节桥接语义。
- 统计与限速都建立在同一条 `sfo-io` 接入链路上，避免计量口径和限速执行口径分叉；其中 source/target 的独立统计视图不能要求新增第二套统计实现。
- proxy tunnel 能由使用者显式选择是否启用端到端载荷加密；启用后 relay/proxy server 只暴露完成路由、控制流和配额所需的最小元数据。
- 本轮 proxy tunnel 加密能力以 `stream` 路径上的 TLS-over-proxy 为交付目标；`datagram` 不提供 TLS 语义，但在 `TlsRequired` 场景下仍保持明文兼容并忽略该模式。
- proxy tunnel 的加密能力具有显式的策略边界，不会把未加密路径误报为具备 confidentiality 的安全通道。
- `PnTunnel` 能在无 active/pending/queued channel 并持续空闲超过配置阈值后进入本地关闭终态，释放本地 tunnel 资源并让等待中的 accept/open 路径获得明确错误，而不是无限期挂起或依赖不可靠的远端 close 通知。
- `PnTunnel` 打开后具备 tunnel 级控制通道；当对端关闭或控制通道断开时，本端能够及时关闭当前 `PnTunnel`，避免继续复用一个远端已经不可用的 logical tunnel。
- 单 SN NAT 打洞路径能以统一 300ms 延迟启动 direct/reverse 竞速，使用本次建链反连候选，并在 proxy 兜底后更快尝试脱代理升级。
- 在 SN 存在且 `TunnelManager` 为本次 candidate intent 开启的 QUIC/UDP NAT 候选上，同源 UDP punch 能以固定 50ms cadence 从同一本地端口发送，默认持续到 1 秒截止；其中 active 起发时机晚于 reverse，以兼顾正向握手推进与反向 NAT 映射抢开，而不引入新的 raw UDP 接收或业务语义。
- 同源 UDP punch 不再由“非 WAN 公网 QUIC endpoint”泛化触发，而是只由 `ServerReflexive` QUIC endpoint 触发，避免对静态公网、映射端口或 LAN endpoint 发送无收益的 NAT punch。
- QUIC tunnel 在保持现有心跳发送间隔的同时，将心跳超时阈值调整为 30 秒，降低短时弱网、调度抖动或 NAT 路径抖动导致的误关闭概率。
- endpoint 选择能区分协议、历史成功和失败；TCP 与 QUIC/UDP 的失败统计不得互相污染。
- SN report / call 相关候选传递保持 `SnCallResp` 与最终 tunnel 连通性结果解耦。
- endpoint area 能明确区分节点自声明公网地址与 SN 反射地址：相同地址可作为 `Wan`，不相同地址作为 `ServerReflexive`，并通过 `S` 文本标记序列化。
- TCP listener 的底层 accept 分发由 `sfo_reuseport::TcpServer` 承担，但入站 tunnel 的 TLS accept、control/data connection 分流、registry/publish 语义保持现有协议行为。
- QUIC listener 的底层 UDP packet 分发由 `sfo_reuseport::QuicServer` 承担，Quinn 仍通过 `Endpoint::new_with_abstract_socket(...)` 管理 QUIC connection 和 incoming tunnel；每个 worker socket 拥有一个 Quinn endpoint，主动 QUIC connect 可随机或轮询选择一个 endpoint，同源 UDP punch 使用首个可用 worker socket。
- `ServerRuntime` 可由外部设置并复用于 TCP/QUIC listener；未设置时仍由 `p2p-frame` 默认创建，保持现有调用方无需显式 runtime 的兼容启动路径。
- 通用 `TunnelNetwork` 调用方通过 `listen(local, out, mapping_port, on_incoming_tunnel)` 注册入站回调并接收 `P2pResult<TunnelRef>`；`NetManager` 负责把该回调接到原有 incoming validator、订阅发布和 reject close 路径，保持外部 tunnel publish 语义不变。
- 通用 `Tunnel` 调用方通过 `listen_stream(vports, on_incoming_stream)` 与 `listen_datagram(vports, on_incoming_datagram)` 注册入站 channel 回调；stream/datagram、TTP 和 PN server/client 侧不再启动公共 `accept_*` 循环，入站 channel 由 tunnel 内部接收后直接投递到对应回调。
- 通用 `Tunnel` 调用方能通过 `open_control_stream(purpose)` / `listen_control_stream(purposes, on_incoming_control_stream)` 获得低频控制数据通道；该能力通过现有 tunnel 控制命令通道上的单一 `Data` 命令承载内部多路复用 frame，但 `control_stream` runtime 和 frame 类型不成为公开 API。
- 当底层控制通道仍健康时，control stream 的 open、listen、read、write、fin/reset 和 purpose 过滤能独立于现有 stream/datagram 数据平面工作；当底层控制通道断开或 tunnel close 时，所有派生 control stream 立即失败或 EOF。
- 所有原 `unbounded_channel` 队列均具备容量上限，默认从顶层配置取得对应位置的 `1024`；外部调用方可以按队列类别或位置独立覆盖容量，不需要为未覆盖位置重复填写默认值；底层组件只消费对应位置已解析容量，并在队列满载时按设计定义的背压、拒绝、关闭或错误路径收敛。
- `ChannelCapacityConfig` 后续清理完成后，stack 顶层不再暴露容量结构、getter 或 setter；默认调用方仍无需设置容量即可启动，保留队列继续以 `1024` 为固定容量上限收敛满载路径。

## 风险
- 旧设计说明与未来实现之间的协议漂移
- 传输或 tunnel 改动缺少运行时证据
- `cyfs-p2p` 和运行时二进制中的跨 crate 回归
- `sfo-io` 尚未在当前工作区中显式接入；依赖选择、版本兼容和运行时行为都可能带来额外设计工作
- 若流量统计挂点放错位置，可能出现重复计数、漏计数，或把未成功落到底层 writer 的字节提前记账
- 若 source/target 双边统计映射关系处理错误，可能出现串户、双记账，或 source/target 查询结果互相覆盖
- 限速引入的背压可能改变 relay bridge 的时延、关闭顺序或超时分布，需要在 testing 中单独建模
- 按用户维度归属统计和限速时，若身份归一化边界不清晰，可能导致统计串户或限速对象错误
- proxy tunnel 端到端加密会把 proxy stream 建链、TLS 身份校验和双端接口约束耦合到一起，若设计不当会直接引入兼容性回归
- relay 统计/限速在加密模式下可能只能看到密文字节和密文时序，这会改变口径认知、调试方式和运维诊断习惯
- 若加密模式与 passive `PnTunnel`/listener 接口装配顺序处理不当，可能破坏现有 open/accept 时序或导致隐性明文窗口
- 若 `datagram` 继续错误继承 `stream` 加密模式，现有依赖明文 datagram 的调用点会在显式开启 `TlsRequired` 后出现非预期拒绝，形成兼容性回归
- 若 channel 计数没有覆盖所有 active、pending 和 queued 状态，idle timeout 可能误关闭仍有业务活动的 tunnel，或永远无法释放空闲 tunnel
- 若 idle close 与 inbound `ProxyOpenReq` 分发没有共享状态约束，后续 channel 可能被错误投递到已关闭对象，而不是重新创建 tunnel
- 若已返回给上层的 stream/datagram 没有可靠的 lease/drop 计数路径，`PnTunnel` 无法判断 channel 数量是否真正归零
- 若 `PnTunnel` 控制通道建立和 channel open 顺序处理不当，可能出现 channel 已交付但控制面尚未 ready、控制面关闭后仍继续投递 channel，或 close 与 idle sweeper 重复竞争的并发问题
- 控制通道会增加每个 logical `PnTunnel` 的 relay 连接或控制面资源占用；若关闭、超时和错误清理不完整，可能引入新的资源泄漏或半开 tunnel
- 过早启动 reverse 可能增加短时并发拨号和日志噪声，也可能在公网直连可快速成功的场景中带来额外候选，需要 endpoint 策略限制触发条件。
- 反连候选不维护新鲜度窗口，真实 NAT 映射可用性仍依赖 SN 当前缓存和现场网络行为。
- 若 active `250ms` / reverse `0ms` 的 50ms cadence 在弱网、平台 socket 语义或 proxy 脱代理短窗口里发送过密，或 active 首包延后导致部分 NAT 映射建立偏慢，可能形成额外日志噪声、发送风暴或连通性波动，需要 design/testing 明确起发时机、上限和裁剪规则。
- proxy 后短窗口脱代理会增加 SN call、direct connect 和 reverse waiter 的频率，需要有退避、上限和并发保护，避免在弱网络下形成重试风暴。
- 若 reverse incoming 无 waiter 时仍发布 tunnel，调用方会看到“本地未等待 reverse 结果但订阅收到 reverse tunnel”的不一致语义。
- 若按协议拆分评分实现不完整，可能出现 TCP 与 QUIC/UDP 路径质量互相污染，降低原本可成功的打洞候选优先级。
- NAT 打洞运行时结果高度依赖真实网络环境，unit 测试只能覆盖调度和候选规则；DV 或 integration 需要明确哪些结果是可自动断言，哪些只能作为运行证据。
- `Default` 改名为 `ServerReflexive` 会影响公开枚举、字符串编解码和已有外部配置/日志；如果下游仍依赖 `D` 或 `is_sys_default()`，实现阶段需要明确兼容或破坏性迁移边界。
- `sfo-reuseport` 的 `TcpServer`/`QuicServer` 是服务注册模型，不是当前 listener 对象直接持有 socket 并循环 accept 的模型；若关闭、任务取消或 handler 错误语义设计不清，可能出现旧 listener 关闭后仍接收新连接或 packet。
- Quinn `AsyncUdpSocket` 适配器需要正确处理 `poll_recv` waker、`try_send` backpressure、local addr、关闭唤醒和 packet metadata；若实现不完整，可能导致 QUIC handshake 卡住、主动 connect 失败或 CPU 空转。
- per-worker Quinn endpoint 依赖 `sfo-reuseport` QUIC route key 与 `QuicCidGenerator` 生成的 worker shard 保持一致；若 CID 生成或 Initial/0-RTT fallback 路由与后续 short-header shard 不一致，可能导致同一 QUIC connection 的 packet 分裂到不同 endpoint。
- 外部注入 `ServerRuntime` 会改变 listener 生命周期所有权；若默认 runtime 与外部 runtime 混用边界不清，可能造成重复 worker、提前 drop 或服务无法关闭。
- `Tunnel` stream/datagram 回调化会改变上层消费时序；若回调执行位置、背压、重复注册或 close 并发没有设计清楚，可能导致入站 channel 丢失、控制循环被回调阻塞、关闭后仍交付 channel，或 TTP/stream/datagram manager 的 listener 生命周期泄漏。
- control stream 在现有控制命令通道上承载外部数据，若 frame 切分、写锁占用、buffer/window 或关闭传播设计不当，可能阻塞内部 heartbeat/close/open response，导致 tunnel 生命周期误判或半开 control stream 泄漏。
- 若 transport 层解析内部 control stream frame，或把 `control_stream` 子协议暴露为公开 API，会导致 TCP/QUIC/PN 的控制面实现耦合到外部调用方，破坏后续演进边界。
- 移除公共 `accept_*` 会影响所有测试替身和下游 crate；若迁移遗漏，可能形成编译回归或旧语义在某个 manager 中被私有队列重新暴露。
- unbounded channel 改为 bounded channel 会把原先隐藏的积压转化为背压或错误；如果容量传递遗漏、满载语义不一致，可能导致 accept/open 等待路径卡住、过早关闭或错误传播不清。
- 若底层组件继续保留局部默认值，实际容量会与顶层配置漂移，造成不同 transport 或测试替身的内存上限不可预测。
- 若不同队列位置继续共用单一容量配置，调用方无法针对高吞吐 tunnel accept、TTP registry、subscription fanout 或 PN 内部队列分别调参，可能在某些路径容量不足时被迫整体放大所有队列上限。

## 验收锚点
- source 侧用户能够通过 `pn_server` 的查询入口看到属于自己这条 bridge 的累计统计，且记账主体使用 relay 规范化后的已认证 `req.from`，不受源端伪造 `from` 影响。
- target 侧用户能够通过 `pn_server` 的查询入口看到属于自己这条 bridge 的累计统计，且记账主体锚定到 relay 成功打开的目标用户 `req.to`，不依赖未认证的业务载荷字段。
- source 与 target 的统计视图都只统计成功握手后的 bridge payload，不统计 `ProxyOpenReq` / `ProxyOpenResp`，并且只在成功写出后入账。
- target 侧统计能力的引入不得把限速主体从 source 扩展成双边限速；现有用户级限速仍只按 source 侧用户生效。
- 双边统计在 TLS-over-proxy 场景下继续以 relay 实际成功转发的 TLS record / 密文字节为口径，而不是尝试恢复业务明文大小。
- `PnTunnel` 在 channel 数量归零并持续超过 idle timeout 后必须进入 `Closed` 或等价错误终态，`accept_stream()` / `accept_datagram()` 必须被唤醒并返回错误。
- idle close 后，本地对同一 `PnTunnel` 的后续 `open_stream()` / `open_datagram()` 必须立即失败，不得重新激活该对象或等待 PN open timeout。
- idle close 后，迟到或后续的同一 `(remote_id, tunnel_id)` inbound open 不得投递给已关闭对象；必须重新创建新的 passive `PnTunnel`。
- `PnTunnel` 打开时必须建立 tunnel 级控制通道；控制通道未 ready 时，不得把该 logical tunnel 作为完全可用 tunnel 返回或继续创建业务 channel。
- 当 `PnTunnel` 对端关闭、控制通道读写失败或控制通道断开时，本端必须进入 `Closed` 或等价错误终态，唤醒 pending `accept_*`，并让后续 `open_*` 明确失败。
- 控制通道关闭与 idle timeout 关闭必须保持幂等，并且 close 后的同一 `(remote_id, tunnel_id)` inbound open 不得投递到旧对象。
- QUIC/UDP NAT 候选场景下，`TunnelManager` 的 direct/reverse 竞速统一延迟 300ms 启动 reverse；具体短延迟规则必须由 design/testing 直接覆盖。
- QUIC/UDP NAT 候选场景下，同源 UDP punch 必须以固定 50ms cadence 发送，并在 1 秒截止或更短的 NAT hedged window 结束时停止；active 首包只能在 `250ms` 起发，reverse 首包必须在 `0ms` 起发。acceptance 必须确认实现没有把该行为扩展成无限重发、独立 UDP socket 或 raw UDP 协议。
- QUIC/UDP NAT punch 必须只对 `ServerReflexive` endpoint 开启；acceptance 必须确认 `Wan`、`Mapped`、`Lan`、TCP、IPv6、0 端口和默认 intent 路径均不会开启 punch。
- QUIC tunnel heartbeat timeout 必须调整为 30 秒，且心跳发送间隔保持现有值不变；acceptance 必须确认心跳超时仍走既有 QUIC tunnel close/error 路径。
- `open_reverse_path()` 或其等价路径发起 `SnCall` 时，必须能携带本次建链的 `reverse_endpoint_array`，且 SN 转发后的 `SnCalled` 保留这些候选。
- proxy tunnel 成功后，后台脱代理升级必须先进入短窗口 direct/reverse 重试，再进入有上限的指数退避；升级成功后非 proxy candidate 必须按统一 register/publish 生命周期可见。
- reverse incoming 无同 `(remote_id, tunnel_id)` waiter 时必须被关闭；acceptance 必须确认它不会进入候选表、不会向订阅者发布，也不会被后续 `get_tunnel(remote)` 复用。
- reverse incoming 命中同 `(remote_id, tunnel_id)` waiter 时必须继续按延后 publish 规则交付给 waiter；acceptance 必须确认该正常 reverse open 路径不被无 waiter close 规则破坏。
- 当同一远端同时存在已发布的非 proxy candidate 和 proxy candidate 时，`get_tunnel()` 或等价默认复用路径必须优先选择非 proxy candidate，即使 proxy candidate 更新时间更晚。
- endpoint 评分必须能按协议独立影响候选顺序，且 TCP 失败不得降低 QUIC/UDP 候选的打洞优先级。
- 单 SN 优化不得引入多 SN fanout 或跨 SN NAT 类型推断；acceptance 必须确认最终实现仍只依赖单 SN 信令与观察端点。
- SN 观察 endpoint 分类必须可验收：当 SN 观察地址与节点自上报地址一致时输出 `Wan`，不一致时输出 `ServerReflexive`；`EndpointArea` 的显示、解析和 raw codec 语义必须使用 `ServerReflexive` / `S`，且不再暴露 `is_sys_default()` 判定。
- TCP listener 必须通过 `sfo_reuseport::TcpServer` 接收入站连接；acceptance 必须确认 `TcpServer` handler 收到的 stream 进入现有 TLS accept、control/data connection 分流、tunnel registry 和 publish 路径，且 listener close 后不再发布新 tunnel。
- QUIC listener 必须通过 `sfo_reuseport::QuicServer::serve_socket(...)` 接收入站 worker socket，并通过 Quinn `AsyncUdpSocket` 适配器交给每个 worker 的 `quinn::Endpoint`；acceptance 必须确认 worker endpoint 的 `accept()` 仍能产出 incoming QUIC tunnel，且不新增 raw UDP tunnel 或业务载荷解析路径。
- QUIC 主动 connect 可使用任一 worker endpoint；同源 UDP punch 必须使用首个可用 worker socket；acceptance 必须确认 punch 的本地端口与 QUIC listener 端口一致，且 `ServerReflexive` candidate policy、50ms cadence、active/reverse 起发时机和截止规则保持不变。
- 外部设置 `ServerRuntime` 时，TCP/QUIC listener 必须使用该 runtime；未设置时默认 runtime 路径必须仍可启动 TCP/QUIC listener。
- `Tunnel` 公共 trait 不得继续包含 `accept_stream()` 或 `accept_datagram()`；所有入站 stream/datagram channel 必须通过 `listen_stream(...)` / `listen_datagram(...)` 注册的回调交付。
- TCP、QUIC 和 PN tunnel 接收到符合 listen 规则的新 stream/datagram channel 后，必须在设计定义的运行时上下文中触发对应回调；关闭后的 tunnel 不得继续触发回调。
- TTP、stream manager、datagram manager 和 PN server/client 调用点必须迁移到回调模型；acceptance 必须确认没有公共 `accept_*` 轮询循环残留。
- `Tunnel` control stream 的对外 API 只能是 `open_control_stream` / `listen_control_stream`；代码审查和测试必须确认内部 `control_stream` runtime/frame 类型未公开导出，且 TCP/QUIC/PN 只新增一个控制命令 `Data` 作为承载。
- control stream 验证必须覆盖 `Data` payload 最大 `64 KiB`、发送侧大 buffer 切分、接收侧超限拒绝，以及底层控制通道断开后所有派生 stream、pending open 和 pending write 失败。
- `p2p-frame` 代码中不得继续存在生产路径 `mpsc::unbounded_channel`、`UnboundedSender` 或 `UnboundedReceiver`；对应队列必须使用 bounded channel，并由顶层配置传入容量。
- 顶层配置未显式设置 channel 容量时，仍存在的每个队列类别或位置的默认值必须为 `1024`；显式设置某一位置容量时，只有对应 network、PN、TTP、manager 等底层队列使用该覆盖值，其他位置继续使用顶层默认值，且底层不得自行覆盖默认值；TCP/QUIC tunnel 不再暴露旧 accept queue 容量项。
- 验收必须确认用户可以不设置任何 channel 容量并获得完整默认配置，也可以只覆盖单个位置容量而不影响其他位置容量。
- bounded channel 满载路径必须有可验收行为：按设计定义背压等待、返回错误、关闭迟到 tunnel/channel 或丢弃已关闭 listener 的迟到事件，不得静默无限缓存。

## Proposal Items
| proposal_id | change_id | Outcome | Constraints / Non-goals | Success Evidence |
|-------------|-----------|---------|--------------------------|------------------|
| P-ENDPOINT-AREA-1 | endpoint_area_server_reflexive | `EndpointArea::Default` 重命名为 `ServerReflexive`，SN 观察到的节点外网地址只有与节点自上报地址一致时标记为 `Wan`，否则标记为 `ServerReflexive`；文本编码使用 `S`，不再保留 `is_sys_default()` system-default 判定。 | 不引入 STUN/TURN、多 SN NAT 类型推断或新的 endpoint area；不继续把 `D` 作为 `ServerReflexive` 的文本标记；不把 SN 反射地址静默等同于节点自声明公网地址。 | unit 能覆盖 SN 观察地址一致/不一致时的 area 分类，覆盖 `Display`/`FromStr`/raw codec 的 `ServerReflexive` / `S` 编解码，并确认 `is_sys_default()` 不再作为公开方法存在。 |
| P-QUIC-SR-NAT-KEEPALIVE-1 | server_reflexive_quic_nat_keepalive | QUIC 同源 UDP punch 只对 `EndpointArea::ServerReflexive` candidate 开启；QUIC tunnel 保持现有控制心跳发送间隔不变，但 heartbeat timeout 调整为 30 秒。 | 不对 `Lan`、`Wan`、`Mapped`、TCP、IPv6、0 端口或默认 intent 发起 punch；不新增 raw UDP keepalive 协议、业务载荷解析或公共 `TunnelNetwork` trait 参数；不改变 QUIC tunnel 现有心跳发送间隔。 | unit 能覆盖 punch candidate policy 只接受 `ServerReflexive` QUIC endpoint，覆盖非 `ServerReflexive` endpoint 不开启 punch；unit 能覆盖 QUIC heartbeat interval 保持现有值且 heartbeat timeout 为 30 秒，且 heartbeat timeout 仍收敛到既有关闭路径。 |
| P-REV-TIMEOUT-1 | reverse_timeout_close_late_tunnel | reverse incoming tunnel 只有命中同 `(remote_id, tunnel_id)` reverse waiter 时才可接收；无 waiter 时必须关闭，不得作为普通 tunnel publish。 | 不改变 direct、proxy、普通 incoming tunnel 的 publish 规则；不改变 SN call/called 协议；不引入 reverse 过期表或跨进程状态；不按 remote 粗粒度关闭其他 tunnel。 | unit 能覆盖无 waiter reverse tunnel 被 close、未 register、未 publish、订阅者收不到、`get_tunnel()` 不返回；unit 能覆盖命中 waiter 的 reverse tunnel 仍正常交付并延后 publish。 |
| P-SFO-TCP-LISTENER-1 | networks_sfo_reuseport_tcp_listener | TCP tunnel listener 基于 `sfo_reuseport::TcpServer` 重构，入站 `sfo_reuseport::TcpStream` 接入现有 TLS accept、TCP control/data connection 分流、registry 和 tunnel publish 流程；`ServerRuntime` 可由外部显式设置，默认路径仍自动创建。 | 不新增 `NetworkServerRuntime` 或 socket factory trait；不改变 TCP tunnel 线协议、TLS 身份校验或 tunnel publish 语义；不把 close 后的旧服务继续暴露为当前 listener 的入站 tunnel。 | unit 能覆盖外部 `ServerRuntime` 被 TCP listener 使用、默认 runtime 可用、`TcpServer` handler 的 stream 进入现有 control/data 分流路径、listener close 后不再发布新 tunnel；integration 能覆盖 TCP tunnel 仍可建立并传输。 |
| P-SFO-QUIC-LISTENER-1 | networks_sfo_reuseport_quic_listener_socket | QUIC tunnel listener 基于 `sfo_reuseport::QuicServer::serve_socket(...)` 重构，使用内部 Quinn `AsyncUdpSocket` 适配器把每个 worker `sfo_reuseport::UdpSocket` 交给对应 `quinn::Endpoint`，并使用 worker-shard CID generator 保持连接路由稳定；主动 connect 可选择任一 worker endpoint，同源 UDP punch 使用首个可用 worker socket；`ServerRuntime` 可由外部显式设置，默认路径仍自动创建。 | 不新增 raw UDP tunnel、业务载荷解析、公共 `TunnelNetwork` NAT 参数或 `NetworkServerRuntime`；不改变 QUIC tunnel 线协议、TLS 身份校验、NAT punch candidate policy、50ms cadence、active/reverse 起发时机、截止规则或 heartbeat 语义。 | unit 能覆盖 worker socket `try_send_to` / `poll_recv_from` 被 Quinn `AsyncUdpSocket` 使用、worker CID generator 绑定对应 worker shard、UDP punch 本地端口与 QUIC listener 端口一致、listener close 后关闭 `QuicServer` 和所有 Quinn endpoint；integration 能覆盖 QUIC tunnel 仍可建立并传输。 |
| P-TUNNEL-NETWORK-CALLBACK-1 | tunnel_network_listen_callback | `TunnelNetwork::listen(...)` 改为由调用方传入入站 tunnel 回调并返回 `P2pResult<()>`；`TunnelNetwork` 不再导出 `TunnelListener` 或提供 `listeners()`，新 tunnel 通过回调通知外部。 | 不改变 `TunnelListener` 内部实现类型可被 TCP/QUIC/PN listener 复用；不改变 tunnel publish、incoming validator、TLS 身份校验、TCP/QUIC/PN 线协议、QUIC NAT punch 策略或 `listener_infos()` 语义；不要求调用方轮询 listener。 | unit 能覆盖 `NetManager::listen(...)` 注册回调后仍走 incoming validator、订阅发布和 reject close 路径；unit 或编译覆盖 TCP/QUIC/PN `listen(...)` 返回 `P2pResult<()>` 且不再暴露 `listeners()`；integration 能覆盖 workspace 调用方迁移后仍可建立入站 tunnel。 |
| P-TUNNEL-CHANNEL-CALLBACK-1 | tunnel_stream_datagram_listen_callback | `Tunnel` trait 移除 `accept_stream()` / `accept_datagram()`，`listen_stream(...)` / `listen_datagram(...)` 改为接收可克隆、线程安全的异步回调；Tunnel 内部在入站 stream/datagram channel 到达时按 vport/purpose listen 规则触发回调。 | 不改变 TCP/QUIC/PN/TTP 线协议、TLS 身份校验、PN proxy channel 协议、业务 payload 格式、vport/purpose 编解码、tunnel publish 规则或 `TunnelNetwork` listener 回调语义；不保留公共 `accept_*` 兼容旁路；不要求调用方同时注册回调又轮询队列。 | schema/admission 能以 `tunnel_stream_datagram_listen_callback` 建立后续准入；unit 或编译覆盖公共 `Tunnel` trait 不再包含 `accept_*`，TCP/QUIC/PN tunnel 以及 TTP/stream/datagram manager 调用点均迁移到 listen 回调；unit 覆盖关闭后不再调用回调、未 listen 的 purpose 仍拒绝或报错、回调满载/背压/错误路径按 design 收敛；integration 覆盖 workspace 调用方迁移后 stream/datagram 仍可建立并传输。 |
| P-TUNNEL-CONTROL-STREAM-API-1 | tunnel_control_stream_api | `Tunnel` trait 新增 `open_control_stream(...)` / `listen_control_stream(...)`，为外部提供低频控制数据通道；内部 `control_stream` 模块通过现有 TCP/QUIC/PN tunnel 控制命令通道上的单一 `Data` 命令多路复用 virtual stream，`Data` payload 最大 `64 KiB`，底层控制通道断开时所有派生 control stream 断开。 | 不公开内部 `control_stream` runtime/frame/stream id/window 类型；不让外部直接读写现有 raw 控制通道；不改变现有 stream/datagram 逻辑、业务 payload 格式、TLS 身份校验、PN proxy 业务 channel 协议、vport/purpose 编解码或 tunnel publish 规则；不将该能力扩展为大流量业务数据平面。 | schema/admission 能以 `tunnel_control_stream_api` 建立后续准入；unit 或编译证据确认公共 API 只有 `Tunnel` trait 方法和 callback/stream 类型，内部 control stream 类型未公开导出；TCP/QUIC/PN 只新增 `Data` 控制命令承载内部 frame；测试覆盖 open/listen、purpose 过滤、64KiB 切分/超限拒绝、控制通道断开后所有派生 stream 和 pending open/write 失败。 |
| P-BOUNDED-CHANNELS-1 | bounded_channel_capacity_config | `p2p-frame` 内部所有 `unbounded_channel` 队列改为 bounded channel；容量由顶层配置按队列类别或位置独立向下传递，每个容量项默认值为 `1024`，用户默认不需要设置，外部可只覆盖某一位置容量，底层组件不定义默认值。 | 不改变 TCP/QUIC/PN/TTP 线协议、身份校验、tunnel publish 规则或业务 payload 格式；不在底层散落硬编码默认容量；不强制所有队列共用一个容量配置；不通过额外 unbounded buffer 绕开容量限制。 | schema/admission 能以 `bounded_channel_capacity_config` 建立后续准入；unit 或编译覆盖默认用户不设置时各容量默认 `1024`、单个位置自定义容量只影响对应底层构造路径；代码搜索确认生产路径不再存在 `mpsc::unbounded_channel`、`UnboundedSender` 或 `UnboundedReceiver`；满载路径具备按设计定义的错误、关闭或背压覆盖。 |
| P-STACK-CHANNEL-CAPACITY-REMOVAL-1 | stack_channel_capacity_config_removal | 删除 `ChannelCapacityConfig`、`P2pConfig` / `P2pStackConfig` 的 channel capacity getter/setter、`P2pEnv` 的容量快照和继承逻辑；`NetManager` 不再保存或暴露 channel capacity；`TtpRuntime` 不再接收无效 channel capacity 参数，`TtpClient` / `TtpServer` 不再为了创建 TTP runtime 从 `NetManager` 读取容量；`PnClient` 不再提供 channel capacity 显式构造入口；保留 bounded channel，内部使用固定 `DEFAULT_CHANNEL_CAPACITY == 1024`。 | 不改变已有 bounded channel 类型、满载错误语义、TCP/QUIC/PN/TTP 线协议、身份校验、tunnel publish 规则或业务 payload 格式；不新增替代公开容量配置 API；不把容量清理扩展为移除所有底层显式构造参数。 | schema/admission 能以 `stack_channel_capacity_config_removal` 建立后续准入；编译或 unit 覆盖 `stack.rs` 不再导出 `ChannelCapacityConfig` 或 stack 层容量 getter/setter，`NetManager::new(...)` / `new_with_incoming_tunnel_validator(...)`、`TtpRuntime::new()` 和 `PnClient::new*` 不再要求容量参数；代码搜索确认生产路径仍无 `unbounded_channel`、`UnboundedSender` 或 `UnboundedReceiver`；unit 或 compile 覆盖默认 stack 构造继续使用固定容量启动相关 bounded queue。 |

## Downstream Follow-up
- 本次 proposal 新增 `tunnel_stream_datagram_listen_callback`，并已完成 proposal 审批、design/testing 同步和 implementation admission。
- Design 已补齐 `Tunnel` trait 新签名、stream/datagram 回调类型、回调 future 的运行时归属、重复注册/关闭后注册/回调错误处理、内部队列与 bounded channel 容量关系，以及 TCP/QUIC/PN/TTP/manager 调用点迁移方案。
- Testing 已补齐公共 trait 编译覆盖、TCP/QUIC/PN tunnel 入站回调、未 listen 拒绝、关闭后不回调、回调背压或满载、TTP/stream/datagram manager 迁移和 workspace integration 覆盖。
- Implementation 已在 proposal/design/testing 均 approved 且 admission 以 `tunnel_stream_datagram_listen_callback` 通过后修改 `p2p-frame/src/networks/tunnel.rs` 及相关生产代码。
- Acceptance 必须重新审计新的 `Tunnel` channel 回调模型是否移除了公共 `accept_*`，且没有通过内部公共队列、下游适配层或测试替身保留旧轮询语义。
- 本次 proposal 已按用户确认从“统一一个容量配置”调整为“按队列类别或位置独立配置，并且用户默认不需要设置”，并重新进入 `approved` 状态。
- Design 已补齐独立容量配置结构、默认构造规则、每个底层队列应消费的配置项，以及只覆盖单一位置容量时的继承行为。
- Testing 已补齐默认用户不配置、单项覆盖不影响其他位置、底层不保留默认值、满载错误传播和无 unbounded mpsc 的验证映射。
- Implementation 必须在 proposal/design/testing 均 approved 且 admission 通过后，把当前统一 `channel_capacity` 改为分位置配置下发。
- Acceptance 必须重新审计新的独立容量配置是否覆盖所有原 unbounded channel 使用点，且没有通过单一全局容量或底层默认值绕过本 proposal。
- 本次后续清理已按用户确认新增 `stack_channel_capacity_config_removal`，允许删除 `ChannelCapacityConfig` 及 stack 层容量覆盖逻辑；Design/Testing/Implementation/Acceptance 必须同步改为以固定默认容量和无 unbounded channel 作为证据重点。
- 本次自动流水线已按用户确认新增 `tunnel_control_stream_api`，必须先补齐 Design/Testing，再通过 implementation admission 后实现；该能力的内部 `control_stream` 模块不得公开导出。
