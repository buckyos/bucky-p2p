---
task_manifest: task.yaml
status: approved
approved_by: user
approved_at: 2026-09-01T18:24:46+08:00
approved_content_sha256: a4653c48371e4d760ad390f84f3da337285da4ffe7ee7e76bc729fd71aed9391
---

# 建立 p2p-frame 真实 socket tunnel 专用测试面

## Workflow Tier Judgment

- Proposed tier: high-risk
- Final tier: high-risk
- Tier rationale / triggered boundaries: 本任务新增覆盖 SN、TunnelManager、QUIC/TCP、NAT 预测、PN fallback 与并发 owner 生命周期的真实网络集成测试面；其结果将成为后续 testing/acceptance 的正式证据。它涉及运行时集成、并发时序、跨 SN 控制链路与受控 NAT socket 拓扑，且失败用例的设计会直接决定协议分支是否被真实覆盖，因此按 high-risk 分阶段完成 design、testing 与独立 acceptance。
- Proposal and tier confirmation: confirmed by user statement `确认，按 high-risk 完成`
- Risk profile: ./risk-profile.yaml

## Background and Goal

当前 `p2p-frame` 已有多类局部测试，但还没有一个专用测试面把完整生产链路组合起来：

- `sn_protocol_real_network.rs` 和同 SN rendezvous 测试使用真实 SN command socket，但目标侧 listener 被测试替换，只证明 rendezvous action 已安排，没有经过真实 `TunnelManager::on_sn_rendezvous`，也没有证明 A 与 B 的 peer tunnel 已建立。
- NAT strategy、fallback、collision 与 owner 生命周期已有较完整的 unit/mock 覆盖，但 mock 结果不能证明生产 socket 连接、打洞、反连、预测与 proxy fallback 能闭环。
- TCP/QUIC network 层已有真实 A/B socket 测试，但它们绕过 SN 查询、`open_tunnel_from_id`、NAT plan 与 rendezvous 协议。
- 现有跨 SN 用例仍以进程内 `DirectInterSnClient` 代替真实 SN-to-SN transport。

目标是在 `p2p-frame` 内建立可由统一 runner 到达的真实 socket 专用测试面。每个成功分支必须从真实节点入口发起，经生产 SN/TunnelManager 路径建立 A↔B tunnel，并以双向唯一 payload 的实际收发作为最终成功信号；协议响应、日志或 action armed 只能作为分支证据，不能替代 tunnel 建立证据。

## Scope

### In scope

- 新建 `p2p-frame/tests/real_p2p_tunnel_socket.rs` 与专用 fixture 目录，启动真实 SN、两个真实 `P2pStack` 节点及按用例需要启动的 PN/TTP 或第二个 SN；所有控制面与数据面交互都通过真实 TCP/UDP/QUIC socket。
- 从生产公开入口（`open_tunnel_from_id` 或其上层 stream connect-by-id 入口）发起连接，保留 `TunnelManager` 安装的真实 SN called/rendezvous listener；禁止由测试直接调用私有 handler 或预注册伪造 tunnel 作为成功路径。
- 使用可重复、进程内但基于真实 UDP forwarding socket 的受控 NAT fixture 形成 NonSymmetricLike、SymmetricLike 与预测端点。NAT profile 必须来自真实 probe/Report/Query 观察链，不能直接注入最终 `NatConnectPlan` 或用 mock network 返回成功。
- 覆盖同一 serving SN 下的连接条件矩阵：callee public、caller public、NonSymmetricLike/NonSymmetricLike、NonSymmetricLike/SymmetricLike、SymmetricLike/NonSymmetricLike、SymmetricLike/SymmetricLike，并校验分别触发 `WaitIncoming`、`ReverseConnectOnly`、`PunchOnly`、`PunchAndReverseConnect` 及需要的预测端点。
- 覆盖 NAT profile 缺失、Unknown、过期时只进入 legacy `SnCall`；覆盖新 rendezvous 请求/目标 action 失败后回落 legacy `SnCall`，以及 legacy direct action 失败后通过真实 PN proxy tunnel 完成双向数据。
- 覆盖双方同时 `open_tunnel_from_id` 的 collision/owner 竞争，证明只有一个稳定 tunnel 成为 winner，loser/cancelled/stale completion 不会清除或替换新 owner，且最终数据通路可用。
- 以代表性用例覆盖 SN command transport 的 TCP/QUIC parity；NAT 打洞矩阵以 QUIC peer tunnel 为主，并至少保留一个真实 TCP peer tunnel 的 public/direct 或 reverse-connect 闭环。
- 增加一个双 serving-SN 代表性用例，使用生产 `TtpInterSnClient` 的真实 control stream 完成查询/relay/rendezvous，并最终证明 A↔B peer tunnel 与双向数据，不再以 `DirectInterSnClient` 作为跨 SN 成功证据。
- 所有 fixture 使用 OS 分配端口、显式 readiness、绝对 deadline 与 RAII teardown，能够串行和有限并行重复运行，不依赖固定 sleep 或固定 localhost 端口。

### Out of scope

- 不修改 `cyfs-p2p-test/**`，不运行或引用 `cyfs-p2p-test` binary、场景、日志或历史结果作为 testing/acceptance 证据。
- 不改变 `SnTunnelRendezvous`、legacy `SnCall`、TunnelManager、NAT plan、PN/TTP 或 TCP/QUIC 的生产语义；若设计阶段发现必须修改生产契约或主动推进状态的 test hook，本 proposal 必须先修订并重新确认。
- 不把 loopback/进程内 NAT forwarding fixture 声称为公开互联网、真实家庭路由器、跨主机、防火墙或运营商 NAT 验证；这些仍是目标环境补充证据。
- 不要求对完整 NAT 矩阵做 TCP 打洞；当前 NAT probing/prediction 语义绑定 QUIC/UDP，TCP 只验证适用的公开可达或反连路径。
- 不以单独的 SN protocol ack、listener 回调、日志文本、tunnel 对象创建或 mock 调用次数判定 tunnel 已建立。

### Boundary with neighboring modules

- 正式测试实现、fixture、runner wiring 与证据均属于 `p2p-frame`；`cyfs-p2p` 和 `cyfs-p2p-test` 不承担本任务验证职责。
- 受控 NAT fixture 只模拟 socket 地址映射与转发，不复制 TunnelManager、SN 或 QUIC 握手决策；连接决策仍由生产代码完成。
- 双 SN 用例使用 `p2p-frame` 已有 inter-SN TTP transport；不为测试引入第二套直连 trait 实现。
- 如果现有只读公开观察不足以区分 rendezvous、legacy 与 proxy 分支，design 必须优先通过 wire-visible 事件和 tunnel form/direction 断言解决；任何生产源代码中的新观察面都需先返回 proposal 明确边界。

## Requirement Review

建立这个专用测试面是合理的：目前各局部测试分别证明了 wire、plan 或 transport，但没有证明这些部件在真实连接入口下闭环。直接把全部组合做成公网/多机 E2E 会使 CI 不稳定且难以精确制造 SymmetricLike 映射；只注入 `NatProfile` 或 mock connect 又无法满足真实 socket 目标。因此采用两层证据：CI 内以真实 loopback socket 和可控 UDP forwarding fixture 构造确定性条件，完整验证生产调用链与数据闭环；公网/多机 NAT 作为明确不由本任务宣称的补充验证。

为控制运行时间，六种策略条件不做 TCP×QUIC×同 SN×跨 SN 的笛卡尔积。完整 NAT 策略矩阵在最相关的 QUIC peer transport 上执行；TCP/QUIC command parity、TCP peer tunnel、PN 与跨 SN 各选择能证明边界的代表性用例。失败注入只能阻断真实 socket 服务或返回真实协议错误，不能用 mock tunnel 直接指定结果。

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-033-1 | real_socket_tunnel_strategy_matrix | 从真实 connect-by-id 入口贯通 SN、TunnelManager 与 A/B socket，覆盖 public 与四种 NAT mapping 组合以及四种 rendezvous operation | 完整 NAT 矩阵以 QUIC peer tunnel 为主；TCP 做代表性真实闭环；profile 来自真实 probe 链 | socket NAT fixture 比直接注入 profile 更复杂，但能同时验证映射观察、预测与实际传输 | 每个 case 断言选定分支/operation、tunnel form/direction/identity（可观察部分）并完成 A→B、B→A 唯一 payload；统一 runner 可重复通过 | 不用 ack、listener armed 或 mock connect 代替数据闭环；不声称公网 NAT |
| P-033-2 | real_socket_legacy_and_proxy_fallbacks | 缺失/Unknown/过期 profile 只能走 legacy `SnCall`；rendezvous 失败回落 legacy；direct action 再失败时通过真实 PN proxy 成功 | 失败必须由真实 socket 不可达、连接关闭或真实协议拒绝产生；不直接注入 tunnel 成败 | 真实 deadline 会增加测试时长，因此要求可控故障与严格绝对上限 | wire-visible 证据区分 rendezvous、legacy call、proxy；最终 legacy direct 或 PN tunnel 完成双向 payload，且禁止分支没有出现 | 不把所有 rendezvous 失败等同于 tunnel 失败；不允许无限重试或固定 sleep |
| P-033-3 | real_socket_collision_and_cross_sn_paths | 覆盖同时发起的 owner collision/stale completion，并以生产 inter-SN TTP control stream 覆盖一个双 serving-SN rendezvous 闭环 | 只做代表性跨 SN 条件，不扩展 inter-SN 协议；并发断言绑定唯一 owner token/generation | 时序用例容易偶发，必须用 barrier、事件与 bounded deadline 提供确定前置关系 | collision 后仅一个稳定 tunnel winner、数据双向可用且无 owner 泄漏；跨 SN case 证明真实 SN-to-SN stream、目标 delivery 与 A/B 数据闭环 | 不使用 `DirectInterSnClient` 作为跨 SN 证据；不做多机集群/故障转移矩阵 |

## Success Criteria

- Concrete system-visible result: `p2p-frame` 拥有独立真实 socket tunnel 测试入口；在同一进程中启动真实节点后，策略矩阵、legacy/PN fallback、collision 与代表性跨 SN 用例均从 connect-by-id 入口得到可用 tunnel，并完成双向数据。
- Required evidence: `testing-coverage-check.py` 覆盖三个 `change_id`；`python3 ./harness/scripts/test-run.py p2p-frame/033-real-p2p-tunnel-socket-tests all` 通过；关键用例至少重复运行并包含有限并发运行；test artifacts 记录每个条件的实际 operation/path、transport、tunnel form 与双向 payload 结果；独立 acceptance 核对没有 mock/direct-handler/`cyfs-p2p-test` 替代真实链路。
- Explicit non-goals: 不声明全工作区测试通过，不声明公网、多主机、真实路由器 NAT、生产 PN/SN 部署或长时间 soak 已验证；不修改生产协议行为。

## Risks

- loopback NAT fixture 若只改 profile 不实际转发对应映射，会制造“分支正确但 socket 不真实”的假阳性；必须由实际 forwarding sockets 同时决定观察结果和数据可达性。
- rendezvous response 仅表示目标 action 已安排，不表示 peer tunnel 已建立；所有成功 case 必须以后续数据 round-trip 收口。
- 同时发起时仅按 `(remote, seq, tunnel_id)` 判断 owner 不足以防止 stale task 影响新 owner；测试必须观察唯一 token/generation 绑定后的最终 tunnel 与清理结果。
- 全矩阵笛卡尔积会显著放大 CI 时间和端口竞争；采用策略矩阵加边界代表例，并要求动态端口、事件 readiness、RAII 与绝对 deadline。
- 当前工作区已有大量未提交改动；后续各阶段必须通过 task-scoped baseline 和 changed-path manifest 隔离本任务，不能把既有修改当作实现或证据。

## Approval Record

- approver: user
- approval_date: 2026-09-01T18:24:46+08:00
- user_statement: "确认，按 high-risk 完成"
