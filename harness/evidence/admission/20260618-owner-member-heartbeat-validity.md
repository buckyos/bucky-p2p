# Implementation Admission Evidence: OwnerMember Heartbeat Validity

## Implementation Admission Evidence
| evidence_item | source | status | notes |
|---------------|--------|--------|-------|
| proposal_read | `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/proposal.md` | pass | Read approved proposal covering `sn_owner_member_heartbeat_validity`. |
| design_read | `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/design.md` | pass | Read approved design and direct mapping table for `sn_owner_member_heartbeat_validity`. |
| change_scope_matches_request | user request and approved packet | pass | User requested OwnerDirectoryServer mutual heartbeat validity for OwnerMember nodes so only valid owner nodes are returned to underlying Serving SN. |
| active_module_resolved | packet front matter and path | pass | Active version is `v0.1`, module is `p2p-frame`, direct submodule is `sn-distributed-directory`. |
| no_chat_only_evidence | proposal/design mapping tables | pass | Implementation admission relies on approved versioned proposal/design rows and not on chat-only requirements. |

## Document Binding
| doc | sha256 |
|-----|--------|
| docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/proposal.md | 1a1cd5c741e3c99bf987552f9ceb57fc9573ed3240d7dc6bd83acfbfc226a211 |
| docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/design.md | af0e5bc8c73ddf23b7e6b806acb579abbcd3b2927eba83d0ce7060ff9a98cfba |

## Coverage Quotes

### Quote: proposal.md sn_owner_member_heartbeat_validity
> | P-SN-DIST-OWNER-HEARTBEAT-1 | sn_owner_member_heartbeat_validity | OwnerDirectoryServer 基于 OwnerMembership 对 OwnerMember 建立连接/心跳有效性校验，并只把当前有效 owner 节点返回给 Serving SN。 | `p2p-frame/src/sn/directory/**`、`p2p-frame/src/sn/inter_sn/**`、必要 `sn-miner` 配置装配。 | 失效 OwnerMember 超过 TTL 后不再进入 serving SN 的 owner set、publish/query 目标或返回结果；恢复心跳后可重新返回。 | 不用心跳替代 ServingLease TTL；不向普通客户端暴露 owner health API。 |

### Quote: design.md sn_owner_member_heartbeat_validity
> | sn_owner_member_heartbeat_validity | P-SN-DIST-OWNER-HEARTBEAT-1 | OwnerMember health state, heartbeat refresh flow, and HRW owner filtering behavior | `p2p-frame/src/sn/directory/**`, `p2p-frame/src/sn/inter_sn/**`, `sn-miner-rust/**` | filters OwnerMembership targets before serving-side publish/query owner selection | health does not replace ServingLease TTL |
