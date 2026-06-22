---
module: sn-miner
version: v0.1
status: approved
approved_by: auto-pipeline
approved_at: 2026-06-22T22:28:30+08:00
approved_content_sha256: 897bf88a8bfc3bd6c9f3a5f9bfd4b3162157093ed528208f9dd454add954f68b
---

# sn-miner 测试

## Test Document Index
| 文档 | 主题 | 范围 |
|------|------|------|
| `testing.md` | 配置驱动 owner/serving 启动验证 | 完整模块 |
| `testplan.yaml` | 机器可读测试入口 | 完整模块 |

## Unified Test Entry
- 机器可读计划：`docs/versions/v0.1/modules/sn-miner/testplan.yaml`
- Unit: `python3 ./harness/scripts/test-run.py sn-miner unit`
- DV: `python3 ./harness/scripts/test-run.py sn-miner dv`
- Integration: `python3 ./harness/scripts/test-run.py sn-miner integration`

## Submodule Tests
| 子模块 | 职责 | 详细测试文档 | 必需行为 | 边界/失败场景 | 测试类型 | 测试文件 | 状态 | Gap / Manual Reason |
|--------|------|--------------|----------|----------------|----------|----------|------|---------------------|
| `config_loader` | 读取 key/value 配置、校验 role 和 section | this file | `role=owner` / `role=serving` 被解析，`--config` 成为主入口 | mixed owner/serving section、缺失 role、无效 endpoint 失败 | unit + DV | `sn-miner-rust/tests/real_process.rs` | ready | |
| `artifact_loading` | 加载或创建 desc/sec | this file | 真实进程用临时 desc path 启动并创建制品 | 制品路径不可读时进程失败 | DV | `sn-miner-rust/tests/real_process.rs` | ready | |
| `role_startup` | 根据 role 只启动一个运行角色 | this file | owner 进程保持运行；serving 进程保持运行 | mixed config 非零退出；测试结束 kill 子进程 | DV + integration | `sn-miner-rust/tests/real_process.rs` | ready | |
| `process_test_harness` | 管理真实进程生命周期 | this file | 子进程启动、观察、超时清理 | 子进程提前退出即测试失败 | DV + integration | `sn-miner-rust/tests/real_process.rs` | ready | |

## Module-Level Tests
| 测试项 | 覆盖边界 | 入口 | 预期结果 | 测试类型 | 测试文件/脚本 | 状态 | Gap / Manual Reason |
|--------|----------|------|----------|----------|-----------------|------|---------------------|
| sn-miner crate 测试 | 配置解析和真实进程用例 | `cargo test -p sn-miner` | 3 个真实进程测试通过 | unit | `sn-miner-rust/tests/real_process.rs` | ready | |
| CLI help DV | 二进制入口可调用 | `cargo run -p sn-miner -- --help` | 帮助命令成功退出 | DV | binary | ready | |
| sn-miner real process DV | owner/serving 独立进程启动 | `cargo test -p sn-miner --test real_process -- --test-threads=1` | invalid config 失败；owner 进程保持运行；serving 进程保持运行 | DV | `sn-miner-rust/tests/real_process.rs` | ready | |
| sn-miner integration | 同上作为集成入口 | `cargo test -p sn-miner --test real_process -- --test-threads=1` | 真实进程行为通过统一入口可重复执行 | integration | `sn-miner-rust/tests/real_process.rs` | ready | |

## External Interface Tests
| 接口 | 职责 | 成功场景 | 失败/边界场景 | 测试类型 | 测试文档/文件 | 状态 | Gap / Manual Reason |
|------|------|----------|----------------|----------|----------------|------|---------------------|
| `--config <path>` | 定位配置文件 | owner/serving 配置被加载 | mixed config 非零退出 | DV + integration | `real_process.rs` | ready | |
| owner role config | 启动 OwnerDirectoryServer 双 listener | owner 进程持续运行直到测试 kill | 缺少 owner_peer_endpoints 或 mixed section 失败 | DV | `real_process.rs` | ready | |
| serving role config | 启动 SnService/PnServer 并消费 owner serving endpoint 配置 | serving 进程持续运行直到测试 kill | mixed section 失败；缺少 owner endpoint 失败 | DV | `real_process.rs` | ready | |
| legacy no-config path | 保持 debug serving 路径 | help 和 crate 测试可运行 | legacy owner flags 不允许混合启动 | unit + DV | `main.rs`, `real_process.rs` | ready | |

## Direct Change Coverage
| change_id | design_source | validation_id | testplan_level | testplan_step_id | Coverage | gap | gap_manual_reason |
|-----------|---------------|---------------|----------------|------------------|----------|-----|-------------------|
| sn_miner_startup_audit | `design.md` Directly Mapped Change Items and Key Call Flows | V-SN-MINER-UNIT | unit | sn-miner-unit | crate 测试覆盖启动入口、配置入口和真实进程 harness；DV help 入口另由 testplan 执行 | no | |
| sn_miner_artifact_defaults | `design.md` Data and State | V-SN-MINER-REAL-PROC | integration | sn-miner-integration | 临时 desc path 真实进程启动会触发 desc/sec 创建路径 | no | |
| sn_miner_config_file_roles | `design.md` Config load flow | V-SN-MINER-REAL-PROC | dv | sn-miner-real-process-dv | `role=owner`、`role=serving`、mixed config 都由真实进程覆盖 | no | |
| sn_miner_owner_role_startup | `design.md` Owner role startup flow | V-SN-MINER-OWNER-PROC | dv | sn-miner-real-process-dv | owner config 进程保持运行，证明 owner-only 启动路径可达 | no | |
| sn_miner_serving_role_startup | `design.md` Serving role startup flow | V-SN-MINER-SERVING-PROC | dv | sn-miner-real-process-dv | serving config 进程保持运行，证明 serving-only 启动路径可达 | no | |
| sn_miner_role_exclusive_validation | `design.md` role selection state | V-SN-MINER-MIXED-REJECT | dv | sn-miner-real-process-dv | mixed owner/serving config 非零退出并输出 ERROR | no | |

## Case-Type Coverage
| change_id | case_type | required | validation_id | level | status | gap_manual_reason |
|-----------|-----------|----------|---------------|-------|--------|-------------------|
| sn_miner_config_file_roles | normal | yes | V-SN-MINER-REAL-PROC | dv | covered | |
| sn_miner_config_file_roles | boundary | yes | V-SN-MINER-MIXED-REJECT | dv | covered | |
| sn_miner_config_file_roles | negative | yes | V-SN-MINER-MIXED-REJECT | dv | covered | |
| sn_miner_config_file_roles | error | yes | V-SN-MINER-MIXED-REJECT | dv | covered | |
| sn_miner_config_file_roles | compatibility | yes | V-SN-MINER-DV | dv | covered | |
| sn_miner_config_file_roles | lifecycle | yes | V-SN-MINER-REAL-PROC | dv | covered | |
| sn_miner_config_file_roles | cross-module | yes | V-SN-MINER-REAL-PROC | integration | covered | |
| sn_miner_owner_role_startup | lifecycle | yes | V-SN-MINER-OWNER-PROC | dv | covered | |
| sn_miner_owner_role_startup | normal | yes | V-SN-MINER-OWNER-PROC | dv | covered | |
| sn_miner_owner_role_startup | boundary | yes | V-SN-MINER-OWNER-PROC | dv | covered | |
| sn_miner_owner_role_startup | negative | yes | V-SN-MINER-MIXED-REJECT | dv | covered | |
| sn_miner_owner_role_startup | error | yes | V-SN-MINER-MIXED-REJECT | dv | covered | |
| sn_miner_owner_role_startup | compatibility | yes | V-SN-MINER-DV | dv | covered | |
| sn_miner_owner_role_startup | cross-module | yes | V-SN-MINER-OWNER-PROC | dv | covered | |
| sn_miner_serving_role_startup | lifecycle | yes | V-SN-MINER-SERVING-PROC | dv | covered | |
| sn_miner_serving_role_startup | normal | yes | V-SN-MINER-SERVING-PROC | dv | covered | |
| sn_miner_serving_role_startup | boundary | yes | V-SN-MINER-SERVING-PROC | dv | covered | |
| sn_miner_serving_role_startup | negative | yes | V-SN-MINER-MIXED-REJECT | dv | covered | |
| sn_miner_serving_role_startup | error | yes | V-SN-MINER-MIXED-REJECT | dv | covered | |
| sn_miner_serving_role_startup | compatibility | yes | V-SN-MINER-DV | dv | covered | |
| sn_miner_serving_role_startup | cross-module | yes | V-SN-MINER-SERVING-PROC | dv | covered | |
| sn_miner_role_exclusive_validation | negative | yes | V-SN-MINER-MIXED-REJECT | dv | covered | |
| sn_miner_role_exclusive_validation | normal | yes | V-SN-MINER-REAL-PROC | dv | covered | |
| sn_miner_role_exclusive_validation | boundary | yes | V-SN-MINER-MIXED-REJECT | dv | covered | |
| sn_miner_role_exclusive_validation | error | yes | V-SN-MINER-MIXED-REJECT | dv | covered | |
| sn_miner_role_exclusive_validation | compatibility | yes | V-SN-MINER-DV | dv | covered | |
| sn_miner_role_exclusive_validation | lifecycle | yes | V-SN-MINER-REAL-PROC | dv | covered | |
| sn_miner_role_exclusive_validation | cross-module | yes | V-SN-MINER-REAL-PROC | integration | covered | |
| sn_miner_artifact_defaults | compatibility | yes | V-SN-MINER-REAL-PROC | integration | covered | |
| sn_miner_artifact_defaults | normal | yes | V-SN-MINER-REAL-PROC | integration | covered | |
| sn_miner_artifact_defaults | boundary | yes | V-SN-MINER-REAL-PROC | integration | covered | |
| sn_miner_artifact_defaults | negative | yes | V-SN-MINER-MIXED-REJECT | dv | covered | |
| sn_miner_artifact_defaults | error | yes | V-SN-MINER-MIXED-REJECT | dv | covered | |
| sn_miner_artifact_defaults | lifecycle | yes | V-SN-MINER-REAL-PROC | integration | covered | |
| sn_miner_artifact_defaults | cross-module | yes | V-SN-MINER-REAL-PROC | integration | covered | |
| sn_miner_startup_audit | normal | yes | V-SN-MINER-UNIT | unit | covered | |
| sn_miner_startup_audit | boundary | yes | V-SN-MINER-DV | dv | covered | |
| sn_miner_startup_audit | negative | yes | V-SN-MINER-MIXED-REJECT | dv | covered | |
| sn_miner_startup_audit | error | yes | V-SN-MINER-MIXED-REJECT | dv | covered | |
| sn_miner_startup_audit | compatibility | yes | V-SN-MINER-DV | dv | covered | |
| sn_miner_startup_audit | lifecycle | yes | V-SN-MINER-REAL-PROC | dv | covered | |
| sn_miner_startup_audit | cross-module | yes | V-SN-MINER-REAL-PROC | integration | covered | |

## Design Element Coverage
| element_type | design_source | derived_cases | level | status | gap_manual_reason |
|--------------|---------------|---------------|-------|--------|-------------------|
| parameter-domain | `SnMinerRole` | owner, serving, invalid mixed config | dv | covered | |
| state-transition | process lifecycle | not started -> running -> killed by test cleanup | dv | covered | |
| failure-path | config validation | mixed role sections rejected before startup | dv | covered | |
| error-handling | startup error reporting | invalid config exits non-zero and prints `ERROR:` | dv | covered | |
| concurrency | real process lifecycle | tests spawn one child per case and clean it up before completion | dv | covered | |
| invariant | one process starts one role | owner and serving are separate process tests | dv | covered | |

## Validation Rationale
| Behavior or Risk | Validation Signal | Why This Is Sufficient | Gap / Manual Reason |
|------------------|-------------------|------------------------|---------------------|
| config must reject mixed owner/serving role | real process exits non-zero on mixed config | proves validation happens before long-running startup | |
| owner role must not require serving service in the same process | owner process stays running with only owner endpoints | proves owner startup branch can run independently | deeper protocol behavior remains p2p-frame responsibility |
| serving role must not start OwnerDirectoryServer in same process | serving process stays running with owner serving endpoint config | proves serving startup branch can run independently | actual owner/serving command exchange is covered by p2p-frame tests and future DV expansion |

## Unit Tests
| Function or Unit | Branch or Condition | Covered Behavior | Test File | Status | Gap / Manual Reason |
|------------------|---------------------|------------------|-----------|--------|---------------------|
| `SnMinerConfig::load` through binary config path | `role=owner` | owner config is accepted and owner process stays running | `sn-miner-rust/tests/real_process.rs` | covered | |
| `SnMinerConfig::load` through binary config path | `role=serving` | serving config is accepted and serving process stays running | `sn-miner-rust/tests/real_process.rs` | covered | |
| `SnMinerConfig::load` validation | mixed owner/serving keys | process exits non-zero before long-running startup | `sn-miner-rust/tests/real_process.rs` | covered | |
| desc/sec artifact loading | temporary desc path missing | process creates/loads artifacts and stays running | `sn-miner-rust/tests/real_process.rs` | covered | |
| process lifecycle harness | child starts and is killed by test cleanup | real process startup is observable and cleaned up | `sn-miner-rust/tests/real_process.rs` | covered | |

## DV Tests
| Workflow | Kind | Entry | Expected Result | Test File or Script | Status | Gap / Manual Reason |
|----------|------|-------|-----------------|---------------------|--------|---------------------|
| CLI help | main | `cargo run -p sn-miner -- --help` | help exits successfully | binary | covered | |
| real process owner/serving roles | lifecycle | `cargo test -p sn-miner --test real_process -- --test-threads=1` | invalid config fails; owner and serving processes start and are cleaned up | `sn-miner-rust/tests/real_process.rs` | covered | |
| mixed config rejection | failure | `cargo test -p sn-miner --test real_process sn_miner_rejects_mixed_owner_and_serving_config -- --test-threads=1` | mixed owner/serving config exits non-zero | `sn-miner-rust/tests/real_process.rs` | covered | |

## Integration Tests
| Contract or Flow | Modules Involved | Success Case | Failure Case | Test File | Status | Gap / Manual Reason |
|------------------|------------------|--------------|--------------|-----------|--------|---------------------|
| sn-miner config role startup | `sn-miner`, `cyfs-p2p`, `p2p-frame` | owner and serving process configs start independently | mixed config exits non-zero | `sn-miner-rust/tests/real_process.rs` | covered | |

## Regression Focus
- 配置文件 schema 与 role 互斥
- desc/sec 默认创建
- owner/serving 单角色启动
- 子进程超时清理

## Definition of Done
- [x] 启动和制品假设明确
- [x] 配置/默认值改动具备测试入口
- [x] 运行时行为具备真实进程 DV 证据
