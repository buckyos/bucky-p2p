---
task_manifest: task.yaml
status: approved
---

# NAT 探测权威存活检查按观测路径判定 Proposal

Risk profile: not-created

## Workflow Tier Judgment
- Proposed tier: standard
- Final tier: standard
- Tier rationale / triggered boundaries: 修复集中在 `p2p-frame` 服务端 NAT 探测权威的存活判定（`service.rs` 的 reconcile 与 `nat_probe_scheduler.rs` 的路径谓词），不修改 `ReportSn`/`NatProbeDirective` wire 字段、版本、签名或持久化数据，不新增依赖、不改公开导出类型。涉及运行期并发与连接生命周期语义（权威注册回收），属 bounded bugfix，按默认规则归为 standard，不进入 high-risk。若确认时要求独立 design/testing/acceptance 文档，则升级 high-risk。
- Proposal and tier confirmation: 用户于 2026-09-13 回复“确认”，确认本提案（含把观测路径口径统一为「协议 + 地址」以及「同路径仍有活流时不回收注册」两项语义决定）与 standard tier 执行。

## Background and Goal
任务 075 已把 NAT 探测权威身份从「某一条命令流」放宽为「认证 peer + 已注册观测路径」，但**存活检查没有同步放宽**：`p2p-frame/src/sn/service/service.rs:1474` 的 `reconcile_nat_probe_authority` 仍然只做 `tunnels.iter().any(|tunnel| tunnel.conn_id == authority_tunnel)`，即只认最初建立注册的那条命令流 A。`p2p-frame/src/sn/service/service.rs:1489` 取不到 A 时，`finish_nat_probe_authority_reconcile` 会走 `remove_peer_if_authority(..., TunnelMissing)` 删除注册，并在 `p2p-frame/src/sn/service/service.rs:1518` 调用 `peer_mgr.invalidate_net_profile` 清空该 peer 的 NAT 画像。

实际后果：命令流会因单次 QA 超时被 `set_disable()` 关闭（客户端 073/075 语义：只关这一条流，不关底层 bearer），A 关闭后即使同路径流 B 与底层连接仍健康，也会被后台 `maintain_nat_probe_state`（以及 `ReportSn`/`SnQuery`/`SnCall` 处理入口调用的 reconcile）判定为权威消失，导致注册被删、画像被清空，进而影响后续 NAT 策略选择——这正是 075 想避免的「A/B 口径不一致」同类缺陷，只是这次不一致发生在存活检查一侧。

目标：权威存活判定与服务端接受判定的身份口径一致，即「认证 peer 的已注册观测路径上仍有活着的命令流」。A 关闭、B（同路径）存活时不得删除注册或清空画像；同路径最后一条流也消失后仍应正常回收。

## Scope
### In scope
- `p2p-frame/src/sn/service/service.rs`：`reconcile_nat_probe_authority` 的存活判定改为「列表中仍存在 `conn_id == authority_tunnel_id` 的流」**或**「存在与注册观测路径相同的活流」；路径相同的判定复用调度器的共享谓词。`finish_nat_probe_authority_reconcile` 的 `(authority_tunnel_id, registration_generation)` 快照防竞态语义保持不变。
- `p2p-frame/src/sn/service/nat_probe_scheduler.rs`：新增一个可被两处共用的观测路径谓词（协议 + 地址）以及一个只读访问器，用同一把锁读出注册观测端点；`is_authoritative_report` 改用该共享谓词，使「接受上报」与「判定存活」由同一口径决定。
- 回归测试：“A 关闭、B 存活”时注册与画像保留（含真实 SN + 真实客户端的端到端用例），“同路径全部关闭”时仍回收（正向对照），以及路径谓词本身的单元用例（同地址不同协议不等价、不同地址不等价）。

### Out of scope
- 不修改 `ReportSn`/`ReportSnResp`/`NatProbeDirective` 的 wire 字段、命令版本、签名、证书或编解码。
- 不改变 075 已确认的语义：同路径多通道上报仍被接受；不同观测路径、非 UDP 隧道仍被忽略。
- 不改变注册建立条件、directive 触发/版本/request_id、画像新鲜度（`observed_at`/TTL）、`expire_due`、`observe_ineligible_report` 与 peer 断开清理路径。
- 不新增依赖、不改 `sfo-cmd-server`/`sfo-pool`、不引入跨 peer/跨 SN 的存活广播或缓存。
- 不改客户端的 active-SN 生命周期、上报选流与回收策略（075 的 `SN_REPORT_STREAM_FAILURE_EVICT_THRESHOLD` 语义不变）。

### Boundary with neighboring modules
- 服务端收口在 `service.rs`（reconcile 触发点与画像失效）与 `nat_probe_scheduler.rs`（权威注册与路径谓词）；下游 `handle_query_sn`/`local_peer_detail` 继续从 scheduler/peer_mgr 读取画像，差异只体现在「同路径仍有活流时不再被误删」。
- TTP bearer、`sfo-cmd-server` 的 peer 连接表语义不变：只读 `get_peer_tunnels` 与每条流的观测远端端点。

## Requirement Review
需求成立，且方向正确：本缺陷与 075 修复的是同一类问题——权威身份有两处判定，只改了一处。因此这里不做「只在客户端把上报固定回原流」这类绕行（那会回退 073 的多通道架构），也不采用「收到同路径上报时把 `authority_tunnel_id` 改写为上报流」的方案：后者依赖上报时序，A 关闭后若 B 尚未上报，仍然会误删注册，无法满足“A 关闭、B 存活”的直接场景。

选中方案：存活判定 = 注册流仍在 **或** 存在同路径活流，路径口径由一个共享谓词承担。谓词取「协议 + 观测地址」而不是仅地址：`needs_registration` 用的地址比较只服务「外网地址是否变化」，而权威身份描述的是「哪条观测路径」，协议不同即路径不同；否则同一 ip:port 上的 TCP 流可能让已失效的 UDP 权威长期存活。该收紧同时消除了「接受判定」与「存活判定」再次分叉的可能，是本次要消除的缺陷根因。代价是：注册期使用 QUIC 的 peer，若出现同地址但不同协议的上报，将不再被判为同一权威路径——这与客户端按 `ActiveSN.sn_endpoint`（含协议）选流的行为一致，不产生实际回归。

## Proposal Items
| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-001 | CHG-authority-liveness-observed-path | 权威存活判定按「认证 peer + 已注册观测路径」进行：注册流存在，或同路径仍有活流时，不得删除注册、不得清空画像 | 仅改 `reconcile_nat_probe_authority` 的存活判定与调度器的共享路径谓词/只读访问器；`remove_peer_if_authority` 快照防竞态、`expire_due`、断开清理语义不变 | 存活检查需要读取候选流的服务端观测端点（可能等待被占用的流句柄），代价受该 peer 命令流数量约束 | 新端到端用例证明 A 关闭后查询仍返回该 peer 的画像、注册未被删除；同路径全部关闭后画像按原语义被清空；调度器单元用例证明路径谓词对同地址不同协议/不同地址返回不等价 | 不改 wire、不改接受判定语义、不改客户端 |
| P-002 | CHG-authority-path-liveness-regression-tests | 覆盖“A 关闭、B 存活”保留与“同路径全部关闭”回收两个方向的回归证据，并覆盖共享谓词边界 | 测试只落在 `p2p-frame` 现有测试面（`src/sn/tests.rs`、`tests/unit/sn_tests/service/service/nat_probe_scheduler_tests.rs`，必要时 `tests/nat_type_aware/sn_profile_flow_tests.rs`） | 端到端用例需要真实 SN + 客户端与真实命令流关闭，运行时长略增 | 上述用例通过，且 `nat_probe_scheduler` 现有 17 个权威用例与 `sn_profile_flow_tests` 全量通过 | 不引入新的测试框架或外部依赖 |

## Success Criteria
- 系统可见结果：同一 peer 的权威命令流 A 被单次 QA 超时关闭、同路径流 B 与底层 bearer 仍健康时，后台 reconcile 不再删除注册，查询仍能取到该 peer 的 NAT 画像。
- 系统可见结果：同路径最后一条命令流也消失后，注册仍按既有 `TunnelMissing` 语义被回收、画像被清空（不因本次修复而永久保留陈旧画像）。
- 所需证据：
  - `cargo test -p p2p-frame --features x509 --lib`（含新增用例与现有 `nat_probe_scheduler`/`sn_profile_flow`/`sn::tests` 全量）
  - `cargo test -p p2p-frame --features x509`（含 `nat_probe_logging_contract` 等集成目标）
  - `cargo check --workspace`
- 非目标：不声明公网/多主机/多 SN 部署或混合版本兼容验证完成；本任务只验证本地套件与定向回归。

## Risks
- 存活扫描需要逐条读取候选命令流的服务端观测端点，读取可能等待该流上正在进行的服务端 QA（例如 `SnTunnelRendezvousNotify`）；扫描范围受该 peer 的命令流数量约束，但会在 reconcile 热路径上引入新的等待点，需要确认不会与现有 QA 超时形成长时间阻塞。
- 路径口径收紧为「协议 + 地址」改变了 075 的单一地址比较：若某个部署确实依赖“同地址不同协议视作同一路径”，本任务会表现为不再接受该上报，需要在确认阶段明确认可。
- 画像保留时间可能变长：只要同路径仍有活流，注册就不会因 A 关闭而被回收；画像是否过期仍由既有 TTL/`expire_due` 决定，需要确认这一分工符合预期。
- 本地套件通过不等价于生产 NAT 环境验证；端到端用例只能覆盖同机 loopback 路径。
