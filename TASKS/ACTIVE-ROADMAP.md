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
| `ADR-023` | Locked | `EL-03` must be green before `EL-04`, and `EL-05` depends on `EL-04`. |
| `ADR-024` | Proposed | Parity-gap inventory, command surface, and deferred work. |
| `ADR-025` | Proposed | First post-gate Phase I dispatch target (`PI-09` through `PI-12`); implementation remains gated on milestone-1 validation. |
| `ADR-026` | Proposed | Follows `ADR-025` closeout and ADR-024 reconciliation (`PI-13` through `PI-16`); implementation remains gated on milestone-1 validation. |
| `ADR-027` | Accepted | Defines the current primary-terminal overlay TUI and command passthrough behavior; supersedes the ADR-018/019 path. |

ADR-025 and ADR-026 are queued immediately after the milestone-1 correctness
gate. This roadmap note is descriptive only and does not relax that gate.

ADR-024 checklist reconciliation is current through merged PRs `#60`, `#63`,
`#71`, `#72`, and `#74`. With those merged batches reflected in the checklist,
`PK-08` (`vex branch` and `vex pr-summary`) is the next unlocked pre-gate
ADR-024 batch.

## Other Open ADRs Tracked In This Repo

`adr/ADR-README.md` also lists these tracked ADRs outside the current
dispatch-facing active set:

| ADR | Status | Note |
| :--- | :--- | :--- |
| `ADR-013` | Proposed | TUI completion and deployment plan. |
| `ADR-018` | Superseded by ADR-027 | Earlier managed-TUI overlay path retained for history only. |

## Repo-Local Task Manifests

- `TASKS/PJ-03-memory-notes.md` — `ADR-024` Gap 16, depends on `PA-01`, describes the `/memory` command surface and session-note injection requirements.
