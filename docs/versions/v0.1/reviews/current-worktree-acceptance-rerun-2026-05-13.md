# 当前工作区复验报告

## 对象与范围
- 模块：p2p-frame、cyfs-p2p、workspace-harness，以及当前 worktree 中仍未跟踪的文件
- 版本：v0.1
- 评审日期：2026-05-13
- 范围内：`endpoint_area_server_reflexive`、workspace harness 准入与 acceptance 规则补齐、当前 worktree 状态复验
- 范围外：未能映射到上述 change_id 的历史评审报告、PN tunnel control channel 文档、p2p-frame 额外协议文档、`CLAUDE.md`

## 输入
- `docs/versions/v0.1/modules/p2p-frame/{proposal.md,design.md,testing.md,testplan.yaml}`
- `docs/versions/v0.1/modules/cyfs-p2p/{proposal.md,design.md,testing.md,testplan.yaml}`
- `docs/versions/v0.1/modules/workspace-harness/{proposal.md,design.md,testing.md,testplan.yaml}`
- `docs/modules/p2p-frame.md`
- `docs/modules/cyfs-p2p.md`
- `harness/rules/acceptance-task-rules.md`
- `harness/rules/acceptance-review-rules.md`
- 当前 git worktree、测试与准入命令输出

## 评审顺序
1. 审查已批准 proposal/design/testing/testplan 的状态与 change_id 映射。
2. 按 acceptance review 规则执行 worktree diff 与未跟踪文件分类。
3. 执行 schema/admission/测试入口。
4. 审查 DV 入口是否满足已批准 testplan 的完成语义。
5. 记录阻塞项和退回路由。

## 发现项
| ID | 严重级别 | 阶段 | 证据 | 问题 | 是否阻塞 |
|----|----------|------|------|------|----------|
| A1 | Info | Testing / DV | p2p-frame/cyfs-p2p 的 `testplan.yaml` 已将 `dv` 标记为 `disabled`，并声明 `cyfs-p2p-test all-in-one` 不作为本模块或其他模块的 DV 证据；`python3 ./harness/scripts/test-run.py p2p-frame dv` 与 `cyfs-p2p dv` 均返回 `unknown level`。 | 原 all-in-one DV 完成语义阻塞已按 harness testing 路由移除；后续新增 DV 必须另行 proposal/design/testing 并提供能自然成功返回的入口。 | 否 |
| A2 | High | Scope / Worktree hygiene | `git status --short --untracked-files=all` 仍列出 `CLAUDE.md`、`docs/versions/v0.1/reviews/p2p-frame-nat-traversal-acceptance-2026-04-25.md`、`docs/versions/v0.1/reviews/p2p-frame-pn-control-channel-acceptance-2026-05-10.md`、`docs/versions/v0.1/modules/p2p-frame/design/pn-tunnel-control-channel.md`、`docs/versions/v0.1/modules/p2p-frame/testing/pn-tunnel-control-channel.md`、`p2p-frame/docs/pn_proxy_forwarding_v2.md`、`p2p-frame/docs/proxy_tunnel_design.md`、`p2p-frame/docs/tcp_data_connection_reuse_protocol.md`、`p2p-frame/docs/tcp_tunnel_protocol_test_cases.md`、`p2p-frame/docs/tunnel_connection_sequence.md` 等未跟踪文件。 | 当前 admission 只覆盖 `endpoint_area_server_reflexive` 和 workspace harness 三个 change_id；上述未跟踪文件未能映射到本轮已批准范围。Acceptance 不能把未批准/未分类的 worktree 内容视为通过。 | 是 |
| A3 | Medium | Governance | `python3 ./harness/scripts/report-approval-status.py v0.1` 输出 p2p-frame/cyfs-p2p approved，但未列出新增的 `workspace-harness` 数据包；`workspace-harness` schema/admission 可单独通过。 | 新增 governance 数据包没有进入审批状态报告索引，后续操作者容易误判 workspace-harness 是否属于受治理模块。 | 否，但应在本轮收口前修正或明确豁免 |

## 一致性摘要
- Proposal 与 design：p2p-frame、cyfs-p2p、workspace-harness 当前均有 approved 文档；`endpoint_area_server_reflexive` 和 workspace harness change_id 的 schema/admission 通过。
- Design 与模块边界文档：p2p-frame/cyfs-p2p 公开语义已能追溯；workspace-harness 仍存在治理索引可见性风险。
- Design 与 implementation：tracked diff 为空，无法从 `git diff` 复核实现增量；当前验收只能基于工作区内容、准入脚本和测试结果判断。
- Testing 文档与 testplan：unit/integration 命令可执行；DV 当前 disabled，且不再映射到 all-in-one。
- Testplan 与实际执行：unit/integration/schema/admission 通过；p2p-frame/cyfs-p2p 的 DV 入口按 disabled 语义不可执行。
- 验收标准可追溯性：范围内 change_id 可追溯；未跟踪的额外文档和历史报告不可追溯到本轮 change_id。

## 执行证据
- `git diff --stat`：无输出。
- `git diff --name-status`：无输出。
- `git diff --cached --name-status`：无输出。
- `git diff --check`：通过，无输出。
- `python3 ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame`：passed。
- `python3 ./harness/scripts/schema-check.py --version v0.1 --module cyfs-p2p`：passed。
- `python3 ./harness/scripts/schema-check.py --version v0.1 --module workspace-harness`：passed。
- `python3 ./harness/scripts/admission-check.py --version v0.1 --module p2p-frame --change-id endpoint_area_server_reflexive`：passed。
- `python3 ./harness/scripts/admission-check.py --version v0.1 --module cyfs-p2p --change-id endpoint_area_server_reflexive`：passed。
- `python3 ./harness/scripts/admission-check.py --version v0.1 --module workspace-harness --change-id workspace_harness_acceptance_review_gate`：passed。
- `python3 ./harness/scripts/admission-check.py --version v0.1 --module workspace-harness --change-id workspace_harness_direct_admission_gate`：passed。
- `python3 ./harness/scripts/admission-check.py --version v0.1 --module workspace-harness --change-id workspace_harness_pipeline_plan_current_change`：passed。
- `python3 ./harness/scripts/verify-workspace-harness.py v0.1`：passed。
- `python3 ./harness/scripts/test-run.py p2p-frame unit`：passed，`cargo test -p p2p-frame`，95 passed。
- `python3 ./harness/scripts/test-run.py p2p-frame integration`：passed，`cargo test --workspace`。
- `python3 ./harness/scripts/test-run.py cyfs-p2p unit`：passed，`cargo test -p cyfs-p2p`。
- `python3 ./harness/scripts/test-run.py cyfs-p2p-test unit`：按当前范围返回 `unknown module`；`cyfs-p2p-test` 不作为独立 harness test-run 模块入口。
- `python3 ./harness/scripts/test-run.py p2p-frame dv`：按当前范围返回 `unknown level`；p2p-frame DV disabled。
- `python3 ./harness/scripts/test-run.py cyfs-p2p dv`：按当前范围返回 `unknown level`；cyfs-p2p DV disabled。

## 结论
- 通过或失败：失败。
- 原因：仍存在 High 阻塞项。worktree 中仍有未映射到本轮 approved change_id 的未跟踪文件。

## 退回路由
- Proposal 任务：若未跟踪的 PN/control-channel/额外协议文档属于本轮交付，需回到 proposal 补充目标、范围和 change_id；否则移出本轮 worktree。
- Design 任务：若未来需要长跑稳定性验证，需在 design/testing 中定义有界通过条件或人工停止判据。
- Testing 任务：当前已将 p2p-frame/cyfs-p2p DV disabled；未来新增 DV 必须定义自然成功返回的自动入口。
- Implementation 任务：当前无需为 all-in-one 补 runner；如未来采用有界 DV，需补实现入口或 runner timeout/round limit，使 `test-run.py <module> dv` 能返回 0。

## 残余风险
- `workspace-harness` 已能单独 schema/admission，但审批状态报告不展示它，治理可见性仍弱。
- tracked diff 为空而 worktree 有大量未跟踪文件；最终提交前需要明确哪些文件加入版本控制、哪些移出或忽略。
