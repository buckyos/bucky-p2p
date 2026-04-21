---
module: cyfs-p2p
version: v0.1
status: draft
approved_by:
approved_at:
---

# cyfs-p2p 设计

## 设计范围
### 目标
- 让适配层边界明确化。
- 将身份/证书适配与栈构建关注点分离。

### 非目标
- 重新定义 `p2p-frame` 内部实现
- 让这个适配层 crate 成为运行时场景的所有者

## 总体方案
- 将 `cyfs-p2p` 视为一个轻薄但高耦合的适配层。
- 后续工作继续按身份适配、栈构建和导出类型行为拆分。
- 通过 crate 本地测试和下游运行时场景共同验证适配层行为。

## 模块拆分
| 子模块 | 类型 | 职责 | 输入 | 输出 | 依赖 | 是否独立文档 |
|--------|------|------|------|------|------|----------------|
| `stack_builder` | adapter | 在 `p2p-frame` 之上构建面向 CYFS 的栈和配置 | CYFS 类型、`p2p-frame` 栈/配置 API | 配置后的 stack 和 env | `p2p-frame` | no |
| `identity_adapter` | adapter | 将 CYFS device/cert/key 语义桥接到 `p2p-frame` 的身份接口 | CYFS 对象和密钥 | `P2pIdentity` 与 `P2pIdentityCert` 实现 | CYFS crates、`p2p-frame` | no |
| `types` | support | 适配层内部共享类型和辅助逻辑 | CYFS 与 `p2p-frame` 值 | 共享转换类型 | 两侧 | no |

## 实现顺序
| 阶段 | 目标 | 前置条件 | 输出 | 依赖 | 可并行 |
|------|------|----------|------|------|--------|
| 1 | 确认适配层范围和公开契约 | 已批准 proposal | 稳定的子模块拆分 | proposal | no |
| 2 | 为身份和栈行为定义验证面 | 已批准 design | `testing.md`、`testplan.yaml` | 阶段 1 | limited |
| 3 | 实现边界明确的适配层改动 | 已批准 testing | 代码与测试 | 阶段 1-2 | yes |

## 接口与依赖
### 公共接口摘要
- 在合适位置重新导出核心能力
- 暴露 CYFS 特定的身份、证书和栈配置行为

### 外部依赖
- `p2p-frame`
- CYFS object/crypto crates

### 运行约束
- 影响运行时启动或 endpoint 处理的适配层改动需要下游证据

## 实现布局
```text
cyfs-p2p/src
├── lib.rs
├── stack_builder.rs
└── types.rs
```

| 路径 | 类型 | 职责 | 备注 |
|------|------|------|------|
| `cyfs-p2p/src/lib.rs` | file | crate 入口和再导出表面 | 公开适配层边界 |
| `cyfs-p2p/src/stack_builder.rs` | file | 身份和栈适配 | 高耦合 |
| `cyfs-p2p/src/types.rs` | file | 共享适配类型 | 对转换敏感 |

## 文档索引
| 文档 | 主题 | 范围 |
|------|------|------|
| `design.md` | 适配层概览 | 完整模块 |

## 当前改动直接映射
| Proposal 条目 | 设计对象 | 代码路径/接口 | 风险/回滚备注 |
|---------------|----------|---------------|----------------|
| `P-1` | 适配层边界与子模块拆分 | `cyfs-p2p/src/lib.rs`、`cyfs-p2p/src/types.rs` | 避免把核心语义重新定义到适配层 |
| `P-2` | 栈构建与身份适配职责 | `cyfs-p2p/src/stack_builder.rs` | 若适配路径漂移需回滚具体实现而不是扩大边界 |

## 风险与回滚
- 回滚应当在撤销具体适配层改动的同时，保留已批准的数据包文档
- 身份/证书回归可能需要与运行时使用方协同验证
