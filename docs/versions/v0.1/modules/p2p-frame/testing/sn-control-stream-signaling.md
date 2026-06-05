# SN Control Stream 信令验证

## 验证目标
- SN command service 从 control stream 的 SN purpose 接收入站命令流。
- SN 低频小消息默认使用 `open_control_stream(...)`。
- control stream 打开失败时，SN client 返回当前 SN 命令通道创建/发送错误，不 fallback 到既有普通 `open_stream(...)`。
- SN 实现不直接依赖内部 `control_stream` runtime/frame 类型。

## Unit 覆盖
| 验证项 | 入口 | 通过标准 |
|--------|------|----------|
| SN control stream 入站包装 | `cargo test -p p2p-frame sn_server_wraps_sn_control_stream_into_cmd_tunnel -- --nocapture` | fake tunnel 通过 `listen_control_stream(...)` 交付 SN purpose stream，`SnServer::into_cmd_tunnel(...)` 能读取同一 payload。 |
| SN client 默认发送选择 | code review + `cargo check -p p2p-frame` | `SnClientTunnelFactory::open_cmd_tunnel(...)` 只调用 `open_control_stream(...)`，失败后不调用 `open_stream(...)`。 |
| SN service 默认监听选择 | code review + `cargo check -p p2p-frame` | `SnServer::start_cmd_accept_loop(...)` 只注册 `listen_control_stream(...)`，不注册普通 `listen_stream(...)` 作为 SN 命令入口。 |

## Integration 覆盖
- `python3 ./harness/scripts/test-run.py p2p-frame integration` 或等价 workspace 测试确认下游无需依赖内部 `control_stream` frame/runtime。
- 旧版本或未监听 control purpose 不再由普通 stream fallback 兼容，相关失败按当前 SN 命令错误模型暴露。

## 缺口
- 当前没有独立 DV 入口验证真实混版节点；该缺口沿用 p2p-frame 模块现有 DV disabled 说明。
