# Implementation Admission Evidence: test-run dedupe

## Implementation Admission Evidence
| evidence_item | source | status | notes |
|---------------|--------|--------|-------|
| proposal_read | `docs/versions/v0.1/modules/workspace-harness/proposal.md` | pass | Approved proposal contains `workspace_harness_test_run_dedupe`. |
| design_read | `docs/versions/v0.1/modules/workspace-harness/design.md` | pass | Approved design maps `workspace_harness_test_run_dedupe` to `harness/scripts/test-run.py` and testing artifacts. |
| change_scope_matches_request | User request "优化吧" after identifying duplicate `test-run.py all all` execution | pass | Scope is limited to unified runner dedupe and evidence preservation. |
| active_module_resolved | `workspace-harness` module packet | pass | `harness/scripts/test-run.py` belongs to workspace harness governance. |
| no_chat_only_evidence | proposal/design rows quoted below | pass | Implementation admission relies on approved docs, not only chat context. |

## Document Binding
| doc | sha256 |
|-----|--------|
| docs/versions/v0.1/modules/workspace-harness/proposal.md | ee4f992e66a67049b507e8eff1a2c12fa9f53a2640dcded8e232abd07ec3c248 |
| docs/versions/v0.1/modules/workspace-harness/design.md | 6a7f488b0fef7c8ef28e1d52bfc68d953184550d76a824d3b98c99ddc7ed256b |

## Coverage Quotes

### Quote: proposal.md workspace_harness_test_run_dedupe
> | P-HARNESS-TEST-RUN-1 | workspace_harness_test_run_dedupe | `test-run.py` avoids duplicate command execution within one run while preserving module/level coverage evidence and deterministic ordering. | `harness/scripts/test-run.py`, related unified-test-entry rules if needed, and workspace-harness test metadata. | dry-run shows repeated commands collapsed or marked reused; lightweight module all passes; one root shortcut all/all artifact passes and includes reuse evidence. | Do not skip registered coverage, remove artifact evidence, change business module test commands, or require duplicate physical all/all runs by default. |

### Quote: design.md workspace_harness_test_run_dedupe
> | workspace_harness_test_run_dedupe | P-HARNESS-TEST-RUN-1 | command source precedence, in-run command-result reuse, artifact step fields, no-dedupe opt-out, and workspace-harness self-admission for governance scripts are defined in this design. | `harness/scripts/test-run.py`, `harness/scripts/stage-scope-check.py`, `docs/versions/v0.1/modules/workspace-harness/testing.md`, `docs/versions/v0.1/modules/workspace-harness/testplan.yaml` | backward-compatible CLI extension: adds optional `--no-dedupe`; artifact step fields are additive; scope checker change is limited to module `workspace-harness` and admitted scope paths | No business module test commands are changed. |
