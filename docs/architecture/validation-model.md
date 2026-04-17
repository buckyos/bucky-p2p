# 验证模型

## 目标
- 让验证过程可观察、可由机器执行，并在模块间保持一致。

## 验证面
- 人类可读：
  - `testing.md`
  - `acceptance.md`
  - `docs/versions/<version>/reviews/` 下的验收报告
- 机器可读：
  - `testplan.yaml`
  - `python3 ./harness/scripts/test-run.py <module> <level>`
  - `python3 ./harness/scripts/verify-module-packet.py <version> <module>`
  - `python3 ./harness/scripts/check-implementation-admission.py <version> <module>`

## 必需层级
- `unit`：crate 内局部行为与直接子模块逻辑。
- `dv`：单模块开发验证或可运行场景验证。
- `integration`：相邻模块或工作区级契约验证。

## 一致性规则
- `testing.md` 必须解释 `testplan.yaml` 所声明的同一组验证面。
- Acceptance 必须把实现与结果回溯到已批准的 proposal。
- 任何必需文件缺失，或缺少必需元数据，模块数据包都视为不完整。

## 阶段门禁
- 在 implementation 之前：
  - `proposal.md`、`design.md` 与 `testing.md` 全部存在
  - 且每个文档都为 `status: approved`
- 在 DV 之前：
  - 模块数据包存在
  - unit 计划存在
  - 所需运行时前置条件已记录
- 在 integration 之前：
  - 已声明 unit 与 DV 验证面
  - `testing.md` 中已点名跨模块契约

## 失败路由
- 需求矛盾：退回 proposal。
- 结构或接口矛盾：退回 design。
- 覆盖或计划矛盾：退回 testing。
- 代码或结果矛盾：退回 implementation。

## 严格性策略
- 缺失证据是失败，不是警告。
- 只有在 `testplan.yaml` 中明确标记并记录原因时，才允许手工验证。
- 临时命令不算规范验证，除非它们通过 `harness/scripts/test-run.py` 路由执行。
