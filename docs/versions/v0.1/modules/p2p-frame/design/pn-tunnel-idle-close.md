# PN Tunnel Idle Close 设计补充

本补充文档聚焦 `pn/client` 中 `PnTunnel` 的本地 idle timeout 生命周期关闭。该设计不增加可靠远端 close 通知；它只定义本端在 channel 全部结束并持续空闲后如何原子关闭本地 tunnel、释放资源，并让后续同一 `(remote_id, tunnel_id)` 的 inbound open 重新创建新的 passive `PnTunnel`。

## 范围

### 范围内
- `PnTunnel` 本地 idle timeout 关闭语义，默认策略为 channel 数量归零后空闲 30 分钟
- active、pending、queued channel 的统一计数模型
- 已返回给上层的 stream/datagram lease wrapper 生命周期跟踪
- idle sweeper 与手动 `close()` 共享的幂等关闭路径
- `accept_stream()` / `accept_datagram()` 在 idle close 后被唤醒并返回错误
- `open_stream()` / `open_datagram()` 在 idle close 后立即失败
- 关闭后从 live registry 移除，防止同一 `(remote_id, tunnel_id)` 的后续 inbound open 投递到已关闭对象

### 范围外
- 引入 `ProxyCloseReq`、可靠远端关闭通知、全局 lease heartbeat 或 relay session 生命周期协议
- idle timeout 时强制中断仍处于 active 状态并已交给上层的 channel
- 改变 `Tunnel` / `TunnelNetwork` trait 签名
- 改变 `tunnel_id` 或 `candidate_id` 的稳定身份语义

## 状态模型

`PnTunnel` 需要从单一 `AtomicBool closed` 扩展为受同一把状态锁保护的生命周期状态：

```text
Open {
    active_channels,
    pending_channels,
    queued_channels,
    zero_since,
}
Closing { reason }
Closed { reason, closed_at }
```

其中：

- `active_channels` 覆盖已经成功 `open_*` / `accept_*` 并返回给上层的 channel。
- `pending_channels` 覆盖本地正在执行 PN open 握手、TLS 包装或 accept 响应但尚未交给上层的 channel。
- `queued_channels` 覆盖已经进入 inbound queue、尚未被 `accept_*` 消费的 channel。
- `zero_since` 只在三类计数全部为 0 时记录开始空闲的时间。

状态更新必须集中在 `PnTunnel` 内部 helper 中完成，避免 open、accept、inbound 投递、lease drop 和 sweeper 各自读写分散字段。

## Channel Lease

`PnTunnel` 必须能知道已交给上层的 channel 何时结束。设计上应在返回给上层前包装 read/write：

- stream: `open_stream()` / `accept_stream()` 返回的 read/write 共享同一个 channel lease。
- datagram: `open_datagram()` 返回的 write 和 `accept_datagram()` 返回的 read 也必须拥有对应 lease。
- lease 在创建时递增 active 计数，在最后一个 wrapper drop 或明确关闭时只递减一次。
- TLS-over-proxy 包装不能绕过 lease；TLS wrapper 应包在 lease-tracked IO 外侧或确保最终 drop 能归还同一个 lease。

如果某条 channel 在 open/accept 过程中失败，必须递减 pending 计数并刷新 idle 判定。

## Idle Sweeper

`PnShared` 或等价 owner 负责启动 PN client 侧 idle sweeper。sweeper 周期扫描注册表中的 live tunnel，并在每个 tunnel 的状态锁内检查：

```text
state == Open
active_channels == 0
pending_channels == 0
queued_channels == 0
zero_since + idle_timeout <= now
```

满足后，sweeper 必须调用与手动 close 共用的内部关闭入口，将状态原子切换到 `Closing` / `Closed`。idle timeout 应可配置；默认值为 30 分钟，测试可使用短时钟或短 timeout。

## Close 路径

手动 `close()` 与 idle close 必须共享同一内部路径：

1. 在状态锁内从 `Open` 切到 `Closing` 或直接 `Closed`，记录 close reason。
2. 拒绝新的 local open 和 inbound channel 投递。
3. drain inbound queue；对已取出的 queued channel 尽力写回失败 `ProxyOpenResp`，避免 opener 等待 `PN_OPEN_TIMEOUT`。
4. 唤醒 `accept_stream()` / `accept_datagram()` 等待者，使其返回 `Interrupted` 或等价关闭错误。
5. 在 `PnShared` 中移除 live registry 内的旧 weak entry。
6. 幂等返回；重复 close 不得重复递减计数或重新打开状态。

close 不负责杀死仍处于 active 的 channel。idle close 只有在 active 计数已经为 0 时才可触发；手动 close 后已返回给上层的 channel 继续由各自 IO/drop 路径收敛。

## Inbound 分发与重新创建

`PnShared` 的 live tunnel registry 只保存仍可用的 tunnel。`PnListener` 收到 `ProxyOpenReq` 时，分发顺序必须是：

1. 若 live tunnel 存在且状态仍为 `Open`，在状态锁内增加 queued 计数并入队。
2. 若 live tunnel 缺失、weak 已失效或已有 tunnel 已关闭，必须移除旧 registry entry，并按现有逻辑创建新的 passive `PnTunnel`。
3. 新建 passive tunnel 后重新注册同一 `(remote_id, tunnel_id)`，后续同 key channel 再投递到这个新对象。

该语义允许关闭后复用稳定的 `tunnel_id` / `candidate_id` 作为 logical tunnel 身份，但每个 `PnTunnel` 对象自身仍是一次本地生命周期；关闭后的对象不得重新打开。

## Open 与 Accept

- `open_stream()` / `open_datagram()` 进入前先在状态锁内检查 `Open` 并登记 pending；若已关闭立即失败。
- open 握手成功并完成必要 wrapper 后，pending 转 active，并返回 lease-tracked IO。
- open 失败时，pending 归还并刷新 idle。
- `accept_stream()` / `accept_datagram()` 等待队列时必须同时等待 close notify；close 后立即返回错误。
- accept 从 queued 取出 channel 时递减 queued 并登记 pending；响应成功且 wrapper 完成后转 active，失败则归还 pending 并写回错误。

## 配置与默认值

- 默认 idle timeout 为 30 分钟。
- 配置应落在 PN client / stack 显式配置路径上，不改变通用 tunnel trait。
- timeout 为 `None` 或 0 的行为必须在实现前明确；第一版建议 `None` 表示禁用 sweeper，0 不作为公开默认值。

## 兼容性

- `tunnel_id` 与 `candidate_id` 在 `PnTunnel` 生命周期内保持不变。
- idle close 不要求远端同步关闭；远端只会在后续 open/I/O 或自身 idle close 中收敛。
- 已建立 channel 的 EOF/shutdown 行为继续由底层 stream/datagram IO 决定。
- 该设计只改变 `PnTunnel` 本地生命周期和等待者失败语义，不改变 `ProxyOpenReq` / `ProxyOpenResp` 格式。

## 风险

- lease wrapper 若未覆盖 TLS 和 datagram 路径，会导致 channel 计数泄漏或提前归零。
- 如果 registry 没有在 close 后移除旧对象，后续 inbound open 可能被错误投递到 closed tunnel。
- 如果重新创建路径没有重新注册新对象，后续同 key channel 可能重复触发 `accept_tunnel()` 而不是进入同一新 tunnel 的队列。
- sweeper 与 inbound 分发若没有共享状态锁，会出现误关闭或关闭后入队竞态。
