---
module: cyfs-p2p
version: v0.1
status: approved
approved_by: user
approved_at: 2026-05-13
---

# cyfs-p2p 测试

## 测试文档索引
| 文档 | 主题 | 范围 |
|------|------|------|
| `testing.md` | 适配层验证基线 | 完整模块 |

## 统一测试入口
- 机器可读计划：`docs/versions/v0.1/modules/cyfs-p2p/testplan.yaml`
- Unit: `python3 ./harness/scripts/test-run.py cyfs-p2p unit`
- DV: 当前 disabled；`cyfs-p2p-test all-in-one` 不作为 cyfs-p2p 或其他模块的 DV 证据
- Integration: `python3 ./harness/scripts/test-run.py cyfs-p2p integration`

## 子模块测试
| 子模块 | 职责 | 详细测试文档 | 必需行为 | 边界/失败场景 | 测试类型 | 测试文件 |
|--------|------|--------------|----------|----------------|----------|----------|
| `identity_adapter` | CYFS 身份与证书桥接 | none | 身份编码/解码、证书校验、endpoint 更新 | 无效证书/签名、endpoint 不匹配 | unit + integration | `cyfs-p2p/src/stack_builder.rs` |
| `stack_builder` | 构建并配置 stack/env | none | 栈创建、配置转换、`ServerReflexive` endpoint area 适配转换 | 配置不匹配、运行时 feature 假设、把 `ServerReflexive` 误提升为 `Wan` 或恢复旧 `p2p-frame::EndpointArea::Default` 依赖 | unit + integration | `cyfs-p2p/src/stack_builder.rs` |
| `types` | 共享适配辅助逻辑 | none | 辅助逻辑一致性和转换语义 | 序列化/hash 边界场景 | unit | `cyfs-p2p/src/types.rs` |

## 模块级测试
| 测试项 | 覆盖边界 | 入口 | 预期结果 | 测试类型 | 测试文件/脚本 |
|--------|----------|------|----------|----------|-----------------|
| 适配层 crate 测试集 | 适配层本地行为 | `cargo test -p cyfs-p2p` | crate 测试通过 | unit | crate 本地测试 |
| 工作区兼容性 | 下游使用方仍能编译/测试 | `cargo test --workspace` | 工作区测试通过 | integration | workspace |

## 外部接口测试
| 接口 | 职责 | 成功场景 | 失败/边界场景 | 测试类型 | 测试文档/文件 |
|------|------|----------|----------------|----------|----------------|
| `p2p-frame` 边界 | 在无语义漂移的前提下消费核心 API | 栈和身份适配成功 | 核心行为变化破坏适配层假设 | integration | workspace |
| 运行时使用方边界 | 支持 `cyfs-p2p-test` 和 `sn-miner` | 运行时使用方能编译并通过 workspace 兼容性验证 | 启动/配置回归 | integration | `cyfs-p2p-test`、`sn-miner-rust` |

## Direct Change Coverage
| change_id | validation_id | testplan_level | testplan_step_id | Coverage | gap | gap_manual_reason |
|-----------|---------------|----------------|------------------|----------|-----|-------------------|
| cyfs_adapter_traceability | V-1 | unit | cyfs-p2p-unit | 验证 crate 本地适配行为仍可执行 | no | |
| cyfs_runtime_coverage | V-2 | dv |  | 当前无自动 DV 入口；`cyfs-p2p-test all-in-one` 不作为模块 DV 证据。 | yes | 当前没有满足 harness 自动完成语义的 cyfs-p2p DV 入口，运行时使用方兼容性由 integration 承担。 |
| cyfs_runtime_coverage | V-3 | integration | cyfs-p2p-integration | 验证工作区兼容性 | no | |
| endpoint_area_server_reflexive | V-ENDPOINT-AREA-ADAPTER-UNIT | unit | cyfs-p2p-unit | 覆盖 `cyfs-p2p/src/stack_builder.rs` 能消费 `p2p-frame::EndpointArea::ServerReflexive`，并保持到当前 CYFS `EndpointArea::Default` 的兼容映射。 | no | |
| endpoint_area_server_reflexive | V-ENDPOINT-AREA-ADAPTER-INTEGRATION | integration | cyfs-p2p-integration | 运行 workspace 测试，确认 `ServerReflexive` 公开 enum 变更不会破坏适配层和下游使用方编译测试边界。 | no | |

## 回归关注点
- `stack_builder.rs` 中的身份与证书适配
- `lib.rs` 中的公开再导出行为
- 混合 CYFS/核心 endpoint 处理

## 完成定义
- [ ] 直接子模块已映射到验证面
- [ ] `testplan.yaml` 与声明的命令一致
- [ ] 影响运行时的适配层改动具备下游证据
- [ ] 已检查跨模块兼容性
