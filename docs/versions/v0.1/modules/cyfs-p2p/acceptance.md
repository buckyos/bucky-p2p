# cyfs-p2p 验收标准

## 验收基线
- 成功与否以已批准的 `proposal.md` 为准。

## 必需证据
- 已批准的 `proposal.md`
- 已批准的 `design.md`
- 已批准的 `testing.md`
- `testplan.yaml`
- `docs/modules/cyfs-p2p.md`
- 实现代码与测试
- 当行为跨越 crate 边界时，需要下游运行时证据

## 验收标准
- 适配层边界与 `p2p-frame` 保持一致
- 身份、证书和栈构建行为已被声明的测试覆盖
- 变更后的行为仍与运行时使用方兼容

## 失败条件
- 适配层语义偏离已批准范围
- 影响运行时的改动缺少 DV 或 integration 证据
- 下游兼容性假设未文档化或未测试
