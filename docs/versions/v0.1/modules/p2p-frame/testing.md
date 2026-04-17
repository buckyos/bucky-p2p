---
module: p2p-frame
version: v0.1
status: draft
approved_by:
approved_at:
---

# p2p-frame 测试

## 测试文档索引
| 文档 | 主题 | 范围 |
|------|------|------|
| `testing.md` | 模块级验证基线 | 完整模块 |
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
| `tunnel` | tunnel 生命周期和 manager 行为 | none | active/passive/proxy tunnel 创建、状态迁移 | 同时存在多个 tunnel、选择回退、失败清理 | unit | `p2p-frame/src/tunnel/tunnel_manager.rs` |
| `ttp` | 复用命令/流协议 | `p2p-frame/docs/ttp_module_design.md` | 流注册、server/client 协议交互 | 无效命令流、channel 关闭 | unit | `p2p-frame/src/ttp/tests.rs` |
| `sn` | 信令与对端管理 | `p2p-frame/docs/sn_design.md` | 注册、查询、呼叫路由 | 对端缺失、并发、陈旧状态 | unit + DV | `p2p-frame/src/sn/tests.rs`、`p2p-frame/src/sn/service/*.rs` |
| `pn` | relay tunnel 行为 | `docs/versions/v0.1/modules/p2p-frame/testing/pn-server.md` | PN tunnel relay、请求校验、响应转发以及 server bridge 行为 | relay 启动失败、validator 拒绝、target 打开失败、握手不匹配 | unit + DV | `p2p-frame/src/pn/client/pn_tunnel.rs`、`p2p-frame/src/pn/service/pn_server.rs` |
| `identity_tls` | 身份、TLS、X509 辅助逻辑 | none | 证书处理和身份正确性 | 无效证书、握手不匹配、feature-gated 路径 | unit | `p2p-frame/src/x509.rs`、`p2p-frame/src/tls/**` |

## 模块级测试
| 测试项 | 覆盖边界 | 入口 | 预期结果 | 测试类型 | 测试文件/脚本 |
|--------|----------|------|----------|----------|-----------------|
| 核心库 unit 测试集 | `p2p-frame` 内部直接子模块 | `cargo test -p p2p-frame` | crate 测试通过 | unit | 源码内 `#[test]` 和 `#[tokio::test]` 测试集 |
| All-in-one 运行时场景 | 带本地 signaling/proxy 流程的 stack runtime | `cargo run -p cyfs-p2p-test -- all-in-one` | 运行时场景能够启动并完成，且无协议/运行时失败 | DV | `cyfs-p2p-test/src/main.rs` |
| 工作区兼容性 | 下游使用方仍能与核心库一起编译和测试 | `cargo test --workspace` | 工作区测试通过 | integration | workspace |

## 外部接口测试
| 接口 | 职责 | 成功场景 | 失败/边界场景 | 测试类型 | 测试文档/文件 |
|------|------|----------|----------------|----------|----------------|
| `cyfs-p2p` 适配层边界 | 在无语义漂移的前提下消费 `p2p-frame` | stack 创建和身份适配成功 | 核心行为变化破坏适配层假设 | integration | `cargo test --workspace` |
| 运行时场景边界 | 支持 `cyfs-p2p-test` 场景执行 | all-in-one 场景可运行 | 配置或启动回归 | DV | `cyfs-p2p-test/src/main.rs` |

## 回归关注点
- `p2p-frame/src/networks/tcp/network.rs` 拥有密集的异步测试覆盖，必须持续与协议说明保持一致。
- `p2p-frame/src/tunnel/tunnel_manager.rs` 协调大量状态迁移，是行为回归的热点区域。
- `p2p-frame/src/pn/service/pn_server.rs` 是 PN open-result 映射和双向 bridge 启动的 relay 侧瓶颈点。
- 密码学和 X509 路径改动频率较低，但一旦修改，风险更高。

## 完成定义
- [ ] 直接子模块至少映射到一个验证面
- [ ] `testplan.yaml` 与声明的命令一致
- [ ] transport、tunnel、PN、SN 和 TTP 行为都声明了证据路径
- [ ] 针对协议/运行时/密码学/配置改动记录了触发的额外检查
- [ ] 跨 crate 边界的改动具备运行时场景证据
