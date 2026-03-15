# Task Dispatch Map

Descriptive index for repo-local dispatch sources, active task manifests, and tracked-file map dependencies.

Canonical active ADR summary: `TASKS/ACTIVE-ROADMAP.md`.
Whole-repo tracked file map: `TASKS/completed/REPO-RAW-URL-MAP.md`.

## Current Repo-Local Task Manifests

| Manifest | ADR source | Depends on | Scope |
| :--- | :--- | :--- | :--- |
| `TASKS/PJ-03-memory-notes.md` | `ADR-024` Gap 16 | `PA-01` | `/memory`, `/memory add`, `/memory clear`, and session-note injection. |

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
| `adr/ADR-028-application-facade-and-transport-boundaries.md` | Proposed |
| `adr/ADR-029-stream-parser-completeness-and-session-persistence.md` | Proposed |

## Immediate Dependency Notes

```text
PA-01 -> PJ-03
EL-08 -> EL-09 -> EL-10 -> EL-11 -> EL-12 -> EL-13 (complete)

Milestone-1 gate (passed 2026-03-15):
ADR-022 phases 1-8 + ADR-023 deterministic edit loop validated end-to-end
  -> ADR-025 PI-09 + PI-11 (parallel)
  -> PI-10 after PI-09
  -> PI-12 after PI-09 through PI-11
  -> ADR-026 PI-13 + PI-14 (parallel) after PI-12 and ADR-024 reconciliation
  -> PI-15 after PI-13 and PI-14
  -> PI-16 last
```

ADR-025 and ADR-026 are the first post-gate Phase I dispatch track. ADR-028 is
the boundary ADR that later CLI and LocalApiServer refactors must respect.
ADR-029 remains part of the active ADR set but does not supersede the passed
milestone-1 gate or ADR-025 sequencing.

## Current Next Dispatcher Batch

The next dispatcher batch is ADR-025 Phase I kickoff (`PI-09` through `PI-12`).

- Milestone-1 validation is complete; use the recorded gate result in ADR-022 as the Phase I entry condition.
- Keep ADR-026 sequenced after ADR-025 closeout and ADR-024 reconciliation.
- Treat ADR-028 as the boundary ADR for the post-gate Phase I work, not as the next immediate implementation batch.

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
  on `main`. Milestone-1 validation passed on `2026-03-15`, so ADR-025 is now
  the next dispatcher-owned implementation track, with ADR-026 and ADR-028
  remaining dependency-sequenced follow-ups.
