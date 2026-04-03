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

## Amendment — 2026-05-01: Pending-row replacement and live input preview

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
