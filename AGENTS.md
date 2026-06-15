# P2P 工作区 Agent 地图

## 概览
- 本仓库是一个 Rust workspace，包含点对点网络栈、CYFS 集成、运行时验证工具及其配套二进制。
- 仓库采用严格的 Harness Engineering 工作流，以 proposal-first 验收和硬性的实现准入门禁为核心。

## 工作模型
- 由人类定义意图、范围、约束和审批边界。
- Agent 只能在仓库定义的阶段边界内执行。
- Testing 是 post-implementation 阶段：基于 proposal、design 和已交付代码设计并生成测试实现。
- Acceptance 是对完整证据链的独立审计，与 implementation 分离。
- Auto-pipeline 规则已安装，但模式默认不进入；只有用户显式要求启动、进入或运行自动流水线时，已批准的 proposal 才能作为后续 planning、design、implementation、post-implementation testing 和 acceptance 任务的前置条件。

## 规则优先级
当规则冲突时按以下顺序处理（数字越小优先级越高）：
1. 当前任务中的用户显式指令。用户可以选择阶段、模式和范围，但不能跳过机械门禁、审批来源、实现准入、阶段范围检查、验证或验收审计。
2. `harness/rules/task-entry-gate-rules.md`。
3. 当前阶段对应的 `harness/rules/` 规则文件。
4. `harness/custom-rules/` 中的项目自定义规则；自定义规则只能收紧，不能放松或绕过生成规则。
5. 模块 packet 文档与长期模块文档。

若按上述顺序仍存在真实矛盾，必须停止并报告给用户，不能静默选择一边。

## 任务决策流
1. 如果用户显式指定 proposal / design / testing / acceptance 阶段，则只进入该阶段写作用域。
2. 如果请求新增、删除、收窄、扩大或重分类目标、范围、非目标、支持/不支持行为或验收边界，默认进入 proposal 阶段。
3. 如果请求会改变生产代码、bugfix、optimization 或 refactor，先执行任务入口门禁：定位 packet，读取已批准的 `proposal.md` 与 `design.md`，创建 admission evidence，并通过 `schema-check.py` 与 `admission-check.py` 后才能改代码。
4. 任一门禁失败时，退回最早缺失或不覆盖当前任务的文档阶段，而不是继续编码。
5. 单阶段任务结束前运行 `stage-scope-check.py`，并携带当前阶段和 packet 参数。

| 阶段 | 写作用域 | 完成前检查 |
|---|---|---|
| proposal | 当前 packet 的 `proposal.md` | `doc-structure-check.py --docs proposal`；`stage-scope-check.py --stage proposal --version <v> --module <m>` |
| design | `design.md`、`design/`、必要的长期边界同步 | `doc-structure-check.py --docs design`；`stage-scope-check.py --stage design --version <v> --module <m>` |
| implementation | 生产代码、必要非测试运行时/构建资源、当前任务 admission evidence | 先运行 `schema-check.py` 与带 `--evidence-file` 的 `admission-check.py`；完成后运行带 `--change-id` 的 implementation scope check |
| testing | 测试代码、夹具、runner、统一入口 wiring、testing artifacts | `doc-structure-check.py --docs testing`；`testing-coverage-check.py`；`test-run.py <module> all`；testing scope check |
| acceptance | 评审报告与可选 packet `acceptance.md` | 按 `acceptance-review-rules.md` 运行验收审计、`quality-check.py`，并用 `acceptance-report-check.py <report>` 校验报告 |

## 任务读取顺序
1. 先读本文件。
2. 对任何可能影响代码、测试、运行时、构建、资源、bugfix、optimization 或 refactor 的请求，先读 `harness/rules/task-entry-gate-rules.md`。
3. 再读 `docs/versions/<version>/modules/<module>/` 下当前活跃的模块数据包。
4. 读取 `docs/modules/<module>.md` 了解长期模块边界。
5. 读取 `docs/architecture/` 了解工作区级约束。
6. 读取 `harness/rules/` 了解硬性不变量和门禁。
7. 当任务类型或模块级别要求时，再读取 `harness/process_rules/`、`harness/checklists/` 和 `harness/human-rules/`。

## 阶段边界
- Proposal 职责：把用户意图转化为可审批的目标、范围、非目标、约束与风险边界基线。Proposal 任务只能修改 `proposal.md`。
- 流水线规划职责：在下游执行开始前创建阶段图、依赖、输出和退回路径。Planning 任务只能修改 `harness/pipeline-plan.md` 和任务制品。
- Design 职责：把已批准的 proposal 转化为可执行结构、接口、模块拆分和实现顺序。Design 任务只能修改 `design.md`、`design/`，以及必须同步的 `docs/modules/<module>.md` 边界说明。
- Testing 职责：在 implementation 完成后，基于 proposal、design 和已交付代码设计并生成可运行验证覆盖。Testing 任务只能修改测试代码、测试夹具、测试入口，以及可选的 `testing.md`、`testing/` 和 `testplan.yaml`。
- Implementation 职责：交付满足已批准 proposal 与 design 的最小生产代码和必需运行时/构建资源变更。Implementation 任务只能修改生产代码和必需的非测试运行时/构建资源。
- Acceptance 职责：独立审计 proposal、design、testing、implementation、测试与结果是否仍然一致。Acceptance 任务只写评审报告。

## 硬门禁
- 除 `harness/rules/module-doc-exception-rules.md` 中显式列出的模块级文档豁免外，在 `proposal.md` 与 `design.md` 全部存在且标记为 `status: approved` 之前，implementation 和 bugfix 工作都不得开始。
- implementation 和 bugfix 工作还必须明确 `version`、`module` 与一个或多个 `change_id`，创建 `harness/evidence/admission/<YYYYMMDD>-<task-slug>.md`，并通过 `schema-check.py` 与带 `--evidence-file` 的 `admission-check.py`；审批状态本身不是充分准入证据。
- Agent 不得自批准文档；`status: approved` 只允许来自记录在 `## Approval Record` 的用户显式批准，或来自带 pipeline launch evidence 的 `auto-pipeline`。
- 已批准文档必须记录 `approved_content_sha256`；文档内容被批准后再修改会使审批失效，必须重新审批。
- 下游文档不得静默缩窄、扩展或违背已批准的 proposal。
- 后续阶段如果发现上游问题，必须把工作退回责任阶段，而不是就地修改上游制品。
- 如果问题相关文档已为 `status: approved`，实现前仍必须核对文档中是否已经定义相关逻辑；如果没有，必须先退回相应文档阶段补齐并重新以文档为依据，禁止仅依据用户在对话中的直接说明实现；模块级文档豁免仅适用于规则中显式声明的模块。
- 最终验收以已批准的 `proposal.md` 为准，而不是以实现便利性为准；模块级文档豁免按 `harness/rules/module-doc-exception-rules.md` 执行。

## 必需路径
- 项目约束：
  - `docs/architecture/principles.md`
  - `docs/architecture/workspace-constraints.md`
  - `docs/architecture/validation-model.md`
- 长期模块边界：
  - `docs/modules/p2p-frame.md`
  - `docs/modules/cyfs-p2p.md`
  - `docs/modules/cyfs-p2p-test.md`
  - `docs/modules/sn-miner.md`
  - `docs/modules/desc-tool.md`
- 版本化数据包：
  - `docs/versions/v0.1/modules/_template/`
  - `docs/versions/v0.1/modules/p2p-frame/`
- 验收输出：
  - `docs/reviews/_template/acceptance-report.md`
  - `docs/versions/v0.1/reviews/`

## 验证入口
- Unit：`python3 ./harness/scripts/test-run.py <module> unit`
- DV：`python3 ./harness/scripts/test-run.py <module> dv`
- Integration：`python3 ./harness/scripts/test-run.py <module> integration`
- 结构检查：`python3 ./harness/scripts/verify-module-packet.py v0.1 <module>`
- 工作区结构检查：`python3 ./harness/scripts/verify-workspace-harness.py v0.1`
- 结构准入检查：`python3 ./harness/scripts/schema-check.py --version v0.1 --module <module>`
- 准入文档哈希：`python3 ./harness/scripts/admission-check.py --version v0.1 --module <module> --print-doc-hashes`
- 改动准入检查：`python3 ./harness/scripts/admission-check.py --version v0.1 --module <module> --change-id <change_id> --evidence-file harness/evidence/admission/<task-id>.md`
- 兼容准入入口：`python3 ./harness/scripts/check-implementation-admission.py v0.1 <module> --evidence-file harness/evidence/admission/<task-id>.md <change_id>`
- 阶段范围检查：`python3 ./harness/scripts/stage-scope-check.py --stage <stage> --version v0.1 --module <module>`
- Harness 自检：`python3 ./harness/scripts/harness-self-check.py`
- 文档结构检查：`python3 ./harness/scripts/doc-structure-check.py --version v0.1 --module <module> --docs <all|mandatory|proposal|design|testing>`
- 测试覆盖检查：`python3 ./harness/scripts/testing-coverage-check.py --version v0.1 --module <module> [--change-id <id>]`
- 质量门禁：`python3 ./harness/scripts/quality-check.py`
- 验收报告检查：`python3 ./harness/scripts/acceptance-report-check.py <report>`
- 自动流水线计划检查：`python3 ./harness/scripts/pipeline-plan-check.py harness/pipeline-plan.md`
- 全量 harness 检查：`python3 ./harness/scripts/check-all.py`
- 全量验证入口：`./test-run.sh all all` 或 `test-run.bat all all`
- 审批状态报告：`python3 ./harness/scripts/report-approval-status.py v0.1`

## 治理索引
- 硬规则：`harness/rules/`
  - 模块级文档豁免：`harness/rules/module-doc-exception-rules.md`
  - 测试设计深度规则：`harness/rules/test-design-rules.md`
- 项目自定义规则：`harness/custom-rules/`（用户拥有；harness refresh 不得修改）
- 阶段执行指南：`harness/process_rules/`
- 评审清单：`harness/checklists/`
- 人类治理规则：`harness/human-rules/`
- 机器可读的工作区治理：`harness/workspace-governance.yaml`
- 质量门禁声明：`harness/quality-gates.yaml`
- Git hook wiring：`harness/hooks/pre-commit`
- CI wiring：`harness/ci/harness-checks.yml`
- Claude Code hook 示例：`harness/claude-settings-hooks.example.json`
- 当前自动流水线计划：`harness/pipeline-plan.md`

## 护栏
- 先文档，后代码，最后验证。
- 除非有已批准的 design 明确要求改变，否则保持现有 crate 边界和当前 runtime/test 命令不变。
- 将 `p2p-frame` 与 `cyfs-p2p` 视为高耦合模块，要求更强证据。
- 不要把生成出来的文件树当作完成证明；acceptance 要求证据与一致性。
