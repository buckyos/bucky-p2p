# Implementation Admission: SN Service Runtime External Injection

## Implementation Admission Evidence
| evidence_item | source | status | notes |
|---------------|--------|--------|-------|
| proposal_read | `docs/versions/v0.1/modules/p2p-frame/proposal.md` | pass | Approved proposal read; `sn_service_runtime_external_injection` requires SN service to consume an externally supplied or stack-level `ServerRuntime` and forbids service-local runtime creation. |
| design_read | `docs/versions/v0.1/modules/p2p-frame/design.md` | pass | Approved design read; `sn_service_runtime_external_injection` maps implementation to `p2p-frame/src/sn/service/service.rs`, necessary `p2p-frame/src/sn/**` call points, `sn-miner-rust` and `cyfs-p2p-test` startup call sites, requires missing runtime to return a configuration error, and preserves SN protocol behavior. |
| change_scope_matches_request | proposal `P-SN-SERVICE-RUNTIME-EXTERNAL-1` / design `sn_service_runtime_external_injection` | pass | The current request is to stop constructing `ServerRuntime` inside `create_sn_service(...)` and require an externally supplied runtime. |
| active_module_resolved | `docs/versions/v0.1/modules/p2p-frame` | pass | `p2p-frame/src/sn/service/service.rs` belongs to the approved `p2p-frame` module packet and the design records workspace startup callers `sn-miner-rust` / `cyfs-p2p-test` as compatibility migration scope for this public constructor change. |
| no_chat_only_evidence | approved proposal/design rows quoted below | pass | Admission relies on approved versioned documents; the chat confirmation supplies approval/launch and the original intent, not a substitute for document coverage. |

## Document Binding
| doc | sha256 |
|-----|--------|
| docs/versions/v0.1/modules/p2p-frame/proposal.md | 0d44a287897110fd067f277fb96fdc28eaa79347864ff3c5634c97337ee0635a |
| docs/versions/v0.1/modules/p2p-frame/design.md | b5687d64b861287f47249d5103dc7bc6457c6196ff3553b9f4b795624a249a42 |

## Coverage Quotes

### Quote: proposal.md sn_service_runtime_external_injection
> | P-SN-SERVICE-RUNTIME-EXTERNAL-1 | sn_service_runtime_external_injection | SN service 的 `ServerRuntime` 必须由外部调用方或 stack 级全局 runtime 显式传入；`create_sn_service(...)`、`SnServiceConfig` 装配和 `SnServer` 构造路径不得在 `p2p-frame/src/sn/service/service.rs` 内部自行调用 `ServerRuntime::start(...)` 创建新的 service-local runtime。 | 不改变 SN command wire、control-stream-only 信令、TCP/QUIC tunnel wire、TLS 身份校验、endpoint 分类、SN 连接验证器语义或 TCP 来源地址过滤语义；不新增 `NetworkServerRuntime`、socket factory trait 或其他通用 runtime 抽象；不把 runtime 所有权隐藏在 SN service fallback 中。 | schema/admission 能以 `sn_service_runtime_external_injection` 建立后续准入；design/testing 需要定义缺少 runtime 时的构造/API 错误边界或 stack 级默认注入路径；implementation 后代码搜索确认 `p2p-frame/src/sn/service/service.rs` 生产路径不存在 `ServerRuntime::start(...)` 兜底，unit 或 compile 覆盖调用方显式传入 runtime 后 SN service 可启动。 |

### Quote: design.md sn_service_runtime_external_injection
> | sn_service_runtime_external_injection | P-SN-SERVICE-RUNTIME-EXTERNAL-1 | `create_sn_service(...)` 不再为缺省 config 自行启动 `ServerRuntime`；`SnServiceConfig` 的 runtime 由外部或 stack 注入，缺失时返回配置错误；`SnServer::new(...)` 只消费已解析 runtime 并向内部组件 clone；workspace 直接调用方迁移为启动路径创建并传入进程级 runtime。 | `p2p-frame/src/sn/service/service.rs`、必要 `p2p-frame/src/sn/**` 调用点、`sn-miner-rust/src/main.rs`、`sn-miner-rust/Cargo.toml`、`cyfs-p2p-test/src/main.rs`、`cyfs-p2p-test/Cargo.toml`、`Cargo.lock`、相关 tests | 回滚时恢复 `create_sn_service(...)` 内部默认 runtime fallback 并撤回下游调用点迁移；不得借回滚改变 SN command wire、control-stream-only 信令、validator 输入、endpoint 分类或 TCP 来源地址过滤。 |
