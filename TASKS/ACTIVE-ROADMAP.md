# Active Roadmap

Descriptive index for the ADRs and task manifests that are currently active in
this repository.

## Dispatch-Facing Active ADR Set

Current task-dispatch dependency state:

| ADR | Status | Dependency note |
| :--- | :--- | :--- |
| `ADR-021` | Accepted, follow-up maintenance remains | Audit and cleanup items can still affect `src/`, tests, or docs shape. |
| `ADR-022 amendment` | Proposed | Constrains first-milestone scope relative to `ADR-022`. |
| `ADR-022` | Proposed | Free/open roadmap target and config/interface decision surface. |
| `ADR-023` | Locked | `EL-08` through `EL-13` are now on `main`. The ADR-023 implementation track is complete; milestone-1 validation has passed and the post-gate ADR-025 Phase I work is now active. |
| `ADR-024` | Proposed | Parity-gap inventory, command surface, and deferred work. |
| `ADR-025` | Proposed | Phase I kickoff (`PI-09`, `PI-11`) and continuation (`PI-10`, `PI-12`) are implemented in the current tree; ADR-026 `PI-13` through `PI-16` are implemented, and ADR-028 follow-up work now runs against the active facade boundary. |
| `ADR-026` | Proposed | `PI-13` through `PI-16` are implemented in the current tree; downstream CLI and LocalApiServer changes must now preserve the active ADR-028 facade boundary. |
| `ADR-027` | Accepted | Defines the current full-screen TUI with command-session capture; supersedes the ADR-018/019 path. |
| `ADR-028` | Active | Phase 1 / Phase 2 facade extraction and 2026-03-17 debug fixes are in the current tree; remaining work continues to shrink `src/app.rs` and harden facade/transport seams. |
| `ADR-029` | Proposed | Extends the active ADR set with stream-parser completeness and session-persistence follow-up work without changing the milestone-1 gate result. |
| `ADR-030` | Active | Task-state-owned orchestration invariants and 2026-03-17 control-flow fixes are active requirements for downstream runtime work. |
| `ADR-031` | Active | Extends the operator surface overhaul with adaptive timeline/transcript/composer behavior, task-state-visible selection, and merge-gated UI batching on top of ADR-030 task-state ownership. |

ADR-025, ADR-026, ADR-028, ADR-029, ADR-030, and ADR-031 are the active post-gate ADR set.

ADR-024 checklist reconciliation is current through merged PRs `#60`, `#63`,
`#71`, `#72`, `#74`, `#75`, `#78`, and `#79`. `PK-08` (`vex branch` and
`vex pr-summary`), the ADR-027 command-session follow-up, and the full
ADR-023 implementation track (`EL-01` through `EL-13`) are now on `main`.
Milestone-1 validation passed on `2026-03-15` on branch
`dispatcher/vexcoder-adr-022-m1-validation-gate`; the ADR-025, ADR-026,
ADR-028, ADR-029, ADR-030, and ADR-031 post-gate work now remains sequenced only by their
documented dependencies.

## Current Next Dispatcher Batch

The current dispatcher batch continues ADR-031 operator-surface follow-up on top of the active ADR-028 facade boundary.

- Milestone-1 validation remains the recorded Phase I gate result in `adr/ADR-022-free-open-coding-agent-roadmap.md`.
- ADR-025 now has the canonical runtime handoff types, schemas, normalization layer, and BatchMode parity tests in the current tree.
- ADR-026 now has the loopback HTTP transport adapter, schema bundle endpoint, transport/security guards, and PI-16 validation coverage in the current tree.
- ADR-028 now has its phase-1/phase-2 facade split and the 2026-03-17 debug fixes for localhost protocol routing, full-screen task activity visibility, and live orchestration rows in the current tree.
- ADR-031 now has its adaptive task-surface groundwork on `main`, including runtime-visible selection state, structured timeline derivation, human-readable header rendering, flowing transcript styling, inline approval cards, and the cumulative `~N.Nk ctx` header indicator.
- `src/app/layout.rs` and `src/ui/draw.rs` still describe the task surface with different region terms and output semantics; follow-up work should collapse those descriptions onto one adaptive timeline/transcript/composer contract.
- Selected-step detail rendering and transcript-first active-turn rendering still share the same `TaskLayoutState` output channel; follow-up work should make that contract explicit across both renderers before more UI batches land.
- `src/ui/draw.rs` still derives header fields by parsing the formatted `status_line` string; follow-up work should replace that string coupling with structured task-surface fields once the facade boundary exposes them.
- Task 6: keep documentation refresh, descriptive PR motivation text, and brand-neutral agent-authored review prose in scope for later ADR-031 batches so UI-overhaul branches do not land with stale docs or third-party product wording in the review trail.
- Continue shrinking `src/app.rs` behind the facade boundary while ADR-029 remains an active stream/parser and persistence follow-up ADR, ADR-030 defines the task-state/orchestrator control-flow contract that downstream work must preserve, and ADR-031 continues the operator-surface batching/polish follow-up.

## Other Open ADRs Tracked In This Repo

`adr/ADR-README.md` also lists these tracked ADRs outside the current
dispatch-facing active set:

| ADR | Status | Note |
| :--- | :--- | :--- |
| `ADR-013` | Proposed | TUI completion and deployment plan. |
| `ADR-018` | Superseded by ADR-027 | Earlier managed-TUI overlay path retained for history only. |

## Repo-Local Task Manifests

- `TASKS/PJ-03-memory-notes-injection.md` — `ADR-024` Gap 16, depends on `PA-01`, describes the `/memory` command surface and session-note injection requirements.
