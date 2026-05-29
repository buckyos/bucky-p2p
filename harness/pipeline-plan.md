# 流水线计划

## 触发条件
- 已批准的 proposal：`docs/versions/v0.1/modules/p2p-frame/proposal.md`
- 用户启动确认：已于 `2026-05-29` 确认进入自动后续阶段，实现 `p2p-frame` networks 基于 `sfo-reuseport` 的 TCP listener 与 QUIC per-worker endpoint listener 重构
- 当前 `change_id`：
  - `networks_sfo_reuseport_tcp_listener`
  - `networks_sfo_reuseport_quic_listener_socket`

## 验收基线
- 最终验收以下列文档为准：
  - `docs/versions/v0.1/modules/p2p-frame/proposal.md`
- 本流水线不得改变 TCP/QUIC tunnel 线协议、TLS 身份校验、`TunnelNetwork` 公共 trait、QUIC NAT punch policy、heartbeat 语义或 tunnel publish 规则。

## 阶段图
| 任务 ID | 阶段 | 职责 | 范围 | 父任务 | 依赖项 | 输出 | 完成条件 |
|---------|------|------|------|--------|--------|------|----------|
| P-SFO-NET-1 | planning | 为 networks sfo-reuseport listener 重构创建阶段图、依赖、输出和退回路由 | `p2p-frame` / `networks` | root | 用户确认并启动自动流水线 | `harness/pipeline-plan.md` | 本计划覆盖 design、implementation、testing、acceptance 任务 |
| D-SFO-NET-1 | design | 把已批准 proposal 转成 TCP/QUIC listener、`ServerRuntime` 注入和 Quinn `AsyncUdpSocket` 适配设计 | `design.md`、必要 `design/`、必要长期边界文档 | root | proposal approved | design 制品 | design approved 且直接映射两个 change_id |
| I-SFO-NET-1 | implementation | 在已批准 proposal/design 边界内实现 TCP/QUIC listener 重构 | `p2p-frame/src/networks/**`、`p2p-frame/src/stack.rs`、必要 build/runtime 资源 | root | D-SFO-NET-1 approved，implementation admission 通过 | production code | TCP/QUIC listener 使用 `sfo-reuseport`，默认和外部 runtime 路径可用 |
| T-SFO-NET-1 | testing | 基于 proposal、design 和已交付代码生成验证覆盖 | test code、test fixtures、`testing.md`、`testing/`、`testplan.yaml` | root | I-SFO-NET-1 complete | testing 制品 + tests | testing approved，验证入口覆盖两个 change_id |
| A-SFO-NET-1 | acceptance | 审计 proposal、design、testing、implementation 和验证证据是否一致 | `p2p-frame` networks 交付 | root | T-SFO-NET-1 complete | acceptance report | acceptance 通过或明确退回责任阶段 |

## 子任务
| 任务 ID | 阶段 | 职责 | 子模块 | 父任务 | 依赖项 | 输出 | 完成条件 |
|---------|------|------|--------|--------|--------|------|----------|
| D-SFO-NET-1.1 | design | 定义 `ServerRuntime` 外部注入和默认创建路径 | `stack` / `networks` | D-SFO-NET-1 | proposal approved | `design.md` | 配置入口、所有权和关闭语义明确 |
| D-SFO-NET-1.2 | design | 定义 TCP listener 基于 `TcpServer` 的入站连接接入路径 | `networks/tcp` | D-SFO-NET-1 | proposal approved | `design.md` 或 `design/sfo-reuseport-listeners.md` | 入站 TLS accept、control/data 分流和 close 语义明确 |
| D-SFO-NET-1.3 | design | 定义 QUIC listener 基于 `QuicServer::serve_socket(...)`、per-worker Quinn endpoint、worker CID shard 和 Quinn `AsyncUdpSocket` 适配路径 | `networks/quic` | D-SFO-NET-1 | proposal approved | `design.md` 或 `design/sfo-reuseport-listeners.md` | `poll_recv`、`try_send`、worker endpoint、CID route、punch 同源 socket 和 close 语义明确 |
| I-SFO-NET-1.1 | implementation | 实现 `ServerRuntime` 配置和 network 构造传递 | `stack` / `networks` | I-SFO-NET-1 | admission passed | production code | 默认 runtime 与外部 runtime 都能被 TCP/QUIC listener 使用 |
| I-SFO-NET-1.2 | implementation | 实现 TCP listener `TcpServer` 接入 | `networks/tcp` | I-SFO-NET-1 | admission passed | production code | TCP tunnel 建立和 data/control 分流保持兼容 |
| I-SFO-NET-1.3 | implementation | 实现 QUIC listener `QuicServer::serve_socket(...)` + per-worker Quinn endpoint 接入 | `networks/quic` | I-SFO-NET-1 | admission passed | production code | QUIC accept/connect/punch 使用同源 worker listener socket |
| T-SFO-NET-1.1 | testing | 覆盖 runtime 注入和 TCP listener 行为 | unit/integration | T-SFO-NET-1 | implementation complete | tests + testing docs | 覆盖 `networks_sfo_reuseport_tcp_listener` |
| T-SFO-NET-1.2 | testing | 覆盖 QUIC per-worker endpoint socket 适配、CID shard 和 punch 同源端口 | unit/integration | T-SFO-NET-1 | implementation complete | tests + testing docs | 覆盖 `networks_sfo_reuseport_quic_listener_socket` |

## 退回规则
- 如果 proposal 无法支撑 `TcpServer`/`QuicServer` 或 `ServerRuntime` 注入：
  - 退回 proposal 澄清任务
- 如果 `ServerRuntime` 所有权、TCP close、QUIC `AsyncUdpSocket` 适配、per-worker endpoint、CID shard 路由或 punch 同源约束不明确：
  - 退回 design 任务
- 如果 implementation admission 未通过，或实现需要改变 `TunnelNetwork` 公共 trait、TCP/QUIC 线协议、TLS 身份校验或 NAT punch proposal 边界：
  - 退回对应前置阶段
- 如果缺少 runtime 注入、TCP listener、QUIC packet adapter、listener close 或 punch 同源端口验证覆盖：
  - 退回 testing 任务
- 如果验收发现证据链不一致：
  - 按问题归属退回 proposal、design、testing 或 implementation

## 退出条件
- [ ] proposal、design、testing 均为 approved
- [ ] implementation admission 通过两个 change_id
- [ ] TCP listener 使用 `sfo_reuseport::TcpServer`
- [ ] QUIC listener 使用 `sfo_reuseport::QuicServer::serve_socket(...)`、per-worker Quinn endpoint 和 Quinn `AsyncUdpSocket` 适配器
- [ ] QUIC 主动发送使用 worker endpoint，UDP punch 使用首个可用 worker socket 同源端口
- [ ] 外部 `ServerRuntime` 注入和默认 runtime 路径均可用
- [ ] 必需 unit/integration 证据存在
- [ ] 已基于 `proposal.md` 通过最终验收
