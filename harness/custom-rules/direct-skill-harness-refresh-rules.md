# Skill 驱动 Harness 刷新直接执行规则

## 目标

- 当当前用户显式要求使用 `harness-engineering` skill 安装、更新、刷新、同步或移除项目 Harness 脚手架时，直接完成该维护，不为 Harness 自身再创建 task packet。
- 避免 Harness 工具缺失或规则/脚本版本漂移导致“先为修 Harness 建任务、该任务又被缺失 Harness 门禁阻塞”的自举循环。

## 显式触发条件

- 仅当当前用户明确要求更新、刷新、同步、安装或移除项目 Harness，且明确指向 `harness-engineering` skill 或 skill 提供的 Harness 脚手架时生效。
- 不得仅因产品任务碰到 Harness 检查失败、某个 Harness 文件缺失、用户要求继续、任务紧急或历史上执行过 refresh 而推断触发。
- 当前用户只要求修改某一条 Harness 规则时，直接维护范围仍限制在该规则及其必要索引；不得借此进行全量 scaffold refresh。

## 直接维护范围

- skill 管理的 `AGENTS.md` Harness 导航内容。
- `harness/rules/`、规则索引和触发规则。
- `harness/scripts/` 中由 skill 提供的 Harness 执行器、checker 与共享 helper。
- `harness/process_rules/`、`harness/checklists/`、`harness/human-rules/` 和 `harness/templates/` 中由 skill 管理的脚手架。
- Harness task/document/review 模板、`docs/changes/_template.md`、`harness/quality-gates.yaml`、根测试快捷入口，以及 `.harness/` / `.gitignore` 的必要运行态接线。
- 为使上述刷新后的 Harness 自洽、可运行而必需的窄适配与验证。

## 执行规则

- 在创建或选择 task packet、分配序号、分类 tier/stage、请求 proposal 确认、运行 task lifecycle 或写 admission evidence 之前应用本规则。
- 命中本规则的 Harness 刷新直接检查当前仓库与 skill 资产，执行最小兼容更新，并运行针对 Harness 自身的结构、索引、正负契约和自检。
- 不得为该 Harness 刷新创建 `task.yaml`、`proposal.md`、change record、risk profile、design/testing/acceptance 制品或 unfinished-task index 条目。
- skill 管理的生成文件可以按当前 skill 版本刷新；仓库适配必须保留，除非已被新版通用实现替代。
- `harness/custom-rules/` 是用户拥有的政策区。除当前用户明确点名的自定义规则外，刷新必须逐字保留其现有内容和索引条目。
- 如果显式 Harness 刷新用于解除一个已存在产品任务的门禁，先直接完成并验证 Harness 刷新，再恢复原产品任务；不得为刷新再建立 sibling 或 globals Harness 任务。

## 混合请求边界

- 同一请求中的 Harness 刷新按本规则直接执行；产品代码、产品测试、运行时、构建、发布或业务文档变更仍按其自身任务规则处理。
- 直接刷新不批准、关闭或扩大任何已有产品 task，也不把 Harness 自检结果当作产品测试或 acceptance 证据。
- 本规则覆盖生成的 `task-entry-gate-rules.md` 和 `implementation-rules.md` 中要求非规则 Harness tooling 进入普通任务流的默认政策，但只覆盖上述“当前用户显式 skill 驱动 Harness 刷新”条件。
- 本规则不覆盖 system/developer 指令、安全要求、文件系统权限或当前用户仍保留的产品范围约束。
