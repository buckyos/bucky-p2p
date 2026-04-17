# 工作区原则

## 目标
- 在保留现有多 crate P2P 架构的前提下，让工作区对 agent 和审阅者都保持清晰可读。

## 仓库事实
- 工作区根目录定义了五个成员：`p2p-frame`、`cyfs-p2p`、`cyfs-p2p-test`、`sn-miner-rust` 和 `desc-tool`。
- `p2p-frame` 是架构中心。所有严格验证规则都假设这里的改动会产生全系统级影响面。
- `p2p-frame/docs/` 下现有的协议和设计说明仍然是有效参考材料，并属于设计证据链的一部分。

## 不可协商的约束
- 仓库是唯一事实来源。工作流知识必须保存在版本化文件中，而不能只存在于聊天记录里。
- 阶段责任必须机械化划分：
  - proposal 负责意图
  - design 负责方案形态
  - testing 负责验证设计
  - implementation 负责代码与测试
  - acceptance 负责审计结果
- 除非版本化规则发布了更窄的例外，否则始终执行硬性的 implementation admission。
- 验收遵循 proposal-first。已批准的 proposal 是最终成功基线。

## 架构倾向
- 优先保持稳定的 crate 边界，而不是做横切式重写。
- 优先围绕当前工作区做增量式补强，而不是把交付模型打平重来。
- 每个验证层级优先只有一个规范入口，而不是为每个任务临时拼命令。
- 优先使用显式模块文档、触发规则和评审清单，而不是依赖隐性的口口相传知识。

## 高风险面
- `p2p-frame/src/networks/**`、`p2p-frame/src/ttp/**`、`p2p-frame/src/tunnel/**`、`p2p-frame/src/sn/**` 和 `p2p-frame/src/pn/**` 中的传输协议与线协议兼容性。
- `p2p-frame/src/tls/**`、`p2p-frame/src/x509*` 以及 `desc-tool` 中的密码学、TLS 与身份边界。
- `cyfs-p2p-test`、`sn-miner-rust` 以及生成的 desc/sec/device 制品中由 CLI/配置驱动的运行时行为。

## 必需证据链
- `proposal.md`
- `design.md` 以及任何 `design/` 补充说明
- `testing.md` 以及任何 `testing/` 补充说明
- `testplan.yaml`
- `acceptance.md`
- `docs/modules/` 下的长期模块文档
- 实现代码与测试
- 实际测试结果

## 改造策略
- 不要把现有协议说明或源码布局重写成 harness 启动工作的一部分。
- 先在代码库外围补治理，再谈别的。
- 先把 `p2p-frame` 变成第一个完整的版本化数据包，因为它风险最高、依赖最多。
