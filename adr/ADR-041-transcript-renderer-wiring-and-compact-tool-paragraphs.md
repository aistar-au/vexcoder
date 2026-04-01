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
