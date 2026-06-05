# Tunnel Control Stream API 设计补充

本补充文档定义通用 `Tunnel` 的低频外部控制数据通道。该能力通过 `Tunnel` trait 暴露 `open_control_stream(...)` 与 `listen_control_stream(...)`，但内部 `control_stream` runtime、frame、stream id、window 和 buffer 协议不对 `p2p-frame` 外部公开。

## 范围

### 范围内
- 公共 `Tunnel` trait 新增 `open_control_stream(purpose)`。
- 公共 `Tunnel` trait 新增 `listen_control_stream(purposes, callback)`。
- TCP、QUIC 和 PN tunnel 的控制命令枚举各新增一个 `Data` 命令，用于承载内部 control stream frame。
- 新增 `networks` 私有 `control_stream` runtime，负责 virtual stream 多路复用、purpose 过滤、读写、fin/reset、buffer/window 和关闭传播。
- 单个外层 `Data` 命令 payload 最大 `64 KiB`。
- 底层控制通道断开或 tunnel close 时，所有派生 control stream 必须断开。

### 范围外
- 公开导出内部 `ControlStreamRuntime`、frame enum、stream id、window、buffer 或 `MAX_CONTROL_DATA_FRAME_SIZE`。
- 允许外部调用方直接读写 TCP/QUIC/PN 的 raw 控制通道。
- 用 control stream 替代普通 `open_stream` / `listen_stream` 业务数据平面。
- 重写现有 TCP/QUIC/PN ready、heartbeat、close、claim/open、PN control open 或业务 stream/datagram open 逻辑。

## 公共接口

`p2p-frame/src/networks/tunnel.rs` 增加：

```rust
pub type IncomingControlStream = (TunnelPurpose, TunnelStreamRead, TunnelStreamWrite);
pub type IncomingControlStreamCallbackFuture =
    Pin<Box<dyn Future<Output = ()> + Send + 'static>>;
pub type IncomingControlStreamCallback =
    Arc<dyn Fn(P2pResult<IncomingControlStream>) -> IncomingControlStreamCallbackFuture
        + Send
        + Sync
        + 'static>;

async fn listen_control_stream(
    &self,
    purposes: ListenVPortsRef,
    callback: IncomingControlStreamCallback,
) -> P2pResult<()>;

async fn open_control_stream(
    &self,
    purpose: TunnelPurpose,
) -> P2pResult<(TunnelStreamRead, TunnelStreamWrite)>;
```

重复 `listen_control_stream(...)` 默认替换旧 purpose 规则和 callback，关闭后的 `listen_control_stream(...)` 返回 `Interrupted` 或等价错误。该语义与当前 `listen_stream(...)` 保持一致。

## 内部模块

新增 `p2p-frame/src/networks/control_stream.rs`，模块可见性为 `pub(crate)` 或更窄，不从 crate 公共 API re-export runtime/frame 类型。

内部 runtime 负责：
- 分配 virtual stream id，建议 active/open 发起侧使用奇数，passive/被动发起侧使用偶数。
- 编解码私有 `ControlStreamFrame`。
- 管理 listen purpose 与 callback。
- 实现 virtual stream 的 `AsyncRead` / `AsyncWrite`。
- 管理 bounded inbound buffer 与 window/credit。
- 在底层控制通道关闭时 `close_all(reason)`。

私有 frame 至少包含：

```text
Open { stream_id, purpose }
OpenResp { stream_id, result }
Data { stream_id, bytes }
Fin { stream_id }
Reset { stream_id, reason }
Window { stream_id, credit }
```

## Transport 接入

TCP、QUIC、PN 各自只新增一个控制命令：

```text
Data { payload: Vec<u8> }
```

transport 层职责只包含：
- outbound：接收 `control_stream` runtime 生成的 payload，包装成自身控制命令 `Data` 并写入现有控制命令通道。
- inbound：控制命令读循环遇到 `Data` 后，把 payload 交给 `control_stream.on_data(payload)`。
- close：底层控制通道 EOF、decode 失败、write 失败、heartbeat timeout、remote close、本地 close 或 tunnel 进入 closed/error 时调用 `control_stream.close_all(reason)`。

transport 层不得解析内部 `ControlStreamFrame`，不得改变现有 heartbeat/close/open response 状态机。

## Frame 上限

外层 `Data.payload` 上限固定为 `64 KiB`。

- 发送侧：用户 `write` 大于 `64 KiB` 时，内部 runtime 必须切分成多个 `Data` 命令。
- 接收侧：收到超过 `64 KiB` 的外层 `Data.payload` 必须视为协议错误，关闭底层 tunnel 或至少关闭所有派生 control stream，并拒绝继续解析。
- `64 KiB` 是外层控制命令 payload 上限，不是用户单次 `write` 的 API 上限。

## 关闭语义

底层控制通道生命周期支配所有派生 control stream。

触发条件：
- control read EOF
- control decode 失败
- control write 失败
- heartbeat timeout
- 收到 remote close
- 本地 `close()`
- tunnel 进入 closing/closed/error

结果：
- 所有 virtual stream read 返回 EOF 或 `Interrupted`。
- 所有 pending write 返回 `Interrupted`。
- 所有 pending `open_control_stream(...)` 返回失败。
- 已注册 callback 不再收到新的成功 stream。
- 所有 inbound/outbound buffer 释放。
- 后续 `open_control_stream(...)` 立即失败。

## 风险与回滚

- control stream 数据与内部 heartbeat/close 共用控制命令通道；实现必须分片并避免长期持有控制写锁。
- 如果某个 transport 遗漏 `Data` 命令接入，公共 trait 会出现部分实现不可用的半迁移状态。
- 回滚必须成组删除 trait 方法、callback 类型、内部 runtime、TCP/QUIC/PN `Data` 命令和测试覆盖。
