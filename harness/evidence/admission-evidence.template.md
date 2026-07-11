# Task <task-id> Admission Evidence

<!--
File name contract: docs/versions/<version>/evidence/admission/<YYYYMMDD>-<task-slug>.md
- <YYYYMMDD> is today's date; admission-check.py rejects malformed or future dates.
- Generate the ## Document Binding hashes with:
  uv run --active python ./harness/scripts/admission-check.py --version <version> --module <packet-module> [--submodule <task-name>] [--target-module <project>] --print-doc-hashes
- In no-doc auto-pipeline mode, the hash output includes task-local `pipeline/plan.md` instead of `design.md`; quote its scope rows for design coverage. Mutable sibling `pipeline/state.json` is deliberately not admission-bound.
- Quotes must be copied verbatim from the current approved documents; admission-check.py
  re-verifies hashes and quotes against the documents, so editing a document after writing
  this file invalidates the evidence.
- A passing admission-check.py run writes <task-id>.<module>[.<submodule>][.<target-module>].stamp.json next
  to this file. Never create or edit stamp files by hand; `admission-check.py --verify-only`
  and `check-all.py` revalidate the stamp and its design Scope Paths.
- `check-all.py` reruns `admission-check.py --verify-only` for every evidence/stamp pair and
  fails orphan evidence, malformed stamps, stale document bindings, or altered scope paths.
-->

## Implementation Admission Evidence
| evidence_item | source | status | notes |
|---------------|--------|--------|-------|
| proposal_read | docs/versions/<version>/modules/<module>/proposal.md | pass | task-specific note on what was read |
| design_read | docs/versions/<version>/modules/<packet-module>/<task-name>/design.md or task-local pipeline/plan.md `## Implementation Scope Bindings` | pass | cite target_module plus task-specific design coverage |
| change_scope_matches_request | proposal <proposal_id> / design mapping <change_id> | pass | why the admitted change covers the current request |
| active_module_resolved | docs/versions/<version>/modules/<module> | pass | how the active module was resolved |
| same_module_task_selection | docs/versions/<version>/modules/tasks.md and docs/modules/<module>.md Current/Active Task | pass | confirm reused tasks are same-module only, or that different-module unfinished tasks were excluded and a new packet was created |
| no_chat_only_evidence | versioned docs only | pass | confirm no chat-only assumptions were used |

## Document Binding
| doc | sha256 |
|-----|--------|
| docs/versions/<version>/modules/<module>/proposal.md | <64-hex LF-normalized sha256> |
| docs/versions/<version>/modules/<module>/design.md | <64-hex LF-normalized sha256> |
| docs/versions/<version>/modules/<packet-module>/<task-name>/pipeline/plan.md | <64-hex LF-normalized sha256> |

## Coverage Quotes

### Quote: proposal.md <change_id>
> | <proposal_id> | <change_id> | <outcome> | <success evidence> |

### Quote: design.md <change_id>
> | <change_id> | <proposal_id> | <design coverage> | <scope paths> |

### Quote: pipeline/plan.md <change_id>
> | <change_id> | <proposal_id> | <design coverage> | <scope paths> |
