# ADR-041: Transcript Renderer Wiring and Compact Tool Paragraphs

- **Status:** Accepted
- **Date:** 2026-04-01
- **Deciders:** Core maintainer
- **Depends on:** ADR-029, ADR-030, ADR-031, ADR-040
- **Deprecates:** None
- **Deprecated by:** None

## Context

A live session recording (tui3-session.log) exposed several rendering
defects in the TUI transcript pipeline:

1. **Normaliser state leak across streaming rounds** — When the LLM
   produces text with embedded `function=<name>` markup that has a
   missing or malformed close tag, the `StreamTextNormaliser` enters
   `in_tool_block` mode with an active `current_param_name`.  All
   subsequent text deltas (including the final model response after
   tool execution) get accumulated into `current_param_value` and
   never reach the transcript pane.  The telemetry line confirms
   357 generated tokens that were never displayed.

2. **Tool paragraph verbosity** — Each `[tool]` block emitted 5-8
   rows (header + Scope + Command + Input + Result + up to 6 evidence
   lines + overflow).  With multiple tool retry attempts this fills
   the entire viewport, pushing the prompt area off-screen.

3. **Telemetry label verbosity** — The older `read:` and `generate:`
   segment labels in the turn-timing summary consume horizontal space
   without adding clarity for operators who run dozens of turns.

## Decision

### D1: Auto-close stale tool blocks in the normaliser

When `normalise()` encounters a new `function=<name>` open tag while
already inside a tool block, it flushes the stale block (draining any
pending parameter value as a `[detail]` transcript line) before
entering the new block.

A new `flush()` method drains pending state and is called by
`forward_conversation_update()` whenever a textual `BlockStart`
(FinalText or Thinking) arrives, ensuring the normaliser is clean
before the model's follow-up response text begins streaming.

### D2: Compact tool paragraphs

`pending_tool_paragraph_rows()` now emits only the `[tool]` header
line plus a single `[detail] Input:` line (no Scope or Command rows).

`completed_tool_paragraph_rows()` emits the header, Input, Result,
and at most 3 evidence lines (reduced from 6) plus an overflow
indicator.  This keeps each tool call to 5-6 rows maximum instead of
10+.

### D3: Arrow telemetry labels

The turn-timing summary replaces the `read:` label with `↑:` and
`generate:` with `↓:`.  Both the generation path (`append_turn_timing_line`)
and the rendering paths (`draw_telemetry_summary`,
`draw_inline_telemetry_summary`, `format_waiting_status`) are updated.

### D4: Overflow escalation to detail surfaces

Compact tool paragraphs remain the default transcript representation. When
diff output, tool evidence, approval detail, or inspector content exceeds the
inline budget, the transcript keeps the compact summary and exposes a stable
detail target through an overlay, pager, or inspector surface instead of
appending unlimited inline rows.

The transcript therefore remains readable under repeated tool retries, large
diffs, and long command output while preserving access to full detail on
demand.

## Consequences

- Model response text after tool execution is now reliably displayed
  even when inline tool markup has malformed close tags.
- Tool call sections consume roughly half the vertical space they did
  before, keeping the prompt area visible during multi-tool turns.
- The telemetry line is more compact; `↑`/`↓` are standard Unicode
  arrows supported by all CLI hosts that support the existing
  braille spinner characters.
- Snapshot and unit tests updated to reflect the new evidence-line
  count (3 instead of 6) and arrow labels.

## Risks

- If a model intentionally spans tool blocks across multiple streaming
  rounds (unlikely — inline markup is per-message), the auto-close
  would prematurely terminate the block.  Mitigated: structured block
  protocol (chat-compat tool_calls) is the primary path for
  multi-round tool execution and is unaffected.
- The arrow symbols (`↑`/`↓`) may be unfamiliar to new users.
  Mitigated: the colon-separated `label:value` format is preserved,
  and the symbols are self-documenting as directional indicators.

---

## Amendment — 2026-04-03: Delta-native rendering foundation

### D5: Structured transcript delta types

`TranscriptDelta` and `TranscriptBlockKind` types are added in
`src/state/transcript_delta.rs` alongside a streaming block buffer that
uses bounded suffix comparison — O(new_text) instead of
O(total_content) — to deduplicate cumulative streaming updates.

Accumulators are keyed by block index in TuiMode and are created on
`StreamBlockStart`, fed on `StreamBlockDelta`, and completed/removed
on `StreamBlockComplete`. This runs in parallel with the existing
prefix-marker line path so that both rendering strategies coexist.

### D6: Delta-native render methods

The ratatui render module's `render_task_layout()` in `src/ui/render/mod.rs`
is the sole rendering path for the task surface after the cutover in PR 347.
It reads from `TaskLayoutState.output_rows` (populated by
`task_output_view_with()` in `src/app/layout.rs`).

`expand_rows_for_display()` in `src/ui/render/transcript.rs` applies
word-wrap and structural detection for all row kinds before rendering.

The earlier D6 delta-consumer helper design was superseded by the
ratatui-native cutover (PR 347); that staged helper set was not carried
forward into `src/ui/render/`.

### D7: Bounded suffix deduplication

`bounded_incremental_suffix()` is a performance-optimised alternative
to the existing `append_incremental_suffix()`. It compares only the
prefix window up to `existing.len()` bytes rather than scanning the
full accumulated content on every chunk.

---

## Amendment — 2026-05-01: Pending-row replacement, live input preview, and sequenced streamed text

### D8: Pending transcript row replacement on block completion

`PendingTurnToolCall` gains two fields — `transcript_row_start: usize`
and `transcript_row_count: usize` — that record the half-open range
`[start, start + count)` of rows the pending handler wrote into
`history_state.lines` at `StreamBlockStart(ToolCall)`.

When `StreamBlockStart(ToolResult)` arrives for that call, the handler
drains those exact rows from the lines buffer before writing the
completed-paragraph rows. A follow-up loop adjusts `transcript_row_start`
for any other in-flight pending calls whose rows sit after the drained
range, keeping multi-call concurrent state consistent.

This eliminates the stale "triple Input" display regression where
pending rows and completed rows both appeared for the same tool call,
multiplied across concurrent same-name invocations.

**Contract invariant:** the drain happens strictly before
`completed_tool_paragraph_rows()` is called, so completed rows always
land cleanly at the end of the buffer with no interleaved stale rows.

### D9: Live input preview via bounded-suffix JSON parsing

`StreamBlockDelta(ToolCall)` now reparses the pending input preview on
every chunk and replaces the full pending paragraph row range in
`history_state.lines` rather than updating only the trailing detail row.

While the accumulated JSON is incomplete, `preview_partial_tool_input`
surfaces the streamed fragment in the pending tool preview.
Once the buffered JSON parses, `preview_tool_input` is used with the
structured style and both `input` and `input_preview` are replaced with
the parsed structure.

Because the pending paragraph can grow or shrink as the preview format
changes, the handler also updates `transcript_row_count` for the active
call and shifts `transcript_row_start` for any later in-flight pending
calls whose row ranges sit after the replaced block.

This keeps the operator-facing transcript aligned with the tool call's
true streamed input state without waiting for `ToolResult`, even when
the preview spans multiple lines.

### D10: Flush completed streamed text segments at non-text boundaries

When structured block streaming is active, revised `StreamDelta`
chunks remain the only visible assistant-text source, but they are no
longer rendered as one monolithic trailing `current_turn_response`
blob. `push_history_line()` materializes the current streamed text
segment into `history_state.lines` before any non-text transcript row
is appended, and `commit_completed_turn()` drains any final in-flight
segment before turn persistence.

`transcript_display_rows()` therefore appends only the currently active
segment as the live cursor row. This preserves transcript order around
tool-call paragraphs and other structured boundaries without
introducing a second renderer for assistant text.

### D11: Reuse bounded suffix extraction in conversation streaming

`src/state/conversation/streaming.rs` now routes
`append_incremental_suffix()` through
`crate::state::transcript_delta::bounded_incremental_suffix()`.

This keeps cumulative model-runtime text updates on the same bounded
O(new_text) suffix path used by the structured transcript delta
accumulators, avoiding repeated scans across the full accumulated
buffer on every chunk.

---

## Amendment — 2026-05-02: Delta-native path activation and accumulator drain

### D12: Accumulator drain at block completion, tests, and bounded-suffix cleanup

`StreamBlockComplete` removes the associated `StreamingBlockBuffer` from the
map.  The buffer's content is not drained on completion because the parallel
rendering paths (D9 live input preview for ToolCall, D10 streamed text
segments for FinalText/Thinking) have already materialised the same content
into `history_state`.

`bounded_incremental_suffix()` is covered by inline tests in
`src/state/transcript_delta.rs`. It routes through production code in
`src/state/conversation/streaming.rs`, so no suppression annotation is needed.

The buffer's `content()` and `kind()` methods are used in production:
- `transcript_display_rows()` calls `kind()` to gate the streaming cursor —
  the `▌` character is shown only while a FinalText or Thinking buffer remains
  in the map (i.e. while a textual block is in-flight).
- `task_output_view_with()` calls `content().len()` on all active buffers to
  populate the "Transcript · Nb" live-throughput indicator in the pane title
  during structured streaming.

See Amendment D16 for the streaming block buffer rename.

---

## Amendment — 2026-04-03: Chunk-safe tagged-markup buffering and wrapper stripping

### D13: Buffer chunk-split tool markup at the transcript boundary

`StreamTextNormaliser` now treats the incoming SSE text stream as an
incremental byte sequence rather than a newline-only parser surface.
Partial `<tool_call>`, `<function=...>`, and `<parameter=...>` control
fragments stay in the internal `pending` buffer until they become
complete enough to classify as transcript control or plain assistant
text.

Complete wrapper markers (`<tool_call>`, `</tool_call>`) are
suppressed, complete function and parameter tags can be consumed even
when they arrive as standalone deltas without trailing newlines, and a
new function opener encountered mid-parameter auto-closes the previous
tool block before entering the new one.

This keeps the transcript-first live path aligned with the model runtime's
JSON delta stream instead of leaking raw line fragments into the
operator surface when the model server splits tool markup across
arbitrary chunk boundaries.

### D14: Strip wrapper-only remnants from the assistant text fallback

`rewrite_assistant_text()` now removes `<tool_call>` wrappers and
their incomplete suffixes in addition to the existing
`<function=...>`/`<parameter=...>` cleanup. The fallback assistant
history therefore preserves the tagged tool-call protocol where
required for the next round, while dropping wrapper-only transport
noise from the visible transcript and persisted assistant text.

Focused regression coverage now includes:
- chunk-split `<tool_call>` + `<function=...>` streams at the
  normaliser boundary
- wrapper stripping in the assistant-text sanitiser
- wrapper-tagged text-protocol tool rounds that still execute and
  append `tool_result` context for the next turn

## Amendment — 2026-05-04: Word-wrap and display-row expansion

### D15: Pre-expand logical rows into display-bounded rows before viewport calculation

`expand_rows_for_display(rows, cols)` converts the logical `output_rows`
slice into a display-bounded `Vec<String>` before the viewport window is
computed.  Each plain-prose row whose display width exceeds `cols` is
broken at word boundaries by `word_wrap_plain_row`.  Structural transcript
markers — bracket-delimited control lines (`[tool:...]`, `[thinking]`,
etc.), four-space and six-space disclosure indents, icon-prefixed lines
(two-space prefix), code fences, horizontal rules, blockquotes, telemetry
summaries, and markdown headers — pass through unchanged because
`draw_transcript_line` handles their own per-category truncation.

`transcript_window_rows(total, anchor, scroll_offset, viewport_height)`
replaces the direct access to `state.output_rows.len()` in the viewport
calculation so both `draw_transcript_full` and `draw_transcript_incremental`
use the post-expansion row count.  The scroll indicator and
`output_lines_flushed` are likewise updated to reflect display row counts.

A backward-compatible `transcript_window(state, viewport_height)` wrapper
delegates to `transcript_window_rows` so existing scroll-window tests
continue to exercise the same logic.

The root cause this resolves: long model responses emitted without embedded
newlines produced a single logical row that was silently truncated to the
display width by `truncate_to_width` inside `draw_inline_markdown`, making
the remainder of the response invisible and preventing upward scrolling past
the truncated row.

Test coverage:
- `word_wrap_plain_row_passthrough_for_short_line` — short lines unchanged
- `word_wrap_plain_row_passthrough_for_empty` — empty line unchanged
- `word_wrap_plain_row_skips_bracket_marker` — structural marker passes through
- `word_wrap_plain_row_skips_disclosure_indent` — indented lines pass through
- `word_wrap_plain_row_wraps_long_prose` — long prose splits to ≤ cols width
- `word_wrap_to_cols_splits_at_boundary` — word-boundary split and reconstruction
- `word_wrap_to_cols_handles_single_long_word` — oversized words are truncated
- `expand_rows_for_display_wraps_long_plain_rows` — long rows produce multiple display rows
- `expand_rows_for_display_leaves_structural_rows_intact` — structural rows unaffected
- `transcript_window_rows_bottom_anchor_scrolled` — correct viewport window at offset
- `transcript_window_rows_bottom_anchor_at_tail` — last-page clamping
- `transcript_window_rows_top_anchor_clamps_to_six` — inspector anchor uses six-row window

---

## Amendment — 2026-05-04: StreamingBlockBuffer rename and unused-code elimination

### D16: Rename the accumulator type to StreamingBlockBuffer, remove staged infrastructure

The older accumulator type is renamed to `StreamingBlockBuffer` — a name that
communicates its role: accumulating the live text of a single streaming
block for the transcript render path.

Simultaneously, all staged-but-not-yet-wired infrastructure is removed to
eliminate unused-code suppressions:

- `TranscriptDelta`, `flush_pending()`, the `VecDeque<TranscriptDelta>`
  pending queue, `last_emitted_len`, and `set_block_kind()` are deleted.
  These existed to drive a delta-extraction pipeline that was never
  connected to any consumer in production code.

- `StreamingBlockBuffer` is simplified to `content: String` +
  `kind: TranscriptBlockKind` with `new()`, `append_delta()`,
  `content()`, and `kind()` as its public surface.

- The temporary lint-suppression annotation on `TranscriptDelta` and the
  `#[allow(unused)]` annotations on `content()` and `set_block_kind()` are
  removed together with the unused-code warnings they were suppressing.

Production wiring of `content()` and `kind()`:
- `transcript_display_rows()` reads `kind()` from all active buffers to
  gate the streaming cursor (▌) on the presence of a FinalText or
  Thinking block in `delta_accumulators`.
- `task_output_view_with()` reads `content().len()` from all active
  buffers and shows "Transcript · Nb" in the output pane title when
  bytes are being received during structured streaming.

Additional unused-code cleanup in the same pass:
- `#[allow(unused)]` removed from `ChatCompatChunk`, `ChatCompatChoice`,
  `ChatCompatDelta`, `ChatCompatToolCallDelta` in `src/api/stream.rs` —
  all fields are used by the chat-compat conversion logic.
- `estimate_history_tokens`, `should_compact_proactively`, and
  `compact_with_summary` in `src/state/conversation/history.rs` converted
  to `#[cfg(test)]` — they are fully-tested compaction helpers not yet
  wired into the production flow.
- `execute_tool_with_timeout` in `tools/mod.rs` and
  `execute_tool_dispatch` in `tools/dispatch.rs` converted to
  `#[cfg(test)]` — simplifiedshims used only by unit tests.

---

## Superseded amendment — 2026-04-08: Host scrollback sink and live viewport technical cutover

This amendment is retained as rejected design history only. A 2026-04-09
follow-up reversed the host-owned scrollback direction after the split
introduced double-writer transcript state, raw tagged-tool leakage risk, and
width/reflow complexity with no operator-surface benefit. D15 remains the main
transcript path: the active architecture keeps one owned transcript surface in
the task layout and sanitises textual block deltas before they mutate UI state.

### D17: Host scrollback sink abstraction

Introduce `HostScrollbackSink` as the abstraction for committed transcript
insertion into host scrollback. The sink accepts fully-wrapped committed
rows and writes them above the reserved live viewport using the active
`HostInsertMode`:

The preferred implementation path is ratatui-native. The current tree already
pins `ratatui = 0.30`, whose `Viewport::Inline(..)`,
`try_init_with_options(..)`, and `insert_before(..)` APIs map
directly onto this contract. `HostScrollbackSink` should therefore be a thin
app-local wrapper over the ratatui API, not a parallel bespoke
renderer.

- `ScrollRegionInsert` — preferred; uses the ratatui inline viewport path and
  `insert_before(..)`. When ratatui's `scrolling-regions` feature
  is enabled, that API uses backend scroll-region insertion above the
  reserved viewport without disturbing the live tail.
- `BottomNewlineFallback` — when scroll-region insertion is unavailable,
  committed lines are flushed via ratatui's non-scrolling-region insertion
  path or explicit newline writes that scroll the host naturally.
- `OwnedTranscriptFallback` — when neither host insertion mode is viable
  (non-TTY output, pipe mode), the existing app-owned transcript
  renderer remains active and D15 governs its display-row expansion.

Because `src/tui_handle.rs` does not enter the alternate screen today, host
scrollback remains available to own committed history. Batch F must preserve
that property.

The managed interaction-surface draw loop in `src/tui_frontend.rs` is the integration
point for this change because it currently owns host setup, viewport
sizing, and the `render_task_layout` / `render_messages` dispatch.

The tree now enables ratatui's `scrolling-regions` feature in `Cargo.toml`, so
Batch F's preferred path can rely on scroll-region semantics on supported
backends. The remaining correctness concern is the resize fallback around
`insert_before(..)`, because prior committed lines cannot be rewrapped in place
when the host width changes after a flush.

### D18: Split committed transcript rows from live viewport rows

The cutover splits the managed interaction surface and renderer in
`src/tui_frontend.rs` and `src/ui/render/mod.rs` into three explicit
responsibilities:

- `flush_committed_history()` — writes stable committed paragraphs into
  host scrollback through the `HostScrollbackSink`.
- `render_live_bottom_viewport()` — renders the live tail (current response,
  active tools, approval surfaces) in the reserved bottom viewport.
- `render_detail_overlay()` — renders the detail/overlay surface for
  structured transcript navigation.

The current `render_messages()` and `render_task_layout()` functions are the
cut points. They remain available for the `OwnedTranscriptFallback` path
but are no longer the main rendering path.

### D19: Restrict main-surface scroll state to live tail and detail overlays

After the host-owned scrollback cutover, `transcript_scroll_offset` in
`src/app.rs` no longer governs committed history position on the main
surface. The replacement state model:

- `committed_history_flush_cursor` — tracks flush progress into host scrollback
  history. Replaces the main-surface meaning of `transcript_scroll_offset`.
- `detail_overlay_scroll_offset` — scroll offset for the overlay/detail
  surface only.
- `surface_mode` — the active `HostInsertMode` selection.

Committed history is not app-scrolled on the main path. The host's
scrollback buffer or the detail overlay provides review.

### D20: Width-aware wrapping for new rendering paths

Width-aware wrapping is retained for:
- Live viewport rendering (`wrap_live_tail_line`)
- Transcript overlay rendering (D15 `expand_rows_for_display` path)
- Owned-transcript fallback (D15 path)

A new `wrap_committed_history_line` helper wraps committed rows to
`reserved_viewport_text_width` before flushing to host scrollback. A
`build_terminal_history_insert_lines` helper batches wrapped committed
rows into host scrollback insertion sequences.

Stop using main-surface full-history display-row expansion as the indefinite
scroll owner. `expand_rows_for_display()` in `src/ui/render/transcript.rs`
is streamlined to overlay and owned-transcript fallback use only.

### D21: Turn-boundary reset semantics

Turn-boundary resets in `src/app/turn.rs` (`reset_turn_capture`,
`begin_turn_capture`, `complete_turn_if_idle`) and `src/app/model_update.rs`
(`EditLoopComplete`, `UiUpdate::Error`) may clear:

- Live-tail ephemeral state (streaming buffers, pending rows)
- Overlay-local detail scroll offset (`detail_overlay_scroll_offset`)

Turn-boundary resets must **not** forcibly destroy committed-history review
semantics. Specifically, they must not zero the committed history flush
cursor or interfere with the operator's position in the host
scrollback.

The current `self.transcript_scroll_offset = 0` assignments at turn
boundaries become live-tail and overlay resets only, not main-surface
committed-transcript position destruction.

### D22: Idle path no longer uses u16 Paragraph scroll

After the cutover, the idle rendering path in `src/ui/render/mod.rs`
(`render_messages`) no longer uses the `u16` `Paragraph::new().scroll()`
tail-pin as the long-session history mechanism. Under host-owned scrollback
history, idle committed content is flushed to host scrollback and is no
longer a `Paragraph` scroll surface. The `u16` cap (~65,000 display rows)
ceases to be a session-length constraint.

### Bug resolution mapping

The six identified scroll defects map to this cutover:

1. **Idle 65k cap** (`src/ui/render/mod.rs` u16 cast) — resolved by D22;
   idle committed history is no longer a `Paragraph` scroll surface.
2. **Jump-to-bottom resets** (`src/app/turn.rs`, `src/app/model_update.rs`
   forced `transcript_scroll_offset = 0`) — resolved by D21; turn-boundary
   resets no longer destroy committed-history review position.
3. **Structural row clipping** (`src/ui/render/transcript.rs`,
   `src/ui/render/mod.rs` no-wrap on structural lines) — becomes overlay or
   fallback concern only (D20).
4. **O(n) scroll cost** (`src/ui/render/transcript.rs`
   `expand_rows_for_display`, `src/app/scroll.rs`) — drops out of the main
   render loop (D18, D20); full expansion remains only for overlay/fallback.
5. **Weak inspector cap** (`src/app/layout.rs` six-row hard cap) — becomes
   an overlay/pager concern rather than the main output surface.
6. **No idle interactive scroll** (`src/ui/render/mod.rs` always tail-pinned)
   — resolved by delegating idle review to host scrollback (D17, D19).
