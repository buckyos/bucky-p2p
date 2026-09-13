---
task_manifest: task.yaml
status: approved
---

# 多通道 SN 上报权威身份与失败补救 Proposal

Risk profile: not-created

## Workflow Tier Judgment
- Proposed tier: standard
- Final tier: standard
- Tier rationale / triggered boundaries: 三项修复都在 `p2p-frame` 单模块内：SN 服务端 NAT 探测权威身份判定、SN 客户端 rendezvous 错误码映射、ActiveSN 生命周期回收边界。不修改 `ReportSn`/`NatProbeDirective` wire 字段、签名、版本协商或持久化数据，不新增依赖，不改变公开导出类型；涉及运行期并发与连接生命周期语义，属于 bounded bugfix，按默认规则归为 standard，不进入 high-risk。若确认时要求独立 design/testing/acceptance 文档，则升级 high-risk。
- Proposal and tier confirmation: 用户于 2026-09-13 回复“确认”，确认本提案与 standard tier 执行；P-003 采用提案中已展示的“有界重试确认注册不可用后才回收”口径，未采用 Open Questions 中的备选口径。

## Background and Goal
任务 073 把 SN 命令面从“固定 `ActiveSN.conn_id` 流”改成“按分类复用/新建命令流”，方向正确，但有三处行为随之退化：

1. **多通道 ReportSn 与服务端权威流机制不兼容（P1）**。`p2p-frame/src/sn/client/sn_service.rs:1219` 的探测结果上报和 1187 的周期上报都用 `SnTunnelClassification::new(None, active_sn.sn_endpoint)` 交给池重新选流。服务端在 `p2p-frame/src/sn/service/nat_probe_scheduler.rs:226` 只接受与 `authority_tunnel_id` 完全相同的命令流：注册发生在流 A、上报选到流 B 时，结果与随附 `net_profile` 一起被忽略，但 `handle_report_sn` 仍返回 `result=Ok`，客户端据此记录上报成功、`latest_time` 前移，服务端画像长期缺失或过期。
2. **rendezvous 丢失原有本地补救（P1）**。`p2p-frame/src/sn/client/sn_service.rs:849` 把命令流的获取失败、超时、传输错误统一转换为 `ConnectFailed`；`rendezvous_via_sn` 直接用 `?` 透传。而 `p2p-frame/src/tunnel/tunnel_manager.rs:1585` 的 `is_ambiguous_rendezvous_failure` 只接受 `IoError`/`Unmatch`/`InvalidData`：对端已收到 notify、仅响应超时或丢失时，客户端现在直接失败退出，不再执行原有的本地打洞/等待动作。
3. **单次新建命令流失败误删健康 ActiveSN（P2）**。`p2p-frame/src/sn/client/sn_service.rs:816` 在 `get_send_by_classified` 失败后调用 `remove_active_sn`。池在“已有匹配流正被占用且未达 `sn_tunnel_count` 上限”时会改为新建流；新建失败只说明这一次扩容失败，不代表既有流不可用。此时删除 ActiveSN 会让在线状态消失，并让 `on_rendezvous_notify` 的 `active_sn_matches` 拒绝该 SN 经健康流发来的 rendezvous 通知。这与 073 已批准的 P-005/失败边界（“单条流失败不得使仍有健康流的 ActiveSN 失效”）也相矛盾。

目标：让多通道 SN 命令面与 NAT 探测权威身份语义重新一致；恢复 rendezvous 的既有本地补救分支；把 ActiveSN 回收收窄到“注册确实不可用”的证据上。

## Scope
### In scope
- 服务端权威身份改为“同一认证 peer 在同一被观测路径上注册的 UDP 命令流”：`observe_capable_report`、`observe_reported_profile`、`observe_control` 三个入口不再要求命令流 id 完全相等，而是要求上报命令流为 UDP 族且其服务端观测远端端点与当前注册观测端点一致；满足条件时接受结果/画像，并对不同观测端点或非 UDP 隧道保持现有忽略语义。
- 客户端 `rendezvous_via_sn` 恢复 073 之前的错误码映射：SN 命令流获取/发送/响应失败映射为 `P2pErrorCode::IoError`，使 `is_ambiguous_rendezvous_failure` 重新命中本地 action 补救分支（复用同一 tunnel_id/waiter 重试本地打洞或等待）。
- ActiveSN 回收收窄：共享命令流获取路径不再无条件删除整个 SN；只有当“注册不可用”被有界证据确认（获取失败后再做一次有界的新流健康尝试仍失败）时才回收 ActiveSN，使 ping 循环重新注册；失败判定与恢复策略抽成可单测的纯逻辑。
- 反例与回归测试：同 peer 同观测路径的第二条多通道上报被接受并更新画像；不同观测路径或 TCP 上报仍被忽略；rendezvous 的 SN 调用失败仍进入本地 action 复用分支；单次获取失败不删除 ActiveSN、持续不可用时可回收并重新注册。

### Out of scope
- 不修改 `ReportSn`/`ReportSnResp`/`NatProbeDirective`/`SnTunnelRendezvous*` 的 wire 字段、版本号、签名或编解码。
- 不修改 TTP 传输、`sfo-cmd-server`/`sfo-pool` 依赖与其版本，不新增依赖。
- 不改变 073 的多通道调度：`SnCall`/`SnQuery`/`SnTunnelRendezvous`/`SnCalledResp` 继续走分类池，`ActiveSN` 不重新持有通用固定流句柄。
- 不改变 NAT 探测周期、directive 版本/request_id 语义、画像新鲜度（`observed_at`/TTL）排序、`observe_ineligible_report` 的非 UDP 失效语义。
- 不改 SN 列表刷新、`reset_sn`/`stop` 生命周期，不改 `wait_online` 语义。

### Boundary with neighboring modules
- 服务端收口在 `p2p-frame/src/sn/service/nat_probe_scheduler.rs`（权威判定）与 `service.rs`（`reconcile_nat_probe_authority` 之后的合并点）。
- 客户端收口在 `p2p-frame/src/sn/client/sn_service.rs`（`send_sn_qa`、`report`、`rendezvous_via_sn`、`remove_active_sn` 调用点）。
- 补救分支的消费方 `p2p-frame/src/tunnel/tunnel_manager.rs` 只作为契约参照：其 `is_ambiguous_rendezvous_failure` 判定集合不变。
- 下游 `handle_query_sn`/`local_peer_detail` 继续从 scheduler/peer_mgr 读取 profile，差异只体现在“同路径多通道上报不再被丢弃”。

## Requirement Review
需求成立。第 1 项不是“客户端选错流”而是权威身份定义与多通道架构不匹配：073 之后同一 peer 到同一 SN 端点会按需在同一 bearer 上打开多条命令流，这些流在服务端共享同一个观测远端地址；把权威绑死在“注册时那一条命令流 id”会随池的调度策略随机失效，且客户端无法从成功响应中察觉。把权威身份下沉为“peer + 已注册的观测路径”既保留 068/073 已确认的安全边界（非 UDP 隧道、不同观测路径一律忽略），又不再依赖某条具体命令流，因此不改 wire、不需要客户端固定流句柄（避免回退 073 的 P-005）。现有 068 反例测试用不同观测端点区分“非权威隧道”，与本方向完全兼容。

第 2 项是 073 引入的确定性回归：错误码语义是 `tunnel_manager` 使用的内部契约，`ConnectFailed` 不在歧义集合内，导致“对端可能已收到通知”的补救路径被跳过。

第 3 项同样应与 073 的既有要求对齐：单条命令流创建失败不构成注册失效证据。由于 `sfo-cmd-server` 0.4 未暴露“是否存在可用既有流”的只读查询，本提案采用“有界重试后才能判定不可用”的近似判定，恢复延迟换取不误删。

## Proposal Items
| proposal_id | change_id | requirement | boundary | tradeoff | success_evidence | non_goal |
|-------------|-----------|-------------|----------|----------|------------------|----------|
| P-001 | CHG-nat-probe-authority-observed-path | NAT 探测权威身份改为“peer + 已注册观测路径”，同路径多通道 ReportSn/控制上报被接受 | 仅在 `observe_capable_report`/`observe_reported_profile`/`observe_control` 用观测远端端点一致性替代命令流 id 相等；非 UDP 或不同观测路径仍忽略；`authority_tunnel_id` 仍用于 reconcile 权威存活检查 | 权威不再等价于某条命令流，需要明确“同路径”比较口径与日志 reason 命名 | 新反例测试证明同 peer 同观测端点的第二条 UDP 流可上报并被接受，不同观测端点与 TCP 流仍被忽略；调度器/服务层/相关库套件通过 | 不改 wire 字段、不放开非 UDP、不放开不同观测路径 |
| P-002 | CHG-rendezvous-ambiguous-sn-failure | rendezvous SN 命令调用失败恢复为歧义失败语义，重新进入本地补救分支 | 只调整 `rendezvous_via_sn` 的错误码映射；`is_ambiguous_rendezvous_failure` 的判定集合与其确定性反例测试不变 | 获取流失败也一并映射为 `IoError`（与 073 之前一致），本地补救可能对“实际未送达”的请求多做一次本地动作 | 定向测试证明 SN 调用超时/失败后 `tunnel_manager` 复用同一 tunnel_id/waiter 执行本地 action；确定性错误仍直达 proxy | 不扩大歧义集合、不改 `ConnectFailed` 在 call/query 路径的语义 |
| P-003 | CHG-active-sn-eviction-scope | 单次命令流获取失败不再删除健康 ActiveSN；仅在确认注册不可用后回收 | 共享 `send_sn_qa` 不再直接 `remove_active_sn`；回收决策收敛到报告路径的有界证据；健康流仍可接收 rendezvous 通知 | 判定为近似实现，恢复延迟比“命中即删”略长 | 新增/改造测试证明单次获取失败后 ActiveSN 仍在且健康流通知被接受；注册确实不可用时有界重试后回收并重新注册（现有 unreachable 恢复测试语义保留） | 不引入池内部只读查询、不新增依赖、不改 `reset_sn`/`stop` |

## Success Criteria
- 系统可见结果：同一 SN 端点上按需新建的第二条命令流上报的探测结果与 `net_profile` 能在服务端被接受；不同观测路径与非 UDP 上报仍被忽略。
- 系统可见结果：对端已收到 rendezvous notify 但响应超时/丢失时，客户端仍执行本地打洞/等待补救，而不是直接失败退出。
- 系统可见结果：单条命令流创建/获取失败不再清空该 SN 的在线状态，也不拒绝该 SN 经健康流发来的 rendezvous 通知；注册确实不可用时仍能回收并重新注册。
- 所需证据：
  - `UV_CACHE_DIR=.harness/uv-cache uv run --active python ./harness/scripts/test-run.py p2p-frame/075-sn-authority-and-recovery all`
  - `cargo test -p p2p-frame --features x509 --lib` 与 `cargo test -p p2p-frame --features x509 --test nat_probe_logging_contract`
  - 与改动路径相关的定向测试（scheduler 权威反例、sn client report/active_sn、tunnel_manager 歧义失败判定）
- 非目标：不声明公网/多主机/部署环境或混合版本兼容验证完成；本任务只验证本地套件与定向反例。

## Risks
- “同观测路径”比较口径若与 `needs_registration` 不一致，可能出现“接受上报却触发重新注册”或多路径抖动；实现必须与既有 `needs_registration` 使用同一端点比较口径，并保留忽略日志便于生产判别。
- 有界重试放宽了 ActiveSN 回收时机：真实不可用的 SN 需要一轮额外尝试才会被回收，期间 `wait_online` 仍可能提前返回；需在 completion 报告中明确这一残余暴露。
- rendezvous 错误码映射放宽后，确定性失败若被误判为歧义会多做一次本地动作；必须保证 `InvalidParam`/`NotFound`/`UserCanceled` 等确定性路径不进入歧义集合。
- 三项修复共同影响 SN 命令面的运行时语义，缺少公网/多 SN 部署证据；本地套件通过不等价于生产 NAT 环境验证。

## Open Questions
- 确认时请一并确认 P-003 的回收口径：接受“有界重试（获取失败后再做一次新的获取+发送，仍失败才回收）”的近似判定，还是要求改为“获取失败一律不回收、仅由传输发送失败或 `reset_sn`/`stop` 触发回收”。
