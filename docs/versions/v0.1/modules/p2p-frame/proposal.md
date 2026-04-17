---
module: p2p-frame
version: v0.1
status: draft
approved_by:
approved_at:
---

# p2p-frame 提案

## 背景与目标
- `p2p-frame` 是整个工作区的核心传输和 tunnel 库。
- 这个数据包当前的直接目标，是为未来核心网络栈的改动建立一个严格、可评审的基线，确保协议、传输和运行时改动无法绕过 proposal、design、testing 和 acceptance。

## 范围
### 范围内
- 核心库的长期模块边界
- 对 `p2p-frame/docs/` 下现有协议说明建立设计索引
- 为 unit、DV 和 integration 定义明确的测试面
- 为未来所有 `p2p-frame` 工作定义硬性的 implementation admission 规则

### 范围外
- 重写当前协议实现
- 改变当前工作区成员布局
- 用新副本替换现有协议说明

### 与相邻模块的边界
- `cyfs-p2p` 可以适配 `p2p-frame`，但不拥有 `p2p-frame` 的协议语义。
- `cyfs-p2p-test` 提供运行时验证场景，但不能替代 `p2p-frame` 的 testing 设计。
- `sn-miner-rust` 和 `desc-tool` 消费相邻能力，它们是 integration 邻居，而不是核心栈的所有者。

## 约束
- 允许使用的库/组件：
  - 现有工作区 crate 和当前协议说明
  - 基于 cargo 的验证命令
- 禁止采用的方式：
  - 协议或运行时改动绕过阶段审批
  - 在不更新 testing 和 trigger-rule 覆盖的情况下静默改变传输行为
- 系统约束：
  - 保持当前以 tokio 为优先的运行时策略
  - 保持混合 edition 的工作区布局
  - 保持当前协议说明中记录的兼容性预期

## 高层结果
- 未来的 `p2p-frame` 改动必须通过显式模块数据包进入流程。
- 协议和运行时高风险改动必须触发更强的评审和验证。
- 核心栈工作的 acceptance 必须把结果回溯到 proposal 意图。

## 风险
- 旧设计说明与未来实现之间的协议漂移
- 传输或 tunnel 改动缺少运行时证据
- `cyfs-p2p` 和运行时二进制中的跨 crate 回归
