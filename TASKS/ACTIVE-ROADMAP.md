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
| `ADR-025` | Proposed | Phase I kickoff (`PI-09`, `PI-11`) and continuation (`PI-10`, `PI-12`) are now implemented on the active dispatcher branch; ADR-026 is the next dependency-sequenced transport batch after ADR-024 reconciliation. |
| `ADR-026` | Proposed | Follows `ADR-025` closeout and ADR-024 reconciliation (`PI-13` through `PI-16`); `PI-13` and `PI-14` are the next dependency-sequenced implementation items. |
| `ADR-027` | Accepted | Defines the current full-screen TUI with command-session capture; supersedes the ADR-018/019 path. |
| `ADR-028` | Proposed | Defines the application facade and transport boundary that later CLI and LocalApiServer refactors must follow. |
| `ADR-029` | Proposed | Extends the active ADR set with stream-parser completeness and session-persistence follow-up work without changing the milestone-1 gate result. |
| `ADR-030` | Proposed | Defines the task-state-owned orchestrator model so canonical runtime events update task state before continuation, completion, and command-session decisions are made. |

ADR-025, ADR-026, ADR-028, ADR-029, and ADR-030 are the active post-gate ADR set.

ADR-024 checklist reconciliation is current through merged PRs `#60`, `#63`,
`#71`, `#72`, `#74`, `#75`, `#78`, and `#79`. `PK-08` (`vex branch` and
`vex pr-summary`), the ADR-027 command-session follow-up, and the full
ADR-023 implementation track (`EL-01` through `EL-13`) are now on `main`.
Milestone-1 validation passed on `2026-03-15` on branch
`dispatcher/vexcoder-adr-022-m1-validation-gate`; the ADR-025, ADR-026,
ADR-028, ADR-029, and ADR-030 post-gate work now remains sequenced only by their
documented dependencies.

## Current Next Dispatcher Batch

The next dispatcher batch is ADR-026 Phase I transport binding (`PI-13` and `PI-14` in parallel) after ADR-024 reconciliation.

- Milestone-1 validation remains the recorded Phase I gate result in `adr/ADR-022-free-open-coding-agent-roadmap.md`.
- ADR-025 now has the canonical runtime handoff types, schemas, normalization layer, and BatchMode parity tests on the active dispatcher branch.
- Keep `PI-13` and `PI-14` sequenced immediately after ADR-025 closeout and ADR-024 reconciliation, then keep `PI-15` after both and `PI-16` last.
- Treat ADR-028 as the boundary ADR for the post-gate CLI and LocalApiServer work, while ADR-029 remains an active stream/parser and persistence follow-up ADR and ADR-030 defines the task-state/orchestrator control-flow contract that downstream work must preserve.

## Other Open ADRs Tracked In This Repo

`adr/ADR-README.md` also lists these tracked ADRs outside the current
dispatch-facing active set:

| ADR | Status | Note |
| :--- | :--- | :--- |
| `ADR-013` | Proposed | TUI completion and deployment plan. |
| `ADR-018` | Superseded by ADR-027 | Earlier managed-TUI overlay path retained for history only. |

## Repo-Local Task Manifests

- `TASKS/PJ-03-memory-notes.md` — `ADR-024` Gap 16, depends on `PA-01`, describes the `/memory` command surface and session-note injection requirements.
