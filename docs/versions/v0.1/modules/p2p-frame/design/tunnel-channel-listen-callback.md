---
module: p2p-frame
version: v0.1
status: approved
approved_by: auto-pipeline
approved_at: 2026-06-02T11:14:58+08:00
approved_content_sha256: cec874fda59f2180eb0bbc3684e180e934a6050219495e5c7d93ad1d4abcd65b
---

# Tunnel stream/datagram listen 回调设计

## 目标
- 公共 `Tunnel` trait 移除 `accept_stream()` 与 `accept_datagram()`。
- `listen_stream(...)` 与 `listen_datagram(...)` 注册入站 channel 回调，并继续携带 listen vports / purpose 过滤语义。
- TCP、QUIC、PN tunnel 在内部入站处理路径中触发回调；外部调用方不再启动公共 accept loop。
- TTP、stream manager、datagram manager 和 PN server/client 从 accept loop 迁移为回调消费。

## 非目标
- 不改变 TCP、QUIC、PN、TTP 线协议。
- 不改变 TLS 身份校验、PN proxy channel 协议、业务 payload 格式、vport/purpose 编解码或 tunnel publish 规则。
- 不保留公共 `accept_*` 兼容旁路。
- 不让外部回调阻塞 `sfo-reuseport` worker socket loop、QUIC endpoint accept loop、PN control loop 或 TTP frame dispatch。

## 公共接口
- 在 `p2p-frame/src/networks/tunnel.rs` 定义 `IncomingStreamCallback` 与 `IncomingDatagramCallback` 或等价命名类型。
- stream 回调输入为 `P2pResult<(TunnelPurpose, TunnelStreamRead, TunnelStreamWrite)>` 或等价 named tuple。
- datagram 回调输入为 `P2pResult<(TunnelPurpose, TunnelDatagramRead)>` 或等价 named tuple。
- 回调必须可 `Clone + Send + Sync + 'static`，返回 `Send + 'static` future。
- `Tunnel::listen_stream(vports, callback)` 与 `Tunnel::listen_datagram(vports, callback)` 返回 `P2pResult<()>`。
- `Tunnel` trait 不再包含 `accept_stream()` 或 `accept_datagram()`。

## 状态与注册语义
- 每个 tunnel 分别保存 stream listen state 和 datagram listen state。
- 首次 `listen_*` 设置 listen 规则与回调。
- 重复 `listen_*` 替换同类 listen 规则与回调；旧回调不再接收后续 channel。
- tunnel 已关闭时调用 `listen_*` 返回 `P2pErrorCode::Interrupted` 或等价关闭错误。
- tunnel 关闭时禁用已注册回调，并唤醒或拒绝内部等待投递的 channel。

## 入站投递语义
1. tunnel 入站处理路径收到 stream/datagram channel。
2. 检查 tunnel 状态；若已关闭或正在关闭，关闭或拒绝该 channel。
3. 检查对应 listen state；若未 listen 或 purpose 不匹配，按现有 open 失败语义返回 `PortNotListen`、`NotSupport` 或等价错误。
4. 若匹配，构造回调输入并调度回调 future。
5. 回调调度成功后，入站处理路径不得再等待调用方轮询公共 accept queue。

## 运行时与背压
- 回调 future 不得在 transport/control loop 中同步 await 到完成。
- TCP/QUIC/PN tunnel 可以直接 spawn 回调任务，也可以通过已配置 bounded channel 交给分发任务。
- 对基于 `sfo-reuseport` worker 的 TCP/QUIC 入站路径，worker callback 只负责把 channel 交给 tunnel dispatch；外部回调 future 不得长期占用 worker accept/recv loop。
- 如果内部投递 bounded channel 满载，返回 `P2pErrorCode::OutOfLimit` 或等价错误，并关闭迟到 channel。
- 回调 future panic/abort 不重新打开 tunnel，不重新入队 channel；实现可记录错误，但不得改变线协议。

## 调用点迁移
- `TtpRuntime::attach_tunnel(...)` 注册 stream/datagram 回调；回调中执行原有 registry lookup 和 listener queue send。
- `StreamManager` 注册 stream 回调；回调中执行原有 stream listener lookup 和 accepted stream 分发。
- `DatagramManager` 注册 datagram 回调；回调中执行原有 datagram listener lookup 和 accepted datagram 分发。
- `PnServer` 和 PN client/listener 中依赖 tunnel inbound channel 的路径迁移为回调消费，保持 proxy bridge、统计、限速和 TLS-over-proxy 语义不变。
- 测试替身必须实现新 `listen_*` 回调接口，不得继续实现公共 `accept_*`。

## 关闭和错误语义
- tunnel close 后不得再调用 stream/datagram 回调。
- close 与入站 channel 并发时，若 channel 尚未交付给回调，则关闭或拒绝；若已进入回调 future，遵循已交付 channel 的正常生命周期。
- 未 listen 的 purpose 继续按现有 open 失败语义反馈给远端。
- 回调替换后，旧回调不接收新 channel；已经交付给旧回调的 channel 不被强制中断。

## 实现顺序
1. 在 `networks/tunnel.rs` 调整公共 callback 类型和 `Tunnel` trait。
2. 修改 TCP/QUIC/PN tunnel 保存 listen state，并把原 accepted queue 消费端改为回调触发。
3. 迁移 TTP、stream manager、datagram manager、PN server/client 的公共 accept loops。
4. 更新测试替身和调用点编译覆盖。
5. 进入 post-implementation testing 补充关闭、未 listen、重复 listen、满载和 integration 覆盖。

## 回滚
- 回滚必须成组恢复公共 `accept_*` trait 方法、TCP/QUIC/PN tunnel accepted queue 消费模型、TTP/stream/datagram/PN 调用点和测试替身。
- 不允许保留回调和公共 accept 双入口；若需要兼容窗口，必须退回 proposal 重新批准。
