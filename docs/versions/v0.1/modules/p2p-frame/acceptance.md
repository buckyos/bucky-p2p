# p2p-frame 验收标准

## 验收基线
- 最终成功与否以已批准的 `proposal.md` 为准。
- `design.md`、`testing.md` 和 `testplan.yaml` 负责把 proposal 落地，但不得与其冲突。

## 必需证据
- 已批准的 `proposal.md`
- 已批准的 `design.md`
- 已批准的 `testing.md`
- `testplan.yaml`
- `docs/modules/p2p-frame.md`
- `p2p-frame/docs/` 下相关设计说明
- 实现代码与测试
- 实际的 unit、DV 和 integration 结果

## 验收标准
- 核心库边界、子模块拆分和已索引的设计说明保持一致。
- `testing.md` 中的直接子模块覆盖映射到了 `testplan.yaml` 中实际可运行的命令。
- 协议、密码学、运行时和配置敏感改动都应用了 trigger 规则。
- 对 `cyfs-p2p` 或运行时二进制有影响的行为具备下游 integration 证据。
- 任何失败都能明确归属并路由回 proposal、design、testing 或 implementation。

## 失败条件
- 必需的上游阶段文档缺少批准
- 跨 crate 改动缺少运行时或 integration 证据
- 设计说明和实现不再对协议行为保持一致
- 触发的额外检查被省略，且没有已批准的例外
- 验收报告无法把阻塞问题映射到责任阶段
