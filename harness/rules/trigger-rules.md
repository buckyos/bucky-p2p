# 触发规则

## 目标
- 定义哪些类型的变更需要额外检查。
- 让升级逻辑公开、稳定、可评审。
- 当触发判断存在歧义时，默认按已触发处理。

## 范围
- 适用于整个工作区所有模块；对 Tier 0 和 Tier 1 模块执行更严格的升级规则。
- 适用于 proposal、design、testing、implementation 与 acceptance 中可能影响以下范围的任务：
  - public 或 internal contract
  - 持久数据、schema、迁移或兼容性
  - 安全、隐私、认证、授权或权限
  - runtime、integration、后台任务或分布式流程
  - build、依赖、配置、打包、部署或环境行为
  - 测试基础设施、harness 规则、准入检查或 release gate

## 触发判断规则
- implementation 准入前和 acceptance 前都必须评估触发规则。
- 若改动命中任一触发类别，所属 proposal/design/testing 文档必须记录该触发类别和必需额外检查。
- 如果无法确定某触发是否适用，在所属文档阶段给出明确不适用证据前，按触发处理。
- 触发类别不得仅凭聊天假设标记为不适用；必须基于版本化文档或已检查代码。
- 触发的检查只能在 testing 文档记录 reason、owner、risk 与 acceptance impact 后延期。

## 契约或协议变更
- 触发条件：
  - `p2p-frame/src/networks/**`、`p2p-frame/src/tunnel/**`、`p2p-frame/src/ttp/**`、`p2p-frame/src/sn/**` 或 `p2p-frame/src/pn/**` 下的文件发生变化
  - `p2p-frame/docs/*design*.md`、`*protocol*.md` 或 `*test_cases*.md` 发生变化
- 额外检查：
  - unit
  - DV
  - integration
  - 更新后的 design/testing 数据包评审
- 评审关注点：
  - 线协议兼容性
  - 状态迁移
  - 下游适配层假设

## 通用契约或接口变更
- 触发条件：
  - API、CLI、RPC、event、message、file format、extension point、public function、module boundary 或 cross-module interface 被添加、删除、重命名、重定型、重排序或语义改变
- 额外检查：
  - 兼容性测试或兼容性评审
  - caller/callee 影响评审
  - versioning 或 migration 说明
  - 至少一个边界导向的验证路径
- 评审关注点：
  - 向后兼容性
  - 调用方依赖的未文档化行为
  - 错误语义
  - 幂等性
  - 跨模块准入覆盖

## 持久数据或制品变更
- 触发条件：
  - `desc-tool/**` 中 descriptor 或密钥处理发生变化
  - 生成的 desc/sec/device 制品语义发生变化
- 额外检查：
  - unit
  - 面向操作员的 DV
- 评审关注点：
  - 文件兼容性
  - 签名正确性
  - 意外数据破坏

## 通用数据、Schema 或迁移变更
- 触发条件：
  - 持久数据、数据库 schema、序列化状态、cache key、index、migration、默认值、reset 行为、import/export 形状或数据保留语义发生变化
- 额外检查：
  - migration dry-run 或文档化手工验证
  - rollback 评估
  - 数据兼容性评审
  - backup/recovery 影响说明
- 评审关注点：
  - 不可逆写入
  - 部分迁移失败
  - stale reader
  - downgrade 行为
  - 数据丢失风险

## 运行时或集成变更
- 触发条件：
  - `cyfs-p2p-test/**`、`sn-miner-rust/**`、运行时 feature flag 或启动/配置默认值发生变化
- 额外检查：
  - DV
  - integration
- 评审关注点：
  - 启动行为
  - 场景假设
  - 配置与制品前置条件

## 通用运行时或集成变更
- 触发条件：
  - startup/shutdown、调度、重试、timeout、并发、顺序、网络调用、后台工作、外部服务、资源限制或可观测性发生变化
- 额外检查：
  - failure-mode 测试
  - timeout/retry 评审
  - dependency availability 评审
  - log/metric 评审
  - integration 或 DV 运行；若未运行，必须在 testing 中记录 manual/disabled 理由
- 评审关注点：
  - race condition
  - stuck work
  - duplicate side effects
  - resource leak
  - 运维恢复路径不清晰

## 安全或身份变更
- 触发条件：
  - `p2p-frame/src/tls/**`、`p2p-frame/src/x509*`、`p2p-frame/src/p2p_identity.rs` 或安全敏感的 `desc-tool` 代码发生变化
- 额外检查：
  - unit
  - 定向 design 评审
  - acceptance 评审必须显式点出安全敏感范围
- 评审关注点：
  - 握手正确性
  - 证书校验
  - feature-gated 行为

## 通用安全、隐私或权限变更
- 触发条件：
  - authentication、authorization、identity、secret、token、encryption、transport security、audit log、input trust boundary、PII、tenant isolation、sandboxing 或 privilege check 发生变化
- 额外检查：
  - permission matrix 评审
  - secret handling 评审
  - input validation 评审
  - audit/logging 评审
  - denied access 回归检查
- 评审关注点：
  - fail-open path
  - confused deputy
  - 信息泄露
  - 不安全默认值
  - 日志暴露敏感数据

## 构建、依赖、配置或部署变更
- 触发条件：
  - build script、package metadata、lockfile、dependency version、feature flag、config key/default、environment variable、release packaging、deployment script 或 generated resource 发生变化
- 额外检查：
  - clean build 或等价文档化验证
  - config 兼容性评审
  - dependency risk 评审
  - deployment rollback 说明
- 评审关注点：
  - 环境特定行为
  - 隐式依赖升级
  - 生成文件漂移
  - 改变生产行为的默认值

## Harness、准入或流程规则变更
- 触发条件：
  - `AGENTS.md`、`harness/rules/**`、`harness/process_rules/**`、`harness/checklists/**`、`harness/human-rules/**`、`harness/scripts/**`、模块模板、`testplan.yaml` schema、CI 入口或 acceptance report 格式发生变化
- 额外检查：
  - 运行受影响 checker，或记录无法运行的原因
  - 检查新增或修改规则引用的路径真实存在
  - 验证新规则失败关闭，而不是只添加建议性 prose
  - 检查规则、模板和脚本之间没有互相矛盾
- 评审关注点：
  - 模板与规则矛盾
  - 缺失生成文件
  - 可绕过的措辞
  - 未验证预期条件却返回通过的检查器

## 阶段门禁
- 在 proposal approval 之前：
  - 列出命中的触发类别、受影响 surface、明确非目标和未解决触发问题
- 在 design approval 之前：
  - 将每个触发类别映射到受影响文件、接口、兼容性预期以及 rollback/mitigation 说明
- 在 testing approval 之前：
  - 定义必需额外检查，将每项标记为 automated、manual 或 disabled，并记录每个 manual/disabled 路径的原因
- 在 implementation admission 之前：
  - 每个触发类别都必须对 admitted `change_id` 具备直接 proposal、design、testing 和 `testplan.yaml` 覆盖
- 在 DV 之前：
  - 已记录 unit 命令与运行时前置条件
- 在 integration 之前：
  - 对影响运行时的变更，已声明 DV 路径
- 在 acceptance 之前：
  - 已显式记录触发的额外检查，以及任何有意延后的证据

## 输出要求
每个处理触发变更的 proposal/design/testing/acceptance 制品都必须记录：

| Trigger Category | Applies? | Evidence | Required Checks | Completed Checks | Deferred Checks and Reason | Residual Risk |
|------------------|----------|----------|-----------------|------------------|----------------------------|---------------|
| contract/protocol | yes / no | | | | | |
| data/schema | yes / no | | | | | |
| security/privacy/permission | yes / no | | | | | |
| runtime/integration | yes / no | | | | | |
| build/dependency/config/deployment | yes / no | | | | | |
| harness/process | yes / no | | | | | |

规则：
- `Applies?` 只有在 `Evidence` 说明为什么不适用时才可填 `no`。
- `Deferred Checks and Reason` 必须包含 owner、reason 与 acceptance impact。
- 必需触发检查缺失且没有已批准 deferral 时，acceptance 不得通过。
