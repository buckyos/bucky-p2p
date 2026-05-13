# Tunnel NAT 打洞优化设计补充

本补充文档定义单 SN 场景下提升 NAT 打洞成功率的设计边界。目标不是重写 SN/PN 协议，也不是引入多 SN、STUN/TURN 或二层虚拟局域网能力，而是在现有 `TunnelManager`、SN call/called 和 proxy 兜底模型内调整候选质量与建链时机。

## 范围

### 范围内
- `TunnelManager` 的 direct/reverse hedged 建链触发条件和短延迟窗口
- `SnCall` 发起时携带本次建链可解释的 `reverse_endpoint_array`
- QUIC/UDP NAT 候选建链窗口内的 best-effort 原生 UDP punch burst
- `ServerReflexive` QUIC NAT keepalive：UDP punch 只面向 `ServerReflexive` QUIC candidate，QUIC heartbeat interval 保持现有值且 timeout 为 30 秒
- SN 服务端在单 SN 内保留并扩展候选端点的转发边界
- proxy tunnel 建立后的短窗口 direct/reverse 脱代理升级调度
- endpoint 评分按协议和历史结果拆分
- endpoint area 语义更新：`Default` 重命名为 `ServerReflexive`，用于 SN 观察到但未与节点自上报地址一致的外网地址

### 范围外
- 多 SN fanout、跨 SN NAT 类型推断或跨 SN 观察结果合并
- 完整 STUN/TURN 协议栈或替换 PN relay
- 改变 `SnCallResp` 语义；它仍只表示 SN 已受理并尝试转发，不表示最终 tunnel 已连通
- 改变 `Tunnel` / `TunnelNetwork` trait 形状
- 改变 PN proxy 的 stream/datagram 加密语义
- 引入上层可见的 raw UDP tunnel、UDP 业务载荷协议、raw UDP 接收解析器或独立于 QUIC listener 端口的新 UDP 打洞 socket
- 继续保留 `Default` system-default 语义，继续使用 `D` 文本标记，或把 SN 反射地址无条件等同于节点自声明 `Wan`

## 核心策略

### 1. NAT-aware direct/reverse 竞速

`TunnelManager` 继续把一次逻辑建链建模为同一个 `tunnel_id` 下的多个候选。direct 和 reverse 仍走现有统一 `register -> publish` 生命周期，reverse waiter 仍按 `(remote_id, tunnel_id)` 匹配。

所有 hedged direct/reverse 场景统一使用短延迟策略：direct 先发起，reverse 固定延迟 300ms 启动，不再按 endpoint area、历史 direct 成功或 NAT 类型推断区分 2 秒长窗口。具体延迟是 `TunnelManager` 内部策略常量，不暴露到通用 `TunnelNetwork` trait。

### 2. QUIC 同源 UDP punch burst

对有 SN 信令参与且 endpoint area 为 `ServerReflexive` 的 QUIC/UDP 候选，`TunnelManager` 可以为本次 `TunnelConnectIntent` 开启 UDP punch，`networks/quic` 再在发起 QUIC handshake 前后发送少量原生 UDP punch 包，帮助 NAT 在同一五元组方向上提前建立映射。该机制是建链辅助，不是新的 tunnel 类型；没有 SN service 时默认保持关闭，因为没有反连协同时 punch 本身没有有效收益。

发送边界：

- punch 包必须从当前 QUIC listener 绑定的同一个 UDP socket/本地端口发出；实现可以在 listener bind 阶段保存该 socket 的 send-only clone，但不得为 punch 新建不同源端口的 `UdpSocket`。
- punch 发送必须受 `TunnelConnectIntent` 中的单次连接开关控制，默认不发送；直接构造 `QuicTunnelNetwork` 或使用默认 intent 时也必须保持关闭。`TunnelManager` 只有在 SN service 存在且候选符合 `ServerReflexive` QUIC 策略时，才会为本次 candidate intent 开启该开关。
- punch 只对 `EndpointArea::ServerReflexive` 且满足 QUIC、非 LAN IPv4、非 0 端口的候选启用；`Lan`、`Wan`、`Mapped`、TCP、IPv6、0 端口、PN proxy fallback 和默认 intent 路径不发送 punch。映射端点在当前实现中归类为 WAN，因此不触发 punch。
- 每个候选的 punch burst 都固定按 50ms cadence 持续发送；reverse burst 在启动后立即发送第一包，active burst 则从 offset `250ms` 才发送第一包。两者默认最晚都到 offset `1000ms` 截止。若本次 NAT hedged window 更短，则必须在更短窗口内裁剪；proxy 脱代理短窗口中的 punch 也必须受同一 cadence 与截止上限保护。
- punch 发送失败只记录 trace/debug 级诊断，不改变 `create_tunnel_with_intent(...)` 的返回错误，也不提前发布 candidate。

载荷边界：

- punch 载荷为 `p2p-frame` 私有短探测内容，可以是每包重新生成的随机长度随机字节；长度范围限定为 `5..=30` 字节，不得要求固定 magic/version、logical `tunnel_id`、`candidate_id` 或本地方向标记。
- punch 载荷必须显式避开 QUIC packet invariant：首字节不得设置 QUIC fixed bit `0x40`，以降低私有探测包被 QUIC endpoint 误判为候选 QUIC 包的风险。
- punch 载荷不得携带业务数据、身份密钥或任何可被本地、远端或上层依赖的协议语义。
- 接收侧不新增 raw UDP 读取路径、不解析 punch，也不发送 punch ack；远端 QUIC listener 继续把该 datagram 作为无效 QUIC 输入丢弃。
- tunnel 成功条件仍是 QUIC handshake 成功，并经 `TunnelManager` 统一 register/publish 生命周期可见。

控制心跳边界：

- QUIC tunnel 的控制心跳发送间隔保持现有实现值不变。
- QUIC tunnel 的 heartbeat timeout 调整为 30 秒。
- timeout 调整只改变失活判定阈值，不新增业务心跳、raw UDP keepalive 协议或远端 punch payload 解析。
- heartbeat timeout 后仍使用既有 QUIC tunnel close/error 路径收敛，不引入独立 NAT keepalive 状态机。

调用边界：

- 主动 direct 连接时，如果 `TunnelManager` 在有 SN service 的前提下为本次候选 intent 开启 punch，`QuicTunnelNetwork::open_or_connect(...)` 在 `connect_with_ep(...)` 前后对目标 remote endpoint 调度 punch burst。
- reverse called 连接同样通过 `TunnelConnectIntent::reverse_logical(...)` 的 QUIC 建链路径触发 punch，但必须受本次 intent 的开关控制，并保证 direct/reverse 共享同一 logical `tunnel_id`。
- active 与 reverse 的 punch burst 都使用同一固定 cadence，但起发时机不同：reverse 在 burst 启动时立即发送首包，active 则在 `250ms` offset 发送首包，随后都每 `50ms` 一包，直到 `1s` 截止或更短的 hedged window 耗尽；实现可以在 `networks/quic` 内部决定 burst 与 QUIC handshake 的前后衔接，但不得改变起发时机、cadence、截止时长或同源 socket 约束。
- 该机制不得要求 `TunnelNetwork` trait 新增公共方法；若实现需要通用化，只能先在 `networks/quic` 内部完成并由 design 重新评审。

### 3. 反连候选

`open_reverse_path()` 或等价路径在发起 `SNClientService::call(...)` 前，应构造本次建链的反连候选列表，并作为 `reverse_endpoint_array` 传入 SN call。当前实现不维护候选新鲜度窗口，只使用当前可解释的本地 listener、SN 当前缓存的 WAN 观察结果和映射端口组合。

候选来源：

- 当前本地 listener 可解释出的 endpoint
- `SNClientService` 当前缓存的 SN 观察 WAN endpoint
- 当前 listener 上配置的映射端口与 SN 观察 IP 组合出的 WAN endpoint
- 远端 desc 中的 endpoint，作为收到 SN called 后的补充候选

候选构造必须去重。SN 服务端继续按当前 `SnCall` 流程转发：先保留调用方传入的 `reverse_endpoint_array`，再扩展单 SN 观察到的发起方公网端点和映射端口；映射端点按 WAN 端点处理。本轮不改变 `SnCallResp`，也不要求 SN 服务端确认被叫是否真的建链成功。

### 3.1. ServerReflexive endpoint area

`EndpointArea::Default` 不再表示 system default，也不再使用 `D` 文本标记。该枚举项必须重命名为 `EndpointArea::ServerReflexive`，用于表示 SN 从连接来源观察到的 server-reflexive endpoint。`Endpoint` 的 `Display`、`FromStr` 与 raw codec 必须同步该语义：

- `Display` 使用 `S` 作为 `ServerReflexive` 的 area 前缀。
- `FromStr` 只把 `S` 解析为 `ServerReflexive`；`D` 不再作为本轮新语义的兼容输入。
- raw codec 可继续使用原 area bit 位置，但常量名和映射语义必须改为 `SERVER_REFLEXIVE`。
- `Endpoint::is_sys_default()` 必须删除；本轮不再提供 system-default 判定入口。

SN 服务端扩展观察 endpoint 时必须把节点自上报 endpoint 集合作为静态 WAN 依据。若 SN 观察到的 socket address 与节点自上报 endpoint 在协议、IP 和端口上完全一致，该 endpoint 可以标记为 `Wan`；若不一致，必须标记为 `ServerReflexive`。映射端口组合出的 endpoint 仍按映射/WAN 类候选处理，不与 `ServerReflexive` 合并。

`TunnelManager` 和 NAT 候选消费端可以把 `ServerReflexive` 纳入反连候选集合，但不得把它当成 `is_static_wan()` 的静态公网地址。`is_static_wan()` 的语义继续只覆盖 `Wan` 和 `Mapped`。

### 4. proxy 短窗口脱代理

proxy tunnel 仍是最终兜底连通性。对已知 `remote_id` 的 proxy candidate，一旦 register/publish 成功，`TunnelManager` 必须安排 direct/reverse 脱代理升级。

本轮将新建 proxy candidate 的首次升级从常规 5 分钟提前到短窗口调度。短窗口仍需避开 PN 首次 open 的 5 秒响应窗口，避免代理隧道刚建好就被后台 direct/reverse 升级探测抢占：

- 首次短窗口：15 秒
- 后续短窗口：30 秒、60 秒、120 秒
- 短窗口耗尽后进入现有指数退避，最大仍不超过 2 小时

升级路径必须禁止把再次建立 proxy 视为升级成功。只要 direct 或 reverse 获得非 proxy candidate，就清理该远端 proxy upgrade 状态，并按统一 publish 入口暴露新候选。

### 5. endpoint 评分隔离

endpoint 评分不再只以 `Endpoint` 为键记录成功和失败。实现阶段应至少按 `(Protocol, Endpoint)` 隔离历史结果；如果代码结构允许，也可以继续纳入候选来源。

评分输入：

- preferred endpoint：上次成功的同协议 endpoint 继续最高优先
- WAN endpoint：加分
- protocol result：同协议失败扣分，TCP 失败不得扣 QUIC/UDP，QUIC/UDP 失败不得扣 TCP
- success result：同协议成功加分并清零该协议失败计数

候选排序仍应稳定：同分时保持输入顺序，避免无证据的随机抖动。

## 数据结构边界

设计允许新增 `TunnelManager` 内部结构保存：

- endpoint score key：协议 + endpoint
- endpoint area key：`Wan` / `Mapped` / `ServerReflexive` 的来源语义必须保留，SN 反射地址不得在候选排序前被静默改写为 `Wan`
- proxy upgrade step：短窗口阶段和退避阶段
- udp punch policy：每个候选的 active `250ms` 起发、reverse `0ms` 起发、固定 `50ms` cadence、`1s` 截止、`TunnelConnectIntent` 单次连接开关、SN service 存在性和是否启用判断
- quic listener punch sender：QUIC listener 绑定 socket 的 send-only clone 或等价内部发送句柄

这些结构只属于 `p2p-frame/src/tunnel/**`、`p2p-frame/src/networks/quic/**` 或 `sn/client` 内部实现，不新增公共 trait 要求。UDP punch 的下游控制由本次 `TunnelConnectIntent` 承载，并以默认关闭作为保守默认值；没有 SN service 时 `TunnelManager` 不开启 punch，不得要求 `cyfs-p2p` 自行实现另一套策略。

## 行为流程

### 主动打开 tunnel

1. `open_known_tunnel_with_options(...)` 先复用已有健康 candidate；若同一远端同时存在多个可用 candidate，默认复用选择必须遵循 published 优先、非 proxy 优先于 proxy、同类候选内更新时间最新优先。
2. 若缓存方向是 direct/reverse/proxy，按缓存方向尝试，但 NAT-aware 策略可以在 direct 缓存失败时立即进入短延迟 reverse，而不是等待完整 fallback。
3. 构造 direct candidate 列表，并按协议和历史结果排序。
4. 若 SN 可用，启动 direct/reverse hedged 建链，reverse 统一延迟 300ms。
5. 若 SN 可用且候选是 `ServerReflexive` QUIC/UDP，`TunnelManager` 可为本次 candidate intent 开启 UDP punch；默认 intent、无 SN service 和非 `ServerReflexive` 路径下跳过 punch。
6. 若 direct/reverse 都失败，按原语义走 proxy fallback。

### 收到 SN called

1. 校验 `peer_info` 得到远端身份。
2. 若本地已有同一 `tunnel_id` 的健康 candidate，则跳过 reverse dial。
3. 合并 `called.reverse_endpoint_array` 与远端 desc endpoints。
4. 如果 `SNClientService::is_same_lan(...)` 判断同 LAN，则本地 LAN/desc 候选优先；否则调用方传入的 reverse candidates、SN 观察到的 `ServerReflexive` 候选和可证明的 `Wan` / `Mapped` 候选优先。
5. 按 endpoint 评分排序后，以 `TunnelConnectIntent::reverse_logical(called.tunnel_id)` 发起 direct path。
6. 若选中的 reverse direct path 是 `ServerReflexive` QUIC/UDP 候选，且本次 `TunnelConnectIntent` 已开启 UDP punch，建链路径可发送同源 UDP punch burst，但不得等待 punch ack。

## 回滚边界

- 若统一 300ms reverse 导致公网直连场景额外负载过高，需要退回 design 重新引入条件化策略。
- 若候选构造影响兼容性，可保留 `SNClientService::call(..., None, ...)` 兼容路径，并仅让 `TunnelManager` 新路径传入候选。
- 若 `D` 文本输入或 `is_sys_default()` 下游依赖仍必须兼容，需要退回 design 定义迁移窗口；实现阶段不得私自保留旧 system-default 语义。
- 若 proxy 短窗口造成重试过密，可调整短窗口序列，但不得退回到“proxy 建立后长时间不尝试 direct/reverse”的行为。
- 若同源 UDP punch 在部分平台或 Quinn socket clone 约束下无法稳定维持 active `250ms` / reverse `0ms` 的 `50ms` / `1s` cadence，应退回 design 重新界定起发时机、cadence 或裁剪策略；不得改成不同源端口的独立 UDP socket，也不得用无限重发替代固定截止。

## 与已批准设计的关系

- 本文继承 `tunnel-publish-lifecycle.md` 的 register/publish 规则。
- 本文不改变 `pn-proxy-encryption.md` 的 TLS-over-proxy 边界。
- 本文不改变 `pn-tunnel-idle-close.md` 的本地 idle close 语义。
- 本文不改变 `sn_design.md` 中 `SnCallResp` 不是最终连通性结果的语义。
