# P2P 工作区 Agent 地图

## 概览
- 本仓库是一个 Rust workspace，包含点对点网络栈、CYFS 集成、运行时验证工具及其配套二进制。
- 仓库采用严格的 Harness Engineering 工作流，以 proposal-first 验收和硬性的实现准入门禁为核心。

## 工作模型
- 由人类定义意图、范围、约束和审批边界。
- Agent 只能在仓库定义的阶段边界内执行。
- Acceptance 是对完整证据链的独立审计，与 implementation 分离。
- Auto-pipeline 模式已启用：一个已批准的 proposal 可以触发后续的 planning、design、testing、implementation 和 acceptance 任务。

## 任务读取顺序
1. 先读本文件。
2. 再读 `docs/versions/<version>/modules/<module>/` 下当前活跃的模块数据包。
3. 读取 `docs/modules/<module>.md` 了解长期模块边界。
4. 读取 `docs/architecture/` 了解工作区级约束。
5. 读取 `harness/rules/` 了解硬性不变量和门禁。
6. 当任务类型或模块级别要求时，再读取 `harness/process_rules/`、`harness/checklists/` 和 `harness/human-rules/`。

## 阶段边界
- Proposal 职责：把用户意图转化为可审批的目标、范围、非目标、约束与风险边界基线。Proposal 任务只能修改 `proposal.md`。
- 流水线规划职责：在下游执行开始前创建阶段图、依赖、输出和退回路径。Planning 任务只能修改 `harness/pipeline-plan.md` 和任务制品。
- Design 职责：把已批准的 proposal 转化为可执行结构、接口、模块拆分和实现顺序。Design 任务只能修改 `design.md`、`design/`，以及必须同步的 `docs/modules/<module>.md` 边界说明。
- Testing 职责：把已批准的 design 转化为可运行的验证覆盖、执行入口和通过标准。Testing 任务只能修改 `testing.md`、`testing/` 和 `testplan.yaml`。
- Implementation 职责：交付满足已批准 proposal、design 与 testing 输入的最小代码和测试变更。Implementation 任务只能修改代码和测试代码。
- Acceptance 职责：独立审计 proposal、design、testing、implementation、测试与结果是否仍然一致。Acceptance 任务只写评审报告。

## 硬门禁
- 除 `harness/rules/module-doc-exception-rules.md` 中显式列出的模块级文档豁免外，在 `proposal.md`、`design.md` 与 `testing.md` 全部存在且标记为 `status: approved` 之前，implementation 和 bugfix 工作都不得开始。
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
- 准入检查：`python3 ./harness/scripts/check-implementation-admission.py v0.1 <module>`
- 审批状态报告：`python3 ./harness/scripts/report-approval-status.py v0.1`

## 治理索引
- 硬规则：`harness/rules/`
  - 模块级文档豁免：`harness/rules/module-doc-exception-rules.md`
- 阶段执行指南：`harness/process_rules/`
- 评审清单：`harness/checklists/`
- 人类治理规则：`harness/human-rules/`
- 机器可读的工作区治理：`harness/workspace-governance.yaml`
- 当前自动流水线计划：`harness/pipeline-plan.md`

## 护栏
- 先文档，后代码，最后验证。
- 除非有已批准的 design 明确要求改变，否则保持现有 crate 边界和当前 runtime/test 命令不变。
- 将 `p2p-frame` 与 `cyfs-p2p` 视为高耦合模块，要求更强证据。
- 不要把生成出来的文件树当作完成证明；acceptance 要求证据与一致性。
