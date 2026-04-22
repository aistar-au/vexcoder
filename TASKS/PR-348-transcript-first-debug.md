# PR 348 — Transcript-First Audit TODO

Branch: `work/vexapi-tool-call-ratatui-overhaul`
Head: `50995ce479eb6396aae896c5feb08a646e36d71f`

This note records the audited state of PR 348 after the transcript-first
alternate-route removal landed. It is the working TODO for the remaining
follow-up on this branch.

---

## Completed on this branch

The transcript-first cutover is already in place for the API/runtime envelope:

- `src/runtime/json_handoff.rs` no longer defines `AssistantDelta` or
  `AssistantMessage`.
- `src/runtime/json_handoff/derived.rs` derives turn responses from transcript
  final-text blocks only.
- `src/runtime/json_handoff/tests.rs` now verifies the transcript-block-only
  batch derivation path.
- `schemas/runtime_envelope_v1.json` no longer advertises
  `assistant_delta` or `assistant_message`.
- `TASKS/transcript-first-task-state.md` already records that downstream
  consumers must stay on the transcript-first event path.

The alternate-route removal is therefore complete in code and schema. Any
remaining references are deprecated notes or task-planning text, not live
runtime behavior.

---

## Remaining patch items in PR 348

### A. Ratatui must use normalized stream text as the only visible text source

**Files:** `src/app/model_update.rs`, `src/app/tests/task_layout.rs`

`RuntimeContext::forward_conversation_update()` can forward both:

- `UiUpdate::StreamBlockDelta { .. }` for block identity/metadata
- `UiUpdate::StreamDelta(text)` for normalized display text

`BatchMode` already treats `StreamDelta` as the authoritative response-text
path and ignores textual `StreamBlockDelta` for visible output. The ratatui
path should match that contract.

Required behavior for this branch:

- `StreamBlockDelta` for `Thinking` / `FinalText` updates block metadata,
  delta buffers, and cursor state only.
- `StreamDelta` remains the single source for visible assistant text and
  `current_turn_response`.
- Regression tests cover the real runtime sequence:
  `StreamBlockStart` + `StreamBlockDelta` + normalized `StreamDelta`
  without duplicating visible rows.

This keeps the downstream text path aligned with the transcript-first
normalization boundary while the larger task-document refactor is still pending.

### B. Accepted next step is still task-state unification

**File:** `TASKS/transcript-first-task-state.md`

The API route is already transcript-first. The remaining architecture work is
the in-process ratatui task state:

- `history_state.lines`
- `current_turn_stream_segments`
- `active_stream_blocks`

The next focused PR should replace that three-buffer model with one accepted
task document that both rendering and scroll math consume directly.

### C. Raw URL map header is still the current CI breakage

**File:** `TASKS/completed/REPO-RAW-URL-MAP.md`

The branch currently fails `check-map-coverage` because the map header count
must match `git ls-files` for the branch head.

Required fix:

- Regenerate the map or patch the header/entries so the tracked-file count
  matches the audited branch state.
- Keep entries for:
  `TASKS/PR-348-transcript-first-debug.md`,
  and `TASKS/transcript-first-task-state.md`.

---

## Validation

After the ratatui text-source fix and the map refresh:

```sh
cargo fmt --check
cargo test --all-targets
bash scripts/check_forbidden_names.sh
make gate-fast
```

The target is a branch where the transcript-first API contract stays intact,
the ratatui path no longer risks duplicate text from mixed update streams, and
`check-map-coverage` passes again.
