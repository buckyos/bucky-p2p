---
task_manifest: task.yaml
status: approved
approved_by: user
approved_at: 2026-09-16T15:20:00+08:00
---

# 移除 legacy CYFS 工具 crate 并将 cyfs-p2p-test 身份改为 x509 支持的配置

Risk profile: ./risk-profile.yaml

## Workflow Tier Judgment

- Proposed tier: high-risk
- Final tier: high-risk
- Tier rationale / triggered boundaries: 本任务删除三个跨模块 workspace crate，改变依赖图、构建产物与工作区成员；同时把 cyfs-p2p-test 的身份从 CYFS object desc / PrivateKey 切换为 x509 证书身份。命中 material cross-project、dependency/build-graph、produced-artifact、security-identity 与 architectural-boundary，不属于 trivial/standard 的单模块局部改动。
- Proposal and tier confirmation: 用户回复 `确认，自动完成`，确认按 high-risk 执行并显式启动 auto-pipeline，后续 design/implementation/testing/acceptance 不再逐阶段停顿。

## Background and Goal

当前 workspace 有五个成员：`p2p-frame`、`cyfs-p2p`、`cyfs-p2p-test`、`sn-miner-rust`、`desc-tool`。后三个（以及 `cyfs-p2p` 适配层）依赖 `bucky-crypto 0.1.0`，而该 registry crate 与当前解析到的 `ecies 0.2.11` 不兼容，`encapsulate`/`decapsulate` 增加了 `compressed: bool` 参数，导致其自身无法编译。

目标是把 `cyfs-p2p`、`sn-miner-rust`、`desc-tool` 从 workspace 和仓库移除，保留 `p2p-frame` 与 `cyfs-p2p-test`；`cyfs-p2p-test` 不再使用 CYFS `Device`/`PrivateKey` 身份，而是改用 `p2p-frame::x509` 提供的 `X509Identity`、`X509IdentityFactory`、`X509IdentityCertFactory` 与 x509 栈配置。

## Scope

### In scope

- 删除源码目录 `cyfs-p2p/`、`sn-miner-rust/`、`desc-tool/`，并从根 `Cargo.toml` workspace members 中移除，更新 `Cargo.lock`。
- 重写 `cyfs-p2p-test`：
  - 移除 `cyfs-p2p`、`bucky-crypto`、`bucky-objects`、`bucky-raw-codec` 依赖；
  - 用 `p2p_frame::x509::{generate_rsa_x509_identity, X509Identity, X509IdentityFactory, X509IdentityCertFactory}` 替换 `CyfsIdentity`/`CyfsIdentityCert`/相关 factory；
  - 将 `create_cyfs_p2p_config`、`create_cyfs_p2p_stack_config`、`cyfs_to_p2p_endpoint` 的调用改为直接拼装 `p2p_frame::stack` 的 `P2pConfig`/`P2pStackConfig`；
  - 迁移 `main.rs` 中 SN service、PN server、device finder 等对 `cyfs_p2p` 的引用到 `p2p_frame` 等价接口。
- 同步 workspace 活跃引用：`AGENTS.md` 模块列表、`harness/workspace-governance.yaml`、`harness/scripts/verify-workspace-harness.py`、`docs/architecture/*`、`docs/modules/{cyfs-p2p,cyfs-p2p-test,sn-miner,desc-tool}.md`、`harness/human-rules/module-tier-matrix.md`。
- 验证剩余 workspace 可编译，p2p-frame 的 x509 相关测试通过。

### Out of scope

- 不删除 `p2p-frame`，不重写 SN 协议、隧道、NAT 探测或命令流行为。
- 不把 CYFS object desc/sign 能力迁移到新身份体系中。
- 默认保留历史版本化任务文档与验收材料（`docs/versions/v0.1/modules/{cyfs-p2p,sn-miner,desc-tool}/`、reviews、test-results），除非用户明确要求一并删除。
- 默认不修改根目录 gitignored 的运行时产物（`sn.desc`、`sn.sec`、`devices/`、`profile/`、`sn/` 等）。
- 不清退或回退当前 Cargo.lock 中已存在的未提交变更；锁文件只随本次成员删除自然更新。

### Boundary with neighboring modules

`cyfs-p2p-test` 受 `harness/custom-rules/cyfs-p2p-test-stage-ban-rules.md` 约束：它只能作为 implementation 阶段维护的产品代码，不能作为本任务或其他任务的 testing/acceptance 证据。x509 身份的正确性由 p2p-frame 自身测试面验证。

## Requirement Review

移除三个 legacy CYFS crate 是合理的：它们既是 `bucky-crypto` 编译阻塞的主要来源，也是当前架构中不再需要的 CYFS 适配层与工具二进制；移除后 workspace 只剩 p2p-frame 和 x509 化的 cyfs-p2p-test。

主要 tradeoff 与风险：

- 删除的是可执行/库 crate，若未来仍需 CYFS desc 工具或 SN 启动器，只能从 git 历史恢复。
- cyfs-p2p-test 身份语义变化：从“object desc + 对象签名”变为“x509 证书 + 证书签名”，栈内身份名、endpoint/SAN 更新与 SN 列表绑定需要与现有 p2p-frame x509 实现对齐。
- 仓库内散布着大量历史文档引用旧成员；只清理“活跃引用”，不改写历史，避免把流程文档重写卷入本次任务。
- 若用户要求持久化身份，p2p-frame 目前只有 x509 身份生成与证书解码（`X509IdentityCert::from_der/from_pem`），没有公开的“证书+私钥加载为身份”API，需要额外扩展公共接口，属于范围扩大。

方向：按上述范围执行，具体两处边界由用户确认后定稿。

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-001 | remove_cyfs_p2p_crate | 删除 `cyfs-p2p/` 源码目录与 workspace 成员 | 只删该 crate 及其活跃引用 | 失去 CYFS 适配层，换取消除 bucky-crypto 依赖与编译阻塞 | `cyfs-p2p`/`cyfs_p2p` 不再出现在剩余构成与活跃引用中；workspace 编译通过 | 不在 p2p-frame 中重建 CYFS 适配 |
| P-002 | remove_sn_miner_crate | 删除 `sn-miner-rust/` 源码目录与 workspace 成员 | 只删该 crate 及其活跃引用 | SN 启动二进制从仓库移除，运营端需要替代方案或从 git 恢复 | `sn-miner`/`sn_miner` 不再出现在剩余构成与活跃引用中；workspace 编译通过 | 不把 SN 服务合并进 cyfs-p2p-test 作为正式产物 |
| P-003 | remove_desc_tool_crate | 删除 `desc-tool/` 源码目录与 workspace 成员 | 只删该 crate 及其活跃引用 | 失去 CYFS desc 创建/签名工具 | `desc-tool` 不再出现在剩余构成与活跃引用中；workspace 编译通过 | 不把 desc 工具逻辑迁移到 cyfs-p2p-test |
| P-004 | x509_identity_cyfs_p2p_test | cyfs-p2p-test 使用 x509 身份与工厂，移除 CYFS object 身份依赖 | 只改 cyfs-p2p-test 与它的直接依赖 | 身份持久化/配置语义可能变化，x509 证书替代 desc/sec 文件 | cyfs-p2p-test 以纯 p2p-frame 依赖编译；P2pStack 使用 x509 身份启动 | 不把 x509 身份支持做回 CYFS object 兼容层 |
| P-005 | sync_workspace_governance | 同步 AGENTS、workspace 治理、架构/模块文档与校验脚本中的模块清单 | 不重写历史 review/test-results | 活跃文档与代码状态保持一致，需要触碰 harness/scripts 与 governance | `verify-workspace-harness.py` 与 workspace 校验通过；活跃引用无旧成员 | 不修改 Harness 阶段规则本身 |

## Success Criteria

- 可见结果：`Cargo.toml` workspace members 只剩 `./cyfs-p2p-test` 与 `p2p-frame`；`cyfs-p2p-test/Cargo.toml` 不再出现 `cyfs-p2p`、`bucky-crypto`、`bucky-objects`、`bucky-raw-codec`。
- 构建证据：`cargo check --workspace` 或等价工作区检查通过；`cargo check -p cyfs-p2p-test --all-targets` 通过。
- 功能证据：`cargo test -p p2p-frame --features x509 --lib` 通过（x509 身份正确性证据）。
- 引用清理：剩余代码与活跃工作区配置中 `rg "cyfs_p2p|cyfs-p2p|bucky_crypto|bucky-crypto"` 不再命中（历史 review/test-results 除外）。
- 显式非目标：不把 cyfs-p2p-test 的运行输出作为 testing/acceptance 证据；不声称删除工具后仍有等价运营能力。

## Risks

- 构建图残留风险：某处仍引用已删 crate 或模块名会导致 `cargo check` 失败；用全库 `rg` 扫尾与 workspace 校验兜底。
- 删除不可逆风险：源码目录删除后只能从 git 历史恢复；gitignored 运行时产物不受影响。
- 身份语义风险：x509 身份的行为（SAN 名称、endpoint 更新、SN 列表、签名类型）与旧 Device identity 不同，需要按 p2p-frame 现有 x509 契约实现。
- 证据边界风险：cyfs-p2p-test 禁止作为正式测试/验收证据（custom rule），本任务只对其做编译/诊断级验证。
- 范围扩大风险：若需要持久证书/私钥文件配置，需要给 p2p-frame 增加公开加载 API，将扩大本任务范围并提升风险；默认先采用 x509 生成身份配置。

## 待确认问题

1. 历史任务文档：`docs/versions/v0.1/modules/{cyfs-p2p,sn-miner,desc-tool}/` 下的 proposals/designs/acceptance 是否也删除？默认保留历史、只删源码与活跃引用。
2. x509 身份配置语义：每次运行动态生成 x509 身份（默认），还是要支持从 PEM/DER 证书+私钥文件加载持久身份（需要扩展 p2p-frame 公共 API）？
3. 根目录 gitignored 的 `sn.desc`、`sn.sec` 等本地产物是否一并清理？默认保留。

## 确认的默认边界

用户以 `确认，自动完成` 按提案原样确认，三个待定问题采用提案中显示的默认边界：

1. 历史版本化任务文档与验收材料保留，只删除源码目录与活跃引用。
2. x509 身份每次运行动态生成，不扩展 p2p-frame 的持久证书/私钥加载公共 API。
3. 根目录 gitignored 的 `sn.desc`、`sn.sec` 等本地产物保留。
