# TunnelManager Tunnel 发布生命周期设计补充

本补充文档聚焦于 `p2p-frame/src/tunnel/tunnel_manager.rs` 中新 tunnel 的注册与发布生命周期。目标不是改变 tunnel 的外部语义，而是把当前分散在建链路径、入站处理路径和 reverse waiter 交付路径上的 publish 决策收敛成单一模型。

## 范围

### 范围内
- `TunnelManager` 对新建 tunnel 的候选登记、可见性提升和订阅广播规则
- direct、proxy、普通 incoming 和 reverse incoming 这四类成功建链路径的统一 publish 约束
- reverse tunnel 与本地 `(remote_id, tunnel_id)` `reverse_waiter` 的交互边界
- proxy tunnel 在“立即可用”与“后续脱代理升级”之间的候选登记要求

### 范围外
- `Tunnel` / `TunnelNetwork` trait 形状本身的修改
- `StreamManager` / `DatagramManager` 的订阅消费逻辑
- SN called、PN open 和底层 transport 握手协议本身
- 重新定义“哪些 tunnel 可被复用或被 `get_tunnel` 选中”的更大范围策略

## 目标与原则

- 所有对外可用的新 tunnel 都遵循同一条生命周期：`register -> publish`。
- `reverse` tunnel 的特殊性只允许体现在“本地 waiter 尚未完成交付时，publish 时机延后”，不允许演化成另一套 publish 规则。
- publish 是“把已登记候选变成对订阅者和默认复用路径可见”的动作，而不是底层建链成功的同义词。
- 只要某条 tunnel 已经对外可见，它就必须已经被 `TunnelManager` 纳入候选状态管理和后续清理/升级逻辑。
- publish 入口必须幂等，同一 candidate 重复 publish 不得造成重复广播或状态损坏。

## 生命周期模型

### 状态

| 状态 | 含义 | 是否在候选表中 | 是否已对外可见 |
|------|------|----------------|----------------|
| `ConnectedUnregistered` | 底层建链成功，但尚未交给 `TunnelManager` | no | no |
| `RegisteredHidden` | 已登记为候选，但由于命中本地 reverse waiter 而暂不对外发布 | yes | no |
| `RegisteredPublished` | 已登记并完成 publish，可被订阅者消费，也可被默认复用路径优先选中 | yes | yes |

### 状态迁移

| 触发 | 起点 | 终点 | 说明 |
|------|------|------|------|
| direct/proxy/普通 incoming 成功 | `ConnectedUnregistered` | `RegisteredPublished` | 默认路径：先登记，再立即 publish |
| reverse incoming 命中 waiter | `ConnectedUnregistered` | `RegisteredHidden` | 先登记，避免 tunnel 脱离候选管理；仅推迟 publish 时机 |
| waiter 成功收到 reverse tunnel | `RegisteredHidden` | `RegisteredPublished` | 通过与 direct/proxy 相同的 publish 入口提升可见性 |
| waiter 先超时/取消/失败后，reverse tunnel 才到达 | `ConnectedUnregistered` | `RegisteredPublished` | 因 waiter 已清理，不再允许继续隐藏 |

## 各路径设计要求

### 1. direct 和 proxy 主动建链

- direct 或 proxy 成功创建 tunnel 后，调用方只负责把 tunnel 交给统一注册入口。
- 统一注册入口完成候选登记、替换旧 candidate、维护 proxy 升级状态等内部动作。
- 注册完成后，调用方必须通过统一 publish 入口把该 tunnel 变为 `RegisteredPublished`。
- direct/proxy 调用点不再自行推导“这条 tunnel 是否应被 publish”的额外布尔参数。

### 2. 普通 incoming tunnel

- 非 reverse 的 incoming tunnel 在进入 `TunnelManager` 后，必须立即进入 `RegisteredPublished`。
- 其 publish 责任不应由外层 open 流程补做，而应由 incoming 注册路径在本地完成。

### 3. reverse incoming tunnel

- reverse incoming 到达时，`TunnelManager` 先按普通新候选完成登记。
- 若该 tunnel 命中本地 `(remote_id, tunnel_id)` `reverse_waiter`，则该候选先停留在 `RegisteredHidden`，同时把 tunnel 交给 waiter。
- waiter 成功取得 tunnel 并把 reverse 建链视为完成后，必须立刻调用统一 publish 入口，把该 tunnel 提升为 `RegisteredPublished`。
- 若 reverse incoming 到达时本地已无 waiter，则该 tunnel 不再视为“本次 reverse open 的内部结果”，必须像其他新 tunnel 一样立即 publish。

### 4. proxy tunnel 与后续脱代理升级

- 对已知 `remote_id` 的 proxy tunnel，只要它被视为“立即可用”的 tunnel，就必须先登记为候选，再发布；不允许长期存在“只广播、不登记”的已知远端 proxy 语义。
- proxy candidate 一旦登记，就继续沿用当前脱代理升级约束：后台周期性尝试 direct/reverse，成功后新的非 proxy candidate 同样走统一 `register -> publish` 路径。
- 若实现阶段仍需为 `remote_id` 未知的临时 wrapper 保留局部例外，该例外必须限制在无法进入长期候选表的过渡对象上，不能扩散到已知远端的稳定 tunnel。

## 统一 publish 入口要求

- 设计上必须存在一个单一的“已登记 candidate 提升为 published”入口。
- 该入口负责：
  - 把候选状态从 hidden 提升为 published
  - 向 tunnel 订阅者广播新增可见 tunnel
  - 保持幂等，避免同一 candidate 重复广播
- direct/proxy/incoming/reverse 完成路径都必须复用这一入口，而不是在各自调用点复制 publish 逻辑。

## reverse waiter 清理要求

- `reverse_waiter` 的生命周期只覆盖“本地还在等待这次 reverse open 结果”的时间窗口。
- 一旦 reverse open 超时、取消或失败，对应 waiter 必须立即移除。
- waiter 被移除后，同 `(remote_id, tunnel_id)` 的 later-arriving reverse tunnel 不得继续保持 hidden，而必须按普通新 tunnel 规则 publish。
- 实现阶段若发现 hedged direct/reverse 竞争会留下悬空 waiter，必须优先修正 waiter 清理，再推进 publish 简化实现。

## 对现有实现形状的约束

- 实现阶段应收敛“调用方传入 publish 布尔值、外层 open 流程补 publish、内部再判重”的分散式结构。
- 调用方可以决定“是否发起某条建链路径”，但不应负责定义该路径成功后是否属于对外可见 candidate。
- 对外可见性应由 `TunnelManager` 内部的统一生命周期模型决定，而不是由各个 open 路径各自拼接条件。

## 风险与回滚

- 最大风险是误把“延后 publish 的 reverse”改成“提前 publish 的 reverse”，从而改变 hedged open、默认复用和订阅时序。
- 第二类风险是 proxy candidate 若从 publish-only 语义改为 register-and-publish，可能改变 `get_tunnel()`、upgrade 状态清理或空闲清理的覆盖面。
- 若实现阶段无法在不破坏现有 reverse waiter 语义的前提下完成收敛，应先回滚到当前行为并补充 testing 约束，而不是保留半收敛状态。
