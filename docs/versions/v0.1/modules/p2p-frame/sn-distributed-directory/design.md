---
module: p2p-frame
submodule: sn-distributed-directory
version: v0.1
status: approved
approved_by: auto-pipeline
approved_at: 2026-06-22T21:11:15+08:00
approved_content_sha256: 94f3fe3cb83a161c353173747955a0b400726fc8bc0cbe76acabdb1f6dc176a1
---

# SN 分布式目录 Design

## Design Scope
### Goals
- 将 approved proposal 中的 ownerSN control/online 小状态同步落为可实现设计。
- 保留最小 Raft-style owner control plane：ownerSN membership 内节点通过 term、leader、quorum commit 管理 control/online 状态。
- 补齐 owner-to-owner 网络投票、leader heartbeat、leader 失效重选、follower redirect/forward 和 leader online-state replication。
- 用 Serving SN online/offline 状态批量判定 Serving SN 可用性；Serving SN 不区分 session 或 epoch。
- 将 Serving SN online renew 与用户 `ReportSn` 解耦；`ReportSn` 不再每次强制 publish route。
- 让用户级 `PeerRoute` 按固定 ownerSN 列表分区，只写入/query 分区 owner set，不全量复制到所有 ownerSN。
- 接入真实 `SnServer` / inter-SN transport 路径，删除 Owner SN / Serving SN directory 通信的 global registry 或 same-process shortcut fallback。
- 将 Serving SN online heartbeat 和 PeerRoute publish 明确建模为独立 runtime loop、独立配置面和独立失败面。
- 为独立 Owner SN 与 Serving SN 真实进程 DV/integration 保留可测试 transport/config seam。
- 对 owner control forward 增加远端 owner 身份校验，拒绝非 membership owner 或身份不匹配的 forward。
- 增加 5 Owner SN、5 Serving SN、5 user peer 的自动化命令矩阵验证设计，每个 user peer 固定绑定不同 Serving SN，并覆盖 peer-facing、inter-SN、owner serving-facing directory 命令族。
- 保持 Owner SN 和 Serving SN 运行角色分离，`SnService` 继续只承载 Serving SN peer-facing 功能。
- 保持现有客户端可见 `ReportSn`、`SnQuery`、`SnCall`、`SnCalled` 结构和语义不变。

### Non-goals
- 不实现完整 etcd 替代品、持久化 WAL、snapshot、joint consensus、linearizable read 或自动 membership migration。
- 不把用户级 `PeerRoute` 同步到所有 ownerSN。
- 不让 Owner SN 保存完整 NAT/endpoint detail。
- 不改变普通客户端 SN wire 协议。
- 不把 Owner SN control plane 状态放入 `SnService`。
- 不要求每个客户端 `ReportSn` 都触发 route publish 或 Serving SN online renew。
- 不通过 Serving SN session/epoch 区分重启代际。
- 不在 production、test 或 compat 路径中保留 global registry fallback 作为 Owner SN / Serving SN directory 通信方式。

## Overall Approach
`sn_directory` 继续作为 Owner SN 目录域，但内部职责调整为两层状态：

- control plane 小状态：owner membership/config、term/leader/quorum commit、Serving SN online/offline。
- partitioned route 大状态：`peer_id -> serving_sn_id/sequence`，由固定分区算法根据 ownerSN 列表计算分区 owner set。

每个 OwnerDirectoryServer 维护本地 `OwnerControlPlane` 和 `OwnerElectionNode`。`OwnerElectionNode` 持有 local owner id、membership、current term、voted_for、role、leader id、heartbeat deadline 和 owner peer client。leader 正常时，follower 收到 Serving SN online/renew/offline 或 owner control forward 必须验证远端 owner 身份后转发给 leader，或返回 leader id 让调用方重试；follower 不直接提交 online-state mutation。

leader 对 Serving SN online/offline 事件生成 control entry，并通过 owner-to-owner transport 复制到其它 ownerSN。只有获得 quorum ack 后，leader 才提交并应用该 entry；followers 只能应用 leader 已 commit 的 entry，不能在 leader quorum commit 前把 replicated entry 变成查询可见状态。leader heartbeat 超时后 follower 进入 candidate，递增 term，向其它 ownerSN 发 vote request，获得 quorum 后成为 leader 并开始 heartbeat。

Serving SN 使用独立低频 online renew 表示自身可用性，主动 shutdown 时发送 offline；异常断线由 owner control plane online TTL/heartbeat timeout 判定 offline。用户 `ReportSn` 只更新 Serving SN 本地 `PeerManager` detail；route publish 是独立流程，在首次 report、serving 迁移、route refresh window、repair/re-report 或显式 route update 时批量写入 owner route 分区。查询 route 时，Owner SN 必须先确认对应 Serving SN 当前 online，再返回 route；offline Serving SN 的旧 route 在查询语义上隐藏，物理删除由后台 GC 收敛。

Serving SN 与 OwnerDirectoryServer 通信必须通过 serving-facing listener 或显式注入的 fake/test transport seam。`SnServer::new` 在配置 owner membership 时必须构造真实 `TtpInterSnClient` 并传给 `SnService` 与 `StaticOwnerDirectoryClient`，从而让 query/call/detail/relay 和 directory publish/query 走 SnServer/inter-SN transport。global `InterSnRegistry` 或同进程 owner lookup 不得作为 production、test 或 compat fallback；单元测试需要 fake 时必须通过 trait/constructor 显式注入。

Serving SN online heartbeat loop 只负责 online/renew/offline 可用性上报；PeerRoute publish loop 只负责首次 report、serving 迁移、refresh window、repair/re-report 或显式 route update 的 route 写入。两个 loop 可以共享底层 directory client，但不得共享一个触发条件、一个失败状态或一个配置开关。

5x5 命令矩阵验证不引入新的生产协议。测试拓扑使用 5 个 ownerSN id 构造同一 `OwnerMembership`，5 个 Serving SN service 或可替换 test peer 暴露现有 directory/inter-SN 接口，5 个 user peer 以一对一关系连接到不同 Serving SN。矩阵必须执行 peer-facing `ReportSn`/`SnQuery`/`SnCall`/`SnCalled` 及响应路径，inter-SN `Heartbeat`/`PublishLease`/`QueryLease`/`QueryDetail`/`RelayCall`，以及 owner serving-facing `PublishLease`/`QueryLease` dispatch。若真实多进程 TTP 生命周期仍无 harness，unit 层必须覆盖 command dispatch、response decode、topology binding 和 validator/reject no-side-effect，并在 testing 中保留 DV/integration gap。

## Simplicity Check
- Smallest sufficient approach: 内存 owner election + committed online/offline 小状态 + fixed partition route；不实现持久化 Raft。
- Existing components or patterns reused: 复用 `OwnerMembership`、owner-to-owner `TtpNode`/cmd transport、serving-facing `TtpServer`/cmd server、`PeerManager`、inter-SN validator。
- New abstractions introduced or retained: `OwnerControlPlane`、`OwnerElectionNode`、`OwnerPeerControlClient`、`ServingSnOnlineState`、`OwnerOnlineEntry`、`PeerRoute`、`PeerRouteStore`、route publish policy。
- Why each abstraction is necessary: control plane 承载 committed 小状态；election node 承载网络投票和 leader/follower runtime；owner peer client 隔离网络命令；online state 批量失效 servingSN；route store 保存分区 peer route；route publish policy 解耦 `ReportSn` 与 owner route 写入。

## Current Structure
- `sn/directory/mod.rs` 当前导出 owner membership、control plane、election、route、directory client/server 类型。
- `sn/directory/control_plane.rs` 当前仍以 `ServingSnSession` / epoch 表达 Serving SN 可用性，需要改为 online/offline 小状态。
- `sn/directory/election.rs` 当前承载 owner election 和 session replication，需要改为 online-state replication，并确保 follower 只应用 committed entry。
- `sn/directory/route.rs` 当前 `PeerRoute` 含 serving epoch，需要改为不依赖 epoch 的 route sequence 模型。
- `sn/directory/client.rs` 当前 `OwnerDirectoryClient` 发布/query serving lease，需要拆为 Serving SN online renew/offline 与 peer route publish/query。
- `sn/directory/server.rs` 当前 owner peer 命令和 serving-facing 命令仍以 lease/session 命名，需要改为 online/route 命令，并在 owner control forward 校验远端 owner。
- `sn/inter_sn/mod.rs` 当前承载 owner heartbeat、publish/query lease、detail query 和 relay call，需要增加或重命名 online/route 内部请求并接入 real TTP dispatch。
- `sn/service/service.rs` 当前 `ReportSn` 后仍可能触发 route/lease publish，且 `SnServer::new` 仍存在缺省 client 或 legacy registry 兼容路径，需要解耦 report 与 route publish、接入 `TtpInterSnClient`，并删除隐式 registry fallback。

## Invariants to Preserve
- 未配置 owner directory 时，单 SN report/query/call 行为保持等价。
- 客户端可见 `SnQueryResp` / `SnCallResp` 不变。
- `PeerManager` 仍是 Serving SN 本地完整 NAT/detail 的 owner。
- Owner SN 不直接读取 Serving SN `PeerManager`。
- Serving SN 不直接写 owner store，只通过 directory client / inter-SN command。
- `SnService` 不包含 owner control plane、route store、owner command handler。
- owner peer listener 和 serving-facing listener 保持端口、purpose、命令空间和 handler 隔离。
- `PeerRoute` 返回前必须检查 Serving SN online/offline 状态。
- follower ownerSN 不直接提交 Serving SN online/offline 状态；leader 正常时 mutation 只能由 leader commit。
- follower 不得在 leader quorum commit 前应用 replicated online-state entry。
- leader heartbeat 超时后，满足 quorum 的 ownerSN 必须能重新选主；无 quorum 时不能提交新的 online-state。
- remote owner control forward 必须验证连接身份是 membership 内 owner，且远端身份与 forward 来源一致。

## Submodules
| Submodule | Type | Responsibility | Depends On | Exported Interface | Notes |
|-----------|------|----------------|------------|--------------------|-------|
| `sn_directory` | shared | owner control plane、Serving SN online/offline、partitioned peer route、directory client/server、serving-facing listener | `identity_tls`, `sn_inter_service` technical interfaces, `NetManager`, `TtpServer`, `DefaultCmdServerService` | `OwnerControlPlane`, `OwnerDirectoryServer`, `OwnerDirectoryClient`, route/online model | shared because serving integration and owner server both consume it |
| `sn_inter_service` | technical | owner peer command transport、internal request/response、connection/command validator、detail/relay commands | `NetManager`, `TtpNode`, `DefaultCmdNodeService`, `identity_tls` | internal inter-SN client/service | 不导出给普通客户端 |
| `sn_service_integration` | business | Serving SN report/query/call handler, online reporter, route publish policy, detail merge | `sn_directory` client interface, `sn_inter_service`, existing `PeerManager` | existing SN service APIs | 不拥有 owner control/online/route state |
| `sn_miner_config` | assembly | 装配 owner membership、directory server/client、SnServer inter-SN client 和 listener config | `sn_directory`, `sn_service_integration` | sn-miner config wiring | assembly only |

## Large Module Submodule Decision
| Submodule | Source Proposal | Decision | Design Packet | Reason |
|-----------|-----------------|----------|---------------|--------|
| `sn-distributed-directory` | P-SN-DIST-CONTROL-1 / P-SN-DIST-ROUTE-1 | existing direct submodule | `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/design.md` | owner control/online and partitioned route remain inside the existing SN distributed directory boundary; no new independent direct submodule is needed. |

## Boundary Decision Matrix
| boundary | classification | business_responsibility | shared_logic_or_technical_area | decision |
|----------|----------------|-------------------------|--------------------------------|----------|
| owner control/online vs peer route | business | control/online 是小状态集群一致性；peer route 是大状态分区目录 | control plane vs partitioned store | split; only control/online uses quorum |
| `sn_directory` owns control/online/route | business | Owner SN directory domain owns all directory routing state | exported directory client/server interfaces | keep inside directory module |
| Serving SN detail vs Owner SN route | business | serving owns NAT/detail; owner owns route only | `PeerManager` vs route store | keep separate |
| Serving online renew vs user `ReportSn` | business | Serving SN 可用性是服务实例状态；用户 report 是 peer detail 状态 | online reporter vs route publish policy | split; report does not force online renew or route publish |
| owner peer transport vs serving-facing listener | technical | owner-to-owner control traffic and Serving SN directory access have different trust boundaries | `TtpNode + DefaultCmdNodeService` vs `TtpServer + DefaultCmdServerService` | split |
| SnServer inter-SN transport vs registry fallback | technical | production/test/compat cross-SN query/call/route must use real transport or explicit fake seam | `TtpInterSnClient` vs implicit `InterSnRegistry` | wire real transport; remove implicit fallback |
| Serving online loop vs route publish loop | business | serving availability and peer route freshness have independent semantics | online reporter vs route publisher | split config, trigger, error state, and observability |
| real process DV vs in-memory command matrix | technical | process lifecycle proves listener/config/transport boundaries that unit topology cannot | OS process harness vs unit fake transport | both are needed; fake transport must be explicit |

## Boundary Rationale
| Boundary | Classification | Why Separate | Shared Logic / Technical Area | Notes |
|----------|----------------|--------------|-------------------------------|-------|
| control/online state vs `PeerRoute` | business | Serving SN online state is small and cluster-critical; peer route is high-cardinality data | quorum state machine vs partitioned route store | avoids turning ownerSN into a global user online database |
| Owner SN runtime vs Serving SN runtime | business | Owner SN owns directory/control semantics; Serving SN owns client detail and call delivery | directory client/server and inter-SN traits | prevents same-process shortcut coupling |
| route validity vs route storage | business | route may exist physically but be semantically hidden by offline servingSN | route store reads committed online state | supports abnormal Serving SN disconnect |
| leader replication vs follower apply | technical | follower-visible state must reflect committed entries only | commit-qualified replication response or apply-after-commit command | prevents pre-quorum visible state |
| serving-facing listener vs owner peer listener | technical | Serving SN access and owner peer control traffic need different command sets and authorization | separate listener purpose and command id space | preserves existing trust boundary |
| no-registry transport vs explicit fake seam | technical | implicit singleton fallback hides missing endpoint and same-process coupling | constructor-injected fake/client trait | fake is allowed only when explicit in test setup |

## Dependency Graph
| Source | Depends On | Reason | Cycle Check |
|--------|------------|--------|-------------|
| `sn_service_integration` | `sn_directory`, `sn_inter_service`, `PeerManager` | serving report/query/call uses directory client and remote detail/relay | acyclic |
| `sn_directory` | `sn_inter_service` interfaces, `NetManager`, `TtpServer` | owner server/client state and serving-facing listener | acyclic |
| `sn_inter_service` | `TtpNode`, command service, validator, directory protocol payload types | owner peer transport and internal command dispatch | acyclic |
| `sn_miner_config` | `sn_directory`, `sn_service_integration` | runtime assembly | acyclic; nothing depends on assembly |

## Key Call Flows
| Flow | Caller | Callee / Submodule Path | Purpose | Failure Handling | Notes |
|------|--------|--------------------------|---------|------------------|-------|
| Owner leader election | OwnerDirectoryServer control task | `OwnerElectionNode` -> owner peer vote command | elect a leader for owner control/online mutation | no quorum leaves state read-only; stale term rejected | runtime-only election |
| Leader heartbeat/failover | leader/follower `OwnerElectionNode` | owner peer heartbeat command | keep followers attached to current leader | heartbeat timeout triggers candidate election; no quorum leaves node without commit rights | heartbeat interval and timeout are config constants |
| Serving SN online renew | Serving SN online reporter | serving-facing listener -> owner leader -> control plane commit | keep Serving SN available | non-leader forwards or redirects; no quorum leaves previous committed state unchanged | independent of user `ReportSn` |
| Serving SN offline | Serving SN online reporter or timeout task | owner leader -> control plane commit | mark Serving SN unavailable | stale/non-member serving id rejected; committed offline hides old routes | abnormal offline comes from timeout |
| Follower online-state forwarding | serving-facing listener on follower | follower validates remote owner -> leader owner peer command | forward online/offline mutation to leader | missing leader returns redirect/error; invalid remote owner rejected with no side effect | follower does not apply before leader commit |
| Leader online-state replication | leader `OwnerElectionNode` | owner peer append/commit command -> follower control plane | replicate committed Serving SN online/offline state | quorum ack commits; followers apply only committed entry; duplicate entry is idempotent by serving id/state/version | replicates control/online only, not PeerRoute |
| Peer route publish | route publish policy | partition owner set -> route store | record peer ownership route | partial owner failure is allowed if write quorum/replica threshold is met; stale sequence ignored | not triggered by every `ReportSn` |
| Peer route query | Serving SN query handler | partition owner set -> route store + committed online state | find servingSN candidates for peer | query fanout ignores failed owners; route with offline servingSN is filtered | returns current empty query semantics on miss |
| Distributed detail merge | Serving SN query handler | real `TtpInterSnClient` detail query to remote servingSN | build existing `SnQueryResp` | per-serving timeout ignored; successful details are merged; missing configured client fails as configuration/runtime error instead of falling back to registry | client API unchanged |
| Relay call | Serving SN call handler | real `TtpInterSnClient` relay | deliver `SnCalled` via remote servingSN | first accepted relay returns Ok; all failures use existing error path | `SnCallResp` remains acceptance/forwarding only |
| Serving online loop | Serving SN runtime | online reporter -> owner serving-facing endpoint | renew Serving SN availability | failures are retried/logged in online loop and do not publish PeerRoute | independent of report and route publish |
| Route publish loop | Serving SN runtime | route publisher -> owner route partition | publish or refresh peer routes | failures are retried/logged in route loop and do not renew Serving SN online | driven by first report/migration/refresh/repair/update |
| Real process owner/serving DV | test runner | independent owner process + independent serving process | prove config/listener/transport/shutdown boundary | timeout cleanup kills children; invalid config and port conflict must exit non-zero | process evidence is required for `sn_real_process_owner_serving_dv` |
| 5x5 command matrix | test harness | 5 owner nodes + 5 serving nodes + 5 user peers | prove all SN command families remain coherent when users attach to different serving SNs | command reject cases use validator/mismatched serving id and must leave no side effects; transport lifecycle gaps are recorded if the harness is in-memory | topology is validation-only and does not change protocol limits |

## Trigger Matrix
| trigger_category | applies | evidence | design_coverage | required_checks | deferred_checks_and_reason |
|------------------|---------|----------|-----------------|-----------------|----------------------------|
| contract/protocol | yes | owner control/online commands, route publish/query, detail/relay internal commands | Key Call Flows, Interfaces, External protocol constraints | unit + integration/DV where available | owner: testing; risk: internal wire regression |
| data/schema | yes | `OwnerControlPlane`, online state, `PeerRoute`, `PeerRouteStore`, route publish metadata | Data and State | unit tests for term/quorum/online/route transitions | owner: testing; risk: stale online or route |
| security/privacy/permission | yes | SN间 connection and owner forward validation remain required | Interfaces and Key Call Flows | validator reject tests and remote owner mismatch tests | owner: testing; risk: unauthorized owner control or route/detail access |
| runtime/integration | yes | multi-service owner/serving/listener flows, SnServer TTP client, and 5 owner/5 serving/5 user command matrix | Key Call Flows, Implementation Order | targeted unit plus DV/integration gap recording if no harness | owner: testing; risk: partial failure |
| build/dependency/config/deployment | yes | sn-miner owner membership/control config and SnServer TTP assembly | External module dependencies | compile/check sn-miner path if touched | owner: testing; risk: config mismatch |
| ui/datamodel/workflow | no | no UI or external app workflow schema | not-applicable | not-applicable | owner: none |
| harness/process | yes | direct submodule packet maps change_ids | Directly Mapped Change Items | doc-structure/schema/stage-scope/admission | owner: design/implementation |

## Directly Mapped Change Items
| change_id | proposal_id | Design Coverage | Scope Paths | Interface / Boundary Impact | Notes |
|-----------|-------------|-----------------|-------------|-----------------------------|-------|
| sn_owner_control_plane_online_state | P-SN-DIST-CONTROL-1 | Owner control plane, Serving SN online/offline, leader/quorum commit, offline filtering, no session/epoch distinction | `p2p-frame/src/sn/directory/**`, `p2p-frame/src/sn/inter_sn/**`, `p2p-frame/src/sn/mod.rs`, `sn-miner-rust/**` | online-state APIs replace session/epoch APIs inside directory domain | first version in-memory state machine |
| sn_owner_network_leader_election | P-SN-DIST-CONTROL-2 | Owner-to-owner vote request/response, candidate/follower/leader roles, leader heartbeat, heartbeat timeout, failover election, follower redirect/forward | `p2p-frame/src/sn/directory/**`, `p2p-frame/src/sn/inter_sn/**`, `p2p-frame/src/sn/mod.rs`, `sn-miner-rust/**` | owner peer/control command boundary carries election messages | runtime-only election; no persistent WAL/snapshot |
| sn_owner_leader_online_replication | P-SN-DIST-CONTROL-3 | leader-only Serving SN online/offline mutation, follower forwarding, quorum replication, committed-only follower apply, offline propagation | `p2p-frame/src/sn/directory/**`, `p2p-frame/src/sn/inter_sn/**`, `p2p-frame/src/sn/mod.rs`, `sn-miner-rust/**` | online state replication across ownerSN control plane | does not replicate user-level `PeerRoute` |
| sn_partitioned_peer_route | P-SN-DIST-ROUTE-1 | Fixed partition route owner set, route publish/query, online-state filter, route miss behavior, route publish cadence independent from `ReportSn` | `p2p-frame/src/sn/directory/**`, `p2p-frame/src/sn/inter_sn/**`, `p2p-frame/src/sn/service/service.rs`, `p2p-frame/src/sn/mod.rs` | replaces per-report lease refresh as primary route model | no membership migration first version |
| sn_owner_serving_role_boundary | P-SN-DIST-ROLE-1 | Owner role vs Serving role runtime boundary and no same-process production shortcut | `p2p-frame/src/sn/directory/**`, `p2p-frame/src/sn/inter_sn/**`, `p2p-frame/src/sn/service/service.rs`, `p2p-frame/src/sn/mod.rs`, `sn-miner-rust/**` | preserves serving-only `SnService` | no owner state in `SnService` |
| sn_directory_client_server_boundary | P-SN-DIST-DIRECTORY-SERVER-1 | directory client/server split, Serving SN uses client interface, OwnerDirectoryServer owns server state | `p2p-frame/src/sn/directory/**`, `p2p-frame/src/sn/service/service.rs`, `p2p-frame/src/sn/mod.rs`, `sn-miner-rust/**` | serving talks through directory client/server boundary | no `sn::owner` module |
| sn_owner_directory_peer_transport | P-SN-DIST-OWNER-PEER-TRANSPORT-1 | owner peer/control listener, command transport, connection recovery, remote owner identity validation for forward | `p2p-frame/src/sn/directory/**`, `p2p-frame/src/sn/inter_sn/**`, `p2p-frame/src/ttp/node.rs`, `p2p-frame/src/sn/mod.rs`, `sn-miner-rust/**` | owner control commands separated from serving-facing commands | uses existing `TtpNode` path |
| sn_owner_serving_listener_transport | P-SN-DIST-OWNER-SERVING-LISTENER-1 | serving-facing listener and command server for online/route publish/query | `p2p-frame/src/sn/directory/**`, `p2p-frame/src/sn/inter_sn/**`, `p2p-frame/src/sn/protocol/**`, `p2p-frame/src/sn/service/service.rs`, `p2p-frame/src/sn/mod.rs`, `sn-miner-rust/**` | Serving SN publishes/query online and route via serving endpoint | command id space isolated |
| sn_distributed_query_merge | P-SN-DIST-QUERY-1 | route query + remote detail merge through SnServer/inter-SN client | `p2p-frame/src/sn/service/service.rs`, `p2p-frame/src/sn/inter_sn/**`, `p2p-frame/src/sn/directory/**`, `p2p-frame/src/sn/mod.rs` | backward-compatible `SnQueryResp` | client does not see multiple serving SN |
| sn_distributed_relay_call | P-SN-DIST-CALL-1 | route query + remote relay call through SnServer/inter-SN client | `p2p-frame/src/sn/service/service.rs`, `p2p-frame/src/sn/inter_sn/**`, `p2p-frame/src/sn/protocol/**`, `p2p-frame/src/sn/mod.rs` | existing `SnCallResp` semantics preserved | first accepted remote relay is enough |
| sn_inter_service_validation | P-SN-INTER-AUTH-1 | connection and command authorization, including owner control forward remote owner identity checks | `p2p-frame/src/sn/inter_sn/**`, `p2p-frame/src/sn/directory/**`, `p2p-frame/src/sn/service/service.rs`, `p2p-frame/src/sn/mod.rs` | validator remains replaceable | reject has no side effect |
| sn_five_by_five_command_matrix | P-SN-DIST-CMD-MATRIX-1 | validation topology with 5 ownerSN members, 5 servingSN services, 5 user peers each bound to a distinct servingSN; command matrix covers peer-facing report/query/call/called, inter-SN heartbeat/publish/query/detail/relay, and owner serving-facing publish/query dispatch plus reject paths | `p2p-frame/src/sn/**` | test harness uses existing service, directory, and inter-SN seams without adding client-visible commands | real multi-process TTP lifecycle may remain a DV/integration gap if the runnable unit harness uses in-memory peers |
| sn_directory_no_registry_fallback | P-SN-DIST-NO-FALLBACK-1 | production/test/compat directory communication uses real transport or explicit fake seam; global registry fallback and same-process owner lookup are removed | `p2p-frame/src/sn/directory/**`, `p2p-frame/src/sn/inter_sn/**`, `p2p-frame/src/sn/service/**`, `p2p-frame/src/sn/mod.rs` | missing configured transport is an error instead of silent fallback | fake transport must be constructor-injected |
| sn_serving_online_route_independent_loops | P-SN-DIST-ONLINE-ROUTE-LOOPS-1 | Serving SN online heartbeat loop and PeerRoute publish loop have separate config, trigger, lifecycle, retry/error handling and observations | `p2p-frame/src/sn/service/**`, `p2p-frame/src/sn/directory/**`, `p2p-frame/src/sn/mod.rs`, `sn-miner-rust/**` | Serving SN availability and peer route freshness remain semantically separate | shared client allowed; shared trigger/failure state not allowed |
| sn_real_process_owner_serving_dv | P-SN-DIST-REAL-PROCESS-1 | process-level seams expose owner listener config, serving endpoint config, shutdown, invalid config and port conflict behavior | `p2p-frame/src/sn/**`, `sn-miner-rust/**` | testing owns process harness; implementation exposes stable startup/config behavior | no unit-only substitute for final evidence |

## Implementation Order
| Phase | Goal | Preconditions | Outputs | Depends On | Parallel |
|-------|------|---------------|---------|------------|----------|
| 1 | Replace Serving SN session/epoch with online/offline state | approved proposal/design | `OwnerControlPlane` online state, online TTL, offline filtering | none | no |
| 2 | Adjust owner election replication to committed-only follower apply | Phase 1 | online-state entry replication and follower apply only after leader quorum commit | Phase 1 | no |
| 3 | Add or rename directory client/server online/route APIs | Phase 1-2 | serving-facing online renew/offline and route publish/query commands | Phase 2 | no |
| 4 | Decouple `ReportSn` from route publish | Phase 3 | route publish policy separate from report handler; report updates local detail only unless policy says route refresh is due | Phase 3 | no |
| 5 | Wire real SnServer inter-SN client | Phase 3 | `SnServer::new` constructs/passes `TtpInterSnClient`; query/call/detail/relay prefer real transport | Phase 3 | no |
| 6 | Enforce owner control forward identity validation | Phase 2-5 | membership and remote-owner equality checks before forward side effects | Phase 5 | no |
| 7 | Remove global registry fallback | Phase 3-6 | missing configured transport fails explicitly; tests use explicit fake seam | Phase 6 | no |
| 8 | Split Serving SN online and route publish loops | Phase 3-7 | independent config, trigger and error handling | Phase 7 | no |
| 9 | Update sn-miner config and compatibility defaults | Phase 3-8 | owner serving endpoints, owner peer endpoints, no-config single SN behavior | Phase 8 | yes |
| 10 | Add real process and 5x5 test support seams | Phase 1-9 | stable process startup/shutdown/config behavior plus unit topology hooks | Phase 9 | yes |

## Key Decisions
| Decision | Chosen | Alternatives Considered | Rejection Reason |
|----------|--------|-------------------------|------------------|
| Serving SN availability | committed online/offline state | session/epoch per Serving SN generation | proposal explicitly removed session/epoch requirement and only needs online judgment |
| Route refresh trigger | independent route publish policy | publish route on every `ReportSn` | per-report publish couples owner load to online peer count and contradicts approved proposal |
| Follower replication apply | apply only committed online-state entries | apply on replication receive before leader quorum | pre-quorum apply exposes uncommitted state and contradicts leader/quorum design |
| Network election | owner-to-owner runtime vote + heartbeat + failover | static leader only or local-only state machine | static/local-only does not satisfy leader失效重新选举 or follower-to-leader communication requirements |
| Route replication | fixed partition owner set | broadcast every route to every ownerSN | broadcast scales with peer count and violates proposal non-goal |
| SnServer transport | real `TtpInterSnClient` with no global registry fallback | registry-only same-process shortcut; registry fallback for tests/compat | registry hides missing transport config and violates approved no-fallback requirement |
| Owner forward auth | validate remote owner member and identity match | trust envelope payload only | payload-only trust allows unauthorized owner control forward |
| 5x5 matrix level | deterministic unit-level topology plus DV/integration gap when no process harness exists | require immediate real multi-process harness | the current repository records no automated multi-process SN DV harness, so unit command coverage moves the requirement forward without inventing unavailable infrastructure |
| Online vs route runtime loops | separate online heartbeat and route publish loops | one loop triggered by every report | single loop couples owner load and availability semantics to user report frequency |
| Real process evidence | independent owner and serving `sn-miner` processes | in-memory fake-only validation | fake-only validation cannot prove config files, listener binding, process shutdown or missing transport failures |

## Data and State
| Data or State | Owner Submodule | Access For Others | State Transitions |
|---------------|-----------------|-------------------|-------------------|
| Owner control term/leader | `sn_directory` | owner peer command handlers mutate through control plane | follower -> candidate -> leader -> follower; stale term rejected; no quorum leaves old leader state unchanged |
| Owner election vote | `sn_directory` | owner peer vote request/response only | no vote -> voted_for candidate in term -> reject conflicting vote; higher term updates local term |
| Leader heartbeat deadline | `sn_directory` | owner election node updates from heartbeat | follower with fresh heartbeat -> follower; expired heartbeat -> candidate election; elected candidate -> leader |
| Owner committed control log | `sn_directory` | read-only snapshots for directory server/client | empty -> replicated pending -> quorum committed -> applied; pending entries have no visible effect |
| Serving SN online state | `sn_directory` | route query reads committed online state by serving id | absent -> online -> renewed online -> offline/expired -> gc; duplicate renew is idempotent |
| `PeerRoute` | `sn_directory` | Serving SN publishes through client; query reads through route owner set | absent -> present -> refreshed -> shadowed by newer seq -> hidden by offline servingSN -> gc |
| `ServingPeerDetail` | existing `PeerManager` | inter-SN detail query only | absent -> reported -> refreshed -> removed by existing lifecycle |
| SN inter-service authorization | `sn_inter_service` | all owner/serving internal commands consult validator | pending -> accepted/rejected; rejected has no side effect |
| Serving online loop state | `sn_service_integration` | p2p-frame runtime owns retries and observations | stopped -> starting -> renewing -> retrying/error -> stopped/offline |
| Route publish loop state | `sn_service_integration` | route publisher owns queued/refresh events | idle -> pending publish -> published/retry/error -> refresh due |

## Testability
- `OwnerControlPlane` can be unit-tested without network for election, term rejection, quorum commit, online renew/offline/expiry, and no session/epoch filtering.
- `OwnerElectionNode` can be unit-tested with fake owner peer clients for vote quorum, stale term rejection, heartbeat timeout, leader failover, follower forwarding, committed-only apply, and leader online-state replication ack behavior.
- `PeerRouteStore` can be unit-tested with deterministic owner lists, route sequence, online/offline state, and timestamps.
- `OwnerDirectoryClient` can be tested with fake owner endpoints to ensure fanout, partial failure behavior, and no per-report publish.
- `SnService` query/call can use fake directory/inter-SN clients to verify client-visible compatibility and absence of registry fallback.
- Static searches and tests must prove `InterSnRegistry` or equivalent singleton lookup is not used as an implicit fallback for Owner SN / Serving SN directory communication.
- Online loop tests can force owner endpoint failure and verify route publish state is unchanged.
- Route publish tests can force route publish failure and verify Serving SN online state is not renewed or cleared by that failure.
- Real process DV/integration uses `sn-miner` configs to start separate owner and serving processes; p2p-frame owns the transport semantics being exercised.
- Validator reject paths can use the existing replaceable `SnInterServiceValidator`, plus explicit remote owner mismatch tests for control forward.
- `sn_five_by_five_command_matrix` should use a deterministic unit-level topology when real multi-process lifecycle is unavailable: 5 owner ids feed `OwnerMembership`, 5 serving services publish route/detail state, 5 user peers bind one-to-one to serving services, and direct command dispatch helpers assert every command family and response path.
- Real multi-process owner/serving transport is a DV/integration concern; if no harness exists, testing must record the gap while covering state machine and command adapters.

## Interfaces and Dependencies
### Exported interfaces
| Interface | Consumer | Compatibility | Notes |
|-----------|----------|---------------|-------|
| `OwnerControlPlane` | `sn_owner_control_plane_online_state`, `OwnerDirectoryServer` | migration-required | committed owner control/online state machine replaces session/epoch methods |
| `OwnerElectionNode` | `sn_owner_network_leader_election`, `sn_owner_leader_online_replication`, `OwnerDirectoryServer` | migration-required | runtime owner-to-owner election, leader heartbeat, follower forwarding, and online replication coordinator |
| `OwnerPeerControlClient` | `OwnerElectionNode`, owner peer transport adapter | migration-required | trait boundary for vote/heartbeat/replicate/forward commands, testable with fake peers |
| `OwnerDirectoryServer` / owner config builder | `sn_directory_client_server_boundary`, `sn-miner-rust` | backward-compatible | owns owner directory runtime; constructor changes stay internal/default-compatible |
| `OwnerDirectoryClient` | `sn_service_integration`, `sn_partitioned_peer_route` | migration-required | internal serving-side callers migrate from lease-first API to online/route APIs |
| `TtpInterSnClient` wiring in `SnServer` | `sn_distributed_query_merge`, `sn_distributed_relay_call`, `sn_partitioned_peer_route` | backward-compatible | no-config single SN remains valid; configured multi-SN uses real transport |
| `InterSnClient` internal commands | `sn_owner_directory_peer_transport`, `sn_distributed_query_merge`, `sn_distributed_relay_call` | migration-required | internal command callers gain online/route operations and owner forward validation |
| `SnServiceConfig` owner directory configuration | `sn_miner_config`, deployments | backward-compatible | no-config single SN remains valid through local-only default |
| existing `SNClientService::query()` | existing callers | backward-compatible | response shape remains `SnQueryResp` |
| existing `SNClientService::call()` | existing callers | backward-compatible | response semantics unchanged |
| 5x5 command matrix test topology | `sn_five_by_five_command_matrix` | new | test-only topology builder or inline test code constructs fixed owner/serving/user sets and drives existing command interfaces |

### External module dependencies
- `sn-miner-rust` consumes owner membership/control and serving-facing endpoint config.
- `cyfs-p2p` may expose configuration but must not redefine SN directory semantics.

### External service or protocol constraints
- owner peer/control command ids, serving-facing owner directory command ids, and peer-facing SN command ids must remain disjoint.
- owner-to-owner control traffic uses owner peer transport; Serving SN directory traffic uses serving-facing listener.
- Existing peer-facing SN command codes and response structures remain compatible.

## Document Index
| Document | Topic | Scope |
|----------|-------|-------|
| `proposal.md` | SN 分布式目录需求和边界 | direct submodule proposal |
| `design.md` | SN 分布式目录 design | direct submodule design |

## Risks and Rollback
- Risk: runtime-only network election loses term/vote state after restart. Mitigation: first version explicitly scopes persistence out, restarts rejoin through membership and heartbeat/election.
- Risk: split brain during network partition. Mitigation: no quorum means no new online-state commit; higher term heartbeat/vote response steps down stale leaders.
- Risk: no quorum makes online/offline updates unavailable. Mitigation: keep old committed state visible and surface write failure.
- Risk: online TTL too long delays disconnect visibility. Mitigation: design exposes TTL and tests expiry.
- Risk: Serving SN restart without session/epoch may briefly expose old routes after online. Mitigation: detail query failure is ignored, route refresh/repair republishes current routes, and GC removes stale routes.
- Risk: route partition mismatch after owner list change. Mitigation: first version documents bounded membership change and allows route miss/re-report.
- Risk: unauthorized owner forward or route/detail access. Mitigation: validator remains connection- and command-level, and owner control forward validates remote owner identity.
- Rollback: disable owner membership/control config to return to local-only serving behavior.

## Design Guardrails
- Do not rewrite approved proposal intent in `design.md`.
- Keep proposal-level requirement changes out of design.
- Keep implementation code and testing artifacts out of design-stage work.
- Every implementation-ready design item must carry a proposal `change_id`.

## Approval Record
- approver:
- approval_date:
- user_statement: ""
