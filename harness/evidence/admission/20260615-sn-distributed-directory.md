# Implementation Admission Evidence: SN Distributed Directory

## Implementation Admission Evidence
| evidence_item | source | status | notes |
|---------------|--------|--------|-------|
| proposal_read | `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/proposal.md` | pass | Read approved proposal covering `sn_distributed_directory`, `sn_distributed_query_merge`, `sn_distributed_relay_call`, and `sn_inter_service_validation`. |
| design_read | `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/design.md` | pass | Read approved design and direct mapping table for all four admitted change_id values. |
| change_scope_matches_request | user request and approved packet | pass | User requested automatic downstream work for p2p-frame submodule `sn-distributed-directory`; the admitted change_id values are the packet's complete downstream implementation set. |
| active_module_resolved | packet front matter and path | pass | Active version is `v0.1`, module is `p2p-frame`, direct submodule is `sn-distributed-directory`. |
| no_chat_only_evidence | proposal/design mapping tables | pass | Implementation admission relies on approved versioned proposal/design rows and not on chat-only requirements. |

## Document Binding
| doc | sha256 |
|-----|--------|
| docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/proposal.md | f9efb4f925dc9efe37f2df9148c0cac17a3148f783f249481661519f16cc1c40 |
| docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/design.md | 7e6139f854963cf2094e01850b208d35086ae6221bec99fab8d90a580b07967c |

## Coverage Quotes

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
