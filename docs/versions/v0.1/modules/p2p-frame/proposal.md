---
module: p2p-frame
version: v0.1
status: approved
approved_by: user
approved_at: 2026-05-10
---

# p2p-frame 提案

## 背景与目标
- `p2p-frame` 是整个工作区的核心传输和 tunnel 库。
- 这个数据包当前的直接目标，是为未来核心网络栈的改动建立一个严格、可评审的基线，确保协议、传输和运行时改动无法绕过 proposal、design、testing 和 acceptance。
- 当前待落地的直接需求，是在 relay 侧 `pn/service/pn_server.rs` 增加用户流量统计与限速能力，并让经过 proxy server 的 proxy tunnel 支持由使用者显式控制的可选端到端载荷加密；其中 `stream` 路径可显式启用 TLS-over-proxy，而 `datagram` 路径继续保持明文兼容并忽略该加密模式，要求继续复用 `sfo-io` 中已有的统计/限速实现，同时避免在 `p2p-frame` 内重复实现一套新的字节整形逻辑或把业务明文暴露给 relay。本轮新增澄清是：relay 侧统计不再只提供单边 `from` 视图，而是要求 source 和 target 都能独立查询属于自己的桥接流量统计数据；限速仍只按 source 侧用户生效，不因 target 侧统计可见性而扩展成双边限速。
- 本轮新增需求是为 `PnTunnel` 定义本地 idle 生命周期关闭语义：当 tunnel 上无 active、pending 或 queued channel 且持续达到配置的 idle timeout（默认 30 分钟）时，本端必须按与普通 tunnel close 一致的路径原子关闭该 `PnTunnel`，让该对象上的 `accept_*` 等待者出错、后续 `open_*` 被拒绝；若之后又收到同一 `(remote_id, tunnel_id)` 的 inbound open，本端必须按现有 listener 流程重新创建新的 passive `PnTunnel`。
- 本轮新增需求是为 `PnTunnel` 在 logical tunnel 打开时建立一条控制通道，使其具备与 `TcpTunnel` / `QuicTunnel` 同类的对端关闭感知能力；当对端关闭或控制通道断开时，本端不能继续误认为该 tunnel 可用，而必须让本地 `PnTunnel` 进入关闭或错误终态，并唤醒相关 open/accept 等待路径。
- 本轮新增需求是优化单 SN 场景下的 NAT 打洞成功率：在不引入多 SN、不重写 SN/PN 协议、不取消 proxy 兜底的前提下，让 tunnel 建立流程能够使用更实时的候选端点、更适合 QUIC/UDP NAT 打洞的 direct/reverse 竞速窗口、更快的 proxy 脱代理重试，以及更细粒度的 endpoint 评分和刷新策略。本轮新增澄清是：QUIC listener 的同源 UDP punch 不再限定为 2-4 个短包，而是改为在单次 candidate intent 内按固定 50ms 间隔发送，默认持续到 1 秒截止；其中 active punch 从 `250ms` offset 才开始发送，reverse punch 必须立即开始发送，并继续受 SN 存在性、同源 socket 和单次连接开关约束。

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
- NAT 打洞优化必须优先覆盖 QUIC/UDP tunnel；TCP 直连仍可保留现有静态 WAN 或明确映射端口路径，但不得把 TCP 失败扩散为 QUIC/UDP 候选降权依据
- proxy 仍是最终兜底连通性；优化目标是更快从 proxy 升级为 direct/reverse，而不是移除 proxy 或让 proxy 参与后台升级成功判定
- `TunnelManager` 复用已有 tunnel 时，若同一远端同时存在多个可用候选，必须优先返回非 proxy tunnel；proxy 只在没有可用非 proxy candidate 时作为兜底复用路径

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
- 系统约束：
  - 保持当前以 tokio 为优先的运行时策略
  - 保持混合 edition 的工作区布局
  - 保持当前协议说明中记录的兼容性预期
  - 保持现有 crate 边界，不因为本次需求把 `pn_server` 的 relay 语义迁移到其他 crate
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
- QUIC/UDP NAT 打洞场景下，reverse 不应被固定为 direct 失败后的长延迟补救；具体延迟与触发条件必须由 design 明确，并可由 unit 测试验证。
- QUIC/UDP NAT 打洞场景下，同源 UDP punch 必须在本次连接开始后按固定 50ms cadence 发送，默认持续到 1 秒截止；active path 只能从 `250ms` offset 开始发首包，reverse path 必须从 `0ms` 立即发首包。若本次 NAT hedged window 更短，则只能在更短窗口内裁剪，禁止无限重发或跨 candidate 共享重试状态。
- `SnCall` 携带的反连候选必须来自本次可解释的本地 listener、SN 观察端点或映射端口集合，且必须避免重复候选。
- proxy 脱代理升级必须在 proxy 连通后进入短窗口重试，再回到有上限的指数退避；后台升级路径不得把再次建立 proxy 视为升级成功。

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
- endpoint 选择能区分协议、历史成功和失败；TCP 与 QUIC/UDP 的失败统计不得互相污染。
- SN report / call 相关候选传递保持 `SnCallResp` 与最终 tunnel 连通性结果解耦。

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
- 若按协议拆分评分实现不完整，可能出现 TCP 与 QUIC/UDP 路径质量互相污染，降低原本可成功的打洞候选优先级。
- NAT 打洞运行时结果高度依赖真实网络环境，unit 测试只能覆盖调度和候选规则；DV 或 integration 需要明确哪些结果是可自动断言，哪些只能作为运行证据。

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
- `open_reverse_path()` 或其等价路径发起 `SnCall` 时，必须能携带本次建链的 `reverse_endpoint_array`，且 SN 转发后的 `SnCalled` 保留这些候选。
- proxy tunnel 成功后，后台脱代理升级必须先进入短窗口 direct/reverse 重试，再进入有上限的指数退避；升级成功后非 proxy candidate 必须按统一 register/publish 生命周期可见。
- 当同一远端同时存在已发布的非 proxy candidate 和 proxy candidate 时，`get_tunnel()` 或等价默认复用路径必须优先选择非 proxy candidate，即使 proxy candidate 更新时间更晚。
- endpoint 评分必须能按协议独立影响候选顺序，且 TCP 失败不得降低 QUIC/UDP 候选的打洞优先级。
- 单 SN 优化不得引入多 SN fanout 或跨 SN NAT 类型推断；acceptance 必须确认最终实现仍只依赖单 SN 信令与观察端点。
