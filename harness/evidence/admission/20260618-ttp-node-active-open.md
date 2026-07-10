# Task 20260618-ttp-node-active-open Admission Evidence

## Implementation Admission Evidence
| evidence_item | source | status | notes |
|---------------|--------|--------|-------|
| proposal_read | docs/versions/v0.1/modules/p2p-frame/proposal.md | pass | Read approved proposal item `ttp_node_active_open`, covering `TtpNode` with TtpServer-like listener/connector surface and active tunnel creation for `open_stream(...)` / `open_control_stream(...)` when no matching tunnel exists. |
| design_read | docs/versions/v0.1/modules/p2p-frame/design.md | pass | Read approved design mapping for `ttp_node_active_open`, including `TtpNode` as a new TTP type, helper reuse, runtime attach, first-version datagram behavior, and scope paths under `p2p-frame/src/ttp`. |
| change_scope_matches_request | proposal P-TTP-NODE-ACTIVE-OPEN-1 / design ttp_node_active_open | pass | The current request asks to add `TtpNode` logic in the TTP submodule with active tunnel creation in `open_stream` and `open_control_stream`; the approved docs directly cover that behavior. |
| active_module_resolved | docs/versions/v0.1/modules/p2p-frame | pass | TTP code lives under `p2p-frame/src/ttp/**`, which belongs to the approved `p2p-frame` packet. |
| no_chat_only_evidence | versioned docs only | pass | Admission relies on the approved proposal/design rows and scope paths below; chat only supplied the requirement that was approved into versioned documents. |

## Document Binding
| doc | sha256 |
|-----|--------|
| docs/versions/v0.1/modules/p2p-frame/proposal.md | ebe923b709bebfae339b628b13b25a1d65af80248c1ff4f1267345afff162e07 |
| docs/versions/v0.1/modules/p2p-frame/design.md | dbd5ad41d051033040b185bbbe17852437a65167b7fc4313033d2dd8bcfc301c |

## Coverage Quotes

### Quote: proposal.md ttp_node_active_open
> | P-TTP-NODE-ACTIVE-OPEN-1 | ttp_node_active_open | 新增 `TtpNode`，提供与 `TtpServer` 同类的监听和连接接口；`open_stream(...)` 与 `open_control_stream(...)` 对匹配 `TtpTarget` 先复用已有可用 tunnel，缺失时主动通过现有 network 建立 tunnel、attach 到 `TtpRuntime`，再打开 stream/control stream。 | 不改变 `Tunnel` / `TunnelNetwork` trait、TCP/QUIC/PN/TTP 线协议、vport/purpose 编码、身份校验或 tunnel publish 规则；不新增库内 target directory 或自动路由系统；不绕过 `NetManager` / `TunnelNetwork`；不静默改变现有 `TtpServer` lookup-only 行为，除非 design 明确批准兼容重构；`open_datagram(...)` 的缺失 tunnel 行为必须由 design 单独明确。 | schema/admission 能以 `ttp_node_active_open` 建立后续准入；design/testing 需要覆盖 `TtpNode` 接口边界、tunnel cache/target match 复用、缺失 tunnel 时的 active open、runtime attach、`open_stream` / `open_control_stream` 错误传播、`open_datagram` 明确边界、以及代码审查确认 wire/trait/TtpServer 既有行为不被未授权改变。 |

### Quote: design.md ttp_node_active_open
> | ttp_node_active_open | P-TTP-NODE-ACTIVE-OPEN-1 | 新增 `TtpNode` 类型和 `TtpNodeRef`；它实现 `TtpPortListener` / `TtpConnector`，注册 incoming tunnel subscriber 并将入站 tunnel attach/cache；`open_stream(...)`、`open_control_stream(...)` 和第一版 `open_datagram(...)` 通过共享 `get_or_create_tunnel(target)` 复用已有可用 tunnel或按 target endpoint 主动创建、attach、缓存 tunnel 后打开对应 channel；`TtpServer` lookup-only open 行为保持不变。 | `p2p-frame/src/ttp/client.rs`、`p2p-frame/src/ttp/node.rs`、`p2p-frame/src/ttp/server.rs`、`p2p-frame/src/ttp/mod.rs`、`p2p-frame/src/ttp/tests.rs` | 回滚时删除 `TtpNode` 导出和测试，保留 `TtpClient` / `TtpServer` 现有行为；不得改变 `Tunnel` / `TunnelNetwork` trait、wire 协议、target matching 或 `TtpServer` lookup-only 语义。 |
