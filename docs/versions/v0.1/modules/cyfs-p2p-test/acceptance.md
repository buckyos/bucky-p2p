# cyfs-p2p-test 验收标准

## 验收基线
- 成功与否以已批准的 `proposal.md` 为准。

## 必需证据
- 已批准的 `proposal.md`
- 已批准的 `design.md`
- 已批准的 `testing.md`
- `testplan.yaml`
- `docs/modules/cyfs-p2p-test.md`
- 受影响场景路径的运行时证据

## 验收标准
- CLI 和场景边界保持明确
- 本地制品前置条件已文档化
- 运行时 harness 的改动不能被拿来冒充其他模块缺口的证明

## 失败条件
- 场景语义改变但没有更新 testing 证据
- 环境前置条件是隐含的
- 依赖它的模块数据包现在依赖了未声明的运行时行为
