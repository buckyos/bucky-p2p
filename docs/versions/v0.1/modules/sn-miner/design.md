---
module: sn-miner
version: v0.1
status: draft
approved_by:
approved_at:
---

# sn-miner 设计

## 设计范围
### 目标
- 让启动职责明确化
- 分离 CLI 解析、制品加载和服务启动

### 非目标
- 重新定义 SN 服务协议行为
- 吸收本应由 `cyfs-p2p` 负责的职责

## 总体方案
- 保持 `main.rs` 作为操作入口
- 将 desc/sec 生成与规范化视为区别于启动执行的独立风险面
- 对影响启动或生成制品的行为变更要求 DV 证据

## 模块拆分
| 子模块 | 类型 | 职责 | 输入 | 输出 | 依赖 | 是否独立文档 |
|--------|------|------|------|------|------|----------------|
| `cli` | runtime | 解析操作员 flag | CLI 参数 | 选定的 desc 路径 | clap | no |
| `artifact_loading` | runtime | 加载或创建 desc/sec 制品并规范化 endpoint | filesystem、crypto objects | device 和 key pair | CYFS crates | no |
| `service_startup` | runtime | 创建并启动 SN 服务和 PN server | 设备身份和栈服务 | 运行中的服务 | `cyfs-p2p` | no |

## 实现顺序
| 阶段 | 目标 | 前置条件 | 输出 | 依赖 | 可并行 |
|------|------|----------|------|------|--------|
| 1 | 确认启动边界 | 已批准 proposal | 稳定的运行时拆分 | proposal | no |
| 2 | 定义 DV 和 integration 期望 | 已批准 design | `testing.md`、`testplan.yaml` | 阶段 1 | limited |
| 3 | 实现边界明确的启动改动 | 已批准 testing | 代码与测试 | 阶段 1-2 | yes |

## 接口与依赖
### 公共接口摘要
- 面向操作员的 `sn-miner` CLI，支持选择 desc 路径

### 外部依赖
- `cyfs-p2p`
- desc/sec filesystem artifacts

### 运行约束
- 默认启动路径和制品生成必须保持可评审

## 实现布局
```text
sn-miner-rust
├── src/main.rs
└── config/package.cfg
```

| 路径 | 类型 | 职责 | 备注 |
|------|------|------|------|
| `sn-miner-rust/src/main.rs` | file | CLI、制品加载、服务启动 | 运行时关键 |
| `sn-miner-rust/config/package.cfg` | file | package/config 表面 | 对 trigger-rule 敏感 |

## 文档索引
| 文档 | 主题 | 范围 |
|------|------|------|
| `design.md` | 启动概览 | 完整模块 |

## 当前改动直接映射
| Proposal 条目 | 设计对象 | 代码路径/接口 | 风险/回滚备注 |
|---------------|----------|---------------|----------------|
| `P-1` | CLI、制品加载和服务启动拆分 | `sn-miner-rust/src/main.rs` | 维持启动职责边界，不吸收适配层逻辑 |
| `P-2` | 配置和默认值风险面 | `sn-miner-rust/config/package.cfg`、`sn-miner-rust/src/main.rs` | 如默认值漂移需回滚具体启动路径 |

## 风险与回滚
- 回滚应在撤销行为改动的同时，保留显式的启动/默认值假设
