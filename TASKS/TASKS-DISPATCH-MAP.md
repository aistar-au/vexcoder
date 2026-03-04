# Task Dispatch Map

This is the full sequence across all tracks. Docs and TUI are independent
tracks and can run in parallel across sessions.

## Active ADR Dispatch Manifests

Uncompleted dispatch ADRs live in `TASKS/` root.

1. `TASKS/ADR-013-tui-completion-deployment-plan.md`
2. `TASKS/ADR-018-managed-tui-scrollback-streaming-cell-overlays.md`
3. `TASKS/ADR-021-codebase-audit-dead-weight-duplication-shared-code-opportunities.md`
4. `TASKS/ADR-022-free-open-coding-agent-roadmap.md`

## ADR-022 Dispatch Manifests (Wave 1)

Added with ADR-022 (2026-03-01):

1. `TASKS/CORE-15-neutral-config-cutover.md`
2. `TASKS/REF-09-model-backend-seam.md`
3. `TASKS/FEAT-17-command-runner-core.md`
4. `TASKS/FEAT-18-command-cancel-and-pty.md`
5. `TASKS/CRIT-19-diff-native-write-flow.md`
6. `TASKS/CORE-16-capability-approval-policy.md`
7. `TASKS/CORE-17-task-state-persistence.md`
8. `TASKS/FEAT-19-task-first-ui-shell.md`
9. `TASKS/FEAT-20-changed-files-and-evidence-pane.md`
10. `TASKS/CORE-18-repo-navigation-operator-surface.md`
11. `TASKS/DOC-03-adr-022-migration-guide.md`

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

| Batch | Manifests | Gate |
| :--- | :--- | :--- |
| A — Foundation | CORE-15, DOC-03 | No dependencies |
| B — Backend seam | REF-09 | CORE-15 green |
| C — Execution core | FEAT-17, FEAT-18 | REF-09 green |
| D — Safety + policy | CRIT-19, CORE-16 | FEAT-17 green |
| E — Durability | CORE-17 | CORE-16 green |
| F — UX | FEAT-19, FEAT-20 | ADR-018 dispatch gates green + CORE-17 green |
| G — Autonomy | CORE-18 | CORE-16 green |

No batch is promoted until all required dependency manifests are green.

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
