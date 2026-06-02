# p2p-frame

## 类型
- 核心库

## 职责
- 负责传输/网络栈、tunnel 编排、PN/SN 协议行为、设备发现辅助能力、运行时抽象以及身份/TLS 支持。
- `src/networks/quic/**` 负责 QUIC listener/socket 生命周期、QUIC tunnel 控制心跳和与 QUIC 建链同源端口相关的底层 UDP 辅助行为；QUIC listener 可基于 `sfo_reuseport::QuicServer::serve_socket(...)` 与 Quinn `AsyncUdpSocket` 适配器接入，每个 worker socket 对应一个 Quinn endpoint，并使用 worker-shard CID generator 保持 QUIC packet 路由稳定。主动 connect 可选择任一 worker endpoint，punch 必须使用同一 listener 的 worker UDP socket 同源端口。入站 QUIC tunnel 通过 `TunnelNetwork::listen(...)` 注册的回调通知外部，而不是导出 `TunnelListener` 给调用方轮询。这类辅助行为默认关闭，只有 `TunnelManager` 在存在 SN service 且候选是 `ServerReflexive` QUIC endpoint 时才会为本次 `TunnelConnectIntent` 开启，且不得变成上层可见的 raw UDP tunnel 或业务载荷协议。UDP punch 的私有短载荷必须避开 QUIC packet invariant，例如首字节不得设置 QUIC fixed bit。QUIC tunnel 心跳发送间隔保持当前默认，heartbeat timeout 为 30 秒。
- `src/networks/tcp/**` 负责 TCP listener、TLS accept、control/data connection 分流和 TCP tunnel registry；TCP listener 可基于 `sfo_reuseport::TcpServer` 接收入站 stream，并通过 `TunnelNetwork::listen(...)` 的回调交付新 tunnel，但不得改变 TCP tunnel 线协议、TLS 身份校验或 tunnel publish 语义。
- 公共 `Tunnel` trait 的 stream/datagram 入站 channel 暴露使用 `listen_stream(...)` / `listen_datagram(...)` 回调模型；调用方不得通过公共 `accept_stream()` / `accept_datagram()` 轮询 channel。TCP、QUIC、PN tunnel 必须在内部入站处理路径中按 listen vport/purpose 规则触发回调，并保持线协议、TLS 身份校验、PN proxy channel 协议和 tunnel publish 语义不变。
- `src/endpoint.rs` 负责 endpoint area、协议和地址的公开语义及编解码；`ServerReflexive` 表示 SN 观察到但未与节点自上报地址一致的外网地址，文本编码使用 `S`，不得再作为 system default 语义使用。
- `src/sn/**` 负责 SN 观察 endpoint 的归类：只有 SN 观察地址与节点自上报 endpoint 完全一致时才可标记为 `Wan`，否则必须标记为 `ServerReflexive`。
- `src/pn/client/**` 负责 `PnTunnel` 本地生命周期、proxy channel open/accept、tunnel 级控制通道、对端关闭感知、idle timeout 关闭，以及关闭后同一 logical tunnel 后续 open 触发重新创建的语义。
- `src/tunnel/**` 负责 tunnel 候选选择、proxy 兜底连通性，以及 proxy 已连通后的 direct/reverse 脱代理升级策略；当同一远端存在多个可用 tunnel candidate 时，默认复用路径应优先选择已发布的非 proxy tunnel，只有没有可用非 proxy candidate 时才复用 proxy。reverse incoming tunnel 必须命中本地同 `(remote_id, tunnel_id)` reverse waiter；无 waiter 时必须关闭且不得发布为可用候选。

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
- `sfo-reuseport` 的 `ServerRuntime`、`TcpServer`、`QuicServer` 和 listener socket 分发能力
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
- `TunnelManager` 的连接选择、后台升级调度和退避策略变更，至少要有对应的 unit 证据；其中多个已有 candidate 的选择必须覆盖非 proxy 优先于 proxy 的回归断言。
