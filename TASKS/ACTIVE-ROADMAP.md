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
| `ADR-023` | Locked | `EL-07` is merged. The current queued batch is `EL-08` (`ModelProfile` config integration via layered config); `EL-09` follows `EL-08`. |
| `ADR-024` | Proposed | Parity-gap inventory, command surface, and deferred work. |
| `ADR-025` | Proposed | First post-gate Phase I dispatch target (`PI-09` through `PI-12`); implementation remains gated on milestone-1 validation. |
| `ADR-026` | Proposed | Follows `ADR-025` closeout and ADR-024 reconciliation (`PI-13` through `PI-16`); implementation remains gated on milestone-1 validation. |
| `ADR-027` | Accepted | Defines the current full-screen TUI with command-session capture; supersedes the ADR-018/019 path. |

ADR-025 and ADR-026 are queued immediately after the milestone-1 correctness
gate. This roadmap note is descriptive only and does not relax that gate.

ADR-024 checklist reconciliation is current through merged PRs `#60`, `#63`,
`#71`, `#72`, `#74`, `#75`, and `#78`. `PK-08` (`vex branch` and
`vex pr-summary`) and the ADR-027 command-session follow-up are merged. The
remaining milestone-1 queue does not begin at ADR-025 / ADR-026 yet: `EL-08`
still sits in front of the post-gate Phase I track.

## Current Next Dispatcher Batch

`ADR-023 EL-08` is the next dispatcher batch to hand to a low-context coding
agent.

- Anchor test: `test_model_profile_loaded_from_layered_config`
- Primary target files: `src/config.rs`, `src/app.rs`, `src/api/client.rs`
- Required scaffold/docs sync: `src/bin/vex.rs`, `docs/src/configuration.md`
- Supporting tests: `src/config.rs`, `tests/integration_test.rs`

## Other Open ADRs Tracked In This Repo

`adr/ADR-README.md` also lists these tracked ADRs outside the current
dispatch-facing active set:

| ADR | Status | Note |
| :--- | :--- | :--- |
| `ADR-013` | Proposed | TUI completion and deployment plan. |
| `ADR-018` | Superseded by ADR-027 | Earlier managed-TUI overlay path retained for history only. |

## Repo-Local Task Manifests

- `TASKS/PJ-03-memory-notes.md` — `ADR-024` Gap 16, depends on `PA-01`, describes the `/memory` command surface and session-note injection requirements.
