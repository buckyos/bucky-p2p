---
task_manifest: task.yaml
status: approved
---

# 修复非规则 Harness 工具的 Implementation 阶段范围分类

Risk profile: ./risk-profile.yaml

## Workflow Tier Judgment
- Proposed tier: high-risk
- Final tier: high-risk
- Tier rationale / triggered boundaries: 本任务改变 Harness 阶段归属规则及其机械检查器，直接影响后续任务能否完成 implementation scope gate，命中 material Harness-process boundary。
- Proposal and tier confirmation: confirmed by user launch statement `确认，自动完成`

## Background and Goal

任务 030 按规则将 `context.py`、`task-index.py` 和 `task_manifest.py` 作为非规则 Harness 工具送入正常高风险工作流，但本地与当前 bootstrap kit 的 `stage-scope-check.py` 又无条件拒绝 implementation manifest 中的全部 `harness/` 路径。因此规则要求“正常任务流”，机械门禁却不给任何合法交付阶段，形成自举死锁。

目标是明确非规则 Harness 工具属于可治理的 implementation 产物，并让 checker 仅在带具体 sibling task、`packet_module: globals` 与具体 `target_module` 的 implementation 检查中接受 `harness/scripts/`；规则政策目录、自定义规则、阶段文档、测试产物和普通产品任务仍保持原有边界。

## Scope

### In scope

- 更新 `harness/rules/task-entry-gate-rules.md`，明确非规则 Harness 工具经 globals packet 进入 implementation。
- 更新 `harness/rules/implementation-rules.md`，定义 `harness/scripts/` 在上述窄条件下属于 implementation 阶段产物。
- 更新 `harness/scripts/stage-scope-check.py`，机械实现同一分类。
- 验证 globals 正例以及非-globals、规则目录、testing/acceptance 阶段负例。
- 修复通过后恢复并完成任务 030 的 implementation、testing 与 acceptance。

### Out of scope

- 不修改 `harness/custom-rules/`。
- 不允许 implementation 修改 `harness/rules/`、`harness/custom-rules/`、`harness/process_rules/`、`harness/checklists/` 或 `harness/human-rules/`。
- 不把 design `Scope Paths` 转化为文件读写权限或第二个路径授权门禁。
- 不放宽 testing、acceptance、proposal 或 design 的阶段产物分类。
- 不修改产品代码、测试 runner 或其他 Harness checker。

### Boundary with neighboring modules

修复只改变工作区级 Harness 流程的产物分类，不改变 `p2p-frame`、`cyfs-p2p` 或其他产品模块行为。两个 concrete target 仅用于证明 globals packet 的跨模块绑定保持明确。

## Requirement Review

用户选择修正规则与 checker，而不是跳过 Harness。这个方向能消除根因，但必须避免把 `harness/` 整体开放为 implementation 产物。最窄方案是要求 `packet_module: globals`、非空 sibling task、具体非-globals `target_module`，并只放行 `harness/scripts/`；规则和人工治理目录继续拒绝。checker 在本任务完成时用修复后的自身执行 scope check，这是一次受 proposal、admission、精确 manifest 和负向用例约束的 self-hosting 更新。

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-001 | define_harness_tooling_implementation_artifacts | 规则明确非规则 Harness scripts 在 globals 高风险 packet 中属于 implementation 产物 | 仅 `harness/scripts/`；要求 globals packet、具体 sibling task 与 concrete target module | 增加一种明确的流程工具实现类别，换取消除自举死锁 | 规则索引、自检和正负 stage-scope 契约验证通过 | 不把规则政策或任意 `harness/` 路径归入 implementation |
| P-002 | enforce_globals_harness_tooling_scope | checker 对上述窄条件放行，并继续拒绝非-globals 任务与 Harness 政策目录 | 不使用 design Scope Paths 作授权；不改变其他阶段 | checker 需要识别 packet module 上下文，换取规则与机械执行一致 | globals scripts 正例通过；普通模块 scripts、globals rules、testing scripts 与 acceptance scripts 负例均失败 | 不改变产品路径、测试路径或文档阶段分类 |

## Success Criteria

- Concrete user-visible or system-visible result: 任务 030 的三个 `harness/scripts/` 路径能通过 implementation scope check，而规则目录和普通产品 packet 不能借此放行 Harness 工具。
- Required evidence: 精确正负矩阵、`context.py --validate-index`、`harness-self-check.py`、workspace verification、任务 031 的自托管 stage-scope check 以及恢复后的任务 030 完整流水线证据。
- Explicit non-goals: 不声明所有 Harness 工具类别都已重新设计，不修改 bootstrap kit 全局资产，不触碰自定义规则或产品代码。

## Risks

- Gate-bypass risk: 条件过宽会让普通实现任务把 Harness 工具混入阶段 manifest；通过 globals + sibling + concrete target + scripts-only 条件收窄。
- Self-hosting risk: checker 用修改后的自身验证自身；必须由未修改前的 schema/admission 先约束任务，再以独立负向矩阵证明拒绝边界仍在。
- Drift risk: 当前项目修复不自动修改全局 skill/bootstrap kit；验收只声明本仓库已恢复一致性。
