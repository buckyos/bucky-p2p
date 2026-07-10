# Implementation Admission Evidence: Owner SN Startup and No-Bypass Fix

## Implementation Admission Evidence
| evidence_item | source | status | notes |
|---|---|---|---|
| proposal_read | docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/proposal.md | pass | Read approved proposal coverage for ownerSN network election, leader-only session mutation, owner peer transport, serving-facing listener, and directory client/server role boundaries. |
| design_read | docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/design.md | pass | Read approved design coverage for OwnerDirectoryServer control task, owner peer transport, follower forwarding, leader replication, and serving-facing directory command boundary. |
| change_scope_matches_request | user request, proposal items, design mapping | pass | User requested: "问题1，需要启动 / 问题2，不能绕过 / 问题3，不上多进程DV测试". The implementation scope is limited to starting ownerSN control behavior and removing local bypass paths; no multi-process DV is added in this implementation task. |
| active_module_resolved | docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/{proposal.md,design.md} | pass | Active packet is version `v0.1`, module `p2p-frame`, submodule `sn-distributed-directory`. |
| no_chat_only_evidence | proposal/design direct rows quoted below | pass | Chat clarified which existing documented gaps to fix, but implementation admission relies on the approved proposal/design rows quoted below. |

## Document Binding
| doc | sha256 |
|---|---|
| docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/proposal.md | e42ea0b6903c8a3b6d1948a590c0a9a2e2f8d56d2a84692b7374f6967b5d07d7 |
| docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/design.md | 66fa322b9e2887aa0dee0409279a63bbbb9878cec60364bc8a794beae0f39f72 |

## Coverage Quotes
### Quote: proposal.md sn_owner_network_leader_election
> | P-SN-DIST-CONTROL-2 | sn_owner_network_leader_election | Owner SN 之间通过网络投票选举 leader；leader 正常时 follower 与 leader 通信，leader 失效后 quorum 内重新选主。 | `p2p-frame/src/sn/directory/**`、`p2p-frame/src/sn/inter_sn/**`、`p2p-frame/src/sn/mod.rs`、必要 `sn-miner` 配置装配。 | 多 ownerSN 进程或等价 DV 中可观察到投票选主；停止 leader 后可重新选出 leader；follower 不直接提交 session 变更。 | 不实现完整 etcd/Raft 持久化、snapshot、joint consensus 或 linearizable read。 |

### Quote: design.md sn_owner_network_leader_election
> | sn_owner_network_leader_election | P-SN-DIST-CONTROL-2 | Owner-to-owner vote request/response, candidate/follower/leader roles, leader heartbeat, heartbeat timeout, failover election, follower redirect/forward | `p2p-frame/src/sn/directory/**`, `p2p-frame/src/sn/inter_sn/**`, `p2p-frame/src/sn/mod.rs`, `sn-miner-rust/**` | owner peer/control command boundary gains election messages | runtime-only election; no persistent WAL/snapshot |

### Quote: proposal.md sn_owner_leader_session_replication
> | P-SN-DIST-CONTROL-3 | sn_owner_leader_session_replication | Serving SN online/offline/renew/revoke 事件由 leader 提交并同步给其它 ownerSN，Serving SN 异常离线事件也由 leader 复制到集群其它机器。 | `p2p-frame/src/sn/directory/**`、`p2p-frame/src/sn/inter_sn/**`、必要 owner directory serving-facing listener。 | Serving SN 状态上报到 follower 时被转发或 redirect 到 leader；leader commit 后其它 ownerSN 查询同一 Serving SN session/epoch 状态一致；leader 失效重选后可继续处理后续状态事件。 | 不复制用户级 `PeerRoute` 到所有 ownerSN；只复制 control/session 小状态。 |

### Quote: design.md sn_owner_leader_session_replication
> | sn_owner_leader_session_replication | P-SN-DIST-CONTROL-3 | leader-only Serving SN session mutation, follower forwarding, quorum replication, committed session apply on followers, offline/revoke propagation | `p2p-frame/src/sn/directory/**`, `p2p-frame/src/sn/inter_sn/**`, `p2p-frame/src/sn/mod.rs`, `sn-miner-rust/**` | session state replication across ownerSN control plane | does not replicate user-level `PeerRoute` |

### Quote: proposal.md sn_owner_directory_peer_transport
> | P-SN-DIST-OWNER-PEER-TRANSPORT-1 | sn_owner_directory_peer_transport | OwnerDirectoryServer 提供独立 listener 和 owner peer/control transport，承载 owner control plane 与 owner directory 内部命令。 | `p2p-frame/src/sn/directory/**`、必要 peer transport 和命令节点接入、`sn-miner-rust` owner directory 装配。 | OwnerDirectoryServer 不依赖 SnServer listener 启动即可监听 owner 端口；owner/control 命令与 serving-facing 命令隔离。 | 不把 owner peer/control transport 和普通客户端 SN 协议混用。 |

### Quote: design.md sn_owner_directory_peer_transport
> | sn_owner_directory_peer_transport | P-SN-DIST-OWNER-PEER-TRANSPORT-1 | owner peer/control listener and command transport | `p2p-frame/src/sn/directory/**`, `p2p-frame/src/sn/inter_sn/**`, `p2p-frame/src/ttp/node.rs`, `p2p-frame/src/sn/mod.rs`, `sn-miner-rust/**` | owner control commands separated from serving-facing commands | uses existing `TtpNode` path |

### Quote: proposal.md sn_owner_serving_listener_transport
> | P-SN-DIST-OWNER-SERVING-LISTENER-1 | sn_owner_serving_listener_transport | OwnerDirectoryServer 提供独立 serving-facing listener，供 Serving SN 发布/query session 和 peer route；该入口与 owner peer listener 端口、命令空间和 handler 集合隔离。 | `p2p-frame/src/sn/directory/**`、`p2p-frame/src/sn/inter_sn/**`、必要 serving directory command 接入、`sn-miner-rust` owner directory 装配。 | OwnerDirectoryServer 启动后同时拥有 owner peer/control listener 和 serving-facing listener；Serving SN 能通过 serving-facing listener 访问 owner directory；两类命令互不复用。 | 不通过 SnServer listener、owner peer listener 或同进程 shortcut 处理 Serving SN 到 owner directory 的 publish/query。 |

### Quote: design.md sn_owner_serving_listener_transport
> | sn_owner_serving_listener_transport | P-SN-DIST-OWNER-SERVING-LISTENER-1 | serving-facing listener and command server | `p2p-frame/src/sn/directory/**`, `p2p-frame/src/sn/inter_sn/**`, `p2p-frame/src/sn/protocol/**`, `p2p-frame/src/sn/service/service.rs`, `p2p-frame/src/sn/mod.rs`, `sn-miner-rust/**` | Serving SN publishes/query session and route via serving endpoint | command id space isolated |

### Quote: proposal.md sn_directory_client_server_boundary
> | P-SN-DIST-DIRECTORY-SERVER-1 | sn_directory_client_server_boundary | Owner SN 功能从 `SnService` 中拆出，作为 `sn/directory` 模块内部 server 侧能力承载；Serving SN 只通过 directory client 接口交互。 | `p2p-frame/src/sn/directory/**`、`p2p-frame/src/sn/service/**`、`p2p-frame/src/sn/mod.rs`、必要装配路径。 | `SnService` 不包含 owner membership/control/route store/owner command handler；directory server 可独立验证 route publish/query；Serving SN 只通过 directory client/inter-SN 接口交互。 | 不在 `SnService` 内保留 ownerSN 状态字段或 ownerSN command 分发；不再暴露独立 `sn::owner` 模块。 |

### Quote: design.md sn_directory_client_server_boundary
> | sn_directory_client_server_boundary | P-SN-DIST-DIRECTORY-SERVER-1 | directory client/server split | `p2p-frame/src/sn/directory/**`, `p2p-frame/src/sn/service/service.rs`, `p2p-frame/src/sn/mod.rs`, `sn-miner-rust/**` | serving talks through directory client | no `sn::owner` module |
