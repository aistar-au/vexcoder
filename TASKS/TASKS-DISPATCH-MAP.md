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
| `adr/ADR-021-codebase-audit-unused-code-duplication-shared-code-opportunities.md` | Accepted | 4 items (0 P1, 4 P2, 0 P3); Tier 6 batch completed Items 9/13/20/24/25/28/32/33; Items 15 and 22 were already complete |
| `adr/ADR-022-amendment-2026-03-13.md` | Amended | Amendment only |
| `adr/ADR-022-free-open-coding-agent-roadmap.md` | Proposed (milestone-1 passed) | Post-milestone G/H |
| `adr/ADR-024-zero-licensing-cost-agent-parity-gaps.md` | Proposed (pre-milestone complete) | 7 items (all post-milestone) |
| `adr/ADR-028-application-facade-and-transport-boundaries.md` | Active | Ongoing alignment |
| `adr/ADR-029-stream-parser-completeness-and-session-persistence.md` | Accepted | All 8 decision items verified (2026-03-28) |
| `adr/ADR-030-runtime-task-state-and-orchestrator-control-flow.md` | Accepted | All 6 coverage requirements verified (2026-03-28) |
| `adr/ADR-031-operator-surface-ui-overhaul.md` | Complete (Batches A-E merged) | Verification; fullscreen auto-fit behavior documented for resize and snapped-window layouts |
| `adr/ADR-032-prompt-area-interactivity-and-context-guard.md` | Active | Items 4-5 verified (2026-03-28); remaining items 1-3, 6-8 merged; item 9 deferred; auto-fit prompt surface docs synced |
| `adr/ADR-033-hybrid-retrieval-context-architecture.md` | Complete (Phases 1-4 merged) | Integration |
| `adr/ADR-034-multi-agent-parallel-task-execution.md` | Active (Phase A + B-E merged) | Hardening |

### Moved to completed/ (2026-03-27)

ADR-013, ADR-018, ADR-025, ADR-026, ADR-027 moved to `adr/completed/`.
ADR-023 is complete (EL-01 through EL-13) and remains in `adr/`.

## Remaining Work Summary (12 items across 3 active tiers)

Tiers 1–5 and 7 are complete; Tier 6 is mostly complete; 12 items remain across Tiers 6, 8, and 9.

| Tier | Source | Items | Status | Description |
| :--- | :--- | :--- | :--- | :--- |
| ~~1~~ | PRs 231/232/233/234 | ~~4~~ | Complete | Open PRs merged |
| ~~2~~ | ADR-024 Phases D/F | ~~6~~ | Complete | Sandbox drivers and MCP runtime |
| ~~3~~ | ADR-024 | ~~3~~ | Complete | Workspace tools, MCP HTTP auth, `/plan` + `/context` |
| ~~4~~ | ADR-021 P1 | ~~4~~ | Complete | Unbounded buffers, unhandled errors, comment debt |
| ~~5~~ | ADR-029/030/032 | ~~3~~ | Complete | Verification of already-implemented work (2026-03-28) |
| 6 | ADR-021 P2 | 13 tracked items; 4 remaining | Mostly complete (2026-03-28) | Items 9/13/20/24/25/28/32 completed in this batch; Items 15 and 22 were already complete; Items 10/11/12/14 deferred |
| ~~7~~ | ADR-021 P3 | ~~1~~ | Complete (2026-03-28) | Item 33 idle backoff tuning comment added |
| 8 | ADR-024 G/H + ADR-022 | 7 | Open | Post-milestone release pipeline and packaging |
| 9 | Multiple | 1 | Open | Housekeeping: verify ADR-028 remaining work |

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
are the active post-gate ADR set. ADR-025, ADR-026, and ADR-027 have been
moved to `adr/completed/` as of 2026-03-27.

## Current Next Work Batch

Tiers 1–5 and 7 are complete. Tier 6 (ADR-021 P2) is mostly complete:
Items 9/13/20/24/25/28/32 completed in this batch, Items 15 and 22 were
already complete, and Items 10/11/12/14 are deferred to a future batch.
See `TASKS/ACTIVE-ROADMAP.md` for the breakdown.

## Tracking Notes

- Update `TASKS/ACTIVE-ROADMAP.md` when the active ADR set changes.
- Update this file when repo-local task manifests are added, removed, or re-scoped.
- Update `TASKS/completed/REPO-RAW-URL-MAP.md` in the same change set when tracked files are added or removed.
- Source ADR documents and task manifests remain the behavioral source of truth.
