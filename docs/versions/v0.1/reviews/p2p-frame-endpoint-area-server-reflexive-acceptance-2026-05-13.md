# 模块验收报告

## 对象与范围
- 模块：p2p-frame
- 版本：v0.1
- 评审日期：2026-05-13
- 范围内：`EndpointArea::Default` 重命名为 `EndpointArea::ServerReflexive`，文本 codec 使用 `S`，raw codec 语义常量改名，删除 `is_sys_default()`，SN 观察到的节点外网地址按“与节点自报地址完全一致则 `Wan`，否则 `ServerReflexive`”分类。
- 范围外：改变 `Lan`、`Wan` 既有语义；改变外部 `bucky_objects` crate 的 endpoint area 枚举；引入 STUN/TURN、多 SN NAT 类型推断或新的公开 endpoint area。

## 输入
- `docs/versions/v0.1/modules/p2p-frame/proposal.md`
- `docs/versions/v0.1/modules/p2p-frame/design.md`
- `docs/versions/v0.1/modules/p2p-frame/design/tunnel-nat-traversal.md`
- `docs/versions/v0.1/modules/p2p-frame/testing.md`
- `docs/versions/v0.1/modules/p2p-frame/testing/tunnel-nat-traversal.md`
- `docs/versions/v0.1/modules/p2p-frame/testplan.yaml`
- `docs/modules/p2p-frame.md`
- `p2p-frame/src/endpoint.rs`
- `p2p-frame/src/sn/service/service.rs`
- `p2p-frame/src/tunnel/tunnel_manager.rs`
- `cyfs-p2p/src/stack_builder.rs`
- 实际验证结果

## 评审顺序
1. 审查已批准 proposal 的基线
2. 审查 design 与边界同步
3. 审查 testing 策略与机器可读测试计划
4. 审查实现与测试代码
5. 审查实际结果
6. 记录一致性与路由结论

## 发现项
| ID | 严重级别 | 阶段 | 证据 | 问题 | 是否阻塞 |
|----|----------|------|------|------|----------|
| F-001 | 低 | Testing | `harness/scripts/test-run.py` 的 p2p-frame DV 入口执行 `cargo run -p cyfs-p2p-test -- all-in-one`；`cyfs-p2p-test/src/main.rs` 的 all-in-one 主体为无限循环，仅在 case 失败时返回 | DV 入口不能自然产生成功退出码；本次只能记录长跑窗口内未观察到失败，不能把手动停止的退出码视为完整 DV 退出码通过 | 否 |

## 一致性摘要
- Proposal 与 design：一致。已批准 proposal 要求 `Default` 改名为 `ServerReflexive`，并将 SN 看到但未与节点自报地址匹配的外网地址分类为 `ServerReflexive`。
- Design 与模块边界文档：一致。长期模块文档已同步 endpoint area 语义，说明 `ServerReflexive` 表示由 SN 观察得到、不能作为节点自证 WAN 地址的外网反射地址。
- Design 与 implementation：一致。实现将 enum 变体、Display/FromStr 和 raw codec 常量语义更新为 `ServerReflexive`/`S`，删除 `is_sys_default()`，且 `is_static_wan()` 不包含 `ServerReflexive`；SN service 在 report/query/call 路径中基于节点自报 endpoint 集合分类观察地址，匹配则 `Wan`，不匹配则 `ServerReflexive`；map-port 派生候选使用 `Mapped`，符合 proposal 对 `Wan` 与 SN 反射地址的区分。`cyfs-p2p` 适配层仅在桥接外部 `bucky_objects` 类型时保留对外部 `Default` 的映射。
- Testing 文档与 testplan：一致。测试计划覆盖 endpoint 文本 codec、raw codec、SN observed address 匹配与不匹配两类分类，以及 workspace integration。
- Testplan 与实际执行：部分一致。`python3 ./harness/scripts/schema-check.py --version v0.1 --module p2p-frame`、`python3 ./harness/scripts/admission-check.py --version v0.1 --module p2p-frame --change-id endpoint_area_server_reflexive` 与 `python3 ./harness/scripts/verify-module-packet.py v0.1 p2p-frame` 均通过。`python3 ./harness/scripts/test-run.py p2p-frame unit` 完整通过，实际执行 `cargo test -p p2p-frame`，95 个测试通过。`python3 ./harness/scripts/test-run.py p2p-frame integration` 完整通过，实际执行 `cargo test --workspace`，workspace 测试通过且 p2p-frame 95 个测试再次通过。`python3 ./harness/scripts/test-run.py p2p-frame dv` 的 all-in-one 场景在 25 轮、300 个 case 全部通过后仍继续运行；因入口不会自然退出，本次人工停止，退出码不能作为 DV 通过退出码。
- 验收标准可追溯性：满足。`EndpointArea` codec 行为由 unit 精确覆盖；SN 观察地址分类由 service 单元测试覆盖；workspace integration 验证跨 crate 编译与现有测试集兼容。

## 结论
- 通过或失败：通过
- 原因：实现与已批准 proposal/design/testing 一致，结构检查与准入检查通过，`cargo test -p p2p-frame` 与 `cargo test --workspace` 均通过。DV 入口自身为既有非终止运行器，本次长跑窗口内未观察到回归失败，相关风险已记录为非阻塞残余风险。

## 退回路由
- Proposal 任务：无需退回
- Design 任务：无需退回
- Testing 任务：无需退回
- Implementation 任务：无需退回

## 残余风险
- p2p-frame DV 入口当前不会自然退出，无法形成无需人工解释的成功退出证据；后续应为 `cyfs-p2p-test all-in-one` 增加轮次或时长上限参数。
- 外部 `bucky_objects::EndpointArea` 仍使用旧 `Default` 名称；当前仅由 `cyfs-p2p` 适配层映射，若未来外部类型支持 `ServerReflexive`，需要同步桥接语义。
