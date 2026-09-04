# cyfs-p2p-test 测试与验收禁用规则

## 目标
- 保证正式测试证据来自被测模块自身的专用测试面，而不是运行时场景驱动器。
- 保证验收独立审计可复现的测试实现与结果，不把 `cyfs-p2p-test` 的运行输出当作完成证明。

## 适用范围
- 所有模块的 high-risk testing 与 acceptance 阶段。
- manual 与 auto-pipeline 模式。
- 即使当前任务的目标模块是 `cyfs-p2p-test`，本规则仍然适用。

## Testing 规则
- Testing 任务 MUST NOT 新增、修改或删除 `cyfs-p2p-test/**` 下的文件。
- Testing 任务 MUST NOT 把 `cyfs-p2p-test` crate、binary、命令、场景、配置或运行时制品注册为测试实现、测试步骤、runner、fixture、成功标准或证据来源。
- `testing.md`、`testing/`、`testplan.yaml`、统一测试入口和测试运行制品 MUST NOT 以 `cargo test -p cyfs-p2p-test`、`cargo run -p cyfs-p2p-test`、`cyfs-p2p-test` binary 或其派生制品满足任何 `change_id` 的覆盖要求。
- 所需覆盖 MUST 放在被测生产模块的专用测试文件、测试目录或其他专用 test-only crate/package 中，并通过 `harness/scripts/test-run.py <module>/<task-name> all` 到达；该 test-only crate/package MUST NOT 是 `cyfs-p2p-test`。
- 如果现有 testing 计划只能通过 `cyfs-p2p-test` 验证，testing 不得完成，必须改写验证方案或记录为阻塞缺口。

## Acceptance 规则
- Acceptance 任务 MUST NOT 执行 `cyfs-p2p-test` crate、binary、命令或场景，也 MUST NOT 修改 `cyfs-p2p-test/**`。
- Acceptance MUST NOT 把当前或历史 `cyfs-p2p-test` 输出、日志、截图、配置、生成文件或退出状态作为通过证据。
- Acceptance 必须检查 testing 计划、测试实现、统一入口与运行制品是否绕过本规则。任何 `change_id` 仅由 `cyfs-p2p-test` 覆盖时，必须记录 blocking testing finding，并将结论设为 `needs changes`；不得得出 accepted 结论。

## 边界与优先级
- 本规则不禁止 implementation 阶段维护 `cyfs-p2p-test` 的产品代码，也不禁止阶段外由人类执行临时运行时诊断；这些诊断结果不得转化为 testing 或 acceptance 证据。
- 本规则在 testing 与 acceptance 阶段覆盖长期模块文档中要求通过 `cyfs-p2p-test` 提供 integration 证据的旧约定，也覆盖 `module-doc-exception-rules.md` 对该模块的 legacy 文档豁免。
- 本规则不放松 proposal、design、implementation、统一测试入口、测试覆盖或验收审计的其他要求。
