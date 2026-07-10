# Implementation Admission Evidence: Owner Directory Synchronous Constructor

## Implementation Admission Evidence
| evidence_item | source | status | notes |
|---|---|---|---|
| proposal_read | `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/proposal.md` | pass | Approved proposal covers OwnerDirectoryServer independent owner peer listener, serving-facing listener, and ownerSN network leader behavior. |
| design_read | `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/design.md` | pass | Approved design covers OwnerDirectoryServer control task, owner peer/control listener, serving-facing listener, and sn-miner config wiring. |
| change_scope_matches_request | user request, proposal items, design mapping | pass | User requested `OwnerDirectoryServer::new` be synchronous and that async operations move into `start`. This is a lifecycle/API shape refinement inside the approved owner directory server and sn-miner assembly boundary. |
| active_module_resolved | packet front matter and path | pass | Active packet is version `v0.1`, module `p2p-frame`, submodule `sn-distributed-directory`. |
| no_chat_only_evidence | proposal/design direct rows | pass | User request selects constructor/start placement, but implementation admission relies on approved proposal/design scope rows quoted below. |

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

### Quote: proposal.md sn_owner_directory_peer_transport
> | P-SN-DIST-OWNER-PEER-TRANSPORT-1 | sn_owner_directory_peer_transport | OwnerDirectoryServer 提供独立 listener 和 owner peer/control transport，承载 owner control plane 与 owner directory 内部命令。 | `p2p-frame/src/sn/directory/**`、必要 peer transport 和命令节点接入、`sn-miner-rust` owner directory 装配。 | OwnerDirectoryServer 不依赖 SnServer listener 启动即可监听 owner 端口；owner/control 命令与 serving-facing 命令隔离。 | 不把 owner peer/control transport 和普通客户端 SN 协议混用。 |

### Quote: design.md sn_owner_directory_peer_transport
> | sn_owner_directory_peer_transport | P-SN-DIST-OWNER-PEER-TRANSPORT-1 | owner peer/control listener and command transport | `p2p-frame/src/sn/directory/**`, `p2p-frame/src/sn/inter_sn/**`, `p2p-frame/src/ttp/node.rs`, `p2p-frame/src/sn/mod.rs`, `sn-miner-rust/**` | owner control commands separated from serving-facing commands | uses existing `TtpNode` path |

### Quote: proposal.md sn_owner_serving_listener_transport
> | P-SN-DIST-OWNER-SERVING-LISTENER-1 | sn_owner_serving_listener_transport | OwnerDirectoryServer 提供独立 serving-facing listener，供 Serving SN 发布/query session 和 peer route；该入口与 owner peer listener 端口、命令空间和 handler 集合隔离。 | `p2p-frame/src/sn/directory/**`、`p2p-frame/src/sn/inter_sn/**`、必要 serving directory command 接入、`sn-miner-rust` owner directory 装配。 | OwnerDirectoryServer 启动后同时拥有 owner peer/control listener 和 serving-facing listener；Serving SN 能通过 serving-facing listener 访问 owner directory；两类命令互不复用。 | 不通过 SnServer listener、owner peer listener 或同进程 shortcut 处理 Serving SN 到 owner directory 的 publish/query。 |

### Quote: design.md sn_owner_serving_listener_transport
> | sn_owner_serving_listener_transport | P-SN-DIST-OWNER-SERVING-LISTENER-1 | serving-facing listener and command server | `p2p-frame/src/sn/directory/**`, `p2p-frame/src/sn/inter_sn/**`, `p2p-frame/src/sn/protocol/**`, `p2p-frame/src/sn/service/service.rs`, `p2p-frame/src/sn/mod.rs`, `sn-miner-rust/**` | Serving SN publishes/query session and route via serving endpoint | command id space isolated |
