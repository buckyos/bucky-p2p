---
task_manifest: task.yaml
status: approved
---

# 使用真实 socket 验证 p2p tunnel 建立流程

Risk profile: ./risk-profile.yaml

## Workflow Tier Judgment

- Proposed tier: high-risk
- Final tier: high-risk
- Tier rationale / triggered boundaries: 本任务生成覆盖 SN、TunnelManager、PN/TTP、跨 SN relay、并发和超时清理的正式 runtime 测试证据。虽然不再要求真实 NAT punch 成功，也不改变生产协议，但测试会验证并发生命周期、跨模块真实 socket 控制链路和回退选择，因此继续采用 high-risk staged workflow。
- Proposal and tier confirmation: confirmed by user statement `确认，自动完成`

## Background and Goal

任务 033 原计划在 loopback fixture 中同时验证 NAT profile、rendezvous operation、真实 QUIC punch 和最终 peer tunnel。Implementation 审计确认生产 wire 与 QUIC punch 会拒绝 loopback/private `ServerReflexive` 地址，而且过期 profile 不会通过生产 Query 作为 stale profile 返回，因此原成功标准在不增加 production test hook 的前提下不可执行。

用户现已明确：不需要验证真实打洞，主要验证 tunnel 建立流程是否正确。本 sibling task 收窄正式证据：控制面仍使用真实 TCP/UDP/QUIC socket 和生产入口，验证策略选择、命令传递、回退、并发及清理流程；只有在当前 loopback 环境真实可达的代表路径上要求最终 tunnel 和双向 payload。NAT operation 用例不再把实际 punch packet 或 NAT 穿透成功作为通过条件。

## Scope

### In scope

- 建立 `p2p-frame` 专用测试入口和 fixture，启动真实 SN、真实 `P2pStack` 节点，并按代表路径启动真实 PN/TTP 或第二个 serving SN；控制面交互使用真实 socket，不以 mock SN、mock tunnel、直接私有 handler 调用或替换 TunnelManager listener 作为成功证据。
- 从 `StreamManager::connect_from_id` / `TunnelManager` 生产入口触发查询和 tunnel 建立流程，使用结构化 correlation 事件、公开 connection-info cache、真实协议响应和 bounded outcome 验证流程分支。
- 覆盖 callee public、caller public、NonSymmetricLike/NonSymmetricLike、NonSymmetricLike/SymmetricLike、SymmetricLike/NonSymmetricLike、SymmetricLike/SymmetricLike 的策略选择，确认对应 `WaitIncoming`、`ReverseConnectOnly`、`PunchOnly`、`PunchAndReverseConnect` operation 以及 prediction 是否按生产规则生成或被有界拒绝。
- NAT mapping 类型仍由真实 UDP probe/reflector/forwarding socket 观察产生；对需要 punch 的 operation，只要求证明 production plan、rendezvous request/action 或本地 validation/fallback 流程正确，不要求 punch datagram 实际发出、目标 peer tunnel 建立或 NAT 穿透成功。
- 覆盖 profile 缺失、Unknown 和生产过期语义：过期后 SN Query 不返回 profile，因而与 missing 一样进入 legacy；不要求把 stale `NatProfile` 对象注入 TunnelManager。
- 覆盖 rendezvous 请求或目标 action 的有界失败回落到真实 legacy `SnCall`，以及 direct action 失败后通过真实 PN proxy tunnel 完成双向 payload。
- 覆盖双方同时 connect 的流程去重/竞争与有界清理；用真实可达代表路径证明最终稳定连接可重复承载 payload，不要求外部测试直接读取 owner token 或内部 owner table。
- 以代表性用例覆盖 SN command TCP/QUIC parity；覆盖生产 `TtpInterSnClient` 真实 control stream 的双 serving-SN query/relay/rendezvous 流程。若 NAT peer data path 不可达，跨 SN 用例以目标 action acknowledgement、correlation 和有界完成作为控制流程证据，不宣称 peer tunnel 建立。
- public/direct、legacy direct 和 PN proxy 等 loopback 可达代表路径必须建立真实 peer/proxy tunnel，并完成 A→B、B→A 唯一 payload。
- fixture 使用动态端口、显式 readiness、绝对 deadline 和 RAII teardown；正式证据只通过统一 task runner 产生。

### Out of scope

- 不要求或宣称 loopback 环境完成真实 NAT 打洞、预测端口可达性、家庭路由器/运营商 NAT 穿透、公开互联网或跨主机 peer tunnel。
- 不要求每个 NAT operation 最终得到 Connected tunnel；operation/action acknowledgement、结构化事件和有界 fallback 是流程证据，不能被表述为 tunnel 建立成功。
- 不增加 production endpoint-classification hook、stale-profile injection、mock transport、测试专用协议字段或公开 API。
- 不替换 `SNClientService` 中由 `TunnelManager` 安装的 called/rendezvous listener，不直接调用私有 handler，不预注册伪 tunnel。
- 不修改或使用 `cyfs-p2p-test/**`，也不引用其 binary、日志、场景或历史结果作为 testing/acceptance 证据。
- 不要求 TCP NAT punch 矩阵；TCP 只做真实可达 direct/reverse 或 command transport 代表闭环。

### Boundary with neighboring modules

- 测试实现、fixture、runner wiring 和正式 evidence 全部属于 `p2p-frame`；`cyfs-p2p` 与 `cyfs-p2p-test` 不承担本任务验证职责。
- 真实 socket 说明传输使用 OS socket 和生产协议/服务，并不意味着每个故障或 punch operation 必须建立 peer tunnel；每项证据必须明确区分 selected、sent、action-armed、fallback、Connected 和 payload-complete。
- 任务 033 保留为原始、更强要求及其阻塞审计；本任务不修改其已批准 packet，而以新的验收边界取代其未交付测试目标。

## Requirement Review

该收窄与用户实际目标一致。Tunnel 建立流程包含策略选择、SN command、目标 action、direct/legacy/proxy 回退和最终数据面等不同完成边界；把所有 NAT 条件都要求成 loopback 真打洞会迫使测试改变生产地址安全策略，反而不能忠实验证当前流程。新的证据模型保留真实 socket 和 production listener，同时把 NAT operation 验证停在其真实可观察边界；只有环境确实支持的数据路径才以双向 payload 收口。

结构化日志或 action acknowledgement 只能证明流程到达对应边界。任何用例只有在明确要求 tunnel success 时，才以 connection info、stream identity 和双向 payload 判定成功。测试报告必须逐项标注 `selected`、`request-sent`、`action-armed`、`fallback`、`connected`、`payload-complete`，避免把不同阶段混为一谈。

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-034-1 | real_socket_tunnel_flow_strategy_selection | 使用真实节点、SN socket 与真实 probe profile 覆盖 public 和四种 NAT mapping 组合的 production strategy/operation 选择 | 需要 punch 的 operation 只验证选择、请求/目标 action 可达边界或明确的本地拒绝与 fallback，不要求 punch packet 或 peer tunnel | 放弃 NAT 穿透真实性，换取无需生产 hook、可重复且准确区分阶段的流程证据 | 每个条件记录真实 profile、预期/实际 operation、prediction 要求、到达的流程边界和有界结果；public/direct 代表例完成双向 payload | 不宣称 NAT punch 或公网穿透成功 |
| P-034-2 | real_socket_tunnel_flow_fallbacks | 验证 missing/Unknown/expired-to-missing 进入 legacy、rendezvous/action 失败回落 legacy，以及 direct 失败回落真实 PN | 过期遵循生产 Query 语义，不注入 stale profile；只有可达 legacy/PN 路径要求 tunnel payload | 不再单独证明 TunnelManager 收到 stale 对象，但准确验证生产系统真实可出现的过期结果 | wire/correlation 证据区分 rendezvous、legacy、proxy；legacy direct 与 PN proxy 代表例完成双向 payload，所有失败有绝对 deadline | 不以日志或 ack 代替要求成功的数据路径 |
| P-034-3 | real_socket_tunnel_flow_collision_cross_sn | 验证 simultaneous connect 流程去重、稳定可用连接、清理，以及生产 TTP inter-SN query/relay/rendezvous 控制链 | 不直接读取内部 owner token/table；不可达 NAT 跨 SN 只验证真实 control stream、目标 delivery/action 和有界结果 | 黑盒证据不能声明内部实现细节，但能证明对调用者可见的流程与稳定性 | simultaneous public/direct 代表例在竞争后重复双向 payload；跨 SN 记录真实 TTP control correlation、目标 action 和 bounded completion | 不宣称 NAT 跨 SN peer data tunnel 或内部 owner table 状态 |

## Success Criteria

- Concrete system-visible result: `p2p-frame` 拥有独立、统一 runner 可达的真实 socket tunnel-flow 测试面，能清晰报告每个条件到达 selected/request-sent/action-armed/fallback/connected/payload-complete 中的哪个边界。
- Required evidence: `testing-coverage-check.py` 覆盖三个 change_id；`python3 ./harness/scripts/test-run.py p2p-frame/034-verify-p2p-tunnel-flow-real-sockets all` 通过；策略条件、fallback、collision 和跨 SN 用例均有有界、可重复的真实 socket 证据；仅 public/direct、legacy direct、PN proxy 等明确要求成功的代表路径必须完成双向 payload。
- Explicit non-goals: 不声明真实 NAT punch、公网/多机/真实路由器 NAT、所有 operation 的 peer tunnel、生产部署或全工作区测试已验证；不修改生产协议或地址分类语义。

## Risks

- 最大风险是再次把 operation selected、request sent、action armed 和 tunnel connected 混为一谈；测试命名、断言和 evidence 必须显式标注完成边界。
- 结构化日志属于流程 correlation 证据，不是数据面成功证据；要求成功的代表路径仍必须完成双向 payload。
- 全局日志捕获和真实 socket 后台任务可能造成并发干扰；专用入口必须串行运行相关 case，并按 case correlation 过滤事件。
- 动态端口预留仍存在 bind race；fixture 必须有有限重试和 readiness，不依赖固定 sleep。
- 任务 033 与本 sibling 的边界不同；Acceptance 必须只按本 proposal 判断，不得复用 033 的“所有 NAT 条件最终 tunnel 成功”标准。

## Approval Record

- approver: user
- approval_date: 2026-09-01
- user_statement: "确认，自动完成"
