# 测试文档规则

## 目标
- 定义 `testing.md`、`testing/` 与 `testplan.yaml` 的最低结构和审批要求。

## 范围
- `docs/versions/<version>/modules/<module>/testing.md`
- `docs/versions/<version>/modules/<module>/testing/`
- `docs/versions/<version>/modules/<module>/testplan.yaml`

## 必需元数据
- `module`
- `version`
- `status`
- `approved_by`
- `approved_at`

## 必需内容
- 从 `design.md` 映射出的直接子模块覆盖
- 模块级验证
- 外部接口验证
- 完成定义
- 与 `testplan.yaml` 对齐的规范 unit、DV、integration 入口
- 面向当前改动的直接验证覆盖
- 没有直接验证时的明确缺口记录

## 护栏
- Testing 必须把已批准的 proposal 与 design 意图转化为可执行验证。
- Testing 任务不得重写 proposal、design 或 implementation 制品。
- 三个层级都必须声明。若某一层级被禁用或只能手工执行，必须在 `testplan.yaml` 和验收评审中明确说明原因。
- 每个 implementation-ready 的改动都必须有直接验证覆盖，或者在 `testing.md` 与 `testplan.yaml` 中留下明确缺口和原因。
- 如果 testing 发现上游设计问题，应退回 design，而不是静默扩大范围。
