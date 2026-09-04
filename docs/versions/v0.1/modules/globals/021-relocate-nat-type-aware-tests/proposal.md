---
module: globals
task_name: 021-relocate-nat-type-aware-tests
submodule: 021-relocate-nat-type-aware-tests
version: v0.1
status: draft
approved_by:
approved_at:
approved_content_sha256:
---

# 020 NAT-Aware 测试文件归位 Proposal

## Background and Goal

已完成的 `020-nat-type-aware-traversal` 在 `p2p-frame` 与 `sn-miner-rust` 中新增了多份独立测试文件，其中一部分位于 `<crate>/src/**`。这不符合“测试实现归属于对应 crate 的 `<crate>/tests/**`”的布局要求；其中 `p2p-frame/src/sn/protocol/sn/nat_type_wire_tests.rs` 还会命中仓库根 `.gitignore` 的 `sn/` 规则，存在测试通过但文件没有进入版本控制的风险。

本任务作为 020 的 sibling 修正任务，只调整 020 相关测试实现、测试专用 fixture 和 `#[cfg(test)]` 装配位置：所有相关测试文件进入所属 crate 的 `tests` 目录，测试命令、测试语义、私有项访问能力和 020 已有覆盖保持不变。

## Scope

### In scope

- 将 020 直接新增在 `p2p-frame/src/**` 下的独立测试文件迁入 `p2p-frame/tests/unit/**`，按 `nat_type`、`networks`、`sn`、`tunnel` 等责任保留清晰目录结构。
- 将 `sn-miner-rust/src/nat_probe_config_tests.rs` 迁入 `sn-miner-rust/tests/unit/**`。
- 把 020 新增在生产源文件内的测试实现和测试专用 fixture 提取到所属 crate 的 `tests/unit/**`；生产文件只允许保留必要的 `#[cfg(test)]` 模块声明、路径装配或最小测试可见性 seam，不保留 020 测试函数正文。
- 对需要 crate 私有项的 unit 测试，使用 `#[cfg(test)]` 配合 `#[path]` 或 `include!` 从 `tests/unit/**` 装配到原 crate 的 unit-test 编译上下文；不得为了迁移测试扩大生产 API 可见性。
- 保留已经位于 `p2p-frame/tests/nat_type_aware/**` 和 `p2p-frame/tests/nat_profile_public_api.rs` 的测试，只在必要时更新装配路径。
- 更新本 sibling task 的测试注册与验证入口，使原 020 的模型、wire、SN、TunnelManager、probe、PunchOnly 和 sn-miner 配置覆盖继续真实执行，并增加静态检查证明 020 相关测试文件不再位于任一 crate 的 `src` 目录。
- 验证所有迁移后的文件都可被版本控制发现，特别是原先被 `sn/` ignore 规则遮蔽的 wire 测试。

### Out of scope

- 不修改 020 已接受的 NAT profile、SN query/call、连接矩阵、预测、PunchOnly 或 PN fallback 生产行为。
- 不直接修改已完成的 020 proposal、pipeline plan/state、testplan、artifact 或 acceptance report；本任务产生独立的测试与验收证据。
- 不迁移仅被 020 回归执行、但由 017—019 或其他历史任务拥有的既有测试文件，例如历史 QUIC listener cadence 测试。
- 不做全仓库所有历史 `src/**/tests.rs`、内联 `mod tests` 或其他 crate 测试布局的统一清理。
- 不新增 Harness 规则、修改 `.gitignore`、改变 Cargo feature、依赖、生产可见性或测试断言语义。

### Boundary with neighboring modules

- `p2p-frame/tests/unit/**` 拥有需要 crate 内部上下文的 020 unit/DV 测试实现；`p2p-frame/tests/nat_type_aware/**` 继续拥有现有 NAT-aware 场景文件。
- `sn-miner-rust/tests/unit/**` 拥有配置解析 unit 测试；`sn-miner-rust/src/main.rs` 只保留 test-only 装配。
- `p2p-frame/src/**` 与 `sn-miner-rust/src/**` 的生产代码和公开接口不因测试迁移改变；必要的 `#[cfg(test)]` 模块装配不进入 release 构建。
- 020 的历史任务 packet 保持不可变；021 只证明迁移后的等价覆盖和文件可追踪性。

## Requirement Review

- 该要求合理。独立测试文件放在 crate 的 `tests` 树能让测试资产边界更清晰，也消除路径被宽泛 ignore 规则吞掉的风险。
- Rust 顶层 `tests/*.rs` 会作为独立 integration crate 编译，无法直接访问 crate 私有项；因此私有 unit 测试应放在不会被 Cargo 自动当作 integration target 的 `tests/unit/**`，再从原模块的 `#[cfg(test)]` 声明装配。直接把所有文件平铺到 `tests/` 会迫使扩大生产 API，反而违背本任务目标。
- 迁移必须保持原测试模块名或同步更新精确过滤器，避免出现命令成功但实际执行 0 个测试。
- 本任务只处理 020 引入的测试和必要装配。把所有历史测试一次性迁移会扩大审阅面，并使 020 修正与无关模块重构混在一起。

## Proposal Items

| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-NTATL-1 | nat_type_aware_test_file_layout | 020 相关独立测试文件、测试函数正文和 test-only fixture 必须位于所属 crate 的 `tests/**`，`src/**` 只保留最小 `#[cfg(test)]` 装配 | 仅 `p2p-frame`、`sn-miner-rust` 中由 020 引入的测试资产 | 需要维护相对路径和私有 unit-test 装配，换取清晰且可追踪的测试边界 | 路径清单与静态扫描证明目标文件全部位于对应 crate 的 `tests`；`git check-ignore` 不再隐藏相关文件 | 不迁移无关历史测试，不扩大生产 API |
| P-NTATL-2 | nat_type_aware_test_registration_parity | 迁移后保留 020 原有测试语义、测试数量、精确过滤器和 unit/DV/integration 覆盖，不允许零测试过滤器或只编译不执行 | 新 sibling task 注册迁移后的测试；020 历史 artifact 不修改 | 文件路径改变会影响模块装配和测试名，必须显式验证等价性 | 迁移前后测试名称/数量对照、精确测试命令非零执行、020 相关 task-scoped run 全部成功 | 不借迁移改写断言、放松失败条件或扩大运行到根级全量套件 |

## Success Criteria

- 020 直接新增的独立测试文件全部位于 `p2p-frame/tests/**` 或 `sn-miner-rust/tests/**`，不存在对应的 `src/**` 测试文件副本。
- 020 新增在生产文件中的测试函数正文和 test-only fixture 已进入对应 crate 的 `tests/unit/**`；生产源文件仅保留 release 不可见的最小装配。
- 私有 unit 测试无需把 production symbol 改成 `pub` 或新增公开测试 API。
- 原先位于 `p2p-frame/src/sn/protocol/sn/` 且被忽略的 wire 测试迁移后能被 `git status`/`git ls-files` 正常识别，不再命中 ignore 规则。
- 迁移前后 020 相关测试名称与执行数量有机械对照；每个精确过滤命令至少执行一个测试，原 unit、DV、integration 和兼容性行为继续通过。
- 020 已验收 packet 与历史 017—019 测试不被修改；不运行或宣称根级 `all all`、未授权 quality gate 或公网 NAT 验证。

## Risks

- `#[path]` 的相对基准取决于声明所在模块文件；计算错误会导致文件找不到或装配到错误模块名。
- 将私有 unit 测试误做成独立 integration target 会造成私有项不可见，并可能诱发不必要的 production API 扩张。
- 改变模块层级可能让旧的精确测试过滤器执行 0 个测试；验证必须检查实际执行数量，而不能只看退出码。
- 源文件当前包含用户和 020 的未提交改动；迁移必须以 task baseline 区分既有内容，避免覆盖无关工作。

## Trigger Matrix

| Trigger Category | Applies? | Evidence | Required Checks | Completed Checks | Deferred Checks and Reason | Residual Risk |
|------------------|----------|----------|-----------------|------------------|----------------------------|---------------|
| contract/protocol | no | 不改变公开 API、SN wire、TunnelNetwork trait 或 NAT 语义，仅移动 test-only 文件和装配 | 对照 production diff 不含非 `#[cfg(test)]` 语义变化 | proposal 已限定不扩大可见性、不修改 020 行为 | owner: acceptance; reason: 后续审计 production diff；acceptance impact: 发现生产契约变化则阻断 | test-only 装配错误属于验证风险 |
| data/schema | no | 不读写持久化数据、缓存 schema、desc/sec 或迁移 | 审计 scope 不含持久化路径 | proposal 明确排除 | owner: none; reason: not applicable; acceptance impact: none | none |
| security/privacy/permission | no | 不改变公网 UDP、输入验证、身份、权限或 secret 处理 | 复用原 020 安全测试，确认断言未变 | proposal 明确只迁移测试 | owner: none; reason: not applicable; acceptance impact: none | none |
| runtime/integration | no | production runtime 不变；变化只存在于 `cfg(test)` 编译 | 运行原 020 unit/DV/integration 相关用例并核对非零测试数 | proposal 已要求覆盖等价 | owner: testing; reason: 迁移后执行；acceptance impact: 任一行为回归阻断 | 测试模块名漂移可能造成漏跑 |
| build/dependency/config/deployment | no | 不修改 Cargo.toml、依赖、feature、配置或 release build，只改变测试源文件位置 | 编译两个 crate 的相关测试 targets | proposal 明确禁止 build surface 变化 | owner: testing; reason: 迁移后执行；acceptance impact: 测试 target 编译失败阻断 | 不同平台的相对路径解析需由 Rust 编译验证 |
| ui/datamodel/workflow | no | 不涉及 UI、数据模型展示或用户流程 | 审计 scope 不含 UI crate | proposal 明确排除 | owner: none; reason: not applicable; acceptance impact: none | none |
| harness/process | no | 不修改 Harness 规则、脚本、schema、模板或 CI；只为 021 创建正常任务测试注册 | 使用现有 doc、scope 和 task runner 检查 | proposal 明确不新增规则 | owner: none; reason: not applicable; acceptance impact: 现有 checker 失败仍阻断 | none |

## Approval Record

- approver:
- approval_date:
- user_statement: ""
