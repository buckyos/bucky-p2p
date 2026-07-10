# Implementation Admission: SN 5x5 Warmup Online Fix

## Implementation Admission Evidence
| evidence_item | source | status | notes |
|---------------|--------|--------|-------|
| proposal_read | `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/proposal.md` | pass | Approved proposal read; it defines `sn_five_by_five_command_matrix` as the 5 Owner SN, 5 Serving SN, 5 user peer command matrix requirement, including client online/report evidence. |
| design_read | `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/design.md` | pass | Approved design read; it maps `sn_five_by_five_command_matrix` to `p2p-frame/src/sn/**` and requires deterministic unit-level topology coverage when multi-process lifecycle is unavailable. |
| change_scope_matches_request | user failure report and approved proposal/design | pass | The failing `sn_single_serving_with_owner_client_client_online` warmup test is the single-serving online prerequisite for the approved 5x5 command matrix test entry. |
| active_module_resolved | `module: p2p-frame`, `submodule: sn-distributed-directory` | pass | Direct submodule packet is `docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/`. |
| no_chat_only_evidence | approved proposal/design rows quoted below | pass | Implementation admission uses approved docs and the reported failing test path, not chat-only requirements. |

## Document Binding
| doc | sha256 |
|-----|--------|
| docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/proposal.md | 884246e455716e0013dd52360080cb5d7f5bb03e5639ba0931f509c548516164 |
| docs/versions/v0.1/modules/p2p-frame/sn-distributed-directory/design.md | 37e4606785a1a85646bb7b4b8c58a7c3d76f43c4bb28cc8747730d0b58adea46 |

## Coverage Quotes
### Quote: proposal.md sn_five_by_five_command_matrix
> | P-SN-DIST-CMD-MATRIX-1 | sn_five_by_five_command_matrix | 自动化构造 5 Owner SN、5 Serving SN、5 user peer 的测试拓扑，每个 user peer 分别连接不同 Serving SN，并覆盖所有 SN 命令族的成功、跨 serving 和关键失败路径。 | `p2p-frame/src/sn/**` 中测试或必要测试支撑；如需真实多进程 harness，范围必须在后续 design/testing 中明确。 | 测试证据显示 5 个用户分别通过不同 Serving SN 完成 `ReportSn`、跨 Serving SN `SnQuery`、`SnCall`/`SnCalled`，并覆盖 inter-SN `Heartbeat`、`PublishLease`、`QueryLease`、`QueryDetail`、`RelayCall` 以及 owner serving-facing `PublishLease`、`QueryLease` 的 dispatch/response/reject 路径。 | 不新增客户端可见命令；不要求客户端选择 Serving SN；不把测试拓扑规模解释为生产部署上限。 |

### Quote: design.md sn_five_by_five_command_matrix
> | sn_five_by_five_command_matrix | P-SN-DIST-CMD-MATRIX-1 | validation topology with 5 ownerSN members, 5 servingSN services, 5 user peers each bound to a distinct servingSN; command matrix covers peer-facing report/query/call/called, inter-SN heartbeat/publish/query/detail/relay, and owner serving-facing publish/query dispatch plus reject paths | `p2p-frame/src/sn/**` | test harness uses existing service, directory, and inter-SN seams without adding client-visible commands | real multi-process TTP lifecycle may remain a DV/integration gap if the runnable unit harness uses in-memory peers |
