---
module: p2p-frame
version: v0.1
status: approved
approved_by: user
approved_at: 2026-05-13
---

# p2p-frame 测试

## 测试文档索引
| 文档 | 主题 | 范围 |
|------|------|------|
| `testing.md` | 模块级验证基线 | 完整模块 |
| `docs/versions/v0.1/modules/p2p-frame/testing/tunnel-publish-lifecycle.md` | `TunnelManager` 新 tunnel register/publish 生命周期验证补充 | `tunnel` |
| `docs/versions/v0.1/modules/p2p-frame/testing/tunnel-nat-traversal.md` | 单 SN NAT 打洞优化与 `ServerReflexive` endpoint area 验证补充 | `endpoint`、`tunnel`、`sn/client`、必要 `sn/service` |
| `docs/versions/v0.1/modules/p2p-frame/testing/pn-proxy-encryption.md` | proxy tunnel `stream` 的 TLS-over-proxy 验证补充 | `pn/client` |
| `docs/versions/v0.1/modules/p2p-frame/testing/pn-tunnel-idle-close.md` | `PnTunnel` idle timeout 生命周期关闭验证补充 | `pn/client` |
| `docs/versions/v0.1/modules/p2p-frame/testing/pn-tunnel-control-channel.md` | `PnTunnel` 控制通道、ready gate、heartbeat 和远端关闭感知验证补充 | `pn/client`、`pn/protocol`、必要 `pn/service` |
| `docs/versions/v0.1/modules/p2p-frame/testing/pn-server.md` | relay 侧 PN server 验证补充 | `pn/service` |
| `p2p-frame/docs/tcp_tunnel_protocol_test_cases.md` | TCP tunnel 协议用例 | transport/tunnel |

## 统一测试入口
- 机器可读计划：`docs/versions/v0.1/modules/p2p-frame/testplan.yaml`
- Unit: `python3 ./harness/scripts/test-run.py p2p-frame unit`
- DV: 当前 disabled；`cyfs-p2p-test all-in-one` 不作为 p2p-frame 或其他模块的 DV 证据
- Integration: `python3 ./harness/scripts/test-run.py p2p-frame integration`

## 子模块测试
| 子模块 | 职责 | 详细测试文档 | 必需行为 | 边界/失败场景 | 测试类型 | 测试文件 |
|--------|------|--------------|----------|----------------|----------|----------|
| `networks` | TCP/QUIC 传输和 listener 行为 | `p2p-frame/docs/tcp_tunnel_protocol_test_cases.md` | 连接建立、复用、地址处理、network manager 行为 | listener 失败、连接复用边界、协议不匹配 | unit + integration | `p2p-frame/src/networks/tcp/network.rs`、`p2p-frame/src/networks/net_manager.rs`、`p2p-frame/src/networks/quic/network.rs` |
| `endpoint` | endpoint area、协议和地址编解码 | `docs/versions/v0.1/modules/p2p-frame/testing/tunnel-nat-traversal.md` | `ServerReflexive` enum 命名、`S` 文本编解码、raw codec area 映射、`is_sys_default()` 移除 | 旧 `D` 文本不得继续作为新语义输入，`ServerReflexive` 不得被 `is_static_wan()` 判定为静态公网 | unit | `p2p-frame/src/endpoint.rs` |
| `tunnel` | tunnel 生命周期和 manager 行为 | `docs/versions/v0.1/modules/p2p-frame/testing/tunnel-publish-lifecycle.md`、`docs/versions/v0.1/modules/p2p-frame/testing/tunnel-nat-traversal.md` | active/passive/proxy tunnel 创建、统一 register/publish 生命周期、reverse waiter 命中时的延后 publish 和完成后的 publish、reverse incoming 无 waiter close、proxy tunnel 发布后的后台 direct/reverse 升级重试、已有多个 candidate 时非 proxy 优先复用、direct/reverse 统一 300ms 短延迟竞速、SN 存在时仅对 `ServerReflexive` QUIC candidate 开启同源 UDP punch 调度、proxy 短窗口脱代理、按协议隔离 endpoint 评分、`ServerReflexive` 与静态 `Wan` / `Mapped` 区分 | 同时存在多个 tunnel、选择回退、后注册 proxy 不得覆盖已有可用非 proxy candidate、reverse waiter 命中时的延后 publish、无 waiter reverse incoming close 且不 register/publish、失败清理、proxy 升级路径持续失败后的退避封顶，升级流程不得回退成 proxy，TCP 失败不得拖累 QUIC/UDP 候选，默认 intent、无 SN service 和非 `ServerReflexive` 路径不得发送 punch，punch 发送失败不得改变建链结果，SN 反射地址不得被误判为静态公网 | unit | `p2p-frame/src/tunnel/tunnel_manager.rs`、`p2p-frame/src/networks/quic/network.rs`、`p2p-frame/src/networks/quic/tunnel.rs` |
| `ttp` | 复用命令/流协议 | `p2p-frame/docs/ttp_module_design.md` | 流注册、server/client 协议交互 | 无效命令流、channel 关闭 | unit | `p2p-frame/src/ttp/tests.rs` |
| `sn` | 信令与对端管理 | `p2p-frame/docs/sn_design.md`、`docs/versions/v0.1/modules/p2p-frame/testing/tunnel-nat-traversal.md` | 注册、查询、呼叫路由、`SnCall` 携带本次 `reverse_endpoint_array`、SN called 转发保留调用方候选并扩展单 SN 观察候选、SN 观察地址按节点自上报地址一致性标记 `Wan` 或 `ServerReflexive` | 对端缺失、并发、候选去重和单 SN 边界，观察地址不一致时不得标记为 `Wan` | unit + integration | `p2p-frame/src/sn/tests.rs`、`p2p-frame/src/sn/client/sn_service.rs`、`p2p-frame/src/sn/service/*.rs` |
| `pn` | relay tunnel 行为 | `docs/versions/v0.1/modules/p2p-frame/testing/pn-server.md`、`docs/versions/v0.1/modules/p2p-frame/testing/pn-proxy-encryption.md`、`docs/versions/v0.1/modules/p2p-frame/testing/pn-tunnel-idle-close.md`、`docs/versions/v0.1/modules/p2p-frame/testing/pn-tunnel-control-channel.md` | PN tunnel relay、请求校验、响应转发、source/target 双边用户流量统计、仅 source 侧生效的 server bridge 限速、proxy stream 的 TLS-over-proxy 行为、client 级 TLS 模式配置快照、`datagram` 在 `TlsRequired` 下继续保持明文兼容、`PnTunnel` idle timeout 本地关闭，以及 `PnTunnel` control channel ready gate、heartbeat 和远端关闭感知 | relay 启动失败、validator 拒绝、target 打开失败、control ready 失败或超时、control 与业务 channel 混淆、control EOF/decode/write 失败未关闭 tunnel、heartbeat timeout 不收敛、remote close 后 open/accept 仍挂起、双端 TLS 约定不一致、TLS 证书校验失败、`datagram` 错误继承 `stream` TLS 模式、client 级模式误用导致后续 tunnel 意外继承 TLS、idle close 误关闭 active channel、idle close 后 accept 不醒、open 继续等待 timeout、迟到 inbound open 被错误投递到 closed tunnel、统计失真、source/target 串户、target 统计错误触发限速、限速背压异常 | unit + integration | `p2p-frame/src/pn/protocol.rs`、`p2p-frame/src/pn/client/pn_tunnel.rs`、`p2p-frame/src/pn/client/pn_client.rs`、`p2p-frame/src/pn/client/pn_listener.rs`、`p2p-frame/src/pn/service/pn_server.rs` |
| `identity_tls` | 身份、TLS、X509 辅助逻辑 | none | 证书处理和身份正确性 | 无效证书、握手不匹配、feature-gated 路径 | unit | `p2p-frame/src/x509.rs`、`p2p-frame/src/tls/**` |

## 模块级测试
| 测试项 | 覆盖边界 | 入口 | 预期结果 | 测试类型 | 测试文件/脚本 |
|--------|----------|------|----------|----------|-----------------|
| 核心库 unit 测试集 | `p2p-frame` 内部直接子模块，包括 endpoint area 编解码、SN 观察地址分类、`TunnelManager` 的统一 publish 生命周期、NAT-aware direct/reverse 竞速、`ServerReflexive` only 同源 UDP punch 调度、QUIC heartbeat timeout、proxy 短窗口脱代理、endpoint 评分隔离、`SnCall` 本次候选、`pn_server` 的统计/限速桥接路径、proxy stream 的 TLS-over-proxy 行为、`PnTunnel` idle close 状态机和 `PnTunnel` control channel 生命周期 | `cargo test -p p2p-frame` | crate 测试通过，且 `ServerReflexive` / `S` 编解码、SN 观察地址一致/不一致 area 分类、tunnel publish 生命周期断言、NAT 打洞调度断言、UDP punch 仅对 `ServerReflexive` QUIC candidate 开启、QUIC heartbeat interval 保持现有值且 timeout 为 30 秒、SN 候选断言、relay 统计/限速断言、proxy TLS 断言、`PnTunnel` idle close 断言与 control ready/close/heartbeat 断言成立 | unit | 源码内 `#[test]` 和 `#[tokio::test]` 测试集 |
| 工作区兼容性 | 下游使用方仍能与核心库一起编译和测试 | `cargo test --workspace` | 工作区测试通过 | integration | workspace |

## 外部接口测试
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
- `p2p-frame/src/endpoint.rs` 中 `ServerReflexive` 命名、`S` 文本编解码、raw codec area 映射和公开方法删除会成为新的兼容性回归热点。
- `p2p-frame/src/sn/service/service.rs` 中 SN 观察地址归类会成为新的 NAT 候选回归热点，必须覆盖观察地址与节点自上报地址一致/不一致两类输入。
- `p2p-frame/src/pn/service/pn_server.rs` 是 PN open-result 映射和双向 bridge 启动的 relay 侧瓶颈点。
- `p2p-frame/src/pn/service/pn_server.rs` 新增 `sfo-io` 接入后，bridge 的计量口径、source/target 双边统计映射、背压和关闭顺序会成为新的回归热点。
- `p2p-frame/src/pn/client/pn_tunnel.rs` 新增 TLS-over-proxy 后，会把 proxy stream 建链与 TLS client/server 握手串到一起；同时当前实现还支持 `PnClient` 级模式配置与 passive accept 快照继承，是新的回归热点。
- `p2p-frame/src/pn/client/pn_tunnel.rs` 新增 idle close 后，会把 channel lease drop、pending/queued 计数、accept 唤醒、local open 拒绝和关闭后重新创建串到一起，是新的并发回归热点。
- `p2p-frame/src/pn/client/pn_tunnel.rs` 新增 control channel 后，会把 logical tunnel ready gate、remote close、heartbeat timeout、manual close 通知和 idle close 复用同一状态机，是新的并发和协议回归热点。
- `p2p-frame/src/pn/protocol.rs` 新增 control open/command 后，必须持续验证 control channel 与业务 `ProxyOpenReq` / `ProxyOpenResp` 不混淆，避免旧业务 channel 被误解释成 tunnel 生命周期控制面。
- 密码学和 X509 路径改动频率较低，但一旦修改，风险更高。

## 当前改动直接验证
| Design 条目 | 验证对象 | 层级/入口 | 通过标准 |
|------------|----------|-----------|----------|
| relay bridge 上的 source/target 双边独立统计视图 | `p2p-frame/src/pn/service/pn_server.rs` 的统计查询与 bridge 计量路径 | unit: `python3 ./harness/scripts/test-run.py p2p-frame unit` | unit 测试能分别断言 source 与 target 两侧查询结果存在、互不覆盖、且只统计成功写出的 payload 字节 |
| 仅 source 侧生效的用户级限速，与 target 统计视图解耦 | `p2p-frame/src/pn/service/pn_server.rs` 的限速配置与 bridge 背压路径 | unit: `python3 ./harness/scripts/test-run.py p2p-frame unit` | unit 测试能断言 source 侧限速仍生效，而 target 侧新增统计查询不会引入新的限速行为 |
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
| proxy 短窗口脱代理升级 | `p2p-frame/src/tunnel/tunnel_manager.rs` 的 proxy upgrade state | unit: `python3 ./harness/scripts/test-run.py p2p-frame unit` | unit 测试能断言新建 proxy candidate 先进入 15s/30s/60s/120s 短窗口，再进入有上限的指数退避，且升级路径不把 proxy 视为成功 |
| 多个已有 candidate 的非 proxy 优先复用 | `p2p-frame/src/tunnel/tunnel_manager.rs` 的 `get_tunnel()` 选择策略 | unit: `python3 ./harness/scripts/test-run.py p2p-frame unit` | unit 测试能断言同一远端同时存在 published 非 proxy 和更新时间更晚的 published proxy 时，默认复用路径仍返回非 proxy；只有没有可用非 proxy candidate 时才返回 proxy |
| reverse incoming 无 waiter 关闭 | `p2p-frame/src/tunnel/tunnel_manager.rs` 的 incoming reverse 分支 | unit: `python3 ./harness/scripts/test-run.py p2p-frame unit` | unit 测试能断言无 waiter reverse tunnel 被关闭，不 register、不 publish，订阅者收不到，`get_tunnel()` 不返回；命中 waiter 时 incoming 分支只交付、不注册，waiter 接收方随后 register/publish |
| endpoint 评分按协议隔离 | `p2p-frame/src/tunnel/tunnel_manager.rs` 的 endpoint score | unit: `python3 ./harness/scripts/test-run.py p2p-frame unit` | unit 测试能断言 TCP 失败不会降低 QUIC/UDP 候选优先级 |

## Direct Change Coverage
| change_id | validation_id | testplan_level | testplan_step_id | Coverage | gap | gap_manual_reason |
|-----------|---------------|----------------|------------------|----------|-----|-------------------|
| endpoint_area_server_reflexive | V-ENDPOINT-AREA-UNIT | unit | p2p-frame-unit | 覆盖 `EndpointArea::ServerReflexive` 命名、`S` 文本编解码、raw codec 往返、`is_sys_default()` 删除、`is_static_wan()` 不包含 `ServerReflexive`，以及 SN 观察地址一致/不一致时的 `Wan` / `ServerReflexive` 分类。 | no | |
| endpoint_area_server_reflexive | V-ENDPOINT-AREA-DV | dv |  | 当前无自动 DV 入口；`cyfs-p2p-test all-in-one` 不作为模块 DV 证据。 | yes | 当前没有满足 harness 自动完成语义的 p2p-frame DV 入口，runtime 兼容性由 unit 和 integration 承担。 |
| endpoint_area_server_reflexive | V-ENDPOINT-AREA-INTEGRATION | integration | p2p-frame-integration | 运行 workspace 测试，确认公开 enum/method 和文本编码变更不会破坏下游编译测试边界。 | no | |
| server_reflexive_quic_nat_keepalive | V-SR-QUIC-NAT-UNIT | unit | p2p-frame-unit | 覆盖 UDP punch candidate policy 只接受 `ServerReflexive` QUIC endpoint，反例覆盖 `Lan`、`Wan`、`Mapped`、TCP、IPv6、0 端口和无 SN service；覆盖 QUIC heartbeat interval 保持 5 秒且 timeout 为 30 秒。 | no | |
| server_reflexive_quic_nat_keepalive | V-SR-QUIC-NAT-DV | dv |  | 当前无自动 DV 入口；`cyfs-p2p-test all-in-one` 不作为模块 DV 证据。 | yes | 当前没有满足 harness 自动完成语义的 p2p-frame DV 入口，runtime 兼容性由 unit 和 integration 承担。 |
| server_reflexive_quic_nat_keepalive | V-SR-QUIC-NAT-INTEGRATION | integration | p2p-frame-integration | 运行 workspace 测试，确认 punch policy 收窄和 heartbeat timeout 调整不会要求下游修改公开 trait 或新增 raw UDP 处理。 | no | |
| reverse_timeout_close_late_tunnel | V-REV-TIMEOUT-UNIT | unit | p2p-frame-unit | 覆盖 reverse incoming 无同 `(remote_id, tunnel_id)` waiter 时被关闭，且不会 register、publish、进入订阅流或被 `get_tunnel()` 返回；覆盖命中 waiter 时 incoming 分支只交付、不注册，waiter 接收方随后 register/publish。 | no | |
| reverse_timeout_close_late_tunnel | V-REV-TIMEOUT-DV | dv |  | 当前无自动 DV 入口；`cyfs-p2p-test all-in-one` 不作为模块 DV 证据。 | yes | 当前没有满足 harness 自动完成语义的 p2p-frame DV 入口，runtime 兼容性由 unit 和 integration 承担。 |
| reverse_timeout_close_late_tunnel | V-REV-TIMEOUT-INTEGRATION | integration | p2p-frame-integration | 运行 workspace 测试，确认 reverse incoming 无 waiter close 不改变公开 trait、SN 协议或下游构造路径。 | no | |

## 完成定义
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
