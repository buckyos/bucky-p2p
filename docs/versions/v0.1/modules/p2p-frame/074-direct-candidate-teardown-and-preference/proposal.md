---
task_manifest: task.yaml
status: approved
---

# 直连候选败者显式关闭与隧道复用优选 Proposal

## Workflow Tier Judgment

- Proposed tier: standard
- Final tier: standard
- rationale: 改动集中在 `p2p-frame` 单模块的直连候选编排与候选选择规则：`TunnelManager::open_direct_path` 返回胜者后对未选中候选的收尾方式，`select_preferred_tunnel_entry` 的优先级来源，以及 `TunnelActivity`/`Tunnel` 上的一个只读 additive 访问器（带默认实现）。与 042/043/062/064 同型：内部运行时连接编排与生命周期修复，无 wire/协议/命令码/配置/数据/依赖/发布/安全面变更，targeted verification 可由 `p2p-frame` 自身测试面提供。不满足 `trivial`（涉及并发与生命周期、跨调用方 hand-out 语义），也没有 `high-risk` 的具体触发证据（不改协议与公开必选 API，无部署/回滚协调、无跨项目边界）。
- triggered_boundaries: 若实现中确认必须新增/修改 wire 或命令码（例如新增“候选败者”通知命令、Hello 确认握手）或必须给 `Tunnel` 增加**无默认实现**的必选方法才能满足验收，则属于需求/风险升级，必须回到 proposal 复核后再继续；本提案默认只做进程内行为修复加一个带默认实现的只读访问器。
- confirmation_statement: 用户在 2026-09-13 回复“确认”，确认按本提案原文执行，并接受默认分类 standard（未给出显式 tier 覆盖、未提出 unresolved question 的修改）。

Risk profile: not-created (replace with ./risk-profile.yaml only after high-risk confirmation)

## Background and Goal

现场证据（人类诊断运行，非验证证据）：本机 all-in-one 运行在 `stream_direct_success` 通过后，`stream_direct_return_success` 以 `IoError at p2p-frame/src/networks/command.rs:127` / `ConnectionLost(ApplicationClosed(ApplicationClose { error_code: 0, reason: b"" }))` 失败。

同一次运行的时间线：

- client（`1p0hu2572…`）的 `open_direct_path` 对本机 8 个 endpoint 并发建链，其中 5 个 IPv4 候选被 direct_target（`1p0hu2574…`）接受、注册并发布为 `form=Passive` 可用隧道。
- `127.0.0.1` 候选（`2584936889`）先成功：日志 `direct path success` → `direct path selected`（`p2p-frame/src/tunnel/tunnel_manager.rs:2306`、`:2346`）。该函数返回时把剩下的 `FuturesUnordered` 一并 drop，仍在 `QuicTunnel::connect` 中、尚未返回 tunnel 的 4 个候选的 `quinn::Connection` 随之被 drop。
- quinn 0.11 的 `Drop for ConnectionRef` 对未关闭连接调用 `implicit_close`，固定发送 `close(0u32, b"")`（`quinn-0.11.9/src/connection.rs:1240-1243`）；因此对端看到的是**空 reason 的应用层关闭**，本仓库 `close_with_error` 产生的 message 不会为空（`p2p-frame/src/networks/quic/tunnel.rs:469-475`）。
- 同一时刻 direct_target 的 `get_tunnel` 只把 890/891/893 判为 Closed，`2584936892`（`10.111.170.1`）仍为 available，而 `select_preferred_tunnel_entry` 取 `updated_at` 最新者（`p2p-frame/src/tunnel/tunnel_manager.rs:700-722`），于是选中了刚被放弃的候选；`open_stream` 等待 `OpenChannelResp` 时读到连接已死，返回 `IoError`。

目标：

1. 竞速失败（未被选中）的直连候选必须被**显式关闭**，不得依赖 future 被 drop 触发 quinn 隐式关闭，也不得在本地注册/发布为可用隧道。
2. 隧道复用选择不得再无条件“最新注册优先”：已确认承载过业务流量的候选优先，未确认使用的候选只作为回退。

## Scope

### In scope

- `TunnelManager::open_direct_path` 的败者收尾：胜者确定后，其余并发尝试不得因为 future 被 drop 而静默消失。败者必须在其建链尝试结束（成功或失败，受既有 connect 预算约束）后被显式 `close()`，且不得进入本进程的注册/发布集合、不得更新 `conn_info_cache`。胜者行为、返回时机与 endpoint 计分语义保持不变。
- 候选选择：`select_preferred_tunnel_entry` 在非 Proxy 条目之间优先选择**已确认在用**的隧道（有业务流量的最近使用时间较新者优先）；从未承载过业务流量的候选保持现有顺序（`updated_at` 最新优先）作为回退。Proxy 回退分支同样遵守“已确认在用优先”。
- 判定“已确认在用”的只读信号：`TunnelActivity` 记录“首次/最近一次业务活动时间”，仅由 stream/datagram/control channel 的打开或接受（`begin_pending`/`acquire_work_instances`/`note_activity`）更新；隧道建立时间、heartbeat、listener 注册不算业务活动。`Tunnel` trait 增加带默认实现（默认 `None`）的只读访问器，QUIC/TCP 隧道实现它；PN 与其他实现保持默认值。
- 回归测试（`p2p-frame` 自有测试面）：胜者的败者显式关闭且不进入可用集合；有使用记录的候选优先于更新但未使用的候选；未确认使用的候选仍按原有顺序回退；既有选择与 idle 清理用例保持通过。

### Out of scope

- 不改 wire/命令码/握手：不新增“候选败者/候选确认”协议消息，不把候选身份传到对端。
- 不做“复用前 RTT 存活探测”：不新增探测 API，不在 `open_stream`/`select` 前额外往返；该候选修复方向记录为 residual risk（见 Risks）。
- 不改 idle cleanup、proxy upgrade、rendezvous/NAT-aware 计划、`preferred_direct_endpoints` 计分、Direct 缓存失效阈值（043 语义）。
- 不改 `cyfs-p2p-test` 场景代码，也不把 `cyfs-p2p-test` 运行输出作为本任务的验证或验收证据（`harness/custom-rules/cyfs-p2p-test-stage-ban-rules.md`）；上文的运行日志仅作人类诊断背景。
- 不整理工作区中其他既有未提交改动（073 及其测试面等）。

### Boundary with neighboring modules

- `p2p-frame/src/networks/tunnel.rs`：只在 `TunnelActivity` 增加“业务活动时间”记录与只读取用，`try_retire_idle`/`retire`/lease 语义不变；`Tunnel` trait 只增加带默认实现的只读方法，既有实现与测试替身无需改动即可编译。
- `p2p-frame/src/networks/quic/tunnel.rs`、`p2p-frame/src/networks/tcp/tunnel.rs`：只把 `TunnelActivity` 的新信号透出，既有 accept/open/close 流程与日志不变。
- `p2p-frame/src/tunnel/tunnel_manager.rs`：改动限于 `open_direct_path` 的败者收尾与 `select_preferred_tunnel_entry` 的排序来源；`register_tunnel`/`publish_registered_tunnel`、`cleanup_closed_tunnels`、reverse/proxy 路径与 NAT-aware/rendezvous 编排语义不变。

## Requirement Review

需求合理：当前失败不是 NAT/SN 或身份问题，而是“竞速中未被选中的候选被静默丢弃”与“对端按最新注册优先复用”共同造成的时序缺陷。两处都属实现缺陷所在的责任面，且修复范围可控。

选择的权衡：

- 败者收尾选择“保留尝试直到其自然结束并显式 close”，而不是立即取消：立即取消时 `quinn::Connection` 只存在于 `QuicTunnel::connect` 内部，进程内拿不到可关闭对象，仍会退化为隐式关闭；保留到结束可以给出显式关闭原因并让对端确定性地移除该候选。代价是败者连接会存活到自身 connect 预算耗尽（通常远小于超时），期间对端仍可能短暂持有该候选。
- 选择规则选择“已确认在用优先”，而不是把“最新优先”整体翻转成“最早优先”：翻转会改变 reverse/direct 混合、重连后新旧隧道共存等场景的选择，影响面不可控；而“已确认在用优先 + 未使用回退原序”只影响“多个候选并存且其中一个确实承载过业务”的场景，正是本次缺陷场景。
- 备选方案与被排除理由：新增协议级“候选确认/败者通知”能从根上消除对端的 stale 候选集合，但属于 wire/协议变更与更高 tier，本提案不做；复用前 RTT 存活探测能覆盖“无任何候选承载过业务”的冷启动竞速，但引入额外延迟和新探测 API，本提案不做，作为 residual risk 记录。

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-DCAND-1 | direct_candidate_loser_explicit_teardown | `open_direct_path` 选中胜者后，其余并发候选必须在自身尝试结束后被显式 `close()`（而非因 future 被 drop 触发 quinn 隐式关闭），且不得注册/发布到本地隧道集合、不得写入 `conn_info_cache` | 只改 `open_direct_path` 的候选编排与收尾；胜者选择、返回时机、`on_direct_connect_result` 计分与 `conn_timeout` 预算不变 | 败者连接存活到自身 connect 尝试结束，换取对端可观察的显式关闭原因与确定性的移除 | 单元测试断言：胜者返回后所有其他候选最终被显式关闭、均未出现在已发布集合、无候选被中途丢弃；失败场景不再产生 `ApplicationClose { error_code: 0, reason: b"" }` | 不新增 wire/命令码，不改握手，不做协议级候选确认 |
| P-DCAND-2 | prefer_confirmed_tunnel_entry | `select_preferred_tunnel_entry` 不得无条件“最新注册优先”：已确认承载过业务流量的候选优先（最近使用时间较新者优先），从未承载业务流量的候选保持现有回退顺序；Proxy 回退分支同样处理 | 只改候选排序输入；不改 `register_tunnel`/publish、`cleanup_closed_tunnels`、reverse/proxy 建立路径与 connect 流程 | 未承载过业务的冷启动竞速仍按原顺序回退，残余选出“刚被放弃候选”的窗口由 P-DCAND-1 收窄 | 单元测试断言：最新注册但未使用的候选不再优先于已承载过业务流量的候选；两者都未使用时的既有选择行为保持不变 | 不做复用前存活探测，不新增必选 trait 方法，不引入时间衰减 |
| P-DCAND-3 | direct_candidate_selection_regression_tests | 在 `p2p-frame` 自有测试面补齐覆盖：败者显式关闭且不进入可用集合；已使用候选优先于更新但未使用的候选；未使用候选回退顺序不变；既有选择/idle 清理用例保持通过 | 只新增/调整 `p2p-frame` 内测试与最小测试替身能力；不引入固定 sleep 依赖，不使用 `cyfs-p2p-test` | 用测试替身记录 `close()` 调用与活动时间戳断言行为，避免依赖真实时序 | 相关 lib 测试通过，并给出可直接复跑的命令与结果 | 不新增独立 testing/design/acceptance 制品，不把 loopback 结果写成公网 NAT 或部署证据 |

## Success Criteria

- Concrete system-visible result: 直连竞速中未被选中的候选被显式关闭，对端在收到关闭后即可从可用集合中移除；复用隧道时优先选择已经确认承载过业务流量的候选，不再因为“注册时间最新”而选中刚被放弃的候选；两处修复合起来消除“返回流打开即 `ConnectionLost`”的失败路径。
- Required evidence: 实现后的调用链 diff 与 `p2p-frame` 内回归测试证据（败者关闭与集合归属、已用候选优先、未用候选回退不变），以及既有 tunnel 选择/清理相关用例保持通过；明确 loopback/mock 证据不等于公网 NAT 或多机部署证据。
- Explicit non-goals: 不改 wire/协议/命令码，不新增必选公开 API，不做复用前存活探测，不改 idle cleanup/proxy upgrade/rendezvous 语义，不用 `cyfs-p2p-test` 作为验证或验收证据。

## Risks

- 败者在自身 connect 尝试结束前仍会短暂存活，对端在这段窗口内仍可能注册该候选；P-DCAND-1 保证的是“显式关闭 + 不进本地可用集合”，窗口收窄而非归零。窗口归零需要协议级候选确认（本提案 non-goal）。
- “已确认在用优先”会改变“旧但已用候选”与“新但未用候选”并存时的选择结果：优先保持已确认可用的路径被视为更稳；代价是当旧路径已劣化但尚未关闭时不会自动切换到新候选（新候选仍可通过成功业务或旧候选关闭后接管）。
- 若某候选只在失败路径上产生活动记录（`track_*_result` 的 Err 分支调用 `note_activity`），它也会被视为“已使用”；该候选本身仍必须是 available 才会参与选择，因此影响限于“有活动但从未成功承载业务”的候选，需在实现中确认记录粒度不弱化 P-DCAND-2 的目标。
- 冷启动竞速（所有候选都未承载业务）仍按 `updated_at` 最新优先回退，属于本次未消除的残余风险；如需消除需评估复用前存活探测或协议级候选确认，另行提案。
- 工作区存在大量既有未提交改动（073 等）；本任务的 lower-tier baseline 与 changed-path 证据绑定本任务 manifest，避免把无关改动归入 074。

## Approval Record

- approver: user
- approval_date: 2026-09-13
