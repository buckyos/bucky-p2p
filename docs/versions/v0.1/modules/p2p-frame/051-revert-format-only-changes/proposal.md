---
task_manifest: task.yaml
status: approved
---

# 回退纯格式变化 Proposal

Risk profile: not-created

## Workflow Tier Judgment
- Proposed tier: trivial
- Final tier: trivial
- Tier rationale / triggered boundaries: 只恢复 7 个已跟踪 Rust 文件到 `HEAD`；这些文件的工作区内容与 `HEAD` 内容分别经同一版本 rustfmt 规范化后逐字节一致，因此没有可识别的语义或内容变化。任务不改变协议、运行时行为、测试逻辑、依赖、构建图或公共接口，并可通过精确路径差异检查验证。
- Proposal and tier confirmation: 用户于 2026-09-04 确认所展示的 proposal 及建议的 `trivial` tier。

## Background and Goal
当前工作区包含大量来自其它任务的未提交修改，其中部分已跟踪 Rust 文件只有格式变化。目标是仅回退已机械确认的纯格式噪音，保留所有实质修改、删除、未跟踪文件以及 Harness 任务成果。

## Scope
### In scope
- 将以下 7 个 unstaged 文件恢复到 `HEAD`：
  - `p2p-frame/src/networks/control_stream.rs`
  - `p2p-frame/src/networks/control_stream/tests.rs`
  - `p2p-frame/src/networks/tcp/tunnel.rs`
  - `p2p-frame/src/pn/service/pn_server.rs`
  - `p2p-frame/src/pn/service/pn_server/tests/traffic_manager_tests.rs`
  - `p2p-frame/tests/cmd_pkg_len_compatibility.rs`
  - `p2p-frame/tests/endpoint_from_str_safety.rs`
- 回退前后分别检查精确路径状态，确认候选文件从未提交差异中消失。
- 确认其它已跟踪修改、删除和未跟踪文件保持不变。

### Out of scope
- 不回退包含实质内容变化的文件或其中的任何区块。
- 不删除任何未跟踪文件，不恢复当前已有的删除项，不修改 staged 区域。
- 不运行全仓库格式化，也不调整 Rust 代码行为或测试行为。

### Boundary with neighboring modules
任务只处理 `p2p-frame` 中经规范化比较确认的 7 个文件；`sn-miner-rust`、Harness、文档以及 `p2p-frame` 的其它变更均视为现有用户工作并保留。

## Requirement Review
请求合理，但直接依据增删行数或肉眼判断回退会有误删实质修改的风险。采用“`HEAD` 与工作区分别经同一 rustfmt 规范化后完全一致”作为候选条件，可将范围限定为纯格式差异；整文件恢复这些候选比逐 hunk 手工反向编辑更可审计，也不会留下部分格式噪音。

## Proposal Items
| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-001 | CHG-revert-format-only-changes | 仅回退机械确认的纯格式修改，并保留工作区中的全部实质修改。 | 仅限 Scope 中列出的 7 个 unstaged 已跟踪文件。 | 规范化比较以当前 `rustfmt 1.9.0-stable` 为准；它保留注释和 token 内容，因此比忽略空白 diff 更严格。 | 7 个路径不再出现在 `git diff --name-only`；其它路径的状态集合及内容保持不变；针对候选和非候选执行新鲜差异复核。 | 清理其它脏工作区内容、格式化新增代码或改变 staged 区域。 |

## Success Criteria
- Concrete user-visible or system-visible result: 当前工作区不再包含上述 7 个文件的纯格式差异。
- Required evidence: 回退前的 rustfmt 规范化等价清单；回退后的精确路径 `git diff` 为空；非目标工作区差异保持不变；完成一次独立的比例化缺陷复核。
- Explicit non-goals: 不证明其它文件没有格式噪音；不对包含实质变化的文件做局部格式回退；不运行行为测试，因为交付目标是恢复到已跟踪基线且不改变规范化后的源码内容。

## Risks
主要风险是误把实质修改当作格式变化。范围已通过双向 rustfmt 规范化后的逐字节相等性筛选，并明确排除新增、删除、staged 以及规范化后仍不同的文件。执行时仍会保存 Harness 基线并在回退后复核非目标路径，防止扩大影响。
