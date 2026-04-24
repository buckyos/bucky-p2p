---
module: p2p-frame
version: v0.1
status: approved
approved_by: user
approved_at: 2026-04-24
---

# p2p-frame 设计

> 该数据包为治理现有核心库定义设计基线。现有协议说明继续作为参考文档存在，并在此处建立索引。

## 设计范围
### 目标
- 让核心库边界足够明确，使未来工作可以按子模块拆分。
- 复用现有协议说明，而不是重复编写设计内容。
- 为 planning、testing 和 acceptance 定义具备阶段可执行性的子模块责任归属。
- 为本轮 `pn/service/pn_server.rs` 的用户流量统计与限速需求建立可执行的设计边界，并把 `sfo-io` 接入限定在 relay bridge 路径内，包括 source/target 双边独立统计视图与 source 单边限速的区分。
- 为本轮 proxy tunnel `stream` 路径的“可选 TLS-over-proxy 载荷加密、由使用者显式接口控制且由两端在 tunnel 外约定”需求建立可执行设计边界，并明确 `datagram` 路径继续保持明文兼容且忽略该 `stream` 加密模式，同时把加密接口限制在 PN 专有 API 内，而不是污染通用 `Tunnel` / `TunnelNetwork` trait。
- 为本轮 `PnTunnel` idle timeout 生命周期关闭需求建立可执行设计边界，明确 channel lease 计数、原子关闭、accept/open 失败和关闭后重新创建语义。
- 为本轮 `tunnel/TunnelManager` 中“当远端当前通过 proxy tunnel 连通时，后台周期性重试 direct/reverse 升级并限制失败退避上限”的行为建立设计边界，避免连接长期粘连在代理路径上。
- 为本轮 `tunnel/TunnelManager` 中“新建 tunnel 的统一 register/publish 生命周期，以及 reverse tunnel 只延后 publish 时机而不改变 publish 规则”的行为建立可执行设计边界，收敛当前散落在多处的发布决策。

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
| `tunnel` | orchestration | tunnel 生命周期、连接选择、统一 register/publish 生命周期、proxy 回退与后续脱代理升级行为 | 传输事件、身份、发现能力 | active/passive/proxy tunnel 状态 | `networks`、`finder`、`pn`、`sn` | yes |
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
- `sfo-io` 提供的流量统计与限速实现，供 `pn/service` 在 bridge 数据路径中复用
- `rustls`、`tokio-rustls` 以及现有 `p2p-frame/src/tls/**` 组件，供 proxy tunnel 在成功建链后复用 TLS 握手与身份校验

### 运行约束
- 传输和运行时行为必须能够通过日志进行诊断。
- 影响协议的改动必须在代码改动开始前更新 design/testing 证据。
- `pn_server` 的统计与限速不得改变现有 `ProxyOpenReq` / `ProxyOpenResp` 握手顺序和结果码映射。
- `pn_server` 的统计主体必须分别锚定到 relay 已认证并规范化后的 source 身份与已打开的 target 身份；限速主体仍只使用 source 侧身份。
- proxy tunnel 的“是否加密”必须通过 PN 专有显式入口或显式的 client/stack 配置决定；通用 `TunnelNetwork` / `Tunnel` trait 不新增 TLS 参数。未显式配置的 `PnClient` 上，`create_tunnel*` 与 `open_*` 默认保持当前明文兼容语义；若调用方先显式设置 `PnClient::set_stream_security_mode(...)` 或 `P2pStackConfig::set_proxy_stream_encrypted(true)`，则该 `PnClient` 后续通过通用 trait 创建的 proxy tunnel，以及同一 listener 被动接受到的 `PnTunnel`，都会继承当时的 TLS 模式快照。
- 是否启用 TLS 由 proxy tunnel 两端在 tunnel 外预先约定；PN open 控制流不额外承载 TLS 模式协商。
- 若调用方显式选择加密，则失败路径必须直接失败，不允许静默回退到明文桥接。
- 本轮加密设计只覆盖 proxy `stream`；proxy `datagram` 不进入 TLS-over-proxy 范围，但必须忽略 `stream` 加密模式并保持当前明文兼容语义。
- `PnTunnel` idle close 必须按本地生命周期关闭建模，不引入可靠远端 close 通知；触发后 tunnel 状态进入 `Closed` 或等价错误终态，并复用普通 close 的本地效果。
- `PnTunnel` 必须在统一状态临界区内维护 channel lease 计数、pending inbound queue 计数、idle deadline 和 close reason；本地 open、inbound 投递、channel drop 与 idle sweeper 都必须通过该状态执行竞态判定。
- `PnShared` 必须在 tunnel 关闭后移除 live registry 中的旧对象，确保后续同 `(remote_id, tunnel_id)` inbound `ProxyOpenReq` 不会投递到已关闭 tunnel，而是创建新的 passive tunnel。
- 当某个远端当前只有 proxy tunnel 可用时，`TunnelManager` 必须在后台继续尝试 direct 或 reverse 建链，而不是无限期停留在 proxy 路径。
- 上述脱代理尝试不得再次把“升级任务”回退成 proxy 建链；proxy 只作为对外可用的兜底连通性，而不是后台升级路径的成功条件。
- 持续失败的脱代理尝试可以延长重试间隔，但必须有上限；当前实现约束为初始 5 分钟、失败后指数退避、最大不超过 2 小时。
- 对外可用的新 tunnel 必须先进入 `TunnelManager` 候选注册，再进入统一 publish 路径；除 `remote_id` 未知的临时 wrapper 外，不允许存在“只广播不登记”的长期语义分支。
- `TunnelManager` 的 publish 可见性决策必须收敛到单一生命周期模型：默认“register 后立即 publish”，唯一允许的延后场景是命中本地 `(remote_id, tunnel_id)` `reverse_waiter` 的 reverse tunnel。
- 上述延后 publish 只影响时机，不改变规则：reverse tunnel 一旦完成本地 waiter 交付，就必须通过与 direct/proxy 相同的 publish 入口变为可见候选。
- reverse 建链若在等待期间超时、取消或失败，相关 waiter 必须被清理；该 waiter 之后才到达的同 `(remote_id, tunnel_id)` reverse tunnel 不得继续隐藏，而必须按普通新 tunnel 进入 publish 路径。

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
| `docs/versions/v0.1/modules/p2p-frame/design/tunnel-publish-lifecycle.md` | `TunnelManager` 新 tunnel 注册/发布生命周期与 reverse 延后 publish 规则补充 | `tunnel` |
| `docs/versions/v0.1/modules/p2p-frame/design/pn-proxy-encryption.md` | PN client / tunnel 侧可选端到端载荷加密、显式接口和 TLS 叠加设计补充 | `pn/client` |
| `docs/versions/v0.1/modules/p2p-frame/design/pn-tunnel-idle-close.md` | `PnTunnel` idle timeout 生命周期关闭、channel lease 计数和关闭后重新创建设计补充 | `pn/client` |
| `docs/versions/v0.1/modules/p2p-frame/design/pn-server.md` | relay 侧 PN server、`sfo-io` 流量统计与限速设计补充 | `pn/service` |
| `p2p-frame/docs/tunnel_design.md` | tunnel 概念 | tunnel |
| `p2p-frame/docs/tunnel_command_protocol_design.md` | tunnel 命令协议 | tunnel/ttp |
| `p2p-frame/docs/tcp_tunnel_protocol_design.md` | TCP tunnel 协议 | networks/tunnel |
| `p2p-frame/docs/quic_tunnel_design.md` | QUIC tunnel 协议 | networks |
| `p2p-frame/docs/sn_design.md` | SN 行为 | sn |
| `p2p-frame/docs/pn_design.md` | PN 协议以及 client/server 参考说明 | pn |
| `p2p-frame/docs/ttp_module_design.md` | TTP 模块行为 | ttp |

## 当前改动直接映射
| Proposal 条目 | 设计对象 | 代码路径/接口 | 风险/回滚备注 |
|---------------|----------|---------------|----------------|
| relay 侧 `pn_server` 在成功握手后的双向字节桥接路径上增加按用户统计的流量计量 | relay bridge 上的 source/target 双边独立统计视图 | `p2p-frame/src/pn/service/pn_server.rs`、`PnTrafficManager`、统计查询接口 | 若实现阶段无法在不改变握手与 bridge 契约的前提下同时暴露 source/target 视图，应先退回 design 重新澄清统计模型，而不是把 target 统计挤进 source 视图。 |
| relay 侧 `pn_server` 在成功握手后的双向字节桥接路径上增加按用户生效的限速能力 | 仅 source 侧生效的用户级限速，与 target 统计视图解耦 | `p2p-frame/src/pn/service/pn_server.rs`、限速配置/查询接口 | 若实现阶段发现 target 统计可见性会迫使限速扩展成双边模型，应先退回 proposal，而不是静默扩大限速范围。 |
| `PnTunnel` idle timeout 生命周期关闭 | `PnTunnel` 本地状态机、channel lease 计数、idle sweeper 和关闭后重新创建 | `p2p-frame/src/pn/client/pn_tunnel.rs`、`p2p-frame/src/pn/client/pn_client.rs`、`p2p-frame/src/pn/client/pn_listener.rs` | 若实现阶段无法可靠追踪已返回给上层的 channel 生命周期，应先退回 design 重新划分 lease wrapper，而不是只统计 inbound queue。 |
| 核心库的长期模块边界 | `TunnelManager` 的统一 register/publish 生命周期 | `p2p-frame/src/tunnel/tunnel_manager.rs` | 收敛 publish 逻辑时，优先保持 reverse waiter、候选复用和 proxy 升级语义不变；若实现阶段发现现有测试/运行时依赖旧的分散式时序，则先回滚到文档阶段补充约束。 |

## 风险与回滚
- 协议或传输改动可能破坏所有下游 crate。
- 面向运行时或密码学的回归需要比孤立工具改动更强的回滚姿态。
- 回滚应优先撤销具体实现改动，同时保留已批准的 proposal/design/testing 证据，为下一次尝试复用。
