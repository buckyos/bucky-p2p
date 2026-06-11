---
module: cyfs-p2p
version: v0.1
status: approved
approved_by: user
approved_at: 2026-05-13
approved_content_sha256: 711fe011d2d04db4bdef306d5ba1669beb6773764734830d6d09ea3bbe8591be
---

# cyfs-p2p 提案

## 背景与目标
- `cyfs-p2p` 是构建在 `p2p-frame` 之上的面向 CYFS 的适配层。
- 这个数据包的目标，是让未来的适配层变更和核心栈一样，必须经过严格的 proposal、design、testing 和 acceptance 链路。
- 本轮新增需求是跟随 `p2p-frame` 的 endpoint area 语义变更：当核心库将 `EndpointArea::Default` 重命名为 `ServerReflexive` 后，适配层必须继续把该反射地址语义映射到当前 CYFS 下游仍使用的 `bucky_objects::EndpointArea::Default` 表示，且不得在适配层重新定义或吞并 `p2p-frame` 的 `Wan` / `ServerReflexive` 语义。

## 范围
### 范围内
- 身份与证书适配
- 基于 `p2p-frame` 的栈/配置适配
- 面向 CYFS 的导出接口和类型转换
- `cyfs-p2p/src/stack_builder.rs` 中对 `p2p-frame::EndpointArea::ServerReflexive` 的消费适配，保持与既有 CYFS endpoint area 枚举的兼容映射

### 范围外
- 重新定义 `p2p-frame` 的核心传输或 tunnel 语义
- 替换由 `cyfs-p2p-test` 负责的运行时场景
- 在 `cyfs-p2p` 中引入新的 endpoint area、NAT 类型推断、STUN/TURN 行为或替代 `p2p-frame` 的 SN 观察地址分类规则

### 与相邻模块的边界
- `p2p-frame` 负责协议和传输语义。
- `cyfs-p2p-test` 是端到端适配层行为的主要运行时证据面。
- `sn-miner` 在运行时消费该模块，但不拥有其 API 契约。

## 约束
- 保持与当前 `p2p-frame` 契约兼容。
- 避免 CYFS 身份适配与核心身份行为之间发生语义漂移。
- 影响运行时的改动需要下游 DV 或 integration 证据。
- `ServerReflexive` 在 `cyfs-p2p` 中只能作为核心库公开 enum 的新名称被消费；适配层不得把它提升为静态 `Wan`，也不得把下游 `Default` 反向解释为 `p2p-frame` 的旧 system default 语义。

## 高层结果
- 适配层变更变得可追踪、可评审。
- 公开转换和栈构建行为拥有明确的测试覆盖。

## Proposal Items
| proposal_id | change_id | Outcome | Constraints / Non-goals | Success Evidence |
|-------------|-----------|---------|--------------------------|------------------|
| P-1 | cyfs_adapter_traceability | 适配层改动可追踪、可评审 | 不得重写 `p2p-frame` 语义 | proposal/design/testing/acceptance 证据链齐全 |
| P-2 | cyfs_runtime_coverage | 公开转换和栈构建行为拥有明确测试覆盖 | 运行时场景仍由下游模块证明 | testing 与 testplan 能对应到公开适配行为 |
| P-ENDPOINT-AREA-ADAPTER | endpoint_area_server_reflexive | `cyfs-p2p` 适配层消费 `p2p-frame::EndpointArea::ServerReflexive`，并保持到现有 CYFS endpoint area 表示的兼容映射。 | 不在适配层重新定义 endpoint area 语义；不把 `ServerReflexive` 静默升级为 `Wan`；不引入新的 NAT 探测或下游协议语义。 | design/testing 需要覆盖 `stack_builder.rs` 的转换路径；integration 需要确认 workspace 下游仍能编译测试。 |

## 风险
- 与 `p2p-frame` 的语义静默漂移
- 身份/签名适配回归
- 下游二进制中的运行时启动回归

## Approval Record
- approver: user
- approval_date: 2026-06-11
- user_statement: 将已有文档都迁移到新的要求吧
