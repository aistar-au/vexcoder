# Task PM-04: Auto-Memory

**Target Files:** `src/state/conversation/state.rs`,
`src/state/conversation/core.rs`, `src/state/memory.rs`,
`src/state/memory/auto.rs` (new), `src/config.rs`, `src/config/load.rs`,
`src/commands.rs`

**Depends on:** None (green on current main)

---

## Issue

The agent forgets context between sessions. Users must re-explain project
conventions, naming patterns, preferred approaches, and past decisions at
the start of each conversation. The existing memory-notes system requires
explicit `/memory` commands — the agent never writes memory on its own.

---

## Decision

### Automatic memory extraction

At the end of each conversation turn (after the agent's response is
finalized), run a lightweight extraction pass that identifies
memory-worthy facts from the conversation. Categories of extractable facts:

1. **User corrections**: "No, use snake_case" / "We use PostgreSQL, not MySQL"
2. **Project conventions**: "Tests go in tests/integration/"
3. **Preferences**: "Always add error handling" / "Use anyhow for errors"
4. **Decisions**: "We decided to use reqwest for HTTP"

### Extraction method

The extraction prompt is appended as a follow-up LLM call with the
instruction: "Extract up to 3 short factual notes from this conversation
that would be useful in future sessions. Return JSON array of strings, or
empty array if nothing is worth remembering."

### Storage

Extracted notes are appended to the existing memory-notes file
(`~/.config/vex/memory.md` or the configured memory path). Each entry is
prefixed with a timestamp and tagged `[auto]` to distinguish from manual
entries.

### Configuration surface

```toml
# ~/.config/vex/config.toml

[auto_memory]
enabled          = true     # default: false
max_notes_per_turn = 3      # max notes extracted per turn
```

### `/memory auto` subcommand

- `/memory auto on` — enable auto-memory for the current session
- `/memory auto off` — disable auto-memory for the current session
- `/memory auto clear` — remove all `[auto]` entries from memory file

---

## Constraints

- Auto-memory is disabled by default. Users must opt in via config or
  `/memory auto on`.
- The extraction LLM call must use the same model and credentials as the
  main conversation. No separate API keys or endpoints.
- Extracted notes must be plain text, one line each. No structured data,
  no code blocks.
- The extraction prompt is hardcoded. Not user-configurable (prevents
  prompt injection via config).
- Auto-memory must not delay or block the agent's response. Extraction
  runs after the response is sent to the user.
- Must not write to memory if the extraction returns an empty array.
- Must not regress existing tests.

---

## Definition of Done

1. When `auto_memory.enabled = true`, notes are extracted after each turn.
2. Extracted notes are appended to the memory file with `[auto]` tag.
3. `/memory auto on|off` toggles extraction for the current session.
4. `/memory auto clear` removes `[auto]` entries from memory file.
5. Extraction failure does not affect the conversation.
6. No notes are written when the extraction returns an empty array.
7. `cargo test --all-targets` is green.

---

## Anchor Tests

`test_auto_memory_disabled_by_default`
`test_auto_memory_extracts_notes_when_enabled`
`test_auto_memory_tags_entries_with_auto`
`test_auto_memory_respects_max_notes_per_turn`
`test_auto_memory_empty_extraction_writes_nothing`
`test_auto_memory_clear_removes_tagged_entries`
`test_auto_memory_config_loads_from_user_layer`

Primary verification anchor:

```rust
#[test]
fn test_auto_memory_disabled_by_default() {
    // Given a Config with no [auto_memory] section,
    // auto_memory.enabled must be false.
}
```
