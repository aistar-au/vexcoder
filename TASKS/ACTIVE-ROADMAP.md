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
| `ADR-023` | Locked | `EL-08`, `EL-09`, `EL-10`, and `EL-11` are merged or in review. `EL-12` (`/context` reporting pass) and `EL-13` (`/commands`/`/help` reporting pass) are code-complete on `main`; checkpoint updates are next. |
| `ADR-024` | Proposed | Parity-gap inventory, command surface, and deferred work. |
| `ADR-025` | Proposed | First post-gate Phase I dispatch target (`PI-09` through `PI-12`); implementation remains gated on milestone-1 validation. |
| `ADR-026` | Proposed | Follows `ADR-025` closeout and ADR-024 reconciliation (`PI-13` through `PI-16`); implementation remains gated on milestone-1 validation. |
| `ADR-027` | Accepted | Defines the current full-screen TUI with command-session capture; supersedes the ADR-018/019 path. |
| `ADR-028` | Proposed | Defines the application facade and transport boundary that later CLI and LocalApiServer refactors must follow. |

ADR-025, ADR-026, and ADR-028 are the post-gate Phase I and boundary ADR set.
This roadmap note is descriptive only and does not relax the milestone-1 gate.

ADR-024 checklist reconciliation is current through merged PRs `#60`, `#63`,
`#71`, `#72`, `#74`, `#75`, `#78`, and `#79`. `PK-08` (`vex branch` and
`vex pr-summary`), the ADR-027 command-session follow-up, and `EL-09` are
merged. The remaining milestone-1 queue does not begin at the ADR-025,
ADR-026, and ADR-028 stage yet: `EL-10` still sits in front of that
post-gate set.

## Current Next Dispatcher Batch

`ADR-023 EL-12` and `EL-13` are the next dispatcher batches (combined reporting
pass). Both commands are code-complete on `main`; only ADR-023 evidence blocks
and roadmap checkpoint updates are needed.

- EL-12: `/context` — `test_tui_context_renders_without_model_turn`; `test_tui_context_shows_tilde_token_estimate`; `test_tui_context_shows_active_grants_count`
- EL-13: `/commands`/`/help` — `test_tui_commands_renders_all_registered_commands`; `test_tui_help_is_alias_for_commands`; `test_commands_output_does_not_call_start_turn`; `test_missing_command_description_is_compile_error`

## Other Open ADRs Tracked In This Repo

`adr/ADR-README.md` also lists these tracked ADRs outside the current
dispatch-facing active set:

| ADR | Status | Note |
| :--- | :--- | :--- |
| `ADR-013` | Proposed | TUI completion and deployment plan. |
| `ADR-018` | Superseded by ADR-027 | Earlier managed-TUI overlay path retained for history only. |

## Repo-Local Task Manifests

- `TASKS/PJ-03-memory-notes.md` — `ADR-024` Gap 16, depends on `PA-01`, describes the `/memory` command surface and session-note injection requirements.
