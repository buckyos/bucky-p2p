---
module: cyfs-p2p
version: v0.1
status: draft
approved_by:
approved_at:
---

# cyfs-p2p 提案

## 背景与目标
- `cyfs-p2p` 是构建在 `p2p-frame` 之上的面向 CYFS 的适配层。
- 这个数据包的目标，是让未来的适配层变更和核心栈一样，必须经过严格的 proposal、design、testing 和 acceptance 链路。

## 范围
### 范围内
- 身份与证书适配
- 基于 `p2p-frame` 的栈/配置适配
- 面向 CYFS 的导出接口和类型转换

### 范围外
- 重新定义 `p2p-frame` 的核心传输或 tunnel 语义
- 替换由 `cyfs-p2p-test` 负责的运行时场景

### 与相邻模块的边界
- `p2p-frame` 负责协议和传输语义。
- `cyfs-p2p-test` 是端到端适配层行为的主要运行时证据面。
- `sn-miner` 在运行时消费该模块，但不拥有其 API 契约。

## 约束
- 保持与当前 `p2p-frame` 契约兼容。
- 避免 CYFS 身份适配与核心身份行为之间发生语义漂移。
- 影响运行时的改动需要下游 DV 或 integration 证据。

## 高层结果
- 适配层变更变得可追踪、可评审。
- 公开转换和栈构建行为拥有明确的测试覆盖。

## 验收锚点
| 条目 ID | 结果描述 | 约束/非目标 | 验收如何判断 |
|---------|----------|-------------|--------------|
| P-1 | 适配层改动可追踪、可评审 | 不得重写 `p2p-frame` 语义 | proposal/design/testing/acceptance 证据链齐全 |
| P-2 | 公开转换和栈构建行为拥有明确测试覆盖 | 运行时场景仍由下游模块证明 | testing 与 testplan 能对应到公开适配行为 |

## 风险
- 与 `p2p-frame` 的语义静默漂移
- 身份/签名适配回归
- 下游二进制中的运行时启动回归
