# 模块文档豁免规则

## 目标
- 为少数明确声明的模块提供比默认 Harness 工作流更窄的文档豁免路径。

## 范围
- `harness/workspace-governance.yaml` 中 `implementation_gate.document_exempt_modules` 列出的模块
- 当前工作区中唯一启用该豁免的模块：`cyfs-p2p-test`

## 规则
- `cyfs-p2p-test` 的本模块代码与测试改动，不要求先补齐或审批自身模块数据包中的 `proposal.md`、`design.md`、`testing.md` 与 `testplan.yaml`。
- 对上述本模块改动，默认也不要求单独补写本模块的 acceptance 文档或评审报告。
- 该豁免只作用于 `cyfs-p2p-test` 自身作为运行时 harness 的改动；如果改动同时改变 `cyfs-p2p`、`p2p-frame` 或其他模块的公开契约、默认值、验证语义或长期边界，受影响模块仍必须按各自模块数据包走完整文档阶段。
- `cyfs-p2p-test` 即使享受文档豁免，也仍然受触发规则、统一验证入口和用户明确指定的验证要求约束。

## 机械化要求
- `harness/workspace-governance.yaml` 必须把豁免模块记录在 `implementation_gate.document_exempt_modules`。
- `python3 ./harness/scripts/schema-check.py --version <version> --module <module>` 与 `python3 ./harness/scripts/admission-check.py --version <version> --module <module> --change-id <change_id>` 必须对这些模块放行，而不是继续要求阶段文档审批。
- 兼容入口 `python3 ./harness/scripts/check-implementation-admission.py <version> <module> <change_id>` 必须沿用同一豁免。
- 人类治理矩阵和顶层 Agent 地图必须显式反映该豁免，避免与默认硬门禁冲突。

## 失败处理
- 把跨模块契约改动误报成 `cyfs-p2p-test` 本地 harness 改动：退回 proposal/design/testing。
- 治理文档或脚本未同步该豁免：先修治理层，再执行实现。
