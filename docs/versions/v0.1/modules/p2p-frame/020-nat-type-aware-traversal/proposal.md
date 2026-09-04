---
module: p2p-frame
task_name: 020-nat-type-aware-traversal
submodule: 020-nat-type-aware-traversal
version: v0.1
status: draft
approved_by:
approved_at:
approved_content_sha256:
---

# NAT 映射探测与双边自适应穿透 Proposal

## Workflow Tier Judgment

- Proposed tier: `high-risk`
- Final tier: `pending`
- Rationale: 本任务同时改变 SN 公网 UDP 探针、NAT profile wire 语义、SN peer 内存信息、TunnelManager 的 caller/callee 角色选择，以及 QUIC punch/connect 生命周期。错误实现可能造成 mixed-version 不兼容、首次建链退化、额外公网 UDP 流量或重复 tunnel。
- Confirmation statement: 本 proposal 已按用户要求重写并回到 `draft`；等待重新批准后再执行 design、implementation、post-implementation testing 和 acceptance。

## Background and Goal

现有 P2P tunnel 在非公网 endpoint 间主要采用 direct 与延迟 reverse 的固定 hedge，并对 `ServerReflexive` QUIC candidate 发送同源 UDP punch。该流程不知道两端当前映射是否会随目标端口变化，也无法据此决定哪一侧应发起 QUIC、哪一侧只需 punch 开孔。

单台 SN、单一公网 IP 的多个 UDP 端口不能识别传统 full-cone、restricted-cone、port-restricted-cone、symmetric 四类 NAT。使用同一个本地 UDP socket 向同一 SN IP 的两个不同端口发包，只能观察“目标端口改变时外部映射保持不变”或“外部映射发生变化”。这一结果只描述本次、该本地 socket、该 SN IP 下的实际行为，不能证明换成另一个 peer IP 后仍相同。

本任务以这一真实观测边界重建 NAT profile 和 tunnel 建链逻辑：SN 保存每个 peer 最新的观测；caller 从本次 `SnQueryResp.net_profile` 得到 callee profile，并把计划所用双方快照放入 `SnCall.nat_context`；callee 从原样转发的 `SnCalled.nat_context` 得到 caller/callee 同一有序快照；`TunnelManager` 据此生成 connector、candidate、`PunchOnly`、`WaitIncoming` 和 fallback 计划。一次计划默认只有一侧发起 QUIC，另一侧可以只发送有界 punch，而不是把 SN 通知等同于反连。

此前 task 020 的实现与下游阶段文档已经回退。本 proposal 取代此前 task 020 的需求内容，旧 approval hash、design、testing、implementation 和 acceptance 证据均不得复用，后续必须基于本版本重新执行完整任务。

## Scope

### In scope

- SN 显式配置同一公网 IP 下的至少两个 NAT 探测/反射 UDP 端口，默认关闭。客户端从同一本地 UDP socket 向两个不同目标端口发送带随机 token 的小包；SN 无状态回显观察到的完整源 endpoint 与 token。
- `sn-miner-rust` 与 `SnServiceConfig` 只配置探针端口列表。SN 从 identity cert 的静态 WAN IPv4 endpoint 推导唯一 advertised IP，并绑定相应端口；启用探针但没有唯一 advertised IPv4、端口重复或只有一个端口时配置无效。SN 通过 `ReportSnResp.nat_probe_endpoints` 的版本化可选尾扩展下发由该 IP 与端口组成的实际 endpoints；客户端不得从普通 SN endpoint 猜测。0 个 endpoint 表示关闭，缺字段按旧节点/关闭处理。
- 探针输出先记录真实 mapping observation，再派生 tunnel 策略类型：
  - `Unknown`：样本缺失、失败、不一致或过期；它是状态，不是第三种 NAT 类型。
  - `NonSymmetricLike`：面向同一 SN IP 的不同目标端口时，完整 observed endpoint 未变化。
  - `SymmetricLike`：面向同一 SN IP 的不同目标端口时，完整 observed endpoint 发生变化。
- `SymmetricLike` profile 可携带两个观测端口之间的 `port_delta`、奇偶关系与样本时间，但这些字段只作为有界预测 hint，不承诺 NAT 面向 peer IP 时使用相同步进。若 observed IP 变化、端口 hint 不可用或样本不一致，则不得生成预测窗口。
- `Public` 不由双端口比较产生。只有独立可靠的本地公网 endpoint/静态 WAN 可达证据才能启用 public fast path。
- 额外探针端口若用于从未联系的源端口回包，只能产生同一 SN IP 下的 filtering hint。它不得生成 ConeLike、RestrictedConeLike 或 PortRestrictedConeLike 类型，不得将一次 timeout 当成确定过滤行为，也不得单独作为跳过 peer punch 的依据。
- SN `PeerManager::CachedPeerInfo` 保存每个 peer 最新 `NatProfile`、观测时间和过期状态。Profile 只保存在 SN 内存 peer 信息中，不写 desc/sec、不做磁盘持久化、不跨 SN 同步。
- SN client 保存自己的本地 profile 与探针状态，但不建立 `remote_nat_profiles` 或其他远端 peer/profile 跨建链缓存。`SnQueryResp.net_profile` 是 caller 本次 peer 查询结果的一部分，必须随临时 peer lookup 结果直接交给当前 tunnel 建链上下文，不得塞入 identity-cert cache；`SnCalled.nat_context.caller_profile` 则把 caller 实际用于计划的 profile 快照交给 callee，`callee_profile` 必须等于 caller 本次 query 使用的快照。
- 客户端本地 profile 按 `sn_peer_id` 隔离，属于对应 `ActiveSN`/per-SN 状态；callee 必须用 `SnCalled.sn_peer_id` 选择本地 profile，不得用一个跨 SN 的全局 profile。分布式查询时 `SnDetailResp`/`ServingPeerDetail` 必须携带目标 peer 的同一 profile 快照，使最终 `SnQueryResp.net_profile` 与本地 SN 查询语义一致；这只是响应转发，不是跨 SN profile 状态复制。
- Caller 生成计划时把实际使用的 caller 本地 profile 与 `SnQueryResp.net_profile` 封装成版本化、可选的 `NatTraversalContext`，随 `SnCall` 发送并由 SN 原样转入 `SnCalled`。Callee 使用该上下文中的同一有序快照生成动作；不得在 query→call 间改用后来刷新的 profile，避免双方得出不同 connector。缺失、非法或与 peer 角色不匹配的上下文按 `Unknown` 回退。
- `SnCallResp` 不承载远端 profile，也不是 caller 选择或启动 `ConnectPlan` 的门禁。Caller 在发送 `SnCall` 前已经使用本次 `SnQueryResp.net_profile` 生成计划；rendezvous 发送与 connector/`PunchOnly`/`WaitIncoming` 动作由同一 logical-tunnel owner 并发驱动。若 lookup 路径没有本次 `SnQueryResp`、响应缺字段、值不兼容或已过期，则立即按 `Unknown` 启动兼容基线，不额外等待 `SnCallResp`。
- SN 控制消息继续使用 additive/可版本化字段传递 profile。旧节点缺字段、旧 profile 值无法安全转换或任一侧 profile 过期时，一律按 `Unknown` 进入兼容基线。
- `TunnelManager` 用确定性的 `ConnectPlan` 替代单一 `prediction_mode`。计划至少包含：caller/callee 有序 profile、connector、connector candidate mode、另一侧 `PunchOnly` candidate mode、`WaitIncoming`、是否为 reverse QUIC、owner/deadline 和 PN/legacy fallback。
- 对双方均非 Public、profile 新鲜且所需预测 hint 可用的情况，采用以下组合矩阵。`base` 表示 SN 当前观察到的候选，仍然只是 best-effort；`predicted` 表示受 TTL、数量、奇偶和总并发上限约束的窗口。

| Caller A | Callee B | Connector | Peer action | Reverse QUIC? |
|----------|----------|-----------|-------------|---------------|
| `NonSymmetricLike` | `NonSymmetricLike` | A connect `B.base` | B punch `A.base`, then wait incoming | no |
| `NonSymmetricLike` | `SymmetricLike` | B connect `A.base` | A punch/spray `B.predicted`, then wait incoming | yes |
| `SymmetricLike` | `NonSymmetricLike` | A connect `B.base` | B punch/spray `A.predicted`, then wait incoming | no |
| `SymmetricLike` | `SymmetricLike` | A connect/spray `B.predicted` | B punch/spray `A.predicted`, then wait incoming | no; caller is deterministic tie-break |

- 混合 `NonSymmetricLike + SymmetricLike` 配对优先让 SymmetricLike 一侧成为 connector，使 QUIC 面向 NonSymmetricLike 的单个基础候选；另一侧只对 SymmetricLike 的预测窗口发送廉价 punch/spray，避免为每个预测端口启动完整 QUIC connect。
- `SnCall`/`SnCalled` 只承担 rendezvous、不可变 `NatTraversalContext`/candidate 传递和动作触发，不天然表示“被叫必须反连”。`on_sn_called` 必须使用 `SnCalled.nat_context` 的 caller/callee 有序快照按计划选择 `Connect`、`PunchOnly` 或 `WaitIncoming`，不得另取刷新后的 profile 或无条件调用反向 `open_direct_path`。
- `PunchOnly` 从 QUIC listener 的同源 UDP socket 发包，只负责开孔，不发起 QUIC。它必须绑定当前 logical tunnel 的 owner、deadline、取消与 listener-close；入站 tunnel 成功、任务取消、deadline 或 listener close 任一发生时立即停止，不允许 detached 发送。
- TunnelManager 将现有仅接受 reverse tunnel 的 waiter 泛化为带 expected direction 的 incoming-plan waiter，并在 `SnCall` 或 connector 启动前注册。Active 与 reverse 入站都能按 `(remote_id, tunnel_id, direction)` 通知对应计划；waiter guard 在成功、deadline、owner drop 或失败时负责删除注册并停止 `PunchOnly`。
- Connector 的 QUIC Initial 同时承担 connector 一侧的出站开孔。最终成功只由 QUIC/TLS 握手确认；punch 包本身不表示成功，不新增 ack/echo。
- `SymmetricLike` 但 prediction hint 不可用时不得进入预测矩阵。若一侧有独立 Public 证据，则另一侧可向 Public 侧发起 QUIC；否则只允许有界 best-effort，并由既有 PN proxy 收敛。
- 任一 profile 为 `Unknown`、缺失、过期或 mixed-version 不兼容时，保持现有 direct/reverse hedge、首包偏移、punch cadence、退避和 PN fallback 基线。
- `sn-miner-rust` 提供探针端口配置；0 个端口表示关闭，1 个端口配置无效，2 个端口可完成 mapping stable/changed 观察，3 个及以上端口只能增加样本或可选 filtering hint，不能扩大 NAT 类型结论。

### Out of scope

- 通过单 SN IP 多端口宣称识别传统四类 NAT、完整 endpoint-independent/address-dependent mapping 或完整 filtering 行为。
- 把 `NonSymmetricLike` 解释为面对所有 peer IP 都复用 SN observed endpoint，或把 `port_delta`/奇偶 hint 解释为确定预测。
- 将一次未收到 alternate-source 响应解释为已证明 port-restricted filtering。
- 在 SN client 中缓存远端 peer/profile，或把 profile 写入身份凭证、desc/sec、磁盘、数据库或跨 SN 同步状态。
- 默认让双方同时发起 QUIC connect；增加 punch ack/echo、业务 UDP punch payload、TURN、UPnP/NAT-PMP、在线学习或长期 keepalive spray。
- 改变 QUIC/TLS success 判定、PN proxy 协议、身份认证或公开 Tunnel API。
- 在 proposal 阶段修改 design、生产代码、测试、配置或运行时资源。

### Boundary with neighboring modules

- `p2p-frame/src/sn/nat_probe.rs` 拥有双端口探测协议、真实 observation 与预测 hint；不决定 tunnel connector。
- `p2p-frame/src/sn/service/**` 拥有 SN `CachedPeerInfo` profile；`ReportSnResp.nat_probe_endpoints`、`SnQueryResp.net_profile`、分布式 detail 响应和 `SnCalled` 的 traversal context 只形成当次传递。`p2p-frame/src/sn/client/**` 按 SN 拥有本地探测，不拥有远端跨建链 cache；`DeviceFinder`/tunnel lookup 必须把本次 query 的临时 peer 信息传到 `TunnelManager`，不能只返回 cert 后丢弃 profile。
- `p2p-frame/src/tunnel/**` 拥有两方有序组合矩阵、connector 方向、candidate 生成和 logical tunnel 编排；不得把 SN call 与反连绑定。
- `p2p-frame/src/networks/quic/**` 拥有 connect-owned 和 punch-only 两类同源发送生命周期；不推断 NAT 类型，不自行改变 caller/callee 角色。
- `sn-miner-rust` 只承载显式探针端口配置；未配置时保持现有行为。
- `cyfs-p2p` 与 `cyfs-p2p-test` 只消费兼容字段与构造验证场景，不重新定义 profile 或建链矩阵。

## Requirement Review

- 将 NAT 分类收敛到双端口真实 observation 是必要修正。Mapping 与 filtering 是独立维度；同一 IP 的不同端口不能证明面对不同 peer IP 的行为，也不能恢复传统四类 NAT。
- `NonSymmetricLike` 只表示本次对同 SN IP 改变目标端口时未观察到 mapping 变化。因此 `base` candidate 仍是 best-effort，所有非 Public 组合都必须保留 PN fallback。
- 双边组合是有序的：`N→S` 与 `S→N` 需要不同 connector。混合配对让 S 侧连接 N 侧，可把预测工作限制为廉价 punch，而不是多个完整 QUIC connect。
- `S→S` 用 caller connector、callee puncher 作为确定性 tie-break，牺牲无条件双向 QUIC 竞速，换取更少 endpoint-dependent mapping、明确资源预算和一致的 tunnel ownership。
- SN client 不缓存远端 profile 后，caller 策略必须由本次 `SnQueryResp.net_profile` 直接驱动，callee 策略必须由本次 `SnCalled.nat_context` 中原样转发的有序快照驱动。冷缓存首次 peer query 已经取得远端证书、endpoint 和 profile，不再增加一次等待 `SnCallResp` 的依赖；没有本次 query profile 的路径直接按 `Unknown` 使用旧时间线。
- `PunchOnly` 比 reverse connect 更轻，但从现有 `Quinn::Connecting` 生命周期拆出后仍必须拥有等价的 owner、deadline、取消和 listener-close 约束。
- 两端必须使用同一个纯函数式矩阵，并以 caller/callee 顺序作为输入；否则 profile 相同也可能选择不同 connector，形成双方都等待或双方都 connect 的故障。

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-NTAT-1 | sn_nat_probe_ports | SN 提供默认关闭的同 IP 双端口最小探针，并通过 `ReportSnResp.nat_probe_endpoints` 下发实际目标；客户端从同一本地 socket 得到完整 endpoint 的 stable/changed/unknown observation，额外端口只增加样本或 filtering hint | `sn/nat_probe` 与 `sn-miner-rust` 配置；token、包长、限速、IPv4 UDP 和 versioned optional response tail 边界不放松 | 无法输出传统四类 NAT，换取结论可被真实证据支持 | unit/DV 覆盖 0/1/2/3+ 配置、旧响应无 endpoints、完整 endpoint 比较、token、丢包/不一致 Unknown，以及 alt-source 不扩大类型 | 不实现多 IP STUN 四象限或把探针端口用于数据通道 |
| P-NTAT-2 | nat_type_peer_cache_and_exchange | SN `CachedPeerInfo` 保存最新 profile；SN client 按 `sn_peer_id` 仅保存本地 profile；caller 通过本次 `SnQueryResp.net_profile` 得到 callee profile，并将双方计划快照随 `SnCall`/`SnCalled` 转发给 callee；分布式 detail/query 保持相同返回语义；不建立远端 cache，`SnCallResp` 不作为 profile 来源或计划门禁 | 内存 peer 信息、per-SN 本地状态、临时 query/traversal context 与 additive/versioned wire；不写 desc/sec、identity-cert cache、磁盘或跨 SN 状态复制 | 每次 NAT-aware 建链需要本次 peer query；无 query profile 时立即兼容回退，换取首次尝试不依赖历史 profile 缓存、刷新竞态或额外等待 call response | 测试覆盖 SN 写入/TTL、per-SN 隔离、冷状态首次 local/distributed query、快照一致性、无 remote map、SnCallResp 非依赖、旧新双向解码和 Unknown 立即回退 | 不新增独立持久化查询协议或远端 peer cache |
| P-NTAT-3 | nat_type_aware_strategy_selection | TunnelManager 按有序 N/N、N/S、S/N、S/S profile 组合生成显式 ConnectPlan，并单独处理 Public、Unknown、过期和不可预测 S | 只改变内部 tunnel 策略，不改变身份、公开 API、QUIC/TLS success 或 PN 协议 | 显式矩阵比布尔 intent 更复杂，但能表达 connector 方向和 peer action | 单元测试覆盖四组合及顺序、Public、Unknown、过期、mixed-version 与不可预测 S | 不根据未经观察的传统 NAT 标签选择策略 |
| P-NTAT-4 | symmetric_port_prediction | 对 Changed/SymmetricLike observation 仅生成有界 best-effort 预测窗口；完整 endpoint 变化但无端口 hint 时禁止预测 | IPv4 QUIC ServerReflexive candidate；TTL、候选数、总并发、去重和奇偶均封顶 | 有限样本可能预测落空并产生短时 UDP 流量 | unit/DV 覆盖 delta/奇偶、IP 变化、无 hint、窗口上限、去重、命中与落空 PN | 不承诺随机或跨目标 IP 分配器可预测，不做常驻 keepalive |
| P-NTAT-5 | nat_aware_connect_flow | 分离 rendezvous、Connect、PunchOnly 和 WaitIncoming；按矩阵默认只保留一个 QUIC connector，另一侧 punch 后等待，N/S 仅在 callee S 时真正反连 | TunnelManager 编排与 QUIC listener 同源生命周期；保持 Unknown 基线和 PN fallback | 新增 punch-only owner 与方向切换，换取避免不必要反连和双向 mapping 扰动 | unit/DV/integration 覆盖四组合动作、首次 profile 时序、首个成功取消、deadline、owner drop、listener close、重复 tunnel 与 PN fallback | 不允许 detached punch、双方无条件 connect 或 punch success 判定 |

## Success Criteria

- Profile 只表达单 SN IP 双端口实际观察到的 stable/changed/unknown；代码和文档不再声称获得完整四类 NAT。
- `Public` 只来自独立公网可达证据；alternate-source filtering hint 不生成 Cone/PortRestricted 类型，也不作为跳过 peer punch 的充分条件。
- SN `CachedPeerInfo` 保存 profile；SN client 不存在远端 profile map，本地 profile 按 `sn_peer_id` 隔离。冷状态首次 `SnQueryResp.net_profile` 直接驱动 caller 计划，同一双方快照经 `SnCall`/`SnCalled` traversal context 驱动 callee 计划；本地与分布式 query 语义一致；`SnCallResp` 不提供 profile，caller 不等待它才启动计划。
- N/N、N/S、S/N、S/S 分别产生 proposal 矩阵规定的 connector、candidate mode 和 PunchOnly 动作；N/S 与 S/N 的方向差异有明确证据。
- 每个正常计划默认只有一个 QUIC connector；PunchOnly 与 direction-aware incoming-plan waiter 共享 owner、deadline、取消和 listener-close 生命周期，active/reverse 入站成功或终止后都没有后台残留发送。
- 预测只使用新鲜、可用的 hint，窗口、奇偶、候选数和总并发受限；不可预测或落空时由既有 PN fallback 收敛。
- Unknown、过期、缺字段和 mixed-version 保持现有建链时间线，不因新 profile 语义产生额外依赖。
- 旧 task 020 的 design、implementation、testing 和 acceptance 证据不复用；后续阶段必须基于本 proposal 重新生成并验证完整证据链。
- Explicit non-goals: 不做多 IP STUN 完整分类、持久化/跨 SN profile、TURN/ack/echo/在线学习，不改变身份、公开 Tunnel API 或 PN 协议。

## Risks

- Same-IP stable 不等于 cross-IP stable；错误把 NonSymmetricLike 当作 endpoint-independent 会继续产生错误候选。
- 两个端口只能给出有限预测样本；delta/parity 可能因 CGNAT、负载和分配冲突漂移，必须限制为 hint。
- N/S 将 callee 设为 connector，会改变 reverse waiter、publish、锁和重复 tunnel 时序；design/testing 必须防止入站先于 waiter 注册。
- PunchOnly 脱离 `Quinn::Connecting` 后可能发生 deadline 后发送或 listener close 后残留；必须建立等价 owner。
- NatProfile wire 语义改变可能让旧值被新节点误解；不能静默重用旧枚举值表达更强或不同含义。
- 两端计划函数不一致可能造成双方同时等待或同时 connect；caller/callee 顺序、profile freshness 和 hint validity 必须确定性。
- 探针与 spray 都面对公网不可信 UDP；token、长度、频率、candidate 总数和默认关闭配置必须继续限制放大面。

## Trigger Matrix

| Trigger Category | Applies? | Evidence | Required Checks | Completed Checks | Deferred Checks and Reason | Residual Risk |
|------------------|----------|----------|-----------------|------------------|----------------------------|---------------|
| contract/protocol | yes | `NatProfile` 语义、`SnQueryResp.net_profile`、`SnCall`/`SnCalled.nat_context` 消费方式及 `SnCallResp` 非依赖发生变化 | design 列出版本化字段、旧值映射、临时 peer-info/快照传递和 caller/callee 消费者；testing 做新旧双向正负兼容 | proposal 已要求不安全转换 fail-safe 到 Unknown 且立即使用旧时间线 | owner: design/testing; reason: 编码方案待下游；acceptance impact: mixed-version、快照分歧或错误等待 SnCallResp 的证据缺失阻断验收 | 两端可能选择不同计划 |
| data/schema | no | Profile 只存在 SN 内存 peer 信息和单次 tunnel context，不进入持久化 schema | 审计 design Scope Paths 无持久化路径 | proposal 明确禁止 desc/sec、磁盘和跨 SN 同步 | owner: acceptance; reason: 仅需边界审计；acceptance impact: 发现持久化改动退回 proposal | TTL 漂移属于 runtime 风险 |
| security/privacy/permission | yes | SN 探针与 punch/spray 使用公网不可信 UDP | 保持 token、包长、限速、默认关闭；测试伪造、超时和发送预算 | proposal 已限定最小协议与有界流量 | owner: design/testing; reason: abuse case 待生成；acceptance impact: 无负例或预算证据阻断验收 | 误分类可能放大 UDP 流量 |
| runtime/integration | yes | connector 方向、reverse waiter、首次 rendezvous、punch owner、取消和 PN 收敛均变化 | design 描述并发/顺序/超时/恢复；unit、DV、integration 覆盖四组合 | proposal 已定义动作矩阵和终止条件 | owner: design/testing; reason: 可运行场景待后续；acceptance impact: 生命周期证据缺失阻断验收 | 重复 tunnel、双等待或任务泄漏 |
| build/dependency/config/deployment | yes | SN 探针端口配置语义变为 0=关闭、1=无效、2=最小 mapping observation、3+=附加样本/hint | design 说明旧配置兼容和错误处理；testing 覆盖各端口数量 | proposal 保持显式启用和默认关闭 | owner: design/testing; reason: 配置接口待设计；acceptance impact: 配置兼容失败阻断验收 | 运维可能误解第三端口能力 |
| ui/datamodel/workflow | no | 不修改 UI、展示数据模型或用户工作流 | 审计 Scope Paths 不含 UI | proposal 无 UI 消费者 | owner: none; reason: not applicable; acceptance impact: none | none |
| harness/process | no | 本次仅重写 task 020 业务需求，不修改 Harness 规则、脚本、模板或 CI | 后续恢复正常阶段检查 | proposal 不包含 Harness 改动 | owner: none; reason: not applicable; acceptance impact: 后续 checker 失败仍阻断执行 | none |

## Approval Record

- approver:
- approval_date:
- user_statement: ""
