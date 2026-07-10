# p2p-frame Remove SnServiceContractServer Acceptance

## 对象与范围
- 模块：`p2p-frame`
- 版本：`v0.1`
- 评审日期：`2026-06-05`
- 范围内：`remove_sn_service_contract_server`
- 范围外：SN command 线协议变更、`sn/protocol` receipt wire 类型删除、SN server/client 基础 report/call/called 能力、peer manager、endpoint 分类、连接验证器、control-stream-only 信令选择、计费/合约/账本新系统

## 发现项
| ID | 严重级别 | 阶段 | 证据 | 问题 | 是否阻塞 |
|----|----------|------|------|------|----------|
| None | - | - | proposal/design/testing/code/test evidence | 未发现阻塞问题 | no |

## 输入
- `docs/versions/v0.1/modules/p2p-frame/proposal.md`
- `docs/versions/v0.1/modules/p2p-frame/design.md`
- `docs/versions/v0.1/modules/p2p-frame/testing.md`
- `docs/versions/v0.1/modules/p2p-frame/testplan.yaml`
- `docs/modules/p2p-frame.md`
- `harness/pipeline-plan.md`
- `p2p-frame/src/sn/client/mod.rs`
- `p2p-frame/src/sn/service/mod.rs`
- `p2p-frame/src/sn/service/service.rs`
- 已删除：`p2p-frame/src/sn/client/contract.rs`
- 已删除：`p2p-frame/src/sn/service/receipt.rs`
- 实际验证结果：schema、admission、搜索、编译、targeted unit、module unit、integration、全量入口

## 一致性摘要
- Proposal 与 design：一致。proposal 将目标收窄为移除 `SnServiceContractServer` 相关服务合约/回执生产路径，同时明确保留 `sn/protocol` 的 receipt wire 类型和消息字段兼容性；design 映射到删除 client contract、service receipt、server contract 注入点和导出路径。
- Design 与模块边界文档：一致。长期 `p2p-frame` 模块文档已同步说明当前 SN 主流程不保留未集成的 service contract/receipt 生产路径，未来计费/合约/回执系统需要新 proposal。
- Design 与 implementation：一致。实现删除 `contract.rs`、`receipt.rs`，移除 `SnServiceContractServer` / `DefaultSnServiceContractServer` / `set_contract(...)` / service `_contract` 字段 / config `contract` 字段，并保留 SN report/call/called、连接验证器和 control stream 命令通道。
- Testing 文档与 testplan：一致。`testing.md` 和 `testplan.yaml` 均加入 `remove_sn_service_contract_server` 的 unit、integration 和搜索/编译审计覆盖。
- Testplan 与实际执行：一致。结构准入、实现准入、无残留搜索、编译、targeted SN tests、module unit、integration 和全量入口均已通过。
- 验收标准可追溯性：通过。删除范围、保留 wire 兼容范围、非目标和验证证据均可从 approved proposal 追溯到 design、testing、实现和命令结果。

## 实现证据
- `p2p-frame/src/sn/client/mod.rs` 不再声明 `mod contract`。
- `p2p-frame/src/sn/service/mod.rs` 不再声明或导出 `receipt`。
- `p2p-frame/src/sn/service/service.rs` 不再包含 `SnServiceContractServer`、默认 contract server、`SnServiceConfig::set_contract(...)`、`SnServer` contract 字段或 `SnService` `_contract` 字段。
- `SnServiceConfig::new(...)` 和 `create_sn_service(...)` 仍保留证书工厂、连接验证器和 SN 基础服务构造路径。
- `p2p-frame/src/sn/protocol/sn.rs` 的 `SnServiceReceipt`、`ReceiptWithSignature`、`contract_id` 和 `receipt` 字段按 proposal 非目标保留，用于 wire 兼容。
- 搜索 `SnServiceContractServer|DefaultSnServiceContractServer|set_contract|mod contract|mod receipt|receipt::` 在 `p2p-frame/src/sn` 与 `p2p-frame/src/lib.rs` 中无匹配。

## 命令结果
- `uv run --active python ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame`：passed
- `uv run --active python ./harness/scripts/admission-check.py --version v0.1 --module p2p-frame --change-id remove_sn_service_contract_server`：passed
- `rg -n 'SnServiceContractServer|DefaultSnServiceContractServer|set_contract|mod contract|mod receipt|receipt::' p2p-frame/src/sn p2p-frame/src/lib.rs`：passed, no matches
- `cargo check -p p2p-frame`：passed
- `cargo test -p p2p-frame sn_service -- --nocapture`：passed, 3 targeted SN service tests
- `python3 ./harness/scripts/test-run.py p2p-frame unit`：passed, 118 lib unit tests, 0 doc tests
- `python3 ./harness/scripts/test-run.py p2p-frame integration`：passed, workspace compatibility tests
- `python3 ./harness/scripts/test-run.py all all`：passed
- `./test-run.sh`：passed, exit 0
- `git diff --check`：passed

## Scope Notes
- `stage-scope-check.py --stage design/testing/proposal` reports violations because the worktree already contains many staged and untracked governance/review files plus this automatic pipeline updated multiple stage artifacts.
- Those pre-existing dirty files were not reverted. Acceptance for this change relies on approved docs, schema/admission checks, focused code search, compile, targeted tests, module tests, integration tests, and full test entry evidence.
- `./test-run.sh` prints `.venv/bin/activate: OSTYPE: parameter not set` at startup, then continues and exits 0.

## 结论
- 通过。
- 原因：approved proposal/design/testing 均直接覆盖 `remove_sn_service_contract_server`；实现删除 service contract/receipt 生产路径并保留 SN wire receipt 兼容结构；搜索、编译、targeted、module、integration 和全量验证入口均通过；未发现文档、实现或测试一致性阻塞问题。

## 退回路由
- Proposal 任务：不需要
- Design 任务：不需要
- Testing 任务：不需要
- Implementation 任务：不需要

## 残余风险
- `sn/protocol` receipt 类型和字段仍保留，用于既有线协议兼容；未来如果要重新引入计费、合约或回执生产系统，需要新的 proposal/design。
- `sn-miner-rust/src/main.rs` 仍有既有 unused import warning：`cyfs_p2p::executor::Executor`，不属于本轮范围。
