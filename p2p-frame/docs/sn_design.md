# SN 设计文档

## 目标

`SN` 模块用于在 `p2p-frame` 中提供一组中心化信令能力，核心职责包括：

- 为节点提供上线登记与保活
- 帮助节点查询其他节点的最新可达信息
- 在两个节点之间转发呼叫信令，协助建立后续直连/反连通信

当前实现位于 `p2p-frame/src/sn/`，分为客户端和服务端两部分：

- 客户端：`p2p-frame/src/sn/client/`
- 服务端：`p2p-frame/src/sn/service/`

## 功能概览

当前 SN 实现包含以下能力：

- `ReportSn`：客户端向 SN 报告自身信息、监听端点和端口映射，并维持在线状态
- `SnQuery`：客户端向 SN 查询目标 peer 的 desc 和端点列表
- `SnCall`：客户端请求 SN 帮助联系目标 peer
- `SnCalled`：SN 将来电通知转发给被叫方
- `SnCalledResp`：被叫方回应该次通知处理结果

命令信道使用固定的 tunnel stream `vport`：

- `SN_CMD_VPORT = 0xfff0`

定义见 `p2p-frame/src/sn/types.rs`。

## 总体架构

SN 体系由三层组成：

### 1. Ttp / 命令承载层

当前实现中，SN 不再直接操作 `NetManager` 建立和接收入站命令流，而是通过 `Ttp` 层统一完成：

- 客户端通过 `TtpClient` 主动连向 SN，并打开固定 `vport=0xfff0` 的 stream
- 服务端通过 `TtpServer` 监听固定 `vport=0xfff0` 的入站 stream

`Ttp` 底层仍建立在 `NetManager + TunnelNetwork + Tunnel` 之上，但这些细节已经被下沉到 `Ttp` 内部。

这一层的关键适配类型是：

- `SnTunnelRead`
- `SnTunnelWrite`
- `SnTunnelClassification`

其作用是把通用 tunnel stream 适配给 `sfo_cmd_server` 使用。

### 2. SN 命令分发层

客户端和服务端都基于 `sfo_cmd_server` 封装命令收发：

- 客户端：`SnCmdClient`
- 服务端：`SnCmdServer`

这一层负责：

- 建立和复用命令 tunnel
- 发送命令码 + body
- 按命令码注册 handler
- 在请求与响应之间做异步等待和唤醒

### 3. SN 业务层

业务层负责具体语义：

- 客户端：维护 active SN，周期性上报，执行 query/call
- 服务端：维护在线 peer 信息，处理 report/query/call，并向目标节点转发 `SnCalled`

## 目录结构

### `sn/client`

- `sn_service.rs`
  - `SNClientService`
  - `SnClientTunnelFactory`
  - active SN 管理、保活、call/query 流程
- `contract.rs`
  - 服务评价/回执相关逻辑雏形

### `sn/service`

- `service.rs`
  - `SnServer`
  - `SnService`
  - 服务端监听与命令处理主流程
- `peer_manager.rs`
  - peer 在线信息缓存与更新
- `call_stub.rs`
  - 对重复 call 的去重
- `receipt.rs`
  - 服务合同/回执检查接口

### `sn/types.rs`

定义命令 tunnel 的包装类型和固定命令端口。

## 客户端架构

客户端核心对象是 `SNClientService`。

它负责：

- 持有 SN 列表 `SnList`
- 持有 `NetManager`
- 管理 `SnCmdClient`
- 维护 `active_sn_list`
- 周期性向 SN 发送 `ReportSn`
- 发送 `SnCall` / `SnQuery`
- 接收 `SnCallResp` / `SnQueryResp` / `ReportSnResp` / `SnCalled`

### 命令 tunnel 建立

客户端通过 `SnClientTunnelFactory + TtpClient` 建立命令 tunnel：

1. 构造 `TtpTarget`
2. 调用 `TtpClient.open_stream(target, SN_CMD_VPORT)`
3. 由 `Ttp` 内部建立或复用到底层 SN 的 tunnel
4. 取回 `(meta, read, write)` 后包装成 `SnTunnelRead/Write`
5. 交给 `SnCmdClient`

这意味着：

- SN 命令通道不是独立网络连接，而是建立在现有 tunnel 体系之上
- 每个命令 tunnel 本质上是一个“到某个 SN 的固定 `vport` stream”

### active SN 维护

客户端用 `ActiveSN` 表示一个已连通且完成 `ReportSn` 的 SN：

- `sn_peer_id`
- `conn_id`
- `latest_time`
- `recv_future`
- `wan_ep_list`

其中：

- `recv_future` 用于按 `Sequence` 等待 `SnCallResp` / `SnQueryResp`
- `wan_ep_list` 保存当前 SN 观察到的本端外网端点

### 客户端状态机

`SNServiceState` 保存：

- `pinging_handle`：后台保活任务
- `active_sn_list`
- `latest_sn_interval`
- `cur_report_future`

行为上，客户端状态很简单：

- 未在线：没有 active SN
- 在线：至少有一个 active SN
- 保活：周期性向 active SN 发送 `ReportSn`

## 服务端架构

服务端由 `SnServer + SnService` 两层组成。

其中：

- `SnServer` 负责监听、accept 和服务端生命周期调度
- `SnService` 负责拿到命令连接后的业务处理

整体负责：

- 创建专用 `NetManager`
- 监听本地 endpoint
- 从入站 tunnel 中提取 `SN_CMD_VPORT` 命令流
- 将命令连接交给 `SnService`
- 维护在线 peer 信息
- 执行 call/query/report 的业务逻辑

### `SnServer`

`SnServer` 是服务端接入层与生命周期入口。

它通过 `TtpServer` 获取按 `vport` 分好的入站 stream，然后：

1. 监听 `SN_CMD_VPORT`
2. 将接收到的 `(meta, read, write)` 转为 `CmdTunnel<SnTunnelRead, SnTunnelWrite>`
3. 交给 `SnService.handle_tunnel()`

当前实现中：

- `SN_CMD_VPORT` 的 stream 会被转成 `CmdTunnel<SnTunnelRead, SnTunnelWrite>`
- 其他特定 `vport` 仍可通过同一个 `TtpServer` 被其他子系统复用

就 SN 本身而言，它只消费 `SN_CMD_VPORT`。

### `SnService`

`SnService` 是服务端业务层。

它负责：

- 注册 SN 命令处理器
- 基于 `DefaultCmdServerService` 管理命令 tunnel
- 维护 `PeerManager`
- 执行 `ReportSn` / `SnQuery` / `SnCall` / `SnCalledResp` 的处理逻辑

### `PeerManager`

服务端通过 `PeerManager` 保存已知 peer 信息，主要包括：

- `desc`
- `map_ports`
- `local_eps`
- `is_wan`

其主要职责：

- 在 `ReportSn` 时新增或更新 peer
- 在 `SnQuery` 时返回目标 peer 当前缓存的信息
- 在 `SnCall` 时检查被叫方是否存在

### `CallStub`

`CallStub` 用 `(from_peer_id, tunnel_id)` 去重，避免同一呼叫被重复转发。

当前逻辑：

- 首次插入成功才会继续转发 `SnCalled`
- 相同键再次插入则视为重复呼叫

## 命令码与报文

SN 使用的命令码如下：

| 命令 | Code | 方向 | 说明 |
| --- | --- | --- | --- |
| `SnCall` | `0x20` | Client -> SN | 发起呼叫请求 |
| `SnCallResp` | `0x21` | SN -> Client | 呼叫请求处理结果 |
| `SnCalled` | `0x22` | SN -> Callee | SN 向目标节点转发来电通知 |
| `SnCalledResp` | `0x23` | Callee -> SN | 被叫处理结果 |
| `ReportSn` | `0x24` | Client -> SN | 上线登记/保活上报 |
| `ReportSnResp` | `0x25` | SN -> Client | 上报响应，带外网端点 |
| `SnQuery` | `0x26` | Client -> SN | 查询目标节点 |
| `SnQueryResp` | `0x27` | SN -> Client | 查询响应 |

定义见 `p2p-frame/src/protocol/common.rs`。

### 1. `ReportSn`

字段要点：

- `sn_peer_id`
- `from_peer_id`
- `peer_info`
- `map_ports`
- `local_eps`

用途：

- 告诉 SN 当前客户端是谁
- 告诉 SN 本地监听端点和端口映射信息
- 让 SN 记录该 peer 可供别人使用的候选地址

### 2. `ReportSnResp`

字段要点：

- `result`
- `end_point_array`
- `peer_info`
- `receipt`

当前实现里最关键的是 `end_point_array`，它表示 SN 观察到的客户端外网端点列表。

### 3. `SnQuery`

字段要点：

- `seq`
- `query_id`

### 4. `SnQueryResp`

字段要点：

- `peer_info`
- `end_point_array`

如果目标不存在，则这两个字段为空。

### 5. `SnCall`

字段要点：

- `seq`
- `tunnel_id`
- `sn_peer_id`
- `to_peer_id`
- `from_peer_id`
- `reverse_endpoint_array`
- `active_pn_list`
- `peer_info`
- `call_type`
- `payload`

其中：

- `call_type` 表示后续目标通信类型，当前是 `Stream` 或 `Datagram`
- `reverse_endpoint_array` 是发起方提供的可用于反向连接的候选端点
- `peer_info` 是发起方 desc/cert

### 6. `SnCallResp`

字段要点：

- `seq`
- `sn_peer_id`
- `result`
- `to_peer_info`

语义：

- 表示 SN 是否成功受理这次呼叫
- 不是被叫真正建立连接成功的确认

### 7. `SnCalled`

字段要点：

- `to_peer_id`
- `sn_peer_id`
- `peer_info`
- `tunnel_id`
- `call_send_time`
- `call_type`
- `payload`
- `reverse_endpoint_array`
- `active_pn_list`

它是 SN 向被叫方转发的“来电通知”。

### 8. `SnCalledResp`

字段要点：

- `seq`
- `sn_peer_id`
- `result`

当前主要表示被叫回调处理是否成功。

## 典型通信流程

### 流程一：客户端上线与保活

1. 客户端启动 `SNClientService`
2. `SnClientTunnelFactory` 通过 `TtpClient` 根据本地监听端点与 SN 列表建立命令 tunnel
3. 在 tunnel 上打开 `SN_CMD_VPORT`
4. 客户端发送 `ReportSn`
5. 服务端 `handle_report_sn()`：
   - 提取 `from_peer_id`
   - 用 `peer_info` 更新 `PeerManager`
   - 记录 `map_ports` 和 `local_eps`
   - 返回 `ReportSnResp`
6. 客户端收到 `ReportSnResp` 后，将该 SN 加入 `active_sn_list`
7. 后台任务继续周期性发送 `ReportSn` 作为保活

结果：

- 客户端进入 online 状态
- SN 拥有该客户端的最新地址信息
- 客户端获得自己的外网观察结果

### 流程二：查询节点

1. 客户端从 `active_sn_list` 选择一个 SN
2. 发送 `SnQuery { query_id }`
3. SN 在 `PeerManager` 中查找该目标
4. 若存在：
   - 返回 `peer_info`
   - 返回 `end_point_array`
5. 若不存在：
   - 返回空 `peer_info`
   - 返回空端点列表

结果：

- 调用方拿到目标最新缓存 desc 和候选端点

### 流程三：SN 呼叫转发

假设 `A` 需要联系 `B`：

1. `A -> SN` 发送 `SnCall`
   - 包含 `to_peer_id=B`
   - 包含 `from_peer_id=A`
   - 包含 `tunnel_id`
   - 可带 `reverse_endpoint_array`
   - 可带 `payload`
2. SN 收到 `SnCall` 后：
   - 检查 `A` 是否已知
   - 检查 `B` 是否已知
   - 用 `CallStub` 去重
   - 从 `PeerManager` 取出 `B` 的信息
3. SN 构造 `SnCalled`
   - 将 `A` 的 `peer_info` 放入 `peer_info`
   - 复制 `payload`
   - 复制/扩展 `reverse_endpoint_array`
4. `SN -> B` 通过现有命令 tunnel 发送 `SnCalled`
5. 同时 `SN -> A` 返回 `SnCallResp`
   - `result=Ok` 表示 SN 已经受理并尝试转发
   - `to_peer_info` 附带 `B` 的 desc
6. `B` 收到 `SnCalled` 后触发本地 `SNEvent::on_called`
7. `B -> SN` 返回 `SnCalledResp`

结果：

- 发起方拿到被叫 desc
- 被叫拿到发起方 desc、候选反连地址和附带 payload
- 后续真实数据连接建立由更高层 tunnel/connect 逻辑处理

## 关键对象说明

### `SnTunnelClassification`

用于 `SnCmdClient` 对命令 tunnel 做分类复用，键包含：

- `local_ep`
- `remote_ep`

其作用是让客户端优先按“本地出口 + 远端 SN 地址”复用命令 tunnel。

### `ActiveSN`

表示当前已工作的 SN 连接：

- `conn_id`：命令 tunnel id
- `recv_future`：等待响应的挂起请求表
- `wan_ep_list`：SN 看到的外网端点

### `SNEvent`

客户端对外暴露的回调接口：

- `on_called(called: SnCalled)`

作用：

- 上层在收到 `SnCalled` 后决定如何处理来电通知

## 启动方式

### 客户端

通常由 `P2pStack` 在构建时创建 `SNClientService`：

1. 传入本地 identity、SN 列表、超时参数
2. 创建 `SnCmdClient`
3. 启动后台 `ping_proc`
4. 等待至少一个 active SN 建立成功

可配置参数主要包括：

- `sn_list`
- `sn_ping_interval`
- `sn_call_timeout`
- `sn_query_interval`
- `sn_tunnel_count`

### 服务端

`SnServer` 通过 `create_sn_service(SnServiceConfig)` 创建：

1. 初始化 TLS 与 `NetManager`
2. 创建 TCP/QUIC tunnel network
3. 创建 `TtpServer`
4. 创建内层 `SnService`
5. 在 `start()` 时监听本地 endpoint
6. 拉起 `SN_CMD_VPORT` accept loop
7. 将接受到的命令连接交给 `SnService`

若需要 PN relay，则由独立的 `PnServer` 基于同一个 `TtpServer` 显式创建和启动。

服务端默认同时支持：

- TCP tunnel network
- QUIC tunnel network

## 测试覆盖

当前 `p2p-frame/src/sn/tests.rs` 已覆盖几个关键行为：

- `wait_online()` 在成功 `ReportSn` 后返回成功
- `SnQuery` 对已注册 peer 返回完整信息
- `SnQuery` 对未知 peer 返回空结果
- `SnCall` 的 `Stream` 路径字段检查
- `SnCall` 的 `Datagram` 路径字段检查
- 对未知目标 `SnCall` 返回 `NotFound`

这些测试说明当前 SN 主要保障的是：

- 在线登记流程可用
- 查询流程可用
- 呼叫信令转发字段正确

## 当前实现的边界与说明

### 1. `SnCallResp` 不是最终连通性结果

当前 `SnCallResp` 表示的是：

- SN 是否成功接受并处理该请求
- 是否找到被叫及其 desc

它不表示后续的直连/反连/代理连接已经建立成功。

### 2. `PeerManager` 是 SN 的在线缓存核心

SN 是否能成功 query/call 某个 peer，取决于：

- 该 peer 是否已通过 `ReportSn` 上报
- `PeerManager` 中是否仍保留其记录

### 3. 合同/回执逻辑还未完全落地

`SnServiceContractServer`、`SnServiceReceipt`、`contract.rs` 说明系统有服务计费/评估方向，但当前主流程还没有完整接入这些校验结果。

### 4. 去重能力当前较轻量

`CallStub` 仅做基于 `(remote, tunnel_id)` 的短时去重，避免重复 `SnCalled`，并不是完整事务跟踪系统。

### 5. 服务端入站 tunnel 分发当前有复用点

`SnServer` 当前直接建立在 `TtpServer` 之上，不只消费 `SN_CMD_VPORT`，还可以继续复用其他 `vport` 的入站 stream。这说明 SN 服务端已经切到统一的 `Ttp` 传输接入层。

## 小结

当前 SN 设计可以概括为：

- 用固定 `SN_CMD_VPORT` 在 tunnel 上承载命令通道
- 客户端维护到多个 SN 的命令 tunnel 与 active SN 状态
- 服务端维护在线 peer 信息缓存
- `ReportSn` 负责上线/保活
- `SnQuery` 负责查找 peer
- `SnCall` / `SnCalled` 负责中继呼叫信令

它本质上是一个“基于 tunnel 命令通道的中心化信令服务”，为后续真正的数据 tunnel 建立提供辅助信息和转发能力。
