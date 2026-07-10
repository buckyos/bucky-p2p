# Implementation Admission: SN Distributed Directory Online Refresh

## Implementation Admission Evidence
| evidence_item | source | status | notes |
|---------------|--------|--------|-------|
| proposal_read | `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/proposal.md` | pass | Approved proposal read; it defines Serving SN online/offline state, route publish decoupling, owner leader election, online replication, role boundary, transport, query/call, and authorization requirements. |
| design_read | `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/design.md` | pass | Approved design read; it maps the requested changes to concrete Scope Paths and implementation phases. |
| change_scope_matches_request | user request and approved proposal/design | pass | The task matches the user's seven requested fixes: consistency fixes, design-consistent leader replication, online-only Serving SN state, decoupled ReportSn route publish, SnServer wiring, remote owner validation, and doc consistency. |
| active_module_resolved | `module: p2p-frame`, `submodule: sn-distributed-directory` | pass | Direct submodule packet is `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/`. |
| no_chat_only_evidence | approved proposal/design rows quoted below | pass | Implementation admission uses approved docs, not chat-only requirements. |

## Document Binding
| doc | sha256 |
|-----|--------|
| docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/proposal.md | 98282d1fa2a0b6f929a533f7bf3dd44d9d4fbd67cfde8416770aa7147e40cbe7 |
| docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/design.md | 3514569862c3745086e68a27714688bb764d59b3c8c180533e85dc6ce85df5c8 |

## Coverage Quotes
### Quote: proposal.md sn_owner_control_plane_online_state
> | P-SN-DIST-CONTROL-1 | sn_owner_control_plane_online_state | Owner SN 集群同步 owner/control/online 小状态，并用 Serving SN online/offline 批量判定 Serving SN 可用性。 | `p2p-frame/src/sn/directory/**`、`p2p-frame/src/sn/inter_sn/**`、必要 `sn-miner` 配置装配。 | Serving SN 正常 online renew 时可继续返回其 route；主动 offline 或异常 offline 超时后，其旧 route 不再被 query 返回。 | 不同步用户级 `PeerRoute` 到所有 ownerSN；不区分 Serving SN session/epoch；不在 proposal 固定日志和字段结构。 |

### Quote: design.md sn_owner_control_plane_online_state
> | sn_owner_control_plane_online_state | P-SN-DIST-CONTROL-1 | Owner control plane, Serving SN online/offline, leader/quorum commit, offline filtering, no session/epoch distinction | `p2p-frame/src/sn/directory/**`, `p2p-frame/src/sn/inter_sn/**`, `p2p-frame/src/sn/mod.rs`, `sn-miner-rust/**` | online-state APIs replace session/epoch APIs inside directory domain | first version in-memory state machine |

### Quote: proposal.md sn_owner_network_leader_election
> | P-SN-DIST-CONTROL-2 | sn_owner_network_leader_election | Owner SN 之间通过网络投票选举 leader；leader 正常时 follower 与 leader 通信，leader 失效后 quorum 内重新选主。 | `p2p-frame/src/sn/directory/**`、`p2p-frame/src/sn/inter_sn/**`、`p2p-frame/src/sn/mod.rs`、必要 `sn-miner` 配置装配。 | 多 ownerSN 进程或等价 DV 中可观察到投票选主；停止 leader 后可重新选出 leader；follower 不直接提交 online/offline 变更。 | 不实现完整 etcd/Raft 持久化、snapshot、joint consensus 或 linearizable read。 |

### Quote: design.md sn_owner_network_leader_election
> | sn_owner_network_leader_election | P-SN-DIST-CONTROL-2 | Owner-to-owner vote request/response, candidate/follower/leader roles, leader heartbeat, heartbeat timeout, failover election, follower redirect/forward | `p2p-frame/src/sn/directory/**`, `p2p-frame/src/sn/inter_sn/**`, `p2p-frame/src/sn/mod.rs`, `sn-miner-rust/**` | owner peer/control command boundary carries election messages | runtime-only election; no persistent WAL/snapshot |

### Quote: proposal.md sn_owner_leader_online_replication
> | P-SN-DIST-CONTROL-3 | sn_owner_leader_online_replication | Serving SN online/renew-online/offline 事件由 leader 提交并同步给其它 ownerSN，Serving SN 异常离线事件也由 leader 复制到集群其它机器。 | `p2p-frame/src/sn/directory/**`、`p2p-frame/src/sn/inter_sn/**`、必要 owner directory serving-facing listener。 | Serving SN 状态上报到 follower 时被转发或 redirect 到 leader；leader commit 后其它 ownerSN 查询同一 Serving SN online/offline 状态一致；leader 失效重选后可继续处理后续状态事件。 | 不复制用户级 `PeerRoute` 到所有 ownerSN；只复制 control/online 小状态。 |

### Quote: design.md sn_owner_leader_online_replication
> | sn_owner_leader_online_replication | P-SN-DIST-CONTROL-3 | leader-only Serving SN online/offline mutation, follower forwarding, quorum replication, committed-only follower apply, offline propagation | `p2p-frame/src/sn/directory/**`, `p2p-frame/src/sn/inter_sn/**`, `p2p-frame/src/sn/mod.rs`, `sn-miner-rust/**` | online state replication across ownerSN control plane | does not replicate user-level `PeerRoute` |

### Quote: proposal.md sn_partitioned_peer_route
> | P-SN-DIST-ROUTE-1 | sn_partitioned_peer_route | 用户级 peer route 由固定分区逻辑管理，Serving SN 根据 ownerSN 列表本地计算 route owner set，并按较长刷新周期、首次上报、迁移或重报修复触发批量写入/query。 | `p2p-frame/src/sn/directory/**`、`p2p-frame/src/sn/inter_sn/**`、必要 `sn/service` 接入。 | route 只写入分区 owner set；query fanout 到分区 owner set；route 返回前必须校验 Serving SN online 状态；每次 `ReportSn` 不强制 publish route。 | 不做 route 全 ownerSN 广播；不把 route publish 作为 Serving SN online renew；第一版不做 membership 变更期迁移。 |

### Quote: design.md sn_partitioned_peer_route
> | sn_partitioned_peer_route | P-SN-DIST-ROUTE-1 | Fixed partition route owner set, route publish/query, online-state filter, route miss behavior, route publish cadence independent from `ReportSn` | `p2p-frame/src/sn/directory/**`, `p2p-frame/src/sn/inter_sn/**`, `p2p-frame/src/sn/service/service.rs`, `p2p-frame/src/sn/mod.rs` | replaces per-report lease refresh as primary route model | no membership migration first version |

### Quote: proposal.md sn_owner_serving_role_boundary
> | P-SN-DIST-ROLE-1 | sn_owner_serving_role_boundary | Owner SN 与 Serving SN 作为独立运行角色和业务边界建模，生产逻辑不假设两者同进程或共享 peer-facing handler。 | `p2p-frame/src/sn/directory/**`、`p2p-frame/src/sn/inter_sn/**`、`p2p-frame/src/sn/service/**` 中角色接口与装配边界。 | Owner 目录能力可独立于 serving detail/call 逻辑验证；Serving SN 通过接口发布/query route 和拉取/relay detail，不直接访问 owner 内部 store。 | 不为了单 SN 兼容把 owner/serving 逻辑重新混成同一个业务路径。 |

### Quote: design.md sn_owner_serving_role_boundary
> | sn_owner_serving_role_boundary | P-SN-DIST-ROLE-1 | Owner role vs Serving role runtime boundary and no same-process production shortcut | `p2p-frame/src/sn/directory/**`, `p2p-frame/src/sn/inter_sn/**`, `p2p-frame/src/sn/service/service.rs`, `p2p-frame/src/sn/mod.rs`, `sn-miner-rust/**` | preserves serving-only `SnService` | no owner state in `SnService` |

### Quote: proposal.md sn_directory_client_server_boundary
> | P-SN-DIST-DIRECTORY-SERVER-1 | sn_directory_client_server_boundary | Owner SN 功能从 `SnService` 中拆出，作为 `sn/directory` 模块内部 server 侧能力承载；Serving SN 只通过 directory client 接口交互。 | `p2p-frame/src/sn/directory/**`、`p2p-frame/src/sn/service/**`、`p2p-frame/src/sn/mod.rs`、必要装配路径。 | `SnService` 不包含 owner membership/control/route store/owner command handler；directory server 可独立验证 route publish/query；Serving SN 只通过 directory client/inter-SN 接口交互。 | 不在 `SnService` 内保留 ownerSN 状态字段或 ownerSN command 分发；不再暴露独立 `sn::owner` 模块。 |

### Quote: design.md sn_directory_client_server_boundary
> | sn_directory_client_server_boundary | P-SN-DIST-DIRECTORY-SERVER-1 | directory client/server split, Serving SN uses client interface, OwnerDirectoryServer owns server state | `p2p-frame/src/sn/directory/**`, `p2p-frame/src/sn/service/service.rs`, `p2p-frame/src/sn/mod.rs`, `sn-miner-rust/**` | serving talks through directory client/server boundary | no `sn::owner` module |

### Quote: proposal.md sn_owner_directory_peer_transport
> | P-SN-DIST-OWNER-PEER-TRANSPORT-1 | sn_owner_directory_peer_transport | OwnerDirectoryServer 提供独立 listener 和 owner peer/control transport，承载 owner control plane 与 owner directory 内部命令。 | `p2p-frame/src/sn/directory/**`、必要 peer transport 和命令节点接入、`sn-miner-rust` owner directory 装配。 | OwnerDirectoryServer 不依赖 SnServer listener 启动即可监听 owner 端口；owner/control 命令与 serving-facing 命令隔离。 | 不把 owner peer/control transport 和普通客户端 SN 协议混用。 |

### Quote: design.md sn_owner_directory_peer_transport
> | sn_owner_directory_peer_transport | P-SN-DIST-OWNER-PEER-TRANSPORT-1 | owner peer/control listener, command transport, connection recovery, remote owner identity validation for forward | `p2p-frame/src/sn/directory/**`, `p2p-frame/src/sn/inter_sn/**`, `p2p-frame/src/ttp/node.rs`, `p2p-frame/src/sn/mod.rs`, `sn-miner-rust/**` | owner control commands separated from serving-facing commands | uses existing `TtpNode` path |

### Quote: proposal.md sn_owner_serving_listener_transport
> | P-SN-DIST-OWNER-SERVING-LISTENER-1 | sn_owner_serving_listener_transport | OwnerDirectoryServer 提供独立 serving-facing listener，供 Serving SN 发布/query online 状态和 peer route；该入口与 owner peer listener 端口、命令空间和 handler 集合隔离。 | `p2p-frame/src/sn/directory/**`、`p2p-frame/src/sn/inter_sn/**`、必要 serving directory command 接入、`sn-miner-rust` owner directory 装配。 | OwnerDirectoryServer 启动后同时拥有 owner peer/control listener 和 serving-facing listener；Serving SN 能通过 serving-facing listener 访问 owner directory；两类命令互不复用。 | 不通过 SnServer listener、owner peer listener 或同进程 shortcut 处理 Serving SN 到 owner directory 的 publish/query。 |

### Quote: design.md sn_owner_serving_listener_transport
> | sn_owner_serving_listener_transport | P-SN-DIST-OWNER-SERVING-LISTENER-1 | serving-facing listener and command server for online/route publish/query | `p2p-frame/src/sn/directory/**`, `p2p-frame/src/sn/inter_sn/**`, `p2p-frame/src/sn/protocol/**`, `p2p-frame/src/sn/service/service.rs`, `p2p-frame/src/sn/mod.rs`, `sn-miner-rust/**` | Serving SN publishes/query online and route via serving endpoint | command id space isolated |

### Quote: proposal.md sn_distributed_query_merge
> | P-SN-DIST-QUERY-1 | sn_distributed_query_merge | `SnQuery` 本地 miss 或多 SN 命中时，由 serving SN 拉取 remote detail 并合并 endpoint 后返回现有 `SnQueryResp`。 | `p2p-frame/src/sn/service/**`、`p2p-frame/src/sn/inter_sn/**`、现有 `SnQueryResp` 构造路径。 | 客户端 API 不变；多个 serving SN detail endpoint 去重合并；无可用 detail 时返回当前空 query 语义。 | 不新增客户端可见 `SnQueryServing` 或多 serving SN 选择接口。 |

### Quote: design.md sn_distributed_query_merge
> | sn_distributed_query_merge | P-SN-DIST-QUERY-1 | route query + remote detail merge through SnServer/inter-SN client | `p2p-frame/src/sn/service/service.rs`, `p2p-frame/src/sn/inter_sn/**`, `p2p-frame/src/sn/directory/**`, `p2p-frame/src/sn/mod.rs` | backward-compatible `SnQueryResp` | client does not see multiple serving SN |

### Quote: proposal.md sn_distributed_relay_call
> | P-SN-DIST-CALL-1 | sn_distributed_relay_call | `SnCall` 本地 miss 时通过分区 route 找到 remote serving SN 并 relay call，由 remote serving SN 投递 `SnCalled`。 | `p2p-frame/src/sn/service/**`、`p2p-frame/src/sn/inter_sn/**`。 | 本地 call 路径不变；remote serving SN accepted 后 `SnCallResp` 仍只表示受理/转发。 | 不改变最终 tunnel 连通性语义；第一版不要求 fanout 到所有 serving SN。 |

### Quote: design.md sn_distributed_relay_call
> | sn_distributed_relay_call | P-SN-DIST-CALL-1 | route query + remote relay call through SnServer/inter-SN client | `p2p-frame/src/sn/service/service.rs`, `p2p-frame/src/sn/inter_sn/**`, `p2p-frame/src/sn/protocol/**`, `p2p-frame/src/sn/mod.rs` | existing `SnCallResp` semantics preserved | first accepted remote relay is enough |

### Quote: proposal.md sn_inter_service_validation
> | P-SN-INTER-AUTH-1 | sn_inter_service_validation | 新增 SN 间连接建立准入和命令级 publish/query/detail/relay 授权接口。 | `p2p-frame/src/sn/inter_sn/validator.rs`、`SnServiceConfig` 装配路径。 | validator reject 时不建立 SN 间内部协议连接或不产生 owner write/detail/relay side effect。 | 不固化使用方权限模型；默认实现可 allow-all。 |

### Quote: design.md sn_inter_service_validation
> | sn_inter_service_validation | P-SN-INTER-AUTH-1 | connection and command authorization, including owner control forward remote owner identity checks | `p2p-frame/src/sn/inter_sn/**`, `p2p-frame/src/sn/directory/**`, `p2p-frame/src/sn/service/service.rs`, `p2p-frame/src/sn/mod.rs` | validator remains replaceable | reject has no side effect |
