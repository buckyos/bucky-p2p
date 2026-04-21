# 自动流水线规则

## 目标
- 定义 proposal 获批后，仓库如何自动推进下游工作流。

## 触发条件
- 启动信号：用户确认某个已批准的 `proposal.md`
- 规划输出：`harness/pipeline-plan.md`

## 验收基线
- 最终验收基线：已批准的 `proposal.md`
- `design.md`、`testing.md` 与 `acceptance.md` 可以细化执行细节，但不得与 proposal 冲突、缩窄其范围，或静默扩展其范围

## 阶段职责
- Proposal：定义目标、范围、非目标和约束的已批准基线
- 流水线规划：在执行开始前规划任务图、依赖、输出和退回路由
- Design：把已批准 proposal 转成可执行的结构与接口
- Testing：把已批准 design 转成可运行的验证覆盖与入口
- Implementation：在已批准边界内交付代码和测试改动
- Acceptance：审计证据链，并把失败路由回正确的前置阶段

## 规划规则
- 流水线在执行开始前，必须先创建 design、testing、implementation 和 acceptance 任务。
- 每个阶段任务必须声明：
  - task id
  - stage
  - responsibility
  - scope
  - dependencies
  - outputs
  - done condition
- 如果 `design.md` 标识出可分离负责的直接子模块，计划应当包含子模块任务。

## 实现准入规则
- 任一 implementation 任务在以下条件满足前都不得启动：
  - `proposal.md` 存在且 `status: approved`
  - `design.md` 存在且 `status: approved`
  - `testing.md` 存在且 `status: approved`
- implementation 任务必须读取这些已批准文档，并确认当前任务范围能直接映射到 proposal、design 与 testing 的具体条目。
- 若 `testplan.yaml` 缺失，或 testing 制品尚未声明当前任务的验证入口/缺口说明，implementation 任务不得启动。
- 若 implementation 任务被阻塞，必须退回到对应的前置责任阶段，而不是在部分假设下继续推进。

## 退回路由规则
- 验收失败不代表流水线完成。
- 流水线必须重新打开或重新创建正确的前置阶段任务，并附带：
  - 阻塞问题 ID
  - 责任阶段
  - 证据
  - 期望修复输出

## 退出条件
- 仅当以下条件全部满足时，流水线才结束：
  - proposal 定义的结果已满足
  - 阻塞问题已关闭
  - 必需的测试证据存在
  - 最终验收通过
