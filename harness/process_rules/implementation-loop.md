# 实现循环

## 目标
- 定义实现准入开放后，默认采用的开发者迭代循环。

## 前置条件
- `python3 ./harness/scripts/check-implementation-admission.py <version> <packet-module> --submodule <task-name> --target-module <module> --evidence-file docs/versions/<version>/evidence/admission/<task-id>.md <change_id>` 校验通过
- 当前改动已经能映射到 proposal 与 design 中的直接条目
- implementation 与 post-implementation testing 默认分阶段；只有用户显式授权跨阶段同步时才可在同一任务中处理两类制品，并记录该授权。

## 循环步骤
1. 读取已批准的 proposal 与 design，确认当前改动边界。
2. 仅修改生产代码和必需的非测试运行时/构建资源。
3. 只有在用户明确要求、调试需要证据，或上游任务/仓库规则要求时，才运行对应的规范验证入口。
4. 若运行了验证，则检查日志、运行时产物或编译/测试输出，并继续修正代码。
5. 重复以上步骤，直到达到声明的通过条件或把问题退回上游阶段。

## 工作区特定说明
- 如果任务明确要求局部 unit 验证，优先使用 `python3 ./harness/scripts/test-run.py <module> unit`。
- 如果运行时行为受影响且任务明确要求 DV，使用 `python3 ./harness/scripts/test-run.py <module> dv` 路由到规范场景。
- 如果下游 crate 受影响且任务明确要求 integration，使用 `python3 ./harness/scripts/test-run.py <module> integration`。
- 将 `logs/`、`devices/`、`profile/` 和 `sn/` 视为运行时证据或本地状态，而不是阶段文档的替代品。

## 禁止行为
- 不要在实现任务中更新 proposal/design/testing 文档。
- 不要在 implementation 阶段顺手生成测试代码，除非用户明确要求合并 implementation/testing。
- 不要把“习惯性自测”当成实现阶段默认动作。
