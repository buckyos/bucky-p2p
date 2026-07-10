# Implementation Admission: SN Client QA Correlation Fix

## Implementation Admission Evidence
| evidence_item | source | status | notes |
|---------------|--------|--------|-------|
| proposal_read | `docs/versions/v0.1/modules/p2p-frame/sn-client-qa-correlation-fix/proposal.md` | pass | Approved sibling proposal read; it requires Report/Call/Query QA transactions, removal of SN-owned waiter state, business body validation, coordinated migration, and preserved SnCalled behavior. |
| design_read | `docs/versions/v0.1/modules/p2p-frame/sn-client-qa-correlation-fix/design.md` | pass | Approved design read; it assigns QA header correlation to sfo-cmd-server, defines client/service return flows, callback cancellation cleanup, exact Scope Paths, file order, and failure transitions. |
| change_scope_matches_request | proposal/design rows for `sn_client_qa_transactions`, `sn_client_qa_pending_cleanup`, and `sn_client_qa_migration_boundary` | pass | These three change ids cover the requested QA migration, removal of application pending state, dependency cleanup needed on all exits, and the no-fallback coordinated deployment boundary. |
| active_module_resolved | `module: p2p-frame`, `submodule: sn-client-qa-correlation-fix`, `version: v0.1` | pass | The sibling packet directly owns the existing `sn/client`, `sn/service`, and minimal callback-result dependency patch boundaries. |
| no_chat_only_evidence | approved proposal/design rows quoted below | pass | The user approved and launched the pipeline, but implementation behavior and paths are admitted only from the content-bound approved documents. |

## Document Binding
| doc | sha256 |
|-----|--------|
| docs/versions/v0.1/modules/p2p-frame/sn-client-qa-correlation-fix/proposal.md | e08cfd77628e04f9de75efcc35bf3275bfebd300707686bdd2f1b9b630272cfc |
| docs/versions/v0.1/modules/p2p-frame/sn-client-qa-correlation-fix/design.md | 232703f8e046956d6ffa8c25f4293ea0f7b879cc2bb9d83f3a028ead54da8e73 |

## Coverage Quotes

### Quote: proposal.md sn_client_qa_transactions
> | P-SN-CLIENT-QA-1 | sn_client_qa_transactions | ReportSn、SnCall、SnQuery 使用 command QA transaction 返回各自 response；SN 层不管理 tunnel/header sequence 或 pending map，只校验 response body 的业务 `seq` 与适用的 `sn_peer_id`。 | `p2p-frame/src/sn/client/**`、`p2p-frame/src/sn/service/**` 和必要的 SN QA regression seams；不改变 protocol body codec。 | 正向三类 QA 均成功；report red-green 证明 A late 不完成 B；Call/Query 错误或迟到 body 不完成其他请求；代码审查确认 SN 层无 QA header key 管理。 | 不迁移 SnCalled/inter-SN/owner commands，不增加 legacy handler fallback，不改变 endpoint/call/query 业务规则。 |

### Quote: design.md sn_client_qa_transactions
> | sn_client_qa_transactions | P-SN-CLIENT-QA-1 | Overall Approach; Report/Call/Query Key Call Flows; body validation; SnCalled invariant | `p2p-frame/src/sn/client/sn_service.rs`, `p2p-frame/src/sn/service/service.rs` | migration-required command framing; public methods/body codecs unchanged; exported waiter state API removed | Coordinated client/server deployment and source migration required. |

### Quote: proposal.md sn_client_qa_pending_cleanup
> | P-SN-CLIENT-QA-2 | sn_client_qa_pending_cleanup | 删除 application-level `cur_report_future`、`ActiveSN.recv_future`、`SnResp` 和三个 response completion handler；response、编码/发送失败、timeout、取消、tunnel close 和校验失败后均无可被迟到响应命中的 live pending request，依赖层状态不得无界残留。 | SN client Report/Call/Query completion 生命周期以及必要的 `sfo-cmd-server` dependency/API 适配；不得借机重写通用 command runtime。 | 分支级 unit 证据覆盖三类请求退出路径；代码审查确认无业务 waiter；依赖 waiter 审计或测试证明发送失败/取消后无可复用 live waiter 和无界增长。 | 不以禁用并发、延长 timeout 或关闭全部 SN 连接替代 cleanup。 |

### Quote: design.md sn_client_qa_pending_cleanup
> | sn_client_qa_pending_cleanup | P-SN-CLIENT-QA-2 | Remove SN waiter state/handlers; callback future drop guard and lifecycle | `p2p-frame/src/sn/client/sn_service.rs`, `Cargo.toml`, `Cargo.lock`, `third-party/callback-result/**` | backward-compatible callback-result API behavior fix; removes private SN state | Local patch version remains 0.2.4. |

### Quote: proposal.md sn_client_qa_migration_boundary
> | P-SN-CLIENT-QA-3 | sn_client_qa_migration_boundary | Report/Call/Query QA framing 采用 coordinated client/server upgrade；在没有已批准 capability negotiation 前，不提供 silent legacy fallback，也不宣称 mixed-version 兼容。 | 仅限定三类 request/response command header framing 和部署迁移说明；body codec、SnCalled/Resp、其他 SN command 与 SN identity/TLS 不变。 | design 给出新/新成功、旧/新与新/旧明确结果矩阵及 rollback；testing 覆盖支持组合并确认 unsupported 组合快速、可诊断失败且不污染状态。 | 不在本任务新增协议协商、双发送或全 SN command version negotiation。 |

### Quote: design.md sn_client_qa_migration_boundary
> | sn_client_qa_migration_boundary | P-SN-CLIENT-QA-3 | Mixed-version matrix, no legacy fallback, unchanged body codecs and SnCalled flow | `p2p-frame/src/sn/client/sn_service.rs`, `p2p-frame/src/sn/service/service.rs` | migration-required Report/Call/Query command framing | No capability negotiation. |
