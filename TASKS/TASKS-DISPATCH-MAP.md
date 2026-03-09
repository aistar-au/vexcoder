# Task Dispatch Map

Descriptive index for repo-local dispatch sources, active task manifests, and tracked-file map dependencies.

Canonical active ADR summary: `TASKS/ACTIVE-ROADMAP.md`.
Whole-repo tracked file map: `TASKS/completed/REPO-RAW-URL-MAP.md`.

## Current Repo-Local Task Manifests

| Manifest | ADR source | Depends on | Scope |
| :--- | :--- | :--- | :--- |
| `TASKS/PJ-03-memory-notes.md` | `ADR-024` Gap 16 | `PA-01` | `/memory`, `/memory add`, `/memory clear`, and session-note injection. |

## Open ADR Dispatch Sources

Source of truth: `docs/adr/ADR-README.md`.

| ADR source file | Status |
| :--- | :--- |
| `docs/adr/ADR-013-tui-completion-deployment-plan.md` | Proposed |
| `docs/adr/ADR-018-managed-tui-scrollback-streaming-cell-overlays.md` | Proposed |
| `docs/adr/ADR-021-codebase-audit-dead-weight-duplication-shared-code-opportunities.md` | Accepted, follow-up maintenance remains |
| `docs/adr/ADR-022-amendment-2026-03-03.md` | Proposed |
| `docs/adr/ADR-022-free-open-coding-agent-roadmap.md` | Proposed |
| `docs/adr/ADR-023-deterministic-edit-loop.md` | Locked |
| `docs/adr/ADR-024-zero-licensing-cost-agent-parity-gaps.md` | Proposed |

## Immediate Dependency Notes

```text
PA-01 -> PJ-03
EL-03 -> EL-04 -> EL-05
```

## Tracking Notes

- Update `TASKS/ACTIVE-ROADMAP.md` when the active ADR set changes.
- Update this file when repo-local task manifests are added, removed, or re-scoped.
- Update `TASKS/completed/REPO-RAW-URL-MAP.md` in the same change set when tracked files are added or removed.
- Source ADR documents and task manifests remain the behavioral source of truth.
