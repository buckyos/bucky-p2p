---
module: p2p-frame
submodule: sn-client-qa-correlation-fix
version: v0.1
status: approved
approved_by: user
approved_at: 2026-07-10T21:20:30+08:00
approved_content_sha256: 109ca1634c7f580f05911b1ba632055e069c9c8040489a174d7060a3a211e6d7
---

# SN Client QA Correlation Fix Proposal

## Background and Goal
`SNClientService` 当前把所有正在等待的 `ReportSnResp` 共享到一个全局 `cur_report_future`。客户端接收 `ReportSnResp` 时取走这个全局 future，但没有用 command tunnel、请求 `seq` 和目标 SN 身份确认响应属于哪一次 report；report 发送失败或 timeout 时，部分退出路径也不会及时清理该 future。

这会形成跨请求串扰：Report A timeout 后，Report B 覆盖全局 future；A 的迟到响应随后可能唤醒 B，使 A 的 endpoint 数据按 B 的 SN 身份进入 active SN、endpoint 分类以及 QUIC/TCP fallback 结果。

`SnCall` 与 `SnQuery` 虽然比 report 更严格，当前仍由 `ActiveSN.recv_future` 以业务 `seq` 保存 application-level waiter，并由独立 response handler 按 active connection 查找；发送失败路径在切换到下一个 SN 前没有移除已插入 waiter。继续为三类同步交互维护两套业务层 completion 机制，会留下不一致的清理与错误处理边界。

本提案要求把客户端发起且由 SN 服务端直接返回结果的 `ReportSn`、`SnCall` 和 `SnQuery` 请求/响应统一迁移为 command 层 QA 交互。SN 业务层不再自行管理 tunnel/header sequence 或 pending map；这些关联由 `sfo-cmd-server` QA runtime 内部完成。客户端解析 QA response body 后仍校验业务 `seq`，并在响应包含 `sn_peer_id` 时校验目标 SN 身份。成功、发送失败、timeout、取消和 tunnel 关闭等退出路径都不得留下可被迟到响应命中的 live pending request。目标是消除 SN client 自建 response waiter 的串扰、残留和重复实现，而不是把服务端主动推送的所有 SN 命令都改成 QA。

## Scope
### In scope
- 将客户端 `ReportSn`、`SnCall` 和 `SnQuery` 发送与各自 response 接收改为 command QA 事务，不再依赖全局 `cur_report_future`、`ActiveSN.recv_future`、`SnResp` enum 或独立的 `ReportSnResp` / `SnCallResp` / `SnQueryResp` completion handler。
- SN client/service 不自行生成、保存或匹配 QA header sequence，也不建立以 tunnel/header sequence 为键的 application-level pending map；`sfo-cmd-server` 负责按自身 QA contract 关联 response。
- 解析 response body 后，客户端必须确认 `ReportSnResp.seq == ReportSn.seq`、`ReportSnResp.sn_peer_id` 等于目标 SN，确认 `SnCallResp.seq == SnCall.seq`、`SnCallResp.sn_peer_id` 等于目标 SN，并确认 `SnQueryResp.seq == SnQuery.seq`。
- 迟到、重复、QA transaction 不匹配、body sequence 不匹配或 SN 身份不匹配的响应不得完成其他请求；不得污染 active SN、report endpoint、call 结果、query 结果或 QUIC/TCP fallback。
- 成功、编码失败、发送失败、timeout、future 取消、command tunnel 关闭和响应校验失败路径都必须结束对应 QA transaction；不得保留仍可被后续响应命中的 live waiter。
- SN 服务端 `ReportSn`、`SnCall` 和 `SnQuery` handler 必须通过 QA response 返回原有 response body，并保持业务字段与既有成功/错误语义。
- 生成能够稳定复现“Report A timeout、Report B 开始、A 迟到”的 red-green 回归证据，并为 Call/Query 覆盖迟到、错误 body identity、发送失败和 timeout 不残留业务 waiter。

### Out of scope
- 不把 SN 服务端主动发起的 `SnCalled` / `SnCalledResp`、inter-SN command 或 owner directory command 迁移为 QA；`SnCalled` 是独立异步推送边界，应另行评估。
- 不改变 `ReportSn` / `ReportSnResp` body 字段、raw codec、SN endpoint 分类规则、QUIC-first/TCP-fallback 顺序或 active SN 去重规则。
- 不用一个新的全局 future、单槽 waiter、只按 body `seq` 的 map 或只按 SN id 的 map 替代 `cur_report_future`。
- 不通过接受第一个到达的 `ReportSnResp`、延长 timeout、串行禁止后续 report 或关闭全部 SN 连接来掩盖关联缺陷。
- 不保留与 QA completion 并行竞争的 legacy response handler 或 application-level pending map。

### Boundary with neighboring modules
- `p2p-frame/src/sn/client` 负责发起 Report/Call/Query QA、解析并校验响应 body、决定 report fallback/call/query 结果，以及移除 application-level report/call/query waiter 状态。
- `p2p-frame/src/sn/service` 负责在处理同一个 Report/Call/Query request 时返回对应 QA response body；peer 更新、endpoint 生成、call relay/Called 推送和 query lookup 仍由既有 SN service 逻辑负责。
- `sfo-cmd-server` 负责 QA header 的 tunnel/cmd/sequence 关联和 response waiter 生命周期。若当前依赖在发送失败或取消路径只能留下已取消 tombstone，design 必须证明该状态不可被迟到响应命中且不会无界增长，或定义最小依赖修复/升级路径。
- `p2p-frame/src/sn/protocol` 的 Report/Call/Query request/response body codec 不在本次修改范围内；command header 的 request/response framing 属于本次兼容性审计范围。

## Assumptions and Ambiguities
- Assumptions:
  - 当前 `sfo-cmd-server` 的 `send_by_specify_tunnel_with_resp(...)` 与 handler `Ok(Some(CmdBody))` 是仓库已采用依赖中的正式 QA 接口。
  - command tunnel 已绑定经过认证的远端 SN；即使如此，response body 的 `sn_peer_id` 仍需与目标 SN 显式比对，避免错误服务端逻辑或错误 body 被接受。
  - Report/Call/Query response 的业务 `seq` 继续回显对应 request 的业务 `seq`，用于端到端一致性校验；SN 业务层不生成、保存或比对 QA header sequence。
- Open ambiguities:
  - QA framing 会把三个独立 response command 改为各自 request command 的 response frame；当前没有已批准的混版本协商或 fallback 机制。
  - 当前 `sfo-cmd-server` QA waiter 在正常 timeout 后清理，但发送在 waiter future 被轮询前失败时，依赖内部可能保留已取消条目；design 必须审计其容量与清理语义。
- Decision needed before approval:
  - 本次迁移客户端发起、SN 服务端直接响应的 `ReportSn`、`SnCall` 和 `SnQuery`；不迁移服务端主动发起的 `SnCalled` 或其他 SN command。
  - QA 迁移按 coordinated client/server upgrade 处理；未设计并验证安全协商前，不承诺新旧端混跑时 Report/Call/Query 成功，也不增加 legacy response handler fallback。
  - 任何依赖层已取消条目必须不可完成后续请求；若可能无界积累，则 design 必须纳入依赖升级、patch 或等价的有界清理方案。

## Constraints
- Allowed libraries/components:
  - 现有 `sfo_cmd_server::client::ClassifiedCmdSend::send_by_specify_tunnel_with_resp(...)` QA API。
  - 现有 command handler 的 `CmdResult<Option<CmdBody>>` response 返回能力。
  - 现有 Report/Call/Query request/response raw codec 和 `call_timeout`。
- Disallowed approaches:
  - 继续使用 `SNServiceState.cur_report_future`、`ActiveSN.recv_future`、`SnResp` 或等价 application-level response waiter。
  - 由 SN client/service 读取或维护 QA header sequence，并在业务层复制一套 tunnel/header pending map。
  - QA timeout 后回退到 legacy response handler，造成双 completion 竞争。
  - 为兼容旧端静默双发 Report/Call/Query，除非后续 proposal 明确定义幂等、协商与副作用边界。
- System constraints:
  - `p2p-frame` 是 Tier 0 模块；SN command framing、timeout、并发和 mixed-version 行为必须在 design 与 post-implementation testing 中直接覆盖。
  - 不得改变 SN endpoint area 分类、TCP observed endpoint 过滤或 QUIC/TCP fallback 的既有批准语义。
  - 实现前仍需独立批准 design，并以本提案 change_id 通过 schema/admission 门禁。

## Requirement Challenge
| question | evaluation | risk_or_tradeoff | decision |
|----------|------------|------------------|----------|
| 将 ReportSn、SnCall、SnQuery 改为 QA 是否能消除业务层 waiter 管理？ | 能。这三类都是客户端发起、SN handler 直接产生 response body；现有 QA runtime 内部完成 tunnel/cmd/header sequence 关联，SN 业务层可直接等待返回 body。 | QA runtime 仍依赖内部关联，但 SN 层不应观察或复制该 key；body 业务字段仍需做内容一致性检查。 | 三类交互统一使用 QA，删除 SN 层 report/call/query waiter；QA header correlation 仅作为依赖保证。 |
| 是否应直接实现一个 `(tunnel_id, seq, sn_peer_id)` pending map 以保持旧 wire？ | 该方案能精确修复 Report bug，也更容易保留独立 response command 的混版本兼容，但会在 SN client 重复维护 command 层已有的 request/response waiter 机制。 | 自有 map 需要正确处理插入冲突、所有退出清理、取消与迟到响应；QA 则引入 framing 迁移和依赖 waiter 清理审计。 | 按用户要求采用 QA；SN 层不管理 tunnel/header sequence。若 mixed-version 是硬要求且无安全协商，必须退回 proposal。 |
| 是否应把 SnCalled 也改成 QA？ | 不应自动纳入。`SnCalled` 是 SN 主动向被叫 peer 推送的异步通知，并非原 caller 在同一 tunnel 上等待的直接 response；它属于 SnCall 后续投递流程。 | 一并迁移会改变 server-initiated、多 tunnel fan-out 和被叫回执语义，显著扩大协议面。 | 本次只迁移 Report/Call/Query；SnCalled/Resp 保持既有异步 command。 |
| QA 是否自动满足“所有退出路径清理”？ | 正常 response/timeout 路径由当前依赖管理，但发送失败发生在 waiter future 开始等待之前时需要额外审计；不能仅凭 API 名称假设无残留。 | 忽略已取消 tombstone 可能消除串扰却引入长期内存增长。 | 将发送失败、取消和 tunnel close 的 pending 清理列为 design 阻塞项和 acceptance 审计项。 |
| scope 是否仍有影响验收的歧义？ | mixed-version Report/Call/Query 是否必须成功会决定是否能使用纯 QA。仓库当前没有 QA capability negotiation，legacy handler fallback 会重新引入双 completion 风险。 | coordinated upgrade 会使混版本请求失败；兼容协商则扩大 wire/design/testing 范围。 | 当前提案选择 coordinated upgrade、无 silent fallback；若用户要求混版本兼容，需在批准前修订本提案。 |

## Large Module Submodule Decision
| Submodule | New or Existing | Responsibility | Proposal Packet | Reason |
|-----------|-----------------|----------------|-----------------|--------|
| `sn-client-qa-correlation-fix` | new sibling task packet over existing `sn/client` and `sn/service` responsibilities | 将客户端 Report/Call/Query 统一迁移到 QA，删除业务层 response waiter 并约束 pending 生命周期，不创建新的生产子模块 | `docs/versions/v0.1/modules/p2p-frame/sn-client-qa-correlation-fix/proposal.md` | `p2p-frame` 根 packet 已批准且未覆盖该行为；独立 fix packet 可隔离审批、兼容决策、准入和 red-green 证据，避免修改已批准父 packet。 |

## Trigger Matrix
| trigger_category | applies | evidence | required_checks | deferred_checks_and_reason |
|------------------|---------|----------|-----------------|----------------------------|
| contract/protocol | yes | `p2p-frame/src/sn/client/sn_service.rs` 当前接收独立 `ReportSnResp`、`SnCallResp`、`SnQueryResp` command；QA 将使用各 request command 的 response framing，同时保留原 body codec | design 记录三类 wire/framing 与 mixed-version 决策；正向 QA、错误 body identity、旧新端兼容边界和 caller/callee 影响检查 | none |
| data/schema | no | `p2p-frame/src/sn/protocol/sn.rs` 与 `v0.rs` 的 Report/Call/Query request/response body 字段和 raw codec 明确保持不变，也不涉及持久化数据 | source/codec review 确认无 body schema diff | none |
| security/privacy/permission | yes | Report/Call response 的 `sn_peer_id` 决定客户端接受哪个 SN 的 endpoint/call 结果；错误归属会污染身份相关状态 | 负向覆盖错误 `sn_peer_id` 和错误业务 `seq` 被拒绝且不更新状态；Query 错误业务 `seq` 被拒绝；日志不得泄露证书或敏感 payload | none |
| runtime/integration | yes | `SNClientService` 的 `cur_report_future` 与 `ActiveSN.recv_future` 分别管理 report 和 call/query 的 send/timeout/cancel，存在迟到响应、残留和 fallback 顺序风险 | report red-green A timeout/B pending/A late；Call/Query send error、timeout、cancel、tunnel close、duplicate/late/mismatched response；QUIC failure 后 TCP report | none |
| build/dependency/config/deployment | yes | QA 依赖 `sfo-cmd-server` waiter 行为；若需 dependency upgrade/patch 会影响 `Cargo.toml`/`Cargo.lock` 和部署混版本边界 | design 审计 0.3.3 API/cleanup；若变更依赖则做 clean build、版本风险与 rollback 检查 | none |
| ui/datamodel/workflow | no | 变更仅影响 Rust 网络栈内部 SN client QA transaction，不存在 UI、展示模型或交互流程 | 确认无 UI surface | none |
| harness/process | no | 本任务使用既有 proposal/design/admission/testing/acceptance 规则，不修改 harness 文件、schema 或统一入口 | 运行既有 doc-structure、stage-scope、admission 和后续 testing/acceptance checks | none |

## High-Level Outcomes
- Report A 的迟到或重复响应永远不能完成 Report B。
- Report、Call、Query 都直接等待 command QA response，不再通过 SN application-level waiter/handler 配对。
- Report/Call 只接受业务 `seq/sn_peer_id` 与请求一致的 response；Query 只接受业务 `seq` 一致的 response。
- 所有退出路径都不会留下可完成后续请求的 SN 业务 pending 状态；`cur_report_future`、`ActiveSN.recv_future` 和 `SnResp` 不再存在。
- endpoint、active SN、call/query 结果和 QUIC/TCP fallback 只消费本次已验证的 response。
- Report/Call/Query body codec 与既有业务语义保持不变；SnCalled/Resp 仍为异步 command。

## Proposal Items
| proposal_id | change_id | Outcome | Scope Boundary | Success Evidence | Explicit Non-Goal |
|-------------|-----------|---------|----------------|------------------|-------------------|
| P-SN-CLIENT-QA-1 | sn_client_qa_transactions | ReportSn、SnCall、SnQuery 使用 command QA transaction 返回各自 response；SN 层不管理 tunnel/header sequence 或 pending map，只校验 response body 的业务 `seq` 与适用的 `sn_peer_id`。 | `p2p-frame/src/sn/client/**`、`p2p-frame/src/sn/service/**` 和必要的 SN QA regression seams；不改变 protocol body codec。 | 正向三类 QA 均成功；report red-green 证明 A late 不完成 B；Call/Query 错误或迟到 body 不完成其他请求；代码审查确认 SN 层无 QA header key 管理。 | 不迁移 SnCalled/inter-SN/owner commands，不增加 legacy handler fallback，不改变 endpoint/call/query 业务规则。 |
| P-SN-CLIENT-QA-2 | sn_client_qa_pending_cleanup | 删除 application-level `cur_report_future`、`ActiveSN.recv_future`、`SnResp` 和三个 response completion handler；response、编码/发送失败、timeout、取消、tunnel close 和校验失败后均无可被迟到响应命中的 live pending request，依赖层状态不得无界残留。 | SN client Report/Call/Query completion 生命周期以及必要的 `sfo-cmd-server` dependency/API 适配；不得借机重写通用 command runtime。 | 分支级 unit 证据覆盖三类请求退出路径；代码审查确认无业务 waiter；依赖 waiter 审计或测试证明发送失败/取消后无可复用 live waiter 和无界增长。 | 不以禁用并发、延长 timeout 或关闭全部 SN 连接替代 cleanup。 |
| P-SN-CLIENT-QA-3 | sn_client_qa_migration_boundary | Report/Call/Query QA framing 采用 coordinated client/server upgrade；在没有已批准 capability negotiation 前，不提供 silent legacy fallback，也不宣称 mixed-version 兼容。 | 仅限定三类 request/response command header framing 和部署迁移说明；body codec、SnCalled/Resp、其他 SN command 与 SN identity/TLS 不变。 | design 给出新/新成功、旧/新与新/旧明确结果矩阵及 rollback；testing 覆盖支持组合并确认 unsupported 组合快速、可诊断失败且不污染状态。 | 不在本任务新增协议协商、双发送或全 SN command version negotiation。 |

## Success Criteria
- Concrete user-visible or system-visible result:
  - Report A timeout 后开始 Report B，即使 A 的响应随后到达，也不会唤醒或完成 B。
  - 客户端不会把 A 的 endpoint 数据登记为 B 的 SN，也不会因此改变 active SN、endpoint 分类或 QUIC/TCP fallback。
  - SnCall/SnQuery 的迟到、重复或业务身份不匹配 response 不会完成其他请求或返回错误 peer/SN 数据。
  - 正常新客户端与新 SN 服务端仍能通过 control-stream command tunnel 完成 Report/Call/Query；SnCalled 异步投递继续工作。
- Required evidence:
  - 已批准 design 明确三类 QA request/response flow、SN 层不管理 QA header sequence、body identity checks、dependency pending owner/cleanup、failure transitions、SnCalled 边界、mixed-version matrix 和 dependency waiter 决策。
  - implementation admission 以 `sn_client_qa_transactions`、`sn_client_qa_pending_cleanup` 和需要时的 `sn_client_qa_migration_boundary` 直接映射通过。
  - Report bugfix red-green 证据，以及 Report/Call/Query 的 send error、timeout、cancel、late/duplicate/mismatched response、正常流程和 protocol fallback post-implementation tests。
  - acceptance 审计 response correlation、资源生命周期、状态完整性、错误处理、wire 兼容边界和无界增长风险。
- Explicit non-goals:
  - 不迁移 SnCalled/inter-SN/owner command，不改变 request/response body codec，不新增 mixed-version capability negotiation，不重写 endpoint、call、query 或 tunnel 选择策略。

## Risks
- QA response 使用 request command code 加 response flag，和当前三个独立 response command 的 framing 不同；未协调升级会导致 mixed-version Report/Call/Query 失败。
- SN 层不应管理 QA header sequence，但仍必须校验业务 response body；若把“无需 header 管理”误解成“无需 body 校验”，可能接受错误业务结果。
- 当前依赖在发送失败前创建 waiter；若 dropped waiter 只被标记 canceled 而不从 map 移除，虽然迟到响应不能唤醒下一请求，仍可能造成长期内存增长。
- 服务端 handler 若在 peer 状态更新后 response write 失败，重试会再次上报；design 必须说明现有 add/update 幂等边界，但本任务不扩展为通用 exactly-once 协议。
- report 与 QUIC/TCP fallback 高耦合；错误映射或 timeout 变化可能导致过早 fallback、重复 active SN 或丢失可用 SN。
- `SnCall` 触发 `SnCalled` 推送后再返回受理结果；QA 改造不得把 `SnCalledResp` 错当成原 caller 的 QA response，也不得改变 call 只表示 SN 受理的既有语义。

## Downstream Follow-Up
| follow_up_id | Owning Stage | Reason | Triggering Proposal Item | Blocking |
|--------------|--------------|--------|--------------------------|----------|
| FU-SN-CLIENT-QA-DESIGN | design | 定义三类 QA handler/client API、SN 层无 header sequence 管理、业务 body 校验、dependency pending owner、失败/取消 transition、SnCalled 边界、waiter 清理和 mixed-version matrix。 | P-SN-CLIENT-QA-1, P-SN-CLIENT-QA-2, P-SN-CLIENT-QA-3 | yes |
| FU-SN-CLIENT-QA-IMPLEMENTATION | implementation | 只有 proposal/design 获批且 admission 通过后，才能删除 Report/Call/Query 业务 waiter 并迁移 client/server handler。 | P-SN-CLIENT-QA-1, P-SN-CLIENT-QA-2 | yes |
| FU-SN-CLIENT-QA-TESTING | testing | implementation 后生成 report red-green 和三类交互的正常、边界、负向、错误、生命周期、并发、兼容测试与 testplan。 | P-SN-CLIENT-QA-1, P-SN-CLIENT-QA-2, P-SN-CLIENT-QA-3 | yes |
| FU-SN-CLIENT-QA-ACCEPTANCE | acceptance | 独立审计文档、代码、测试和结果是否阻止迟到串扰、状态污染、pending 泄漏、SnCalled 语义回归及未声明 wire 兼容。 | P-SN-CLIENT-QA-1, P-SN-CLIENT-QA-2, P-SN-CLIENT-QA-3 | yes |

## Proposal Guardrails
- 本 sibling proposal 不修改已批准的 p2p-frame 根 packet。
- 当前阶段只允许修改本文件；design、生产代码、测试和 acceptance 制品仍被门禁阻止。
- 本文保持 `status: draft`，只有用户显式批准后才能填写审批元数据和内容 hash。
- 如果用户要求 mixed-version Report/Call/Query 兼容或要求把 SnCalled/其他 SN command 也迁移为 QA，必须先修订并重新批准 proposal，不能在 design/implementation 中静默扩大范围。
- Testing 是 post-implementation 阶段，必须基于已批准 proposal/design 和已交付代码生成。

## Approval Record
- approver: user
- approval_date: 2026-07-10T21:20:30+08:00
- user_statement: "确认，自动处理后续步骤"
