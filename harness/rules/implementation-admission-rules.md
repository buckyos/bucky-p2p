# 实现准入规则

## 目标
- 定义实现或 bugfix 工作启动前的硬性前提。

## 范围
- implementation 任务
- bugfix 任务
- 某个版本化模块中的代码与测试代码变更

## 必需输入
- 对默认模块：
  - `docs/versions/<version>/modules/<module>/proposal.md`
  - `docs/versions/<version>/modules/<module>/design.md`
  - `docs/versions/<version>/modules/<module>/testing.md`
  - `docs/versions/<version>/modules/<module>/testplan.yaml`
- 对 `harness/rules/module-doc-exception-rules.md` 中列出的模块：
  - 机器可读治理中已登记该豁免
  - 当前改动确属该模块自身的本地 harness/工具改动，而不是跨模块契约变更

## 准入规则
- 对默认模块，在所有必需输入存在之前，implementation 不得开始。
- 对默认模块，在这三个输入全部为 `status: approved` 之前，implementation 不得开始。
- 对默认模块，`status: approved` 不是充分条件；implementation 与 bugfix 任务必须读取这些已批准文档，并确认当前改动能直接映射到 proposal、design 与 testing 中的具体条目。
- bugfix 任务遵循同样规则，除非仓库在版本化规则中发布了更窄的例外路径。
- 如果当前改动无法映射到直接的 proposal、design 或 testing 条目，implementation 与 bugfix 不得只依赖模块总览、历史设计说明、聊天说明或口头解释启动。
- 如果问题相关的已批准文档未定义实现所需的逻辑、约束、接口或流程，implementation 与 bugfix 不得以对话中的用户直接说明替代文档依据。
- 对默认模块，如果 `testplan.yaml` 缺失，或当前改动对应的验证入口/缺口说明未在 testing 制品中声明，implementation 不得开始。
- 对 `cyfs-p2p-test`，若改动仅限本模块运行时 harness，不要求 proposal/design/testing/testplan 准入；但一旦改动同时改变相邻模块契约、默认值、验证语义或长期边界，必须回到受影响模块的文档阶段。
- 遇到上述缺口时，必须退回对应的 proposal、design 或 testing 阶段补齐文档，并在文档成为实现依据后再继续代码修改。

## 允许的改动
- 代码
- 测试代码

## 禁止的改动
- `proposal.md`
- `design.md`
- `design/`
- `testing.md`
- `testing/`
- `testplan.yaml`
- `acceptance.md`

## 机械化执行
- 在实现开始前运行 `python3 ./harness/scripts/check-implementation-admission.py <version> <module>`。
- 准入检查失败会阻塞任务，而不是仅供参考。

## 默认验证策略
- 工作区默认采用严格实现模式：implementation 阶段不主动把测试执行当成例行动作。
- 只有以下情况允许在实现阶段运行验证命令：
  - 用户明确要求验证
  - 调试需要新的失败证据
  - 已批准的 testing 制品或仓库规则要求必须先跑某个验证入口
- “顺手跑一下”、“最小自测” 或类似习惯性理由，不构成例外。

## 退回路由
- proposal 缺失或仍为 draft：退回 proposal
- design 缺失或仍为 draft：退回 design
- testing 缺失或仍为 draft：退回 testing
- `testplan.yaml` 缺失或 testing 中未声明当前改动对应的验证路径：退回 testing
- proposal 缺少当前改动的直接目标或约束锚点：退回 proposal
- design 缺少当前改动的结构、接口或边界映射：退回 design
- testing 缺少当前改动的直接验证覆盖或明确缺口记录：退回 testing
- `cyfs-p2p-test` 改动实际影响到相邻模块契约，却试图走文档豁免：退回受影响模块的 proposal/design/testing
- 实现期间发现上游矛盾：退回对应的上游责任阶段，而不是就地修补文档
