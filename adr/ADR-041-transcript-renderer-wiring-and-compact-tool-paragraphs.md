# ADR-041: Transcript Renderer Wiring and Compact Tool Paragraphs

- **Status:** Accepted
- **Date:** 2026-04-01
- **Deciders:** Core maintainer
- **Depends on:** ADR-029, ADR-030, ADR-031, ADR-040
- **Supersedes:** None
- **Superseded by:** None

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
  arrows supported by all terminal emulators that support the existing
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
`src/state/transcript_delta.rs` alongside a `DeltaAccumulator` that
uses bounded suffix comparison — O(new_text) instead of
O(total_content) — to deduplicate cumulative streaming updates.

Accumulators are keyed by block index in TuiMode and are created on
`StreamBlockStart`, fed on `StreamBlockDelta`, and completed/removed
on `StreamBlockComplete`. This runs in parallel with the existing
prefix-marker line path so that both rendering strategies coexist.

### D6: Delta-native draw methods

`TaskDraw::apply_transcript_delta()` and
`TaskDraw::consume_transcript_deltas()` provide a direct path from
structured deltas to the output row buffer, bypassing the
`[tool]`/`[detail]`/`[evidence]` prefix-marker chain.

`format_compact_paragraph()` in `transcript_helpers.rs` applies
uniform prefix and width-safe truncation for all block kinds.

### D7: Bounded suffix deduplication

`bounded_incremental_suffix()` is a performance-optimised alternative
to the existing `append_incremental_suffix()`. It compares only the
prefix window up to `existing.len()` bytes rather than scanning the
full accumulated content on every chunk.

---

## Amendment — 2026-05-01: Pending-row replacement, live input preview, and ordered streamed text

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

`StreamBlockDelta(ToolCall)` now updates the `[detail] Input:` row in
`history_state.lines` in-place on every chunk using the
`input_preview` string already maintained on `PendingTurnToolCall`.

The row to update is identified as `history_state.lines[transcript_row_start
+ transcript_row_count - 1]`, guarded by a `starts_with("[detail] Input:")`
check before overwriting.  This gives operators live feedback on
partial JSON argument construction during long tool-call generations
without emitting extra rows.

### D10: Flush completed streamed text segments at non-text boundaries

When structured block streaming is active, sanitized `StreamDelta`
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

This keeps cumulative backend text updates on the same bounded
O(new_text) suffix path used by the structured transcript delta
accumulators, avoiding repeated scans across the full accumulated
buffer on every chunk.

---

## Amendment — 2026-05-02: Delta-native path activation and accumulator drain

### D12: Accumulator drain at block completion, tests, and bounded-suffix cleanup

`StreamBlockComplete` now calls `flush_pending()` on the associated
`DeltaAccumulator` before removing it. This drains any pending deltas
that were not yet consumed by the renderer, ensuring the accumulator
is cleanly emptied rather than silently dropped. The drained content
is discarded since the parallel rendering paths (D9 live input preview
for ToolCall, D10 streamed text segments for FinalText/Thinking) have
already materialised the same content into `history_state`.

`format_compact_paragraph()`, `apply_transcript_delta()`, and
`consume_transcript_deltas()` are covered by unit tests in
`src/ui/draw/tests.rs`, confirming that:
- ToolCall deltas produce `▶`-prefixed rows when consumed via the delta path.
- ToolResult deltas produce `↳`-prefixed rows.
- Empty incomplete deltas produce no output rows.
- FinalText/Thinking deltas forward content without directional prefixes.

`set_block_kind()`, `content()`, `flush_pending()`, and
`bounded_incremental_suffix()` are covered by inline tests in
`src/state/transcript_delta.rs`. `bounded_incremental_suffix` has its
`#[allow(unused)]` annotation removed because it already routes through
production code in `src/state/conversation/streaming.rs`.

The remaining `#[allow(unused)]` annotations on
`TranscriptDelta`, `flush_pending`, `content`, `set_block_kind`,
`format_compact_paragraph`, `apply_transcript_delta`, and
`consume_transcript_deltas` are retained because Rust's reachability analysis for the library
target does not count `#[cfg(test)]` usage as live, and the full production renderer switchover (connecting
`task_layout_state()` → `stream_deltas` → `consume_transcript_deltas`)
is deferred to a follow-on PR.

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

This keeps the transcript-first live path aligned with the backend's
JSON delta stream instead of leaking raw line fragments into the
operator surface when the model server splits tool markup across
arbitrary chunk boundaries.

### D14: Strip wrapper-only remnants from the assistant text fallback

`sanitize_assistant_text()` now removes `<tool_call>` wrappers and
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
terminal width by `truncate_to_width` inside `draw_inline_markdown`, making
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
