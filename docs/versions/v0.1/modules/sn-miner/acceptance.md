# sn-miner 验收标准

## 验收基线
- 成功与否以已批准的 `proposal.md` 为准。

## 必需证据
- 已批准的 `proposal.md`
- 已批准的 `design.md`
- 已批准的 `testing.md`
- `testplan.yaml`
- `docs/modules/sn-miner.md`
- 影响启动行为的运行时证据

## 验收标准
- 启动默认值和 desc/sec 处理保持明确
- 配置和运行时变更触发了必需的额外检查
- 服务启动仍然符合已批准范围

## 失败条件
- 启动行为变化但没有 DV 证据
- 生成制品语义发生变化，但没有可评审的 design/testing 更新
- 配置默认值静默漂移
