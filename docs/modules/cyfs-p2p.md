# cyfs-p2p

## 类型
- 适配层库

## 职责
- 将 `p2p-frame` 的基础能力适配为面向 CYFS 的身份、栈配置和导出接口。

## 关键边界
- 范围内：
  - `cyfs-p2p/src/lib.rs`
  - `cyfs-p2p/src/stack_builder.rs`
  - `cyfs-p2p/src/types.rs`
- 范围外：
  - 属于 `p2p-frame` 的核心传输/协议机制

## 依赖
- `p2p-frame`
- CYFS object and crypto crates

## 下游依赖
- `cyfs-p2p-test`
- `sn-miner-rust`

## 模块级别
- Tier 1：高耦合适配层

## 必需的验证倾向
- 既要验证适配层本身正确性，也要验证它与 `p2p-frame` 的兼容性。
- 任何公开转换或配置行为变更，都需要通过 `cyfs-p2p-test` 提供 integration 证据。
