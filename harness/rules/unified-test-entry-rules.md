# 统一测试入口规则

## 目标
- 定义模块工作使用的规范可运行验证入口。

## 范围
- `harness/scripts/test-run.py`
- `testing.md`
- `testplan.yaml`

## 规范命令
- `python3 ./harness/scripts/test-run.py <module> unit`
- `python3 ./harness/scripts/test-run.py <module> dv`
- `python3 ./harness/scripts/test-run.py <module> integration`

## 一致性规则
- `testing.md` 与 `testplan.yaml` 必须引用同一组层级和验证面。
- 测试脚本必须是非交互式的，返回有意义的退出码，并输出它实际执行的具体命令。
- 新的验证路径必须接入规范入口，而不是在任务提示里额外塞入未治理的命令。

## 执行契约
- 未知模块或未知层级必须非零退出。
- 已启用层级必须按声明顺序执行其步骤。
- 只有 `manual` 或 `disabled` 层级可以在不执行命令时仍然返回成功。
- 每个已启用步骤都应声明稳定的机器可读字段，例如 `id`、`name` 与 `run`。

## 当前登记策略
- `p2p-frame` 的 DV 使用本地 all-in-one 场景，因为核心库没有独立的二进制入口。
- 未登记某个层级的模块必须明确失败，而不是静默通过。
