---
module: p2p-frame
version: v0.1
status: approved
approved_by: user
approved_at: 2026-04-25
---

# p2p-frame 测试

## 测试文档索引
| 文档 | 主题 | 范围 |
|------|------|------|
| `testing.md` | 模块级验证基线 | 完整模块 |
| `docs/versions/v0.1/modules/p2p-frame/testing/tunnel-publish-lifecycle.md` | `TunnelManager` 新 tunnel register/publish 生命周期验证补充 | `tunnel` |
| `docs/versions/v0.1/modules/p2p-frame/testing/tunnel-nat-traversal.md` | 单 SN NAT 打洞优化验证补充 | `tunnel`、`sn/client`、必要 `sn/service` |
| `docs/versions/v0.1/modules/p2p-frame/testing/pn-proxy-encryption.md` | proxy tunnel `stream` 的 TLS-over-proxy 验证补充 | `pn/client` |
| `docs/versions/v0.1/modules/p2p-frame/testing/pn-tunnel-idle-close.md` | `PnTunnel` idle timeout 生命周期关闭验证补充 | `pn/client` |
| `docs/versions/v0.1/modules/p2p-frame/testing/pn-server.md` | relay 侧 PN server 验证补充 | `pn/service` |
| `p2p-frame/docs/tcp_tunnel_protocol_test_cases.md` | TCP tunnel 协议用例 | transport/tunnel |

## 统一测试入口
- 机器可读计划：`docs/versions/v0.1/modules/p2p-frame/testplan.yaml`
- Unit: `python3 ./harness/scripts/test-run.py p2p-frame unit`
- DV: `python3 ./harness/scripts/test-run.py p2p-frame dv`
- Integration: `python3 ./harness/scripts/test-run.py p2p-frame integration`

## 子模块测试
| 子模块 | 职责 | 详细测试文档 | 必需行为 | 边界/失败场景 | 测试类型 | 测试文件 |
|--------|------|--------------|----------|----------------|----------|----------|
| `networks` | TCP/QUIC 传输和 listener 行为 | `p2p-frame/docs/tcp_tunnel_protocol_test_cases.md` | 连接建立、复用、地址处理、network manager 行为 | listener 失败、连接复用边界、协议不匹配 | unit + DV | `p2p-frame/src/networks/tcp/network.rs`、`p2p-frame/src/networks/net_manager.rs`、`p2p-frame/src/networks/quic/network.rs` |
| `tunnel` | tunnel 生命周期和 manager 行为 | `docs/versions/v0.1/modules/p2p-frame/testing/tunnel-publish-lifecycle.md`、`docs/versions/v0.1/modules/p2p-frame/testing/tunnel-nat-traversal.md` | active/passive/proxy tunnel 创建、统一 register/publish 生命周期、reverse waiter 交付后的 publish、proxy tunnel 发布后的后台 direct/reverse 升级重试、direct/reverse 统一 300ms 短延迟竞速、SN 存在时的同源 UDP punch 调度、proxy 短窗口脱代理、按协议隔离 endpoint 评分 | 同时存在多个 tunnel、选择回退、reverse waiter 命中时的延后 publish、waiter 清理后的 later-arriving reverse publish、失败清理、proxy 升级路径持续失败后的退避封顶，升级流程不得回退成 proxy，TCP 失败不得拖累 QUIC/UDP 候选，默认 intent 和无 SN service 路径不得发送 punch，punch 发送失败不得改变建链结果 | unit | `p2p-frame/src/tunnel/tunnel_manager.rs`、`p2p-frame/src/networks/quic/network.rs` |
| `ttp` | 复用命令/流协议 | `p2p-frame/docs/ttp_module_design.md` | 流注册、server/client 协议交互 | 无效命令流、channel 关闭 | unit | `p2p-frame/src/ttp/tests.rs` |
| `sn` | 信令与对端管理 | `p2p-frame/docs/sn_design.md`、`docs/versions/v0.1/modules/p2p-frame/testing/tunnel-nat-traversal.md` | 注册、查询、呼叫路由、`SnCall` 携带本次 `reverse_endpoint_array`、SN called 转发保留调用方候选并扩展单 SN 观察候选 | 对端缺失、并发、候选去重和单 SN 边界 | unit + DV | `p2p-frame/src/sn/tests.rs`、`p2p-frame/src/sn/client/sn_service.rs`、`p2p-frame/src/sn/service/*.rs` |
| `pn` | relay tunnel 行为 | `docs/versions/v0.1/modules/p2p-frame/testing/pn-server.md`、`docs/versions/v0.1/modules/p2p-frame/testing/pn-proxy-encryption.md`、`docs/versions/v0.1/modules/p2p-frame/testing/pn-tunnel-idle-close.md` | PN tunnel relay、请求校验、响应转发、source/target 双边用户流量统计、仅 source 侧生效的 server bridge 限速、proxy stream 的 TLS-over-proxy 行为、client 级 TLS 模式配置快照、`datagram` 在 `TlsRequired` 下继续保持明文兼容，以及 `PnTunnel` idle timeout 本地关闭 | relay 启动失败、validator 拒绝、target 打开失败、双端 TLS 约定不一致、TLS 证书校验失败、`datagram` 错误继承 `stream` TLS 模式、client 级模式误用导致后续 tunnel 意外继承 TLS、idle close 误关闭 active channel、idle close 后 accept 不醒、open 继续等待 timeout、迟到 inbound open 被错误投递到 closed tunnel、统计失真、source/target 串户、target 统计错误触发限速、限速背压异常 | unit + DV | `p2p-frame/src/pn/client/pn_tunnel.rs`、`p2p-frame/src/pn/client/pn_client.rs`、`p2p-frame/src/pn/client/pn_listener.rs`、`p2p-frame/src/pn/service/pn_server.rs` |
| `identity_tls` | 身份、TLS、X509 辅助逻辑 | none | 证书处理和身份正确性 | 无效证书、握手不匹配、feature-gated 路径 | unit | `p2p-frame/src/x509.rs`、`p2p-frame/src/tls/**` |

## 模块级测试
| 测试项 | 覆盖边界 | 入口 | 预期结果 | 测试类型 | 测试文件/脚本 |
|--------|----------|------|----------|----------|-----------------|
| 核心库 unit 测试集 | `p2p-frame` 内部直接子模块，包括 `TunnelManager` 的统一 publish 生命周期、NAT-aware direct/reverse 竞速、同源 UDP punch 调度、proxy 短窗口脱代理、endpoint 评分隔离、`SnCall` 本次候选、`pn_server` 的统计/限速桥接路径、proxy stream 的 TLS-over-proxy 行为和 `PnTunnel` idle close 状态机 | `cargo test -p p2p-frame` | crate 测试通过，且 tunnel publish 生命周期断言、NAT 打洞调度断言、UDP punch 调度断言、SN 候选断言、relay 统计/限速断言、proxy TLS 断言与 `PnTunnel` idle close 断言成立 | unit | 源码内 `#[test]` 和 `#[tokio::test]` 测试集 |
| All-in-one 运行时场景 | 带本地 signaling/proxy 流程的 stack runtime | `cargo run -p cyfs-p2p-test -- all-in-one` | 运行时场景能够在当前显式启用 `proxy_stream_encrypted` 的 stack 配置下启动并完成，且无协议/运行时失败 | DV | `cyfs-p2p-test/src/main.rs` |
| 工作区兼容性 | 下游使用方仍能与核心库一起编译和测试 | `cargo test --workspace` | 工作区测试通过 | integration | workspace |

## 外部接口测试
| 接口 | 职责 | 成功场景 | 失败/边界场景 | 测试类型 | 测试文档/文件 |
|------|------|----------|----------------|----------|----------------|
| `cyfs-p2p` 适配层边界 | 在无语义漂移的前提下消费 `p2p-frame` | stack 创建和身份适配成功 | 核心行为变化破坏适配层假设 | integration | `cargo test --workspace` |
| 运行时场景边界 | 支持 `cyfs-p2p-test` 场景执行 | all-in-one 场景可运行 | 配置或启动回归 | DV | `cyfs-p2p-test/src/main.rs` |

## 回归关注点
- `p2p-frame/src/networks/tcp/network.rs` 拥有密集的异步测试覆盖，必须持续与协议说明保持一致。
- `p2p-frame/src/tunnel/tunnel_manager.rs` 协调大量状态迁移，是行为回归的热点区域。
- `p2p-frame/src/tunnel/tunnel_manager.rs` 中新 tunnel 的 register/publish 生命周期收敛后，reverse waiter 命中时的延后 publish、waiter 清理后的 later-arriving reverse、统一 publish 入口的幂等性，以及 `get_tunnel()` 对 published candidate 的偏好都会成为新的回归热点。
- `p2p-frame/src/tunnel/tunnel_manager.rs` 新增 proxy tunnel 脱代理调度后，后台重试时序、成功切换后的 publish 行为，以及失败退避上限会成为新的回归热点。
- `p2p-frame/src/tunnel/tunnel_manager.rs` 新增 NAT-aware 建链策略后，QUIC/UDP 短延迟 reverse、同源 UDP punch 调度、proxy 短窗口脱代理、endpoint 评分隔离、同一 `tunnel_id` 下 direct/reverse 竞速和 waiter 清理会成为新的回归热点。
- `p2p-frame/src/networks/quic/listener.rs` 与 `p2p-frame/src/networks/quic/network.rs` 新增同源 UDP punch 后，socket clone 生命周期、只发送不接收、发送失败降级和不同平台 socket 语义会成为新的回归热点。
- `p2p-frame/src/sn/client/sn_service.rs` 与 `p2p-frame/src/sn/service/service.rs` 新增本次建链候选传递后，`reverse_endpoint_array` 的来源、去重和单 SN 转发边界会成为新的回归热点。
- `p2p-frame/src/pn/service/pn_server.rs` 是 PN open-result 映射和双向 bridge 启动的 relay 侧瓶颈点。
- `p2p-frame/src/pn/service/pn_server.rs` 新增 `sfo-io` 接入后，bridge 的计量口径、source/target 双边统计映射、背压和关闭顺序会成为新的回归热点。
- `p2p-frame/src/pn/client/pn_tunnel.rs` 新增 TLS-over-proxy 后，会把 proxy stream 建链与 TLS client/server 握手串到一起；同时当前实现还支持 `PnClient` 级模式配置与 passive accept 快照继承，是新的回归热点。
- `p2p-frame/src/pn/client/pn_tunnel.rs` 新增 idle close 后，会把 channel lease drop、pending/queued 计数、accept 唤醒、local open 拒绝和关闭后重新创建串到一起，是新的并发回归热点。
- 密码学和 X509 路径改动频率较低，但一旦修改，风险更高。

## 当前改动直接验证
| Design 条目 | 验证对象 | 层级/入口 | 通过标准 |
|------------|----------|-----------|----------|
| relay bridge 上的 source/target 双边独立统计视图 | `p2p-frame/src/pn/service/pn_server.rs` 的统计查询与 bridge 计量路径 | unit: `python3 ./harness/scripts/test-run.py p2p-frame unit` | unit 测试能分别断言 source 与 target 两侧查询结果存在、互不覆盖、且只统计成功写出的 payload 字节 |
| 仅 source 侧生效的用户级限速，与 target 统计视图解耦 | `p2p-frame/src/pn/service/pn_server.rs` 的限速配置与 bridge 背压路径 | unit: `python3 ./harness/scripts/test-run.py p2p-frame unit` | unit 测试能断言 source 侧限速仍生效，而 target 侧新增统计查询不会引入新的限速行为 |
| 双边统计对默认构造路径和下游调用方保持兼容 | `PnServer::new(...)` 调用点与运行时场景 | dv: `python3 ./harness/scripts/test-run.py p2p-frame dv`；integration: `python3 ./harness/scripts/test-run.py p2p-frame integration` | all-in-one 与 workspace 入口继续可运行，不要求下游立刻理解新的 target 统计查询接口细节 |
| `PnTunnel` idle timeout 生命周期关闭 | `p2p-frame/src/pn/client/pn_tunnel.rs` 的状态机、channel lease、idle sweeper 和 close notify 路径 | unit: `python3 ./harness/scripts/test-run.py p2p-frame unit` | unit 测试能断言 channel 计数归零并超过 idle timeout 后 tunnel 进入 closed/error 终态，pending `accept_*` 被唤醒并失败，后续 `open_*` 立即失败 |
| `PnTunnel` 关闭后重新创建 | `p2p-frame/src/pn/client/pn_client.rs` 与 `p2p-frame/src/pn/client/pn_listener.rs` 的 inbound 分发路径 | unit: `python3 ./harness/scripts/test-run.py p2p-frame unit` | unit 测试能断言 idle close 后同 `(remote_id, tunnel_id)` 的后续 inbound `ProxyOpenReq` 不会投递到已关闭对象，而是能创建新的 passive tunnel |
| direct/reverse 统一短延迟竞速 | `p2p-frame/src/tunnel/tunnel_manager.rs` 的 hedged connect 调度 | unit: `python3 ./harness/scripts/test-run.py p2p-frame unit` | unit 测试能断言 reverse 统一延迟 300ms 启动，且 direct/reverse 共享同一 logical `tunnel_id` |
| QUIC/UDP NAT 候选下同源 UDP punch burst | `p2p-frame/src/networks/network.rs`、`p2p-frame/src/networks/quic/listener.rs` 与 `p2p-frame/src/networks/quic/network.rs` 的 punch 发送路径 | unit: `python3 ./harness/scripts/test-run.py p2p-frame unit` | unit 测试能断言 punch 默认关闭；有 SN service 且 `TunnelManager` 为本次 candidate intent 开启后，punch 使用 QUIC listener 同一本地端口的发送句柄，只对非 WAN UDP 候选启用，有发送次数/间隔上限，发送失败不改变 `connect_with_ep(...)` 的建链结果 |
| `SnCall` 本次反连候选 | `p2p-frame/src/sn/client/sn_service.rs` 与 `p2p-frame/src/sn/service/service.rs` 的 call/called 字段 | unit: `python3 ./harness/scripts/test-run.py p2p-frame unit` | unit 测试能断言 `SnCall` 携带调用方本次传入的 `reverse_endpoint_array`，SN called 保留这些候选并扩展单 SN 观察候选 |
| proxy 短窗口脱代理升级 | `p2p-frame/src/tunnel/tunnel_manager.rs` 的 proxy upgrade state | unit: `python3 ./harness/scripts/test-run.py p2p-frame unit` | unit 测试能断言新建 proxy candidate 先进入 15s/30s/60s/120s 短窗口，再进入有上限的指数退避，且升级路径不把 proxy 视为成功 |
| endpoint 评分按协议隔离 | `p2p-frame/src/tunnel/tunnel_manager.rs` 的 endpoint score | unit: `python3 ./harness/scripts/test-run.py p2p-frame unit` | unit 测试能断言 TCP 失败不会降低 QUIC/UDP 候选优先级 |

## 完成定义
- [ ] 直接子模块至少映射到一个验证面
- [ ] `testplan.yaml` 与声明的命令一致
- [ ] transport、tunnel、PN、SN 和 TTP 行为都声明了证据路径
- [ ] 针对协议/运行时/密码学/配置改动记录了触发的额外检查
- [ ] 跨 crate 边界的改动具备运行时场景证据
- [ ] `pn_server` 的统计准确性和限速行为已映射到 unit/DV/integration 证据
- [ ] proxy tunnel `stream` 的 TLS-over-proxy 成功、失败、双端约定错配与明文兼容路径已映射到 unit/DV/integration 证据
- [ ] `datagram` 在 `TlsRequired` 下忽略 `stream` 加密模式并保持明文兼容的路径已映射到 unit/DV/integration 证据
- [ ] `TunnelManager` 中 direct/proxy/普通 incoming 的“register 后立即 publish”、reverse waiter 命中时的延后 publish、以及 waiter 释放后的统一 publish 路径已映射到 unit 证据
- [ ] `reverse_waiter` 超时、取消或失败后的清理，以及清理后 reverse tunnel 到达时不再隐藏的路径已映射到 unit 证据
- [ ] proxy tunnel 发布后的 direct/reverse 升级重试、失败退避和 2 小时封顶行为已映射到 unit 证据
- [ ] `PnTunnel` idle close 的 channel lease 计数、accept 唤醒、open 立即失败、active channel 未归零不触发 idle，以及关闭后重新创建已映射到 unit 证据
- [ ] 单 SN NAT 打洞优化的 direct/reverse 300ms 短延迟竞速、`SnCall` 本次候选、同源 UDP punch burst、proxy 短窗口脱代理、endpoint 评分隔离和无多 SN fanout 边界已映射到 unit/DV/integration 证据
