# Tunnel Control Stream API 测试补充

本补充文档定义 `tunnel_control_stream_api` 的验证面。测试目标是证明外部只能通过 `Tunnel` trait 使用 control stream，而内部 runtime/frame 不公开；TCP/QUIC/PN 仅以控制命令 `Data` 承载内部 frame；`Data` payload 最大 `64 KiB`；底层控制通道断开会关闭所有派生 control stream。

## Unit 覆盖

| 验证项 | 范围 | 预期 |
|--------|------|------|
| 公共 trait 编译 | `p2p-frame/src/networks/tunnel.rs`、测试替身 | 所有 `Tunnel` 实现提供 `open_control_stream(...)` / `listen_control_stream(...)`；公共 API 不导出内部 runtime/frame 类型。 |
| open/listen 成功路径 | `networks/control_stream.rs` | 已 listen 的 purpose 能收到 inbound control stream，主动 open 返回 read/write。 |
| purpose 过滤 | `networks/control_stream.rs` | 未 listen 或不匹配 purpose 时 open 返回 `PortNotListen` / `ListenerClosed`。 |
| 重复 listen | `networks/control_stream.rs` | 重复注册替换旧 callback，旧 callback 不再收到后续 stream。 |
| 64KiB 发送切分 | `networks/control_stream.rs` | 用户写入大于 `64 KiB` 的 buffer 时，发送侧拆成多个不超过 `64 KiB` 的外层 `Data` payload。 |
| 64KiB 接收校验 | `networks/control_stream.rs`、TCP/QUIC/PN adapter | 收到超过 `64 KiB` 的外层 `Data` payload 时按协议错误关闭底层 tunnel 或关闭所有派生 control stream。 |
| 控制通道关闭传播 | TCP/QUIC/PN tunnel | control EOF、decode 失败、write 失败、heartbeat timeout、remote close、本地 close 后，所有 virtual stream、pending open、pending write 失败或 EOF。 |
| transport adapter | TCP/QUIC/PN tunnel | transport 层只包装/拆出 `Data.payload`，不解析内部 control stream frame。 |
| tunnel 建立后 control stream 端到端收发 | TCP/QUIC/PN tunnel | 两端 tunnel 建立并连通后，一端 `listen_control_stream(...)`，另一端 `open_control_stream(...)`，callback 收到匹配 purpose，双方 read/write 能双向传输 payload。 |

## Integration 覆盖

`python3 ./harness/scripts/test-run.py p2p-frame integration` 继续作为 workspace 兼容入口，验证新增 trait 方法不会要求下游依赖内部 frame/runtime，也不会改变现有 stream/datagram、TLS 身份校验、PN proxy 业务 channel 或 tunnel publish 行为。

## Direct Change Coverage

| change_id | validation_id | 入口 | 覆盖 |
|-----------|---------------|------|------|
| tunnel_control_stream_api | V-TUNNEL-CONTROL-STREAM-UNIT | `python3 ./harness/scripts/test-run.py p2p-frame unit` / `cargo test -p p2p-frame --features x509 control_stream` | open/listen、purpose 过滤、重复 listen、64KiB 切分/超限、关闭传播、transport adapter，以及 TCP/QUIC/PN tunnel 建立后的 control stream 双向收发。 |
| tunnel_control_stream_api | V-TUNNEL-CONTROL-STREAM-COMPILE | `rustup run stable cargo test -p p2p-frame --no-run` | 公共 trait、全部 `Tunnel` 实现和内部类型可见性边界。 |
| tunnel_control_stream_api | V-TUNNEL-CONTROL-STREAM-INTEGRATION | `python3 ./harness/scripts/test-run.py p2p-frame integration` | 下游兼容性和现有 stream/datagram 行为不回归。 |
