---
module: desc-tool
version: v0.1
status: draft
approved_by:
approved_at:
---

# desc-tool 设计

## 设计范围
### 目标
- 按职责拆分子命令族
- 显式标识安全敏感的输出路径

### 非目标
- 重新定义底层 CYFS 对象格式
- 把这个工具变成运行时服务

## 总体方案
- 将 CLI 视为稳定入口
- 后续工作按 create/show/modify/sign/calc 流程拆分
- 对影响输出的改动要求面向操作员的 DV 和更强评审

## 模块拆分
| 子模块 | 类型 | 职责 | 输入 | 输出 | 依赖 | 是否独立文档 |
|--------|------|------|------|------|------|----------------|
| `create` | tooling | 创建 descriptor 制品 | CLI 参数 | 新文件 | CYFS object crates | no |
| `show` | tooling | 检查 descriptor 内容 | 文件 | 人类可读输出 | CYFS object crates | no |
| `modify` | tooling | 修改 descriptor 字段 | 文件和参数 | 更新后的文件 | CYFS object crates | no |
| `sign` | tooling | 对 descriptor 制品签名 | 文件和密钥 | 已签名制品 | crypto crates | no |
| `calc` | tooling | 计算 nonce 或派生值 | 文件和参数 | 派生值/日志 | object parsing | no |
| `shared` | support | 通用辅助逻辑和 desc 类型 | 共享值 | helper API | 内部模块 | no |

## 实现顺序
| 阶段 | 目标 | 前置条件 | 输出 | 依赖 | 可并行 |
|------|------|----------|------|------|--------|
| 1 | 确认命令族责任归属 | 已批准 proposal | 稳定的子模块拆分 | proposal | no |
| 2 | 定义文件输出和面向操作员 DV 的期望 | 已批准 design | `testing.md`、`testplan.yaml` | 阶段 1 | limited |
| 3 | 实现边界明确的工具改动 | 已批准 testing | 代码与测试 | 阶段 1-2 | yes |

## 接口与依赖
### 公共接口摘要
- 子命令：`create`、`show`、`calc`、`modify`、`sign`

### 外部依赖
- CYFS object/crypto crates
- local filesystem

### 运行约束
- 影响输出的改动必须保持可复现、可评审

## 实现布局
```text
desc-tool/src
├── main.rs
├── create.rs
├── desc.rs
├── modify.rs
├── show.rs
├── sign.rs
└── util.rs
```

| 路径 | 类型 | 职责 | 备注 |
|------|------|------|------|
| `desc-tool/src/main.rs` | file | CLI 分发 | 面向操作员 |
| `desc-tool/src/create.rs` | file | 制品创建 | 对输出敏感 |
| `desc-tool/src/show.rs` | file | 制品检查 | 面向操作员 |
| `desc-tool/src/modify.rs` | file | 制品修改 | 对输出敏感 |
| `desc-tool/src/sign.rs` | file | 签名行为 | 安全敏感 |
| `desc-tool/src/desc.rs` | file | desc 结构/辅助逻辑 | 共享 |
| `desc-tool/src/util.rs` | file | 工具辅助逻辑 | 共享 |

## 文档索引
| 文档 | 主题 | 范围 |
|------|------|------|
| `design.md` | 工具概览 | 完整模块 |

## 风险与回滚
- 回滚应优先撤销文件输出变更，同时保留审批轨迹
