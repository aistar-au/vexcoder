# Tool-Call Cutover

This note records the current tool-call and transcript rendering findings for
the ratatui task surface, the deliberate cutover choices applied in PR 348,
and the remaining architecture work after that cutover.

## Current constraints

The ratatui task surface keeps the composer immediately below the visible
transcript content. Any surplus rows in the inline viewport accumulate below
the composer rather than between the content and the input pane. By default
the viewport height follows the current display height; `VEX_INLINE_VIEWPORT_ROWS`
can clamp it to a smaller fixed line count when local tuning is needed.
The remaining complexity is no longer the pane split; it is the live
transcript state.

Today the transcript is assembled from three mutable sources:

1. `history_state.lines` for committed transcript paragraphs and tool rows.
2. `current_turn_stream_segments` for in-progress assistant text.
3. `active_stream_blocks` for typed block metadata and live cursor state.

That split means paragraph replacement has to keep multiple structures in sync
whenever a pending tool preview turns into a completed tool-result paragraph.
It also means the renderer has to infer one live transcript from several
buffers instead of reading one canonical document.

## Research summary

The attached tool-call research compared three approaches.

### 1. Keep the current split model and patch individual bugs

This is the lowest-disruption option, but it keeps the same root problem:
scroll math, parser normalization, and paragraph replacement all remain spread
across unrelated buffers.

### 2. Normalize streamed events into an intermediate adapter layer

This improves protocol coverage, but it still leaves paragraph assembly split
between the adapter and the ratatui transcript state. It reduces duplication
without removing it.

### 3. Move to a unified document model with a block-aware virtual viewport

This is the recommended direction. A single paragraph/block store becomes the
source of truth for:

- pending tool previews
- completed tool results
- final assistant text
- waiting-state telemetry
- wrapped-row viewport math

The viewport then consumes one ordered document instead of reconstructing rows
from multiple mutable sources.

## PR 348 cutover choices

PR 348 keeps the ratatui-native transcript surface and makes four explicit
choices so the UI, parser, and API route all move in the same direction.

### 1. Viewport contract

- The composer sits immediately below the visible transcript content.
  Surplus rows in the inline viewport accumulate below the composer so blank
  space never appears between content and the input pane.
- `Viewport::Inline` keeps a fixed line count. In the current CLI surface that
  count defaults to the host display height and can be overridden with
  `VEX_INLINE_VIEWPORT_ROWS` for a smaller reservation.
- Short transcript bodies start directly below the status row and grow
  downward; the output pane is sized to exactly the visible content rows.
- As new rows arrive, the transcript grows downward until it fills the body.
  Once the body is full, the live window follows the bottom and older rows
  scroll upward out of view.

### 2. Transcript rendering contract

- Pending tool paragraphs still render directly into the transcript body
  instead of a separate timeline strip.
- Completed tool-result replacement preserves scroll position by using the net
  transcript growth across the full replacement, not the height of the
  inserted paragraph alone.
- Normalized `StreamDelta` text remains the single visible assistant-text path
  for downstream consumers. Textual `StreamBlockDelta` updates keep block
  identity and cursor metadata, but they do not form a second display-text
  stream.

### 3. API-route contract

- The local API/runtime envelope is now transcript-first.
- Plain `StreamDelta` text is normalized into synthetic
  `final_text` transcript blocks (`transcript_block_start`,
  `transcript_block_delta`, `transcript_block_complete`) instead of emitting a
  separate live `assistant_delta` / final `assistant_message` pair.
- The `assistant_delta` and `assistant_message` events are removed. All
  downstream consumers must read transcript block events only.

### 4. Parser contract

- Local text-protocol turns default to the hybrid parser chain.
- Tagged `<function=...>` parsing stays the fast path.
- Generic `<tool_call>`, `<invoke>`, and `<tool_use>` wrappers are accepted as
  fallback input, then normalized into the tagged text protocol for assistant
  history and the next tool round.

## Next cutover

The next architecture step is to replace the split transcript state with one
canonical task document. The API route has already cut over to the
transcript-first shape; the remaining work is to make the in-process task
state match that same model.

That cutover should:

1. Store pending tool previews, completed tool results, waiting rows, and
   assistant text as one ordered paragraph list.
2. Keep block identity stable so scroll math can reason about net insert,
   replace, and remove operations directly.
3. Let the ratatui viewport render wrapped display rows from that paragraph
   list without reconstructing state from `history_state.lines`,
   `current_turn_stream_segments`, and `active_stream_blocks`.
4. Remove the remaining split between `history_state.lines`,
  `current_turn_stream_segments`, and `active_stream_blocks` so the renderer
  and the runtime both consume one ordered document.

Until that larger cutover lands, the ratatui transcript path should continue
to prefer paragraph-preserving repairs over additional side buffers.
