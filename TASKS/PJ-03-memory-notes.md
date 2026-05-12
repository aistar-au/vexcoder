# Task PJ-03: User Persistent Notes (`/memory`)

**Target files:**
- Slash-command routing in `src/app.rs` — `/memory`, `/memory add`, `/memory clear`
- Notes file storage: explicit user-config `notes_path` or XDG-style `~/.config/vex/memory.md`
- Session prompt injection: `src/session_notes.rs` and `src/api/client/mod.rs` (after project instructions, within token budget)

**ADR:** ADR-024 Gap 16

**Parity items:** PJ-01 (`/compact`), PJ-02 (`/fork`), PJ-03 (`/memory`)

**Depends on:** PA-01 (layered config — notes file path resolved from user config layer only)

---

## Issue

There is no operator-level persistent notes surface. Comparable coding-agent
surfaces use file-backed Markdown instructions or notes so recurring
operator-specific facts do not have to be retyped in every prompt. Vex keeps
that state in an operator-scoped notes file, injects it only within a configured
token budget, and leaves over-budget content on disk. This is distinct from
project instructions (Gap 4 / `AGENTS.md`): project instructions are
project-scoped and committed; user notes are operator-scoped and never
committed. PJ-03 must not begin until PA-01 (layered config) is passing.

---

## Decision

1. **Storage path:** explicit user-config `notes_path` when set; otherwise
   `XDG_CONFIG_HOME/vex/memory.md` or `~/.config/vex/memory.md`. No repo-local
   or legacy-home notes fallback is used. The file is created on first
   `/memory add` if absent.

2. **Session injection:** Read notes file at session start, append to system
   prompt after project instructions, within `VEX_MAX_MEMORY_TOKENS` budget
   (default: 2048). Exceeding budget emits a startup warning, skips injection,
   and keeps the source content on disk for operator review.

3. **Commands** (added to `try_handle_slash_command`):
```
   /memory
       Renders notes file contents to transcript via push_history_line.
       No model pulse. If file absent or empty: "[memory] no notes".

   /memory add <note>
       Appends <note> as a new line to the notes file. Creates the file if
       absent. Emits "[memory: note added]". No model pulse.

   /memory clear
       Clears all notes after in-TUI confirmation prompt
       ("clear all notes? [y/N]" via existing overlay input path).
       Emits "[memory: cleared]" on confirmation; "[memory: cancelled]"
       otherwise. No model pulse.
```

4. **Constraints:**
   - Notes file path resolved from user config layer only (priority 3 in
     Gap 3 layered chain). Must not be settable via repo-local `.vex/config.toml`.
   - `/memory` commands must never start a model pulse.
   - `/memory clear` requires confirmation overlay in `TuiMode`.
   - `BatchMode` treats `/memory clear` as denied unless `--auto-approve` passed.
   - Token budget overflow is a warning only; session proceeds without injection.
   - Notes file is never committed to source control.

---

## Definition of Done

- `cargo test --all-targets` passes.
- `make gate-fast` passes.
- `/memory add "test note"` appends to the configured notes file or the XDG
  default at `~/.config/vex/memory.md`.
- Notes file content appears in session system prompt on next start (within budget).
- Token budget overflow emits a startup warning and skips injection without aborting.
- All seven anchor tests below pass.

---

## Anchor tests
```rust
#[test]
fn test_tui_memory_renders_empty_notes() { ... }

#[test]
fn test_tui_memory_add_appends_to_file() { ... }

#[test]
fn test_tui_memory_clear_requires_confirmation() { ... }

#[test]
fn test_tui_memory_clear_cancellable() { ... }

#[test]
fn test_tui_memory_does_not_call_start_turn() { ... }

#[test]
fn test_memory_injection_within_budget() { ... }

#[test]
fn test_memory_injection_over_budget_emits_warning() { ... }
```

**What NOT to do:**

- Do not read the notes file path from repo-local `.vex/config.toml`.
- Do not start a model pulse from any `/memory` subcommand.
- Do not skip the confirmation overlay for `/memory clear` in `TuiMode`.
- Do not implement auto-memory (model-initiated capture) — formally deferred per ADR-024 Gap 16.
- Do not begin this task until PA-01 (layered config) is passing.
