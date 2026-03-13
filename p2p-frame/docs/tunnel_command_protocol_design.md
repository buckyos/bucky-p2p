# Tunnel 命令协议规范

## 目标

定义 `Tunnel` 内部命令的统一范式，使不同 transport 的命令面遵循同一套通用结构、编解码规则和分派方式。

本文只定义命令的通用设计，不定义任何具体 tunnel 的命令集合或业务语义。

适用范围：

- TCP tunnel control connection
- QUIC tunnel command channel
- Proxy tunnel 的命令握手或后续 command channel

## 设计原则

### 原则 1：所有命令都使用统一外壳

每条命令都必须由两部分组成：

- 命令头 `TunnelCommandHeader`
- 命令体 `body`

命令头只负责：

- 标识协议版本
- 标识命令类型
- 标识命令体长度

具体业务语义全部由命令体负责。

### 原则 2：所有命令都使用 `RawEncode` / `RawDecode`

无论是命令头还是命令体，都统一使用：

- `RawEncode`
- `RawDecode`

不再为 tunnel command 额外定义独立的手写编解码格式。

### 原则 3：`command_id` 是命令类型编号

`command_id` 表示“命令类型 ID”，不是某条请求实例的唯一序号。

例如某条命令如果需要表达：

- `request_id`
- `conn_id`
- `lease_seq`
- `channel_id`
- `result`

这些字段必须定义在该命令自己的 `body` 中，而不是放入通用命令头。

### 原则 4：统一的是通用框架，不是中心化命令表

本文要求不同 transport 复用统一的命令外壳和读写框架，但不要求把所有具体命令集中定义在一个公共枚举或一个公共模块中。

也就是说：

- 通用层统一 `TunnelCommandHeader`
- 通用层统一 `TunnelCommand<T>`
- 通用层统一 `TunnelCommandBody`
- 通用层统一读写辅助函数
- 具体命令体由具体 tunnel 自己定义
- 具体 `command_id` 由具体 tunnel 自己维护

### 原则 5：读取流程依赖“头 + 具体命令体”

读取方必须先读取命令头，再根据 `header.command_id` 选择对应命令体类型进行解码。

完整识别一条命令依赖：

- `header.version`
- `header.command_id`
- `header.data_len`
- 对应命令体中的业务字段

## 通用结构

### 命令头

```rust
#[derive(Clone, Debug, RawEncode, RawDecode)]
pub struct TunnelCommandHeader {
    pub version: u8,
    pub command_id: u8,
    pub data_len: u32,
}
```

字段说明：

- `version`
  - 命令协议版本号
  - 用于未来做不兼容升级
- `command_id`
  - 命令类型编号
  - 类型为 `u8`
  - 用于决定后续 `body` 应按哪种结构解码
- `data_len`
  - 命令体长度
  - 表示后续 `body` 的编码字节数

### 通用命令包装

```rust
pub struct TunnelCommand<T>
where
    T: TunnelCommandBody,
{
    pub header: TunnelCommandHeader,
    pub body: T,
}
```

它的职责是：

- 让所有具体命令都复用统一的 envelope
- 让具体命令体保持独立结构
- 便于统一校验、读写和封装

### 命令体 Trait

```rust
pub trait TunnelCommandBody:
    RawEncode + for<'de> RawDecode<'de> + Sized
{
    const COMMAND_ID: u8;
    const VERSION: u8 = 1;
}
```

这样每个命令体可以自行声明：

- 自己的 `COMMAND_ID`
- 使用的协议版本

## 编码规范

一条完整命令的编码顺序必须是：

1. 先编码 `body`
2. 计算 `body` 的编码长度，写入 `header.data_len`
3. 填充 `header.version`
4. 填充 `header.command_id`
5. 发送：`header bytes + body bytes`

即：

```text
[TunnelCommandHeader]
[body]
```

要求：

- `header.data_len` 必须等于 `body` 的实际编码字节长度
- `header.command_id` 必须与 `T::COMMAND_ID` 一致
- `header.version` 必须与 `T::VERSION` 一致

## 解码规范

一条完整命令的解码顺序必须是：

1. 先读取并解码 `TunnelCommandHeader`
2. 按 `header.data_len` 精确读取后续命令体字节
3. 根据 `header.command_id` 选择对应命令体类型 `T`
4. 将命令体字节按 `T` 做 `RawDecode`
5. 组合成 `TunnelCommand<T>` 并校验头与命令体是否匹配

也就是说，命令分派以 `header.command_id` 为准，而不是依赖中心化的全局命令枚举。

## 命令 ID 规范

`command_id` 的约束如下：

- 类型固定为 `u8`
- 表示命令种类，不表示请求实例
- 在同一套 command channel / 同一协议版本内必须唯一
- 一旦形成稳定协议，不应随意复用或改号
- 新增命令时只允许追加，不应重排已有编号

本文不定义任何 transport 共享的全局命令编号表。

## 命令体定义规范

每个具体命令体都必须：

- 使用独立 `struct` 或 `enum`
- 实现 `RawEncode` / `RawDecode`
- 实现 `TunnelCommandBody`
- 自己声明业务字段

命令体中可以定义任意该命令所需的业务字段，例如：

- 请求标识
- 连接标识
- 结果码
- 统计字段
- 关联对象信息

这些字段都属于具体命令语义，不属于通用命令头。

## 业务字段归属规范

下面这类字段不属于通用命令头，应由具体命令体自己定义：

- `request_id`
- `conn_id`
- `lease_seq`
- `channel_id`
- `claim_nonce`
- `result`
- `final_tx_bytes`
- `final_rx_bytes`

原因：

- 它们只对特定命令有意义
- 它们是具体命令语义的一部分
- 它们不属于通用 envelope 的职责

## 命令定义归属规范

具体命令体不应集中定义在通用模块中。

推荐职责划分如下：

- `p2p-frame/src/networks/command.rs` 只保留通用命令框架
- 各 tunnel 的具体命令体、命令 id 定义放在各自实现文件中

通用模块负责：

- `TunnelCommandHeader`
- `TunnelCommand<T>`
- `TunnelCommandBody`
- 通用校验逻辑
- 通用读写辅助函数

具体 tunnel 模块负责：

- 本 tunnel 使用的命令 id 定义
- 本 tunnel 使用的命令体定义
- 本 tunnel 内部的命令分派逻辑

## 读写辅助函数建议

建议通用层提供统一辅助函数，例如：

- `write_tunnel_command(...)`
- `read_tunnel_command_header(...)`
- `read_tunnel_command_body<T>(...)`

读取方建议按如下流程工作：

1. 调用 `read_tunnel_command_header(...)`
2. 根据 `header.command_id` 在当前 tunnel 内部选择目标命令体类型
3. 调用 `read_tunnel_command_body<T>(...)`
4. 执行对应处理逻辑

不再推荐在通用层定义类似 `AnyTunnelCommand` 的中心化分派枚举，因为这会把 transport-specific 命令重新耦合回公共模块。

## 错误处理规范

以下情况应视为命令级错误：

- `version` 不支持
- `command_id` 未定义
- `data_len` 非法
- 命令体字节长度与 `data_len` 不符
- 按 `command_id` 选择的命令体解码失败
- 头部声明与命令体 `COMMAND_ID` / `VERSION` 不匹配

建议：

- 记录详细日志
- 对于会破坏命令面一致性的错误，直接关闭当前 command channel 或 tunnel
- 业务级失败应通过具体命令体中的字段表达，而不是通过破坏命令格式表达

## 与 transport 的关系

### TCP

TCP 的 control connection 中承载的命令，应遵循本文定义的统一 envelope 和编解码规范。

### QUIC

QUIC 的 command channel 中承载的命令，应直接使用本文定义的统一 envelope 和编解码规范。

### Proxy

Proxy 在过渡期如果仍采用轻量握手，也应尽量与本文定义的统一 envelope 保持一致；如果未来引入长期 command channel，则应直接复用本文格式。

## 兼容性规范

- 新版本新增命令时，应追加新的 `command_id`
- 已存在命令的 `command_id` 不得复用
- 已存在命令体若需做不兼容修改，应通过 `version` 升级处理
- 不允许为同一 command channel 定义多套不同的 envelope

