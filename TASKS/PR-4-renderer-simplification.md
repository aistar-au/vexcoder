# PR 4 — Simplify Renderer and Remove Sticky Bottom Surface

**Branch:** `work/vexcoder-task-document-pr4`
**Follows:** PR 3 (`work/vexcoder-task-document-pr3`, PR #351)
**Source spec:** batch-start.txt PR 4

**Current status (2026-04-07):** Implemented across PR #352 and PR #353 and
merged on main. The sticky footer removal, overlay routing, and standalone
`TaskViewProjection` cutover landed. Two consistency gaps remain against the
original source spec: `src/ui/render/transcript.rs` still decodes structured
marker prefixes, and `TuiMode` still owns live `command_sessions` outside
`task_doc`.

---

## Scope

The renderer currently consumes `TaskLayoutState` plus a suite of string
markers produced by `helpers.rs`. It uses a footer split that renders a
pseudo-prompt area in the bottom pane for `input_hint`, shell context, and
approval notices. PR 4 collapses that surface:

- The renderer consumes a small `TaskViewProjection`, not `TaskLayoutState`
  plus string markers.
- The bottom pane renders only the composer.
- Approval and resume prompts live in overlays or modals, not in a
  pseudo-prompt footer.
- The whole marker-row protocol layer is deleted from `helpers.rs`.
- Any new files introduced in this lane use descriptive, domain-specific names
  and stay near the ~300-line ceiling whenever the split boundary is clear.

## Consistency Debug

Matched in PR #352:

- The sticky footer and `input_hint` path were removed.
- The bottom pane now renders only the composer.
- Approval and memory-clear prompts render through overlays instead of the
  footer.
- The marker-row helper layer was deleted from `src/app/layout/helpers.rs` and
  absorbed into projection code.

Residual deltas against `batch-start.txt`:

- `src/ui/render/transcript.rs` still interprets marker-prefixed rows such as
  `[tool]`, `[detail]`, and `[approval]` instead of acting as a pure viewport
  slicer over fully projected rows.
- `TuiMode` still carries live `command_sessions` outside `task_doc`, so the
  PR 3 "document projector in one shot" cutover is functionally landed for
  transcript ownership but not fully complete for command-session ownership.

### Post-merge follow-up todo

- Open the next narrow cleanup lane for the remaining deltas from PRs #350-
  #353: remove marker decoding from `src/ui/render/transcript.rs` and move
  command-session ownership fully into `task_doc`.

---

## Edit Order

1. **`mod.rs`** — remove the footer split from `render_task_input`; remove
   the footer argument and footer rendering path; normalize the bottom pane
   to composer-only output.

2. **`transcript.rs`** — delete string-marker interpretation; keep only
   viewport slicing of already-projected rows from `project_transcript_rows`.

3. **`layout.rs`** — delete `input_hint` construction; delete the
   `task_step_views` synthesis path that duplicates `TaskDocument` state;
   delete `transcript_display_rows` and `visible_changed_files` derivation
   that now belongs to the projection layer.

4. **`helpers.rs`** — delete the whole marker-row helper layer (approval
   paragraph rows, tool paragraph markers, pending-paragraph replacement logic
   that feeds string markers rather than typed entries).

5. **`layout.rs`** (second pass) — clean up any remaining `TaskLayoutState`
   fields that are only exercised by the deleted code paths.

---

## Expected Deletions by File

| File | Expected deletions |
| :--- | :--- |
| `src/ui/render/mod.rs` | `footer` argument, footer rendering branch in `render_task_input`, `input_hint` normal-mode rendering, sticky-prompt area |
| `src/ui/render/transcript.rs` | String-marker decoding logic, row-type reconstruction from marker convention |
| `src/app/layout.rs` | `input_hint` construction, `task_step_views` in current form, `transcript_display_rows`, `visible_changed_files` (if still present after PR 3) |
| `src/app/layout/helpers.rs` | Whole marker-row helper layer |

---

## Prerequisites

- PR 3 merged (PR #351): `TuiMode` no longer owns `history_state.lines` or
  stream segment buffers; `project_transcript_rows` is the single transcript
  source.
- ADR-044 accepted: test files split before adding projection-layer tests.

## Testing Foundation Coupled to This Lane

If PR 4 adds renderer or projection tests beyond the current split points,
carry the ADR-044 foundation work in the same lane instead of adding more ad
hoc helpers:

- Add or reuse `tests/all.rs` as the integration-test aggregator.
- Add or extend `tests/common/test_support.rs` for shared SSE fixtures,
  tagged-tool helpers, and renderer-specific builders.
- Route new runtime-context setup through `MockContextBuilder` rather than new
  `setup_*` helpers.
- Use `TempEnv` for environment-variable mutations in renderer and slash
  command scenarios.
- Default new async renderer tests to
  `#[tokio::test(flavor = "multi_thread", worker_threads = 2)]` when they
  depend on scheduler behavior.
- Prefer `#[test_case]` when the same rendering assertion matrix varies only by
  tool state, approval state, or transcript content.

---

## Acceptance Criteria

- [x] `cargo nextest run --no-fail-fast` passes (count must not regress below
  1301).
- [x] `make check-arch` clean.
- [x] `make check-names` clean (no `#[cfg(test)]` workaround needed in `src/` or
  `tests/`).
- [x] `cargo clippy --all-targets --all-features -- -D warnings` clean.
- [x] `cargo fmt --check` clean.
- [ ] The renderer reads only `TaskViewProjection` fields; no access to
  `TaskLayoutState` from within `src/ui/render/`.
- [ ] `src/ui/render/transcript.rs` performs only viewport slicing of
  already-projected rows; no marker-prefix decoding remains.
- [x] No `input_hint` string is constructed anywhere in `src/app/layout.rs`.
- [x] No marker-row protocol functions remain in `src/app/layout/helpers.rs`.
- [x] Approval and resume prompts route through the overlay system, not the
  footer.
- [x] Any new files or split modules use descriptive names and remain near the
  ~300-line ceiling.

---

## Files to Touch

Primary:
- `src/ui/render/mod.rs`
- `src/ui/render/transcript.rs`
- `src/app/layout.rs`
- `src/app/layout/helpers.rs`

Secondary (test updates):
- `src/app/tests/task_layout.rs` — must be split per ADR-044 Rule 1 before
  adding new projection tests (999 lines, ceiling is ~300)
- `src/app/tests/transcript.rs` — update assertions to projection-only style
- `tests/all.rs` and `tests/common/test_support.rs` — add or extend if PR 4
  needs shared renderer-test fixtures beyond the current app-local helpers

---

## Test File Splits Required (ADR-044 Rule 1)

`src/app/tests/task_layout.rs` (999 lines) must be split before new tests
are added. Suggested split boundaries:

| New file | Content |
| :--- | :--- |
| `task_layout/mod.rs` | Aggregator + shared fixtures |
| `task_layout/timeline.rs` | Timeline entry and step-index tests |
| `task_layout/transcript_rows.rs` | `project_transcript_rows` output tests |
| `task_layout/layout_state.rs` | `TaskLayoutState` field projection tests |

---

## Notes

- Do not make model-facing shell access decisions in this PR. Keep
  `src/app/shell.rs` and `src/tools/mod.rs` unchanged. The shell-exposure
  question deferred to the optional PR 7.
- The `input_hint` removal eliminates the UI prompt that makes shell look
  like a normal composer-mode tool. This is a pure deletion, not a
  replacement.
- Keep naming explicit in both production and test modules. Prefer file names
  like `transcript_projection.rs` or `task_view_projection.rs` over generic
  groups such as `helpers2.rs` or `misc.rs`.
