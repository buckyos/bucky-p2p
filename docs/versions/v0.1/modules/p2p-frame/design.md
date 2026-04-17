---
module: p2p-frame
version: v0.1
status: draft
approved_by:
approved_at:
---

# p2p-frame 设计

> 该数据包为治理现有核心库定义设计基线。现有协议说明继续作为参考文档存在，并在此处建立索引。

## 设计范围
### 目标
- 让核心库边界足够明确，使未来工作可以按子模块拆分。
- 复用现有协议说明，而不是重复编写设计内容。
- 为 planning、testing 和 acceptance 定义具备阶段可执行性的子模块责任归属。

### 非目标
- 对 `p2p-frame/docs/` 下已存在的每个协议细节做完整重写
- 在 harness 启动改造阶段重组源码文件

## 总体方案
- 将 `p2p-frame` 视为 Tier 0 核心模块。
- 把本文件作为顶层设计索引。
- 将 `p2p-frame/docs/` 下现有协议说明视为传输、tunnel、PN、SN 和 TTP 行为的从属设计证据。
- 未来工作按直接子模块拆分，并赋予真实的责任边界与验证边界。

## 模块拆分
| 子模块 | 类型 | 职责 | 输入 | 输出 | 依赖 | 是否独立文档 |
|--------|------|------|------|------|------|----------------|
| `networks` | core transport | TCP/QUIC 监听器、endpoint、validator 和底层网络行为 | sockets、runtime、TLS | 传输事件和 tunnel plumbing | runtime、TLS | no |
| `tunnel` | orchestration | tunnel 生命周期、连接选择和 manager 行为 | 传输事件、身份、发现能力 | active/passive/proxy tunnel 状态 | `networks`、`finder`、`pn`、`sn` | no |
| `ttp` | protocol | tunnel 上的命令和流复用协议 | tunnel IO | 带帧的命令/流行为 | `tunnel` | no |
| `sn` | service | 对端注册、信令和调用转发 | tunnel/ttp、身份 | SN 服务行为 | `ttp`、`p2p_identity` | no |
| `pn` | service | proxy-node 中继行为 | tunnel/ttp | 基于 relay 的连通性 | `tunnel`、`ttp` | yes |
| `finder` | support | 设备与 outer device 查询缓存 | endpoints、设备元数据 | 发现辅助能力 | `stack` | no |
| `identity_tls` | support | P2P 身份、TLS、X509 和密码学辅助逻辑 | keys、certs、握手元数据 | 已认证连接 | `tls`、`x509`、`p2p_identity` | no |
| `stack_runtime` | assembly | 高层 stack 编排和运行时抽象 | 所有下层 | 端到端 P2P 栈 | 几乎全部子模块 | no |

## 实现顺序
| 阶段 | 目标 | 前置条件 | 输出 | 依赖 | 可并行 |
|------|------|----------|------|------|--------|
| 1 | 确认 proposal 范围和直接子模块 | 已批准 proposal | 稳定的模块拆分 | proposal | no |
| 2 | 为每个直接子模块定义或更新 testing 覆盖 | 已批准的 design 拆分 | `testing.md`、`testplan.yaml` | 阶段 1 | limited |
| 3 | 在硬性准入检查下实现子模块改动 | 已批准 testing | 代码与测试 | 阶段 1-2 | yes |
| 4 | 依据 proposal 审计证据链 | implementation 证据已就绪 | acceptance report | 阶段 3 | no |

## 接口与依赖
### 公共接口摘要
- `p2p-frame` 暴露核心网络和 tunnel 栈，供 `cyfs-p2p` 与运行时二进制消费。
- 直接子模块契约必须持续与当前协议说明和公开 crate 导出保持一致。

### 外部依赖
- async/runtime crates
- crypto and TLS crates
- CYFS-adjacent consumers through `cyfs-p2p`

### 运行约束
- 传输和运行时行为必须能够通过日志进行诊断。
- 影响协议的改动必须在代码改动开始前更新 design/testing 证据。

## 实现布局
```text
p2p-frame/src
├── networks/
├── tunnel/
├── ttp/
├── sn/
├── pn/
├── finder/
├── tls/
├── x509/
├── datagram/
├── dht/
├── stack.rs
└── p2p_identity.rs
```

| 路径 | 类型 | 职责 | 备注 |
|------|------|------|------|
| `p2p-frame/src/networks/` | dir | 传输层 | 高风险 trigger surface |
| `p2p-frame/src/tunnel/` | dir | tunnel 编排 | 高风险 trigger surface |
| `p2p-frame/src/ttp/` | dir | tunnel transport protocol | 高风险 trigger surface |
| `p2p-frame/src/sn/` | dir | SN 服务逻辑 | 高风险 trigger surface |
| `p2p-frame/src/pn/` | dir | PN 中继逻辑 | 高风险 trigger surface |
| `p2p-frame/src/pn/client/` | dir | PN client、listener 和 tunnel 行为 | 与 PN 参考说明配套 |
| `p2p-frame/src/pn/service/` | dir | relay 侧 PN server、校验和桥接 | 由 `pn_server` 补充文档建立索引 |
| `p2p-frame/src/finder/` | dir | 设备查询辅助逻辑 | 对邻接模块敏感 |
| `p2p-frame/src/tls/` | dir | TLS/密码学辅助逻辑 | 安全敏感 |
| `p2p-frame/src/x509.rs` 和 `p2p-frame/src/x509/` | file/dir | X509 支持 | 受 feature gate 控制且安全敏感 |
| `p2p-frame/docs/*.md` | docs | 协议/设计参考 | 在下方建立索引 |

## 文档索引
| 文档 | 主题 | 范围 |
|------|------|------|
| `design.md` | 模块概览和任务拆分 | 完整模块 |
| `docs/versions/v0.1/modules/p2p-frame/design/pn-server.md` | relay 侧 PN server 设计补充 | `pn/service` |
| `p2p-frame/docs/tunnel_design.md` | tunnel 概念 | tunnel |
| `p2p-frame/docs/tunnel_command_protocol_design.md` | tunnel 命令协议 | tunnel/ttp |
| `p2p-frame/docs/tcp_tunnel_protocol_design.md` | TCP tunnel 协议 | networks/tunnel |
| `p2p-frame/docs/quic_tunnel_design.md` | QUIC tunnel 协议 | networks |
| `p2p-frame/docs/sn_design.md` | SN 行为 | sn |
| `p2p-frame/docs/pn_design.md` | PN 协议以及 client/server 参考说明 | pn |
| `p2p-frame/docs/ttp_module_design.md` | TTP 模块行为 | ttp |

## 风险与回滚
- 协议或传输改动可能破坏所有下游 crate。
- 面向运行时或密码学的回归需要比孤立工具改动更强的回滚姿态。
- 回滚应优先撤销具体实现改动，同时保留已批准的 proposal/design/testing 证据，为下一次尝试复用。
