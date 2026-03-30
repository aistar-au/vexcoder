# Active Roadmap

Single canonical source for what is active. Both `onboarding.md` Section 2b
and `TASKS/TASKS-DISPATCH-MAP.md` reference this file -- they do not duplicate it.

Updated by the merge workflow after each ADR-scoped PR lands on main.
Do not edit manually except via the standard exact-diff workflow.

Last updated: 2026-03-31 (ADR-038 phase 2 Level 0 foundation + config cache)

---

## Active ADRs

| ADR | Status | Remaining items | Dependency note |
| :--- | :--- | :--- | :--- |
| ADR-021 | Accepted | 0 (all items complete) | All P1/P2/P3 items complete; see Tier 6 section |
| ADR-022 amendment | Amended | Amendment only | Tightens milestone-1 command-execution rules relative to ADR-022 |
| ADR-022 | Proposed (milestone-1 passed) | Post-milestone G/H | Roadmap; spawns ADR-023, ADR-024, ADR-027, ADR-031 |
| ADR-024 | Proposed (pre-milestone complete) | 1 item (PG-03 tap auto-dispatch deferred) | PA–PM and PP done; PG-01/PG-02/PG-03 template complete; PH-01/PH-02/PH-03 complete; PL-01 (pre/post-tool hooks, Gap 26) complete |
| ADR-028 | Active | Ongoing boundary alignment | Phase 1, 2, and transport extraction committed 2026-03-25; boundary tests now cover direct, grouped, multiline, and `super::`-relative `server`/`bin` imports for all inner layers |
| ADR-029 | Accepted | 0 items remaining | All 8 decision items verified in Tier 5 (PR #249) |
| ADR-030 | Accepted | 0 items remaining | All 6 coverage requirements verified in Tier 5 (PR #249) |
| ADR-031 | Accepted (all batches A-E merged) | 0 items remaining | Status updated in Tier 9 (PR #252) |
| ADR-032 | Accepted | 0 items remaining | Items 1-8 complete; item 4-5 verified Tier 5; item 9 transferred to ADR-033 |
| ADR-033 | Accepted (all phases 1-4 merged) | 0 items remaining | Status updated in Tier 9 (PR #252) |
| ADR-034 | Accepted (all phases A-E + watch-stream merged) | 0 items remaining | Phase E2 watch-stream added: GET /v1/session-tasks/{id}/watch SSE with immediate snapshot + broadcast fan-out; PR #261 closes Phase E watch-stream |
| ADR-035 | Accepted | 0 items remaining | Gap 14 `/undo` rollback strategy is now specified and implemented with binary-safe checkpoints |
| ADR-038 | Active (Phase 2 merged) | 2 items remaining | Phase 1: bounded context cache + opt-in auto git; Phase 1a: search lane tightening; Phase 2: disk_policy.rs + config/cache.rs; follow-ups: config decomposition, operator enforcement, task-state durability |

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

## Remaining Work: 1 Active In-Tree ADR + 1 Deferred External Dependency

ADR-038 now tracks the active in-tree TTFC follow-up around memory-first
context assembly. Phase 2 adds `disk_policy.rs` (DiskPermission classifier)
and `config/cache.rs` (OnceLock config cache). Remaining follow-ups:
config load.rs decomposition, operator-level FileSystem trait enforcement,
strict policy CI gates, and optional task-state WAL. The only deferred
external follow-up is still ADR-024 PG-03 tap auto-dispatch, which stays
blocked until the separate `homebrew-vex` tap repository exists.

### ~~Tier 1 -- Open PRs~~ (cleared 2026-03-27)

PRs 231, 232, 233, 234 all merged to main.

### ~~Tier 2 -- Sandbox and MCP Completion (ADR-024)~~ (cleared 2026-03-27)

PD-02, PD-03 (PR 231), PF-01, PF-02 (PR 232), and PI-06/PI-07 are complete.

### ~~Tier 3 -- Workspace Tools and MCP Extensions (ADR-024)~~ (cleared 2026-03-27)

PP-01 (`list_dir`, `glob_files`, gitignore-aware `search_files`) is complete.
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

All 4 verification items confirmed in-tree:
- ADR-029: All 8 decision items verified present (StreamEvent, ContentBlock, Delta, ApiUsage, MessageDelta, MessageStartData, chat-completions, TaskState)
- ADR-030: All 6 coverage requirements have named tests in the tree
- ADR-032: Items 4 (character count indicator) and 5 (focus indicator) verified implemented in src/ui/draw/mod.rs
- ADR-031/ADR-032/docs: fullscreen composer auto-fit behavior documented consistently for live row/column resize and snapped terminal layouts

### ~~Tier 6 -- Code Quality (ADR-021 P2)~~ (cleared 2026-03-28)

All 13 tracked items complete.

- ~~Item 9: Tool error dispatch block repeated~~ (done 2026-03-28; `emit_tool_error` helper added in core.rs)
- ~~Item 10: Scroll handling duplication~~ (done 2026-03-28; `apply_bounded_scroll` extracted; patch overlay and inspector scroll delegate to it)
- ~~Item 11: Approval input parsing duplicated~~ (addressed; `parse_approval_selection` already centralized; per-handler response logic is not reducible further without a callback interface)
- ~~Item 12: Diff row styling logic duplicated~~ (done 2026-03-28; `diff_line_color` helper centralized; both callers delegate to it)
- ~~Item 13: required_tool_string variants overlapping~~ (done 2026-03-28; `required_tool_string` delegates to `required_tool_string_any`)
- ~~Item 14: Auto-follow behavior duplication~~ (done 2026-03-28; `apply_auto_follow_or_clamp` helper extracted; both sites in model_update.rs delegate to it)
- ~~Item 15: MAX_INPUT_PANE_ROWS not applied in prod~~ (done 2026-03-28; fullscreen composer now auto-fits within the live terminal viewport)
- ~~Item 20: edit_file TOCTOU race condition~~ (done 2026-03-28; TOCTOU risk documented with structured comment)
- ~~Item 22: StreamBlock::ToolCall deltas ignored~~ (done 2026-03-28)
- ~~Item 24: Startup event draining heuristics~~ (done 2026-03-28; `VEX_DISABLE_STARTUP_FILTER=1` env gate added)
- ~~Item 25: Late StreamDelta dropped~~ (done 2026-03-28; debug observability added under `#[cfg(debug_assertions)]`)
- ~~Item 28: Read-only intent heuristic false positives~~ (done 2026-03-28; `VEX_FORCE_MUTATING_TURN=1` env gate added)
- ~~Item 32: KeyEventKind::Release filtering~~ (done earlier; confirmed 2026-03-28; filter in tui_frontend.rs)

### Tier 7 -- Tuning (ADR-021 P3) -- 1 item (done)

- ~~Item 33: IDLE_LOOP_BACKOFF tuning~~ (done 2026-03-28; tuning comment added noting 62Hz practical cap)

### ~~Tier 8 -- Post-Milestone (ADR-024 G/H + ADR-022)~~ (cleared 2026-03-28) -- 0 items

PG-01 and PG-02 are complete (2026-03-28). PG-03, PH-01, PH-02, PH-03 complete 2026-03-28.
ADR-022 Decision 11 maps to PH-01/PH-02/PH-03 and is satisfied by the Phase H implementation.
The tap auto-dispatch update (sending a repository-dispatch to homebrew-vex on tag push) is
explicitly deferred per ADR-024 §PG-03 — it requires the homebrew-vex tap repo to be created
first and is not a blocker for the Phase H distribution milestone.

- ~~PG-01: Release workflow -- Linux/macOS targets~~ (done 2026-03-28; existing release.yml targets verified; ADR-024 PG-01 checked)
- ~~PG-02: Release workflow -- Windows (gnu) target~~ (done 2026-03-28; x86_64-pc-windows-gnu added to release matrix via cross on ubuntu-24.04)
- ~~PG-03: Package-manager tap formula~~ (done 2026-03-28; packaging/homebrew/vex.rb template + scripts/update_homebrew_formula.py added; tap auto-dispatch deferred)
- ~~PH-01: macOS app layer -- process management~~ (done 2026-03-28; packaging/macos/src/main.rs + bundle.rs added; vex-launcher opens Terminal.app with bundled vex binary)
- ~~PH-02: macOS app layer -- keychain credential storage~~ (done 2026-03-28; packaging/macos/src/keychain.rs added; Security.framework FFI reads VEX_MODEL_TOKEN from system keychain)
- ~~PH-03: macOS code signing + notarisation + .dmg~~ (done 2026-03-28; packaging/macos/build-app.sh + release.yml macos-pkg job added; codesign + xcrun notarytool + hdiutil .dmg; signing conditional on APPLE_DEVELOPER_ID_CERT secret)
- ~~ADR-022 Decision 11: Native packaging (post-milestone-1)~~ (satisfied by PH-01/PH-02/PH-03 above)

### ~~Tier 9 -- Housekeeping~~ (cleared 2026-03-28) -- 0 items (all 8 cleared)

ADR-013, ADR-018, ADR-025, ADR-026, ADR-027 moved to completed/.
ADR-031 status updated to Accepted (Batches A-E merged).
ADR-033 status updated to Accepted (Phases 1-4 merged).
ADR-028 status verified: Phase 1, 2, and transport extraction committed 2026-03-25; grouped, multiline, and relative `super::` `server`/`bin` import coverage now closes the remaining known boundary-test bypasses for inner layers.

### Tier 10 -- Memory-First TTFC Hardening (ADR-038) -- 3 items

- Phase 1 complete: bounded in-memory context snapshot cache and opt-in automatic git context merged.
- Phase 2 pending: split `src/config/load.rs` into cache, path, and merge seams with process-local config caching.
- Phase 3 pending: add explicit disk-permission boundaries so `.vex/index/` and `.vex/state/` remain the deliberate durable layers.
- Phase 4 pending: evaluate task-state WAL and strict CI enforcement after the hot path is stable.

---

## Active Feature Branches (not yet merged)

| Task | Branch | PR | Status | Description |
| :--- | :--- | :--- | :--- | :--- |
| PL-01-ext | `work/vexcoder-http-hooks` | #270 | **Merged** | HTTP webhook support for tool events (`[[http_hooks]]` config section) |
| PM-01 | `work/vexcoder-conversation-compaction` | #271 | Implementation complete, draft PR | In-memory summarization of older turns when token count exceeds threshold |
| PM-02 | `work/vexcoder-undo-checkpoints` | #272 | Implementation complete, draft PR | `/undo` slash command and per-change checkpoint stack |
| PM-03 | `work/vexcoder-code-search` | #273 | Implementation complete, draft PR | Code search hardening and `/reindex` command |
| PM-04 | `work/vexcoder-auto-memory` | #274 | Implementation complete, draft PR | Automatic extraction of memory-worthy facts from conversation turns |

These branches are pushed to remote with draft PRs. Each contains a task
manifest in `TASKS/` defining scope, constraints, and anchor tests.

---

## Dependency Graph

```
ADR-022 (Roadmap, milestone-1 passed)
  +-- ADR-023 (Edit Loop) -- COMPLETE (EL-01 through EL-13)
  +-- ADR-024 (Parity Gaps) -- 1/56 item remaining (tap auto-dispatch deferred pending tap repo creation)
  |     +-- ADR-025 (Handoff Contract) -- COMPLETE
  |     +-- ADR-026 (Transport Binding) -- COMPLETE
  +-- ADR-027 (Command Sessions) -- COMPLETE
  +-- ADR-031 (UI Overhaul) -- Batches A-E merged
        +-- ADR-032 (Prompt/Context Guard) -- items 4-5 verified; fullscreen auto-fit docs synced
              +-- ADR-033 (Hybrid Retrieval) -- Phases 1-4 merged

ADR-029 (Stream Parser) --> ADR-030 (Orchestrator) --> ADR-031
ADR-028 (Facade) --> ADR-030 --> ADR-031
ADR-034 (Multi-Agent) --> ADR-028, ADR-030
ADR-038 (Memory-first TTFC) --> ADR-029, ADR-030, ADR-033, ADR-034
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
