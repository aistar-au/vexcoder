# Task PJ-03: User Persistent Notes (`/memory`)

**Target Files:** `src/app.rs`, `src/config.rs`, `tests/integration_test.rs`

**ADR:** ADR-024 Gap 16

**Depends on:** PA-01 (layered config resolution chain — green at PR #48)

---

## Issue

No mechanism exists for operators to persist notes across sessions or inject
them into the system context. ADR-024 Gap 16 defines a user-level notes
surface stored at a user-config-layer path and injected at session start
within a token budget.

---

## Decision

1. Resolve the notes file path from the user config layer (priority 3 in the
   layered chain). The path must not be settable via repo-local config —
   notes are operator-personal and project-scoping them is a security boundary
   violation.

2. Default path: `~/.config/vex/memory.md` (XDG). Fallback:
   `~/.vex/memory.md`. The file is created on first `/memory add` if absent.

3. Add three commands to `try_handle_slash_command` in `src/app.rs`:

   ```
   /memory
       Render current notes file contents via push_history_line. No model turn.
       If file missing or empty: "[memory] no notes".

   /memory add <note>
       Append <note> as a new line. Create file if absent.
       Emit "[memory: note added]". No model turn.

   /memory clear
       Confirmation overlay ("clear all notes? [y/N]") via existing overlay path.
       On confirm: "[memory: cleared]". On cancel: "[memory: cancelled]".
       No model turn. BatchMode without --auto-approve: error.
   ```

4. At session start, inject notes file contents into the system context
   within a token budget. Budget overflow is a warning emitted via
   `push_history_line`; the session must continue without the notes rather
   than abort.

5. Auto-memory (model-initiated extraction) is formally deferred. Do not
   implement it here.

---

## Constraints

- `/memory` commands must never call `ctx.start_turn`. All output is via
  `push_history_line`.
- The notes file path is resolved from the user config layer only.
  Repo-local `.vex/config.toml` specifying a notes path is a hard startup
  failure with a diagnostic naming the file.
- The notes file is never committed to source control. Do not add it to
  `.gitignore` in this task — that belongs to `vex init` (PJ-04).
- Do not modify `src/runtime/`, `src/state/`, or `src/api/` in this task.
- Do not implement `/compact` (PJ-01), `/fork` (PJ-02), or `vex init` (PJ-04)
  in this task.

## Implementation Addendum

- The merged implementation required bounded wiring in `src/api/client/mod.rs`
  and `src/runtime/context.rs` so session-start notes injection could be shared
  across TUI and batch runtime entry points. `src/state/` was not modified.
- The operator branch also carries PE-01/PE-02 work (`src/batch_mode.rs`,
  `src/bin/vex.rs`, `src/runtime/task_state/mod.rs`, and
  `TASKS/PE-01-batch-mode.md`). That combined scope is a branch-traceability
  matter, not a notes-surface correctness issue.
- In batch mode, `/memory clear` is handled locally. Without `--auto-approve`
  it returns an error; with `--auto-approve` it clears the notes file without
  forwarding the command to the model.
---

## Definition of Done

1. `/memory` renders notes or "[memory] no notes" with no model turn.
2. `/memory add <note>` appends and creates the file if absent.
3. `/memory clear` shows the confirmation overlay; cancellable.
4. Notes are injected at session start within the token budget.
5. Budget overflow emits a warning and continues the session.
6. Repo-local config specifying a notes path is rejected at load time.
7. `cargo test --all-targets` is green.

---

## Anchor Tests

`test_tui_memory_renders_empty_notes`
`test_tui_memory_add_appends_to_file`
`test_tui_memory_clear_requires_confirmation`
`test_tui_memory_clear_cancellable`
`test_tui_memory_does_not_call_start_turn`
`test_memory_injection_within_budget`
`test_memory_injection_over_budget_emits_warning`

Primary verification anchor:

```rust
#[test]
fn test_tui_memory_does_not_call_start_turn() {
    // Given a session with a populated notes file,
    // /memory, /memory add, and /memory clear confirmation-cancel
    // must each complete without calling ctx.start_turn.
    // Verified by asserting no AppAction::StartTurn is emitted.
}
```

---

## Dispatch Verification (dispatch only — implementation not yet merged into current `main`)

### [PJ-03] - User persistent notes (`/memory`)

- Deprecated branch name: omitted
- Commit: `c289cac`
- Files changed:
  - `TASKS/PJ-03-memory-notes-injection.md` (this file)
  - `TASKS/PL-01-pre-post-tool-hooks.md`
- Validation:
  - `cargo test --all-targets` : pass
  - `bash scripts/check_no_alternate_routing.sh` : pass
  - `bash scripts/check_forbidden_imports.sh` : pass
- Notes:
  - This branch stages the PJ-03 dispatch manifest only.
  - Do not mark PJ-03 green until the implementation branch lands and all
    anchor tests pass.

---

## Completion Verification (fill in when implementation lands)

### [PJ-03] - User persistent notes (`/memory`)

- Deprecated branch name: omitted
- Commit: `3e0405f6697812f686da7a17c3f9ca7fc27a068f`
- Files changed:
  - `src/app.rs` (+`354` -`6`)
  - `src/config.rs` (+`27` -`0`)
  - `tests/integration_test.rs` (+`23` -`0`)
- Validation:
  - `cargo test test_tui_memory_does_not_call_start_turn --all-targets` : pass
  - `cargo test test_memory_injection_within_budget --all-targets` : pass
  - `make gate-fast` : pass
  - `cargo test --all-targets` : pass
  - `bash scripts/check_no_alternate_routing.sh` : pass
  - `bash scripts/check_forbidden_imports.sh` : pass
- Notes:
  - Notes injection and /memory commands implemented per ADR-024 Gap 16.
  - Notes path remains user-config-layer only; repo-local override rejected.
  - Scope note: implementation also touches `src/api/client/mod.rs`,
    `src/runtime/context.rs`, `src/session_notes.rs`, and PE-01/PE-02 files
    (`src/batch_mode.rs`, `src/bin/vex.rs`, `src/runtime/task_state/mod.rs`,
    `TASKS/PE-01-batch-mode.md`).
  - Batch mode now handles `/memory clear` locally: no `--auto-approve`
    returns an error; `--auto-approve` clears notes with no model turn.
