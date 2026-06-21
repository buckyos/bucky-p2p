# Admission Evidence: SN client connects TTP server target

## Implementation Admission Evidence
| evidence_item | source | status | notes |
|---------------|--------|--------|-------|
| proposal_read | docs/versions/v0.1/modules/p2p-frame/proposal.md | pass | Read approved proposal coverage for `sn_control_stream_signaling`, including SN client signaling over TTP control stream and no fallback to ordinary stream. |
| design_read | docs/versions/v0.1/modules/p2p-frame/design.md | pass | Read approved design coverage for `sn_control_stream_signaling`, whose scope includes `p2p-frame/src/sn/client/sn_service.rs` and SN client use of `Tunnel::open_control_stream(...)`. |
| change_scope_matches_request | P-SN-CONTROL-STREAM-1 / sn_control_stream_signaling | pass | User requested the SN client TTP client itself connect/register the SN server target when creating the SN TTP command path, rather than doing this as a stack-level SN proxy special case. |
| active_module_resolved | docs/versions/v0.1/modules/p2p-frame | pass | The production behavior belongs to p2p-frame SN client signaling and TTP target registration. |
| no_chat_only_evidence | approved proposal/design mapping | pass | Admission relies on approved p2p-frame proposal/design rows and bound hashes; chat only supplied the concrete implementation placement. |

## Document Binding
| doc | sha256 |
|-----|--------|
| docs/versions/v0.1/modules/p2p-frame/proposal.md | 40781e1709b82117f8ae9ffaa40d636612056910458d2e29430f33dfe616aef5 |
| docs/versions/v0.1/modules/p2p-frame/design.md | f1dafd6a84aec0b7193242add91ca13c470399359583584c3bb1c994d9eac0ef |

## Coverage Quotes
### Quote: proposal.md sn_control_stream_signaling
> | P-SN-CONTROL-STREAM-1 | sn_control_stream_signaling | SN report、call、called、response 或等价低频小消息通过 `Tunnel` control stream 交互，不再为每次 SN 信令新建普通业务 `open_stream()`，且不保留普通 stream fallback。 | 不把 control stream 扩展成 SN 大流量数据平面；不公开或依赖内部 `control_stream` frame/stream id/window 协议；不改变 SN 最终连通性语义、单 SN 边界、SN 观察 endpoint 分类或 `SnCallResp` 只表示 SN 受理结果的语义；控制通道不可用、远端未监听 SN purpose 或 control stream 打开失败时当前 SN 命令通道建立或发送失败，不得 fallback 到普通 stream。 | schema/admission 能以 `sn_control_stream_signaling` 建立后续准入；unit 或编译证据确认 SN 信令路径调用 `open_control_stream` / `listen_control_stream` 而不是 `open_stream` / `listen_stream`；测试覆盖 control stream purpose 过滤、控制通道不可用或未监听时失败，以及 SN report/call/called/response 小消息仍能通过 control stream 完成。 |

### Quote: design.md sn_control_stream_signaling
> | sn_control_stream_signaling | P-SN-CONTROL-STREAM-1 | SN report、call、called、response 或等价低频小消息使用固定 SN control purpose 调用 `Tunnel::open_control_stream(...)` / `listen_control_stream(...)`；不默认建立普通业务 `open_stream()`；bootstrap、控制通道不可用、远端未监听或旧版本不支持时当前 SN 命令通道创建/发送失败，不回退到普通 stream。 | `p2p-frame/src/sn/client/sn_service.rs`、`p2p-frame/src/sn/service/service.rs`、必要 `p2p-frame/src/sn/**`、`p2p-frame/src/networks/tunnel.rs`、`docs/versions/v0.1/modules/p2p-frame/design/sn-control-stream-signaling.md`、相关 tests | 回滚时恢复 SN 默认普通 stream 信令路径并删除 SN control purpose 注册/发送逻辑；不得删除通用 `Tunnel` control stream API，也不得改变 SN 命令、单 SN、endpoint 分类或 `SnCallResp` 语义。 |
