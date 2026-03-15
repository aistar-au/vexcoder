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
| `ADR-023` | Locked | `EL-08` through `EL-13` are now on `main`. The ADR-023 implementation track is complete; the next dependency step is the milestone-1 validation gate before ADR-025 and ADR-026 dispatch resumes. |
| `ADR-024` | Proposed | Parity-gap inventory, command surface, and deferred work. |
| `ADR-025` | Proposed | First post-gate Phase I dispatch target (`PI-09` through `PI-12`); implementation remains gated on milestone-1 validation. |
| `ADR-026` | Proposed | Follows `ADR-025` closeout and ADR-024 reconciliation (`PI-13` through `PI-16`); implementation remains gated on milestone-1 validation. |
| `ADR-027` | Accepted | Defines the current full-screen TUI with command-session capture; supersedes the ADR-018/019 path. |
| `ADR-028` | Proposed | Defines the application facade and transport boundary that later CLI and LocalApiServer refactors must follow. |

ADR-025, ADR-026, and ADR-028 are the post-gate Phase I and boundary ADR set.
This roadmap note is descriptive only and does not relax the milestone-1 gate.

ADR-024 checklist reconciliation is current through merged PRs `#60`, `#63`,
`#71`, `#72`, `#74`, `#75`, `#78`, and `#79`. `PK-08` (`vex branch` and
`vex pr-summary`), the ADR-027 command-session follow-up, and the full
ADR-023 implementation track (`EL-01` through `EL-13`) are now on `main`.
The next milestone-1 step is validation closeout before the ADR-025,
ADR-026, and ADR-028 post-gate work begins.

## Current Next Dispatcher Batch

The next dispatcher batch is the milestone-1 validation gate.

- Confirm ADR-022 phases 1 through 8 remain validated together with the completed ADR-023 edit-loop track.
- Use that gate result to decide when ADR-025 PI-09 through PI-12 can begin.
- Do not claim the milestone-1 gate is passed until the dedicated validation batch lands.

## Other Open ADRs Tracked In This Repo

`adr/ADR-README.md` also lists these tracked ADRs outside the current
dispatch-facing active set:

| ADR | Status | Note |
| :--- | :--- | :--- |
| `ADR-013` | Proposed | TUI completion and deployment plan. |
| `ADR-018` | Superseded by ADR-027 | Earlier managed-TUI overlay path retained for history only. |

## Repo-Local Task Manifests

- `TASKS/PJ-03-memory-notes.md` — `ADR-024` Gap 16, depends on `PA-01`, describes the `/memory` command surface and session-note injection requirements.
