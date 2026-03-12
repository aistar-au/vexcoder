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
| `adr/ADR-018-managed-tui-scrollback-streaming-cell-overlays.md` | Proposed |
| `adr/ADR-021-codebase-audit-dead-weight-duplication-shared-code-opportunities.md` | Accepted, follow-up maintenance remains |
| `adr/ADR-022-amendment-2026-03-03.md` | Proposed |
| `adr/ADR-022-free-open-coding-agent-roadmap.md` | Proposed |
| `adr/ADR-023-deterministic-edit-loop.md` | Locked |
| `adr/ADR-024-zero-licensing-cost-agent-parity-gaps.md` | Proposed |
| `adr/ADR-025-runtime-json-handoff-contract.md` | Proposed |
| `adr/ADR-026-localapiserver-transport-binding.md` | Proposed |

## Immediate Dependency Notes

```text
PA-01 -> PJ-03
EL-03 -> EL-04 -> EL-05

Milestone-1 gate:
ADR-022 phases 1-8 + ADR-023 deterministic edit loop validated end-to-end
  -> ADR-025 PI-09 + PI-11 (parallel)
  -> PI-10 after PI-09
  -> PI-12 after PI-09 through PI-11
  -> ADR-026 PI-13 + PI-14 (parallel) after PI-12 and ADR-024 reconciliation
  -> PI-15 after PI-13 and PI-14
  -> PI-16 last
```

ADR-025 and ADR-026 are the first post-gate Phase I dispatch track. This
dispatch note is descriptive only and does not relax the milestone-1 gate.

## Tracking Notes

- Update `TASKS/ACTIVE-ROADMAP.md` when the active ADR set changes.
- Update this file when repo-local task manifests are added, removed, or re-scoped.
- Update `TASKS/completed/REPO-RAW-URL-MAP.md` in the same change set when tracked files are added or removed.
- Source ADR documents and task manifests remain the behavioral source of truth.
