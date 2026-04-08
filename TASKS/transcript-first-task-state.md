# Transcript-First In-Process Task State Unification

## Status

In progress — PR #349 (`work/vexcoder-task-document-pr1`) introduces the
canonical `TaskDocument` runtime module rooted at `src/runtime/task_document.rs`
with focused submodules in `src/runtime/task_document/{model,reducer,snapshot,tests}.rs`.
All core types (`TaskDocument`, `TaskMeta`, `ActiveTurnDocument`,
`TurnDocument`, `TurnEntry`, `TaskDocumentReducer`, `TaskMutationSummary`) and
the snapshot round-trip adapter (`persistable_snapshot` /
`restore_from_snapshot`) are now implemented and exported from
`crate::runtime`. PR #349 stays within the PR-1 scope: no TUI ownership
changes land here, approval events accept the runtime's hyphenated capability
strings, and persisted tool outcomes now derive from `ToolResult` semantics
rather than the intermediate `ToolStatus` display state.

## Context

The API route is already transcript-first. `RuntimeEnvelopeNormalizer` emits
`transcript_block_start`, `transcript_block_delta`, and
`transcript_block_complete` for every streamed text turn. All downstream batch
derivation and live event consumers read this single shape.

The ratatui-native in-process task state is not yet transcript-first. It
maintains three separate mutable buffers that must be kept in sync:

| Field | Module | Role |
| :--- | :--- | :--- |
| `history_state.lines` | `src/app/model_update.rs` | Committed transcript rows and tool paragraphs |
| `current_turn_stream_segments` | `src/app/model_update.rs` | Current-turn streamed assistant text, indexed by `active_stream_segment_index` |
| `active_stream_blocks` | `src/app/model_update.rs` | Typed block metadata (`Thinking`, `FinalText`, `ToolCall`, `ToolResult`) and cursor state |

A block completing (via `StreamBlockComplete`) removes it from
`active_stream_blocks` and resets `active_stream_segment_index`. A tool result
replaces rows in `history_state.lines`. Both operations must also be reflected
in `current_turn_stream_segments`.

The renderer (`src/ui/render/transcript.rs`) reconstructs the display from all
three buffers every frame.

Immediate downstream guardrail for PR 348:

- Normalized `UiUpdate::StreamDelta` is the authoritative visible-text path for
  downstream consumers.
- Textual `UiUpdate::StreamBlockDelta` remains block metadata and cursor state,
  not a second display-text stream.
- `BatchMode` already follows this rule; the ratatui path must keep matching it
  until the task document replaces the split state entirely.

## Design win

Normalizing the in-process state to the same block model the API emits
eliminates the need to:

- Hold `active_stream_segment_index` as a separate cursor.
- Call `clamp_transcript_after_mutation` at multiple independent callsites.
- Snapshot `previous_output_len` before mutations to preserve scroll.
- Re-derive the current-turn response text from `current_turn_stream_segments`
  at turn completion.

Downstream consumers are simple: any tool that reads the `RuntimeEnvelope`
stream can also read the in-process block list without a separate adapter.

## Proposed model

Replace the three-buffer model with one ordered `TaskDocument` that holds a
list of `TaskParagraph` values:

```
TaskParagraph =
  | TranscriptLine(String)
  | AssistantBlock { index: usize, kind: BlockKind, text: String, status: BlockStatus }
  | ToolPreview    { index: usize, name: String, input_preview: String, step_id: String }
  | ToolResult     { index: usize, name: String, output_preview: String, is_error: bool }
  | WaitingRow     { message: String }
```

`BlockKind` maps directly to `StreamBlock::Thinking | StreamBlock::FinalText`.
`BlockStatus` is one of `Streaming | Complete`.

Incoming `UiUpdate` events mutate the document in place:

| Event | Document mutation |
| :--- | :--- |
| `UiUpdate::TranscriptLine(s)` | Append `TranscriptLine(s)` |
| `UiUpdate::StreamBlockStart { index, block }` | Append `AssistantBlock { index, kind, text: "", status: Streaming }` |
| `UiUpdate::StreamBlockDelta { index, delta }` | Find `AssistantBlock` at `index`, push `delta` to `text` |
| `UiUpdate::StreamBlockComplete { index }` | Set `AssistantBlock` at `index` to `status: Complete` |
| `UiUpdate::StreamDelta(text)` | Append to the last `AssistantBlock` with `status: Streaming`, or open a new one |
| Tool pending | Append `ToolPreview` |
| Tool result received | Replace `ToolPreview` at same `index` with `ToolResult` |

The renderer reads the document sequentially and emits display rows. Scroll
math operates on the document paragraph list, not on `history_state.lines`.

## Items

### TF-01 — Define `TaskDocument` and condenser-owned turn types

**Files:** `src/runtime/task_document.rs`, `src/runtime/task_document/model.rs`,
`src/runtime/task_document/condenser.rs`, `src/runtime/task_document/snapshot.rs`

Define the canonical task and turn types at the runtime layer, keep the condenser
adjacent to the model, and add the snapshot adapter that round-trips through
`TaskState` and `TurnEvidenceState` without introducing a parallel event model.
Add focused unit tests for approval parsing, grant persistence, and snapshot
restore semantics.

### TF-02 — Replace `history_state` + `current_turn_stream_segments` initialization

**File:** `src/app/model_update.rs` (TuiMode struct)

Replace the `history_state`, `current_turn_stream_segments`, and
`active_stream_segment_index` fields with a single `task_document: TaskDocument`.
Keep `active_stream_blocks` only for tool-approval metadata that has no
direct display counterpart. Migrate `turn_in_progress`, `cancel_pending`,
and `active_assistant_index` to the document or a thin wrapper.

### TF-03 — Migrate `on_model_update` to document mutations

**File:** `src/app/model_update.rs`

Rewrite the `on_model_update` match arms to call `task_document` methods
instead of mutating `history_state.lines` and `current_turn_stream_segments`.
Remove `clamp_transcript_after_mutation` calls; the document model handles
bounds naturally.

### TF-04 — Update renderer to consume `TaskDocument`

**File:** `src/ui/render/transcript.rs`

Replace the three-argument render signature
`(history_state, stream_segments, active_blocks)` with `(&TaskDocument)`.
Port the existing row-expansion and wrapping logic.

### TF-05 — Migrate scroll preservation to document-level operations

**File:** `src/app/scroll.rs`

Replace `expanded_output_row_count()` snapshots and `previous_output_len`
tracking with a document-level `row_count_since(paragraph_index)` helper.
Remove `append_stream_segment_delta`.

### TF-06 — Update layout and tests

**Files:** `src/app/layout.rs`, `src/app/tests/task_layout.rs`,
`src/app/tests/model_turn.rs`

Update `completed_tool_paragraph_rows` and any test helpers that construct
`history_state` or `current_turn_stream_segments` directly to use
`TaskDocument` builders instead.

### TF-07 — Remove three-buffer fields from `TuiMode`

After TF-02 through TF-06 compile and pass, remove the old fields and the
`append_stream_segment_delta` helper from `scroll.rs`. Run `cargo clippy` to
confirm the compiler finds no unreferenced code.

## Dependency

TF-01 through TF-07 should land in a single focused feature branch in one
comprehensive draft PR. Do not split layout, renderer, tests, and scroll
into separate overlapping PRs for the same lane.

## Validation

After all items:

```sh
cargo fmt --check
cargo nextest run -j 2
cargo test --all-targets
bash scripts/check_forbidden_names.sh
make gate-fast
```
