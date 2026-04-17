---
module: example-module
version: v0.1
status: draft
approved_by:
approved_at:
---

# 示例模块测试

## 测试文档索引
| 文档 | 主题 | 范围 |
|------|------|------|
| none | 目前尚未拆分测试文档 | 完整模块 |

## 统一测试入口
- 机器可读计划：`docs/versions/v0.1/modules/example-module/testplan.yaml`
- Unit: `python3 ./harness/scripts/test-run.py example-module unit`
- DV: `python3 ./harness/scripts/test-run.py example-module dv`
- Integration: `python3 ./harness/scripts/test-run.py example-module integration`

## 子模块测试
| 子模块 | 职责 | 详细测试文档 | 必需行为 | 边界/失败场景 | 测试类型 | 测试文件 |
|--------|------|--------------|----------|----------------|----------|----------|
| | | | | | | |

## 模块级测试
| 测试项 | 覆盖边界 | 入口 | 预期结果 | 测试类型 | 测试文件/脚本 |
|--------|----------|------|----------|----------|-----------------|
| | | | | | |

## 外部接口测试
| 接口 | 职责 | 成功场景 | 失败/边界场景 | 测试类型 | 测试文档/文件 |
|------|------|----------|----------------|----------|----------------|
| | | | | | |

## 回归关注点
<!-- 高风险区域和历史缺陷区域 -->

## 完成定义
- [ ] Testing 覆盖全部直接子模块，或已说明遗漏原因
- [ ] `testplan.yaml` 与声明的命令一致
- [ ] 已存在模块级和外部接口检查
- [ ] 已包含触发的额外检查
