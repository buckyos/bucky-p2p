# Implementation Admission: Server Runtime Ownership Fix

## Implementation Admission Evidence
| evidence_item | source | status | notes |
|---------------|--------|--------|-------|
| proposal_read | `docs/versions/v0.1/modules/p2p-frame/server-runtime-ownership-fix/proposal.md` | pass | Approved sibling proposal read; it requires one process-owned runtime, caller-supplied CYFS configuration, and role-specific readiness evidence without protocol changes. |
| design_read | `docs/versions/v0.1/modules/p2p-frame/server-runtime-ownership-fix/design.md` | pass | Approved design read; it defines the helper signature, exact combined startup flow, readiness emission ordering, migration paths, and concrete Scope Paths. |
| change_scope_matches_request | proposal/design rows for `process_runtime_single_owner`, `cyfs_config_runtime_injection`, and `sn_miner_role_readiness_evidence` | pass | These implementation change ids cover the production changes requested in issues 2, 3, and the startup observability needed by issue 5; documentation alignment and feature/test implementation remain in their owning stages. |
| active_module_resolved | `module: p2p-frame`, `submodule: server-runtime-ownership-fix`, `version: v0.1` | pass | The sibling packet owns the cross-module runtime call flow; its design records a merged implementation reason and exact production paths. |
| no_chat_only_evidence | approved proposal/design rows quoted below | pass | The user request triggered the task, but implementation coverage and scope come from the approved versioned sibling packet. |

## Document Binding
| doc | sha256 |
|-----|--------|
| docs/versions/v0.1/modules/p2p-frame/server-runtime-ownership-fix/proposal.md | 341f095f7678a62014baebdbe4e898344878b759c225bf844131964122901cc7 |
| docs/versions/v0.1/modules/p2p-frame/server-runtime-ownership-fix/design.md | b4e72a040dd6486b49b0c078ce8a2cd01def0cc76d980c7388679e4e1d5e7ca3 |

## Coverage Quotes

### Quote: proposal.md process_runtime_single_owner
> | P-RUNTIME-FIX-2 | process_runtime_single_owner | Combined startup creates one `ServerRuntime` and clones it to SN and stack consumers. | `cyfs-p2p-test` assembly and required runtime-plumbing API only. | regression evidence demonstrates a single creation point in the combined path and successful SN/stack startup using clones of the same runtime. | Do not introduce a singleton or change worker scheduling. |

### Quote: design.md process_runtime_single_owner
> | process_runtime_single_owner | P-RUNTIME-FIX-2 | Combined startup key flow and runtime state ownership | `cyfs-p2p-test/src/main.rs`, `p2p-frame/src/sn/tests.rs` | internal assembly behavior; no public protocol impact | One start call, clones to consumers. |

### Quote: proposal.md cyfs_config_runtime_injection
> | P-RUNTIME-FIX-3 | cyfs_config_runtime_injection | CYFS P2P config construction receives a caller-owned runtime and never starts or panics on a hidden runtime. | `cyfs-p2p` config helper and its workspace callers. | source/API checks show no `ServerRuntime::start` in the helper; compile/integration covers migrated callers; runtime startup errors remain caller-handleable. | Do not add a default-runtime compatibility overload that hides ownership. |

### Quote: design.md cyfs_config_runtime_injection
> | cyfs_config_runtime_injection | P-RUNTIME-FIX-3 | CYFS helper interface and caller migration | `cyfs-p2p/src/stack_builder.rs`, `cyfs-p2p-test/src/main.rs`, `cyfs-p2p/examples/cyfs_perf.rs` | migration-required public Rust helper signature | No hidden compatibility overload. |

### Quote: proposal.md sn_miner_role_readiness_evidence
> | P-RUNTIME-FIX-5 | sn_miner_role_readiness_evidence | Real-process owner and serving tests wait for deterministic role readiness, diagnose early exit/timeout, and always clean up. | `sn-miner-rust` startup observability and integration tests; no distributed-protocol expansion. | owner readiness proves both required owner listener surfaces are started/reachable; serving readiness proves SN service startup/listener readiness; timeout and early-exit paths fail with retained output; children are reaped. | Do not use a fixed sleep or liveness alone as readiness. |

### Quote: design.md sn_miner_role_readiness_evidence
> | sn_miner_role_readiness_evidence | P-RUNTIME-FIX-5 | Owner/serving readiness key flows and lifecycle | `sn-miner-rust/src/main.rs`, `sn-miner-rust/tests/real_process.rs` | backward-compatible operational stdout marker; stronger integration test contract | No public health protocol. |
