---
module: p2p-frame
version: v0.1
status: approved
approved_by: user
approved_at: 2026-04-23
---

# p2p-frame 提案

## 背景与目标
- `p2p-frame` 是整个工作区的核心传输和 tunnel 库。
- 这个数据包当前的直接目标，是为未来核心网络栈的改动建立一个严格、可评审的基线，确保协议、传输和运行时改动无法绕过 proposal、design、testing 和 acceptance。
- 当前待落地的直接需求，是在 relay 侧 `pn/service/pn_server.rs` 增加用户流量统计与限速能力，并让经过 proxy server 的 proxy tunnel 支持由使用者显式控制的可选端到端载荷加密；其中 `stream` 路径可显式启用 TLS-over-proxy，而 `datagram` 路径继续保持明文兼容并忽略该加密模式，要求继续复用 `sfo-io` 中已有的统计/限速实现，同时避免在 `p2p-frame` 内重复实现一套新的字节整形逻辑或把业务明文暴露给 relay。本轮新增澄清是：relay 侧统计不再只提供单边 `from` 视图，而是要求 source 和 target 都能独立查询属于自己的桥接流量统计数据；限速仍只按 source 侧用户生效，不因 target 侧统计可见性而扩展成双边限速。

## 范围
### 范围内
- 核心库的长期模块边界
- 对 `p2p-frame/docs/` 下现有协议说明建立设计索引
- 为 unit、DV 和 integration 定义明确的测试面
- 为未来所有 `p2p-frame` 工作定义硬性的 implementation admission 规则
- relay 侧 `pn_server` 在成功握手后的双向字节桥接路径上增加按用户统计的流量计量
- relay 侧 `pn_server` 为 source 与 target 两端分别保留各自可查询的用户流量统计视图
- relay 侧 `pn_server` 在成功握手后的双向字节桥接路径上增加按用户生效的限速能力
- 为 `pn_server` 明确 `sfo-io` 的接入边界、配置输入和统计口径，保证流量统计与限速共用同一套底层实现
- 记录 relay 侧按已认证 peer 身份归属流量和限速决策的要求，避免信任未经 relay 规范化的报文字段
- 明确 source 侧统计主体使用规范化后的已认证 `req.from`，target 侧统计主体使用目标侧已打开流对应的 `req.to`，且两者各自独立查询、互不覆盖
- 为 proxy tunnel 的 `stream` 路径定义“经由 relay 传输但不向 relay 暴露业务明文”的可选 TLS 载荷加密能力
- 明确 proxy tunnel 加密与已认证 peer 身份、relay bridge、用户统计和限速之间的边界
- 定义 proxy tunnel 的显式策略输入和调用接口，使使用者可以自行选择启用或关闭加密
- 约束“未开启加密”和“外部约定启用加密但 TLS 建立失败”时的行为，避免把明文 fallback 伪装成具备 confidentiality 的安全通道
- 明确 proxy tunnel 的 `datagram` 路径不承载 TLS-over-proxy 语义，即使同一 tunnel 选择了 `stream` 加密模式也继续按明文 datagram 行为工作，而不是返回 `NotSupport`

### 范围外
- 重写当前协议实现
- 改变当前工作区成员布局
- 用新副本替换现有协议说明
- 为非 `pn/service` 子模块引入同一轮的统一流量整形改造
- 在 `p2p-frame` 内重新实现一套独立于 `sfo-io` 的统计器、令牌桶或限速器
- 因为需要 target 侧统计可见性，就把用户级限速从 source 侧扩展为对 target 侧也生效的双边整形
- 让 relay/proxy server 持有、派生或恢复用于解密业务载荷的明文密钥
- 为 `pn` 之外的 active/reverse/direct tunnel 一次性引入同级别的业务载荷加密改造
- 为 proxy tunnel 的 `datagram` 路径在本轮同时引入 TLS 等价的载荷加密
- 借本轮需求重写整套 PN 协议

### 与相邻模块的边界
- `cyfs-p2p` 可以适配 `p2p-frame`，但不拥有 `p2p-frame` 的协议语义。
- `cyfs-p2p-test` 提供运行时验证场景，但不能替代 `p2p-frame` 的 testing 设计。
- `sn-miner-rust` 和 `desc-tool` 消费相邻能力，它们是 integration 邻居，而不是核心栈的所有者。
- `pn_server` 的流量统计与限速属于 `p2p-frame/src/pn/service/**` 的 relay 责任，不下沉给 `cyfs-p2p` 做语义分叉。
- `sfo-io` 负责提供被调用的统计/限速实现；`p2p-frame` 只负责在 relay bridge 生命周期中正确装配、调用和暴露所需观测点。
- source 与 target 的独立统计视图都属于 `p2p-frame/src/pn/service/**` 的 relay 责任边界；下游适配层只能消费查询结果，不能自行推导另一套统计口径。
- proxy tunnel 的端到端加密语义属于 `p2p-frame` 的 PN/tunnel 责任边界；`cyfs-p2p` 只能消费或配置该能力，不能在适配层静默定义另一套不受 relay 约束的 proxy 加密语义。
- proxy tunnel 的 `datagram` 明文兼容语义同样属于 `p2p-frame` 的 PN/tunnel 责任边界；`cyfs-p2p` 不得通过适配层把 `stream` 加密模式扩展成 `datagram` 拒绝或隐式加密语义。

## 约束
- 允许使用的库/组件：
  - 现有工作区 crate 和当前协议说明
  - 基于 cargo 的验证命令
  - `sfo-io` 中已存在的流量统计与限速实现
- 禁止采用的方式：
  - 协议或运行时改动绕过阶段审批
  - 在不更新 testing 和 trigger-rule 覆盖的情况下静默改变传输行为
  - 在 `pn_server` 中复制或旁路 `sfo-io` 的统计/限速逻辑，导致两套行为源
  - 通过未认证的 `from`/`to` 报文字段归属用户流量或决定限速对象
  - 以“经过 relay 后仍由 relay 可见明文”的方式宣称 proxy tunnel 已具备端到端加密
  - 把是否加密变成隐式默认行为，导致调用方无法明确控制 proxy tunnel 的保密语义
  - 在两端已显式约定启用加密的情况下静默降级到明文 proxy tunnel
- 系统约束：
  - 保持当前以 tokio 为优先的运行时策略
  - 保持混合 edition 的工作区布局
  - 保持当前协议说明中记录的兼容性预期
  - 保持现有 crate 边界，不因为本次需求把 `pn_server` 的 relay 语义迁移到其他 crate
  - 流量统计口径必须与 relay 实际成功转发的字节数一致；仅进入用户态缓冲但未成功写出的字节不得提前计入
  - source 与 target 两个统计视图都必须以各自“成功写到对端”的字节为准；不得把同一批字节重复累计到同一用户视图，也不得因为双边都可见而丢失任何一侧的记账
  - 限速应作用于 relay 成功握手后的桥接数据路径，不改变握手前 `ProxyOpenReq`/`ProxyOpenResp` 的控制流时序
  - target 侧新增统计可见性不得改变当前限速主体；若未来需要 target 侧限速或双边配额，必须单独通过新的 proposal 扩展
  - proxy tunnel 的加密边界必须位于 relay 无需解密业务载荷也能继续完成转发、统计和限速的层次
  - 若 relay 继续承担统计或限速，其默认口径必须以实际成功转发的密文字节为准，除非后续设计另行定义并证明明文口径
  - 加密模式下的身份认证仍必须锚定到底层 relay 已认证并规范化后的 peer 身份，而不是依赖未认证的业务载荷字段
- 使用者必须能通过显式接口或配置为单条 proxy tunnel、单个调用点或明确的构造路径选择“加密”或“不加密”，而不是依赖全局隐式副作用
- TLS 是否启用由 proxy tunnel 两端在 tunnel 外通过配置、调用约定或同一构造路径显式决定；relay 不负责为此增加额外协商语义
- `stream` 加密模式只约束 `stream` channel；`datagram` channel 在本轮必须忽略该模式并保持当前明文兼容语义，不能因为 `TlsRequired` 而被本地或对端 open/accept 路径拒绝

## 高层结果
- 未来的 `p2p-frame` 改动必须通过显式模块数据包进入流程。
- 协议和运行时高风险改动必须触发更强的评审和验证。
- 核心栈工作的 acceptance 必须把结果回溯到 proposal 意图。
- relay 侧 `pn_server` 能按 source 与 target 两端各自的用户维度统计上下行传输字节，并把两侧统计结果都接到 `sfo-io` 的实现上。
- relay 侧 `pn_server` 能按已认证用户维度执行限速，并保持成功握手后的透明字节桥接语义。
- 统计与限速都建立在同一条 `sfo-io` 接入链路上，避免计量口径和限速执行口径分叉；其中 source/target 的独立统计视图不能要求新增第二套统计实现。
- proxy tunnel 能由使用者显式选择是否启用端到端载荷加密；启用后 relay/proxy server 只暴露完成路由、控制流和配额所需的最小元数据。
- 本轮 proxy tunnel 加密能力以 `stream` 路径上的 TLS-over-proxy 为交付目标；`datagram` 不提供 TLS 语义，但在 `TlsRequired` 场景下仍保持明文兼容并忽略该模式。
- proxy tunnel 的加密能力具有显式的策略边界，不会把未加密路径误报为具备 confidentiality 的安全通道。

## 风险
- 旧设计说明与未来实现之间的协议漂移
- 传输或 tunnel 改动缺少运行时证据
- `cyfs-p2p` 和运行时二进制中的跨 crate 回归
- `sfo-io` 尚未在当前工作区中显式接入；依赖选择、版本兼容和运行时行为都可能带来额外设计工作
- 若流量统计挂点放错位置，可能出现重复计数、漏计数，或把未成功落到底层 writer 的字节提前记账
- 若 source/target 双边统计映射关系处理错误，可能出现串户、双记账，或 source/target 查询结果互相覆盖
- 限速引入的背压可能改变 relay bridge 的时延、关闭顺序或超时分布，需要在 testing 中单独建模
- 按用户维度归属统计和限速时，若身份归一化边界不清晰，可能导致统计串户或限速对象错误
- proxy tunnel 端到端加密会把 proxy stream 建链、TLS 身份校验和双端接口约束耦合到一起，若设计不当会直接引入兼容性回归
- relay 统计/限速在加密模式下可能只能看到密文字节和密文时序，这会改变口径认知、调试方式和运维诊断习惯
- 若加密模式与 passive `PnTunnel`/listener 接口装配顺序处理不当，可能破坏现有 open/accept 时序或导致隐性明文窗口
- 若 `datagram` 继续错误继承 `stream` 加密模式，现有依赖明文 datagram 的调用点会在显式开启 `TlsRequired` 后出现非预期拒绝，形成兼容性回归

## 验收锚点
- source 侧用户能够通过 `pn_server` 的查询入口看到属于自己这条 bridge 的累计统计，且记账主体使用 relay 规范化后的已认证 `req.from`，不受源端伪造 `from` 影响。
- target 侧用户能够通过 `pn_server` 的查询入口看到属于自己这条 bridge 的累计统计，且记账主体锚定到 relay 成功打开的目标用户 `req.to`，不依赖未认证的业务载荷字段。
- source 与 target 的统计视图都只统计成功握手后的 bridge payload，不统计 `ProxyOpenReq` / `ProxyOpenResp`，并且只在成功写出后入账。
- target 侧统计能力的引入不得把限速主体从 source 扩展成双边限速；现有用户级限速仍只按 source 侧用户生效。
- 双边统计在 TLS-over-proxy 场景下继续以 relay 实际成功转发的 TLS record / 密文字节为口径，而不是尝试恢复业务明文大小。
