---
module: p2p-frame
version: v0.1
status: approved
approved_by: user
approved_at: 2026-04-17
---

# p2p-frame 提案

## 背景与目标
- `p2p-frame` 是整个工作区的核心传输和 tunnel 库。
- 这个数据包当前的直接目标，是为未来核心网络栈的改动建立一个严格、可评审的基线，确保协议、传输和运行时改动无法绕过 proposal、design、testing 和 acceptance。
- 当前待落地的直接需求是为 relay 侧 `pn/service/pn_server.rs` 增加用户流量统计与限速能力，并明确要求复用 `sfo-io` 中已有的统计/限速实现，而不是在 `p2p-frame` 内重复实现一套新的字节整形逻辑。

## 范围
### 范围内
- 核心库的长期模块边界
- 对 `p2p-frame/docs/` 下现有协议说明建立设计索引
- 为 unit、DV 和 integration 定义明确的测试面
- 为未来所有 `p2p-frame` 工作定义硬性的 implementation admission 规则
- relay 侧 `pn_server` 在成功握手后的双向字节桥接路径上增加按用户统计的流量计量
- relay 侧 `pn_server` 在成功握手后的双向字节桥接路径上增加按用户生效的限速能力
- 为 `pn_server` 明确 `sfo-io` 的接入边界、配置输入和统计口径，保证流量统计与限速共用同一套底层实现
- 记录 relay 侧按已认证 peer 身份归属流量和限速决策的要求，避免信任未经 relay 规范化的报文字段

### 范围外
- 重写当前协议实现
- 改变当前工作区成员布局
- 用新副本替换现有协议说明
- 改变 PN open 握手命令、结果码或现有 `ProxyOpenReq`/`ProxyOpenResp` 线协议
- 为非 `pn/service` 子模块引入同一轮的统一流量整形改造
- 在 `p2p-frame` 内重新实现一套独立于 `sfo-io` 的统计器、令牌桶或限速器

### 与相邻模块的边界
- `cyfs-p2p` 可以适配 `p2p-frame`，但不拥有 `p2p-frame` 的协议语义。
- `cyfs-p2p-test` 提供运行时验证场景，但不能替代 `p2p-frame` 的 testing 设计。
- `sn-miner-rust` 和 `desc-tool` 消费相邻能力，它们是 integration 邻居，而不是核心栈的所有者。
- `pn_server` 的流量统计与限速属于 `p2p-frame/src/pn/service/**` 的 relay 责任，不下沉给 `cyfs-p2p` 做语义分叉。
- `sfo-io` 负责提供被调用的统计/限速实现；`p2p-frame` 只负责在 relay bridge 生命周期中正确装配、调用和暴露所需观测点。

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
- 系统约束：
  - 保持当前以 tokio 为优先的运行时策略
  - 保持混合 edition 的工作区布局
  - 保持当前协议说明中记录的兼容性预期
  - 保持现有 crate 边界，不因为本次需求把 `pn_server` 的 relay 语义迁移到其他 crate
  - 流量统计口径必须与 relay 实际成功转发的字节数一致；仅进入用户态缓冲但未成功写出的字节不得提前计入
  - 限速应作用于 relay 成功握手后的桥接数据路径，不改变握手前 `ProxyOpenReq`/`ProxyOpenResp` 的控制流时序

## 高层结果
- 未来的 `p2p-frame` 改动必须通过显式模块数据包进入流程。
- 协议和运行时高风险改动必须触发更强的评审和验证。
- 核心栈工作的 acceptance 必须把结果回溯到 proposal 意图。
- relay 侧 `pn_server` 能按已认证用户维度统计上下行传输字节，并把统计结果接到 `sfo-io` 的实现上。
- relay 侧 `pn_server` 能按已认证用户维度执行限速，并保持成功握手后的透明字节桥接语义。
- 统计与限速都建立在同一条 `sfo-io` 接入链路上，避免计量口径和限速执行口径分叉。

## 风险
- 旧设计说明与未来实现之间的协议漂移
- 传输或 tunnel 改动缺少运行时证据
- `cyfs-p2p` 和运行时二进制中的跨 crate 回归
- `sfo-io` 尚未在当前工作区中显式接入；依赖选择、版本兼容和运行时行为都可能带来额外设计工作
- 若流量统计挂点放错位置，可能出现重复计数、漏计数，或把未成功落到底层 writer 的字节提前记账
- 限速引入的背压可能改变 relay bridge 的时延、关闭顺序或超时分布，需要在 testing 中单独建模
- 按用户维度归属统计和限速时，若身份归一化边界不清晰，可能导致统计串户或限速对象错误
