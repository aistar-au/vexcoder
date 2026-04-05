# ADR-021: Codebase Audit — Unused Code, Duplication, and Shared-Code Opportunities

- **Status**: Accepted (P0 items 1–4 implemented)
- **Date**: 2026-02-22
- **Context**: Consolidated static review of the current `main` codebase.
- **Goal**: Reduce maintenance drag, align production behavior with tested behavior, and remove duplicated control flow.

## Accuracy Review of Submitted Findings

This ADR records the submitted audit with verification against the current tree.
Items are marked as:

- **Confirmed**: validated directly in code.
- **Partially accurate**: valid concern but scope/details need correction.
- **Not accurate (current tree)**: claim does not hold for current `main`.
- **Pending deep audit**: plausible, but not fully validated in this pass.

## P0 — Fix Now

### 1) `InputEditor` test-only coverage vs production behavior
- **Status**: **Completed (2026-02-22)**
- **Evidence**:
  - Added shared production editor module: `src/ui/editor/mod.rs`.
  - Exported editor module from `src/ui.rs`.
  - `src/bin/vex.rs` now owns `InputEditor` and delegates editing/submit actions through `InputAction`.
  - Removed test-only editor implementation duplication from `src/app.rs`.

### 2) `submit_input()` trims leading whitespace
- **Status**: **Completed (2026-02-22)**
- **Evidence**:
  - Shared submit path now uses:
    - `trim_end_matches('\n')`
    - `trim_end_matches('\r')`
  - Leading whitespace is preserved for submitted prompts.

### 3) Scroll metrics mismatch with wrapped rendering
- **Status**: **Completed (2026-02-22)**
- **Evidence**:
  - `src/ui/render/mod.rs` now computes visual line count with wrapping:
    `history_visual_line_count(messages, content_width)`.
  - Added shared width helper: `history_content_width_for_area(messages, area)`.
  - `src/app.rs` uses width-aware count for `status_line()` and viewport calculations via `expand_rows_for_display()`.
  - `src/bin/vex.rs` updates `TuiMode` history content width each frame before render.

### 4) UTF-8 cursor logic duplicated across test/prod editors
- **Status**: **Completed (2026-02-22)**
- **Evidence**:
  - Consolidated UTF-8 cursor and edit operations into `src/ui/editor/mod.rs`.
  - Removed duplicate cursor/edit implementations from `src/bin/vex.rs`.
  - `src/app.rs` tests now import and exercise the shared editor module.

## P0 Implementation Delta (Insertions/Deletions)

Measured with:

```bash
git add -N src/ui/editor/mod.rs
git diff --numstat -- src/ui/editor/mod.rs src/app.rs src/bin/vex.rs src/ui.rs src/ui/render/mod.rs
```

| File | Insertions | Deletions |
| :--- | ---: | ---: |
| `src/ui/editor/mod.rs` | 274 | 0 |
| `src/app.rs` | 14 | 258 |
| `src/bin/vex.rs` | 45 | 139 |
| `src/ui.rs` | 1 | 0 |
| `src/ui/render/mod.rs` | 25 | 8 |
| **Total** | **359** | **405** |

## P1 — Unused Code / Cleanup Claims

### 5) `execute_tool_blocking_with_operator` unused wrapper
- **Status**: **Not accurate (current tree)**
- **Correction**:
  - Non-test wrapper is used by production execution path from
    `execute_tool_with_timeout` in `src/state/conversation/tools/mod.rs`.

### 6) `looks_like_terminal_transcript` family likely bypassed
- **Status**: **Not accurate (current tree)**
- **Correction**:
  - Functions are active and used in production path:
    `src/bin/vex.rs:101` and `src/bin/vex.rs:113`.

### 7) Empty `on_model_update` in `runtime/loop.rs` as unreachable production code
- **Status**: **Not accurate (current tree)**
- **Correction**:
  - Empty implementation is in test-only `InterruptMode` under `#[cfg(test)]`
    in `src/runtime/loop.rs`.

### 8) Post-cutover comment debt
- **Status**: **Partially accurate**
- **Note**:
  - There are transition-era comments; triage should separate stale comments
    from still-useful rationale.

## P2 — Live Duplication Claims

### 9) Tool error dispatch block repeated in conversation core
- **Status**: **Completed (2026-03-28)**
- **Note**:
  - `emit_tool_error` helper added to `src/state/conversation/core.rs`;
    consolidates the repeated emit/truncate/push tail across the four
    sequential-path guard branches and the denial branch (5 call sites).

### 10) Scroll handling duplication in app state
- **Status**: **Confirmed**
- **Evidence**:
  - Repeated line/page/home/end patterns in `src/app.rs`.

### 11) Approval input parsing duplicated
- **Status**: **Confirmed**
- **Evidence**:
  - `handle_approval_input` and `handle_patch_overlay_input` in `src/app.rs`.

### 12) Diff row styling logic duplicated
- **Status**: **Confirmed**
- **Evidence**:
  - `history_row_style` and `styled_diff_line` in `src/ui/render/mod.rs`.

### 13) `required_tool_string*` variants are mostly overlapping
- **Status**: **Completed (2026-03-28)**
- **Evidence**:
  - `required_tool_string` now delegates to `required_tool_string_any` with a
    single-key slice, removing the duplicated get/trim/bail logic.
  - Both functions trim via `str::trim`; consolidation is safe.

### 14) Auto-follow reconciliation repeated outside shared helper
- **Status**: **Confirmed**
- **Evidence**:
  - `on_model_update` branches in `src/app.rs` repeat follow/clamp behavior.

### 15) `MAX_INPUT_PANE_ROWS` not applied in production path
- **Status**: **Completed (2026-03-28)**
- **Evidence**:
  - `src/ui/layout.rs` exports `MAX_INPUT_PANE_ROWS` for shared production use.
  - `src/tui_frontend.rs` clamps the current composer against that shared cap.
  - `src/ui/render/mod.rs` recomputes composer height
    from wrapped content and current display geometry, so the fullscreen surface
    auto-fits to row and column changes instead of reserving a fixed prompt
    block.

## P3 — Architectural Opportunities

The following are design proposals and were not evaluated as strict true/false
bugs in this pass:

- Extract `send_message` in `src/state/conversation/core.rs`.
- Promote editor into production module (e.g., `src/ui/editor/mod.rs`).
- Unify scroll behavior behind shared abstraction.
- Introduce shared approval parser helper.
- Centralize tool metadata (`ToolKind`/registry approach).
- Separate structured/text protocol output strategies.
- Increase use of `StatefulWidget`-style encapsulation for UI state.
- Split large app module for navigation ergonomics.
- Expand `src/util.rs` for repeated parsing/truncation helpers.
- Add stronger `src/test_support.rs` harness helpers.
- Move prompt/schema blobs out of `src/api/client/mod.rs` where practical.

## External Audit Follow-up (2026-02-22)

This section triages the externally submitted debugging report against the
current tree.

### 16) “Runtime not wired after REF-08” in `src/bin/vex.rs`
- **Status**: **Not accurate (current tree)**
- **Evidence**:
  - `src/bin/vex.rs` uses `#[tokio::main]`.
  - `main` constructs runtime/context via `build_runtime(config)?`.
  - `runtime.run(&mut frontend, &mut ctx).await` is executed.
- **Note**:
  - This was a historical issue reflected in older review text, now resolved.

### 17) Unconditional redraw loop / hot idle rendering
- **Status**: **Completed (2026-02-22)**
- **Evidence**:
  - Added runtime render scheduling controls:
    - `IDLE_RENDER_TICK` at `src/runtime/loop.rs:12`
    - `IDLE_LOOP_BACKOFF` at `src/runtime/loop.rs:13`
  - Updated `Runtime::run` to render only when first frame, state change, or
    tick due (`src/runtime/loop.rs:24`, `src/runtime/loop.rs:35`,
    `src/runtime/loop.rs:46`, `src/runtime/loop.rs:48`).
  - Added idle-regression test:
    `test_render_is_tick_or_state_driven_when_idle`
    (`src/runtime/loop.rs:242`).
- **Priority**: **P0**
- **Implementation (short)**:
  - Replaced unconditional per-iteration draw with tick/state-driven draw and
    idle backoff sleep to avoid hot idle redraw loops while preserving first
    frame + responsive updates.

### 17.a) P0.17 Delta (Insertions/Deletions)

Measured with:

```bash
git diff --numstat -- src/runtime/loop.rs
```

| File | Insertions | Deletions |
| :--- | ---: | ---: |
| `src/runtime/loop.rs` | 80 | 9 |
| **Total** | **80** | **9** |

### 18) Unbounded input buffer in production editor
- **Status**: **Confirmed**
- **Evidence**:
  - `src/ui/editor/mod.rs::insert_str` appends without size cap.
  - Large paste input can grow buffer unbounded.
- **Priority**: **P1**
- **Follow-up**:
  - Add max input length cap (configurable/default bounded).

### 19) SSE parse failures are logged but not surfaced to UI
- **Status**: **Confirmed**
- **Evidence**:
  - `src/api/stream.rs` logs parse failures via `emit_sse_parse_error(...)`.
  - No parse-error event is emitted into `ConversationStreamUpdate`/`UiUpdate`;
    UI may only observe a stalled turn.
- **Priority**: **P1**
- **Follow-up**:
  - Add explicit parse-error propagation path to `UiUpdate::Error`.

### 20) `edit_file` race condition (read-modify-write window)
- **Status**: **Completed (2026-03-28)**
- **Evidence**:
  - `src/tools/operator/file_ops.rs::edit_file` performs read/validate/write sequence.
  - A concurrent external writer can race between read and write.
- **Risk posture**:
  - Acceptable for current single-user local-agent target, but still a known
    TOCTOU class risk.
- **Priority**: **P2**
- **Follow-up**:
  - Structured comment added above the read-validate-write sequence documenting
    the TOCTOU risk and the advisory-lock/O_EXCL path to take if multi-writer
    scenarios are added.

### 21) “Unhandled `git` command panics” in `ToolOperator::run_git`
- **Status**: **Not accurate (current tree)**
- **Evidence**:
  - `src/tools/operator/git_ops.rs::run_git` uses:
    `Command::new("git").current_dir(...).args(...).output().context(...) ?`.
  - Spawn/exec failures are returned as `anyhow::Error`; they do not panic.
- **Priority**: **N/A**

### 22) `StreamBlock::ToolCall` deltas ignored in `TuiMode::on_model_update`
- **Status**: **Completed (2026-03-28)**
- **Evidence**:
  - `src/app/model_update.rs` now accumulates `StreamBlockDelta` payloads for
    `StreamBlock::ToolCall` into a turn-scoped `tool_input_raw_buffers` map.
  - While the accumulated JSON is incomplete, `preview_partial_tool_input`
    surfaces the streamed fragment in the pending tool preview.
  - Once the payload parses as valid JSON, the pending tool call's `input` and
    `input_preview` are replaced with the parsed structure.
  - Buffers are cleared on block completion, turn end, and error paths.
  - Regression test in `src/app/tests/model_turn.rs` covers the partial and
    complete streaming scenarios.

### 23) UTF-8 safety concern in `strip_incomplete_tool_tag_suffix`
- **Status**: **Not accurate (current tree)**
- **Evidence**:
  - `rfind('<')` returns the byte index of ASCII `<`, which is always a valid
    UTF-8 scalar boundary.
  - `truncate(last_open)` therefore truncates on a valid boundary.
- **Priority**: **N/A**

### 24) Startup event draining and paste-noise heuristics
- **Status**: **Completed (2026-03-28)**
- **Evidence**:
  - `should_ignore_startup_paste` and `should_ignore_startup_submission` in
    `src/tui_frontend.rs` are heuristic/best-effort filters.
  - `startup_filter_active()` helper added; reads `VEX_DISABLE_STARTUP_FILTER=1`
    env var to disable both filters for observability.
- **Priority**: **P2**

### 25) Late `StreamDelta` dropped when no active turn slot
- **Status**: **Completed (2026-03-28)**
- **Evidence**:
  - `src/app/model_update.rs` drops `UiUpdate::StreamDelta` when
    `active_assistant_index` is `None` and `turn_in_progress` is false.
  - This is intentional stale-delta protection after cancel/complete, but can
    discard straggler data from delayed streams.
- **Priority**: **P2**
- **Follow-up**:
  - Guard is preserved; debug observability added under `#[cfg(debug_assertions)]`
    that emits a line to stderr when a stale delta is dropped.
- **ADR note**:
  - This is intentional stale-delta protection, so it is not treated
    as a hard ADR-009 contract break.

### 26) SSE parser buffer can grow unbounded without frame delimiter
- **Status**: **Confirmed**
- **Evidence**:
  - `src/api/stream.rs::StreamParser::process` appends chunk data into
    `self.buffer`.
  - Drain only occurs when `\n\n` frame delimiters are found (`start > 0`).
  - A malformed upstream stream with no delimiters can grow buffer
    indefinitely.
- **Priority**: **P1**
- **Follow-up**:
  - Add a bounded buffer cap (e.g., `MAX_SSE_BUFFER_CHARS`) and fail fast with
    a surfaced `UiUpdate::Error` path when exceeded.

### 27) `active_assistant_index` drift race claim during history cap
- **Status**: **Not accurate (current tree)**
- **Evidence**:
  - UI updates are processed sequentially in the runtime loop (single consumer
    of `UiUpdate` stream), not concurrently mutating `TuiMode`.
  - `enforce_history_cap` uses `checked_sub` for index rebasing.
  - Existing regression tests validate index safety under cap+stream flow.
- **Priority**: **N/A**
- **Note**:
  - Related stale-delta behavior remains tracked in item 25.

### 28) Read-only intent heuristic can produce false positives
- **Status**: **Completed (2026-03-28)**
- **Evidence**:
  - `src/state/conversation/tools/mod.rs::is_read_only_user_request` uses keyword
    heuristics over read-only/mutating hint sets.
  - Mixed-intent prompts can still be misclassified despite mutating hint
    checks.
- **Priority**: **P2**
- **Follow-up**:
  - Guard is preserved as default safety net; `VEX_FORCE_MUTATING_TURN=1` env
    override added so operators can bypass the heuristic when needed.
  - Unit test `test_vex_force_mutating_turn_overrides_heuristic` added to
    `src/state/conversation/tools/mod.rs`.

### 29) `append_incremental_suffix` overlap algorithm cost on large deltas
- **Status**: **Completed (2026-02-24 via PR #16 / run-2026-02-24-040000)**
- **Evidence**:
  - `src/state/conversation/streaming.rs::append_incremental_suffix` no longer
    performs repeated overlap scans.
  - Logic now keeps only safe cumulative/prefix checks and appends other deltas
    as-is, closing both correctness and overlap-scan concerns.
- **Priority**: **Closed**

### 30) `RuntimeContext` `RwLock` optimization suggestion
- **Status**: **Not accurate (current tree)**
- **Evidence**:
  - `ConversationManager` work is mutation-heavy per turn; lock is held for
    send/stream orchestration by design.
  - `RwLock` does not meaningfully improve current single active-turn model.
- **Priority**: **N/A**

## Additional Triage from Latest ADR-022 Draft (Critical First)

### 31) UTF-8 fragmentation risk in SSE chunk ingestion (`from_utf8_lossy` per chunk)
- **Status**: **Completed (2026-02-23 via PR #14 / run-2026-02-23-162456)**
- **Evidence**:
  - `src/api/stream.rs::StreamParser` now buffers raw bytes (`Vec<u8>`) and
    appends incoming chunks via `extend_from_slice`.
  - Frame delimiter detection occurs on bytes; decoding is deferred until frame
    extraction, removing per-chunk lossy conversion.
- **Priority**: **Closed**

### 32) `KeyEventKind::Release` filtering portability concern
- **Status**: **Completed (earlier; confirmed 2026-03-28)**
- **Evidence**:
  - Filter is present in `src/tui_frontend.rs` at line 736:
    `if key.kind == KeyEventKind::Release { return None; }`.
  - ADR-021 item referenced `src/bin/vex.rs` but the current code is in
    `src/tui_frontend.rs`; behavior is correct.
  Behavior can vary by cli/backend; current filtering does not
    imply guaranteed double-processing by itself.
- **Priority**: **P2** (closed)
- **Follow-up**:
  - Keep current behavior; add backend-specific regression coverage if field
    reports show duplicate/missed key handling.

### 33) Idle backoff tuning claim (`IDLE_LOOP_BACKOFF=4ms`)
- **Status**: **Completed (2026-03-28)**
- **Evidence**:
  - Runtime now uses tick/state-driven rendering (P0.17 complete).
  - Frontend polling in `src/tui_frontend.rs` is now adaptive: 1ms during
    an active model turn (streaming) and 16ms when idle. The practical idle
    loop rate is around 62Hz; during streaming the shorter timeout reduces
    per-iteration latency.
- **Priority**: **P3** (closed)
- **Follow-up**:
  - Tuning comment added to `IDLE_LOOP_BACKOFF` in `src/runtime/loop.rs`
    noting the 62Hz cap and the requirement to profile before reducing further.

### 34) Non-sequential block-index divergence / out-of-bounds claim
- **Status**: **Not accurate (current tree)**
- **Evidence**:
  - `upsert_turn_block` pads missing indices before insert/update, preventing
    direct out-of-bounds writes in normal flow.
  - Runtime block-text tracking uses index-keyed map and tolerates sparse keys.
- **Priority**: **N/A**

### 35) `src/runtime.rs` + `src/runtime/mod.rs` dual-entry claim
- **Status**: **Not accurate (current tree)**
- **Evidence**:
  - Repository has `src/runtime.rs` and nested entries under `src/runtime/*`.
  - `src/runtime/mod.rs` does not exist.
- **Priority**: **N/A**

## Promotion Traceability (2026-02-23 to 2026-02-24)

- PR #14 (`run-2026-02-23-162456`, merge `15f7ffd`): closed item 31 (UTF-8
  fragmentation risk) and advanced SSE parser hardening.
- PR #15 (`run-2026-02-24-030001`, merge `43506f1`): restored remote
  `read_file` content visibility in model context
  (`src/state/conversation/history.rs`), addressing a critical reliability gap
  discovered during follow-up audits.
- PR #16 (`run-2026-02-24-040000`, merge `9eda7cc`): closed item 29 by
  replacing destructive overlap stripping in stream delta dedupe.

## Immediate Dispatch Recommendation

1. Keep P0 items 1–4 and P0.17 closed.
2. Address remaining P1 items: P1.18, P1.19, and P1.26.
3. Treat items 21, 23, 27, 30, 34, and 35 as closed (`not accurate`).
4. Keep items 22, 24, 25, 28, and 32 as P2 design/observability follow-ups.
5. Keep item 33 as a P3 optimization/tuning candidate.
6. Continue P2/P3 refactors in ADR-backed, test-gated batches.

## Validation Commands

```bash
cargo check
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
```
