# 当前工作树验收报告

## Findings

| ID | 严重级别 | 阶段 | 证据 | 问题 | 是否阻塞 |
|----|----------|------|------|------|----------|
| F1 | High | implementation / hygiene | `git diff --check` 退出码 2；`p2p-frame/docs/ttp_module_design.md` 大量 trailing whitespace；`sn-miner-rust/config/package.cfg` 行尾空白 | 完整 diff 不满足验收评审门禁；该规则明确要求 `git diff --check` 通过。 | 是 |
| F2 | High | proposal/design/testing admission | `cyfs-p2p/src/stack_builder.rs` 已修改；`python3 ./harness/scripts/admission-check.py --version v0.1 --module cyfs-p2p --change-id endpoint_area_server_reflexive` 失败：`proposal.md is not approved` | 当前 diff 触及 `cyfs-p2p` 适配层，但该模块没有已批准且直接映射的准入证据。 | 是 |
| F3 | High | proposal/design/testing admission | `sn-miner-rust/config/package.cfg` 已修改；`python3 ./harness/scripts/admission-check.py --version v0.1 --module sn-miner --change-id endpoint_area_server_reflexive` 失败：`proposal.md is not approved` | 当前 diff 触及 `sn-miner` 配置，且还存在 CRLF/whitespace churn；该模块没有已批准且直接映射的准入证据。 | 是 |
| F4 | High | design / testing | `docs/versions/v0.1/modules/p2p-frame/proposal.md` `approved_at: 2026-05-13`；`design.md` 与 `testing.md` 仍为 `approved_at: 2026-05-10`，同时本轮 diff 修改了这些已批准文档 | 下游 design/testing 的审批时间早于新 proposal 覆盖，且已批准文档发生实质修改但没有重新审批记录。 | 是 |
| F5 | High | testing | `python3 ./harness/scripts/test-run.py p2p-frame dv` 被手动终止，退出码 143；终止前日志显示 `[all-in-one] round=64 total=768 passed=768`，但命令未自然完成 | DV 入口没有形成完成态，不能作为通过证据；测试通过片段不能替代规范入口完成。 | 是 |
| F6 | Medium | scope / repository hygiene | `git status --short` 显示 staged `harness/scripts/__pycache__/*.pyc`、未跟踪 `.codex`、`.idea/`、`devices/`、`profile/`、`sn/`、`sn.desc`、`sn.sec`、`tree.txt` 等 | 工作树包含未准入的生成制品、本地配置和运行时状态；当前验收对象不干净，无法把完整 diff 归因到已批准范围。 | 是 |
| F7 | Medium | scope / governance | `harness/pipeline-plan.md`、`harness/rules/acceptance-task-rules.md`、`harness/rules/trigger-rules.md`、`harness/scripts/verify-workspace-harness.py` 已修改，并有多个新 harness 规则/脚本未跟踪 | 当前验收请求主要映射到 `p2p-frame` 的 endpoint change，但 diff 同时包含治理层变更；这些变更没有单独的已批准范围和验收基线。 | 是 |

## 对象与范围
- 模块：完整工作树审计，重点为 `p2p-frame`
- 版本：`v0.1`
- 评审日期：2026-05-13
- 直接映射的 `change_id`：`endpoint_area_server_reflexive`
- 范围内可审计基线：`p2p-frame` 的 `EndpointArea::ServerReflexive` 语义、SN 观察地址分类、相关 unit/DV/integration 验证入口
- 范围外或未准入：`cyfs-p2p` 适配层改动、`sn-miner` 配置改动、harness 治理改动、生成制品和本地运行时文件

## Diff 命令摘要
- `git status --short`：存在 `p2p-frame`、`cyfs-p2p`、`sn-miner`、harness、文档和大量未跟踪文件。
- `git diff --stat`：22 个 tracked 文件，约 1136 insertions / 771 deletions；包含 `p2p-frame/docs/ttp_module_design.md` 的大规模行尾/换行 churn。
- `git diff --name-status`：包含 `M cyfs-p2p/src/stack_builder.rs`、`M p2p-frame/src/endpoint.rs`、`M p2p-frame/src/sn/service/service.rs`、`M p2p-frame/src/tunnel/tunnel_manager.rs`、`M sn-miner-rust/config/package.cfg`，以及 staged pycache 删除/新增状态。
- `git diff --check`：失败，阻塞验收。

## 变更文件分类
- 范围内改动：`p2p-frame/src/endpoint.rs`、`p2p-frame/src/sn/service/service.rs`、`p2p-frame/src/tunnel/tunnel_manager.rs`，以及 `docs/versions/v0.1/modules/p2p-frame/**` 中与 `endpoint_area_server_reflexive` 相关的文档。
- 跨模块改动：`cyfs-p2p/src/stack_builder.rs`、`sn-miner-rust/config/package.cfg`。
- 治理层改动：`harness/pipeline-plan.md`、`harness/rules/**`、`harness/scripts/verify-workspace-harness.py`，以及未跟踪的 harness 规则和准入脚本。
- 无关 churn：`p2p-frame/docs/ttp_module_design.md` 大规模 CRLF/trailing whitespace 改写；`sn-miner-rust/config/package.cfg` CRLF/trailing whitespace。
- 生成制品/本地状态：`harness/scripts/__pycache__/*.pyc`、`.idea/`、`devices/`、`profile/`、`sn/`、`sn.desc`、`sn.sec`、`tree.txt`。

## Harness 命令结果
- `python3 ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame`：passed
- `python3 ./harness/scripts/admission-check.py --version v0.1 --module p2p-frame --change-id endpoint_area_server_reflexive`：passed
- `python3 ./harness/scripts/schema-check.py --version v0.1 --module cyfs-p2p`：passed
- `python3 ./harness/scripts/admission-check.py --version v0.1 --module cyfs-p2p --change-id endpoint_area_server_reflexive`：failed，proposal 未 approved
- `python3 ./harness/scripts/schema-check.py --version v0.1 --module sn-miner`：passed
- `python3 ./harness/scripts/admission-check.py --version v0.1 --module sn-miner --change-id endpoint_area_server_reflexive`：failed，proposal 未 approved

## 测试证据
- Unit：`python3 ./harness/scripts/test-run.py p2p-frame unit` 通过；`cargo test -p p2p-frame` 结果为 95 passed。
- DV：`python3 ./harness/scripts/test-run.py p2p-frame dv` 未自然完成；终止前 all-in-one 日志显示 round 64、768/768 passed；最终命令退出码 143。
- Integration：`python3 ./harness/scripts/test-run.py p2p-frame integration` 通过；`cargo test --workspace` 完成。

## 一致性摘要
- Proposal 与 design：不合格。`proposal.md` 的审批时间为 2026-05-13，而 design/testing 仍标为 2026-05-10，且本轮修改了已批准文档。
- Design 与模块边界文档：部分一致。`docs/modules/p2p-frame.md` 已同步 `ServerReflexive` 边界，但同步本身属于已批准文档后的实质变更，需要重新审批记录。
- Design 与 implementation：`p2p-frame` 的 endpoint/SN/tunnel 代码大体映射到 `endpoint_area_server_reflexive`，但跨模块改动未准入。
- Testing 文档与 testplan：映射存在，但 DV 入口没有完成态。
- Testplan 与实际执行：unit 和 integration 有完成证据；DV 无完成证据。
- 完整 diff 审计：不合格，存在格式失败、生成制品、本地状态和未准入模块改动。

## 结论
- 结论：rejected
- 原因：当前工作树不满足 acceptance review gate。任一 High finding 都足以阻塞验收；本次同时存在 `git diff --check` 失败、跨模块准入失败、审批时间不一致、DV 未完成和工作树污染。

## 退回路由
- Proposal 任务：为 `cyfs-p2p` 和 `sn-miner` 改动补齐或审批直接覆盖，或从本次交付中移除这些改动。
- Design 任务：重新审批或补充 `p2p-frame` design/testing 对 2026-05-13 proposal 覆盖的同步记录。
- Testing 任务：修正 DV 入口，使 `p2p-frame dv` 能自然完成，或在 testing/testplan 中明确它是持续运行场景并定义可验收的停止条件。
- Implementation 任务：清理 CRLF/trailing whitespace、生成制品、本地运行时文件和未准入治理层改动；保持完整工作树只包含已批准范围。

## 残余风险
- `cyfs-p2p` 将 `EndpointArea::ServerReflexive` 映射到 `bucky_objects::EndpointArea::Default`，可能保留旧下游语义；该适配需要独立 design/testing 说明。
- 已存在多个未跟踪历史验收报告和设计补充文档，后续验收前需要确认哪些属于本轮交付，哪些应保持未纳入。
