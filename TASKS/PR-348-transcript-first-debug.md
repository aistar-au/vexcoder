# PR 348 — Transcript-First Cutover: Line-by-Line Debug Manifest

Branch: `work/vexcoder-tool-call-ratatui-overhaul`

This manifest covers every remaining problem that the PR #348 code has either
deferred or left partially resolved. Walk each item in order. A later item
may surface only after an earlier one is resolved.

---

## A. Alternate-route removal

The live `RuntimeEnvelopeNormalizer` path no longer emits `AssistantDelta` or
`AssistantMessage`. But both variants still exist in the `RuntimeEvent` enum
and are still handled in `derive_batch_records`. These are the alternate routes
the user has explicitly asked to remove.

### A-1 — Remove `AssistantDelta` and `AssistantMessage` from `RuntimeEvent`

**File:** `src/runtime/json_handoff.rs`

Current lines (in the `RuntimeEvent` enum):

```rust
AssistantDelta {
    text: String,
},
AssistantMessage {
    content: String,
},
```

Remove both variants. Update the `#[serde(tag = "type", rename_all = "snake_case")]`
attribute coverage accordingly. The enum match arms in `derive_batch_records`
that handle these variants must also be removed.

### A-2 — Remove `delta_response` and `assistant_message` from `DerivedTurnState`

**File:** `src/runtime/json_handoff/derived.rs`

Current fields:

```rust
pub(super) delta_response: String,
pub(super) assistant_message: Option<String>,
```

Remove both. The fallback chain in `into_record()` collapses to:

```rust
let response = self.transcript_response;
```

(`flush_open_final_text_blocks()` is still called before the assignment.)

### A-3 — Remove backward-compat assertions from `test_pi_12`

**File:** `src/runtime/json_handoff/tests.rs`

The test constructs a second turn by manually injecting an `AssistantDelta`
envelope and then asserts `derived.turns[1].response == "fallback"`. After
A-1 the `AssistantDelta` variant will not exist. Replace the second turn
with a `TranscriptBlockStart` / `TranscriptBlockDelta` / `TranscriptBlockComplete`
sequence that emits the same text and keep the assertion on `response`.

### A-4 — Remove `assistant_delta` and `assistant_message` schema definitions

**File:** `schemas/runtime_envelope_v1.json`

Remove the `$ref` entries for `assistant_delta` and `assistant_message` from
the `oneOf` array and delete their `$defs` blocks. The live schema must only
describe events the normalizer can actually emit.

### A-5 — Update `docs/src/tool-call-cutover.md`

The current text says:

> Historical `assistant_delta` and `assistant_message` events remain parseable
> for old recordings and batch derivation, but the live local API path no
> longer depends on them.

After A-1 through A-4 that sentence is no longer accurate. Replace it with:

> The `assistant_delta` and `assistant_message` events are removed. All
> downstream consumers must read transcript block events only.

---

## B. In-process task state split

The ratatui-native TUI maintains three mutable sources of truth that must stay
in sync during live streaming:

| Buffer | Role |
| :--- | :--- |
| `history_state.lines` | Committed transcript rows (tool results, completed paragraphs) |
| `current_turn_stream_segments` | Current-turn streamed assistant text |
| `active_stream_blocks` | Typed block metadata + live cursor state |

Symptoms observed in PR 347 are explained by this split:

- `structured_streaming_active` flag not reset on interrupted turns
  (patched in PR 348 but the root cause is the split, not the flag).
- `clamp_transcript_after_mutation` removed from the completed-tool path
  to prevent double clamping — this is a side-effect symptom, not a fix.
- Scroll preservation requires `previous_output_len` snapshots at multiple
  callsites that are only correct if all three buffers are updated
  atomically.

**This section is a diagnosis only — the fix is tracked in the canonical
task document `TASKS/transcript-first-task-state.md`.**

---

## C. Viewport contract regression

**File:** `src/app/scroll.rs`, `src/app/model_update.rs`

`append_stream_segment_delta()` uses `active_stream_segment_index` to route
deltas into the segment list. `active_stream_segment_index` is reset to `None`
on `StreamBlockStart` (for `Thinking` and `FinalText` blocks) and on
`StreamBlockComplete`. This is correct for the server streaming path.

**Potential regression:** When the ratatui path receives a plain
`UiUpdate::StreamDelta` (not wrapped in a block), `ensure_open_final_text_block`
is called for the API normalizer but `active_stream_segment_index` is not
reset — the delta is appended to the last open segment. Verify in
`src/app/model_update.rs` that `UiUpdate::StreamDelta` also resets
`active_stream_segment_index` to `None` before calling
`append_stream_segment_delta` if it is a new logical paragraph.

---

## D. Forbidden-names gate

**File:** `scripts/check_forbidden_names.sh`

The banned-tone word for stale/removed code paths is disallowed in comments
and log messages. Verify that removing the `AssistantDelta` /
`AssistantMessage` variants does not introduce it.
Run `bash scripts/check_forbidden_names.sh` after every change.

---

## E. REPO-RAW-URL-MAP update

**File:** `TASKS/completed/REPO-RAW-URL-MAP.md`

Any new TASKS file added in this branch must be registered in the map. The
doc-ref-check workflow validates the header count. Adding a file without
updating the map will fail the CI check.

---

## Validation checklist

After completing A-1 through E:

```sh
cargo fmt --check
cargo test --all-targets
bash scripts/check_forbidden_names.sh
make gate-fast
```

All 11 current PR checks are green. The target is to keep all 11 passing after
the alternate-route removal patch.
