# 模块验收清单

- [ ] `proposal.md` 与 `design.md` 全部存在；`testing.md`、`testplan.yaml` 与 `acceptance.md` 在生成时结构有效
- [ ] proposal、design 的元数据齐全，并在 implementation 前已获批准；可选 testing 元数据在生成时有效
- [ ] 长期模块边界文档存在且与当前数据包一致
- [ ] 设计文档已为所依赖的外部设计说明建立索引
- [ ] 测试覆盖每个直接子模块，或明确说明排除原因
- [ ] `testplan.yaml` 声明了 unit、DV、integration 命令
- [ ] 本轮实现改动可以回溯到直接的 proposal、design 条目；测试实现覆盖这些改动或记录明确缺口
- [ ] 已记录触发的额外检查
- [ ] 实际测试结果已附上或给出链接
- [ ] 验收结论将每个阻塞问题路由回正确阶段
- [ ] 最终结论明确写明通过或失败
