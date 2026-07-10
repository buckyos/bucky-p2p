# Implementation Admission: Peer Observed WAN Endpoint Interface

## Implementation Admission Evidence
| evidence_item | source | status | notes |
|---------------|--------|--------|-------|
| proposal_read | `docs/versions/v0.1/modules/p2p-frame/proposal.md` | pass | Approved proposal read; `endpoint_area_server_reflexive` defines SN observed endpoint classification and explicitly separates self-reported `Wan` from SN-reflected addresses. |
| design_read | `docs/versions/v0.1/modules/p2p-frame/design.md` | pass | Approved design read; `endpoint_area_server_reflexive` maps SN observed endpoint classification to `p2p-frame/src/sn/service/service.rs`. |
| change_scope_matches_request | proposal `P-ENDPOINT-AREA-1` / design `endpoint_area_server_reflexive` | pass | The current request keeps the classification logic and asks for an additional peer-id-only interface to retrieve the SN-observed external endpoints; this is a narrow implementation seam inside the approved SN observed endpoint classification scope. |
| active_module_resolved | `docs/versions/v0.1/modules/p2p-frame` | pass | SN service code lives under `p2p-frame/src/sn/service/service.rs`, which belongs to the approved `p2p-frame` packet. |
| no_chat_only_evidence | approved proposal/design rows quoted below | pass | Admission relies on approved endpoint area semantics and scope paths; the chat request only selects the peer-id-only helper shape within that approved implementation scope. |

## Document Binding
| doc | sha256 |
|-----|--------|
| docs/versions/v0.1/modules/p2p-frame/proposal.md | ebe923b709bebfae339b628b13b25a1d65af80248c1ff4f1267345afff162e07 |
| docs/versions/v0.1/modules/p2p-frame/design.md | dbd5ad41d051033040b185bbbe17852437a65167b7fc4313033d2dd8bcfc301c |

## Coverage Quotes

### Quote: proposal.md endpoint_area_server_reflexive
> | P-ENDPOINT-AREA-1 | endpoint_area_server_reflexive | `EndpointArea::Default` 重命名为 `ServerReflexive`，SN 观察到的节点外网地址只有与节点自上报地址一致时标记为 `Wan`，否则标记为 `ServerReflexive`；文本编码使用 `S`，不再保留 `is_sys_default()` system-default 判定。 | 不引入 STUN/TURN、多 SN NAT 类型推断或新的 endpoint area；不继续把 `D` 作为 `ServerReflexive` 的文本标记；不把 SN 反射地址静默等同于节点自声明公网地址。 | unit 能覆盖 SN 观察地址一致/不一致时的 area 分类，覆盖 `Display`/`FromStr`/raw codec 的 `ServerReflexive` / `S` 编解码，并确认 `is_sys_default()` 不再作为公开方法存在。 |

### Quote: design.md endpoint_area_server_reflexive
> | endpoint_area_server_reflexive | P-ENDPOINT-AREA-1 | 将 `EndpointArea::Default` 重命名为 `ServerReflexive`；`Display`/`FromStr` 使用 `S`，raw codec 保持 area bit 位置但更新语义；SN 观察地址与节点自上报 endpoint 完全一致时标记 `Wan`，否则标记 `ServerReflexive`；删除 `is_sys_default()` 并保持 `is_static_wan()` 只覆盖 `Wan` / `Mapped`。 | `p2p-frame/src/endpoint.rs`、`p2p-frame/src/sn/service/service.rs`、必要 `p2p-frame/src/sn/**`、`p2p-frame/src/tunnel/**`、`docs/versions/v0.1/modules/p2p-frame/design/tunnel-nat-traversal.md` | 该变更影响公开枚举和文本编码；若下游依赖 `D` 或旧 method，回滚应恢复旧 enum/codec 并退回 proposal 重新定义兼容窗口。 |
