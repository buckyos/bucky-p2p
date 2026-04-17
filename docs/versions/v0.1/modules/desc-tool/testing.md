---
module: desc-tool
version: v0.1
status: draft
approved_by:
approved_at:
---

# desc-tool 测试

## 测试文档索引
| 文档 | 主题 | 范围 |
|------|------|------|
| `testing.md` | 工具验证基线 | 完整模块 |

## 统一测试入口
- 机器可读计划：`docs/versions/v0.1/modules/desc-tool/testplan.yaml`
- Unit: `python3 ./harness/scripts/test-run.py desc-tool unit`
- DV: `python3 ./harness/scripts/test-run.py desc-tool dv`
- Integration: `python3 ./harness/scripts/test-run.py desc-tool integration`

## 子模块测试
| 子模块 | 职责 | 详细测试文档 | 必需行为 | 边界/失败场景 | 测试类型 | 测试文件 |
|--------|------|--------------|----------|----------------|----------|----------|
| `create` | 创建 descriptor 制品 | none | 正确创建制品 | 参数无效、输出路径错误 | unit + DV | `desc-tool/src/create.rs` |
| `show` | 检查制品 | none | 渲染已知制品 | 不支持的类型、错误文件 | unit + DV | `desc-tool/src/show.rs` |
| `modify` | 修改制品 | none | 应用受支持的修改 | 字段/值无效、输入格式错误 | unit | `desc-tool/src/modify.rs` |
| `sign` | 对制品签名 | none | 生成有效签名输出 | 密钥/签名失败 | unit + DV | `desc-tool/src/sign.rs` |
| `calc` | 计算 nonce/派生输出 | none | 处理受支持的对象类型 | 不支持的对象类型 | unit | `desc-tool/src/main.rs` |

## 模块级测试
| 测试项 | 覆盖边界 | 入口 | 预期结果 | 测试类型 | 测试文件/脚本 |
|--------|----------|------|----------|----------|-----------------|
| crate 本地测试 | 工具本地行为 | `cargo test -p desc-tool` | crate 测试通过 | unit | crate 本地测试 |
| CLI DV 探测 | 二进制入口仍可调用 | `cargo run -p desc-tool -- --help` | 进程成功退出并输出帮助文本 | DV | `desc-tool/src/main.rs` |
| 工作区兼容性 | 工具在工作区中仍可构建 | `cargo test --workspace` | 工作区测试通过 | integration | workspace |

## 外部接口测试
| 接口 | 职责 | 成功场景 | 失败/边界场景 | 测试类型 | 测试文档/文件 |
|------|------|----------|----------------|----------|----------------|
| 操作员 CLI 边界 | 暴露稳定子命令 | help 和分发成功 | 子命令错误或缺少参数 | DV | `desc-tool/src/main.rs` |
| 文件输出边界 | 生成安全制品 | create/modify/sign 行为符合预期 | 输出格式错误或不兼容 | unit + DV | desc 文件与命令流程 |

## 回归关注点
- 签名流程
- 制品修改正确性
- calc/show 路径对不支持对象的处理

## 完成定义
- [ ] 影响输出的改动具备明确、可评审的证据
- [ ] 已明确标出安全敏感流程
- [ ] `testplan.yaml` 与声明的命令一致
