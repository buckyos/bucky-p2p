# Implementation Admission: Stack ServerRuntime External Required

## Implementation Admission Evidence
| evidence_item | source | status | notes |
|---------------|--------|--------|-------|
| proposal_read | `docs/versions/v0.1/modules/p2p-frame/proposal.md` | pass | Approved proposal read; `stack_server_runtime_external_required` requires stack-level `ServerRuntime` to be supplied by external callers and forbids default runtime creation in `p2p-frame/src/stack.rs`. |
| design_read | `docs/versions/v0.1/modules/p2p-frame/design.md` | pass | Approved design read; `stack_server_runtime_external_required` maps implementation to `p2p-frame/src/stack.rs`, `cyfs-p2p/src/stack_builder.rs`, `cyfs-p2p-test/src/main.rs`, and necessary production call points. |
| change_scope_matches_request | proposal `P-STACK-SERVER-RUNTIME-EXTERNAL-1` / design `stack_server_runtime_external_required` | pass | The current request is to remove stack-level default `ServerRuntime::start(...)` and require external runtime injection. |
| active_module_resolved | `docs/versions/v0.1/modules/p2p-frame` | pass | `p2p-frame/src/stack.rs` owns stack config and assembly; listed workspace direct callers are included in design scope for migration. |
| no_chat_only_evidence | approved proposal/design rows quoted below | pass | Admission relies on approved versioned documents; the chat instruction supplies launch/approval but is not used as implementation coverage. |

## Document Binding
| doc | sha256 |
|-----|--------|
| docs/versions/v0.1/modules/p2p-frame/proposal.md | 99703f9bd69b266f2680a6d70cebd9f3dd5ad624801fede3fc056f3204c93f7a |
| docs/versions/v0.1/modules/p2p-frame/design.md | ce89a0dbf78489f8ebdedf98acfed899240cef406fed5a6fbcdbbce42abb4ce5 |

## Coverage Quotes

### Quote: proposal.md stack_server_runtime_external_required
> | P-STACK-SERVER-RUNTIME-EXTERNAL-1 | stack_server_runtime_external_required | Stack 级 `ServerRuntime` 必须由外部调用方显式传入；`P2pConfig` 默认构造、setter/getter 和 `create_p2p_env(...)` 装配路径不得在 `p2p-frame/src/stack.rs` 内部自行调用 `ServerRuntime::start(...)` 创建默认 runtime。 | 不改变 TCP/QUIC tunnel wire、TLS 身份校验、listener reuseport 分发语义、SN command wire、endpoint 分类或 runtime crate feature；不新增 `NetworkServerRuntime`、socket factory trait、全局 singleton runtime 或隐藏 fallback；不把默认启动体验保留在 `p2p-frame` 内部。 | schema/admission 能以 `stack_server_runtime_external_required` 建立后续准入；design/testing 需要定义 `P2pConfig` 缺少 runtime 时的 fallible constructor、必填参数或错误边界，并列出 workspace 直接调用方迁移；implementation 后代码搜索确认 `p2p-frame/src/stack.rs` 生产路径不存在 `ServerRuntime::start(...)` 兜底，unit 或 compile 覆盖显式传入 runtime 后 stack 可构造，缺少 runtime 按 design 失败或无法编译。 |

### Quote: design.md stack_server_runtime_external_required
> | stack_server_runtime_external_required | P-STACK-SERVER-RUNTIME-EXTERNAL-1 | `P2pConfig` 直接保存外部传入的 `ServerRuntime`；`P2pConfig::new(...)` 接收 runtime 必填参数；`create_p2p_env(...)` 不再为缺省 config 调用 `ServerRuntime::start(...)`，只 clone 已传入 runtime 给 TCP/QUIC network；workspace 直接调用方迁移为在 `p2p-frame` 外部创建进程级或测试级 runtime 后传入。 | `p2p-frame/src/stack.rs`、`cyfs-p2p/src/stack_builder.rs`、`cyfs-p2p-test/src/main.rs`、必要生产调用点 | 回滚时恢复 stack 内部默认 runtime fallback 和旧 `P2pConfig::new(...)` 签名；不得借回滚改变 TCP/QUIC wire、TLS 身份校验、listener 分发、SN service runtime 注入或 endpoint 语义。 |
