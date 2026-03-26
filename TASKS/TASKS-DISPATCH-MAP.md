# Task Dispatch Map

Descriptive index for repo-local dispatch sources, active task manifests, and tracked-file map dependencies.

Canonical active ADR summary: `TASKS/ACTIVE-ROADMAP.md`.
Whole-repo tracked file map: `TASKS/completed/REPO-RAW-URL-MAP.md`.

## Current Repo-Local Task Manifests

| Manifest | ADR source | Depends on | Scope |
| :--- | :--- | :--- | :--- |
| `TASKS/PI-10-PI-12-adr025-phase1-continuation.md` | `ADR-025` PI-10, PI-12 | `PI-09`, `PI-11` | Normalization layer and serde/schema/grammar/BatchMode test coverage. Complete. |
| `TASKS/PJ-03-memory-notes-injection.md` | `ADR-024` Gap 16 | `PA-01` | `/memory`, `/memory add`, `/memory clear`, and session-note injection. Complete. |

## Open ADR Dispatch Sources

Source of truth: `adr/ADR-README.md`.

| ADR source file | Status | Remaining |
| :--- | :--- | :--- |
| `adr/ADR-021-codebase-audit-dead-weight-duplication-shared-code-opportunities.md` | Accepted | 17 items (3 P1, 13 P2, 1 P3) |
| `adr/ADR-022-amendment-2026-03-13.md` | Amended | Amendment only |
| `adr/ADR-022-free-open-coding-agent-roadmap.md` | Proposed (milestone-1 passed) | Post-milestone G/H |
| `adr/ADR-024-zero-licensing-cost-agent-parity-gaps.md` | Proposed | 16 items |
| `adr/ADR-028-application-facade-and-transport-boundaries.md` | Active | Ongoing alignment |
| `adr/ADR-029-stream-parser-completeness-and-session-persistence.md` | Accepted | Verification follow-up (8 decision items) |
| `adr/ADR-030-runtime-task-state-and-orchestrator-control-flow.md` | Accepted | Verification follow-up (6 coverage requirements) |
| `adr/ADR-031-operator-surface-ui-overhaul.md` | Active (A-E merged) | Verification |
| `adr/ADR-032-prompt-area-interactivity-and-context-guard.md` | Active | Items 4-5 |
| `adr/ADR-033-hybrid-retrieval-context-architecture.md` | Active (Phases 1-4 merged) | Integration |
| `adr/ADR-034-multi-agent-parallel-task-execution.md` | Active (Phase A + B-E merged) | Hardening |

### Implementation-Complete (pending move to completed/)

| ADR source file | Status |
| :--- | :--- |
| `adr/ADR-013-tui-completion-deployment-plan.md` | Accepted (all phases complete) |
| `adr/ADR-018-managed-tui-scrollback-streaming-cell-overlays.md` | Superseded by ADR-027 |
| `adr/ADR-023-deterministic-edit-loop.md` | Complete (EL-01 through EL-13) |
| `adr/ADR-025-runtime-json-handoff-contract.md` | Complete (PI-09 through PI-12) |
| `adr/ADR-026-localapiserver-transport-binding.md` | Complete (PI-13 through PI-16) |
| `adr/ADR-027-full-screen-tui-command-session-capture.md` | Accepted (complete) |

## Remaining Work Summary (49 items across 9 tiers)

| Tier | Source | Items | Description |
| :--- | :--- | :--- | :--- |
| 1 | PRs 231/232/233/234 | 4 | Open PRs ready to merge |
| 2 | ADR-024 Phases D/F | 6 | Sandbox drivers and MCP runtime |
| 3 | ADR-024 | 3 | Workspace tools, MCP HTTP auth, `/plan` + `/context` |
| 4 | ADR-021 P1 | 4 | Unbounded buffers and unhandled errors |
| 5 | ADR-029/030/032 | 3 | Verification of already-implemented work |
| 6 | ADR-021 P2 | 13 | Code quality and duplication removal |
| 7 | ADR-021 P3 | 1 | Idle backoff tuning |
| 8 | ADR-024 G/H + ADR-022 | 7 | Post-milestone release pipeline and packaging |
| 9 | Multiple | 8 | Housekeeping: move completed ADRs, update status fields |

## Immediate Dependency Notes

```text
ADR-024 Phases (Tier 2-3, after open PRs merge):
  PR 231 -> PD-02, PD-03
  PR 232 -> PF-01, PF-02 -> PI-06, PI-07
  PP-01, PM-02, PI-08 (independent)

Milestone-1 gate (passed 2026-03-15):
  ADR-022 phases 1-8 validated end-to-end
  ADR-023 EL-01 through EL-13 (all complete)
  ADR-025 PI-09 through PI-12 (all complete)
  ADR-026 PI-13 through PI-16 (all complete)
  ADR-028 active follow-up
  ADR-034 Phase A + B-E (merged via PRs 228/229/230)
```

ADR-028 remains the boundary ADR for post-gate work. ADR-029 through ADR-034
are the active post-gate ADR set. ADR-025, ADR-026, and ADR-027 are
implementation-complete and pending housekeeping move to completed/.

## Current Next Work Batch

The highest-priority remaining work is merging the four open PRs
(231/232/233/234, Tier 1) to unblock Tier 2 sandbox and MCP
work. See `TASKS/ACTIVE-ROADMAP.md` for the full 49-item, 9-tier breakdown.

## Tracking Notes

- Update `TASKS/ACTIVE-ROADMAP.md` when the active ADR set changes.
- Update this file when repo-local task manifests are added, removed, or re-scoped.
- Update `TASKS/completed/REPO-RAW-URL-MAP.md` in the same change set when tracked files are added or removed.
- Source ADR documents and task manifests remain the behavioral source of truth.
