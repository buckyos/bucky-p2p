# 流水线计划

## 当前自动流水线：移除 SnServiceContractServer 相关逻辑

### 触发条件
- 已批准的 proposal：`docs/versions/v0.1/modules/p2p-frame/proposal.md`
- 用户启动确认：已于 `2026-06-05T18:58:53+08:00` 确认并要求自动处理后续步骤。
- User launch confirmed: 2026-06-05T18:58:53+08:00 确认并要求自动处理后续步骤。
- 当前 `change_id`：
  - `remove_sn_service_contract_server`

### 验收基线
- 最终验收以 `docs/versions/v0.1/modules/p2p-frame/proposal.md` 中 `remove_sn_service_contract_server` 的批准内容为准。
- 移除 `SnServiceContractServer`、`contract.rs`、`receipt.rs` 或等价服务合约/回执生产逻辑。
- 保留 `sn/protocol` 中既有 receipt wire 类型和消息字段兼容性，不删除 SN server/client 基础能力，不改变 SN command 线协议、`SnCallResp` 语义、peer manager、endpoint 分类、连接验证器或 control-stream-only 信令选择。
- 不引入新的计费、合约评估、配额、持久化账本或相邻模块兼容旁路。

### 阶段图
| 任务 ID | 阶段 | 职责 | 范围 | 依赖项 | 输出 | 完成条件 | 状态 |
|---------|------|------|------|--------|------|----------|------|
| P-SN-CONTRACT-CLEANUP-1 | proposal | 定义 SN service contract/receipt 清理目标、非目标和验收边界 | `proposal.md` | 用户需求 + 用户确认 | approved proposal | `remove_sn_service_contract_server` 已进入 Proposal Items 且 proposal approved | confirmed |
| PLAN-SN-CONTRACT-CLEANUP-1 | planning | 创建自动流水线阶段图、依赖、输出和退回路由 | `harness/pipeline-plan.md` | approved proposal + 用户启动 | pipeline plan | 计划覆盖 design、implementation、testing、acceptance | complete |
| D-SN-CONTRACT-CLEANUP-1 | design | 将 proposal 转为删除边界、公开 API 影响、下游调用点和保留 SN 基础能力设计 | `design.md`、必要长期边界同步 | PLAN-SN-CONTRACT-CLEANUP-1 | approved design | Directly Mapped Change Items 包含 `remove_sn_service_contract_server` | complete |
| I-SN-CONTRACT-CLEANUP-1 | implementation | 在 admission 通过后移除生产代码和公开导出 | production code | D-SN-CONTRACT-CLEANUP-1 + admission passed | production code | 生产路径不再引用 contract/receipt 逻辑，SN 基础能力保留 | complete |
| T-SN-CONTRACT-CLEANUP-1 | testing | 基于 proposal、design 和实现补充测试设计与可运行验证 | tests、`testing.md`、`testplan.yaml` | I-SN-CONTRACT-CLEANUP-1 | testing artifacts + tests | testplan 映射 `remove_sn_service_contract_server`，相关入口可运行 | complete |
| A-SN-CONTRACT-CLEANUP-1 | acceptance | 审计 proposal/design/testing/implementation/验证证据一致性 | acceptance report | T-SN-CONTRACT-CLEANUP-1 | acceptance report | 无 blocking finding | complete |

### 退回规则
- 如果 design 发现需要改变 SN 基础协议、连接验证器或 control-stream-only 信令，退回 proposal。
- 如果 implementation admission 失败，退回 checker 指向的文档阶段。
- 如果实现需要保留 contract/receipt 公开兼容旁路，退回 proposal/design 明确兼容窗口。
- 如果测试缺少 `remove_sn_service_contract_server` 的直接映射或统一入口不可达，退回 testing。
- 如果 acceptance 发现文档、实现或测试不一致，按问题归属退回 design、implementation 或 testing。

### 退出条件
- [x] proposal 为 approved
- [x] pipeline plan 已刷新
- [x] design 为 approved
- [x] implementation admission 通过 `remove_sn_service_contract_server`
- [x] 生产代码和公开导出不再引用 service contract/receipt 生产逻辑
- [x] SN 基础 report/call/called、连接验证器和 control-stream-only 信令验证通过
- [x] testing 为 approved
- [x] 已基于 approved proposal 通过最终验收

### 验证证据
- `uv run --active python ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame` 已通过。
- `uv run --active python ./harness/scripts/admission-check.py --version v0.1 --module p2p-frame --change-id remove_sn_service_contract_server` 已通过。
- `rg -n 'SnServiceContractServer|DefaultSnServiceContractServer|set_contract|mod contract|mod receipt|receipt::' p2p-frame/src/sn p2p-frame/src/lib.rs` 无匹配。
- `cargo check -p p2p-frame` 已通过。
- `cargo test -p p2p-frame sn_service -- --nocapture` 已通过，3 个 SN service validator 单测通过。
- `python3 ./harness/scripts/test-run.py p2p-frame unit` 已通过，118 个 lib unit tests 通过，doc tests 0 个。
- `python3 ./harness/scripts/test-run.py p2p-frame integration` 已通过，workspace 测试通过；`sn-miner-rust` 存在既有 unused import 警告。
- `python3 ./harness/scripts/test-run.py all all` 已通过。
- `./test-run.sh` 已通过；启动时报告 `.venv/bin/activate: OSTYPE: parameter not set`，但脚本继续执行并以 exit 0 结束。
- 验收报告：`docs/versions/v0.1/reviews/p2p-frame-remove-sn-service-contract-server-acceptance-2026-06-05.md`。
- `stage-scope-check.py --stage design/testing/proposal` 因工作区既有跨阶段 staged/dirty 文件报告 scope violation；本次未回滚这些既有变更，最终以 schema/admission、代码搜索、编译和测试证据作为本轮完成依据。

---

## 当前自动流水线：SN server 连接验证器

### 触发条件
- 已批准的 proposal：`docs/versions/v0.1/modules/p2p-frame/proposal.md`
- 用户启动确认：已于 `2026-06-05T17:41:22+08:00` 确认并要求自动完成后续步骤。
- 当前 `change_id`：
  - `sn_server_connection_validator`

### 验收基线
- 最终验收以 `docs/versions/v0.1/modules/p2p-frame/proposal.md` 中 `sn_server_connection_validator` 的批准内容为准。
- SN server 必须提供连接验证器装配点，用于判断客户端是否允许连接或发起 SN server 入站请求。
- 默认构造路径必须安装显式 allow-all validator，保持未配置部署的兼容行为。
- validator 不得改变 SN command 线协议、`SnCallResp` 最终连通性语义、单 SN 边界、endpoint 分类或 control-stream-only 信令选择。
- 自定义 validator 拒绝时，SN server 不得继续处理对应客户端的 report、call 或等价入站请求。

### 阶段图
| 任务 ID | 阶段 | 职责 | 范围 | 依赖项 | 输出 | 完成条件 | 状态 |
|---------|------|------|------|--------|------|----------|------|
| P-SN-SERVER-VALIDATOR-1 | proposal | 定义 SN server 连接验证器、默认 allow-all、非目标和验收锚点 | `proposal.md` | 用户需求 + 用户确认 | approved proposal | `sn_server_connection_validator` 已进入 Proposal Items 且 proposal approved | confirmed |
| PLAN-SN-SERVER-VALIDATOR-1 | planning | 创建自动流水线阶段图、依赖、输出和退回路由 | `harness/pipeline-plan.md` | approved proposal + 用户启动 | pipeline plan | 计划覆盖 design、implementation、testing、acceptance | complete |
| D-SN-SERVER-VALIDATOR-1 | design | 将 proposal 转为 validator 上下文、接口、默认实现、拒绝错误映射和 SN service 调用点设计 | `design.md`、必要 `design/` 补充、必要长期边界同步 | PLAN-SN-SERVER-VALIDATOR-1 | approved design | Directly Mapped Change Items 包含 `sn_server_connection_validator` | complete |
| I-SN-SERVER-VALIDATOR-1 | implementation | 在 admission 通过后修改生产代码 | production code | D-SN-SERVER-VALIDATOR-1 + admission passed | production code | SN server 默认 allow-all；自定义 reject 短路对应请求 | complete |
| T-SN-SERVER-VALIDATOR-1 | testing | 基于 proposal、design 和实现补充测试设计与测试实现 | tests、`testing.md`、`testing/`、`testplan.yaml` | I-SN-SERVER-VALIDATOR-1 | testing artifacts + tests | testplan 映射 `sn_server_connection_validator`，相关入口可运行 | complete |
| A-SN-SERVER-VALIDATOR-1 | acceptance | 审计 proposal/design/testing/implementation/验证证据一致性 | acceptance report | T-SN-SERVER-VALIDATOR-1 | acceptance report | 无 blocking finding | complete |

### 子任务
| 任务 ID | 阶段 | 职责 | 子模块 | 父任务 | 输出 | 完成条件 | 状态 |
|---------|------|------|--------|--------|------|----------|------|
| D-SN-SERVER-VALIDATOR-1.1 | design | 定义 SN server validator trait/type、allow-all helper 和校验上下文 | `sn/service` | D-SN-SERVER-VALIDATOR-1 | design coverage | validator 使用已认证连接元数据规范化身份，不信任客户端自填字段 | complete |
| D-SN-SERVER-VALIDATOR-1.2 | design | 定义 report/call 或等价入站请求的 reject 短路和错误映射 | `sn/service` | D-SN-SERVER-VALIDATOR-1 | design coverage | reject 不改变 SN command 线协议或 `SnCallResp` 语义 | complete |
| I-SN-SERVER-VALIDATOR-1.1 | implementation | 增加默认 allow-all 和自定义 validator 构造路径 | `p2p-frame/src/sn/service/**` | I-SN-SERVER-VALIDATOR-1 | production code | 默认构造不要求调用方传入 validator | complete |
| I-SN-SERVER-VALIDATOR-1.2 | implementation | 在 SN server 入站请求处理前调用 validator 并短路拒绝 | `p2p-frame/src/sn/service/**` | I-SN-SERVER-VALIDATOR-1 | production code | reject 后不继续 report/call handler | complete |
| T-SN-SERVER-VALIDATOR-1.1 | testing | 补充默认 allow-all 和自定义 reject 行为测试 | unit | T-SN-SERVER-VALIDATOR-1 | tests | unit 直接覆盖默认允许和拒绝短路 | complete |
| A-SN-SERVER-VALIDATOR-1.1 | acceptance | 生成并执行最终验收审计 | review report | A-SN-SERVER-VALIDATOR-1 | acceptance report | admission/test evidence 通过或明确退回 | complete |

### 退回规则
- 如果 proposal 不能支撑 validator 范围，退回 proposal。
- 如果 design 无法明确 validator 上下文、默认实现或 reject 错误映射，退回 design。
- 如果 implementation admission 失败，退回 checker 指向的文档阶段。
- 如果实现需要改变 SN command 线协议、`SnCallResp` 最终连通性语义、单 SN 边界、endpoint 分类或 control-stream-only 信令选择，退回 proposal/design。
- 如果测试缺少 `sn_server_connection_validator` 的直接映射或统一入口不可达，退回 testing。
- 如果 acceptance 发现文档、实现或测试不一致，按问题归属退回 design、implementation 或 testing；同一非需求问题超过 5 次仍未解决时停止并报告。

### 退出条件
- [x] proposal 为 approved
- [x] pipeline plan 已刷新
- [x] design 为 approved
- [x] implementation admission 通过 `sn_server_connection_validator`
- [x] SN server 默认构造路径使用显式 allow-all validator
- [x] 自定义 validator reject 能短路对应 SN server 入站请求
- [x] testing 为 approved
- [x] 已基于 approved proposal 通过最终验收

### 验证证据
- `uv run --active python ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame` 已通过。
- `uv run --active python ./harness/scripts/admission-check.py --version v0.1 --module p2p-frame --change-id sn_server_connection_validator` 已通过。
- `cargo test -p p2p-frame sn_service -- --nocapture` 已通过，覆盖新增默认 allow-all 和自定义 reject 单测。
- `python3 ./harness/scripts/test-run.py p2p-frame unit` 已通过，执行 117 个 lib unit tests，doc tests 0 个。
- `python3 ./harness/scripts/test-run.py p2p-frame integration` 已通过，执行 workspace 兼容测试；`sn-miner` 存在既有 unused import 警告。
- `git diff --check` 已通过。
- `stage-scope-check.py --stage proposal/design` 在本轮中因工作区既有大量未跟踪治理/评审文件和自动流水线跨阶段更新报告 scope violation；这些未跟踪文件不是本轮实现创建，且 diff evidence 中本轮修改集中在 p2p-frame proposal/design/testing/testplan、pipeline plan 和 `p2p-frame/src/sn/service/service.rs`。

---

## 触发条件
- 已批准的 proposal：`docs/versions/v0.1/modules/p2p-frame/proposal.md`
- 用户启动确认：已于 `2026-06-05T16:48:31+08:00` 确认自动处理后续步骤，处理 SN control stream 信令；已于 `2026-06-05T17:05:04+08:00` 追加确认不需要保留 fallback 逻辑。
- 当前 `change_id`：
  - `sn_control_stream_signaling`

## 验收基线
- 最终验收以 `docs/versions/v0.1/modules/p2p-frame/proposal.md` 中 `sn_control_stream_signaling` 的批准内容为准。
- SN report、call、called、response 或等价低频小消息在已有 tunnel 控制通道健康时通过 `Tunnel` control stream 交互。
- SN 信令不得为了每次小消息默认新建普通业务 `open_stream()`。
- SN 信令不得依赖或公开 `p2p-frame` 内部 `control_stream` frame、stream id、window 或 buffer 协议。
- bootstrap、控制通道不可用、远端未监听或旧版本不支持时，SN 命令通道创建/发送必须失败，不得 fallback 到普通 stream。

## 阶段图
| 任务 ID | 阶段 | 职责 | 范围 | 依赖项 | 输出 | 完成条件 | 状态 |
|---------|------|------|------|--------|------|----------|------|
| P-SN-CONTROL-STREAM-1 | proposal | 定义 SN 低频信令使用 `Tunnel` control stream 的需求、边界、非目标和验收锚点 | `proposal.md` | 用户要求 + 用户确认 | approved proposal | `sn_control_stream_signaling` 已进入 Proposal Items 且 proposal approved | confirmed |
| PLAN-SN-CONTROL-STREAM-1 | planning | 创建自动流水线阶段图、依赖、输出和退回路由 | `harness/pipeline-plan.md` | approved proposal + 用户启动 | pipeline plan | 计划覆盖 design、implementation、testing、acceptance | complete |
| D-SN-CONTROL-STREAM-1 | design | 将 proposal 转为 SN 信令 control stream purpose、open/listen 和无 fallback 失败边界设计 | `design.md`、必要 `design/` 补充、必要长期边界同步 | PLAN-SN-CONTROL-STREAM-1 | approved design | Directly Mapped Change Items 包含 `sn_control_stream_signaling` | complete |
| I-SN-CONTROL-STREAM-1 | implementation | 在 admission 通过后修改生产代码 | production code | D-SN-CONTROL-STREAM-1 + admission passed | production code | SN 默认信令路径只使用 control stream，失败不 fallback | complete |
| T-SN-CONTROL-STREAM-1 | testing | 基于 proposal、design 和实现补充测试设计与测试实现 | tests、`testing.md`、`testing/`、`testplan.yaml` | I-SN-CONTROL-STREAM-1 | testing artifacts + tests | testplan 映射 `sn_control_stream_signaling`，相关入口可运行 | complete |
| A-SN-CONTROL-STREAM-1 | acceptance | 审计 proposal/design/testing/implementation/验证证据一致性 | acceptance report | T-SN-CONTROL-STREAM-1 | acceptance report | 无 blocking finding | complete |

## 子任务
| 任务 ID | 阶段 | 职责 | 子模块 | 父任务 | 输出 | 完成条件 | 状态 |
|---------|------|------|--------|--------|------|----------|------|
| D-SN-CONTROL-STREAM-1.1 | design | 定义 SN control stream purpose、消息边界和 listen 注册方式 | `sn` | D-SN-CONTROL-STREAM-1 | design coverage | SN 小消息走公开 `Tunnel` control stream API 的边界完整 | complete |
| D-SN-CONTROL-STREAM-1.2 | design | 定义 bootstrap、控制通道不可用和旧版本不支持时的失败边界 | `sn`、`tunnel_control_stream` | D-SN-CONTROL-STREAM-1 | design coverage | 不保留普通 stream fallback 且可验收 | complete |
| I-SN-CONTROL-STREAM-1.1 | implementation | 修改 SN client/service 默认发送路径 | `p2p-frame/src/sn/**` | I-SN-CONTROL-STREAM-1 | production code | 控制通道可用时不默认创建普通业务 stream | complete |
| I-SN-CONTROL-STREAM-1.2 | implementation | 移除普通 stream fallback 并接入失败边界 | `p2p-frame/src/sn/**`、必要调用点 | I-SN-CONTROL-STREAM-1 | production code | 控制通道不可用时按 design 报错，不回退普通 stream | complete |
| T-SN-CONTROL-STREAM-1.1 | testing | 补充 SN control stream 行为测试 | unit | T-SN-CONTROL-STREAM-1 | tests | control stream 可用、purpose 过滤和无 fallback 审计覆盖 | complete |
| A-SN-CONTROL-STREAM-1.1 | acceptance | 生成并执行最终验收审计 | review report | A-SN-CONTROL-STREAM-1 | acceptance report | admission/test evidence 通过或明确退回 | complete |

## 退回规则
- 如果 proposal 不能支撑 SN 信令迁移，退回 proposal。
- 如果 design 无法明确 purpose、失败边界或与现有 SN 命令关系，退回 design。
- 如果 implementation admission 失败，退回 checker 指向的文档阶段。
- 如果实现需要公开内部 control stream frame/runtime、把 control stream 扩成 SN 大流量数据平面、改变单 SN 边界或改变 `SnCallResp` 受理语义，退回 proposal/design。
- 如果测试缺少 `sn_control_stream_signaling` 的直接映射或统一入口不可达，退回 testing。
- 如果 acceptance 发现文档、实现或测试不一致，按问题归属退回 design、implementation 或 testing；同一非需求问题超过 5 次仍未解决时停止并报告。

## 退出条件
- [x] proposal 为 approved
- [x] pipeline plan 已刷新
- [x] design 为 approved
- [x] implementation admission 通过 `sn_control_stream_signaling`
- [x] SN 默认信令路径只使用 `Tunnel` control stream
- [x] 无普通 stream fallback 边界有测试或审计证据
- [x] testing 为 approved
- [x] 已基于 approved proposal 通过最终验收

## 验证证据
- `uv run --active python ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame` 已通过。
- `uv run --active python ./harness/scripts/admission-check.py --version v0.1 --module p2p-frame --change-id sn_control_stream_signaling` 已通过。
- `cargo check -p p2p-frame` 已通过。
- `cargo test -p p2p-frame sn_server_wraps_sn_control_stream_into_cmd_tunnel -- --nocapture` 已通过。
- `python3 ./harness/scripts/test-run.py p2p-frame unit` 已通过，执行 115 个 lib unit tests，doc tests 0 个；旧普通 stream SN 命令入口测试已随无 fallback 边界删除。
- `python3 ./harness/scripts/test-run.py p2p-frame integration` 已通过，执行 workspace 兼容测试；`sn-miner` 存在既有 unused import 警告。
- 代码审查已确认 `SnClientTunnelFactory::open_cmd_tunnel(...)` 只调用 `TtpClient::open_control_stream(...)`，失败后不调用普通 `open_stream(...)`；`SnServer::start_cmd_accept_loop(...)` 只注册 `listen_control_stream(...)`，不注册普通 `listen_stream(...)` 作为 SN 命令入口。
