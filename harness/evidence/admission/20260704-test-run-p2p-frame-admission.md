# Implementation Admission: test-run p2p-frame Admission Fixture

## Implementation Admission Evidence
| evidence_item | source | status | notes |
|---------------|--------|--------|-------|
| proposal_read | `docs/versions/v0.1/modules/p2p-frame/proposal.md` | pass | Read the approved p2p-frame proposal row for `endpoint_area_server_reflexive`. |
| design_read | `docs/versions/v0.1/modules/p2p-frame/design.md` | pass | Read the approved p2p-frame design row for `endpoint_area_server_reflexive`. |
| change_scope_matches_request | proposal `P-ENDPOINT-AREA-1` / design `endpoint_area_server_reflexive` | pass | This fixture is used by workspace-harness test-run admission validation to exercise the current evidence-bound admission-check interface. |
| active_module_resolved | `docs/versions/v0.1/modules/p2p-frame` | pass | Active module is p2p-frame because the admitted change_id belongs to the p2p-frame packet. |
| no_chat_only_evidence | approved proposal/design rows quoted below | pass | Admission relies on approved versioned documents and this bound evidence fixture. |

## Document Binding
| doc | sha256 |
|-----|--------|
| docs/versions/v0.1/modules/p2p-frame/proposal.md | 99703f9bd69b266f2680a6d70cebd9f3dd5ad624801fede3fc056f3204c93f7a |
| docs/versions/v0.1/modules/p2p-frame/design.md | ce89a0dbf78489f8ebdedf98acfed899240cef406fed5a6fbcdbbce42abb4ce5 |

## Coverage Quotes
### Quote: proposal.md endpoint_area_server_reflexive
> | P-ENDPOINT-AREA-1 | endpoint_area_server_reflexive | `EndpointArea::Default` 重命名为 `ServerReflexive`，SN 观察到的节点外网地址只有与节点自上报地址一致时标记为 `Wan`，否则标记为 `ServerReflexive`；文本编码使用 `S`，不再保留 `is_sys_default()` system-default 判定。 | 不引入 STUN/TURN、多 SN NAT 类型推断或新的 endpoint area；不继续把 `D` 作为 `ServerReflexive` 的文本标记；不把 SN 反射地址静默等同于节点自声明公网地址。 | unit 能覆盖 SN 观察地址一致/不一致时的 area 分类，覆盖 `Display`/`FromStr`/raw codec 的 `ServerReflexive` / `S` 编解码，并确认 `is_sys_default()` 不再作为公开方法存在。 |

### Quote: design.md endpoint_area_server_reflexive
> | endpoint_area_server_reflexive | P-ENDPOINT-AREA-1 | 将 `EndpointArea::Default` 重命名为 `ServerReflexive`；`Display`/`FromStr` 使用 `S`，raw codec 保持 area bit 位置但更新语义；SN 观察地址与节点自上报 endpoint 完全一致时标记 `Wan`，否则标记 `ServerReflexive`；删除 `is_sys_default()` 并保持 `is_static_wan()` 只覆盖 `Wan` / `Mapped`。 | `p2p-frame/src/endpoint.rs`、`p2p-frame/src/sn/service/service.rs`、必要 `p2p-frame/src/sn/**`、`p2p-frame/src/tunnel/**`、`docs/versions/v0.1/modules/p2p-frame/design/tunnel-nat-traversal.md` | 该变更影响公开枚举和文本编码；若下游依赖 `D` 或旧 method，回滚应恢复旧 enum/codec 并退回 proposal 重新定义兼容窗口。 |
