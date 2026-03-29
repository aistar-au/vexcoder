# Task PM-02: /undo and Per-Change Checkpoints

**Target Files:** `src/state/conversation/state.rs`,
`src/state/conversation/core.rs`, `src/state/conversation/tools.rs`,
`src/commands.rs`, `src/app/facade.rs`, `src/config.rs`

**Depends on:** None (green on current main)

---

## Issue

When the agent applies a patch or writes a file, the change is permanent.
There is no built-in mechanism to undo the last change or roll back to a
previous checkpoint. Users must manually revert via git or editor undo, which
breaks the conversational flow and requires context switching.

---

## Decision

### `/undo` slash command

Add a `/undo` command that reverts the most recent file-modifying tool call.
The command:

1. Pops the last entry from a per-session checkpoint stack.
2. Restores the affected file(s) to their pre-change content.
3. Emits a confirmation message to the conversation.

### Per-change checkpoint stack

Before each file-modifying tool call (`apply_patch`, `write_file`,
`create_file`), snapshot the affected file(s) into an in-memory checkpoint.
Each checkpoint records:

- Tool call ID
- File path(s)
- Original file content (full bytes)
- Timestamp

The stack has a configurable depth (default: 20). Oldest checkpoints are
dropped when the stack is full.

### Configuration surface

```toml
# ~/.config/vex/config.toml or .vex/config.toml

[undo]
enabled         = true    # default: true
max_checkpoints = 20      # max entries in the undo stack
```

### Execution contract

- `/undo` is a slash command, not a tool call. It does not appear in the
  tool dispatch table and does not trigger hooks.
- `/undo` with an empty stack emits a diagnostic message ("Nothing to undo").
- `/undo` restores file content by writing the snapshot bytes. It does not
  use git operations.
- Checkpoints are per-session and not persisted to disk.
- Binary files are checkpointed by full content (no diff).

---

## Constraints

- Do not use git for undo. The checkpoint system is independent of version
  control.
- Do not persist checkpoints to disk. They are in-memory only and cleared
  when the session ends.
- Do not checkpoint read-only tool calls (read_file, search, etc.).
- Checkpoint capture must happen before the tool call, not after. A failed
  tool call must not leave a checkpoint on the stack.
- `/undo` must not trigger pre/post tool hooks.
- Must not regress existing tests.

---

## Definition of Done

1. `/undo` reverts the most recent file-modifying tool call.
2. `/undo` with an empty stack emits "Nothing to undo".
3. Checkpoints are captured before `apply_patch`, `write_file`, and
   `create_file` tool calls.
4. Stack respects `max_checkpoints` limit.
5. Checkpoints are in-memory only; not persisted to disk.
6. `cargo test --all-targets` is green.

---

## Anchor Tests

`test_undo_reverts_last_write`
`test_undo_empty_stack_emits_diagnostic`
`test_checkpoint_captured_before_tool_call`
`test_checkpoint_stack_respects_max_depth`
`test_undo_does_not_trigger_hooks`
`test_failed_tool_call_does_not_leave_checkpoint`

Primary verification anchor:

```rust
#[test]
fn test_undo_empty_stack_emits_diagnostic() {
    // Given a ConversationManager with an empty checkpoint stack,
    // calling /undo must return a diagnostic message, not an error.
}
```
