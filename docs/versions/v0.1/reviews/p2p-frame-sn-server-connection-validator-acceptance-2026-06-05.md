# p2p-frame SN Server Connection Validator Acceptance

## 对象与范围
- 模块：`p2p-frame`
- 版本：`v0.1`
- 评审日期：`2026-06-05`
- 范围内：`sn_server_connection_validator`
- 范围外：认证协议、计费、限速、NAT 类型推断、跨 SN 策略同步、SN command 线协议变更

## 发现项
| ID | 严重级别 | 阶段 | 证据 | 问题 | 是否阻塞 |
|----|----------|------|------|------|----------|
| None | - | - | proposal/design/testing/code/test evidence | 未发现阻塞问题 | no |

## 输入
- `docs/versions/v0.1/modules/p2p-frame/proposal.md`
- `docs/versions/v0.1/modules/p2p-frame/design.md`
- `docs/versions/v0.1/modules/p2p-frame/testing.md`
- `docs/versions/v0.1/modules/p2p-frame/testplan.yaml`
- `docs/versions/v0.1/modules/p2p-frame/acceptance.md`
- `docs/modules/p2p-frame.md`
- `p2p-frame/src/sn/service/service.rs`
- 实际测试结果：unit、integration、targeted unit

## 一致性摘要
- Proposal 与 design：一致。proposal 要求 SN server validator、默认 allow-all、reject 短路，并将 validator 上下文收窄为只包含 `client_id` 与客户端证书；design 映射到 `sn/service` validator type/helper、构造路径、handler 前校验、证书 id 与 cmd tunnel peer id 一致性检查和错误映射。
- Design 与 implementation：一致。实现中的 `SnConnectionValidateContext` 只包含 `client_id` 与 `client_cert`；`SnConnectionValidator`、`allow_all_sn_connection_validator()`、`SnServiceConfig::set_connection_validator(...)` 保持装配能力，并在 `handle_call` / `handle_report_sn` 开始处校验。
- Testing 文档与 testplan：一致。`testing.md` 和 `testplan.yaml` 均加入 `sn_server_connection_validator` 的 unit、targeted、integration 覆盖，并要求证书不匹配时不调用 validator。
- Testplan 与实际执行：一致。`python3 ./harness/scripts/test-run.py p2p-frame unit` 与 `python3 ./harness/scripts/test-run.py p2p-frame integration` 均已通过；targeted `cargo test -p p2p-frame sn_service -- --nocapture` 已通过。
- 验收标准可追溯性：通过。默认 allow-all 兼容性、自定义 reject 短路、上下文字段收窄和证书 id mismatch 短路均有直接测试证据。

## 实现证据
- 默认构造：`SnServiceConfig::new(...)` 安装 `allow_all_sn_connection_validator()`。
- 自定义构造：`SnServiceConfig::set_connection_validator(...)` 允许调用方显式注入策略。
- 校验上下文：`client_id` 来自 cmd tunnel `peer_id`；`client_cert` 来自当前 SN 请求携带的客户端证书或已缓存的同一客户端证书；实现用 `cert_factory` 解析证书并确认解析出的 id 等于 `client_id` 后才调用 validator。
- 上下文收窄：`SnConnectionValidateContext` 不再包含 command、tunnel id、reported peer、target peer、来源 endpoint 或其他报文载荷派生字段。
- 短路位置：`handle_call(...)` 和 `handle_report_sn(...)` 在状态更新、peer manager 更新或 call 转发前执行 validator。
- 错误语义：reject 映射为 `P2pErrorCode::PermissionDenied`，不新增 SN wire 字段。

## 命令结果
- `uv run --active python ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame`：passed
- `uv run --active python ./harness/scripts/admission-check.py --version v0.1 --module p2p-frame --change-id sn_server_connection_validator`：passed
- `cargo test -p p2p-frame sn_service -- --nocapture`：passed, 3 targeted tests
- `python3 ./harness/scripts/test-run.py p2p-frame unit`：passed, 118 lib unit tests, 0 doc tests
- `python3 ./harness/scripts/test-run.py p2p-frame integration`：passed, workspace compatibility tests
- `git diff --check`：passed

## Scope Notes
- `stage-scope-check.py --stage proposal/design` reports violations because the working tree already contains many untracked governance/review files and this automatic pipeline intentionally updated multiple stage artifacts.
- Diff evidence for this acceptance is limited to the current pipeline files, p2p-frame stage documents, and `p2p-frame/src/sn/service/service.rs`.

## 结论
- 通过。
- 原因：approved proposal/design/testing 均直接覆盖 `sn_server_connection_validator` 的上下文收窄；实现满足默认 allow-all、自定义 reject 短路、`client_id + client_cert` 上下文和证书 id mismatch 短路要求；unit 与 integration 证据通过；未发现文档或实现逻辑阻塞问题。

## 退回路由
- Proposal 任务：不需要
- Design 任务：不需要
- Testing 任务：不需要
- Implementation 任务：不需要

## 残余风险
- 当前测试直接覆盖 report allow/reject/证书不匹配；call 路径使用同一 `validate_context_from_cert(...)` 与 `validate_connection(...)` helper 和 handler 前校验位置，未新增单独 call reject 单测。
- `sn-miner-rust/src/main.rs` 仍有既有 unused import warning：`cyfs_p2p::executor::Executor`，不属于本轮范围。
