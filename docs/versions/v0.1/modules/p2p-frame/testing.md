---
module: p2p-frame
version: v0.1
status: approved
approved_by: auto-pipeline
approved_at: 2026-07-07T11:35:24+08:00
approved_content_sha256: 6fe48e8f439928923d5372b038fba794dec42b420ece11a0f44a1d096d88329f
---

# p2p-frame 测试

## Test Document Index
| 文档 | 主题 | 范围 |
|------|------|------|
| `testing.md` | 模块级验证基线 | 完整模块 |
| `docs/versions/v0.1/modules/p2p-frame/testing/tunnel-publish-lifecycle.md` | `TunnelManager` 新 tunnel register/publish 生命周期验证补充 | `tunnel` |
| `docs/versions/v0.1/modules/p2p-frame/testing/tunnel-nat-traversal.md` | 单 SN NAT 打洞优化与 `ServerReflexive` endpoint area 验证补充 | `endpoint`、`tunnel`、`sn/client`、必要 `sn/service` |
| `docs/versions/v0.1/modules/p2p-frame/testing/sn-control-stream-signaling.md` | SN 低频信令使用 `Tunnel` control stream、purpose 与无 fallback 失败边界验证补充 | `sn/client`、`sn/service`、`ttp` |
| `docs/versions/v0.1/modules/p2p-frame/testing/pn-proxy-encryption.md` | proxy tunnel `stream` 的 TLS-over-proxy 验证补充 | `pn/client` |
| `docs/versions/v0.1/modules/p2p-frame/testing/pn-tunnel-idle-close.md` | `PnTunnel` idle timeout 生命周期关闭验证补充 | `pn/client` |
| `docs/versions/v0.1/modules/p2p-frame/testing/pn-tunnel-control-channel.md` | `PnTunnel` 控制通道、ready gate、heartbeat 和远端关闭感知验证补充 | `pn/client`、`pn/protocol`、必要 `pn/service` |
| `docs/versions/v0.1/modules/p2p-frame/testing/pn-server.md` | relay 侧 PN server 验证补充 | `pn/service` |
| `docs/versions/v0.1/modules/p2p-frame/testing/sfo-reuseport-listeners.md` | `networks` 基于 `sfo-reuseport` 的 TCP/QUIC listener 验证补充 | `networks/tcp`、`networks/quic`、`stack` |
| `docs/versions/v0.1/modules/p2p-frame/testing/tunnel-control-stream-api.md` | `Tunnel` control stream API、内部 runtime、`Data` 控制命令承载和关闭传播验证补充 | `networks/tunnel`、`networks/tcp`、`networks/quic`、`pn/client` |
| `p2p-frame/docs/tcp_tunnel_protocol_test_cases.md` | TCP tunnel 协议用例 | transport/tunnel |

## Unified Test Entry
- 机器可读计划：`docs/versions/v0.1/modules/p2p-frame/testplan.yaml`
- Unit: `python3 ./harness/scripts/test-run.py p2p-frame unit`
- DV: 当前 disabled；`cyfs-p2p-test all-in-one` 不作为 p2p-frame 或其他模块的 DV 证据
- Integration: `python3 ./harness/scripts/test-run.py p2p-frame integration`

## Submodule Tests
| 子模块 | 职责 | 详细测试文档 | 必需行为 | 边界/失败场景 | 测试类型 | 测试文件 |
|--------|------|--------------|----------|----------------|----------|----------|
| `networks` | TCP/QUIC 传输和 listener 行为 | `p2p-frame/docs/tcp_tunnel_protocol_test_cases.md`、`docs/versions/v0.1/modules/p2p-frame/testing/sfo-reuseport-listeners.md`、`docs/versions/v0.1/modules/p2p-frame/testing/tunnel-control-stream-api.md` | 连接建立、复用、地址处理、network manager 行为、TCP listener 由 `sfo_reuseport::TcpServer` 驱动、QUIC listener 由 `sfo_reuseport::QuicServer::serve_socket(...)` 驱动并通过 per-worker 自定义 `AsyncUdpSocket` 交给 Quinn，QUIC connect queue 使用顶层按位置传入的 bounded channel，TCP/QUIC tunnel 入站 stream/datagram 不再保留旧 accept queue 容量参数，TCP/QUIC control stream API 通过控制命令 `Data` 承载内部 frame | listener 失败、连接复用边界、协议不匹配、`ServerRuntime` 注入/default 启动失败、QUIC worker CID 路由不稳定、主动发送 socket 获取失败，bounded queue 满载时不得无限缓存，`Data` payload 超过 64KiB 必须拒绝，底层控制通道断开后派生 control stream 必须关闭 | unit + integration | `p2p-frame/src/networks/tcp/network.rs`、`p2p-frame/src/networks/tcp/listener.rs`、`p2p-frame/src/networks/tcp/tunnel.rs`、`p2p-frame/src/networks/net_manager.rs`、`p2p-frame/src/networks/quic/network.rs`、`p2p-frame/src/networks/quic/listener.rs`、`p2p-frame/src/networks/quic/tunnel.rs` |
| `endpoint` | endpoint area、协议和地址编解码 | `docs/versions/v0.1/modules/p2p-frame/testing/tunnel-nat-traversal.md` | `ServerReflexive` enum 命名、`S` 文本编解码、raw codec area 映射、`is_sys_default()` 移除 | 旧 `D` 文本不得继续作为新语义输入，`ServerReflexive` 不得被 `is_static_wan()` 判定为静态公网 | unit | `p2p-frame/src/endpoint.rs` |
| `tunnel` | tunnel 生命周期和 manager 行为 | `docs/versions/v0.1/modules/p2p-frame/testing/tunnel-publish-lifecycle.md`、`docs/versions/v0.1/modules/p2p-frame/testing/tunnel-nat-traversal.md` | active/passive/proxy tunnel 创建、统一 register/publish 生命周期、reverse waiter 命中时的延后 publish 和完成后的 publish、reverse incoming 无 waiter close、proxy tunnel 发布后的后台 direct/reverse 升级重试、已有多个 candidate 时非 proxy 优先复用、direct/reverse 统一 300ms 短延迟竞速、SN 存在时仅对 `ServerReflexive` QUIC candidate 开启同源 UDP punch 调度、proxy 短窗口脱代理、按协议隔离 endpoint 评分、`ServerReflexive` 与静态 `Wan` / `Mapped` 区分 | 同时存在多个 tunnel、选择回退、后注册 proxy 不得覆盖已有可用非 proxy candidate、reverse waiter 命中时的延后 publish、无 waiter reverse incoming close 且不 register/publish、失败清理、proxy 升级路径持续失败后的退避封顶，升级流程不得回退成 proxy，TCP 失败不得拖累 QUIC/UDP 候选，默认 intent、无 SN service 和非 `ServerReflexive` 路径不得发送 punch，punch 发送失败不得改变建链结果，SN 反射地址不得被误判为静态公网 | unit | `p2p-frame/src/tunnel/tunnel_manager.rs`、`p2p-frame/src/networks/quic/network.rs`、`p2p-frame/src/networks/quic/tunnel.rs` |
| `ttp` | 复用命令/流协议 | `p2p-frame/docs/ttp_module_design.md` | 流注册、server/client/node 协议交互，`TtpNode` 缺失 tunnel 时主动创建并 attach 后打开 stream/control stream/datagram，已有 tunnel 时按 target 复用，`TtpClient` 支持删除 maintained target 并释放 non-maintained idle tunnel cache，`TtpServer` 入站 tunnel 在 attach/cache 前执行可配置 validator 且默认 allow-all | 无效命令流、channel 关闭、`TtpServer` lookup-only 行为保持、`TtpNode` 建链失败显式返回错误、删除 target 后 maintained set 为空、active/pending lease 阻止 idle release、maintained target 不被 non-maintained idle release 清理，`TtpServer` validator reject/error 后不得 attach/cache 且 best-effort close tunnel | unit | `p2p-frame/src/ttp/tests.rs` |
| `sn` | 信令与对端管理 | `p2p-frame/docs/sn_design.md`、`docs/versions/v0.1/modules/p2p-frame/testing/tunnel-nat-traversal.md`、`docs/versions/v0.1/modules/p2p-frame/testing/sn-control-stream-signaling.md` | 注册、查询、呼叫路由、`SnCall` 携带本次 `reverse_endpoint_array`、SN called 转发保留调用方候选并扩展单 SN 观察候选、SN 观察地址按节点自上报地址一致性标记 `Wan` 或 `ServerReflexive`、TCP observed source address 不作为普通候选返回但可与 `map_ports` 构造 `Mapped` 候选、SN 低频小消息通过 control stream 的 SN purpose 交付、SN server 连接验证器默认 allow-all 且自定义 reject 能短路入站请求，且 validator 上下文只包含 `client_id` 与 `client_cert`；SN service runtime 必须由外部或 stack 级全局 `ServerRuntime` 注入，缺少 runtime 返回配置错误；SN service 不再导出或构造 `SnServiceContractServer` / service receipt 装配路径，`sn/protocol` receipt wire 兼容结构保留 | 对端缺失、并发、候选去重和单 SN 边界，TCP source socket address 无 `map_ports` 时不得返回且原始来源端口不得泄漏，观察地址不一致时不得标记为 `Wan`，control stream purpose 未监听或控制通道不可用时必须显式失败且不得 fallback 到普通 stream，自定义 validator reject 后不得继续更新 peer 状态或转发 call，证书解析 id 与 cmd tunnel peer id 不一致时不得调用 validator 或更新状态；缺少 `ServerRuntime` 不得静默创建 service-local runtime；contract/receipt 清理不得删除基础 SN command 字段或在相邻模块重建兼容旁路 | unit + integration | `p2p-frame/src/sn/tests.rs`、`p2p-frame/src/sn/client/sn_service.rs`、`p2p-frame/src/sn/service/*.rs`、`p2p-frame/src/sn/protocol/sn.rs` |
| `pn` | relay tunnel 行为 | `docs/versions/v0.1/modules/p2p-frame/testing/pn-server.md`、`docs/versions/v0.1/modules/p2p-frame/testing/pn-proxy-encryption.md`、`docs/versions/v0.1/modules/p2p-frame/testing/pn-tunnel-idle-close.md`、`docs/versions/v0.1/modules/p2p-frame/testing/pn-tunnel-control-channel.md`、`docs/versions/v0.1/modules/p2p-frame/testing/tunnel-control-stream-api.md` | PN tunnel relay、请求校验、默认 `PnConnectionValidator` 显式 allow-all、显式 validator/assigned-target policy 控制 wrong-PN 准入、响应转发、source/target 双边用户流量统计、仅 source 侧生效的 server bridge 限速、proxy stream 的 TLS-over-proxy 行为、client 级 TLS 模式配置快照、`datagram` 在 `TlsRequired` 下继续保持明文兼容、`PnTunnel` idle timeout 本地关闭、`PnTunnel` control channel ready gate、heartbeat 和远端关闭感知，以及 PN control stream API 通过 `PnControlCmd::Data` 承载内部 frame | relay 启动失败、validator 拒绝、target 打开失败、control ready 失败或超时、control 与业务 channel 混淆、control EOF/decode/write 失败未关闭 tunnel、heartbeat timeout 不收敛、remote close 后 open/accept/control stream 仍挂起、PN `Data` 超过 64KiB 未拒绝、双端 TLS 约定不一致、TLS 证书校验失败、`datagram` 错误继承 `stream` TLS 模式、client 级模式误用导致后续 tunnel 意外继承 TLS、默认构造误拒绝未配置连接、assigned-target policy 漏拒 wrong-PN 目标、idle close 误关闭 active channel、idle close 后 accept 不醒、open 继续等待 timeout、迟到 inbound open 被错误投递到 closed tunnel、统计失真、source/target 串户、target 统计错误触发限速、限速背压异常 | unit + integration | `p2p-frame/src/pn/protocol.rs`、`p2p-frame/src/pn/client/pn_tunnel.rs`、`p2p-frame/src/pn/client/pn_client.rs`、`p2p-frame/src/pn/client/pn_listener.rs`、`p2p-frame/src/pn/service/pn_server.rs` |
| `identity_tls` | 身份、TLS、X509 辅助逻辑 | none | 证书处理和身份正确性 | 无效证书、握手不匹配、feature-gated 路径 | unit | `p2p-frame/src/x509.rs`、`p2p-frame/src/tls/**` |
| `channel_capacity_config` | 顶层 bounded channel 容量配置 | `design.md` | `P2pConfig` 默认构造时各位置容量均为 `1024`，可只覆盖单个位置；`P2pStackConfig` 从 `P2pEnv` 继承分位置容量且可覆盖，生产路径不再存在 unbounded mpsc 类型 | 显式容量未下发、底层残留默认值、测试替身继续使用 unbounded queue | unit + integration | `p2p-frame/src/stack.rs`、`p2p-frame/src/**/*.rs` 搜索证据 |

## Module-Level Tests
| 测试项 | 覆盖边界 | 入口 | 预期结果 | 测试类型 | 测试文件/脚本 |
|--------|----------|------|----------|----------|-----------------|
| 核心库 unit 测试集 | `p2p-frame` 内部直接子模块，包括 endpoint area 编解码、SN 观察地址分类、SN control stream 信令、`TunnelManager` 的统一 publish 生命周期、NAT-aware direct/reverse 竞速、`ServerReflexive` only 同源 UDP punch 调度、QUIC heartbeat timeout、proxy 短窗口脱代理、endpoint 评分隔离、`SnCall` 本次候选、TTP server 入站 tunnel validator、TTP client maintained target 删除和 non-maintained idle cache release、`pn_server` 的默认 validator allow-all、显式 reject/assigned-target policy 路径、统计/限速桥接路径、proxy stream 的 TLS-over-proxy 行为、`PnTunnel` idle close 状态机和 `PnTunnel` control channel 生命周期 | `cargo test -p p2p-frame` | crate 测试通过，且 `ServerReflexive` / `S` 编解码、SN 观察地址一致/不一致 area 分类、SN command service 可从 control stream SN purpose 包装为 cmd tunnel、tunnel publish 生命周期断言、NAT 打洞调度断言、UDP punch 仅对 `ServerReflexive` QUIC candidate 开启、QUIC heartbeat interval 保持现有值且 timeout 为 30 秒、SN 候选断言、TTP server 默认 validator allow-all、显式 accept 上下文、reject/error 阻止 attach/cache 并关闭 tunnel、TTP client maintained target 删除、non-maintained idle release、active lease 保留、maintained target 保留断言、PN 默认连接验证器 allow-all、显式 reject/assigned-target policy 可拒绝、relay 统计/限速断言、proxy TLS 断言、`PnTunnel` idle close 断言与 control ready/close/heartbeat 断言成立 | unit | 源码内 `#[test]` 和 `#[tokio::test]` 测试集 |
| 工作区兼容性 | 下游使用方仍能与核心库一起编译和测试 | `cargo test --workspace` | 工作区测试通过 | integration | workspace |

## External Interface Tests
| 接口 | 职责 | 成功场景 | 失败/边界场景 | 测试类型 | 测试文档/文件 |
|------|------|----------|----------------|----------|----------------|
| `cyfs-p2p` 适配层边界 | 在无语义漂移的前提下消费 `p2p-frame` | stack 创建和身份适配成功 | 核心行为变化破坏适配层假设 | integration | `cargo test --workspace` |
| 运行时场景边界 | 支持下游运行时 crate 编译 | 下游 crate 能通过 workspace 兼容性验证 | 配置或启动回归 | integration | `cyfs-p2p-test` |

## 回归关注点
- `p2p-frame/src/networks/tcp/network.rs` 拥有密集的异步测试覆盖，必须持续与协议说明保持一致。
- `p2p-frame/src/tunnel/tunnel_manager.rs` 协调大量状态迁移，是行为回归的热点区域。
- `p2p-frame/src/tunnel/tunnel_manager.rs` 中新 tunnel 的 register/publish 生命周期收敛后，reverse waiter 命中时的延后 publish、无 waiter reverse incoming close、统一 publish 入口的幂等性，以及 `get_tunnel()` 对 published candidate 和非 proxy candidate 的偏好都会成为新的回归热点。
- `p2p-frame/src/tunnel/tunnel_manager.rs` 新增 proxy tunnel 脱代理调度后，后台重试时序、成功切换后的 publish 行为，以及失败退避上限会成为新的回归热点。
- `p2p-frame/src/tunnel/tunnel_manager.rs` 新增 NAT-aware 建链策略后，QUIC/UDP 短延迟 reverse、同源 UDP punch 调度、proxy 短窗口脱代理、endpoint 评分隔离、同一 `tunnel_id` 下 direct/reverse 竞速和 waiter 清理会成为新的回归热点。
- `p2p-frame/src/networks/quic/listener.rs`、`p2p-frame/src/networks/quic/network.rs` 与 `p2p-frame/src/networks/quic/tunnel.rs` 新增同源 UDP punch 和 heartbeat timeout 约束后，socket clone 生命周期、只发送不接收、发送失败降级、不同平台 socket 语义、candidate policy 收窄和心跳失活判定会成为新的回归热点。
- `p2p-frame/src/sn/client/sn_service.rs` 与 `p2p-frame/src/sn/service/service.rs` 新增本次建链候选传递后，`reverse_endpoint_array` 的来源、去重和单 SN 转发边界会成为新的回归热点。
- `p2p-frame/src/sn/client/sn_service.rs` 与 `p2p-frame/src/sn/service/service.rs` 新增 SN control stream 信令后，control purpose 注册、默认 control stream 发送、远端未监听或控制通道不可用时显式失败、以及不得残留旧普通 stream fallback 会成为新的回归热点。
- `p2p-frame/src/sn/service/service.rs` 收紧 runtime 所有权后，`create_sn_service(...)` 缺少外部 `ServerRuntime` 的错误路径、显式 runtime 注入启动路径和服务内部不得自行 `ServerRuntime::start(...)` 会成为新的配置回归热点。
- `p2p-frame/src/endpoint.rs` 中 `ServerReflexive` 命名、`S` 文本编解码、raw codec area 映射和公开方法删除会成为新的兼容性回归热点。
- `p2p-frame/src/sn/service/service.rs` 中 SN 观察地址归类会成为新的 NAT 候选回归热点，必须覆盖观察地址与节点自上报地址一致/不一致两类输入。
- `p2p-frame/src/pn/service/pn_server.rs` 是 PN open-result 映射和双向 bridge 启动的 relay 侧瓶颈点。
- `p2p-frame/src/pn/service/pn_server.rs` 新增 `sfo-io` 接入后，bridge 的计量口径、source/target 双边统计映射、背压和关闭顺序会成为新的回归热点。
- `p2p-frame/src/pn/client/pn_tunnel.rs` 新增 TLS-over-proxy 后，会把 proxy stream 建链与 TLS client/server 握手串到一起；同时当前实现还支持 `PnClient` 级模式配置与 passive accept 快照继承，是新的回归热点。
- `p2p-frame/src/pn/client/pn_tunnel.rs` 新增 idle close 后，会把 channel lease drop、pending/queued 计数、accept 唤醒、local open 拒绝和关闭后重新创建串到一起，是新的并发回归热点。
- `p2p-frame/src/pn/client/pn_tunnel.rs` 新增 control channel 后，会把 logical tunnel ready gate、remote close、heartbeat timeout、manual close 通知和 idle close 复用同一状态机，是新的并发和协议回归热点。
- `p2p-frame/src/pn/protocol.rs` 新增 control open/command 后，必须持续验证 control channel 与业务 `ProxyOpenReq` / `ProxyOpenResp` 不混淆，避免旧业务 channel 被误解释成 tunnel 生命周期控制面。
- `p2p-frame/src/ttp/server.rs` 新增入站 tunnel validator 后，attach/cache 前的 accept/reject/error 分支、validator 上下文字段、默认 allow-all 构造兼容性和 reject/error 后 best-effort close 都会成为新的回归热点。
- 密码学和 X509 路径改动频率较低，但一旦修改，风险更高。

## 当前改动直接验证
| Design 条目 | 验证对象 | 层级/入口 | 通过标准 |
|------------|----------|-----------|----------|
| relay bridge 上的 source/target 双边独立统计视图 | `p2p-frame/src/pn/service/pn_server.rs` 的统计查询与 bridge 计量路径 | unit: `python3 ./harness/scripts/test-run.py p2p-frame unit` | unit 测试能分别断言 source 与 target 两侧查询结果存在、互不覆盖、且只统计成功写出的 payload 字节 |
| 仅 source 侧生效的用户级限速，与 target 统计视图解耦 | `p2p-frame/src/pn/service/pn_server.rs` 的限速配置与 bridge 背压路径 | unit: `python3 ./harness/scripts/test-run.py p2p-frame unit` | unit 测试能断言 source 侧限速仍生效，而 target 侧新增统计查询不会引入新的限速行为 |
| `PnServer::new(...)` 默认连接验证器显式 allow-all | `p2p-frame/src/pn/service/pn_server.rs` 的默认构造和 validator helper | targeted unit: `cargo test -p p2p-frame pn_connection_validator -- --nocapture` | unit 测试能断言 `allow_all_pn_connection_validator()` 对规范化上下文返回 `Accept`，且 `reject_all_pn_connection_validator()` 保留显式拒绝路径 |
| 多 PN assigned target 通过显式策略路径准入 | `assigned_target_pn_connection_validator(...)` / `PnServer::new_with_assigned_target_policy(...)` | unit: `python3 ./harness/scripts/test-run.py p2p-frame unit` | 默认构造不承担 wrong-PN 拒绝；只有显式 validator 或 assigned-target policy 才能拒绝未分配目标 |
| 双边统计对默认构造路径和下游调用方保持兼容 | `PnServer::new(...)` 调用点与运行时使用方 | integration: `python3 ./harness/scripts/test-run.py p2p-frame integration` | workspace 入口继续可运行，不要求下游立刻理解新的 target 统计查询接口细节 |
| `PnTunnel` idle timeout 生命周期关闭 | `p2p-frame/src/pn/client/pn_tunnel.rs` 的状态机、channel lease、idle sweeper 和 close notify 路径 | unit: `python3 ./harness/scripts/test-run.py p2p-frame unit` | unit 测试能断言 channel 计数归零并超过 idle timeout 后 tunnel 进入 closed/error 终态，pending `accept_*` 被唤醒并失败，后续 `open_*` 立即失败 |
| `PnTunnel` 关闭后重新创建 | `p2p-frame/src/pn/client/pn_client.rs` 与 `p2p-frame/src/pn/client/pn_listener.rs` 的 inbound 分发路径 | unit: `python3 ./harness/scripts/test-run.py p2p-frame unit` | unit 测试能断言 idle close 后同 `(remote_id, tunnel_id)` 的后续 inbound `ProxyOpenReq` 不会投递到已关闭对象，而是能创建新的 passive tunnel |
| `PnTunnel` 控制通道建立与 ready gate | `p2p-frame/src/pn/protocol.rs`、`p2p-frame/src/pn/client/pn_client.rs`、`p2p-frame/src/pn/client/pn_listener.rs`、`p2p-frame/src/pn/client/pn_tunnel.rs` 的 control open/ready 和状态切换 | unit: `python3 ./harness/scripts/test-run.py p2p-frame unit` | unit 测试能断言 active `create_tunnel*` 等待 control ready 成功后才返回可用 tunnel，ready 失败/超时不复用当前 `tunnel_id`，control channel 不被业务 `ProxyOpenReq` 混用 |
| `PnTunnel` 远端关闭和控制面故障感知 | `p2p-frame/src/pn/client/pn_tunnel.rs` 的 control read loop、send control、heartbeat 和 close path | unit: `python3 ./harness/scripts/test-run.py p2p-frame unit` | unit 测试能断言 remote `Close`、control EOF、decode 失败、control write 失败和 heartbeat timeout 都让本地 tunnel 进入 closed/error 终态，唤醒 pending `accept_*` 并让后续 `open_*` 立即失败 |
| `PnTunnel` control close 与 idle/manual close 幂等 | `p2p-frame/src/pn/client/pn_tunnel.rs` 与 `p2p-frame/src/pn/client/pn_client.rs` registry 清理路径 | unit: `python3 ./harness/scripts/test-run.py p2p-frame unit` | unit 测试能断言 control close、manual close 和 idle close 任意顺序重复触发都共享同一关闭路径，不重复递减 channel 计数，不保留旧 registry entry，close 后同 key inbound open 不投递到旧对象 |
| direct/reverse 统一短延迟竞速 | `p2p-frame/src/tunnel/tunnel_manager.rs` 的 hedged connect 调度 | unit: `python3 ./harness/scripts/test-run.py p2p-frame unit` | unit 测试能断言 reverse 统一延迟 300ms 启动，且 direct/reverse 共享同一 logical `tunnel_id` |
| QUIC/UDP NAT 候选下同源 UDP punch burst | `p2p-frame/src/networks/network.rs`、`p2p-frame/src/networks/quic/listener.rs` 与 `p2p-frame/src/networks/quic/network.rs` 的 punch 发送路径 | unit: `python3 ./harness/scripts/test-run.py p2p-frame unit` | unit 测试能断言 punch 默认关闭；有 SN service 且 `TunnelManager` 为本次 candidate intent 开启后，punch 使用 QUIC listener 同一本地端口的发送句柄，只对 `EndpointArea::ServerReflexive` QUIC/UDP 候选启用，`Lan`、`Wan`、`Mapped`、TCP、IPv6、0 端口和默认 intent 路径不启用；reverse 在 `0ms` 起发、active 在 `250ms` 起发，之后都以固定 `50ms` cadence 发送并在 `1s` 截止或更短 hedged window 结束时停止；punch payload 可为每包 `5..=30` 字节随机短载荷，且首字节不得设置 QUIC fixed bit `0x40`；payload 不得被接收侧解析或上层依赖，发送失败不改变 `connect_with_ep(...)` 的建链结果 |
| QUIC tunnel heartbeat timeout | `p2p-frame/src/networks/quic/tunnel.rs` 的 heartbeat 常量和 timeout close path | unit: `python3 ./harness/scripts/test-run.py p2p-frame unit` | unit 测试能断言 QUIC heartbeat interval 保持现有 5 秒，heartbeat timeout 为 30 秒，且 timeout 后仍走既有 close/error 路径 |
| `SnCall` 本次反连候选 | `p2p-frame/src/sn/client/sn_service.rs` 与 `p2p-frame/src/sn/service/service.rs` 的 call/called 字段 | unit: `python3 ./harness/scripts/test-run.py p2p-frame unit` | unit 测试能断言 `SnCall` 携带调用方本次传入的 `reverse_endpoint_array`，SN called 保留这些候选并扩展单 SN 观察候选 |
| `EndpointArea::ServerReflexive` enum 与 codec | `p2p-frame/src/endpoint.rs` 的 enum、`Display`、`FromStr`、raw encode/decode 和公开方法 | unit: `python3 ./harness/scripts/test-run.py p2p-frame unit` | unit 测试能断言 `ServerReflexive` 使用 `S` 文本标记且 raw codec 往返保持 area；旧 `D` 不再作为新语义输入；`is_sys_default()` 不再存在，`is_static_wan()` 不包含 `ServerReflexive` |
| SN 观察地址 area 分类 | `p2p-frame/src/sn/service/service.rs` 的观察 endpoint 扩展路径 | unit: `python3 ./harness/scripts/test-run.py p2p-frame unit` | unit 测试能断言 SN 观察地址与节点自上报 endpoint 完全一致时标记 `Wan`，不一致时标记 `ServerReflexive`，映射端口候选仍不被混为 `ServerReflexive` |
| SN TCP 来源地址 mapped-only | `p2p-frame/src/sn/service/service.rs` 的 observed endpoint candidate helper 与 report/query/called 候选组装路径 | targeted unit: `cargo test -p p2p-frame sn_tcp_observed_endpoint -- --nocapture`; unit: `python3 ./harness/scripts/test-run.py p2p-frame unit` | unit 测试断言无 `map_ports` 时 TCP observed source endpoint 不返回；有 `map_ports` 时只返回来源 IP + 上报协议/端口构造出的 `Mapped` endpoint，且原始 TCP 来源端口不进入结果；非 TCP observed endpoint 继续作为直接候选并保持 `Wan` / `ServerReflexive` 分类 |
| proxy 短窗口脱代理升级 | `p2p-frame/src/tunnel/tunnel_manager.rs` 的 proxy upgrade state | unit: `python3 ./harness/scripts/test-run.py p2p-frame unit` | unit 测试能断言新建 proxy candidate 先进入 15s/30s/60s/120s 短窗口，再进入有上限的指数退避，且升级路径不把 proxy 视为成功 |
| 多个已有 candidate 的非 proxy 优先复用 | `p2p-frame/src/tunnel/tunnel_manager.rs` 的 `get_tunnel()` 选择策略 | unit: `python3 ./harness/scripts/test-run.py p2p-frame unit` | unit 测试能断言同一远端同时存在 published 非 proxy 和更新时间更晚的 published proxy 时，默认复用路径仍返回非 proxy；只有没有可用非 proxy candidate 时才返回 proxy |
| reverse incoming 无 waiter 关闭 | `p2p-frame/src/tunnel/tunnel_manager.rs` 的 incoming reverse 分支 | unit: `python3 ./harness/scripts/test-run.py p2p-frame unit` | unit 测试能断言无 waiter reverse tunnel 被关闭，不 register、不 publish，订阅者收不到，`get_tunnel()` 不返回；命中 waiter 时 incoming 分支只交付、不注册，waiter 接收方随后 register/publish |
| endpoint 评分按协议隔离 | `p2p-frame/src/tunnel/tunnel_manager.rs` 的 endpoint score | unit: `python3 ./harness/scripts/test-run.py p2p-frame unit` | unit 测试能断言 TCP 失败不会降低 QUIC/UDP 候选优先级 |
| bounded channel 容量配置化 | `p2p-frame/src/stack.rs` 顶层配置、各生产路径 mpsc queue 和测试替身 | unit: `python3 ./harness/scripts/test-run.py p2p-frame unit`；search: `rg -n 'unbounded_channel|UnboundedSender|UnboundedReceiver' p2p-frame/src -g '*.rs'` | unit 测试断言 `P2pConfig` 默认用户不配置时仍存在的各队列位置容量均为 `1024`、单个位置覆盖不影响其他位置、`P2pStackConfig` 从 `P2pEnv` 继承分位置容量且可覆盖；代码搜索确认源码中不再存在 unbounded mpsc 类型或构造函数，且 TCP/QUIC tunnel 不再保留旧 accept queue 容量参数 |
| `Tunnel` stream/datagram listen 回调化 | `p2p-frame/src/networks/tunnel.rs`、TCP/QUIC/PN tunnel 入站 channel 分发、TTP/stream/datagram manager 调用点 | unit: `python3 ./harness/scripts/test-run.py p2p-frame unit`；compile: `rustup run stable cargo test -p p2p-frame --no-run` | unit/compile 证据确认公共 `Tunnel` trait 不再要求 `accept_stream()` / `accept_datagram()`，`listen_stream(...)` / `listen_datagram(...)` 注册回调后可接收入站 stream/datagram，PN/TTP/SN fake 通过回调投递入口流，关闭后 pending callback receiver 关闭且后续 open 立即失败 |
| `Tunnel` control stream API | `p2p-frame/src/networks/tunnel.rs`、内部 `networks/control_stream.rs`、TCP/QUIC/PN 控制命令 `Data` 接入 | unit: `python3 ./harness/scripts/test-run.py p2p-frame unit`；targeted: `cargo test -p p2p-frame --features x509 control_stream`；compile: `rustup run stable cargo test -p p2p-frame --no-run` | unit/compile 证据确认公共 `Tunnel` trait 只新增 `open_control_stream(...)` / `listen_control_stream(...)` 和 callback/stream 类型，内部 runtime/frame 未公开导出；TCP/QUIC/PN 只通过单一 `Data` 命令承载内部 frame；open/listen、purpose 过滤、64KiB 切分/超限拒绝、底层控制通道关闭后所有派生 stream 和 pending open/write 失败均有覆盖；TCP/QUIC/PN tunnel 建立后均覆盖 control stream 双向收发 |
| SN control stream 信令 | `p2p-frame/src/sn/client/sn_service.rs`、`p2p-frame/src/sn/service/service.rs`、TTP control stream listener 路径 | unit: `python3 ./harness/scripts/test-run.py p2p-frame unit`；targeted: `cargo test -p p2p-frame sn_server_wraps_sn_control_stream_into_cmd_tunnel -- --nocapture` | unit/compile 证据确认 SN command service 为 SN purpose 注册 control stream listener，control stream 入站可被包装成 `CmdTunnel<SnTunnelRead, SnTunnelWrite>` 并进入既有 cmd handler；代码审查确认 client 只调用 `open_control_stream(...)`、失败后不 fallback 到普通 `open_stream(...)`，service 不为 SN command purpose 注册普通 `listen_stream(...)`，且不直接依赖内部 `control_stream` runtime/frame |
| SN server 连接验证器 | `p2p-frame/src/sn/service/service.rs` 的 validator 类型、默认 allow-all 构造路径、`SnServiceConfig::set_connection_validator(...)` 和 report/call 入站校验点 | unit: `python3 ./harness/scripts/test-run.py p2p-frame unit`；targeted: `cargo test -p p2p-frame sn_service -- --nocapture` | unit 证据确认默认 allow-all 不拒绝带客户端证书的 report；自定义 validator reject 返回 `PermissionDenied`，validator 上下文只包含 cmd tunnel `peer_id` 规范化出的 `client_id` 与匹配该 id 的 `client_cert`，且 reject 后不更新 peer manager 或继续处理 report；证书解析 id 与 `client_id` 不一致时返回 `PermissionDenied` 且不调用 validator。 |
| SN service runtime 外部注入 | `p2p-frame/src/sn/service/service.rs` 的 `create_sn_service(...)`、`SnServiceConfig::set_server_runtime(...)`、`sn-miner-rust` / `cyfs-p2p-test` 启动调用点 | targeted unit: `cargo test -p p2p-frame create_sn_service_requires_external_server_runtime -- --nocapture`; workspace compile: `cargo test --workspace --no-run`; unified unit/integration: `python3 ./harness/scripts/test-run.py p2p-frame unit` / `python3 ./harness/scripts/test-run.py p2p-frame integration`; search: `rg -n 'ServerRuntime::start|ServerRuntimeConfig' p2p-frame/src/sn/service/service.rs` | unit 证据确认缺少 runtime 时返回 `InvalidParam`；workspace 编译和 integration 证据确认 `sn-miner-rust` / `cyfs-p2p-test` 调用方迁移为显式 runtime 注入且构造路径仍可用；搜索证据确认 SN service 生产路径不再创建 service-local runtime。 |
| SN service contract/receipt 清理 | `p2p-frame/src/sn/service/service.rs`、`p2p-frame/src/sn/service/mod.rs`、`p2p-frame/src/sn/client/mod.rs`、`p2p-frame/src/sn/protocol/sn.rs` | unit: `python3 ./harness/scripts/test-run.py p2p-frame unit`；compile: `cargo check -p p2p-frame`；search: `rg -n 'SnServiceContractServer|DefaultSnServiceContractServer|set_contract|mod contract|mod receipt|receipt::' p2p-frame/src/sn p2p-frame/src/lib.rs` | 编译证据确认 `SnServiceConfig::new(...)`、`create_sn_service(...)`、SN control stream 信令和 validator 装配仍可用；搜索证据确认服务侧不再导出或构造 contract server / receipt 装配入口；`sn/protocol` 中 `contract_id` / `receipt` wire 字段和 `SnServiceReceipt` 编解码结构保留，不作为失败证据。 |
| `TtpNode` active-open connector | `p2p-frame/src/ttp/client.rs`、`p2p-frame/src/ttp/node.rs`、`p2p-frame/src/ttp/server.rs`、`p2p-frame/src/ttp/mod.rs` | targeted unit: `cargo test -p p2p-frame ttp -- --nocapture`；unit: `python3 ./harness/scripts/test-run.py p2p-frame unit`；compile: `cargo check -p p2p-frame` | unit 证据确认 `TtpNode::open_stream(...)` 和 `open_control_stream(...)` 在缺失 tunnel 时通过 `TunnelNetwork` 主动创建、attach 并打开对应 channel；已有 tunnel 可通过完整 target 或 id-only target 复用；`open_datagram(...)` 与设计一致复用同一 active-open helper；建链失败显式透传；`TtpServer` 缺失 tunnel 仍返回 `NotFound`。 |
| `TtpClient` 连接生命周期 | `p2p-frame/src/ttp/client.rs`、`p2p-frame/src/ttp/tests.rs` | targeted unit: `cargo test -p p2p-frame ttp_client -- --nocapture`；unit: `python3 ./harness/scripts/test-run.py p2p-frame unit`；compile: `cargo check -p p2p-frame` | unit 证据确认 `remove_server(&TtpTarget)` 幂等删除 exact maintained target，删除后 `configured_server_id()` 可回到 `Ok(None)`；non-maintained cache tunnel 超过 test-only idle timeout 且无 lease 时被释放并触发后续重新 create；active stream lease 阻止 idle release；maintained target 不被 non-maintained idle release 清理；代码审查确认 release 只删除本地 cache entry，不改变 tunnel trait/wire/publish/`TtpServer` 行为。 |
| `TtpServer` incoming tunnel validator | `p2p-frame/src/ttp/server.rs`、`p2p-frame/src/ttp/tests.rs` | targeted unit: `cargo test -p p2p-frame ttp:: -- --nocapture`；unit: `python3 ./harness/scripts/test-run.py p2p-frame unit`；compile: `cargo check -p p2p-frame` | unit 证据确认 `TtpServer::new(...)` 默认 allow-all 并继续 attach/cache 入站 tunnel；`new_with_incoming_tunnel_validator(...)` 的 accept 分支记录完整 validator 上下文并继续可 open stream；reject 和 validator error 在 attach/cache 前短路，后续 `open_stream(...)` 仍返回 `NotFound`，且 best-effort 调用 `Tunnel::close()`。 |

## Direct Change Coverage
| change_id | design_source | validation_id | testplan_level | testplan_step_id | Coverage | gap | gap_manual_reason |
|-----------|---------------|---------------|----------------|------------------|----------|-----|-------------------|
| endpoint_area_server_reflexive | `design.md` Directly Mapped Change Items | V-ENDPOINT-AREA-UNIT | unit | p2p-frame-unit | 覆盖 `EndpointArea::ServerReflexive` 命名、`S` 编解码、raw codec、`is_sys_default()` 删除、`is_static_wan()` 语义和 SN 观察地址分类；integration 由 `p2p-frame-integration` 继承。 | no | |
| server_reflexive_quic_nat_keepalive | `design.md` Directly Mapped Change Items | V-SR-QUIC-NAT-UNIT | unit | p2p-frame-unit | 覆盖 UDP punch candidate policy、反例、QUIC heartbeat interval 与 30 秒 timeout；integration 由 `p2p-frame-integration` 继承。 | no | |
| reverse_timeout_close_late_tunnel | `design.md` Directly Mapped Change Items | V-REV-TIMEOUT-UNIT | unit | p2p-frame-unit | 覆盖 incoming reverse 无 waiter close、不 register/publish，以及命中 waiter 后由接收方 register/publish。 | no | |
| networks_sfo_reuseport_tcp_listener | `design/sfo-reuseport-listeners.md` | V-SFO-TCP-LISTENER-UNIT | unit | p2p-frame-unit | 覆盖 TCP listener runtime 注入、`TcpServer::serve(...)` accept handler 和下游兼容编译边界。 | no | |
| networks_sfo_reuseport_quic_listener_socket | `design/sfo-reuseport-listeners.md` | V-SFO-QUIC-LISTENER-UNIT | unit | p2p-frame-unit | 覆盖 QUIC listener worker socket、custom `AsyncUdpSocket`、worker CID 和 UDP punch worker socket 使用。 | no | |
| tunnel_network_listen_callback | `design.md` Directly Mapped Change Items | V-TUNNEL-NETWORK-CALLBACK-UNIT | unit | p2p-frame-unit | 覆盖 `TunnelNetwork::listen(...)` 回调化、`NetManager` incoming dispatch/validator/publish/reject close 和 PN listener 幂等 listen。 | no | |
| bounded_channel_capacity_config | `design.md` Directly Mapped Change Items | V-BOUNDED-CHANNELS-UNIT | unit | p2p-frame-unit | 覆盖默认容量、单项覆盖、`P2pStackConfig` 继承与源码 unbounded mpsc 搜索。 | no | |
| stack_channel_capacity_config_removal | `design.md` Directly Mapped Change Items | V-STACK-CAPACITY-REMOVAL-UNIT | unit | p2p-frame-unit | 编译/搜索确认 stack 层容量 API、`NetManager` 容量字段、QUIC/TTP/PN 显式容量入口已删除。 | no | |
| tunnel_stream_datagram_listen_callback | `design/tunnel-channel-listen-callback.md` | V-TUNNEL-CHANNEL-CALLBACK-UNIT | unit | p2p-frame-unit | 覆盖公共 `Tunnel` trait 入站 stream/datagram 回调化、PN `PortNotListen` 继续分发和关闭后收敛。 | no | |
| tunnel_control_stream_api | `design/tunnel-control-stream-api.md` | V-TUNNEL-CONTROL-STREAM-UNIT | unit | p2p-frame-unit | 覆盖 control stream open/listen、purpose 过滤、64KiB 边界、底层关闭传播和 TCP/QUIC/PN `Data` 控制命令接入。 | no | |
| sn_control_stream_signaling | `design/sn-control-stream-signaling.md` | V-SN-CONTROL-STREAM-UNIT | unit | p2p-frame-unit | 覆盖 SN command service 从 SN purpose control stream 入站包装为 cmd tunnel，且 client 不 fallback 普通 stream。 | no | |
| sn_server_connection_validator | `design.md` Directly Mapped Change Items | V-SN-SERVER-VALIDATOR-UNIT | unit | p2p-frame-unit | 覆盖 SN server 默认 allow-all、validator reject、上下文只含 `client_id`/`client_cert`，以及证书 id 不匹配时不调用 validator。 | no | |
| sn_service_runtime_external_injection | `design.md` Directly Mapped Change Items | V-SN-SERVICE-RUNTIME-EXTERNAL-UNIT | unit | p2p-frame-unit | 覆盖 `create_sn_service(...)` 缺少 `ServerRuntime` 时返回 `InvalidParam`，显式 runtime 注入路径通过 compile/unit 维持可用，并通过源码搜索确认 `sn/service/service.rs` 不再包含 `ServerRuntime::start(...)` 兜底。 | no | |
| sn_client_protocol_priority | `design.md` Directly Mapped Change Items | V-SN-CLIENT-PROTOCOL-PRIORITY-UNIT | unit | p2p-frame-sn-client-protocol-priority | 覆盖 SN client 候选按 QUIC、TCP、Ext 顺序排序；支持协议但无匹配 listener 时仍生成 `local_ep = None` 出站候选；有匹配 QUIC listener 时保留 listener `local_ep`；有匹配 TCP listener 时 unspecified IP 转成 `None`，具体 IP 的 port 转成 `0`，不复用监听端口。代码审查确认候选失败日志包含 SN id、protocol、local_ep、remote endpoint 和错误，ping/report loop 按同一 SN 聚合候选，QUIC `ReportSn` 成功后停止 TCP fallback，并按 `sn_peer_id` 去重 active SN。 | no | |
| sn_tcp_source_mapped_only | `design.md` Directly Mapped Change Items | V-SN-TCP-SOURCE-MAPPED-ONLY-UNIT | unit | p2p-frame-sn-tcp-source-mapped-only | 覆盖无 `map_ports` 时 TCP observed endpoint 不返回、有 `map_ports` 时只返回 `Mapped` candidate、原始 TCP 来源端口不泄漏、非 TCP observed candidate 直接分类，以及 peer cache 不新增 raw TCP source endpoint 存储字段。 | no | |
| remove_sn_service_contract_server | `design.md` Directly Mapped Change Items | V-SN-CONTRACT-CLEANUP-COMPILE | unit | p2p-frame-unit | 编译和搜索确认 SN service contract/receipt 生产路径与公开导出已删除，SN 基础能力和 wire 兼容结构保留。 | no | |
| pn_multi_server_assigned_target | `design.md` + `design/pn-server.md` Directly Mapped Change Items | V-PN-SERVER-DEFAULT-VALIDATOR-TARGETED | unit | p2p-frame-pn-validator | `cargo test -p p2p-frame pn_connection_validator -- --nocapture` 覆盖默认 PN validator 显式 allow-all 和显式 reject helper；`p2p-frame-unit` / `p2p-frame-integration` 继续覆盖显式 assigned-target policy 与下游默认构造兼容。 | no | |
| ttp_node_active_open | `design.md` Directly Mapped Change Items | V-TTP-NODE-ACTIVE-OPEN-UNIT | unit | p2p-frame-unit | 覆盖 `TtpNode` 缺失 tunnel 时主动创建/attach 后打开 stream/control stream，已有 tunnel/id-only target 复用，`open_datagram(...)` 复用 active-open helper，建链错误透传，以及 `TtpServer` lookup-only 兼容边界。 | no | |
| ttp_client_connection_lifecycle | `design.md` Directly Mapped Change Items | V-TTP-CLIENT-LIFECYCLE-UNIT | unit | p2p-frame-unit | 覆盖 `TtpClient::remove_server(&TtpTarget)` exact target 删除、configured server set 变化、non-maintained idle cache release、active lease 阻止 release、maintained target 保留，以及不改变 `TtpServer` lookup-only / tunnel trait / wire 的兼容边界。 | no | |
| ttp_server_tunnel_accept_validator | `design.md` Directly Mapped Change Items | V-TTP-SERVER-TUNNEL-VALIDATOR-UNIT | unit | p2p-frame-unit | 覆盖 `TtpServer::new(...)` 默认 allow-all、`new_with_incoming_tunnel_validator(...)` 显式 accept、validator 上下文字段、reject/error attach/cache 前短路、后续 lookup-only `NotFound` 和 best-effort tunnel close；integration 由 `p2p-frame-integration` 继承默认构造兼容。 | no | |

## Case-Type Coverage
| change_id | case_type | required | validation_id | level | status | gap_manual_reason |
|-----------|-----------|----------|---------------|-------|--------|-------------------|
| pn_multi_server_assigned_target | normal | yes | V-PN-SERVER-DEFAULT-VALIDATOR-TARGETED | unit | covered | |
| pn_multi_server_assigned_target | boundary | yes | V-PN-SERVER-ASSIGNED-TARGET-UNIT | unit | covered | |
| pn_multi_server_assigned_target | negative | yes | V-PN-SERVER-ASSIGNED-TARGET-UNIT | unit | covered | |
| pn_multi_server_assigned_target | error | yes | V-PN-SERVER-ASSIGNED-TARGET-UNIT | unit | covered | |
| pn_multi_server_assigned_target | compatibility | yes | V-PN-SERVER-DEFAULT-VALIDATOR-INTEGRATION | integration | covered | |
| pn_multi_server_assigned_target | lifecycle | yes | V-PN-SERVER-DEFAULT-VALIDATOR-INTEGRATION | integration | covered | |
| pn_multi_server_assigned_target | cross-module | yes | V-PN-SERVER-DEFAULT-VALIDATOR-INTEGRATION | integration | covered | |
| ttp_node_active_open | normal | yes | V-TTP-NODE-ACTIVE-OPEN-UNIT | unit | covered | |
| ttp_node_active_open | boundary | yes | V-TTP-NODE-ACTIVE-OPEN-UNIT | unit | covered | |
| ttp_node_active_open | negative | yes | V-TTP-NODE-ACTIVE-OPEN-UNIT | unit | covered | |
| ttp_node_active_open | error | yes | V-TTP-NODE-ACTIVE-OPEN-UNIT | unit | covered | |
| ttp_node_active_open | compatibility | yes | V-TTP-NODE-ACTIVE-OPEN-UNIT | unit | covered | |
| ttp_node_active_open | lifecycle | yes | V-TTP-NODE-ACTIVE-OPEN-UNIT | unit | covered | |
| ttp_node_active_open | cross-module | yes | V-TTP-NODE-ACTIVE-OPEN-UNIT | unit | covered | |
| ttp_client_connection_lifecycle | normal | yes | V-TTP-CLIENT-LIFECYCLE-UNIT | unit | covered | |
| ttp_client_connection_lifecycle | boundary | yes | V-TTP-CLIENT-LIFECYCLE-UNIT | unit | covered | |
| ttp_client_connection_lifecycle | negative | yes | V-TTP-CLIENT-LIFECYCLE-UNIT | unit | covered | |
| ttp_client_connection_lifecycle | error | yes | V-TTP-CLIENT-LIFECYCLE-UNIT | unit | covered | |
| ttp_client_connection_lifecycle | compatibility | yes | V-TTP-CLIENT-LIFECYCLE-UNIT | unit | covered | |
| ttp_client_connection_lifecycle | lifecycle | yes | V-TTP-CLIENT-LIFECYCLE-UNIT | unit | covered | |
| ttp_client_connection_lifecycle | cross-module | yes | V-TTP-CLIENT-LIFECYCLE-UNIT | unit | covered | |
| ttp_server_tunnel_accept_validator | normal | yes | V-TTP-SERVER-TUNNEL-VALIDATOR-UNIT | unit | covered | |
| ttp_server_tunnel_accept_validator | boundary | yes | V-TTP-SERVER-TUNNEL-VALIDATOR-UNIT | unit | covered | |
| ttp_server_tunnel_accept_validator | negative | yes | V-TTP-SERVER-TUNNEL-VALIDATOR-UNIT | unit | covered | |
| ttp_server_tunnel_accept_validator | error | yes | V-TTP-SERVER-TUNNEL-VALIDATOR-UNIT | unit | covered | |
| ttp_server_tunnel_accept_validator | compatibility | yes | V-TTP-SERVER-TUNNEL-VALIDATOR-COMPAT | integration | covered | |
| ttp_server_tunnel_accept_validator | lifecycle | yes | V-TTP-SERVER-TUNNEL-VALIDATOR-UNIT | unit | covered | |
| ttp_server_tunnel_accept_validator | cross-module | yes | V-TTP-SERVER-TUNNEL-VALIDATOR-COMPAT | integration | covered | |
| sn_client_protocol_priority | normal | yes | V-SN-CLIENT-PROTOCOL-PRIORITY-UNIT | unit | covered | |
| sn_client_protocol_priority | boundary | yes | V-SN-CLIENT-PROTOCOL-PRIORITY-UNIT | unit | covered | |
| sn_client_protocol_priority | negative | yes | V-SN-CLIENT-PROTOCOL-PRIORITY-UNIT | unit | covered | |
| sn_client_protocol_priority | error | yes | V-SN-CLIENT-PROTOCOL-PRIORITY-UNIT | unit | covered | |
| sn_client_protocol_priority | compatibility | yes | V-SN-CLIENT-PROTOCOL-PRIORITY-UNIT | unit | covered | |
| sn_client_protocol_priority | lifecycle | yes | V-SN-CLIENT-PROTOCOL-PRIORITY-UNIT | unit | covered | |
| sn_client_protocol_priority | cross-module | yes | V-SN-CLIENT-PROTOCOL-PRIORITY-UNIT | unit | covered | |
| sn_tcp_source_mapped_only | normal | yes | V-SN-TCP-SOURCE-MAPPED-ONLY-UNIT | unit | covered | |
| sn_tcp_source_mapped_only | boundary | yes | V-SN-TCP-SOURCE-MAPPED-ONLY-UNIT | unit | covered | |
| sn_tcp_source_mapped_only | negative | yes | V-SN-TCP-SOURCE-MAPPED-ONLY-UNIT | unit | covered | |
| sn_tcp_source_mapped_only | error | yes | V-SN-TCP-SOURCE-MAPPED-ONLY-UNIT | unit | covered | |
| sn_tcp_source_mapped_only | compatibility | yes | V-SN-TCP-SOURCE-MAPPED-ONLY-UNIT | unit | covered | |
| sn_tcp_source_mapped_only | lifecycle | yes | V-SN-TCP-SOURCE-MAPPED-ONLY-UNIT | unit | covered | |
| sn_tcp_source_mapped_only | cross-module | yes | V-SN-TCP-SOURCE-MAPPED-ONLY-UNIT | unit | covered | |
| sn_service_runtime_external_injection | normal | yes | V-SN-SERVICE-RUNTIME-EXTERNAL-UNIT | unit | covered | |
| sn_service_runtime_external_injection | boundary | yes | V-SN-SERVICE-RUNTIME-EXTERNAL-UNIT | unit | covered | |
| sn_service_runtime_external_injection | negative | yes | V-SN-SERVICE-RUNTIME-EXTERNAL-UNIT | unit | covered | |
| sn_service_runtime_external_injection | error | yes | V-SN-SERVICE-RUNTIME-EXTERNAL-UNIT | unit | covered | |
| sn_service_runtime_external_injection | compatibility | yes | V-SN-SERVICE-RUNTIME-EXTERNAL-COMPAT | integration | covered | |
| sn_service_runtime_external_injection | lifecycle | yes | V-SN-SERVICE-RUNTIME-EXTERNAL-UNIT | unit | covered | |
| sn_service_runtime_external_injection | cross-module | yes | V-SN-SERVICE-RUNTIME-EXTERNAL-COMPAT | integration | covered | |

## Design Element Coverage
| element_type | design_source | derived_cases | level | status | gap_manual_reason |
|--------------|---------------|---------------|-------|--------|-------------------|
| parameter-domain | `design.md` Interfaces and Dependencies; `design/pn-server.md` Validator and Constructor Defaults | default allow-all, explicit reject, assigned-target policy | unit | covered | |
| state-transition | `design.md` Data and State; `design/pn-server.md` Relay Session State | new logical tunnel admission, relay session reuse | unit | covered | |
| failure-path | `design.md` Key Call Flows; `design/pn-server.md` Errors and Rollback | explicit reject before target open, wrong-PN via policy | unit | covered | |
| error-handling | `p2p-frame/src/pn/service/pn_server.rs` validator result mapping | reject helper and service reject tests | unit | covered | |
| invariant | approved proposal `pn_multi_server_assigned_target`; `design/pn-server.md` Downstream Compatibility | default `PnServer::new(...)` remains allow-all | unit | covered | |
| concurrency | `design/pn-server.md` Relay Session State | relay session registry guards follow-up channels without rechecking assigned target | unit | covered | |
| parameter-domain | approved proposal `ttp_server_tunnel_accept_validator`; `design.md` TTP Server Incoming Tunnel Validator | default allow-all, explicit accept, explicit reject, validator error | unit | covered | |
| state-transition | `design.md` TTP Server Incoming Tunnel Validator | validate before attach/cache, accept continues, reject/error close and stop | unit | covered | |
| failure-path | `design.md` TTP Server Incoming Tunnel Validator | validator reject returns no cached tunnel and no opened stream | unit | covered | |
| error-handling | `p2p-frame/src/ttp/server.rs` validator error mapping | validator error blocks attach/cache and closes tunnel | unit | covered | |
| invariant | approved proposal `ttp_server_tunnel_accept_validator` | `TtpServer::new(...)` remains default allow-all and lookup-only behavior remains `NotFound` without accepted tunnel | unit | covered | |
| parameter-domain | approved proposal `sn_tcp_source_mapped_only`; `design.md` SN TCP source mapped-only candidate construction | empty `map_ports`, TCP observed endpoint, non-TCP observed endpoint, mapped TCP/QUIC ports | unit | covered | |
| invariant | approved proposal `sn_tcp_source_mapped_only`; `design.md` Data and State | raw TCP observed socket address is transient and not a peer cache field | unit | covered | |
| parameter-domain | approved proposal `sn_service_runtime_external_injection`; `design.md` SN service runtime external injection | runtime present, runtime missing | unit | covered | |
| failure-path | approved proposal `sn_service_runtime_external_injection`; `design.md` SN service runtime external injection | missing runtime returns explicit configuration error | unit | covered | |
| invariant | approved proposal `sn_service_runtime_external_injection`; `design.md` SN service runtime external injection | service.rs does not create service-local `ServerRuntime` | unit | covered | |

## Validation Rationale
| Behavior or Risk | Validation Signal | Why This Is Sufficient | Gap / Manual Reason |
|------------------|-------------------|------------------------|---------------------|
| 默认 PN server 不拒绝未配置连接 | targeted unit `pn_connection_validator` | 直接执行默认 validator helper 并断言 `Accept`，覆盖用户指出的默认语义 | |
| 显式策略才承担 wrong-PN 拒绝 | unit validator reject / assigned-target policy 映射 | 默认 helper 与 reject helper 分开验证，防止默认 allow-all 掩盖策略路径 | |
| 下游默认构造兼容 | integration testplan step `p2p-frame-integration` | 工作区入口验证默认 `PnServer::new(...)` 调用方无需显式配置 validator | |
| TTP server 默认 allow-all 兼容 | targeted TTP unit and integration testplan step | `TtpServer::new(...)` 仍接收入站 tunnel，只有显式 `new_with_incoming_tunnel_validator(...)` 才改变准入策略；工作区入口覆盖默认构造兼容 | |
| TTP server 自定义拒绝短路 | TTP unit `server_reject_validator_blocks_attach_and_cache` | 直接驱动 incoming tunnel callback，断言 validator 上下文、后续 lookup-only `NotFound`、无 stream open 和 `close()` 调用 | |
| TTP server validator 错误短路 | TTP unit `server_validator_error_blocks_attach_and_cache` | 将 validator 的 `P2pError` 视为拒绝，覆盖错误路径不会 attach/cache 且 close tunnel | |
| TCP 来源地址不直接返回 | targeted SN service unit `sn_tcp_observed_endpoint_without_map_ports_is_not_returned` | 直接执行候选构造 helper，断言仅有 TCP observed endpoint 且无 `map_ports` 时返回空候选，覆盖 report/query/called 共用的候选来源 | |
| TCP 来源 IP + map_ports 构造 Mapped | targeted SN service unit `sn_tcp_observed_endpoint_with_map_ports_returns_only_mapped_candidates` | 断言结果端口来自 `map_ports` 而不是 raw TCP source port，且 area 为 `Mapped`，覆盖用户要求的外网地址构造边界 | |
| SN service 不再内部创建 runtime | targeted SN service unit `create_sn_service_requires_external_server_runtime`, workspace compile / integration, and source search | 缺少 runtime 直接返回 `InvalidParam`，`sn-miner-rust` / `cyfs-p2p-test` 显式创建并传入进程级 runtime，源码搜索确认 `service.rs` 无 `ServerRuntime::start` fallback | |

## Unit Tests
| Function or Unit | Branch or Condition | Covered Behavior | Test File | Status | Gap / Manual Reason |
|------------------|---------------------|------------------|-----------|--------|---------------------|
| `allow_all_pn_connection_validator()` | default validator path | 返回 `ValidateResult::Accept` | `p2p-frame/src/pn/service/pn_server.rs` | covered | |
| `reject_all_pn_connection_validator()` | explicit reject path | 返回 `ValidateResult::Reject` | `p2p-frame/src/pn/service/pn_server.rs` | covered | |
| `PnService::validate_proxy_open_req(...)` | validator rejects | 不打开 target stream 并返回拒绝结果 | `p2p-frame/src/pn/service/pn_server.rs` | covered | |
| `AssignedTargetConnectionValidator` | policy rejects target | wrong-PN target 由显式 policy 拒绝，默认路径不承担拒绝 | `p2p-frame/src/pn/service/pn_server.rs` | covered | |
| `TtpNode::open_stream(...)` | no matching cached tunnel | 通过 `TunnelNetwork` 主动创建 tunnel、attach 到 runtime，并打开 stream | `p2p-frame/src/ttp/tests.rs` | covered | |
| `TtpNode::open_control_stream(...)` | matching tunnel already cached | 复用首次主动创建的 tunnel，不重复建链，并打开 control stream | `p2p-frame/src/ttp/tests.rs` | covered | |
| `TtpNode::open_control_stream(...)` / `open_datagram(...)` | id-only target | `Endpoint::default()` target 可按 remote id 匹配既有 tunnel 并复用其 remote endpoint metadata | `p2p-frame/src/ttp/tests.rs` | covered | |
| `TtpNode::open_stream(...)` | tunnel creation fails | 建链错误按原始 `P2pErrorCode` 透传，且不会伪造已有 tunnel | `p2p-frame/src/ttp/tests.rs` | covered | |
| `TtpServer::open_stream(...)` | no incoming cached tunnel | 继续返回 `NotFound`，不主动创建 tunnel | `p2p-frame/src/ttp/tests.rs` | covered | |
| `create_sn_service(...)` | missing `SnServiceConfig.server_runtime` | 返回 `P2pErrorCode::InvalidParam`，不启动 service-local `ServerRuntime` | `p2p-frame/src/sn/service/service.rs` | covered | |
| `TtpClient::remove_server(...)` | exact maintained target present or absent | 幂等删除 maintained target；删除后 `configured_server_id()` 回到 `Ok(None)` | `p2p-frame/src/ttp/tests.rs` | covered | |
| `TtpClient::open_stream(...)` cache lifecycle | non-maintained idle timeout elapsed and leases are zero | 本地 cache entry 被释放，后续 open 重新 create tunnel | `p2p-frame/src/ttp/tests.rs` | covered | |
| `TtpClient::open_stream(...)` lease wrapper | active read/write lease still held | idle release 不清理 active tunnel cache，drop 后再次 release 才允许重新 create | `p2p-frame/src/ttp/tests.rs` | covered | |
| `TtpClient::connect_server(...)` maintained cache | maintained target idle timeout elapsed | non-maintained idle release 不清理 maintained target cache | `p2p-frame/src/ttp/tests.rs` | covered | |
| `TtpServer::new(...)` / `allow_all_ttp_incoming_tunnel_validator()` | default allow-all | 默认构造继续接受入站 tunnel、attach/cache 后可打开 stream，且不 close tunnel | `p2p-frame/src/ttp/tests.rs` | covered | |
| `TtpServer::new_with_incoming_tunnel_validator(...)` | explicit accept | validator 接收完整 tunnel metadata 上下文，accept 后继续 attach/cache 并可 open stream | `p2p-frame/src/ttp/tests.rs` | covered | |
| `TtpServer::register_accept_callback(...)` | validator returns `Reject` | reject 在 attach/cache 前短路，后续 lookup-only open 返回 `NotFound`，不打开 stream 并 best-effort close tunnel | `p2p-frame/src/ttp/tests.rs` | covered | |
| `TtpServer::register_accept_callback(...)` | validator returns `Err` | validator 错误在 attach/cache 前短路，后续 lookup-only open 返回 `NotFound`，不打开 stream 并 best-effort close tunnel | `p2p-frame/src/ttp/tests.rs` | covered | |
| `sort_sn_client_listener_entries(...)` | listener entries contain QUIC, TCP, and Ext protocols | SN client candidate protocol order is QUIC first, then TCP, then extension protocols | `p2p-frame/src/sn/client/sn_service.rs` | covered | |
| `sn_client_protocol_candidates(...)` | supported QUIC network exists without matching QUIC listener, TCP listener exists | SN client still creates a QUIC candidate with `local_ep = None`, ordered before TCP fallback | `p2p-frame/src/sn/client/sn_service.rs` | covered | |
| `sn_client_protocol_candidates(...)` | matching QUIC listener exists | SN client preserves the QUIC listener `local_ep` so same-origin UDP semantics remain intact | `p2p-frame/src/sn/client/sn_service.rs` | covered | |
| `sn_client_protocol_candidates(...)` | matching TCP listener uses a concrete local IP | SN client keeps the local IP but rewrites the TCP local port to `0`, so outbound bind uses an ephemeral port instead of the listening port | `p2p-frame/src/sn/client/sn_service.rs` | covered | |
| `sn_client_protocol_candidates(...)` | matching TCP listener uses an unspecified local IP | SN client emits `local_ep = None`, so outbound TCP fallback does not bind to the listening port | `p2p-frame/src/sn/client/sn_service.rs` | covered | |
| `SnService::observed_endpoint_candidates(...)` | TCP observed endpoint without `map_ports` | 不返回 raw TCP source socket address | `p2p-frame/src/sn/service/service.rs` | covered | |
| `SnService::observed_endpoint_candidates(...)` | TCP observed endpoint with `map_ports` | 只返回来源 IP + 上报协议/端口构造出的 `Mapped` endpoint，原始 TCP source port 不泄漏 | `p2p-frame/src/sn/service/service.rs` | covered | |
| `SnService::observed_endpoint_candidates(...)` | non-TCP observed endpoint | 继续按 reported endpoint 一致性分类为 direct candidate | `p2p-frame/src/sn/service/service.rs` | covered | |

## DV Tests
| Workflow | Kind | Entry | Expected Result | Test File or Script | Status | Gap / Manual Reason |
|----------|------|-------|-----------------|---------------------|--------|---------------------|
| p2p-frame DV lifecycle | lifecycle | disabled | 当前无自动 DV 入口 | `docs/versions/v0.1/modules/p2p-frame/testplan.yaml` | disabled | 当前没有满足 harness 自动完成语义的 p2p-frame DV 入口，runtime 兼容性由 unit 和 integration 承担。 |
| p2p-frame DV main workflow | main | disabled | 当前无自动 DV 入口 | `docs/versions/v0.1/modules/p2p-frame/testplan.yaml` | disabled | 当前没有满足 harness 自动完成语义的 p2p-frame DV 入口，runtime 兼容性由 unit 和 integration 承担。 |
| p2p-frame DV failure workflow | failure | disabled | 当前无自动 DV 入口 | `docs/versions/v0.1/modules/p2p-frame/testplan.yaml` | disabled | 当前没有满足 harness 自动完成语义的 p2p-frame DV 入口，runtime 兼容性由 unit 和 integration 承担。 |

## Integration Tests
| Contract or Flow | Modules Involved | Success Case | Failure Case | Test File | Status | Gap / Manual Reason |
|------------------|------------------|--------------|--------------|-----------|--------|---------------------|
| PN server default constructor compatibility | `p2p-frame`, `cyfs-p2p-test`, `sn-miner-rust` | 默认 `PnServer::new(...)` 调用方无需显式 validator | wrong-PN 拒绝只能通过显式 validator/policy 配置验证 | `docs/versions/v0.1/modules/p2p-frame/testplan.yaml` | covered | |
| TTP server default constructor compatibility | `p2p-frame`, `cyfs-p2p-test`, `sn-miner-rust` | 默认 `TtpServer::new(...)` 调用方无需显式 validator，仍接受入站 tunnel | 显式 validator reject/error 才能拒绝入站 tunnel | `docs/versions/v0.1/modules/p2p-frame/testplan.yaml` | covered | |
| SN service runtime startup caller compatibility | `p2p-frame`, `cyfs-p2p-test`, `sn-miner-rust` | workspace 内直接 `create_sn_service(...)` 调用方显式创建并传入进程级 `ServerRuntime`，不再依赖 SN service fallback | 缺少 runtime 时由 SN service unit 返回 `InvalidParam`；workspace 编译失败会暴露未迁移调用点 | `docs/versions/v0.1/modules/p2p-frame/testplan.yaml` | covered | |

## Definition of Done
- [ ] 直接子模块至少映射到一个验证面
- [ ] `testplan.yaml` 与声明的命令一致
- [ ] transport、tunnel、PN、SN 和 TTP 行为都声明了证据路径
- [ ] 针对协议/运行时/密码学/配置改动记录了触发的额外检查
- [ ] 跨 crate 边界的改动具备运行时场景证据
- [ ] `pn_server` 的统计准确性和限速行为已映射到 unit/integration 证据
- [ ] proxy tunnel `stream` 的 TLS-over-proxy 成功、失败、双端约定错配与明文兼容路径已映射到 unit/integration 证据
- [ ] `datagram` 在 `TlsRequired` 下忽略 `stream` 加密模式并保持明文兼容的路径已映射到 unit/integration 证据
- [ ] `PnTunnel` control channel 的 ready gate、remote close、control EOF/write/decode 失败、heartbeat timeout、manual close 通知与 close 幂等已映射到 unit/integration 证据
- [ ] `TunnelManager` 中 direct/proxy/普通 incoming 的“register 后立即 publish”、reverse waiter 命中时的延后 publish、以及 waiter 释放后的统一 publish 路径已映射到 unit 证据
- [ ] `TunnelManager` 中多个已有 candidate 的默认复用选择已映射到 unit 证据，且覆盖非 proxy 优先于更新时间更晚的 proxy
- [ ] `reverse_waiter` 超时、取消或失败后的清理，以及无 waiter reverse tunnel 到达时关闭且不发布的路径已映射到 unit 证据
- [ ] proxy tunnel 发布后的 direct/reverse 升级重试、失败退避和 2 小时封顶行为已映射到 unit 证据
- [ ] `PnTunnel` idle close 的 channel lease 计数、accept 唤醒、open 立即失败、active channel 未归零不触发 idle，以及关闭后重新创建已映射到 unit 证据
- [ ] 单 SN NAT 打洞优化的 direct/reverse 300ms 短延迟竞速、`SnCall` 本次候选、同源 UDP punch burst、proxy 短窗口脱代理、endpoint 评分隔离和无多 SN fanout 边界已映射到 unit/integration 证据
- [ ] `ServerReflexive` endpoint area 的 enum/codec、SN 观察地址分类和下游兼容性已映射到 unit/integration 证据
- [ ] SN client 同一 SN QUIC-first/TCP-fallback 和 active SN 去重已映射到 unit 证据
- [ ] SN server TCP 来源地址 mapped-only 行为已映射到 targeted unit 和 p2p-frame unit 证据
- [ ] bounded channel 分位置容量默认值、单项自定义覆盖、源码 unbounded 搜索和下游兼容性已映射到 unit/integration 证据
