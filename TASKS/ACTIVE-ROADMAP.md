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
| `ADR-032` | Active | Prompt-area interactivity and context-budget guard behavior are now on `main`; downstream retrieval and UI work must preserve the landed picker, focus, and context-recovery contracts. |
| `ADR-033` | Active | Phase 1 structural search, Phase 2 semantic reranking, and the Phase 3/4 write-guard plus history-condensing baseline are on `main`; downstream follow-up should keep model guidance and docs aligned with that landed behavior. |
| `ADR-034` | Proposed | Defines the post-milestone multi-agent execution lane: explicit agent definitions, worktree isolation, orchestrator-owned child tasks, and watch/task-management surfaces. |

ADR-025, ADR-026, ADR-028, ADR-029, ADR-030, ADR-031, ADR-032, and ADR-033 are the active post-gate ADR set. ADR-034 is tracked as a proposed post-milestone lane.

ADR-024 checklist reconciliation is current through merged PRs `#60`, `#63`,
`#71`, `#72`, `#74`, `#75`, `#78`, and `#79`. `PK-08` (`vex branch` and
`vex pr-summary`), the ADR-027 command-session follow-up, and the full
ADR-023 implementation track (`EL-01` through `EL-13`) are now on `main`.
Milestone-1 validation passed on `2026-03-15` and remains recorded in
`adr/ADR-022-free-open-coding-agent-roadmap.md`; the ADR-025, ADR-026,
ADR-028, ADR-029, ADR-030, ADR-031, ADR-032, and ADR-033 post-gate work now
remains sequenced only by their documented dependencies.

## Current Next Work Batch

The current work batch is ADR-034 specification and roadmap alignment for post-milestone multi-agent execution, while ADR-033 prompt and documentation alignment remains follow-up maintenance on top of the active ADR-028 facade boundary and the landed ADR-031/ADR-032 operator-surface work.

- Milestone-1 validation remains the recorded Phase I gate result in `adr/ADR-022-free-open-coding-agent-roadmap.md`.
- ADR-025 now has the canonical runtime handoff types, schemas, normalization layer, and BatchMode parity tests in the current tree.
- ADR-026 now has the loopback HTTP transport adapter, schema bundle endpoint, transport/security guards, and PI-16 validation coverage in the current tree.
- ADR-028 now has its phase-1/phase-2 facade split and the 2026-03-17 debug fixes for localhost protocol routing, full-screen task activity visibility, and live orchestration rows in the current tree.
- ADR-031 Batch A follow-up and ADR-032 prompt-area interactivity work are now on `main`; their earlier review branches no longer carry unique diff against current `main`.
- ADR-033 now has Phases 1 through 4 baseline behavior on `main`, including large-file `write_file` guardrails and condensed historical tool results.
- The current ADR-033 next batch is integration cleanup: keep the system prompt, operator docs, and task-roadmap language aligned with the landed large-file edit and history-condensing contracts.
- ADR-034 now defines the missing dedicated multi-agent / parallel-task execution lane that ADR-024 had deferred; no implementation lane should bypass its worktree-isolation and child-task lifecycle rules.
- `src/api/client.rs` remains the model-guidance enforcement point, while `src/state/conversation/tools.rs` and `src/state/conversation/history.rs` remain the runtime contract points for those guardrails.
- Keep documentation refresh and descriptive PR motivation text in scope for ADR-033 follow-up batches so retrieval changes do not land with stale ADR/task-roadmap state.
- Continue preserving ADR-028 facade boundaries and ADR-030 task-state/orchestrator ownership while ADR-033 follow-up work lands.

## Other Open ADRs Tracked In This Repo

`adr/ADR-README.md` also lists these tracked ADRs outside the current
dispatch-facing active set:

| ADR | Status | Note |
| :--- | :--- | :--- |
| `ADR-013` | Proposed | TUI completion and deployment plan. |
| `ADR-018` | Superseded by ADR-027 | Earlier managed-TUI overlay path retained for history only. |

## Repo-Local Task Manifests

- `TASKS/PJ-03-memory-notes-injection.md` — `ADR-024` Gap 16, depends on `PA-01`, describes the `/memory` command surface and session-note injection requirements.
