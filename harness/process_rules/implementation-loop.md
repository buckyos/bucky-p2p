# 实现循环

## 目标
- 定义实现准入开放后，默认采用的开发者迭代循环。

## 前置条件
- `python3 ./harness/scripts/check-implementation-admission.py <version> <module>` 校验通过
- 模块数据包已经标明规范验证命令

## 循环步骤
1. 针对失败或变更的验证面运行目标验证命令。
2. 检查日志、运行时产物或编译/测试输出。
3. 仅修改代码或测试代码。
4. 重新构建或重新运行同一个验证入口。
5. 重复以上步骤，直到达到声明的通过条件。

## 工作区特定说明
- 做局部 unit 迭代时，优先使用 `cargo test -p <crate>`。
- 当运行时行为受影响时，对已声明的 DV 场景使用 `cargo run -p cyfs-p2p-test -- all-in-one`。
- 当下游 crate 受影响时，使用 `cargo test --workspace` 做 integration 验证。
- 将 `logs/`、`devices/`、`profile/` 和 `sn/` 视为运行时证据或本地状态，而不是阶段文档的替代品。

## 禁止行为
- 不要在实现任务中更新 proposal/design/testing 文档。
- 在未先更新 testing 制品前，不要切换到未通过规范测试入口路由的临时命令。
