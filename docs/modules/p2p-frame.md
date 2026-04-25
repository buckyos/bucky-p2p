# p2p-frame

## 类型
- 核心库

## 职责
- 负责传输/网络栈、tunnel 编排、PN/SN 协议行为、设备发现辅助能力、运行时抽象以及身份/TLS 支持。
- `src/networks/quic/**` 负责 QUIC listener/socket 生命周期和与 QUIC 建链同源端口相关的底层 UDP 辅助行为；这类辅助行为默认关闭，只有 `TunnelManager` 在存在 SN service 且候选符合策略时才会为本次 `TunnelConnectIntent` 开启，且不得变成上层可见的 raw UDP tunnel 或业务载荷协议。
- `src/pn/client/**` 负责 `PnTunnel` 本地生命周期、proxy channel open/accept、idle timeout 关闭，以及关闭后同一 logical tunnel 后续 open 触发重新创建的语义。
- `src/tunnel/**` 负责 tunnel 候选选择、proxy 兜底连通性，以及 proxy 已连通后的 direct/reverse 脱代理升级策略。

## 关键边界
- 范围内：
  - `src/networks/**`
  - `src/tunnel/**`
  - `src/ttp/**`
  - `src/sn/**`
  - `src/pn/**`
  - `src/pn/client/**` for PN client, listener, and tunnel behavior
  - `src/pn/service/**` for relay-side PN server, admission, and bridging behavior
  - `src/finder/**`
  - `src/datagram/**`
  - `src/dht/**`
  - `src/tls/**`
  - `src/x509*`
  - `src/stack.rs`
  - `src/endpoint.rs`
  - `src/error.rs`
- 参考设计说明：
  - `p2p-frame/docs/*.md`

## 依赖
- Rust 异步/网络/密码学相关 crate
- runtime feature flags
- 面向 CYFS 的适配层必须消费本 crate，而不是重新定义协议语义

## 下游依赖
- `cyfs-p2p`
- `cyfs-p2p-test`
- `sn-miner-rust`

## 模块级别
- Tier 0：耦合度最高、回归成本最高

## 必需的验证倾向
- 直接子模块必须具备 unit 覆盖。
- DV 必须使用可运行场景，而不只是编译检查。
- Integration 必须包含工作区级兼容性证据。
- 协议、密码学、运行时和 tunnel 相关改动会触发额外检查。
- `TunnelManager` 的连接选择、后台升级调度和退避策略变更，至少要有对应的 unit 证据。
