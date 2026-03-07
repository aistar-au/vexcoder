# Task Dispatch Map

This is the full sequence across all tracks. Docs and TUI are independent
tracks and can run in parallel across sessions.

Canonical active roadmap state: `TASKS/ACTIVE-ROADMAP.md`.
This file covers pre-ADR-024 dispatch waves and active ADR-024 seed batches.

## Active ADR Index (Auto-Managed)

Source of truth: `TASKS/ACTIVE-ROADMAP.md`.

<!-- AUTO:ACTIVE_ROADMAPS:BEGIN -->
- `ADR-022` - Proposed (amendment)
- `ADR-023` - Locked
- `ADR-024` - Proposed
<!-- AUTO:ACTIVE_ROADMAPS:END -->

## Pre-ADR-024 Dispatch Manifests

Uncompleted pre-ADR-024 dispatch ADRs live in `TASKS/` root.

1. `TASKS/ADR-013-tui-completion-deployment-plan.md`
2. `TASKS/ADR-018-managed-tui-scrollback-streaming-cell-overlays.md`
3. `TASKS/ADR-021-codebase-audit-dead-weight-duplication-shared-code-opportunities.md`
4. `TASKS/ADR-022-free-open-coding-agent-roadmap.md`

## ADR-022 Dispatch Manifests (Wave 1)

All Wave 1 manifests are green and promoted to `TASKS/completed/`.

| Manifest | Batch | Anchor verified at commit |
| :--- | :--- | :--- |
| CORE-15-neutral-config-cutover | A | `37a4012` |
| DOC-03-adr-022-migration-guide | A | `37a4012` |
| REF-09-model-backend-seam | B | `37a4012` |
| FEAT-17-command-runner-core | C | `37a4012` |
| FEAT-18-command-cancel-and-pty | C | `37a4012` |
| CRIT-19-diff-native-write-flow | D | `37a4012` |
| CORE-16-capability-approval-policy | D | `37a4012` |
| CORE-17-task-state-persistence | E | `37a4012` |
| FEAT-19-task-first-ui-shell | F | `37a4012` |
| FEAT-20-changed-files-and-evidence-pane | F | `37a4012` |

## ADR-022 Sequencing

```text
CORE-15 ──► REF-09 ──► FEAT-17 ──► FEAT-18
                           │
                           └──► CRIT-19 ──► CORE-16 ──► CORE-17 ──► FEAT-19 ──► FEAT-20
                                                  │
                                                  └──► CORE-18

CORE-15 ──► DOC-03 (parallel with REF-09)

ADR-018 (must be green) ──► FEAT-19
```

## ADR-022 Execution Batches

| Batch | Manifests | Status |
| :--- | :--- | :--- |
| A — Foundation | CORE-15, DOC-03 | green — merged PR #41 |
| B — Backend seam | REF-09 | green — verified at `37a4012`, zero-diff |
| C — Execution core | FEAT-17, FEAT-18 | green — verified at `37a4012`, zero-diff |
| D — Safety + policy | CRIT-19, CORE-16 | green — verified at `37a4012`, zero-diff |
| E — Durability | CORE-17 | green — verified at `37a4012`, zero-diff |
| F — UX | FEAT-19, FEAT-20 | green — verified at `37a4012`, zero-diff |
| G — Autonomy | CORE-18 | green — verified at `2565354`, zero-diff |

## ADR-024 Dispatch Seeds

Initial ADR-024 dispatch starts with the config foundation because it gates
notes-path resolution and hook config resolution.

| Batch | Manifests | Status |
| :--- | :--- | :--- |
| A - Config foundation | PA-01 | green — merged PR #48 |
| B - Notes + Hooks | PJ-03, PL-01 | dispatch ready |

### ADR-024 Sequencing

```text
PA-01 -> PJ-03
PA-01 -> PL-01
```

### ADR-024 Batch B Gates

PJ-03 and PL-01 are independent of each other and may be dispatched in
parallel. Both are gated on PA-01 (config foundation), which is green.

---

## Prior Waves (TUI Track)

### Manifests Added in Prior Wave

1. `TASKS/ADR-013-tui-completion-deployment-plan.md`
2. `TASKS/completed/CORE-12-bounded-transcript.md`
3. `TASKS/completed/CORE-13-dirty-render-guard.md`
4. `TASKS/completed/CORE-14-panic-hook-terminal-restore.md`
5. `TASKS/completed/FEAT-15-scrollback-viewport.md`
6. `TASKS/completed/FEAT-16-idle-interrupt-input-drop-feedback.md`

### Docs Track (Independent Chain)

```text
CORE-02 -> CORE-03 -> CORE-04
```

### TUI Track (Main Chain)

```text
CORE-09
  |
  |- CORE-07 ---- CORE-08
  |                 |
  |            +----+-----+
  |            CORE-10  CORE-13
  |              |
  |            CORE-11
  |              |
  |      +-------+------+--------+
  |     FEAT-10 FEAT-11 FEAT-13 FEAT-14
  |              |
  |            FEAT-12
  |
  |- CORE-12  (parallel from CORE-09)
  |- FEAT-15  (parallel from CORE-09)
  `- FEAT-16  (after CORE-10)
```

### Flat Dispatch Order (Prior Wave, With Parallelism)

| Step | Task | Can run in parallel with |
| :--- | :--- | :--- |
| 1 | CORE-09 | CORE-14, docs track |
| 2 | CORE-07 | CORE-12, FEAT-15 |
| 3 | CORE-08 | CORE-12, FEAT-15 |
| 4 | CORE-10 | CORE-13 |
| 5 | CORE-11 | FEAT-16 |
| 6 | FEAT-10 | FEAT-11, FEAT-13, FEAT-14 |
| 7 | FEAT-11 | FEAT-10, FEAT-13, FEAT-14 |
| 8 | FEAT-12 | FEAT-13, FEAT-14 |

### Task Naming Convention Reference

| Old name | Correct name | Reason |
| :--- | :--- | :--- |
| TUI-01 scrollback | FEAT-15 | User-visible behavior |
| TUI-02 bounded transcript | CORE-12 | Infrastructure, no new UI |
| TUI-03 dirty render guard | CORE-13 | Infrastructure, no new UI |
| TUI-04 panic hook | CORE-14 | Infrastructure, no new UI |
| TUI-05 idle interrupt + drop | FEAT-16 | User-visible behavior |
