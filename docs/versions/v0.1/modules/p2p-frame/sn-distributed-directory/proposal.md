---
module: p2p-frame
submodule: sn-distributed-directory
version: v0.1
status: approved
approved_by: user
approved_at: 2026-06-22T21:02:36+08:00
approved_content_sha256: 17968fdefb7882fc64c292fe025c3131c3851157bcb48f9e2e56ffe259f025d0
---

# SN 分布式目录 Proposal

## Background and Goal
当前 `p2p-frame` 的 SN 服务以单 SN 的 `PeerManager` 在线缓存为中心：客户端通过 `ReportSn` 上报 detail，通过 `SnQuery` 查询 detail，通过 `SnCall` 请求 SN 投递 `SnCalled`。该模型在单 SN 部署下成立，但多 SN 部署时，用户连接到不同 SN 会导致查询和 NAT call 只能命中本机 `PeerManager`。

本 proposal 定义 SN 从单节点服务扩展为同一 Owner SN 集群内的分布式目录与跨 SN 信令能力。目标是让用户可以连接相同 Owner SN 集群内的任意 Serving SN，同时保持现有客户端可见 `ReportSn`、`SnQuery`、`SnCall`、`SnCalled` 语义不变。

Owner SN 和 Serving SN 是独立运行角色：Owner SN 负责 owner control plane 与分区路由目录，Serving SN 负责客户端连接、detail 生成与 call 投递。两者不要求、也不应假设在同一运行实例内混合执行；跨角色协作只通过显式内部接口和 SN 间协议完成。`SnService` 只能承载 Serving SN peer-facing 功能；Owner SN 能力属于 directory 域，必须在 `sn/directory` 模块内通过 client/server 边界实现。

Owner SN 集群只同步小规模控制面状态，包括 ownerSN membership/config、ownerSN 集群可用性和 Serving SN online/offline 状态。Serving SN 不需要区分 session 或 epoch；Owner SN 查询 route 时只需要确认对应 Serving SN 当前仍在线。用户级 `PeerRoute` 不在 ownerSN 全集群同步；它由固定分区算法根据 ownerSN 列表计算分区 owner set，并只在对应分区副本内保存和查询。

Owner SN control plane 不要求实现完整 etcd/Raft，但必须具备跨 ownerSN 网络投票、leader 选举、leader 失效后重新选举，以及由 leader 向其它 ownerSN 同步 Serving SN online/offline 状态的能力。leader 正常工作期间，其它 ownerSN 作为 follower 通过 owner-to-owner 通道与 leader 通信，Serving SN 在线状态变更必须提交到 leader，再由 leader 复制给集群内其它 ownerSN。

生产和测试路径都不得依赖全局 registry 或同进程 shortcut 作为 Owner SN 与 Serving SN 的目录访问方式。Serving SN 到 OwnerDirectoryServer 的 publish/query 必须走 serving-facing listener 或可替换的显式进程间 transport seam；Owner SN 之间的 vote/heartbeat/replication/forward 必须走 owner peer/control transport seam。单元测试可以使用显式 fake transport，但不能通过全局 registry 模拟 production fallback。

## Scope
### In scope
- 在 `p2p-frame/src/sn/**` 中引入分布式 SN directory 能力，记录 peer 所属 Serving SN 的路由关系，但不把该路由关系全量同步到所有 ownerSN。
- Owner SN 集群维护 ownerSN membership/config、ownerSN 集群有效性和 Serving SN online/offline 状态，并保证这些小状态可被各 ownerSN 用于本地查询判断。
- Owner SN 集群必须通过 owner-to-owner 网络通道完成投票选主；leader 失效或失联后，剩余满足 quorum 的 ownerSN 必须能重新选出 leader。
- leader 正常工作期间，follower ownerSN 不直接提交 Serving SN online/offline 变更；它们必须将变更转发给 leader 或拒绝并返回 leader 信息。
- Serving SN online、renew-online、主动 offline 和异常离线事件必须由 leader 提交并复制给其它 ownerSN，使各 ownerSN 的 Serving SN 在线判断最终收敛到 leader committed 状态。
- Serving SN 在线状态是独立上报逻辑，不由每个客户端 `ReportSn` 或每条 `PeerRoute` publish 隐式刷新；Serving SN 使用低频 online renew 或主动 offline 表示自身可用性，异常断线后由 owner control plane 超时判定 offline。
- `PeerRoute` 由固定分区逻辑管理：Serving SN 拿到 ownerSN 列表后本地计算每个 peer 的 route owner set，并按分区批量写入和查询；同一用户 route 可以使用较长的上报/刷新周期，route publish 不需要在每次 `ReportSn` 时发生。
- `PeerRoute` 查询必须结合 Serving SN online/offline 状态判断可用性；Serving SN 已 offline 时不得返回对应 peer route。
- Owner SN 与 Serving SN 逻辑必须作为独立角色边界建模；它们只通过 directory/inter-SN 接口交互，不通过共享 `PeerManager`、共享 handler 分支或同进程假设耦合。
- `SnService` 只包含 Serving SN 功能，不拥有 owner membership、owner control plane、owner route store 或 owner command handler；Owner SN 功能由 `sn/directory` 内部 server 侧提供，Serving SN 只依赖 `sn/directory` 内部 client 接口。
- `SnQuery` 由 serving SN 内部拉取多个 serving SN 的 detail 并合并去重后返回现有 `SnQueryResp`。
- `SnCall` 本地 miss 后可跨 SN relay 到 remote serving SN，并由 remote serving SN 投递 `SnCalled`。
- OwnerDirectoryServer 必须拥有独立的服务监听和 owner-to-owner 连接逻辑；监听端口、对端连接、连接恢复和目录命令收发属于 directory server/peer transport 边界。
- OwnerDirectoryServer 必须拥有两个独立监听端口或等价独立 endpoint：一个用于 OwnerDirectoryServer 之间的 owner control/peer 通信，一个用于 Serving SN 向 OwnerDirectoryServer 发布/query directory 信息。
- Serving SN 到 OwnerDirectoryServer 的通信必须经过 owner directory server 的 serving-facing listener，不得通过 SnServer listener 或 owner peer listener 旁路。
- Serving SN 到 OwnerDirectoryServer 不得使用全局 registry fallback、同进程 owner lookup 或其它隐式 shortcut；test/compat 路径也不得保留该 fallback。
- SN 间连接建立和 publish/query/detail/relay 命令通过可插拔验证接口控制准入与授权。
- `sn-miner` 可以加载 owner membership 配置并传入 SN service。
- 增加一个自动化多节点 SN 命令矩阵验证边界：构造 5 个 Owner SN、5 个 Serving SN、5 个用户节点，每个用户节点分别连接不同 Serving SN，并覆盖 peer-facing SN 命令、inter-SN 命令和 owner serving-facing directory 命令的成功与关键失败路径。
- 增加真实进程 DV/integration 验证边界：启动独立 Owner SN 与 Serving SN 进程，验证 owner peer/control transport、serving-facing listener、Serving SN online 心跳、PeerRoute publish/query、跨 Serving SN query/call，以及关键失败路径。

### Out of scope
- 不改变现有客户端可见 `SnQueryResp` 结构。
- 不让客户端直接选择 serving SN。
- 不让 Owner SN 保存完整 NAT/endpoint detail。
- 不把 Owner SN 目录逻辑和 Serving SN peer-facing 逻辑混入同一个业务 handler、同一个 `SnService` 结构体或要求两种角色同进程运行。
- 不把用户级 `PeerRoute` 全量复制到 ownerSN 集群的所有节点。
- 不要求 Serving SN 按每个客户端 peer 高频刷新 ownerSN 状态，也不要求每次 `ReportSn` 都 publish `PeerRoute`。
- 不保留 global registry fallback 作为 test/compat 路径；测试必须使用真实 transport 或显式 fake transport seam。
- 不要求 ownerSN membership 大规模变更期间保持 `PeerRoute` 一定可读；第一版允许超出分区副本容忍范围的变更导致 route miss，并由 Serving SN 后续重报修复。
- 不在 proposal 中固定控制面日志、online state、route、分区或 tombstone 的字段结构；这些属于 design 阶段。
- 不在第一版实现 DHT、开放第三方 SN 准入或 gossip membership 作为 owner 计算输入。
- 不要求实现完整 etcd/Raft 功能集，包括持久 WAL、snapshot、joint consensus、linearizable read 或自动 membership 迁移；第一版只要求网络投票、leader/follower、quorum commit、leader failover 和 Serving SN online/offline 状态复制。
- 不改变 `SnCallResp` 只表示受理或转发的语义。
- 不改变 SN control stream 的低频信令边界。

### Boundary with neighboring modules
- `p2p-frame/src/sn/**` 拥有 owner control plane、分区 peer route、SN 间内部协议、detail 合并和 relay call 语义。
- `sn-miner-rust` 只负责启动配置和装配，不重新定义 owner membership、Serving SN 在线状态、peer route 分区或授权语义。
- `cyfs-p2p` 可以暴露配置或消费 SN 行为，但不得分叉核心 SN 协议语义。
- `p2p-frame/src/tunnel/**` 继续消费 SN call/called 结果，不拥有 SN 分布式目录。

## Assumptions and Ambiguities
- Assumptions:
  - 同一 Owner SN 集群内的 SN 使用可验证的 owner member 列表作为查询域边界。
  - Owner SN 与 Serving SN 在目标部署中不会作为同一角色混合运行；需要交互时必须走 SN 间接口或等价的可替换内部接口。
  - Serving SN 可以持有 owner directory 客户端接口，但不得持有 owner control plane 或 owner route store。
  - ownerSN membership 变化通常小于分区副本数；超出该范围的变更第一版允许出现 route miss。
  - Serving SN 断线后的用户归属删除语义优先通过 Serving SN online/offline 状态批量失效保证，物理删除由分区后台 GC 收敛。
  - `PeerRoute` publish 是独立于 Serving SN online renew 的低频或变化驱动逻辑，不由每次 `ReportSn` 强制触发。
  - `PeerRoute` 分区算法固定，Serving SN 获取同一 ownerSN 列表后可以本地计算写入和查询目标。
  - owner-to-owner 通道能够承载投票、leader heartbeat、leader redirect、Serving SN online/offline 状态复制和必要的提交确认。
- Open ambiguities:
  - ownerSN control plane 的投票消息、term 更新、candidate/leader/follower 状态、leader heartbeat、失效判定、日志提交边界和租约过期规则需要 design 固化。
  - follower 收到 Serving SN 状态上报时的 leader redirect、转发、重试和错误返回语义需要 design 固化。
  - leader 向其它 ownerSN 复制 Serving SN online/offline 状态的 fanout、quorum 成功条件、重复消息幂等和落后节点恢复语义需要 design 固化。
  - Serving SN online renew、主动 offline、异常 offline 超时和恢复语义需要 design 固化。
  - `ReportSn` 与 `PeerRoute` publish 的解耦方式、route 长刷新周期、首次上报/迁移触发和重报修复策略需要 design 固化。
  - Serving SN online heartbeat 与 PeerRoute publish 两条独立 runtime loop 的配置、生命周期和失败处理需要 design 固化。
  - `PeerRoute` 分区算法、replica count、读 fanout、写成功条件和路由缺失恢复策略需要 design 固化。
  - ownerSN membership 变更的第一版限制、拒绝条件和可观测告警需要 design 固化。
  - SN 间内部命令的 command code 区间需要 implementation 前确认。
  - OwnerDirectoryServer owner peer transport 和 serving-facing listener 的端口配置、命令空间、handler 生命周期、验证策略和隔离方式需要 design 固化。
  - `sn-miner` 配置文件格式需要后续设计或实现任务结合现有配置入口确定。
- Decision needed before approval:
  - 是否接受第一版只保证小规模 ownerSN membership 变更下的 `PeerRoute` 可读性。
  - 是否接受 `PeerRoute` route miss 通过 Serving SN 后续重报修复，而不是引入变更期迁移。
  - 是否接受 ownerSN control plane 只同步小状态，用户级 route 由分区副本管理。

## Constraints
- Allowed libraries/components:
  - 复用现有 `PeerManager/CachedPeerInfo` 作为 serving peer detail 来源。
  - 复用现有 `SnConnectionValidator` 的 peer-facing 验证模式，并新增 SN 间验证接口。
  - ownerSN control plane 可以参考 etcd/Raft 的 leader、quorum 和 lease 思路，但具体实现细节由 design 决定。
- Disallowed approaches:
  - 不新增客户端可见的多 serving SN 查询协议。
  - 不让 Owner SN 返回或合成 NAT endpoint detail。
  - 不使用纯 gossip membership 直接参与 owner set 计算。
  - 不把用户级 `PeerRoute` 做成 ownerSN 全集群强一致在线会话数据库。
  - 不让 OwnerDirectoryServer 的运行依赖 SnServer 已经启动的 listener 或 servingSN peer-facing handler。
  - 不让 Serving SN 复用 owner peer 端口或 owner peer 命令节点访问 owner directory。
  - 不使用全局 registry、同进程 lookup 或隐式 singleton 作为 Owner SN/Serving SN 目录通信 fallback。
- System constraints:
  - 单 SN 必须是多 SN 的退化形态。
  - 单 SN 退化形态只能通过显式接口组合 owner/serving 能力，不能成为生产实现中混合角色逻辑或把 owner 字段放进 `SnService` 的理由。
  - 现有 `SnQueryResp` / `SnCallResp` 兼容语义必须保留。
  - 详细 NAT/endpoint 只能由真实 serving SN 基于连接来源生成。
  - proposal 只记录需求、范围、非目标、约束和成功证据；数据结构字段、协议流程和分区算法细节必须放入 design。

## Requirement Challenge
| question | evaluation | risk_or_tradeoff | decision |
|----------|------------|------------------|----------|
| Is the stated requirement reasonable for the user's goal? | 合理。多 SN 部署需要跨 SN 查询 peer 所在 Serving SN，否则用户连接任意 SN 后无法稳定 query/call。 | 分布式目录会引入过期 route、配置不一致、SN 间授权和分区读写失败风险。 | keep |
| Is per-peer TTL lease refresh a good primary mechanism? | 不够好。它会让 ownerSN 持续接受所有客户端节点状态刷新，压力随在线 peer 数线性增长，并把 ownerSN 推向高频在线状态库。Serving SN 在线状态应独立低频上报，用户 route 则按较长周期或变化事件发布。 | 需要把 Serving SN online renew、route publish 和 `ReportSn` 拆成不同流程，但可以把 ownerSN 控制面压力从在线 peer 数中解耦。 | revise to Serving SN online state + independent partitioned peer route publish |
| Should Serving SN use session/epoch to invalidate route? | 不需要。当前目标只要求 ownerSN 判断 Serving SN 是否在线；不区分 Serving SN session/epoch，避免把重启代际和 route 可见性耦合。 | 只按 online/offline 过滤可能让重启前旧 route 在 Serving SN 重新 online 后继续可见；需要依赖 route 长刷新、迁移重报和 detail/query 失败忽略来收敛。 | remove session/epoch requirement; use Serving SN online/offline state |
| Should ownerSN synchronize all directory state to every ownerSN node? | 不应同步全部状态。ownerSN 集群只需要同步 owner/member/config 和 Serving SN online/offline 等小状态；用户级 route 应由固定分区逻辑管理。 | 分区 route 需要处理副本数、读 fanout、写失败和 membership 变更限制，但可避免全局复制压力。 | add `sn_partitioned_peer_route` |
| Should ownerSN health use full-mesh heartbeat as the main requirement? | 不作为 proposal 固定要求。ownerSN 可用性属于 control plane 小状态，可参考 leader/quorum/lease；具体探活和提交机制由 design 决定。 | leader/quorum 增加实现复杂度，但比所有 ownerSN 之间同步所有业务状态更可控。 | add `sn_owner_control_plane_online_state`; design selects mechanism |
| Is an in-memory-only Raft-style state machine sufficient? | 不够。它只能验证本机 term/quorum/online-state 语义，不能满足 ownerSN 之间网络投票、leader failover 和 leader 复制 Serving SN 在线状态的部署要求。 | 需要 owner-to-owner command、状态机角色转换、leader heartbeat 和复制确认，复杂度高于本地状态机。 | add `sn_owner_network_leader_election`; still exclude full etcd/Raft persistence and membership migration |
| Is a fixed 5 ownerSN + 5 servingSN + 5 user topology reasonable as required evidence? | 合理，但它应作为自动化命令矩阵验证边界，而不是改变协议本身的部署上限。5 个 ownerSN 可以覆盖 quorum、follower forward 和 leader failover；5 个 servingSN 和 5 个用户分别连接不同 servingSN 可以覆盖跨 serving query/call、route 分区和 command fanout。 | 如果直接要求真实多进程集成，现有 testing 文档已记录 DV/integration harness gap，可能阻塞实现；design/testing 必须决定使用可运行 simulator/unit harness、DV harness，或明确先补真实多进程 harness。 | add `sn_five_by_five_command_matrix` as required downstream design/testing/implementation evidence |
| What does "all SN commands" cover? | 覆盖三类命令边界：peer-facing `ReportSn`/`ReportSnResp`、`SnQuery`/`SnQueryResp`、`SnCall`/`SnCallResp`、`SnCalled`/`SnCalledResp`；inter-SN `Heartbeat`、`PublishLease`、`QueryLease`、`QueryDetail`、`RelayCall`；owner serving-facing directory `PublishLease`、`QueryLease`。 | 若不明确命令集合，后续可能只测试 query/call happy path，遗漏 owner election、route publish/query、detail relay、response decode 和 validator reject。 | define command matrix coverage under `sn_five_by_five_command_matrix` |
| Should global registry fallback remain for tests or compatibility? | 不应保留。fallback 会让测试绕过 owner serving-facing listener 和 owner peer transport，掩盖真实进程、端口、授权和序列化问题。 | 单元测试需要显式 fake transport seam 或真实 listener；旧的同进程便利性会下降。 | remove registry fallback from production, test, and compat paths |
| Are Serving SN online heartbeat and PeerRoute publish one workflow? | 不是。online heartbeat 表示 serving 实例可用性；PeerRoute publish 表示用户归属路由刷新。两者可以使用同一个 owner directory client，但必须是独立 lifecycle、配置和触发条件。 | 实现需要拆分 runtime loop 和错误处理，配置项更多。 | require independent online heartbeat and route publish loops |
| Is unit-level 5x5 evidence enough after adding process-startup requirements? | 不够。5x5 unit matrix 能证明命令语义，但不能证明 `sn-miner` 配置、真实进程、listener、connector、port conflict 和 shutdown。 | 需要新增较慢的 DV/integration harness，但这是部署边界必需证据。 | require real-process DV/integration evidence |
| Is scope ambiguous? | 已明确 proposal 不写具体结构定义，只写需求相关关注；control plane 小状态和 partitioned route 的字段、日志、命令和算法细节留给 design。 | 如果 proposal 写入过细结构，会限制设计空间并导致阶段边界混乱。 | keep proposal at requirement level |

## Large Module Submodule Decision
| Submodule | New or Existing | Responsibility | Proposal Packet | Reason |
|-----------|-----------------|----------------|-----------------|--------|
| sn-distributed-directory | existing | SN 分布式 owner control plane、Serving SN online/offline 状态、分区 peer route、跨 SN detail/query/call relay | `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/proposal.md` | 这是既有 direct submodule packet，本次调整仍属于该分布式目录能力边界，不新增另一个 direct submodule。 |

## Trigger Matrix
| trigger_category | applies | evidence | required_checks | deferred_checks_and_reason |
|------------------|---------|----------|-----------------|----------------------------|
| contract/protocol | yes | 新增/调整 owner control plane、Serving SN online/offline、partitioned peer route、detail/relay 内部命令；客户端可见 SN 命令不变。 | design must define internal wire compatibility, command id mapping, route query behavior, and client-facing invariants; testing must cover old `SnQueryResp`/`SnCallResp` behavior. | owner: design/testing; risk: protocol regression; acceptance impact: verify old client-facing behavior remains stable. |
| data/schema | yes | 新增 owner control plane 小状态、Serving SN online/offline、partitioned peer route 和 route GC 语义。 | design must assign state ownership, partition ownership, online-state invalidation, route sequence/tombstone rules, and GC behavior. | owner: design/testing; risk: stale route, stale online state, split-brain control state, and sequence overwrite; acceptance impact: verify ownership and expiry rules. |
| security/privacy/permission | yes | SN 间连接建立、directory publish/query、detail query 和 relay call 都需要可插拔准入与授权。 | design must define connection and command validators; testing must cover reject paths. | owner: design/testing; risk: unauthorized online-state or NAT detail access; acceptance impact: validator reject must block effects. |
| runtime/integration | yes | 多 SN report/query/call、owner control plane、Serving SN online renew/offline/expiry、partitioned route read/write、OwnerDirectoryServer 双 listener 涉及多个运行时。 | DV/integration must cover multi-SN success and failure workflows plus servingSN offline invalidation, route partition query, owner listener behavior, and relay behavior. | owner: testing; risk: distributed timeout, route miss, port conflict, and partial failure; acceptance impact: online/offline invalidation and route partition evidence must exist. |
| build/dependency/config/deployment | yes | `sn-miner` 需要 owner membership、control plane、Serving SN online 状态和双 listener 配置装配边界。 | design must define config ownership; testing should cover default no-config compatibility and config validation. | owner: design/testing; risk: deployment misconfiguration and port conflict; acceptance impact: single SN default must still work. |
| ui/datamodel/workflow | no | 本 change 不涉及 UI 或外部应用数据模型。 | not-applicable | owner: none; risk: none; acceptance impact: none. |
| harness/process | yes | Existing direct submodule packet remains required for implementation admission, and approved downstream docs become stale after this draft proposal update. | doc-structure-check, schema-check, stage-scope-check. | owner: proposal/design; risk: admission mismatch; acceptance impact: change_id evidence must be regenerated after approval and downstream updates. |

## High-Level Outcomes
- 单 SN 未配置 owner 时，现有 SN report/query/call 行为保持等价。
- 多 SN 配置同一 Owner SN 集群后，用户连接任意 Serving SN 都能发布自身归属。
- Owner SN 集群只同步 owner/control/online 小状态，不全量同步用户级 route。
- Serving SN 异常断线后，其 offline 状态能让相关 `PeerRoute` 在查询语义上批量失效。
- `PeerRoute` 按固定分区逻辑写入和查询，支持按 ownerSN 列表动态计算 route owner set。
- `ReportSn` 不再强制每次触发 `PeerRoute` publish；同一用户 route 可以使用较长刷新周期，Serving SN 在线状态由独立低频逻辑上报。
- 查询方 SN 能通过分区 route 找到一个或多个 Serving SN，并把所有可用 detail 合并成现有 `SnQueryResp` 返回给客户端。
- SN call 本地 miss 时可跨 SN relay，remote serving SN 负责向本机被叫投递 `SnCalled`。
- SN 间连接和内部命令可由使用方提供验证实现控制。
- 自动化验证能在 5 Owner SN、5 Serving SN、5 user peer 的拓扑中覆盖所有 SN 命令族，并证明每个 user peer 分别连接不同 Serving SN 时 report/query/call/called 与内部 route/detail/relay/control 命令仍保持一致。
- 真实进程 DV/integration 能证明配置驱动的 Owner SN 与 Serving SN 作为独立进程通过真实 owner peer/control 与 serving-facing transport 工作，不依赖全局 registry fallback。
- Serving SN online 心跳与 PeerRoute publish 作为独立 runtime loop 可分别配置、分别失败和分别测试；`ReportSn` 不触发 online heartbeat，也不强制每次 publish route。

## Proposal Items
| proposal_id | change_id | Outcome | Scope Boundary | Success Evidence | Explicit Non-Goal |
|-------------|-----------|---------|----------------|------------------|-------------------|
| P-SN-DIST-CONTROL-1 | sn_owner_control_plane_online_state | Owner SN 集群同步 owner/control/online 小状态，并用 Serving SN online/offline 批量判定 Serving SN 可用性。 | `p2p-frame/src/sn/directory/**`、`p2p-frame/src/sn/inter_sn/**`、必要 `sn-miner` 配置装配。 | Serving SN 正常 online renew 时可继续返回其 route；主动 offline 或异常 offline 超时后，其旧 route 不再被 query 返回。 | 不同步用户级 `PeerRoute` 到所有 ownerSN；不区分 Serving SN session/epoch；不在 proposal 固定日志和字段结构。 |
| P-SN-DIST-CONTROL-2 | sn_owner_network_leader_election | Owner SN 之间通过网络投票选举 leader；leader 正常时 follower 与 leader 通信，leader 失效后 quorum 内重新选主。 | `p2p-frame/src/sn/directory/**`、`p2p-frame/src/sn/inter_sn/**`、`p2p-frame/src/sn/mod.rs`、必要 `sn-miner` 配置装配。 | 多 ownerSN 进程或等价 DV 中可观察到投票选主；停止 leader 后可重新选出 leader；follower 不直接提交 online/offline 变更。 | 不实现完整 etcd/Raft 持久化、snapshot、joint consensus 或 linearizable read。 |
| P-SN-DIST-CONTROL-3 | sn_owner_leader_online_replication | Serving SN online/renew-online/offline 事件由 leader 提交并同步给其它 ownerSN，Serving SN 异常离线事件也由 leader 复制到集群其它机器。 | `p2p-frame/src/sn/directory/**`、`p2p-frame/src/sn/inter_sn/**`、必要 owner directory serving-facing listener。 | Serving SN 状态上报到 follower 时被转发或 redirect 到 leader；leader commit 后其它 ownerSN 查询同一 Serving SN online/offline 状态一致；leader 失效重选后可继续处理后续状态事件。 | 不复制用户级 `PeerRoute` 到所有 ownerSN；只复制 control/online 小状态。 |
| P-SN-DIST-ROUTE-1 | sn_partitioned_peer_route | 用户级 peer route 由固定分区逻辑管理，Serving SN 根据 ownerSN 列表本地计算 route owner set，并按较长刷新周期、首次上报、迁移或重报修复触发批量写入/query。 | `p2p-frame/src/sn/directory/**`、`p2p-frame/src/sn/inter_sn/**`、必要 `sn/service` 接入。 | route 只写入分区 owner set；query fanout 到分区 owner set；route 返回前必须校验 Serving SN online 状态；每次 `ReportSn` 不强制 publish route。 | 不做 route 全 ownerSN 广播；不把 route publish 作为 Serving SN online renew；第一版不做 membership 变更期迁移。 |
| P-SN-DIST-ROLE-1 | sn_owner_serving_role_boundary | Owner SN 与 Serving SN 作为独立运行角色和业务边界建模，生产逻辑不假设两者同进程或共享 peer-facing handler。 | `p2p-frame/src/sn/directory/**`、`p2p-frame/src/sn/inter_sn/**`、`p2p-frame/src/sn/service/**` 中角色接口与装配边界。 | Owner 目录能力可独立于 serving detail/call 逻辑验证；Serving SN 通过接口发布/query route 和拉取/relay detail，不直接访问 owner 内部 store。 | 不为了单 SN 兼容把 owner/serving 逻辑重新混成同一个业务路径。 |
| P-SN-DIST-DIRECTORY-SERVER-1 | sn_directory_client_server_boundary | Owner SN 功能从 `SnService` 中拆出，作为 `sn/directory` 模块内部 server 侧能力承载；Serving SN 只通过 directory client 接口交互。 | `p2p-frame/src/sn/directory/**`、`p2p-frame/src/sn/service/**`、`p2p-frame/src/sn/mod.rs`、必要装配路径。 | `SnService` 不包含 owner membership/control/route store/owner command handler；directory server 可独立验证 route publish/query；Serving SN 只通过 directory client/inter-SN 接口交互。 | 不在 `SnService` 内保留 ownerSN 状态字段或 ownerSN command 分发；不再暴露独立 `sn::owner` 模块。 |
| P-SN-DIST-OWNER-PEER-TRANSPORT-1 | sn_owner_directory_peer_transport | OwnerDirectoryServer 提供独立 listener 和 owner peer/control transport，承载 owner control plane 与 owner directory 内部命令。 | `p2p-frame/src/sn/directory/**`、必要 peer transport 和命令节点接入、`sn-miner-rust` owner directory 装配。 | OwnerDirectoryServer 不依赖 SnServer listener 启动即可监听 owner 端口；owner/control 命令与 serving-facing 命令隔离。 | 不把 owner peer/control transport 和普通客户端 SN 协议混用。 |
| P-SN-DIST-OWNER-SERVING-LISTENER-1 | sn_owner_serving_listener_transport | OwnerDirectoryServer 提供独立 serving-facing listener，供 Serving SN 发布/query online 状态和 peer route；该入口与 owner peer listener 端口、命令空间和 handler 集合隔离。 | `p2p-frame/src/sn/directory/**`、`p2p-frame/src/sn/inter_sn/**`、必要 serving directory command 接入、`sn-miner-rust` owner directory 装配。 | OwnerDirectoryServer 启动后同时拥有 owner peer/control listener 和 serving-facing listener；Serving SN 能通过 serving-facing listener 访问 owner directory；两类命令互不复用。 | 不通过 SnServer listener、owner peer listener 或同进程 shortcut 处理 Serving SN 到 owner directory 的 publish/query。 |
| P-SN-DIST-QUERY-1 | sn_distributed_query_merge | `SnQuery` 本地 miss 或多 SN 命中时，由 serving SN 拉取 remote detail 并合并 endpoint 后返回现有 `SnQueryResp`。 | `p2p-frame/src/sn/service/**`、`p2p-frame/src/sn/inter_sn/**`、现有 `SnQueryResp` 构造路径。 | 客户端 API 不变；多个 serving SN detail endpoint 去重合并；无可用 detail 时返回当前空 query 语义。 | 不新增客户端可见 `SnQueryServing` 或多 serving SN 选择接口。 |
| P-SN-DIST-CALL-1 | sn_distributed_relay_call | `SnCall` 本地 miss 时通过分区 route 找到 remote serving SN 并 relay call，由 remote serving SN 投递 `SnCalled`。 | `p2p-frame/src/sn/service/**`、`p2p-frame/src/sn/inter_sn/**`。 | 本地 call 路径不变；remote serving SN accepted 后 `SnCallResp` 仍只表示受理/转发。 | 不改变最终 tunnel 连通性语义；第一版不要求 fanout 到所有 serving SN。 |
| P-SN-INTER-AUTH-1 | sn_inter_service_validation | 新增 SN 间连接建立准入和命令级 publish/query/detail/relay 授权接口。 | `p2p-frame/src/sn/inter_sn/validator.rs`、`SnServiceConfig` 装配路径。 | validator reject 时不建立 SN 间内部协议连接或不产生 owner write/detail/relay side effect。 | 不固化使用方权限模型；默认实现可 allow-all。 |
| P-SN-DIST-CMD-MATRIX-1 | sn_five_by_five_command_matrix | 自动化构造 5 Owner SN、5 Serving SN、5 user peer 的测试拓扑，每个 user peer 分别连接不同 Serving SN，并覆盖所有 SN 命令族的成功、跨 serving 和关键失败路径。 | `p2p-frame/src/sn/**` 中测试或必要测试支撑；如需真实多进程 harness，范围必须在后续 design/testing 中明确。 | 测试证据显示 5 个用户分别通过不同 Serving SN 完成 `ReportSn`、跨 Serving SN `SnQuery`、`SnCall`/`SnCalled`，并覆盖 inter-SN `Heartbeat`、`PublishLease`、`QueryLease`、`QueryDetail`、`RelayCall` 以及 owner serving-facing `PublishLease`、`QueryLease` 的 dispatch/response/reject 路径。 | 不新增客户端可见命令；不要求客户端选择 Serving SN；不把测试拓扑规模解释为生产部署上限。 |
| P-SN-DIST-NO-FALLBACK-1 | sn_directory_no_registry_fallback | 删除 Owner SN / Serving SN 目录通信的全局 registry fallback；生产、测试和兼容路径都必须使用真实 transport 或显式 fake transport seam。 | `p2p-frame/src/sn/directory/**`、`p2p-frame/src/sn/inter_sn/**`、`p2p-frame/src/sn/service/**`、必要测试支撑。 | 搜索和测试证据证明 Serving SN directory publish/query、inter-SN detail/relay、owner control command 不依赖 global registry fallback；unit fake 必须显式注入。 | 不禁止单元测试使用 fake transport trait；禁止隐式 singleton registry fallback。 |
| P-SN-DIST-ONLINE-ROUTE-LOOPS-1 | sn_serving_online_route_independent_loops | Serving SN online heartbeat/renew/offline 与 PeerRoute publish/query 是独立 runtime loop、独立配置和独立失败处理。 | `p2p-frame/src/sn/service/**`、`p2p-frame/src/sn/directory/**`、`sn-miner-rust/**` 装配。 | 测试证明 `ReportSn` 只更新本地 detail；online heartbeat 不写 PeerRoute；route publish 不刷新 Serving SN online；两者失败可独立观测。 | 不要求两者使用不同网络连接；只要求语义和触发独立。 |
| P-SN-DIST-REAL-PROCESS-1 | sn_real_process_owner_serving_dv | 新增真实进程 DV/integration，启动独立 Owner SN 与 Serving SN 进程验证配置、listener、connector、query/call 和失败路径。 | `p2p-frame` DV/integration harness、`sn-miner-rust/**` 配置/启动、必要测试夹具。 | 测试证据包含进程启动、端口分配、owner 双 listener、serving 连接 owner serving-facing endpoint、online heartbeat、route publish/query、跨 serving query/call、shutdown 和至少一个 invalid config/port conflict 失败路径。 | 不把 unit 5x5 command matrix 当作真实进程证据。 |

## Success Criteria
- Concrete user-visible or system-visible result:
  - 单 SN 默认配置无需改变即可继续工作。
  - 多 SN 同一 owner 域内，query/call 可跨 serving SN 工作。
  - ownerSN 集群只同步 owner/control/online 小状态，不因在线 peer 数量增长而全量复制所有 route。
  - ownerSN 集群能通过网络投票选主，leader 失效后能重新选主；leader 正常时 follower 不绕过 leader 提交 Serving SN 在线状态。
  - Serving SN 正常 online renew、主动 offline 和异常 offline 都能影响 query 返回语义。
  - Serving SN online/offline 状态由 leader commit 并复制到其它 ownerSN，使 follower 读取到 committed online 状态。
  - Serving SN 断线后不需要逐个删除 peer route；offline 状态即可让旧 route 不再返回。
  - `ReportSn` 与 `PeerRoute` publish 解耦，同一用户 route 可以使用较长刷新周期，Serving SN 在线状态由独立低频逻辑维护。
  - 分区 route 的读写目标可由固定 ownerSN 列表和分区算法动态计算。
  - ownerSN 和 servingSN 角色边界在设计与实现中可被独立审计，不依赖同进程混合逻辑；`SnService` 审计面只包含 servingSN。
  - 客户端仍使用现有 `SNClientService::query()` / `call()` 与现有响应结构。
  - 5 Owner SN、5 Serving SN、5 user peer 的自动化命令矩阵可运行，并证明 5 个用户分别连接不同 Serving SN 时所有 SN 命令族按设计工作。
- Required evidence:
  - Design 直接映射全部 change_id，并把结构定义、协议字段、分区算法和控制面机制放在 design。
  - Post-implementation testing 覆盖单 SN 等价、多 SN online/offline invalidation、partitioned route、query merge、relay call、validator reject。
  - Post-implementation testing 必须为 `sn_five_by_five_command_matrix` 记录可运行入口、命令覆盖矩阵、拓扑规模、每个 user peer 到不同 Serving SN 的绑定证据、成功路径结果和关键失败路径结果。
  - Post-implementation testing 必须为 `sn_real_process_owner_serving_dv` 记录真实进程入口、配置文件、进程日志/退出码、端口分配、成功路径和失败路径结果。
  - 静态搜索或等价审计必须证明全局 registry fallback 不再是 Owner SN / Serving SN directory 通信路径。
- Explicit non-goals:
  - 不提供客户端选择 serving SN 的新 API。
  - 不引入 DHT/gossip owner 计算。
  - 不改变 SN peer-facing wire 结构。
  - 不在 proposal 中定义具体数据结构字段。

## Risks
- ownerSN control plane leader/quorum 实现复杂度较高；design 必须定义失败、恢复和租约过期语义。
- ownerSN membership 配置不一致会导致分区计算不一致；design 必须定义配置版本、拒绝条件和日志暴露。
- membership 变更超过分区副本容忍范围时，第一版允许 route miss；该限制必须在 design/testing/acceptance 中可见。
- Serving SN online TTL 过短会造成误 offline，过长会延迟断线收敛；design 必须定义默认值、续约频率和恢复条件。
- stale route 可能在 Serving SN online 有效期内返回不可达 serving SN，查询方必须忽略失败 detail 并尝试其它可用 route。
- Serving SN 重启后不区分 session/epoch，旧 route 可能在重新 online 后短暂可见；design/testing 必须覆盖通过 route 长刷新、迁移重报、detail 查询失败忽略和后台 GC 收敛。
- 跨 SN detail 查询暴露 NAT endpoint，需要验证接口在连接和命令两层都能拒绝未授权请求。
- 多 SN query fanout 会增加延迟，第一版应有超时和失败忽略策略。
- OwnerDirectoryServer 双监听端口增加部署复杂度；design 必须定义 owner peer/control 端口和 serving-facing 端口的配置、默认禁用/启用策略、冲突检测和启动失败行为。
- 移除 registry fallback 后，未配置 endpoint 的测试和部署会失败；design/testing 必须提供显式 fake seam 或真实进程配置。
- 真实进程 DV/integration 可能较慢且易受端口冲突影响；testing 必须定义稳定端口分配、超时、日志收集和 cleanup。

## Downstream Follow-Up
| follow_up_id | Owning Stage | Reason | Triggering Proposal Item | Blocking |
|--------------|--------------|--------|--------------------------|----------|
| FU-SN-CONTROL-DESIGN | design | 需要定义 ownerSN control plane、Serving SN online/offline、主动 offline、异常 offline expiry、leader/quorum 或等价机制，以及状态 owner。 | P-SN-DIST-CONTROL-1 | yes |
| FU-SN-NETWORK-LEADER-DESIGN | design | 需要定义 ownerSN 网络投票、candidate/follower/leader 状态、leader heartbeat、leader 失效重选、follower redirect/forward、Serving SN online 状态复制和 quorum commit。 | P-SN-DIST-CONTROL-2 / P-SN-DIST-CONTROL-3 | yes |
| FU-SN-PARTITIONED-ROUTE-DESIGN | design | 需要定义 `PeerRoute` 分区算法、replica count、读 fanout、写成功条件、Serving SN online 校验、较长刷新周期、`ReportSn` 解耦、route miss 修复和 GC。 | P-SN-DIST-ROUTE-1 | yes |
| FU-SN-DIST-DESIGN-REFRESH | design | 既有 design 仍以 `ServingLease` TTL 和 OwnerMember heartbeat 为主要模型，需要按本 proposal 更新结构定义、流程和 Scope Paths。 | all proposal items | yes |
| FU-SN-ROLE-BOUNDARY-DESIGN | design | 需要保持 ownerSN/servingSN 独立运行角色、接口交互边界、单 SN 退化组合方式和禁止同进程 shortcut 的约束。 | P-SN-DIST-ROLE-1 | yes |
| FU-SN-OWNER-PEER-TRANSPORT-DESIGN | design | 需要定义 OwnerDirectoryServer 独立 listener、owner peer/control transport、连接身份、连接恢复、command id/handler/request-response 映射和与 Serving SN directory client 的边界。 | P-SN-DIST-OWNER-PEER-TRANSPORT-1 | yes |
| FU-SN-OWNER-SERVING-LISTENER-DESIGN | design | 需要定义 OwnerDirectoryServer serving-facing listener、Serving SN online/route publish/query 命令、端口配置、命令空间隔离、验证策略和与 owner peer listener 的边界。 | P-SN-DIST-OWNER-SERVING-LISTENER-1 | yes |
| FU-SN-DIST-TESTING | testing | 需要 post-implementation 测试设计覆盖多 SN runtime、online/offline invalidation、partitioned route、`ReportSn` 不逐次 publish、query merge、relay 和 validator reject。 | all proposal items | yes |
| FU-SN-FIVE-BY-FIVE-DESIGN | design | 需要定义 5 Owner SN、5 Serving SN、5 user peer 命令矩阵验证使用真实多进程 DV、in-memory harness 还是混合方式，并映射所有命令族、响应和失败路径。 | P-SN-DIST-CMD-MATRIX-1 | yes |
| FU-SN-FIVE-BY-FIVE-TESTING | testing | 需要生成或更新测试设计、testplan 和可运行测试入口，证明每个 user peer 绑定不同 Serving SN 且所有 SN 命令族被执行或明确记录不可自动化原因。 | P-SN-DIST-CMD-MATRIX-1 | yes |
| FU-SN-FIVE-BY-FIVE-IMPLEMENTATION | implementation | 需要在 design/testing 批准后实现必要测试支撑或 runtime wiring，使命令矩阵测试不依赖 chat-only 假设。 | P-SN-DIST-CMD-MATRIX-1 | yes |
| FU-SN-NO-FALLBACK-DESIGN | design | 需要定义移除 global registry fallback 后的 production transport 和 unit fake seam。 | P-SN-DIST-NO-FALLBACK-1 | yes |
| FU-SN-ONLINE-ROUTE-LOOPS-DESIGN | design | 需要定义 Serving SN online loop 与 route publish loop 的配置、触发、错误处理和观测信号。 | P-SN-DIST-ONLINE-ROUTE-LOOPS-1 | yes |
| FU-SN-REAL-PROCESS-TESTING | testing | 需要设计真实进程 DV/integration，覆盖 sn-miner config、owner/serving 进程、transport 和失败路径。 | P-SN-DIST-REAL-PROCESS-1 | yes |
| FU-SN-REAL-PROCESS-IMPLEMENTATION | implementation | 需要在 design/testing 批准后实现 transport/fallback 清理、loop 拆分和真实进程测试支撑。 | P-SN-DIST-NO-FALLBACK-1 / P-SN-DIST-ONLINE-ROUTE-LOOPS-1 / P-SN-DIST-REAL-PROCESS-1 | yes |
| FU-SN-DIST-ACCEPTANCE | acceptance | 需要审计 proposal/design/code/testing 一致性，尤其客户端兼容性、分区路由、online/offline 批量失效和权限边界。 | all proposal items | yes |

## Proposal Guardrails
- Proposal-stage tasks modify only `proposal.md` unless the user explicitly requests a multi-stage update.
- If this proposal change requires design, implementation, testing, or acceptance updates, record the needed follow-up instead of editing downstream artifacts by default.
- If this is a large module with many independent submodules, classify whether the requested feature is a new direct submodule before design or testing starts.
- Put the split submodule's proposal and design files in a submodule directory under this module packet; optional post-implementation testing artifacts may live there when generated. Do not use `design/<submodule>/` or `testing/<submodule>/` for independent submodule docs.
- Keep human-authored proposal docs under 1000 lines where practical; if a doc would exceed 1000 lines, split it and update the relevant document index.
- If the request has multiple plausible meanings, record the ambiguity instead of silently choosing one.
- Proposal approval should not depend on chat-only context; task-critical assumptions belong in this file.
- Keep implementation strategy out of proposal except where a constraint is part of the requirement.
- Every implementation-ready requirement must have a stable `change_id`.
- A broad module-level statement is not enough for implementation admission; the relevant `change_id` must name the concrete behavior, contract, or implementation unit being admitted.

## Approval Record
- approver: user
- approval_date: 2026-06-22T21:02:36+08:00
- user_statement: "批准 sn-miner proposal 和 p2p-frame/sn-distributed-directory proposal，并启动 auto-pipeline 自动处理后续 design、implementation、testing、acceptance。"
