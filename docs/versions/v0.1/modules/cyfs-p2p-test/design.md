---
module: cyfs-p2p-test
version: v0.1
status: draft
approved_by:
approved_at:
---

# cyfs-p2p-test 设计

## 设计范围
### 目标
- 将运行时 harness 视为一等验证模块。
- 分离 CLI 入口行为、配置解析和场景编排职责。

### 非目标
- 拥有核心协议设计
- 吸收其他模块数据包的验收职责

## 总体方案
- 使用 `main.rs` 统一编排 all-in-one、client 和 server 场景流程。
- 在 testing 文档和 `testplan.yaml` 中显式写明运行时制品假设。

## 模块拆分
| 子模块 | 类型 | 职责 | 输入 | 输出 | 依赖 | 是否独立文档 |
|--------|------|------|------|------|------|----------------|
| `cli` | runtime | 子命令解析和操作员入口 | CLI 参数 | 场景选择 | clap | no |
| `config_runtime` | runtime | 加载配置和本地制品 | 仓库本地路径下的文件 | 运行时参数 | filesystem | no |
| `scenario_orchestration` | runtime | 运行 all-in-one、client 和 server 流程 | 适配层/核心栈 | 可观察的运行时行为 | `cyfs-p2p`、`p2p-frame` | no |

## 实现顺序
| 阶段 | 目标 | 前置条件 | 输出 | 依赖 | 可并行 |
|------|------|----------|------|------|--------|
| 1 | 确认场景责任归属和前置条件 | 已批准 proposal | 稳定的运行时边界 | proposal | no |
| 2 | 定义 DV 和 integration 证据规则 | 已批准 design | `testing.md`、`testplan.yaml` | 阶段 1 | limited |
| 3 | 实现边界明确的 harness 改动 | 已批准 testing | 代码与测试 | 阶段 1-2 | yes |

## 接口与依赖
### 公共接口摘要
- 面向操作员的 CLI，提供 `all-in-one`、`client` 和 `server`

### 外部依赖
- `cyfs-p2p`
- `p2p-frame`
- 本地配置、desc 和日志制品

### 运行约束
- 场景改动需要显式的环境说明

## 实现布局
```text
cyfs-p2p-test/src
└── main.rs
```

| 路径 | 类型 | 职责 | 备注 |
|------|------|------|------|
| `cyfs-p2p-test/src/main.rs` | file | CLI、配置和运行时编排 | 对环境敏感 |

## 文档索引
| 文档 | 主题 | 范围 |
|------|------|------|
| `design.md` | 运行时 harness 概览 | 完整模块 |

## 当前改动直接映射
| Proposal 条目 | 设计对象 | 代码路径/接口 | 风险/回滚备注 |
|---------------|----------|---------------|----------------|
| `P-1` | CLI / 配置 / 场景编排拆分 | `cyfs-p2p-test/src/main.rs` | 维持 harness 作为验证模块而非协议所有者 |
| `P-2` | DV 与 integration 证据职责 | `cyfs-p2p-test/src/main.rs` | 若场景语义漂移需回滚具体入口行为 |

## 风险与回滚
- 回滚应在撤销场景行为改动的同时，保留已声明的运行时前置条件
