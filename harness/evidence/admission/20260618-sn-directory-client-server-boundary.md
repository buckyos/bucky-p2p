# Implementation Admission Evidence: SN Directory Client/Server Boundary

## Implementation Admission Evidence
| evidence_item | source | status | notes |
|---------------|--------|--------|-------|
| proposal_read | `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/proposal.md` | pass | Read approved proposal covering `sn_distributed_directory`, `sn_owner_serving_role_boundary`, `sn_directory_client_server_boundary`, `sn_distributed_query_merge`, `sn_distributed_relay_call`, and `sn_inter_service_validation`. |
| design_read | `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/design.md` | pass | Read approved design and direct mapping table for all admitted change_id values. |
| change_scope_matches_request | user request and approved packet | pass | User clarified that owner and directory are the same domain; admitted scope keeps `SnService` serving-only and places ownerSN client/server behavior inside `sn/directory`. |
| active_module_resolved | packet front matter and path | pass | Active version is `v0.1`, module is `p2p-frame`, direct submodule is `sn-distributed-directory`. |
| no_chat_only_evidence | proposal/design mapping tables | pass | Implementation admission relies on approved versioned proposal/design rows and not on chat-only requirements. |

## Document Binding
| doc | sha256 |
|-----|--------|
| docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/proposal.md | fceaffe056acabc64ba2c1a3faad3bf1044f0e4a5cc32b20539971021cce90d4 |
| docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/design.md | dc8d2a408cc3d41468d49860bacd66903d971a5db44274cb5194b4a97f3f41f9 |

## Coverage Quotes

### Quote: proposal.md sn_directory_client_server_boundary
> | P-SN-DIST-DIRECTORY-SERVER-1 | sn_directory_client_server_boundary | Owner SN 功能从 `SnService` 中拆出，作为 `sn/directory` 模块内部 server 侧能力承载；Serving SN 只通过 directory client 接口交互。 | `p2p-frame/src/sn/directory/**`、`p2p-frame/src/sn/service/**`、`p2p-frame/src/sn/mod.rs`、必要装配路径。 | `SnService` 不包含 owner membership/resolver/store/owner command handler；directory server 可独立验证 lease publish/query；Serving SN 只通过 directory client/inter-SN 接口交互。 | 不在 `SnService` 内保留 ownerSN 状态字段或 ownerSN command 分发；不再暴露独立 `sn::owner` 模块。 |

### Quote: design.md sn_directory_client_server_boundary
> | sn_directory_client_server_boundary | P-SN-DIST-DIRECTORY-SERVER-1 | `sn_directory` client/server split, serving-only `SnService`, OwnerDirectoryClient interface | `p2p-frame/src/sn/directory/**`, `p2p-frame/src/sn/service/service.rs`, `p2p-frame/src/sn/mod.rs`, `sn-miner-rust/**` | moves owner membership/resolver/store/command handler out of `SnService` while keeping ownerSN in directory domain | `SnService` remains servingSN only; no `sn::owner` module |

### Quote: proposal.md sn_owner_serving_role_boundary
> | P-SN-DIST-ROLE-1 | sn_owner_serving_role_boundary | Owner SN 与 Serving SN 作为独立运行角色和业务边界建模，生产逻辑不假设两者同进程或共享 peer-facing handler。 | `p2p-frame/src/sn/directory/**`、`p2p-frame/src/sn/inter_sn/**`、`p2p-frame/src/sn/service/**` 中角色接口与装配边界。 | Owner 目录能力可独立于 serving detail/call 逻辑验证；Serving SN 通过接口发布/query lease 和拉取/relay detail，不直接访问 owner 内部 store。 | 不为了单 SN 兼容把 owner/serving 逻辑重新混成同一个业务路径。 |

### Quote: design.md sn_owner_serving_role_boundary
> | sn_owner_serving_role_boundary | P-SN-DIST-ROLE-1 | Owner role vs Serving role runtime boundary, Owner lease publish/query flow, role-owned Data and State | `p2p-frame/src/sn/directory/**`, `p2p-frame/src/sn/inter_sn/**`, `p2p-frame/src/sn/service/service.rs`, `p2p-frame/src/sn/mod.rs`, `sn-miner-rust/**` | separates owner lease authority from serving peer-facing detail/call authority | local single-process fallback must use the same role interface |

### Quote: proposal.md sn_distributed_directory
> | P-SN-DIST-DIR-1 | sn_distributed_directory | 引入 OwnerMembership + HRW + `ServingLease` owner 目录，支持同一 peer 多 serving SN fresh lease。 | `p2p-frame/src/sn/directory/**`、必要 `sn/service` 接入、`sn-miner` 配置装配。 | Owner store 可保存多条 `(peer_id, serving_sn_id)`；旧 sequence 不覆盖新 sequence；TTL 过期不返回。 | 不做强一致 session 数据库；不让 owner 保存 NAT detail。 |

### Quote: design.md sn_distributed_directory
> | sn_distributed_directory | P-SN-DIST-DIR-1 | `ServingLease`, Owner Resolver, Owner Directory Store, Data and State | `p2p-frame/src/sn/directory/**`, `p2p-frame/src/sn/mod.rs`, `p2p-frame/src/sn/service/service.rs`, `sn-miner-rust/**` | new internal directory and config boundary | owner does not store NAT detail |

### Quote: proposal.md sn_distributed_query_merge
> | P-SN-DIST-QUERY-1 | sn_distributed_query_merge | `SnQuery` 本地 miss 或多 SN 命中时，由 serving SN 拉取 remote detail 并合并 endpoint 后返回现有 `SnQueryResp`。 | `p2p-frame/src/sn/service/**`、`p2p-frame/src/sn/inter_sn/**`、现有 `SnQueryResp` 构造路径。 | 客户端 API 不变；多个 serving SN detail endpoint 去重合并；无可用 detail 时返回当前空 query 语义。 | 不新增客户端可见 `SnQueryServing` 或多 serving SN 选择接口。 |

### Quote: design.md sn_distributed_query_merge
> | sn_distributed_query_merge | P-SN-DIST-QUERY-1 | Query merge detail flow and `ServingPeerDetail` | `p2p-frame/src/sn/service/service.rs`, `p2p-frame/src/sn/inter_sn/**`, `p2p-frame/src/sn/directory/**`, `p2p-frame/src/sn/mod.rs` | backward-compatible client-facing `SnQueryResp` | client does not see multiple serving SN |

### Quote: proposal.md sn_distributed_relay_call
> | P-SN-DIST-CALL-1 | sn_distributed_relay_call | `SnCall` 本地 miss 时通过 owner set 找到 remote serving SN 并 relay call，由 remote serving SN 投递 `SnCalled`。 | `p2p-frame/src/sn/service/**`、`p2p-frame/src/sn/inter_sn/**`。 | 本地 call 路径不变；remote serving SN accepted 后 `SnCallResp` 仍只表示受理/转发。 | 不改变最终 tunnel 连通性语义；第一版不要求 fanout 到所有 serving SN。 |

### Quote: design.md sn_distributed_relay_call
> | sn_distributed_relay_call | P-SN-DIST-CALL-1 | Relay call flow and `SnRelayCall` | `p2p-frame/src/sn/service/service.rs`, `p2p-frame/src/sn/inter_sn/**`, `p2p-frame/src/sn/protocol/**`, `p2p-frame/src/sn/mod.rs` | new internal relay command; existing `SnCallResp` semantics preserved | first accepted remote relay is enough |

### Quote: proposal.md sn_inter_service_validation
> | P-SN-INTER-AUTH-1 | sn_inter_service_validation | 新增 SN 间连接建立准入和命令级 publish/query/detail/relay 授权接口。 | `p2p-frame/src/sn/inter_sn/validator.rs`、`SnServiceConfig` 装配路径。 | validator reject 时不建立 SN 间内部协议连接或不产生 owner write/detail/relay side effect。 | 不固化使用方权限模型；默认实现可 allow-all。 |

### Quote: design.md sn_inter_service_validation
> | sn_inter_service_validation | P-SN-INTER-AUTH-1 | SN 间连接准入 and command authorization | `p2p-frame/src/sn/inter_sn/**`, `p2p-frame/src/sn/service/service.rs`, `p2p-frame/src/sn/mod.rs` | new validator configuration interface | default may allow-all; custom reject must short-circuit |
