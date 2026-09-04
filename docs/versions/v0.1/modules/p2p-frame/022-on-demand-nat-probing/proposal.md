---
module: p2p-frame
task_name: 022-on-demand-nat-probing
submodule: 022-on-demand-nat-probing
version: v0.1
status: draft
approved_by:
approved_at:
approved_content_sha256:
---

# SN 驱动的 NAT 探测 Proposal

## Background and Goal

Task `020-nat-type-aware-traversal` 当前把本地 `NatProfile` 的有效期固定为 10 分钟，并由 SN client 在周期 report 路径发现 profile 过期时重新执行公网 UDP 探测。10 分钟对于通常稳定的 NAT mapping 类型过短，但完全取消周期校验也会让 SN 无法发现未体现为 command tunnel 外网 endpoint 变化的 NAT 行为漂移。

相比客户端，SN server 更清楚 peer 何时上线，也能从已认证 command tunnel 直接观察客户端当前外网源地址和实际传输协议。SN 因而应成为探测调度者：只有当前 client↔SN registration 使用 QUIC command tunnel 时，SN 才能在 peer 上线、SN 实际观察到的客户端外网地址变化、探针配置变化、确有缺失 profile 的连接需求，或距离上次完成的 probe 已达到 2 小时时签发一次版本化 probe directive；客户端只执行有效 directive，不再根据本地 TTL、`reset_sn`、`conn_id` 或本地猜测自行决定探测时机。

## Scope

### In scope

- 删除客户端以 10 分钟 `NatProfile` TTL 自主触发 probe 的行为，改为 SN 对当前权威 QUIC registration 统一调度每 2 小时一次的周期 probe。
- 2 小时周期从最近一次有效 directive 对应的 probe 完成时刻重新计算。上线、外网地址变化、probe 配置变化或真实 demand 提前触发并完成 probe 后，必须重置下一次周期 deadline，不能在旧 deadline 再重复签发。
- 周期 deadline 到达时若已有同 generation probe 执行中，SN 不得并发签发第二条 directive；当前任务完成后再从完成时刻计算新的 2 小时周期。peer 不再拥有权威 QUIC registration 时取消其周期调度。
- 每条 directive 必须有有界执行 deadline。Client 未在 deadline 内回传匹配结果时，SN 将该次 probe 终结为 `Unknown` 并从终结时刻计算下一次 2 小时周期；不得让一个永久 in-flight 状态停止后续周期，也不得立即无退避重发。
- NAT probe eligibility 必须绑定当前已认证 client↔SN command tunnel 的实际协议。只有承载当前 online registration/report 的 tunnel 为 `Protocol::Quic` 时，SN 才能签发 directive，client 才能执行 probe；TCP-only 或无法确认协议时必须保持 `Unknown`，继续 legacy/PN fallback。
- 若同一 peer 同时存在 TCP 与 QUIC command tunnel，SN 必须选择并记录一个当前 QUIC registration generation 作为 NAT profile 的权威 session；TCP tunnel 的上线、report、query/call demand 或外网 endpoint 变化不得单独触发 probe，也不得覆盖该 QUIC generation 的 observation。
- 当前权威 QUIC tunnel 失效且 peer 只剩 TCP tunnel 时，SN 必须停止发布绑定该 QUIC generation 的 profile。之后只有新的 QUIC registration 才能创建 generation 并重新签发 directive；不得沿用旧 QUIC profile，也不得在 TCP 上补探测。
- SN 为每个已认证 peer 维护内存态 NAT probe scheduling state，至少包含：当前在线/registration generation、SN 实际观察到的客户端外网源 endpoint、当前有效 profile、最新 directive generation/request id、执行中状态和失败退避状态。
- SN 只能使用 command tunnel 实际观察到的远端 endpoint 判断外网地址，不得信任 `ReportSn.local_eps`、identity cert 自报 endpoint 或客户端提交的 profile endpoint 作为变化触发证据。当前 `ReportSn` handler 已持有 `peer_id` 与 `tunnel_id`；后续 design 必须把 directive 绑定到实际 report tunnel，并规范化多 tunnel/protocol 观察，避免 tunnel 集合顺序或临时并存造成重复 probe。
- SN 在以下事件签发新的 probe directive：
  - peer 首次完成当前 serving/online registration，且 SN 已配置有效 probe endpoints；
  - SN 从后续已认证 report/control traffic 观察到该 peer 的外网源 endpoint 相对已记录值发生变化；
  - SN 的 probe endpoint 配置 generation 发生变化；
  - profile 缺失/`Unknown` 且 SN 收到针对该 peer 的真实 query/call 需求，失败退避允许重试。
  - 当前权威 QUIC registration 距离最近一次 probe 完成已达到 2 小时。
- `reset_sn`、SN command tunnel 重建和客户端 stack reset 不再作为客户端本地 probe 判断条件。它们只负责重新建立连接并发送认证 report；SN 将新的 serving/online registration 或新的实际外网观察转换为 directive。
- Probe directive 必须是可版本化、可选且有界的 SN→client 指令，至少绑定 SN identity、peer identity、权威 QUIC tunnel/registration generation、directive generation/request id 与 SN 配置的 probe endpoints。它可以由 `ReportSnResp` 在 QUIC report 时返回，也可以由 SN 在 query/call demand 时通过当前已认证 QUIC command tunnel 推送；具体消息拆分由 design 决定，但两条传递路径必须使用同一调度状态和去重语义，且不得回退到 TCP 传递 probe directive。
- Client 只接受当前 active SN、当前连接状态和最新 generation 的 directive；对同一 directive single-flight 执行一次 probe。重复、旧 generation、错误 SN、非法 endpoint 或晚到结果必须忽略/fail closed。
- Client 完成 probe 后通过版本化 report 回传 directive request id/generation 与 `NatProfile`。SN 只接收与当前 scheduling state 匹配的结果；外网地址已经再次变化或 generation 已前进时，晚到结果不得覆盖新状态。
- QUIC eligibility 只约束何时允许执行 probe；它不把 NAT probe UDP request/response 伪装成 QUIC application message，也不自动要求复用 command QUIC socket。探针是否必须复用特定 UDP socket 属于 task 020 既有 mapping-observation 边界，本任务不静默改变。
- 初次、事件或周期 probe 失败时保存 `Unknown`。失败后仍保留 2 小时周期，同时允许新的触发事件或真实 query/call demand 在退避允许时提前重试；不得形成 2 小时周期之外的无需求轮询。当前 query/call/tunnel 不等待 probe，继续既有 legacy/PN fallback。
- 周期 report/keepalive 可以继续维持 peer 在线注册，但客户端不得据此自主 probe，也不得把 keepalive 时间冒充 `observed_at`。SN profile 的发布资格绑定当前在线 registration、外网观察 generation 与 peer cache 生命周期；成功 observation 超过原 10 分钟后仍可使用，直至事件失效或下一次 2 小时周期结果将其替换。
- peer 下线/cache 移除、新 registration 尚未完成 probe、SN 观察到外网地址变化、probe 配置变化或 directive generation 不匹配后，旧 profile 必须立即停止发布。
- mixed-version、缺 directive 字段、非法/未知 generation、非法 profile 和 `Unknown` 继续 fail closed 到现有 legacy/PN fallback。新 SN 对旧 client 不得形成无限 directive 重试或阻断上线。

### Out of scope

- 使用 2 小时以外的固定 NAT probe 周期，或让 client 与 SN 各自维护一套周期 timer。
- 由客户端根据 `reset_sn`、`conn_id`、本地 endpoint、TTL 或 NAT-aware tunnel 状态独立决定 probe。
- 在 TCP client↔SN command tunnel、协议未知 tunnel 或 QUIC generation 已失效后签发/执行 probe，或用 TCP 作为 directive fallback。
- 新增常驻 OS 网络变化监听器；SN 未实际观察到外网变化时，不主动猜测 Wi-Fi、VPN、CGNAT 或路由器状态变化。
- 把 profile 或调度状态写入磁盘/身份凭证、跨进程持久化、跨 SN 复制，或在客户端建立远端 profile cache。
- 让 query/call 同步等待 probe，或改变 task 020 的 NAT observation 分类、预测算法、连接矩阵、QUIC punch 生命周期、PN fallback 和公开 Tunnel API。
- 在 proposal 阶段修改 task 020 已有 packet、生产代码、测试、配置、design 或 acceptance 制品。

### Boundary with neighboring modules

- `p2p-frame/src/sn/service/**` 拥有每个 peer 的权威 QUIC registration generation、真实外网 observation、2 小时周期 deadline、事件/demand directive 调度、失败退避、结果接收和 profile 发布资格；TCP-only registration 不拥有 NAT profile generation 或周期 timer。
- `p2p-frame/src/sn/protocol/**` 承载 additive/versioned directive 与 result correlation；旧节点缺字段时兼容回退。
- `p2p-frame/src/sn/client/**` 只验证并执行 SN directive、single-flight probe 和回传相关结果，不拥有自主周期 timer 或 tunnel-demand 调度器。
- `p2p-frame/src/nat_type.rs` 保留真实 `observed_at`，并支持 SN registration/observation generation 下的可用性判断；不得把 keepalive 时间解释为重新观测。
- `p2p-frame/src/tunnel/**` 消费 SN 已发布的 profile；缺失/Unknown 时继续原有 fallback，不直接触发本地 probe。
- `sn-miner-rust` 仍只配置默认关闭的 SN probe 端口；配置 generation 由 SN runtime 管理。

## Requirement Review

- 由 SN 驱动探测比客户端根据 TTL/重连猜测更符合信息所有权：上线事件、认证 tunnel、实际外网源地址和 peer query/call demand 都在 SN server 一侧可观察。
- 把 eligibility 限定为 QUIC 是合理的 fail-closed 边界：当前 profile 用于 QUIC NAT traversal，而 TCP control tunnel 的外网 observation 不能证明 UDP/QUIC mapping。代价是 TCP-only client 永远不提供 NAT profile，只能使用旧连接策略与 PN fallback。
- 当前 SN 已能通过 `cmd_server.get_peer_tunnels(peer_id)` 读取 tunnel remote endpoint，但 `handle_report_sn` 尚未使用其 `tunnel_id` 做 session correlation，且当前会聚合 peer 的全部 tunnel。Design 必须补齐“当前 report tunnel + 多 tunnel 规范化”语义，不能直接用无序集合变化触发 probe。
- 将周期从 10 分钟延长到 2 小时可以保留低频校验，同时显著减少稳定节点的 probe 流量。周期必须由 SN 的同一状态机调度，否则 event probe 与客户端 timer 会重复执行。
- 仅修改周期常量而保留现有客户端 TTL freshness 判定仍不充分：SN 无法统一处理上线、地址变化、QUIC eligibility 和 generation 竞态。需要把真实 observation 时间、2 小时 schedule 与当前 registration/外网观察 generation 下的发布资格分开。
- 外网地址变化是重探测信号，但不是新的 NAT 类型结论。SN 必须先撤销旧 profile，再签发新 directive；新结果回来前 query/call 使用兼容 fallback。
- Query/call demand 只用于恢复缺失/失败的 profile，不阻塞当前连接。代价是当前尝试可能仍走 PN，后续尝试才能使用新 profile。
- SN 只能看到经过其 command tunnel 的外网变化；若底层连接迁移后 `remote()` 不更新、或真实 NAT 行为变化但外网源 endpoint 未变化，SN 无法感知。本任务不声称消除这一环境缺口。

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-ODNP-1 | sn_nat_probe_trigger_policy | SN 依据 peer 上线、实际外网源 endpoint 变化、probe 配置 generation 变化及缺失 profile 的真实 query/call demand签发 probe；除明确的 2 小时周期外，其他固定时间和客户端本地状态不触发 | 真实观察来自已认证 command tunnel，必须关联 report tunnel 并规范化多 tunnel 状态 | SN 无法检测未反映到 tunnel remote endpoint 的 NAT 行为变化，换取明确可信的触发所有权 | unit/integration 覆盖首次上线一次、稳定在线零重复、外网地址变化一次、配置变化一次、query/call demand 退避及多 tunnel 去重 | 不用客户端自报 endpoint 或 TTL 决策，不新增 OS 网络监听 |
| P-ODNP-2 | sn_nat_probe_directive_protocol | SN→client 使用带 generation/request id 的版本化 directive；client single-flight 执行并相关回报，SN 拒绝重复、跨 SN、过期和晚到结果 | endpoints 只能来自 SN 已验证配置；directive/result 绑定认证 peer、SN 和当前 observation generation | 增加一组低频 wire correlation 状态，换取服务端可控调度和竞态闭包 | codec/unit/DV 覆盖旧新双向兼容、重放、乱序、晚到、错误 identity、非法 endpoint、并发重复及结果回写 | 不改变 probe UDP 包语义，不允许 directive 充当放大器或上线门禁 |
| P-ODNP-3 | nat_profile_server_owned_validity | SN 区分真实 observation 时间与当前 registration/外网 observation generation 下的 profile 发布资格；经过 10 分钟不失效，事件失效或新 probe 返回 Unknown 时立即 Unknown | peer 下线/cache 移除/新 generation 无结果时旧 profile不可发布；客户端和 tunnel 不自行续租 | profile 在两次周期 probe 之间可能暂时陈旧 | 时间推进、断线、重连、地址变化、周期成功/失败、配置变化、cache 清理和 query/context integration 证明 2 小时内可复用且无旧 profile 泄漏 | 不持久化、不跨 SN 复制、不改写 `observed_at`、不把 profile 当连通保证 |
| P-ODNP-4 | sn_nat_probe_quic_eligibility | 只有当前权威 client↔SN registration tunnel 为 QUIC 时才允许 directive、probe 和 profile 发布；TCP-only/unknown protocol fail closed，QUIC 消失后旧 generation 失效 | 协议以 SN 认证 tunnel 的实际 transport 为准；多 tunnel 时 TCP 不能覆盖或触发 QUIC generation | TCP-only peer 无 NAT-aware profile，换取不以 TCP observation 推断 UDP/QUIC mapping | unit/integration 覆盖 QUIC、TCP、未知协议、TCP→QUIC、QUIC→TCP、并存 tunnel、旧 QUIC 结果晚到和无 TCP directive fallback | 不要求 probe 包承载在 QUIC application stream，也不静默改变 probe socket 复用语义 |
| P-ODNP-5 | sn_nat_probe_two_hour_schedule | SN 对权威 QUIC registration 保留 2 小时周期 probe；任意提前 probe 完成后重置 deadline，执行中不重入，失去 QUIC eligibility 时取消 | timer 只存在于 SN per-peer scheduling state，client 无独立 timer；失败受同一退避和 single-flight 约束 | 每 2 小时仍产生一次低频 UDP probe，换取发现未表现为 control endpoint 变化的 NAT 行为漂移 | fake-time/unit 覆盖 2 小时前零 probe、deadline 一次、event 重置、执行中跨 deadline、失败、下线取消及多 peer 独立调度；DV 验证真实周期 directive 计数 | 不使用 10 分钟或其他周期，不允许 event 与 periodic 双重签发 |

## Success Criteria

- 节点稳定在线时，2 小时 deadline 之前不得因时间经过产生 NAT probe；到达 deadline 后 SN 恰好签发一次 directive，客户端没有独立周期 probe。
- 上线、地址变化、配置变化或 demand 在周期中途完成 probe 后，下一次周期必须推迟到该次完成后的 2 小时，不得保留旧 deadline。
- peer 首次上线时 SN 至多签发一个有效 directive；客户端只执行当前 generation 一次并相关回报。
- Peer 首次上线使用 TCP 时不签发 directive、不执行 probe、不发布旧 QUIC profile；使用 QUIC 时才进入上述流程。TCP→QUIC 切换创建新 generation，QUIC→TCP 且无其他权威 QUIC tunnel 时立即撤销 profile。
- SN 从当前已认证 report/control tunnel 观察到客户端外网源 endpoint 变化时，先停止发布旧 profile，再签发新 directive；新结果前 query/call 使用 legacy/PN fallback。
- `reset_sn` 或 command tunnel 重建不要求客户端识别“需要 probe”；重新认证 report 后由 SN online/observation state 决定是否签发。
- 初次失败后不进行 2 小时周期之外的无需求轮询；到达下一次 2 小时 deadline 时仍重试，真实 query/call demand 也可在退避允许时提前触发一次新 directive，但当前请求不等待。
- 成功 profile 的 `observed_at` 保持真实采样时间；超过原 10 分钟仍可在相同 registration/observation generation 下用于 query 和 `NatTraversalContext`，下一次 2 小时 probe 结果按 generation 原子替换它。
- 重复、乱序、错误 SN/peer、过期 generation 和地址变化后的晚到结果不能恢复旧 profile或触发重复 probe。
- Required evidence: unit/fake-time 覆盖 SN 调度状态机、精确 2 小时周期、event deadline 重置、QUIC eligibility、外网 observation、generation、退避和并发；DV 覆盖 QUIC directive→UDP probe→相关 report、周期计数以及 TCP 零 probe；integration 覆盖 report/query/call/context、断线重连、外网地址变化、TCP/QUIC 切换、多 tunnel 和 mixed-version fallback。
- Explicit non-goals: 不改变 NAT 类型分类与穿透矩阵，不使用 2 小时以外周期，不新增 OS 网络监听、持久化、跨 SN 同步、同步等待 probe 或公开 Tunnel API。

## Risks

- 当前 `get_peer_observed_ep` 聚合 peer 的全部 command tunnels；直接比较该集合会把 tunnel 增减/顺序变化误判为公网变化。Design 必须绑定 report `tunnel_id` 或定义稳定的多 tunnel observation key。
- 若只从 remote endpoint 的 `Protocol` 推断权威 tunnel，却不关联 `tunnel_id` 和认证 peer，可能由并存 TCP/QUIC tunnel 造成错误 profile 归属；协议、identity、tunnel 和 generation 必须一起校验。
- 外网地址变化与 directive/result 并发时可能让旧 probe 覆盖新状态；generation 必须在 SN 和 client 两端原子校验。
- 新 directive wire 若被旧 client 忽略，SN 不能反复推送形成流量或日志风暴；mixed-version 能力和退避必须 fail closed。
- profile validity 若直接覆盖现有 `valid_until` 语义，旧节点可能把 profile 解释得过强。Design 必须给出兼容编码或 SN 本地 lease 分层。
- query/call demand 触发 probe 可能被恶意请求放大；只有已通过现有权限校验的真实需求才可触发，并受 per-peer single-flight、退避和全局容量限制。
- 大量 peer 的 2 小时 deadline 若从 SN 启动或整点同步，可能形成 probe burst；design 必须保持每个 peer 以上一次完成时刻为基准，并提供有界调度容量，不得把周期性 deadline 提前到距最近完成不足 2 小时；明确的上线、地址变化、配置变化和退避允许的真实 demand 提前触发属于事件例外。
- Directive 或结果丢失可能把 per-peer 状态永久留在 in-flight；有界执行 deadline 必须终结该 generation，并与失败退避、下一周期和晚到结果拒绝共享同一状态机。

## Approval Record

- approver:
- approval_date:
- user_statement: ""
