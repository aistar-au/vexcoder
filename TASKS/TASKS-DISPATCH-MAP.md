# Task Dispatch Map

Descriptive index for repo-local dispatch sources, active task manifests, and tracked-file map dependencies.

Canonical active ADR summary: `TASKS/ACTIVE-ROADMAP.md`.
Whole-repo tracked file map: `TASKS/completed/REPO-RAW-URL-MAP.md`.

## Current Repo-Local Task Manifests

| Manifest | ADR source | Depends on | Scope |
| :--- | :--- | :--- | :--- |
| `TASKS/PI-10-PI-12-adr025-phase1-continuation.md` | `ADR-025` PI-10, PI-12 | `PI-09`, `PI-11` | Normalization layer and serde/schema/grammar/BatchMode test coverage. |
| `TASKS/PJ-03-memory-notes-injection.md` | `ADR-024` Gap 16 | `PA-01` | `/memory`, `/memory add`, `/memory clear`, and session-note injection. |

## Open ADR Dispatch Sources

Source of truth: `adr/ADR-README.md`.

| ADR source file | Status |
| :--- | :--- |
| `adr/ADR-013-tui-completion-deployment-plan.md` | Proposed |
| `adr/ADR-018-managed-tui-scrollback-streaming-cell-overlays.md` | Superseded by ADR-027 |
| `adr/ADR-021-codebase-audit-dead-weight-duplication-shared-code-opportunities.md` | Accepted, follow-up maintenance remains |
| `adr/ADR-022-amendment-2026-03-13.md` | Proposed |
| `adr/ADR-022-free-open-coding-agent-roadmap.md` | Proposed |
| `adr/ADR-023-deterministic-edit-loop.md` | Locked |
| `adr/ADR-024-zero-licensing-cost-agent-parity-gaps.md` | Proposed |
| `adr/ADR-025-runtime-json-handoff-contract.md` | Proposed |
| `adr/ADR-026-localapiserver-transport-binding.md` | Proposed |
| `adr/ADR-027-full-screen-tui-command-session-capture.md` | Accepted |
| `adr/ADR-028-application-facade-and-transport-boundaries.md` | Active |
| `adr/ADR-029-stream-parser-completeness-and-session-persistence.md` | Proposed |
| `adr/ADR-030-runtime-task-state-and-orchestrator-control-flow.md` | Active |
| `adr/ADR-031-operator-surface-ui-overhaul.md` | Active |
| `adr/ADR-032-prompt-area-interactivity-and-context-guard.md` | Active |
| `adr/ADR-033-hybrid-retrieval-context-architecture.md` | Active |

## Immediate Dependency Notes

```text
PA-01 -> PJ-03
EL-08 -> EL-09 -> EL-10 -> EL-11 -> EL-12 -> EL-13 (complete)

Milestone-1 gate (passed 2026-03-15):
ADR-022 phases 1-8 + ADR-023 deterministic edit loop validated end-to-end
  -> ADR-025 PI-09 + PI-11 (kickoff complete)
  -> ADR-025 PI-10 + PI-12 (implemented in the current tree)
  -> ADR-026 PI-13 + PI-14 (implemented in the current tree)
  -> PI-15 implemented in the current tree
  -> PI-16 implemented in the current tree
  -> ADR-028 active follow-up
```

ADR-025 and ADR-026 are the first post-gate Phase I dispatch track. ADR-028 is
now active in the current tree and remains the boundary ADR that later CLI and
LocalApiServer refactors must respect.
ADR-029 through ADR-033 remain part of the active ADR set but do not supersede
the passed milestone-1 gate or ADR-025 sequencing.

## Current Next Work Batch

The current work batch continues ADR-033 hybrid-retrieval follow-up and ADR-028 application-facade boundary enforcement.

- Milestone-1 validation is complete; use the recorded gate result in ADR-022 as the Phase I entry condition.
- ADR-025 now has the completed kickoff (`PI-09`, `PI-11`) and continuation (`PI-10`, `PI-12`) work in the current tree.
- ADR-026 now has `PI-13` through `PI-16` implemented in the current tree.
- Treat ADR-028 as the active boundary/workstream for the post-gate Phase I follow-up work.
- Treat ADR-030 as the control-flow ADR for the post-gate runtime work: provider events normalize into canonical runtime events, task state owns truth, and orchestrator decisions remain runtime-owned.
- Treat ADR-031 and ADR-032 as landed operator-surface prerequisites that downstream retrieval work must preserve.
- Treat ADR-033 as the current retrieval ADR: Phases 1 through 4 baseline behavior are on `main`, including structural search, semantic reranking, large-file write guards, and context condensing.
- The useful remaining follow-up batch is prompt/documentation alignment for those landed ADR-033 contracts; the old ADR-031 Batch A, ADR-032 prompt-interactivity, and ADR-033 Phase 3/4 review branches no longer differ from current `main`.

ADR-027 supersedes the older ADR-018/019 managed-TUI direction and is the
current reference for full-screen rendering and interactive command-session
capture behavior.

## Tracking Notes

- Update `TASKS/ACTIVE-ROADMAP.md` when the active ADR set changes.
- Update this file when repo-local task manifests are added, removed, or re-scoped.
- Update `TASKS/completed/REPO-RAW-URL-MAP.md` in the same change set when tracked files are added or removed.
- Source ADR documents and task manifests remain the behavioral source of truth.
- ADR-024 checklist reconciliation is current through merged PRs `#60`, `#63`,
  `#71`, `#72`, `#74`, `#75`, `#78`, and `#79`; `PK-08`, the ADR-027 follow-up,
  and the full ADR-023 implementation track (`EL-01` through `EL-13`) are now
  on `main`. Milestone-1 validation passed on `2026-03-15`; ADR-026 `PI-13`
  through `PI-16` are now implemented in the current tree, ADR-031 Batch A and
  ADR-032 prompt interactivity are on `main`, and ADR-033 now has its Phase 3/4
  baseline on `main`; prompt/documentation alignment is the remaining follow-up
  batch under the ADR-028 facade boundary.
