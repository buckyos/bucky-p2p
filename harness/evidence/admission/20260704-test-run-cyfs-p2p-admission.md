# Implementation Admission: test-run cyfs-p2p Admission Fixture

## Implementation Admission Evidence
| evidence_item | source | status | notes |
|---------------|--------|--------|-------|
| proposal_read | `docs/versions/v0.1/modules/cyfs-p2p/proposal.md` | pass | Read the approved cyfs-p2p proposal row for `endpoint_area_server_reflexive`. |
| design_read | `docs/versions/v0.1/modules/cyfs-p2p/design.md` | pass | Read the approved cyfs-p2p design row for `endpoint_area_server_reflexive`. |
| change_scope_matches_request | proposal `P-ENDPOINT-AREA-ADAPTER` / design `endpoint_area_server_reflexive` | pass | This fixture is used by workspace-harness test-run admission validation to exercise the current evidence-bound admission-check interface. |
| active_module_resolved | `docs/versions/v0.1/modules/cyfs-p2p` | pass | Active module is cyfs-p2p because the admitted change_id belongs to the cyfs-p2p packet. |
| no_chat_only_evidence | approved proposal/design rows quoted below | pass | Admission relies on approved versioned documents and this bound evidence fixture. |

## Document Binding
| doc | sha256 |
|-----|--------|
| docs/versions/v0.1/modules/cyfs-p2p/proposal.md | b66a732179c35239a0d0ba94691a7d752ae633ce460bad06abb613ea09fecb03 |
| docs/versions/v0.1/modules/cyfs-p2p/design.md | 595e1b774f4c26e8fbcafc1e47419bc2b6c2541d1404d7af249ce1f6ee3d49fb |

## Coverage Quotes
### Quote: proposal.md endpoint_area_server_reflexive
> | P-ENDPOINT-AREA-ADAPTER | endpoint_area_server_reflexive | `cyfs-p2p` 适配层消费 `p2p-frame::EndpointArea::ServerReflexive`，并保持到现有 CYFS endpoint area 表示的兼容映射。 | 不在适配层重新定义 endpoint area 语义；不把 `ServerReflexive` 静默升级为 `Wan`；不引入新的 NAT 探测或下游协议语义。 | design/testing 需要覆盖 `stack_builder.rs` 的转换路径；integration 需要确认 workspace 下游仍能编译测试。 |

### Quote: design.md endpoint_area_server_reflexive
> | endpoint_area_server_reflexive | P-ENDPOINT-AREA-ADAPTER | `stack_builder` 在把 `p2p-frame::EndpointArea` 转换为 CYFS endpoint area 时，消费新命名的 `ServerReflexive` 分支，并映射到当前下游仍使用的 `bucky_objects::EndpointArea::Default`。 | `cyfs-p2p/src/stack_builder.rs` | 若下游 CYFS endpoint area 后续也引入 `ServerReflexive`，应退回 design 重新定义迁移策略；本轮不在适配层创建新语义。 |
