---
module: p2p-frame
task_name: 024-tunnel-rendezvous-protocol
submodule: 024-tunnel-rendezvous-protocol
version: v0.1
status: draft
approved_by:
approved_at:
approved_content_sha256:
---

# SN Tunnel Rendezvous 独立协议 Proposal

## Workflow Tier Judgment

- Proposed tier: `high-risk`
- Final tier: `pending`
- Rationale: 本任务新增独立 SN wire protocol、双端 rendezvous 状态机、跨 SN 转发、QUIC 实际 socket 上的端口预测与 punch、反向建链控制、mixed-version 回退以及公网命令的防滥用边界。错误实现可能造成双方互等、错误预测第三方 endpoint、重复 tunnel、UDP 放大、后台任务泄漏或旧节点建链退化。
- Confirmation statement: 本 proposal 根据用户提出的独立协议需求创建为新的 sibling task；等待用户批准后，才能进入 design、implementation、post-implementation testing 和 acceptance。

## Background and Goal

当前 NAT-aware tunnel 草案把 NAT 上下文与建链触发附加在 `SnCall` / `SnCalled` 上。新的 rendezvous 协议不负责交换或判断 NAT type；TunnelManager 在调用协议前已经根据双方已知 NAT 信息决定目标端 B 应执行 punch、反向连接、两者同时执行或只等待。

端口预测必须由被预测 endpoint 的拥有者执行。A 只能从 A 实际 traversal socket 的观测生成 A 的 endpoint 列表，B 也只能从 B 实际 traversal socket 的观测生成 B 的 endpoint 列表。任何一端都不得拿对端上报的单个 endpoint，在本地替对端重放 delta、parity 或窗口扩展算法。尤其在双方均为 `SymmetricLike` 时，A 必须先获得 B 自己生成的 endpoints，B 也必须先获得 A 自己生成的 endpoints；否则双方没有足够信息完成有界穿透尝试。

本任务定义一个独立、版本化的 `SnTunnelRendezvous` 协议族。Punch/reverse/wait 由协议 operation/message kind 表达；业务 request body 只包含 B 的动作目标 endpoints 和 B 是否需要预测自己的 endpoints。协议不携带 NAT type、NatProfile 或 prediction hint，不把任何 NAT 组合硬编码进 wire contract。

请求中的 endpoint 列表由 A 提供，是 B 反连或 punch 的目标。若请求要求 B 预测，B 从自己的实际 traversal socket 生成预测 endpoints，并在响应中返回；若不要求预测，成功响应的预测 endpoint 列表必须为空。B 在返回成功响应前必须已经完成所需预测，并已启动或可靠 arm 请求动作。旧 `SnCall` 保留为 mixed-version 兼容路径，不在本任务中删除。

目标是建立一个可验证的首次 tunnel 协调合同：endpoint 归属明确、动作唯一、启动有序、成功条件仍为真实 tunnel 建立、失败可回退、生命周期可取消、公网输入受约束。

## Scope

### In scope

#### 1. 独立协议、请求与响应合同

- 新增独立的 `SnTunnelRendezvous` 协议族和独立 command 空间，不复用 `SnCall` / `SnCalled` 编码，也不把新语义伪装成旧 command 的可选字段。
- NAT type 到 action 的选择完全属于 TunnelManager 策略层。wire request 不携带 NAT type、NatProfile、prediction delta/parity、双方 profile snapshot 或策略矩阵。
- Punch/reverse/wait action 由协议 operation/message kind 表达，不属于 request body。业务 request body 必须且只包含以下两个字段；认证 peer、attempt/tunnel correlation、version、deadline、result code 和幂等信息属于协议 envelope：

| Request field | Contract |
|---------------|----------|
| `endpoints` | B 执行 punch 或 reverse connect 的目标 endpoint 列表；列表由 A 提供并绑定 A 身份 |
| `need_predict_endpoint` | B 是否必须从自身实际 traversal socket 预测并返回 B endpoints |

- 响应业务结构包含 `predicted_endpoints`。当 `need_predict_endpoint=true` 且请求成功时，该列表包含 B 自己生成的预测 endpoints；当 `need_predict_endpoint=false` 时，该列表必须为空。
- 当 `need_predict_endpoint=true` 但 B 无法安全生成预测 endpoints 时，不得用成功响应加空列表表达；必须返回 typed failure，避免 A 把“预测失败”误解成“不需要预测”。
- 业务 response body 必须且只包含 `predicted_endpoints`。通用 response result code 在 envelope 中表达 accepted/action-armed 或 typed failure；它不能把 punch 已发送、reverse connect 已启动或预测已返回表示为 tunnel 已连接。
- 协议运行在现有已认证 SN control tunnel 上，支持同一 SN 和跨 SN relay。客户端与 SN 显式协商 protocol version、action 和 endpoint format capability。
- 新节点可以用该协议覆盖旧 `SnCall` 的通知和反向建链功能；旧 `SnCall` 继续作为 compatibility path。同一 logical attempt 不得同时启动新旧协议。
- Exact command id、binary field layout、Rust 类型名和 decoder remainder 策略属于 design 阶段；design 必须遵守 versioned、长度有界和旧消息不可误解为新消息的合同。

#### 2. 显式目标动作模式

协议以互斥枚举表达目标端 B 的动作，不使用多个布尔值组合；`need_predict_endpoint` 与动作正交：

| Target mode | Request endpoints | 目标端 B 的动作 | A 收到成功响应后的动作 |
|-------------|-------------------|----------------|------------------------|
| `PunchOnly` | 必须非空 | 对列表发送有界 punch，然后等待 A 入站 | 使用既有 B endpoint 或响应中的 B predicted endpoints 主动 connect |
| `PunchAndReverseConnect` | 必须非空 | 对同一列表 punch，并作为唯一 connector 反向 connect | 先安装入站 waiter；需要时对响应中的 B predicted endpoints punch |
| `ReverseConnectOnly` | 必须非空 | 作为唯一 connector 对列表反向 connect，不发送 punch | 等待 B 入站 |
| `WaitIncoming` | 必须为空 | 仅安装入站 waiter | 使用既有 B endpoint 或响应中的 B predicted endpoints 主动 connect |

- 每个 request 必须绑定不可变 action、endpoint 列表、prediction flag、允许的 transport、deadline 和 logical tunnel owner；B 只执行 request 指定的动作，不根据本地 NAT type 重新选择策略。
- Punch mode 仅适用于 QUIC/UDP traversal。`ReverseConnectOnly` 和 `WaitIncoming` 可以在 design 明确支持时承载 TCP 或 QUIC reverse/direct connect，但不得对 TCP 使用 UDP punch 语义。
- `PunchAndReverseConnect` 不等于双方同时建立 QUIC：B 是唯一 connector；A 的 punch 只协助 B 的 reverse connect，A 等待入站。
- 四种 action 覆盖 B 对当前 attempt 的完整动作空间：不 punch/不 connect、只 punch、只 connect、punch+connect。调用方可根据任意已知 NAT 组合选择合法 action，协议自身不接收或解释 NAT type。

#### 3. Endpoint ownership 与端口预测

- Request `endpoints` 是 B 执行 reverse connect 或 punch 的远端目标，由 A 提供并按认证 A 身份校验；B 不修改、不扩展、不重新预测这些 endpoints。
- 当 A 的 NAT 策略要求预测 A endpoints 时，预测必须由 A 自己在发送 request 前完成，再把 concrete endpoints 放入 request。B 不根据 A 的 NatProfile 或 prediction hint 替 A 计算端口。
- 当 `need_predict_endpoint=true` 时，B 必须由 B 自己从实际 traversal socket 生成 concrete predicted endpoints，并通过 response 返回；A 不替 B 预测。
- 当 `need_predict_endpoint=false` 时，response `predicted_endpoints` 必须严格为空，即使 B 本地已有缓存预测结果也不得附带。A 使用在其他查询路径中已经取得的 B endpoint。
- `need_predict_endpoint=true` 的成功响应必须包含至少一个有效 predicted endpoint。无结果、预测过期、样本冲突、socket generation 不匹配或窗口超限均返回 typed failure。
- Request endpoints 和 response predicted endpoints 都必须去重并有硬上限；proposal 基线为每个列表最多 8 个 concrete endpoints，design 只能进一步收紧或用总预算证明调整。
- Endpoint 必须携带或由 envelope 绑定 transport、owner、generation/有效期和 socket binding identity。SN 与双方必须拒绝第三方 IP、非法端口、重复/超限 endpoint、transport 不匹配和过期 generation。

#### 4. 实际 traversal socket 绑定

- QUIC mapping observation、endpoint prediction、punch 和 QUIC connect 必须绑定同一个实际 listener/traversal UDP socket，或绑定到 design 能证明具有相同 NAT mapping 的同一 socket ownership unit。
- 不得使用临时 `0.0.0.0:0` probe socket 得出的端口序列，去预测另一个 QUIC listener socket 将获得的外部映射。
- `socket_binding_generation` 在 listener 重建、端口变化或 ownership 迁移后必须变化；旧 predicted endpoints 立即失效。
- punch 必须从随后接收/发起 QUIC 的同源 UDP socket 发送，受 logical tunnel owner、deadline、取消和 listener-close 控制，不允许 detached sender。

#### 5. Request/Response rendezvous 时序

协议至少覆盖以下语义消息；实际 command 拆分和字段布局由 design 固化：

| Phase | Semantic message | Required outcome |
|-------|------------------|------------------|
| Request | `RendezvousRequest` / `RendezvousNotify` | A 提交 action、B 的动作目标 endpoints、`need_predict_endpoint`、deadline 和 attempt envelope；SN 认证并通知 B |
| Response | `RendezvousResponse` | B 完成可选预测，安装必要 waiter，启动或可靠 arm 请求动作；成功时按 prediction flag 返回非空或空的 predicted endpoints |
| Terminal | `RendezvousComplete` | 任一端报告真实 tunnel 建立或终态失败，供对端和 SN 收敛临时状态 |
| Terminal | `RendezvousCancel` | owner drop、deadline、策略切换、重复 attempt 淘汰或显式取消时停止双方动作 |

- A 在发送 request 前建立 attempt owner；若 B 会 reverse connect，A 必须先安装 incoming waiter。
- B 按固定顺序处理：认证/校验 request -> 必要时预测自己的 endpoints -> 安装 B waiter -> 启动或原子 arm punch/reverse action -> 返回 response。
- `need_predict_endpoint=true` 时，B 必须在 response 前完成预测；超时或无结果返回 typed failure。`false` 时不得为了填充 response 主动预测，且列表严格为空。
- A 只有在收到匹配同一 attempt 的成功 response 后才使用 B predicted endpoints 发起本地 connect/punch；但 B 的 reverse connect 可以在 response 发送前已经启动，因为 A waiter 已先安装。
- 成功 response 表示 prediction contract 已满足且 B action 已 armed，不表示 punch 命中、reverse connect 成功或 tunnel Connected。
- `Complete` / `Cancel` 对远端通知可以 best-effort，但本地 owner 必须可靠停止任务并移除 waiter；SN rendezvous 状态为有 TTL 的内存状态，不持久化。

#### 6. 状态机、幂等与并发

- 协议状态至少覆盖 `Idle -> Requesting/Received -> Predicting(optional) -> Armed/Acting -> Responded -> Connected/Failed/Expired/Cancelled`。
- 每个 attempt 使用不可猜测的 `attempt_id`，并绑定 logical `tunnel_id`、认证 initiator/target、protocol version、request digest 和绝对 deadline。
- 同一 `(initiator, target, tunnel_id, attempt_id)` 的重复消息必须幂等；同 attempt 的字段冲突必须拒绝，不能覆盖现有计划。
- SN 只保存有界、带 TTL 的 rendezvous 临时状态；断开、超时、终态、取消和 peer re-report 必须有确定清理规则。
- 双方同时发起同一 logical tunnel 时，design 必须定义确定性 collision resolution。允许按稳定 peer ordering 淘汰 attempt，或由首个真实 Connected 获胜，但不得导致双方都等待、双方各发布一个 tunnel 或长期双向发送。
- 任一 tunnel 成功后，所有 sibling connector、punch、waiter、prediction refresh 和 PN timer 都必须由同一 owner 收敛取消。

#### 7. 成功、失败与回退

- 唯一成功条件是 QUIC/TLS 或对应 transport 的真实 tunnel 握手完成，并由 TunnelManager 完成注册/发布。成功 response、预测 endpoints 已返回、punch 已发送或 connect 已启动都不是成功。
- Failure 必须使用稳定、可观测的 reason category，至少区分 unsupported、unauthenticated、invalid endpoint、prediction unavailable、prediction timeout、stale generation、request conflict、deadline、cancelled、transport failure 和 SN relay failure。
- capability 不支持、任一 relay hop 不支持、endpoint/prediction 不可用、request 超时或新协议失败时，由上层既有策略选择旧 `SnCall` compatibility path 或 PN fallback。
- fallback 只能接管尚未 Connected 的 logical tunnel；切换前必须取消新协议 owner，避免新旧路径同时反连或发布。
- 新协议不得改变 PN wire protocol、QUIC/TLS 成功判定、身份认证或公开 Tunnel API。

#### 8. 安全与资源约束

- SN 必须从 control tunnel 认证上下文确定 sender peer id；消息内 peer id 只能作为绑定字段，不能覆盖已认证身份。
- B 只接受目标为自身且 initiator 与 relay 认证链一致的请求。A、B 和 SN 都必须校验 endpoint owner、IP 归属、transport、TTL、generation、数量和包长。
- 协议不得成为向任意第三方 IP/port 发包的反射器。预测端口只能落在 owner 已观测公网 IP 上，且窗口、总 datagram、总 connect、每 peer 并发和每 SN 时间窗速率都必须封顶。
- Request/Cancel 必须具备 anti-replay 和幂等边界；过期 attempt、已完成 generation 和不匹配 request digest 不得重新触发公网动作。
- 日志只记录 attempt correlation、动作、候选数量、generation 和 reason code；默认不输出完整认证材料、token 或不必要的公网 endpoint 列表。

#### 9. 可观测性与验证场景

- client、SN relay 和 TunnelManager 使用稳定 event/reason vocabulary 记录 request、predict、action-armed、response、connected、fallback、cancel 和 cleanup。
- 计数必须区分 endpoint accepted/rejected、prediction requested/succeeded/failed、punch attempted、connect attempted、response latency、terminal reason 和 fallback path，不能把一次循环或重复消息误算为独立 attempt。
- Testing 阶段必须包含 unit、DV 和 integration 覆盖；真实双公网 NAT 环境若不可用，必须明确标为未完成的环境证据，不能用本机 UDP 测试声称已经证明真实 symmetric NAT 穿透。

### Out of scope

- 在本任务中删除或改变旧 `SnCall` / `SnCalled` wire contract；旧协议只作为 compatibility fallback 保留。
- 在 rendezvous request/response 中传递或重新判断 NAT type、NatProfile、prediction hint 或 NAT 策略矩阵。
- 修改 PN proxy 协议、公开 Tunnel API、身份体系、证书格式或业务层 session 语义。
- 保证所有 symmetric NAT、CGNAT、防火墙或运营商网络都可穿透；endpoint prediction 仍是有界 best-effort。
- 实现 TURN、UPnP、NAT-PMP、PCP、长期在线学习、持久化 NAT profile、跨重启 rendezvous 恢复或无限 keepalive/spray。
- 允许第三方 endpoint、任意 IP/port punch、无界 endpoint 列表、无界反连或以 response/UDP punch 代替真实 tunnel 成功。
- 在 proposal 阶段确定 numeric command id、最终 binary layout、Rust API、内部 channel 类型或具体文件级实现结构。
- 在 proposal 阶段修改 design、生产代码、测试代码、配置、构建资源或运行时行为。

### Boundary with neighboring modules

- `p2p-frame/src/sn/protocol/**` 拥有独立 rendezvous wire 类型、版本和兼容解码；不执行 tunnel 动作。
- `p2p-frame/src/sn/service/**` 拥有认证、同 SN/跨 SN relay、有界 attempt state、rate limit 和 TTL cleanup；不替 peer 生成 predicted endpoints。
- `p2p-frame/src/sn/client/**` 拥有客户端协议 handler、per-SN capability、owner 本地 prediction 与 response correlation；不替远端重算 endpoint。
- `p2p-frame/src/tunnel/**` 在协议外根据 NAT 信息选择 action、request endpoints 和 prediction flag，并拥有 connector、waiter、collision resolution、fallback 和 terminal convergence。
- `p2p-frame/src/networks/quic/**` 拥有实际 UDP traversal socket、endpoint prediction 所需 socket binding、punch/connect 的同源发送及 listener-close 生命周期；不决定 SN relay 或业务策略。
- TCP 仅在 design 选择的 direct/reverse connect plan 中使用，不参与 UDP punch 或 UDP 端口预测。
- `sn-miner-rust` 仅在后续 design 证明需要时承载协议 capability/资源限制配置；不得重新定义协议语义。
- `cyfs-p2p` 和 `cyfs-p2p-test` 只消费公开 tunnel 结果或建立验证场景，不复制 rendezvous 状态机。

## Requirement Review

- 协议目标不是识别 NAT type，而是在 NAT type 已知后执行一次明确请求。策略层决定 action、request endpoints 和是否要求 B 预测；wire 层不需要知道策略原因。
- Request endpoints 是 B 反连/punch A 的目标，因此由 A 提供。Response predicted endpoints 是 A 后续连接/punch B 的目标，因此由 B 在被要求时生成。
- `need_predict_endpoint` 必须是严格合同：`false` 对应成功空列表；`true` 对应成功非空列表或 typed failure。这样空列表没有双重含义。
- 单轮 request/response 足以建立启动顺序：A 在 request 前安装必要 waiter；B 在 response 前完成预测并 arm 请求动作；A 在 response 后消费预测结果并启动本地动作。
- 四个 action mode 是不同资源和方向合同，不应由 `need_punch`、`need_reverse` 两个布尔值自由组合。互斥枚举可拒绝无效组合，并让 connector 唯一性可测试。
- `PunchAndReverseConnect` 需要 request endpoints 供 B 动作；当 A 还需要 punch B 时，设置 prediction flag 获取 B predicted endpoints。双方 symmetric 时尤其如此。
- 独立协议比扩展 `SnCall` 增加明确的 response、cancel 和 typed failure，但业务交互保持一轮，不引入第二次动作授权请求。
- 预测 socket 与实际 QUIC socket 不一致会让所有数学窗口失去基础。socket binding generation 因而是 predicted endpoint validity 的组成部分，而不是单纯实现细节。
- SN 是认证 relay 和有界协调者，不是 endpoint owner。它可以校验、转发和限速，但不能代替 A/B 生成端口窗口。

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-STRP-1 | sn_tunnel_rendezvous_wire_contract | 定义独立且可协商的 Request/Response/Complete/Cancel 协议族；operation 表达动作，request body 只含 endpoints 和 prediction flag，response body 只含 predicted endpoints | `sn/protocol` 与 authenticated SN control tunnel；NAT 策略不进入 wire，新旧 attempt 不双发 | 新增明确响应，换取预测结果和动作 armed 状态可被调用方可靠区分 | protocol unit/DV 覆盖精确 body 字段、true/false 列表不变量、版本、长度、幂等、跨 SN relay 和 mixed-version fallback | 不在协议层传递/识别 NAT type 或确定 proposal 阶段的 numeric ids |
| P-STRP-2 | sn_tunnel_rendezvous_action_modes | 用 `PunchOnly`、`PunchAndReverseConnect`、`ReverseConnectOnly`、`WaitIncoming` 覆盖 B 的全部合法动作，endpoint list 仅作为 B 动作目标 | `tunnel` strategy 与 SN request contract；punch 仅 QUIC | 枚举和 endpoint 校验更严格，换取各种 NAT 连接情况使用同一最小协议 | unit 覆盖四 action、endpoint 空/非空约束、prediction flag 正交性、transport restriction 和非法组合 | 不把 NAT 矩阵固化到 wire 或允许双方无条件 connect |
| P-STRP-3 | sn_tunnel_rendezvous_endpoint_ownership | Request endpoints 由 A 提供供 B reverse/punch；需要预测时 Response endpoints 由 B 生成供 A connect/punch，不需要时响应列表为空 | per-peer client state 与 versioned wire；每个列表默认最多 8 个 endpoints | 需要 generation/TTL 管理，换取预测归属、方向和空列表语义唯一 | unit/DV 覆盖 prediction true/false、双方 symmetric cold state、owner、TTL、generation、去重、上限和 stale/invalid rejection | 不接受第三方 IP、远端 hint 本地再展开或 endpoint owner 改写 |
| P-STRP-4 | quic_rendezvous_socket_binding | probe observation、prediction、punch 和 QUIC connect 绑定实际 traversal socket 与 generation | `networks/quic` socket ownership；listener close 立即失效 | 需要重构当前独立临时 probe 的 ownership，换取候选与真实 NAT mapping 一致 | integration instrumentation 证明同一 local socket/source port；覆盖 listener rebuild/close 和旧 generation 失效 | 不用临时 socket 结果预测另一个 listener |
| P-STRP-5 | sn_tunnel_rendezvous_lifecycle | 实现单轮 request/response 状态机、action-before-response 顺序、owner guard、deadline、cancel、collision resolution 和 terminal convergence | `sn/client`、`sn/service`、`tunnel`；SN 状态仅内存 TTL | B 必须在响应前完成预测/arm，换取无需第二次动作授权仍有明确顺序 | unit/DV/integration 覆盖 waiter-before-request、action-before-response、超时、重复、乱序、冲突、owner drop、SN disconnect 和首个成功取消 | 不持久化 attempt 或把 response 当成功 |
| P-STRP-6 | sn_tunnel_rendezvous_security | 认证身份绑定、endpoint ownership、anti-replay、rate/packet/concurrency budget 和 typed rejection | client 与 SN 双边校验；公网 UDP 动作有界 | 更严格校验可能更早 fallback，换取阻止反射和放大 | security negative tests 覆盖 forged peer、third-party IP、expired/replay、oversize、rate limit 和 digest mismatch | 不提供通用 UDP relay/reflector |
| P-STRP-7 | tunnel_manager_rendezvous_integration | 将新协议 plan 接入 logical tunnel owner，Connected 后统一 publish/cancel，并在不支持或失败时串行切换 legacy/PN | 内部 TunnelManager；公开 API、PN wire 和 TLS success 不变 | 兼容期保留两套 rendezvous 路径，换取渐进升级 | integration 覆盖新-新、任一旧节点、同/跨 SN、四模式、失败 fallback、无重复 tunnel/后台任务 | 不改变公开 Tunnel API 或 PN 协议 |

## Success Criteria

- 新协议使用独立 command/version/capability，不依赖 `SnCall` 编码；新节点可以用它完成旧 SnCall 的通知和反向建链能力，旧节点仍走单一 legacy path。
- 给定任意已知 NAT 组合，TunnelManager 在协议外选择 action、request endpoints 和 prediction flag；wire request 不含 NAT type/profile。
- Action 由 operation/message kind 表达；request body 只包含 B 动作目标 endpoint 列表和 `need_predict_endpoint`，response body 只包含 `predicted_endpoints`；其他字段只用于 envelope、认证、时限、result code 和幂等。
- `need_predict_endpoint=false` 的每个成功响应都返回空 `predicted_endpoints`；`true` 的成功响应返回至少一个 B-owned predicted endpoint，预测无结果则 typed failure。
- B 只对 request endpoints reverse/punch；A 只使用 response predicted endpoints 连接/punch B。双方都不替对端从 hint 重算端口。
- 双方均为 `SymmetricLike` 时，A 在 request 前生成 A endpoints，B 在 response 前生成 B endpoints；若任一侧不可用则明确失败并进入受控 fallback。
- `PunchOnly`、`PunchAndReverseConnect`、`ReverseConnectOnly`、`WaitIncoming` 四模式分别产生表中规定的双方动作，且每个 plan 恰有一个 connector。
- A 在 request 前安装反连所需 waiter；B 在 response 前完成可选预测并启动/arm 请求动作；A 只在匹配成功 response 后消费 B 预测结果。
- QUIC observation、endpoint prediction、punch 和 connect 使用相同实际 traversal socket/binding generation；listener 重建或关闭使旧 predicted endpoints 和发送任务立即失效。
- 重复、乱序、迟到、重放、digest/generation 不匹配、双方同时发起、SN 断开、owner drop、deadline 和首个 Connected 均有确定状态转移与 cleanup，不遗留 waiter、punch、connector、probe 或 PN timer。
- 只有真实 transport handshake 和 TunnelManager register/publish 计为 Connected；成功 response、预测返回和 punch send 不计成功。
- 同 SN、跨 SN、新-新和 mixed-version 路径均有可运行证据；unsupported/timeout/invalid/relay failure 先取消新 attempt，再由既有策略选择 legacy SnCall 或 PN，不重复反连或发布。
- 公网输入经过身份、owner、IP、transport、TTL、generation、endpoint count、packet/connect budget、rate limit 和 anti-replay 校验，不能诱导节点向任意第三方发包。
- 测试证据覆盖 unit、DV、integration；真实公网双 symmetric NAT 结果单独记录。没有真实环境时，验收报告必须明确该环境缺口。
- Explicit non-goals: 不保证所有 NAT 可穿透，不删除旧 SnCall，不改变 PN/TLS/公开 Tunnel API，不引入 TURN/UPnP/NAT-PMP/持久化或无界 spray。

## Risks

- Request/response 仍增加 SN 与客户端临时状态；若 correlation、TTL 或 idempotency 不严谨，可能发生内存增长、迟到 response 复活或重复公网动作。
- B 预测可能超过 A 的 tunnel 建链 deadline。deadline 必须贯穿 relay、prediction、action、response 和 connect，不能每阶段重新计时。
- 当前 probe 与 QUIC listener socket ownership 若分离，迁移到同一实际 traversal socket 可能触及 listener 生命周期和锁边界，属于 implementation 高风险点。
- 跨 SN relay 可能丢失认证上下文或 capability；任何 hop 不确定都必须 fail closed/fallback，不能信任消息内自报身份。
- 四种 action 或 endpoint 空/非空规则若版本不一致，可能造成错误发送或双方都 wait；request digest 和 capability 必须一致校验。
- Punch/endpoint 数量虽小，仍可能被批量 attempt 放大；per-peer、per-SN、per-attempt 和全局预算都需要 design/test 证据。
- Legacy 和新协议共存期间最容易出现同一 logical tunnel 双发。兼容选择必须在 attempt 创建前确定，fallback 切换也必须先取消旧 owner。
- 本地仿真可证明状态机与 source-port ownership，但不能证明运营商 symmetric NAT 的预测命中率；真实环境结论不得被夸大。

## Trigger Matrix

| Trigger Category | Applies? | Evidence | Required Checks | Completed Checks | Deferred Checks and Reason | Residual Risk |
|------------------|----------|----------|-----------------|------------------|----------------------------|---------------|
| contract/protocol | yes | 新增独立 request/response command family、action、request endpoints、prediction flag、response predicted endpoints 和 terminal reason | design 固化字段、版本、编码、decoder remainder、消息大小、true/false 列表不变量和 consumer closure；testing 做新旧双向负兼容 | proposal 已固定最小请求/响应业务结构和 fail-safe fallback | owner: design/testing; reason: numeric/layout 待下游；acceptance impact: 字段语义、消费者或 mixed-version 证据缺失即阻断 | 旧新节点可能双发或误解空列表 |
| data/schema | no | attempt/endpoint 仅为内存临时状态，不进入持久化 schema | design Scope Paths 审计无持久化；acceptance 检查无 desc/sec/db 写入 | proposal 明确 TTL 内存状态与跨重启不恢复 | owner: acceptance; reason: 边界审计；impact: 发现持久化即退回 proposal | 内存 TTL 清理仍属 runtime 风险 |
| security/privacy/permission | yes | 认证 relay 可触发公网 punch/connect，携带公网 endpoints | threat model、身份绑定、owner/IP 校验、anti-replay、rate/budget 和日志脱敏；负例测试 | proposal 已禁止第三方目标和无界动作 | owner: design/testing; reason: 精确阈值/用例待后续；impact: 无 abuse 证据阻断验收 | 反射、放大、重放或 endpoint 泄露 |
| runtime/integration | yes | 双端 request/response 状态机、prediction wait、action-before-response、socket lifecycle、collision、fallback 均改变 | design 描述时序/并发/取消/恢复；unit、DV、integration 覆盖全部终态 | proposal 已规定状态、owner 和成功条件 | owner: design/testing; reason: runnable implementation 尚不存在；impact: 生命周期缺口阻断验收 | 双等待、重复 tunnel、后台泄漏 |
| build/dependency/config/deployment | yes | 新协议需要 mixed-version capability rollout，资源阈值可能需要 SN 配置 | design 明确默认值、旧配置行为、滚动升级和回退；testing 覆盖 feature negotiation | proposal 保持旧 SnCall 与 PN 兼容 | owner: design/testing; reason: 配置面待 design 决定；impact: 无滚动升级证据阻断验收 | 部分节点升级造成可用性退化 |
| ui/datamodel/workflow | no | 不修改 UI、业务数据模型或终端用户工作流 | 审计后续 Scope Paths | proposal 无 UI 消费者 | owner: none; reason: not applicable; impact: none | none |
| harness/process | no | 本任务遵循现有 Harness，不修改规则、脚本、模板或 CI | 正常执行阶段门禁和 scope check | proposal 单阶段范围已声明 | owner: none; reason: not applicable; impact: checker 失败仍阻断 | none |

## Approval Record

- approver:
- approval_date:
- user_statement: ""
