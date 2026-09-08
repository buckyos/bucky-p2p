# Restore hint validity check in usable_prediction_hint

- Status: complete
- Owner module: p2p-frame
- Task manifest: docs/versions/v0.1/modules/p2p-frame/072-restore-prediction-hint-validation/task.yaml
- Approved proposal: docs/versions/v0.1/modules/p2p-frame/072-restore-prediction-hint-validation/proposal.md
- Affected paths: p2p-frame/src/nat_type.rs, p2p-frame/tests/unit/nat_type/tests.rs, p2p-frame/tests/unit/tunnel/nat_connect_plan/tests.rs
- Explicit tier override: none
- Expanded high-risk packet: none

## Approach

071 移除 `NatProfile.observed_endpoint` 时，`usable_prediction_hint()` 的合法性过滤随之丢失，fresh SymmetricLike profile 只要携带 `Some(hint)` 就会被视为可预测。本任务恢复 `is_usable_with()` 校验，并以 hint 自身 `last_observed` 为 base 做一次性自洽检查：

- `sample_count >= 2`
- `port_delta != 0`
- `first/last/base` 同一 IPv4
- `last - first == delta * (sample_count - 1)`
- 端口奇偶关系与 `port_delta` 一致

`select_connect_plan()` 因而对无效 hint 的对称 profile 恢复为 `BoundedBestEffort`/`Base`，不再错误进入 `Predicted`。

## Risk Screen

- Public contract, protocol, or CLI change: no
- Persistent data, schema, or migration change: no
- Security, privacy, or trust-boundary change: no
- Concurrency, lifecycle, or runtime integration change: no
- Material dependency/build graph, supply-chain trust, produced artifact, production default/feature rollout, release/deployment, compatibility, or rollback impact: no
- Material UI, accessibility, localization, or navigation workflow change: no
- Harness rule, checker, or test-infrastructure change: no
- Cross-project or architectural boundary change: no

## Verification

- Targeted check:
  - `cargo test -p p2p-frame --features x509 fresh_symmetric_profile_rejects_invalid_hints --lib`: 1 项通过
  - `cargo test -p p2p-frame --features x509 symmetric_profiles_with_invalid_hints_keep_base_plan --lib`: 1 项通过
  - `cargo test -p p2p-frame --features x509 nat_type::tests --lib`: 4 项通过
  - `cargo test -p p2p-frame --features x509 tunnel::nat_connect_plan::tests --lib`: 6 项通过
- Result: pass
- Residual risk or follow-up: 全量 `cargo test -p p2p-frame --features x509 --lib` 首轮 505/507，失败的两个 TCP 真实 socket 用例单独重跑均通过，属既有并行端口/时序抖动，与本次 `nat_type` 校验修复无因果链。行为收紧是预期：外部/缓存的无自洽 hint 将不再进入 `Predicted`，计划回退到 `Base`。
