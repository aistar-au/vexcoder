# Active Roadmap

Single canonical source for what is active. Both `onboarding.md` Section 2b
and `TASKS/TASKS-DISPATCH-MAP.md` reference this file -- they do not duplicate it.

Updated by the merge workflow after each ADR-scoped PR lands on main.
Do not edit manually except via the standard exact-diff workflow.

Last updated: 2026-04-02 (Batches 1-6 merged in PR #315, #316, and #317; ADR amendment pack applied via PR #318)

---

## Active ADRs

| ADR | Status | Remaining items | Dependency note |
| :--- | :--- | :--- | :--- |
| ADR-021 | Accepted | 0 (all items complete) | All P1/P2/P3 items complete; see Tier 6 section |
| ADR-022 amendment | Amended | Amendment only | Tightens milestone-1 command-execution rules relative to ADR-022 |
| ADR-022 | Proposed (milestone-1 passed) | Post-milestone G/H | Roadmap; spawns ADR-023, ADR-024, ADR-027, ADR-031 |
| ADR-024 | Proposed (pre-milestone complete) | 1 item (PG-03 tap auto-dispatch deferred) | PA–PM and PP done; PG-01/PG-02/PG-03 template complete; PH-01/PH-02/PH-03 complete; PL-01 (pre/post-tool hooks, Gap 26) complete |
| ADR-028 | Active | Ongoing boundary alignment | Phase 1, 2, and transport extraction committed 2026-03-25; boundary tests now cover direct, grouped, multiline, and `super::`-relative `server`/`bin` imports for all inner layers |
| ADR-029 | Accepted (amended 2026-04-01) | 0 items remaining | All 8 decision items verified in Tier 5 (PR #249); Amendment adds StreamTextNormaliser boundary for embedded tool call markup (PR #305) |
| ADR-030 | Accepted | 0 items remaining | All 6 coverage requirements verified in Tier 5 (PR #249) |
| ADR-031 | Accepted (all batches A-E merged) | 0 items remaining | Status updated in Tier 9 (PR #252) |
| ADR-032 | Accepted | 0 items remaining | Items 1-8 complete; item 4-5 verified Tier 5; item 9 transferred to ADR-033 |
| ADR-033 | Accepted (all phases 1-4 merged) | 0 items remaining | Status updated in Tier 9 (PR #252) |
| ADR-034 | Accepted (all phases A-E + watch-stream merged) | 0 items remaining | Phase E2 watch-stream added: GET /v1/session-tasks/{id}/watch SSE with immediate snapshot + broadcast fan-out; PR #261 closes Phase E watch-stream |
| ADR-035 | Accepted | 0 items remaining | Gap 14 `/undo` rollback strategy is now specified and implemented with binary-safe checkpoints |
| ADR-038 | Accepted (Batches D-H merged) | 0 items remaining | Phase 1: bounded context cache + opt-in auto git; Phase 1a: search lane tightening; Phase 2: disk_policy.rs + config/cache.rs; Batch C: config/load.rs -> directory module (PR #279); Batch D: operator.rs -> directory module (PR #280); Batch E/F: context_assembler split + strict disk-policy gate (PR #281); Batch G: operator policy module + disk-policy wiring (PR #282); Batch H: task-state persist extraction + WAL evaluation (PR #283) |
| ADR-039 | Proposed (Batch A merged on main) | 3 batches (B-D) | Batch A status anchors and semantic color feedback merged in PR #292; search.exclude path-boundary normalization fix in PR #293; remaining work is broader vocabulary, active indicator, and paragraph-oriented progress stream without renaming machine statuses |
| ADR-043 | Proposed | 3 adoption gates | Future structured parser lane remains optional until live runtime wiring, parity coverage, and defect-reduction gates land |

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

## Remaining Work: 1 Proposed In-Tree ADR + 1 Deferred External Dependency

ADR-039 now tracks the next operator-surface lane: a neutral spatial CLI voice
for human-facing transcript text, status copy, ANSI semantic roles, and the
paragraph-oriented progress stream used during long-running tasks. Batch A is
merged on main (PR #292): `Mapping adjacent sectors...`,
`State synchronized.`, and the semantic status-color lane now land on existing
surfaces. A follow-up fix in PR #293 normalizes `search.exclude` entries with
a trailing slash so path-prefix matching enforces directory boundaries.
Remaining work extends into the wider spatial vocabulary, then adds
the active indicator, and only later consolidates the long-running paragraph
stream. ADR-038 is Accepted and
complete: context cache, disk-policy classifier, config cache, module
decompositions (config/load, operator, context_assembler, task_state), strict
policy CI gate, and operator-level durable access assertions are all in-tree.
The only deferred external follow-up remains ADR-024 PG-03 tap auto-dispatch,
which stays blocked until the separate `homebrew-vex` tap repository exists.

Fullscreen transcript-first parity hardening is active under ADR-031,
ADR-040, and ADR-041. Scope: richer footer budgeting, stronger multiline
composer ergonomics, overlay or pager detail surfaces, transient timeline
discoverability, and active or fallback fullscreen convergence without
introducing a permanent telemetry pane. Parser work remains limited to
normalisation hardening unless ADR-043 adoption gates are satisfied.

ADR-041 D5/D6/D7 (delta types, delta-native draw methods, bounded suffix
deduplication) landed in PR #331 (commit e1dd681) on 2026-04-03.
ADR-041 D8/D9/D10/D11/D12/D13 (pending-row replacement, live input preview,
ordered streamed-text segmentation, bounded-suffix reuse in
conversation streaming, accumulator drain cleanup, and chunk-safe
normalisation hardening for wrapper-tagged deltas) are in progress under
`work/vexcoder-delta-consume-switchover`.

### ~~Tier 1 -- Open PRs~~ (cleared 2026-03-27)

PRs 231, 232, 233, 234 all merged to main.

### ~~Tier 2 -- Sandbox and MCP Completion (ADR-024)~~ (cleared 2026-03-27)

PD-02, PD-03 (PR 231), PF-01, PF-02 (PR 232), and PI-06/PI-07 are complete.

### ~~Tier 3 -- Workspace Tools and MCP Extensions (ADR-024)~~ (cleared 2026-03-27)

PP-01 (`list_dir`, `glob_files`, gitignore-aware `search_files`) is complete.
PM-02 (MCP HTTP headers env-var substitution) merged in PR 236.
PI-08 (`/plan`, `/context`) merged in ADR-023 batch.

### ~~Tier 4 -- Security Hardening (ADR-021 P1)~~ (cleared 2026-03-27)

- Item 18: editor MAX_INPUT_BYTES cap in src/ui/editor/mod.rs
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
- ADR-031/ADR-032/docs: fullscreen composer auto-fit behavior documented consistently for current display row/column resize and snapped display layouts

### ~~Tier 6 -- Code Quality (ADR-021 P2)~~ (cleared 2026-03-28)

All 13 tracked items complete.

- ~~Item 9: Tool error dispatch block repeated~~ (done 2026-03-28; `emit_tool_error` helper added in core.rs)
- ~~Item 10: Scroll handling duplication~~ (done 2026-03-28; `apply_bounded_scroll` extracted; patch overlay and inspector scroll delegate to it)
- ~~Item 11: Approval input parsing duplicated~~ (addressed; `parse_approval_selection` already centralized; per-handler response logic is not reducible further without a callback interface)
- ~~Item 12: Diff row styling logic duplicated~~ (done 2026-03-28; `diff_line_color` helper centralized; both callers delegate to it)
- ~~Item 13: required_tool_string variants overlapping~~ (done 2026-03-28; `required_tool_string` delegates to `required_tool_string_any`)
- ~~Item 14: Auto-follow behavior duplication~~ (done 2026-03-28; `apply_auto_follow_or_clamp` helper extracted; both sites in model_update.rs delegate to it)
- ~~Item 15: MAX_INPUT_PANE_ROWS not applied in prod~~ (done 2026-03-28; fullscreen composer now auto-fits within the current display viewport)
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

### Tier 10 -- Memory-First TTFC Hardening (ADR-038) -- 0 items

- Phase 1 complete: bounded in-memory context snapshot cache and opt-in automatic git context merged.
- Phase 2 complete: `src/disk_policy.rs` (DiskPermission classifier) and `src/config/cache.rs` (OnceLock config cache) merged in PR #278.
- Batch C complete: `src/config/load.rs` extracted into directory module (`load/paths.rs`, `load/merge.rs`, `load/parse.rs`) in PR #279.
- Batch D complete: `src/tools/operator.rs` extracted into `src/tools/operator/{mod,core,file_ops,git_ops,search}.rs` in PR #280.
- Batch E complete: `src/runtime/context_assembler.rs` extracted into `src/runtime/context_assembler/{mod,reads}.rs` in PR #281.
- Batch F complete: `src/disk_policy.rs` gains `enforce()` / `enforce_runtime()`, `tests/disk_policy_tests.rs` adds strict/warn/off coverage, `make check-disk-policy` is wired into `arch-contracts.yml` in PR #281.
- Batch G complete: `src/tools/operator/policy.rs` wraps `disk_policy::enforce` for operator-level durable-access assertions; `TaskState::save()` and `TaskState::load()` wired through `assert_durable_access()`; cross-platform `check_path()` fix for Windows backslash separators in PR #282.
- Batch H complete: `src/runtime/task_state.rs` (807 lines) extracted into `src/runtime/task_state/{mod.rs, persist.rs}` in PR #283. WAL evaluation concluded: not warranted because task-state saves are per-session and `write_json_safe` already performs crash-safe writes (temp + fsync + rename).

#### ~~Planned remaining batches (ADR-038)~~ (all complete)

**Batch G -- operator/search policy wiring (Phase 3 completion)** -- MERGED in PR #282
- ~~Add `src/tools/operator/policy.rs` wrapper around `src/disk_policy.rs`~~ Done
- ~~Route operator file/git/search surfaces and durable search/task-state writes through declared policy checks~~ Done (task-state save/load wired)
- ~~Keep `.vex/index/` and `.vex/state/` as the only deliberate durable layers under strict mode~~ Enforced
- ~~Depends on the Batch F harness in PR #281~~ Merged

**Batch H -- task-state persist extraction + WAL evaluation** -- MERGED in PR #283
- ~~Evaluate whether `.vex/state/` writes need a write-ahead log for crash safety~~ Evaluated: not warranted (per-session saves, crash-safe writes via write_json_safe)
- ~~Extract `src/runtime/task_state/{mod.rs,persist.rs}`~~ Done (807L -> 248L mod.rs + 583L persist.rs)
- ~~Gate any WAL-backed writes behind `VEX_TASK_WAL=1` until recovery semantics are stable~~ Not needed (WAL not warranted)
- ~~Depends on Batch G completing the durable-surface inventory~~ Merged

### Tier 11 -- CLI Voice and Status Surface (ADR-039) -- 3 items

The next operator-facing lane standardizes the human-facing CLI voice
without changing machine-facing lifecycle values or diff color semantics.

**Batch A -- status anchors and semantic color feedback** -- merged on main (PR #292)
- `Mapping adjacent sectors...` is now the default human-facing in-progress
  phrase when a more specific display string is unavailable.
- `State synchronized.` now appears on human-facing completion surfaces.
- Tool-call, orchestrator, and agent-enrichment status text now use the
  deep-nebula-violet semantic lane while canonical machine lifecycle strings
  such as `completed` remain unchanged.

**Batch B -- vocabulary normalization**
- Normalize operator-facing copy to spatial terms such as `adjacent`,
  `internal`, `external`, `upper`, `lower`, and `unused` where the wording is
  display-only.
- Do not rename code symbols, persisted schema fields, or JSON payload keys.

Concrete targets (9 display-facing strings across 5 files):

| File | Count | Terms to normalize |
| :--- | :--- | :--- |
| `src/app/commands/mod.rs` | 3 | `parent=` -> `origin=` in watch lines; `branched from` -> `derived from`; `fork aborted` -> `fork halted` |
| `src/bin/vex.rs` | 2 | `parent=` -> `origin=` in session-task status lines |
| `src/app/model_update.rs` | 1 | `aborted` -> `halted` in edit loop approval denial |
| `src/app/input.rs` | 2 | `busy` -> `occupied` in turn-in-progress status lines |

Lower-priority internal-only targets (5 strings): `spawn` -> `start` in error
contexts (`src/mcp.rs`, `src/runtime/command.rs`, `src/runtime/git_snapshot.rs`);
`parent directory` -> `containing directory` (`src/server/socket.rs`, `src/util.rs`).

**Batch C -- active indicator affordance**
- Add the single pulsing-star active indicator where the renderer supports it.
- Ensure reduced-color and plain-text fallbacks remain readable.

Candidate implementation areas:

| File | Scope |
| :--- | :--- |
| `src/ui/render/mod.rs` | ratatui widget for pulsing-star glyph paired with mapping status text |
| `src/ui/draw/transcript.rs` | ANSI plain-text fallback rendering the star as a static glyph |
| `src/status_contract.rs` | `ACTIVE_INDICATOR_GLYPH` constant and accessibility fallback string |

**Batch D -- paragraph progress stream**
- Consolidate long-running tool and agent updates into one paragraph-oriented
  progress lane.
- Add active counters such as files processed and active agents where the runtime
  already knows those values.
- Keep code / diff output visually dominant over status text.

Candidate implementation areas:

| File | Scope |
| :--- | :--- |
| `src/ui/draw/transcript.rs` | Paragraph-stream layout for tool/agent updates in the ANSI renderer |
| `src/ui/render/mod.rs` | ratatui paragraph widget for orchestrator progress lane |
| `src/app/model_update.rs` | Coalesce sequential tool-status updates into a rolling paragraph |
| `src/runtime/core.rs` | Expose active file-count and active-agent-count to the UI update channel |

**Previously planned ANSI semantic-role work is now part of merged Batch A**
- Keep default transcript and code text phosphor white.
- Preserve green insertions and red deletions.
- Reserve deep nebula violet for tool-call, orchestrator, and agent-enrichment
  status text, with reduced-color fallbacks.

---

## Active Feature Branches (not yet merged)

| Task | Branch | PR | Status | Description |
| :--- | :--- | :--- | :--- | :--- |
| EL-extract | `work/vexcoder-edit-loop-tui-extract` | #311 | Draft PR, CI green | Extract oversized edit-loop/TUI modules into path-based submodules; Windows command-cancellation fix |
| Batch-3-4 | `work/vexcoder-batch3-overlay-detail` | #316 | **Merged** | Browse cues, follow-mode fix, nextest cleanup, timeline discoverability |
| Batch-5-6 | `work/vexcoder-batch5-overlay-convergence` | #317 | **Merged** | CLI resize notice, inspector row-count title, parser/normaliser hardening fixtures |
| ADR-amendments | `work/vexcoder-adr-amendments` | #318 | Merged | ADR-043 consequences, ACTIVE-ROADMAP parity lane summary |
| ADR-038-EF | `work/vexcoder-adr-038-reads-and-policy-gate` | #281 | **Merged** | `context_assembler/{mod,reads}.rs` split plus strict disk-policy test/CI gate for ADR-038 Batches E/F |
| ADR-038-G | `work/vexcoder-adr-038-operator-policy-wiring` | #282 | **Merged** | Operator policy module and disk-policy wiring into task-state I/O (ADR-038 Batch G) |
| ADR-038-H | `work/vexcoder-adr-038-task-state-persist` | #283 | **Merged** | Task-state persist extraction + WAL evaluation (ADR-038 Batch H) |
| PL-01-ext | `work/vexcoder-http-hooks` | #270 | **Merged** | HTTP webhook support for tool events (`[[http_hooks]]` config section) |
| PM-01 | `work/vexcoder-conversation-compaction` | #271 | Implementation complete, draft PR | In-memory summarization of older turns when token count exceeds threshold |
| PM-02 | `work/vexcoder-undo-checkpoints` | #272 | Implementation complete, draft PR | `/undo` slash command and per-change checkpoint stack |
| PM-03 | `work/vexcoder-code-search` | #273 | Implementation complete, draft PR | Code search hardening and `/reindex` command |
| PM-04 | `work/vexcoder-auto-memory` | #274 | Implementation complete, draft PR | Automatic extraction of memory-worthy facts from conversation turns |
| ADR-041-D8D13 | `work/vexcoder-delta-consume-switchover` | #332 | Open PR | Pending-row replacement, live input preview, ordered streamed-text segmentation, bounded-suffix streaming reuse, delta accumulator drain activation, and chunk-safe wrapper-tag normalisation for the transcript-first path post PR #331 |

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
ADR-039 (CLI voice) --> ADR-023, ADR-030, ADR-031, ADR-034
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
