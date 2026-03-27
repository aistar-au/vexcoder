# Active Roadmap

Single canonical source for what is active. Both `onboarding.md` Section 2b
and `TASKS/TASKS-DISPATCH-MAP.md` reference this file -- they do not duplicate it.

Updated by the merge workflow after each ADR-scoped PR lands on main.
Do not edit manually except via the standard exact-diff workflow.

Last updated: 2026-03-28

---

## Active ADRs

| ADR | Status | Remaining items | Dependency note |
| :--- | :--- | :--- | :--- |
| ADR-021 | Accepted | 4 (0 P1, 4 P2, 0 P3) | Tier 6 batch completed Items 9/13/20/24/25/28/32/33; Items 10/11/12/14 remain (larger refactors, deferred) |
| ADR-022 amendment | Amended | Amendment only | Tightens milestone-1 command-execution rules relative to ADR-022 |
| ADR-022 | Proposed (milestone-1 passed) | Post-milestone G/H | Roadmap; spawns ADR-023, ADR-024, ADR-027, ADR-031 |
| ADR-024 | Proposed (pre-milestone complete) | 7 items (all post-milestone) | PA–PM and PP done; PG/PH post-milestone deferred |
| ADR-028 | Active | Ongoing alignment | Phase 1+2 merged; facade boundary governs ADR-030/031 |
| ADR-029 | Accepted | 8 decision items to re-verify | Feeds ADR-030; verification follow-up remains in Tier 6 |
| ADR-030 | Accepted | 6 coverage requirements to re-verify | 7 invariants defined; verification evidence remains in Tier 6 |
| ADR-031 | Complete (Batches A-E merged) | Tier 9: update status field | PRs 196/225/226/227 merged |
| ADR-032 | Active | Items 4-5 | Prompt area; items 1-3 and 6-8 merged; item 9 deferred to ADR-033; fullscreen auto-fit docs synced 2026-03-28 |
| ADR-033 | Complete (Phases 1-4 merged) | Tier 9: update status field | PRs 186/191/192/194/199 merged |
| ADR-034 | Active (Phase A + B-E merged) | Follow-up hardening | PRs 228/229/230 merged |

## Implementation-Complete ADRs (moved to completed/)

| ADR | Status | Notes |
| :--- | :--- | :--- |
| ADR-013 | Accepted — moved to completed/ | All phases complete |
| ADR-018 | Superseded — moved to completed/ | Superseded by ADR-027 |
| ADR-023 | Complete | EL-01 through EL-13 all merged |
| ADR-025 | Complete — moved to completed/ | PI-09 through PI-12 all merged |
| ADR-026 | Complete — moved to completed/ | PI-13 through PI-16 all merged |
| ADR-027 | Accepted (complete) — moved to completed/ | Supersedes ADR-018/019 |

---

## Remaining Work: 25 Items Across 5 Active Tiers

Tiers sorted by unblocking impact -- what, if implemented first, unblocks
the most downstream work.

### ~~Tier 1 -- Open PRs~~ (cleared 2026-03-27)

PRs 231, 232, 233, 234 all merged to main.

### ~~Tier 2 -- Sandbox and MCP Completion (ADR-024)~~ (cleared 2026-03-27)

PD-02, PD-03 (PR 231), PF-01, PF-02 (PR 232), PI-06, PI-07 (this PR) all complete.

### ~~Tier 3 -- Workspace Tools and MCP Extensions (ADR-024)~~ (cleared 2026-03-27)

PP-01 (`list_dir`, `glob_files`, gitignore-aware `search_files`) merged in this PR.
PM-02 (MCP HTTP headers env-var substitution) merged in PR 236.
PI-08 (`/plan`, `/context`) merged in ADR-023 batch.

### ~~Tier 4 -- Security Hardening (ADR-021 P1)~~ (cleared 2026-03-27)

- Item 18: editor MAX_INPUT_BYTES cap in src/ui/editor.rs
- Item 26: SSE buffer renamed to MAX_SSE_BUFFER_BYTES; overflow now emits
  StreamEvent::Error instead of bail!, surfacing cleanly to UiUpdate::Error
- Item 19: parse_frame_bytes emits StreamEvent::Error on failure;
  ConversationStreamUpdate::StreamError added; context.rs forwards to
  UiUpdate::Error
- Item 8: stale REF-07/EL-0X task-ID comments removed from production source

### ~~Tier 5 -- Verification and Governance~~ (cleared 2026-03-28)

All 3 verification items confirmed in-tree:
- ADR-029: All 8 decision items verified present (StreamEvent, ContentBlock, Delta, ApiUsage, MessageDelta, MessageStartData, chat-completions, TaskState)
- ADR-030: All 6 coverage requirements have named tests in the tree
- ADR-032: Items 4 (character count indicator) and 5 (focus indicator) verified implemented in src/ui/draw/mod.rs
- ADR-031/ADR-032/docs: fullscreen composer auto-fit behavior documented consistently for live row/column resize and snapped terminal layouts

### Tier 6 -- Code Quality (ADR-021 P2) -- 11 items (8 done; 4 remain)

Duplication removal, race condition fixes, and design follow-ups.

- ~~Item 9: Tool error dispatch block repeated~~ (done 2026-03-28; `emit_tool_error` helper added in core.rs)
- Item 10: Scroll handling duplication
- Item 11: Approval input parsing duplicated
- Item 12: Diff row styling logic duplicated
- ~~Item 13: required_tool_string variants overlapping~~ (done 2026-03-28; `required_tool_string` delegates to `required_tool_string_any`)
- Item 14: Auto-follow behavior duplication
- ~~Item 15: MAX_INPUT_PANE_ROWS not applied in prod~~ (done 2026-03-28; fullscreen composer now auto-fits within the live terminal viewport)
- ~~Item 20: edit_file TOCTOU race condition~~ (done 2026-03-28; TOCTOU risk documented with structured comment)
- ~~Item 22: StreamBlock::ToolCall deltas ignored~~ (done 2026-03-28)
- ~~Item 24: Startup event draining heuristics~~ (done 2026-03-28; `VEX_DISABLE_STARTUP_FILTER=1` env gate added)
- ~~Item 25: Late StreamDelta dropped~~ (done 2026-03-28; debug observability added under `#[cfg(debug_assertions)]`)
- ~~Item 28: Read-only intent heuristic false positives~~ (done 2026-03-28; `VEX_FORCE_MUTATING_TURN=1` env gate added)
- ~~Item 32: KeyEventKind::Release filtering~~ (done earlier; confirmed 2026-03-28; filter in tui_frontend.rs)

### Tier 7 -- Tuning (ADR-021 P3) -- 1 item (done)

- ~~Item 33: IDLE_LOOP_BACKOFF tuning~~ (done 2026-03-28; tuning comment added noting 62Hz practical cap)

### Tier 8 -- Post-Milestone (ADR-024 G/H + ADR-022) -- 7 items

Explicitly deferred until after milestone-1.

- PG-01: Release workflow -- Linux/macOS targets
- PG-02: Release workflow -- Windows (gnu) target
- PG-03: Package-manager tap formula
- PH-01: macOS app layer -- process management
- PH-02: macOS app layer -- keychain credential storage
- PH-03: macOS code signing + notarisation + .dmg
- ADR-022 Decision 11: Native packaging (post-milestone-1)

### Tier 9 -- Housekeeping -- 3 items (5 of 8 cleared 2026-03-27)

ADR-013, ADR-018, ADR-025, ADR-026, ADR-027 moved to completed/.

Remaining:
- Verify ADR-028 remaining work and update status
- Update ADR-031 status to reflect all batches A-E merged
- Update ADR-033 status to reflect all phases 1-4 merged

---

## Dependency Graph

```
ADR-022 (Roadmap, milestone-1 passed)
  +-- ADR-023 (Edit Loop) -- COMPLETE (EL-01 through EL-13)
  +-- ADR-024 (Parity Gaps) -- 7/56 items remaining (all post-milestone PG/PH deferred)
  |     +-- ADR-025 (Handoff Contract) -- COMPLETE
  |     +-- ADR-026 (Transport Binding) -- COMPLETE
  +-- ADR-027 (Command Sessions) -- COMPLETE
  +-- ADR-031 (UI Overhaul) -- Batches A-E merged
        +-- ADR-032 (Prompt/Context Guard) -- items 4-5 verified; fullscreen auto-fit docs synced
              +-- ADR-033 (Hybrid Retrieval) -- Phases 1-4 merged

ADR-029 (Stream Parser) --> ADR-030 (Orchestrator) --> ADR-031
ADR-028 (Facade) --> ADR-030 --> ADR-031
ADR-034 (Multi-Agent) --> ADR-028, ADR-030
```

---

## Completed ADRs (reference only)

| ADR | Completed | Notes |
| :--- | :--- | :--- |
| ADR-001 through ADR-020 | See adr/completed/ | Full history in completed/ directory |
| ADR-025 | 2026-03-27 | PI-09 through PI-12 all merged; moved to completed/ |
| ADR-026 | 2026-03-27 | PI-13 through PI-16 all merged; moved to completed/ |
| ADR-027 | 2026-03-27 | Command sessions complete; supersedes ADR-018/019; moved to completed/ |

---

## How this file is updated

After each ADR-scoped PR merges to main, the follow-up PR updates:

1. This file -- current phase / remaining items for the relevant ADR
2. Nothing else -- do not touch onboarding or dispatch map in the same edit

The PR body for a roadmap update uses the motivation template from
vex-local-bash/SKILL.md with ADR reference pointing to this file.
