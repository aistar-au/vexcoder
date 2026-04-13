# Task Dispatch Map

Descriptive index for repo-local dispatch sources, active task manifests, and tracked-file map dependencies.

Canonical active ADR summary: `TASKS/ACTIVE-ROADMAP.md`.
Whole-repo tracked file map: `TASKS/completed/REPO-RAW-URL-MAP.md`.

## Current Repo-Local Task Manifests

| Manifest | ADR source | Depends on | Scope |
| :--- | :--- | :--- | :--- |
| `TASKS/PI-10-PI-12-adr025-phase1-continuation.md` | `ADR-025` PI-10, PI-12 | `PI-09`, `PI-11` | Normalization layer and serde/schema/grammar/BatchMode test coverage. Complete. |
| `TASKS/PJ-03-memory-notes-injection.md` | `ADR-024` Gap 16 | `PA-01` | `/memory`, `/memory add`, `/memory clear`, and session-note injection. Complete. |
| `TASKS/PM-01-conversation-compaction.md` | Pre-ADR | None | In-memory conversation compaction via LLM summarization. Branch only. |
| `TASKS/PM-02-undo-checkpoints.md` | Pre-ADR | None | `/undo` slash command and per-change checkpoint stack. Branch only. |
| `TASKS/PM-03-code-search.md` | Pre-ADR | None | Code search hardening and `/reindex` command. Branch only. |
| `TASKS/PM-04-auto-memory.md` | Pre-ADR | None | Automatic memory extraction from conversation turns. Branch only. |
| `TASKS/PM-05-crate-boundaries-and-tool-calls.md` | PR #342 docs alignment | `ACTIVE-ROADMAP`, `architecture.md` | Neutral wording, crate-boundary rationale, structured tool-call design, and next-batch dependency decisions. Active. |
| `TASKS/completed/PL-01-pre-post-tool-hooks.md` | `ADR-024` Gap 26 | `PA-01` | Pre/post-tool-call hooks (`[[hooks]]` in user config layer only). All 7 anchor tests pass. Complete. |

## Open ADR Dispatch Sources

Source of truth: `adr/ADR-README.md`.

| ADR source file | Status | Remaining |
| :--- | :--- | :--- |
| `adr/ADR-021-codebase-audit-unused-code-duplication-shared-code-opportunities.md` | Accepted | 0 items remaining; all P1/P2/P3 items complete (Tiers 4/6/7 all cleared) |
| `adr/ADR-022-amendment-2026-03-13.md` | Amended | Amendment only |
| `adr/ADR-022-free-open-coding-agent-roadmap.md` | Proposed (initial validation passed) | Second-stage G/H |
| `adr/ADR-024-zero-licensing-cost-agent-parity-gaps.md` | Proposed (pre-release complete) | 1 external item in the next batch (PG-03 auto-dispatch after tap repo exists); all in-tree G/H work complete 2026-03-28 |
| `adr/ADR-028-application-facade-and-transport-boundaries.md` | Active | Ongoing boundary alignment; Phase 1, 2, and transport extraction committed; boundary tests now cover grouped, multiline, and `super::`-relative `server`/`bin` imports |
| `adr/ADR-029-stream-parser-completeness-and-session-persistence.md` | Accepted | 0 items remaining; all 8 decision items verified in Tier 5 (PR #249) |
| `adr/ADR-030-runtime-task-state-and-orchestrator-control-flow.md` | Accepted | 0 items remaining; all 6 coverage requirements verified in Tier 5 (PR #249) |
| `adr/ADR-031-operator-surface-ui-overhaul.md` | Accepted (all batches A-E merged) | 0 items remaining; status updated in Tier 9 (PR #252) |
| `adr/ADR-032-prompt-area-interactivity-and-context-guard.md` | Accepted | 0 items remaining; items 1-8 complete; item 4-5 verified Tier 5; item 9 transferred to ADR-033 |
| `adr/ADR-033-hybrid-retrieval-context-architecture.md` | Accepted (all phases 1-4 merged) | 0 items remaining; status updated in Tier 9 (PR #252) |
| `adr/ADR-034-multi-agent-parallel-task-execution.md` | Active (Phase A + B-E merged) | Hardening: serialized delegate concurrency enforcement, prompt-length guard, explicit session-task release, handler/stress coverage, and normalized watch status |
| `adr/ADR-035-undo-checkpoints-and-binary-safe-rollback.md` | Accepted | 0 items remaining; Gap 14 rollback strategy formalized for `/undo` |
| `adr/ADR-038-memory-first-architecture-with-minimal-disk-io.md` | Accepted (Batches D-H merged) | 0 items remaining; ADR-038 post-merge bug fix merged in PR #284 |
| `adr/ADR-039-neutral-cli-voice-and-spatial-status-language.md` | Proposed (Batch A merged on main) | Batch A merged in PR #292; search.exclude path-boundary fix in PR #293; 3 remaining batches (B-D): vocabulary, active indicator, paragraph progress stream |

### Moved to completed/ (2026-03-27)

ADR-013, ADR-018, ADR-025, ADR-026, ADR-027 moved to `adr/completed/`.
ADR-023 is complete (EL-01 through EL-13) and remains in `adr/`.

## Remaining Work Summary (1 proposed in-tree ADR + 1 external dependency in the next batch)

Tiers 1–10 are complete for existing in-tree work. ADR-039 now defines the
next proposed operator-surface lane around neutral spatial CLI voice, status
copy, ANSI semantic roles, and paragraph-oriented long-running progress text.
Batch A is merged on main (PR #292): `Mapping adjacent sectors...`,
`State synchronized.`, and semantic color feedback now land on existing
surfaces. A subsequent fix in PR #293 normalizes `search.exclude` entries with
a trailing slash. Remaining work covers the broader vocabulary set, active indicator,
and denser paragraph stream. The only external item in the next batch remains
ADR-024 PG-03 tap auto-dispatch, which stays blocked until the separate tap
repository exists.

| Tier | Source | Items | Status | Description |
| :--- | :--- | :--- | :--- | :--- |
| ~~1~~ | PRs 231/232/233/234 | ~~4~~ | Complete | Open PRs merged |
| ~~2~~ | ADR-024 Phases D/F | ~~6~~ | Complete | Sandbox drivers and MCP runtime |
| ~~3~~ | ADR-024 | ~~3~~ | Complete | Workspace tools, MCP HTTP auth, `/plan` + `/context` |
| ~~4~~ | ADR-021 P1 | ~~4~~ | Complete | Unbounded buffers, unhandled errors, comment debt |
| ~~5~~ | ADR-029/030/032 | ~~3~~ | Complete | Verification of already-implemented work (2026-03-28) |
| ~~6~~ | ADR-021 P2 | ~~13~~ | Complete (2026-03-28) | All 13 items complete; Items 10/12/14 in this batch; Item 11 addressed |
| ~~7~~ | ADR-021 P3 | ~~1~~ | Complete (2026-03-28) | Item 33 idle backoff tuning comment added |
| 8 | ADR-024 G/H + ADR-022 | 1 | Next batch planned (external prerequisite) | Tap repository auto-dispatch after the separate tap repo is created |
| 12 | PR #342 docs alignment | 1 | Active | Neutral crate-boundary wording, structured tool-call docs, and next-batch dependency decisions |
| ~~9~~ | Multiple | ~~1~~ | Complete (2026-03-28) | ADR-028 status verified; grouped, multiline, and relative `super::` import checks close the remaining known `server`/`bin` test bypasses |
| 11 | ADR-039 | 3 | Proposed | Broader vocabulary pass, active indicator, paragraph progress stream |

## Immediate Dependency Notes

```text
ADR-024 Phases (Tier 2-3, after open PRs merge):
  PR 231 -> PD-02, PD-03
  PR 232 -> PF-01, PF-02 -> PI-06, PI-07
  PP-01, PM-02, PI-08 (independent)

Initial validation gate (passed 2026-03-15):
  ADR-022 phases 1-8 validated end-to-end
  ADR-023 EL-01 through EL-13 (all complete)
  ADR-025 PI-09 through PI-12 (all complete)
  ADR-026 PI-13 through PI-16 (all complete)
  ADR-028 active boundary-alignment work with grouped, multiline, and relative-super import coverage
  ADR-034 Phase A + B-E (merged via PRs 228/229/230) plus serialized delegation/release/watch hardening
```

ADR-028 remains the boundary ADR for post-gate work. ADR-029 through ADR-035
plus ADR-038 are accepted in-tree, and ADR-039 is the next proposed
operator-surface lane. ADR-025, ADR-026, and ADR-027 have been moved to
`adr/completed/` as of 2026-03-27.

## Current Next Work Batch

Tiers 1–10 are complete for existing in-tree work. ADR-039 is the next
proposed lane, starting with low-gain status anchors and semantic color
feedback (Batch A, merged on main) before later vocabulary and transcript-model changes. The only external item in the next batch is PG-03
tap auto-dispatch, which depends on the separate tap repository existing first.
See `TASKS/ACTIVE-ROADMAP.md` for the current breakdown.

## Tracking Notes

- Update `TASKS/ACTIVE-ROADMAP.md` when the active ADR set changes.
- Update this file when repo-local task manifests are added, removed, or re-scoped.
- Update `TASKS/completed/REPO-RAW-URL-MAP.md` in the same change set when tracked files are added or removed.
- Source ADR documents and task manifests remain the behavioral source of truth.
