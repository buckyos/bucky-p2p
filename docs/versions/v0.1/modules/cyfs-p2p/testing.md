---
module: cyfs-p2p
version: v0.1
status: draft
approved_by:
approved_at:
---

# cyfs-p2p 测试

## 测试文档索引
| 文档 | 主题 | 范围 |
|------|------|------|
| `testing.md` | 适配层验证基线 | 完整模块 |

## 统一测试入口
- 机器可读计划：`docs/versions/v0.1/modules/cyfs-p2p/testplan.yaml`
- Unit: `python3 ./harness/scripts/test-run.py cyfs-p2p unit`
- DV: `python3 ./harness/scripts/test-run.py cyfs-p2p dv`
- Integration: `python3 ./harness/scripts/test-run.py cyfs-p2p integration`

## 子模块测试
| 子模块 | 职责 | 详细测试文档 | 必需行为 | 边界/失败场景 | 测试类型 | 测试文件 |
|--------|------|--------------|----------|----------------|----------|----------|
| `identity_adapter` | CYFS 身份与证书桥接 | none | 身份编码/解码、证书校验、endpoint 更新 | 无效证书/签名、endpoint 不匹配 | unit + integration | `cyfs-p2p/src/stack_builder.rs` |
| `stack_builder` | 构建并配置 stack/env | none | 栈创建与配置转换 | 配置不匹配、运行时 feature 假设 | unit + DV | `cyfs-p2p/src/stack_builder.rs` |
| `types` | 共享适配辅助逻辑 | none | 辅助逻辑一致性和转换语义 | 序列化/hash 边界场景 | unit | `cyfs-p2p/src/types.rs` |

## 模块级测试
| 测试项 | 覆盖边界 | 入口 | 预期结果 | 测试类型 | 测试文件/脚本 |
|--------|----------|------|----------|----------|-----------------|
| 适配层 crate 测试集 | 适配层本地行为 | `cargo test -p cyfs-p2p` | crate 测试通过 | unit | crate 本地测试 |
| 运行时适配场景 | 本地场景中的适配层行为 | `cargo run -p cyfs-p2p-test -- all-in-one` | 场景启动并走通适配层路径 | DV | `cyfs-p2p-test/src/main.rs` |
| 工作区兼容性 | 下游使用方仍能编译/测试 | `cargo test --workspace` | 工作区测试通过 | integration | workspace |

## 外部接口测试
| 接口 | 职责 | 成功场景 | 失败/边界场景 | 测试类型 | 测试文档/文件 |
|------|------|----------|----------------|----------|----------------|
| `p2p-frame` 边界 | 在无语义漂移的前提下消费核心 API | 栈和身份适配成功 | 核心行为变化破坏适配层假设 | integration | workspace |
| 运行时使用方边界 | 支持 `cyfs-p2p-test` 和 `sn-miner` | 运行时使用方能构建并运行关键入口 | 启动/配置回归 | DV + integration | `cyfs-p2p-test/src/main.rs`、`sn-miner-rust/src/main.rs` |

## 当前改动直接验证
| Design 条目 | 验证层级 | 入口/步骤 ID | 覆盖说明 | 缺口/原因 |
|-------------|----------|--------------|----------|-----------|
| 适配层边界与子模块拆分 | unit | `cargo test -p cyfs-p2p` | 验证 crate 本地适配行为仍可执行 | 无 |
| 栈构建与身份适配职责 | DV / integration | `cargo run -p cyfs-p2p-test -- all-in-one`、`cargo test --workspace` | 验证运行时使用方和工作区兼容性 | 无 |

## 回归关注点
- `stack_builder.rs` 中的身份与证书适配
- `lib.rs` 中的公开再导出行为
- 混合 CYFS/核心 endpoint 处理

## 完成定义
- [ ] 直接子模块已映射到验证面
- [ ] `testplan.yaml` 与声明的命令一致
- [ ] 影响运行时的适配层改动具备下游证据
- [ ] 已检查跨模块兼容性
