# p2p-frame stack channel capacity config removal 验收报告

## 发现项
| ID | 严重级别 | 阶段 | 证据 | 问题 | 是否阻塞 |
|----|----------|------|------|------|----------|
| F-001 | high | validation | `python3 ./harness/scripts/test-run.py p2p-frame unit` | 统一 unit 入口未通过，测试编译失败于既有 `Executor::init()` / `Executor::init_new_multi_thread(...)` 调用缺失，位置包括 `stream_manager.rs:655`、`tunnel_manager.rs:1879`、`datagram_manager.rs:587`、`networks/tcp/listener.rs:419`、`networks/tcp/network.rs:329`、`pn/client/pn_client.rs:875`、`pn/service/pn_server.rs:1276`、`sn/service/service.rs:1171`、`ttp/tests.rs:426`。该失败不来自本次容量配置删除，但阻塞最终验收通过。 | yes |
| F-002 | none | implementation | `p2p-frame/src/stack.rs`、`p2p-frame/src/networks/net_manager.rs`、`p2p-frame/src/networks/quic/network.rs`、`p2p-frame/src/ttp/**`、`p2p-frame/src/pn/client/pn_client.rs`、源码搜索、`cargo check -p p2p-frame` | 未发现 `stack_channel_capacity_config_removal` 的实现不一致：`ChannelCapacityConfig` 与 stack 层容量 getter/setter 已删除，`NetManager` 不再保存或暴露容量，`QuicTunnelNetwork::new(...)` 和 `TtpRuntime::new()` 不再接收容量参数，`TtpClient` / `TtpServer` 不再为 TTP runtime 读取 `NetManager` 容量，`PnClient` 显式容量构造入口已删除，生产源码无 unbounded mpsc。 | no |
| F-003 | info | scope | `stage-scope-check.py --stage implementation` | implementation 单阶段范围检查因本次跨阶段同步文档以及工作树中既有未跟踪/已修改治理文档而失败。该任务由用户确认自动处理后续阶段，故该结果记录为范围证据而非单独实现缺陷。 | no |

## 对象与范围
- 模块：`p2p-frame`
- 版本：`v0.1`
- 评审日期：`2026-06-03`
- change_id：`stack_channel_capacity_config_removal`
- 范围内：删除 `p2p-frame/src/stack.rs` 中 `ChannelCapacityConfig`、`P2pEnv` 容量快照、`P2pConfig` / `P2pStackConfig` 容量 getter/setter 和继承逻辑；删除 `NetManager` 容量字段、构造参数和 getter；删除 `QuicTunnelNetwork::new(...)`、`TtpRuntime::new(...)` 的无效容量参数；删除 `TtpClient` / `TtpServer` 为创建 TTP runtime 读取 `NetManager` 容量的逻辑；删除 `PnClient` 显式容量构造入口；保留 bounded channel 默认容量 `1024`。
- 范围外：TCP/QUIC/PN/TTP 线协议、身份校验、tunnel publish 规则、业务 payload 格式、底层 bounded sender/receiver 类型和满载错误语义。

## 输入
- `docs/versions/v0.1/modules/p2p-frame/proposal.md`
- `docs/versions/v0.1/modules/p2p-frame/design.md`
- `docs/versions/v0.1/modules/p2p-frame/testing.md`
- `docs/versions/v0.1/modules/p2p-frame/testplan.yaml`
- `docs/modules/p2p-frame.md`
- `p2p-frame/src/stack.rs`
- `p2p-frame/src/networks/net_manager.rs`
- `p2p-frame/src/networks/quic/network.rs`
- `p2p-frame/src/ttp/runtime.rs`
- `p2p-frame/src/ttp/client.rs`
- `p2p-frame/src/ttp/server.rs`
- `p2p-frame/src/pn/client/pn_client.rs`
- 实际命令结果

## 一致性摘要
- Proposal 与 design：一致。两者都将本次清理限定为删除 stack/TTP 层容量配置 API，并要求保留 bounded channel 与默认容量 `1024`。
- Design 与 implementation：一致。实现删除 `ChannelCapacityConfig`、env/config 字段、getter/setter、继承逻辑和对应旧测试断言；`NetManager` 不再保存或暴露容量，`QuicTunnelNetwork::new(...)` 和 `TtpRuntime::new()` 改为无容量参数，`TtpClient` / `TtpServer` 不再为 TTP runtime 读取 `NetManager` 容量，PN 默认构造不再依赖 `TtpClient::channel_capacity()`，`PnClient::new_with_channel_capacity(...)` 和 `PnClient::new_with_tls_material_and_channel_capacity(...)` 已删除。
- Testing 文档与 testplan：一致。`testing.md` 和 `testplan.yaml` 均包含 `stack_channel_capacity_config_removal` 的 unit/search/integration 映射。
- Testplan 与实际执行：不完整。schema、admission、搜索和 `cargo check -p p2p-frame` 通过；统一 unit 入口失败于既有 executor test helper API 缺失。
- 验收标准可追溯性：可追溯，但因 required unit evidence 未通过，最终结论不能标记为通过。

## 验证证据
- `uv run --active python ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame`：通过。
- `uv run --active python ./harness/scripts/admission-check.py --version v0.1 --module p2p-frame --change-id stack_channel_capacity_config_removal`：通过。
- `rg -n 'new_with_channel_capacity|new_with_tls_material_and_channel_capacity|pn_channel_capacity|ChannelCapacityConfig|channel_capacities|set_channel_capacity|set_ttp_channel_capacity|set_tunnel_manager_channel_capacity|set_pn_channel_capacity|TtpRuntime::new\([^)]|QuicTunnelNetwork::new\([^)]*DEFAULT_CHANNEL_CAPACITY|pub fn channel_capacity|\.channel_capacity\(\)|channel_capacity:' p2p-frame/src/stack.rs p2p-frame/src/networks/net_manager.rs p2p-frame/src/networks/quic/network.rs p2p-frame/src/ttp p2p-frame/src/pn/client/pn_client.rs -S`：无旧 stack/NetManager/QUIC/TTP/PN 容量配置匹配。
- `rg -n 'unbounded_channel|UnboundedSender|UnboundedReceiver' p2p-frame/src -g '*.rs'`：无匹配。
- `rustup run stable cargo check -p p2p-frame`：通过。
- `python3 ./harness/scripts/test-run.py p2p-frame unit`：失败，详见 F-001。
- `cargo fmt --check`：失败于既有未格式化改动文件 `datagram_manager.rs`、`networks/quic/listener.rs`、`pn/client/pn_tunnel.rs`；本次只对相关改动文件执行 `rustfmt`。
- `uv run --active python ./harness/scripts/stage-scope-check.py --stage implementation`：失败，详见 F-003。

## 结论
- 失败，需补齐验证阻塞项后重新验收。
- 原因：实现与 approved proposal/design/testing 对 `stack_channel_capacity_config_removal` 的要求一致，且编译与搜索证据通过；但 required unit 入口未通过，acceptance 规则不允许在缺失必需测试证据时标记通过。

## 退回路由
- Proposal 任务：无退回。
- Design 任务：无退回。
- Testing 任务：当前 required unit 入口失败，需要先修复或明确记录 executor test helper API 缺失的测试环境/测试代码问题。
- Implementation 任务：无本次容量清理相关退回。

## 残余风险
- 删除公开 stack 容量覆盖 API 可能影响下游源码中尚未编译到的直接调用点；当前 `cargo check -p p2p-frame` 不能替代完整 workspace integration。
- unit 入口失败导致本次无法确认全部 p2p-frame test target 在删除容量 API 后仍可运行。
