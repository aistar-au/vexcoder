# Tool-Call Cutover

This note records the current tool-call and transcript rendering findings for
the ratatui task surface, the short-term repairs applied in PR 348, and the
larger cutover that still remains.

## Current constraints

The ratatui task surface already keeps the composer pinned at the bottom edge
and treats transcript scroll offset `0` as the live bottom anchor. The
remaining complexity is not the layout split; it is the live transcript state.

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

## What PR 348 changes now

The current PR keeps the ratatui-native transcript surface and applies the
lowest-risk repairs needed to stabilize it before the larger document-model
cutover.

### Transcript-side repairs

- Pending tool paragraphs are still rendered directly into the transcript body
  instead of a separate timeline strip.
- Completed tool-result replacement now preserves scroll position by using the
  net transcript growth across the full replacement, not the height of the
  inserted result paragraph alone.
- The composer remains pinned to the bottom while transcript paragraphs scroll
  upward above it.

### Parser-side repairs

- Local text-protocol turns now default to the hybrid parser chain.
- Tagged `<function=...>` parsing stays the fast path.
- Generic `<tool_call>`, `<invoke>`, and `<tool_use>` wrappers are accepted as
  fallback input, then normalized into the tagged text protocol for assistant
  history and the next tool round.

## Next cutover

The next architecture step is to replace the split transcript state with one
canonical task document.

That cutover should:

1. Store pending tool previews, completed tool results, waiting rows, and
   assistant text as one ordered paragraph list.
2. Keep block identity stable so scroll math can reason about net insert,
   replace, and remove operations directly.
3. Let the ratatui viewport render wrapped display rows from that paragraph
   list without reconstructing state from `history_state.lines`,
   `current_turn_stream_segments`, and `active_stream_blocks`.
4. Keep the local API/runtime envelope transcript-first so downstream clients
   do not need to reparse flattened assistant text.

Until that larger cutover lands, the ratatui transcript path should continue
to prefer paragraph-preserving repairs over additional side buffers.