# 实现准入规则

## 目标
- 定义实现或 bugfix 工作启动前的硬性前提。

## 范围
- implementation 任务
- bugfix 任务
- 某个版本化模块中的代码与测试代码变更

## 必需输入
- `docs/versions/<version>/modules/<module>/proposal.md`
- `docs/versions/<version>/modules/<module>/design.md`
- `docs/versions/<version>/modules/<module>/testing.md`

## 准入规则
- 在所有必需输入存在之前，implementation 不得开始。
- 在这三个输入全部为 `status: approved` 之前，implementation 不得开始。
- bugfix 任务遵循同样规则，除非仓库在版本化规则中发布了更窄的例外路径。

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

## 退回路由
- proposal 缺失或仍为 draft：退回 proposal
- design 缺失或仍为 draft：退回 design
- testing 缺失或仍为 draft：退回 testing
- 实现期间发现上游矛盾：退回对应的上游责任阶段，而不是就地修补文档
