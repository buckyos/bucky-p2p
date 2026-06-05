# p2p-frame bounded channel 容量配置验收报告

## 对象与范围
- 模块：`p2p-frame`
- 版本：`v0.1`
- 评审日期：`2026-06-01`
- change_id：`bounded_channel_capacity_config`
- 范围内：顶层 channel 容量默认值与覆盖入口、容量向底层构造路径传递、`p2p-frame/src` 内 unbounded mpsc 替换、测试与验证证据。
- 范围外：TCP/QUIC/PN/TTP 线协议、身份校验、tunnel publish 规则、业务 payload 格式。

## 输入
- `docs/versions/v0.1/modules/p2p-frame/proposal.md`
- `docs/versions/v0.1/modules/p2p-frame/design.md`
- `docs/versions/v0.1/modules/p2p-frame/testing.md`
- `docs/versions/v0.1/modules/p2p-frame/testplan.yaml`
- `harness/pipeline-plan.md`
- `p2p-frame/src/**`
- 实际命令结果

## 发现项
| ID | 严重级别 | 阶段 | 证据 | 问题 | 是否阻塞 |
|----|----------|------|------|------|----------|
| F-001 | none | acceptance | proposal/design/testing/testplan/implementation/search/test evidence | 未发现阻塞问题。`bounded_channel_capacity_config` 已有 approved proposal、approved design、approved testing 与 testplan 映射，生产源码中不再存在 `unbounded_channel` / `UnboundedSender` / `UnboundedReceiver`。 | no |
| F-002 | info | validation | `./test-run.sh all all` | 根入口启动时输出 `.venv/bin/activate: OSTYPE: parameter not set`，但脚本未中断，后续全量验证以 exit 0 完成。 | no |

## 一致性摘要
- Proposal 与 design：一致。proposal 要求容量从外部配置向下传入、顶层默认 `1024`、底层无默认值；design 将该要求映射到 `P2pConfig`、`P2pStackConfig`、`P2pEnv` 和底层构造路径。
- Design 与 implementation：一致。实现新增顶层容量 API，并将容量传递到 `NetManager`、`TunnelManager`、TCP/QUIC listener/tunnel、TTP registry/runtime、PN client/server 等原 unbounded mpsc 使用路径。
- Testing 文档与 testplan：一致。`testing.md` 与 `testplan.yaml` 均包含 `bounded_channel_capacity_config` 的 unit/search/integration 映射。
- Testplan 与实际执行：一致。unit、integration、workspace full 和 root full 入口均已执行通过。
- 验收标准可追溯性：可追溯。默认容量、自定义覆盖、源码无 unbounded mpsc、下游兼容性和满载语义均有对应设计和验证证据。

## 验证证据
- `python3 ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame`：通过。
- `python3 ./harness/scripts/admission-check.py --version v0.1 --module p2p-frame --change-id bounded_channel_capacity_config`：通过。
- `cargo check -p p2p-frame`：通过。
- `cargo test -p p2p-frame`：通过，104 tests。
- `python3 ./harness/scripts/test-run.py p2p-frame unit`：通过，104 `p2p-frame` tests。
- `python3 ./harness/scripts/test-run.py p2p-frame integration`：通过。
- `python3 ./harness/scripts/test-run.py all all`：通过。
- `./test-run.sh all all`：通过。
- `rg -n 'unbounded_channel|UnboundedSender|UnboundedReceiver' p2p-frame/src -g '*.rs'`：无匹配。
- `git diff --check`：通过。

## 结论
- 通过。
- 原因：实现满足 approved proposal 的 bounded channel 容量配置要求，底层不保留局部默认容量或 unbounded mpsc 旁路，测试与全量验证入口通过，未发现 blocking finding。

## 退回路由
- Proposal 任务：无退回。
- Design 任务：无退回。
- Testing 任务：无退回。
- Implementation 任务：无退回。

## 残余风险
- bounded channel 将历史无上限积压转化为满载错误或背压；高压场景的容量选择仍需由调用方按部署规模调参。
- 当前验收覆盖源码搜索与现有测试入口；真实生产流量下的队列饱和分布仍需运行期观测。
