# 工作区约束

## 工作区形态
| Crate | 类型 | 角色 | 主要依赖 | 下游依赖 |
|------|------|------|----------|----------|
| `p2p-frame` | library | 核心传输、tunnel、PN/SN、身份与运行时抽象 | Rust 密码学/网络库 | 工作区内其他所有 crate |
| `cyfs-p2p` | library | 面向 CYFS 的适配与再导出层，构建于 `p2p-frame` 之上 | `p2p-frame`、CYFS 对象/密码学库 | `cyfs-p2p-test`、`sn-miner-rust` |
| `cyfs-p2p-test` | binary | 运行时验证 harness 与场景驱动器 | `cyfs-p2p`、`p2p-frame` | 测试执行者 |
| `sn-miner-rust` | binary | SN 服务启动二进制 | `cyfs-p2p` | 服务操作人员 |
| `desc-tool` | binary/lib | descriptor 的创建、修改、签名与检查工具 | CYFS 对象/密码学库 | 操作人员与工具链 |

## Toolchain 与运行时
- 现有仓库约束中将 toolchain 固定为 `stable-x86_64-pc-windows-msvc`。
- 仓库中混用了多个 Rust edition：
  - `p2p-frame`：2024
  - `cyfs-p2p`、`cyfs-p2p-test`、`desc-tool`：2021
  - `sn-miner-rust`：2018
- 运行时策略由 feature 驱动，目前以 `runtime-tokio` 为优先。

## 现有验证面
- 工作区构建与测试：
  - `cargo check --workspace`
  - `cargo build --workspace`
  - `cargo test --workspace`
- 高信号模块验证：
  - `cargo test -p p2p-frame`
  - `cargo test -p cyfs-p2p`
  - `cargo run -p cyfs-p2p-test -- all-in-one`
  - `cargo run -p sn-miner -- --help`
  - `cargo run -p desc-tool -- --help`

## 边界规则
- `p2p-frame` 负责协议机制、传输抽象、tunnel 管理、SN/PN 行为以及身份/运行时支持。
- `cyfs-p2p` 可以适配或组合 `p2p-frame`，但不得静默分叉核心库的协议语义。
- `cyfs-p2p-test` 负责运行时场景驱动与面向操作员的验证流程；它不能变成正式测试计划的隐藏替代品。
- `sn-miner-rust` 与 `desc-tool` 是操作型二进制。若行为、默认值或契约边界发生变化，这里的改动仍然需要 proposal/design/testing。

## 日志与诊断
- 运行时验证通常会把日志写到仓库本地输出目录，例如 `logs/`。
- `devices/`、`profile/`、`sn/`、`sn.desc` 与 `sn.sec` 下生成的运行时制品必须被视为场景证据或本地状态，而不是已批准 design/testing 文档的替代品。

## 仓库中已存在的设计输入
- `p2p-frame/docs/` 中现有的协议说明可以作为后续模块数据包的设计参考。
- 它们不能替代 `docs/versions/<version>/modules/<module>/design.md`；只能由后者建立索引引用。
